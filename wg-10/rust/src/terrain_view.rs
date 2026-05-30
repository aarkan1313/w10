//! Wg10TerrainView (DESIGN §6.2) — the drop-in terrain Node3D. Owns Gd handles to the page
//! pool, the stream-ahead scheduler, and the 3x3 clipmap rings, and ticks the live loop:
//! streamer.update -> per level per tile (3x3) fetch the resident page via the READ-ONLY
//! get_resident_page (NEVER computes on the render path), coarser fallback on a miss ->
//! rings.bind_tile. Owns no RIDs, no meshes, no scheduling math.

use godot::prelude::*;
use crate::page_pool::Wg10PagePool;
use crate::streamer::Wg10Streamer;
use crate::clipmap_rings::Wg10ClipmapRings;

#[derive(GodotClass)]
#[class(base=Node3D)]
pub struct Wg10TerrainView {
    pool: Option<Gd<Wg10PagePool>>,
    streamer: Option<Gd<Wg10Streamer>>,
    rings: Option<Gd<Wg10ClipmapRings>>,
    num_levels: i32,
    base_span: f64,
    height_scale: f64,
    morph_region: f64,
    relief_ref: f64,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for Wg10TerrainView {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            pool: None,
            streamer: None,
            rings: None,
            num_levels: 0,
            base_span: 0.0,
            height_scale: 1.0,
            morph_region: 0.0,
            relief_ref: 2000.0,
            base,
        }
    }
}

#[godot_api]
impl Wg10TerrainView {
    #[func]
    pub fn configure(
        &mut self,
        pool: Gd<Wg10PagePool>,
        streamer: Gd<Wg10Streamer>,
        rings: Gd<Wg10ClipmapRings>,
        num_levels: i64,
        base_span: f64,
        height_scale: f64,
        morph_region: f64,
        relief_ref: f64,
        _lead_seconds: f64,  // kept for call-signature stability; the view now reads the clamped
                             // led centre from the streamer (coverage_center) instead of leading itself.
    ) {
        self.pool = Some(pool);
        self.streamer = Some(streamer);
        self.rings = Some(rings);
        self.num_levels = num_levels as i32;
        self.base_span = base_span;
        self.height_scale = height_scale;
        self.morph_region = morph_region;
        self.relief_ref = relief_ref;
    }

    #[func]
    pub fn update(&mut self, camera_x: f64, camera_z: f64, vel_x: f64, vel_z: f64) {
        if self.pool.is_none() || self.streamer.is_none() || self.rings.is_none() {
            godot_error!("Wg10TerrainView: update called before configure()");
            return;
        }
        {
            let mut streamer = self.streamer.as_ref().unwrap().clone();
            streamer.bind_mut().update(camera_x, camera_z, vel_x, vel_z);
        }

        // Centre the displayed rings on the SAME clamped velocity-led point the scheduler covers
        // — ask the streamer for it rather than recomputing, so the view can NEVER desync from
        // coverage and always inherits the lead clamp (camera stays inside its ring). Recomputing
        // it here with a raw lead was the bug that flew the ring off into empty ground.
        let streamer = self.streamer.as_ref().unwrap().clone();
        let led = streamer.bind().coverage_center(camera_x, camera_z, vel_x, vel_z);
        let led_x = led.x as f64;       // Vector2 packs world (x, z) as (.x, .y)
        let led_z = led.y as f64;

        // Render model (proven by the prove-one-at-a-time reset, owner-flown): EVERY level draws
        // its full 3x3; coarser levels are drawn UNDERNEATH (lower render_priority, set at
        // configure) as the never-black blanket. For each tile:
        //   - its own level page MISSING  -> HIDE the tile; the coarser full 3x3 shows through.
        //   - present -> SHOW + place in its own slot, sample its own page by world UV; and if it
        //     is not the coarsest level, geomorph toward its REAL PARENT page (level+1) over this
        //     level's 3x3 outer band. Coarsest level: no morph.
        // (This replaces the old per-tile coarse-fallback-in-own-frame model, whose wrong-UV
        // fallback + lead/centre desync caused the seams/flicker/"stuff disappears" the reset fixed.)
        let num = self.num_levels;
        for level in 0..num {
            let span_l = self.base_span * 2f64.powi(level);
            let center_x = (led_x / span_l).floor() * span_l;
            let center_z = (led_z / span_l).floor() * span_l;
            // this level's 3x3 neighborhood centre (= the middle tile's centre) + half-extent
            // (3 tiles wide -> half is 1.5*span); the geomorph rises to 1 at the outer ring.
            let level_center_x = center_x + span_l * 0.5;
            let level_center_z = center_z + span_l * 0.5;
            let level_half_extent = 1.5 * span_l;
            let is_coarsest = level == num - 1;
            let parent_span = span_l * 2.0;

            for dz in -1..=1 {
                for dx in -1..=1 {
                    let po_x = center_x + dx as f64 * span_l;
                    let po_z = center_z + dz as f64 * span_l;

                    let mut rings = self.rings.as_ref().unwrap().clone();

                    // own-level page (READ-ONLY; never computes on the render path).
                    let tex = self
                        .pool
                        .as_ref()
                        .unwrap()
                        .bind()
                        .get_resident_page(level as i64, po_x, po_z);
                    let Some(ht) = tex else {
                        // Target page not resident yet. Finer levels HIDE (the coarser full 3x3
                        // underneath covers the gap). The COARSEST level has nothing underneath, so
                        // hiding it would leave a HOLE — instead HOLD LAST-GOOD: leave the tile
                        // showing its current (still-resident, still-correct-for-its-world-spot)
                        // page until the new one streams in. The coarse blanket lags the camera by
                        // up to a page on a coarse-boundary cross but NEVER blinks to sky. (This was
                        // the bug: crossing a coarsest boundary repages all 9 coarse tiles at once,
                        // the 4/frame acquire budget can't fill them, and hiding them blanked the
                        // screen. Pillars: structural never-black, zero added compute, no magic
                        // numbers — vs. budget-spiking or lead-tuning fixes that only reduce it.)
                        if !is_coarsest {
                            rings.bind_mut().set_tile_visible(level as i64, dx as i64, dz as i64, false);
                        }
                        // coarsest: do nothing -> the tile keeps its last valid page + position.
                        continue;
                    };

                    // morph target: this level's REAL parent page (level+1) covering this tile's
                    // centre, in the parent's own UV frame. If the parent isn't resident (or this
                    // is the coarsest level), morph OFF and the coarse sampler just points at the
                    // own page (unused at morph=0).
                    let (coarse_tex, coarse_span, cco_x, cco_z, morph) = if is_coarsest {
                        (ht.clone(), span_l, po_x, po_z, 0.0)
                    } else {
                        let tc_x = po_x + span_l * 0.5;
                        let tc_z = po_z + span_l * 0.5;
                        let p_ox = (tc_x / parent_span).floor() * parent_span;
                        let p_oz = (tc_z / parent_span).floor() * parent_span;
                        let ptex = self
                            .pool
                            .as_ref()
                            .unwrap()
                            .bind()
                            .get_resident_page((level + 1) as i64, p_ox, p_oz);
                        match ptex {
                            Some(pt) => (pt, parent_span, p_ox, p_oz, self.morph_region),
                            None => (ht.clone(), span_l, po_x, po_z, 0.0),
                        }
                    };

                    rings.bind_mut().bind_tile(
                        level as i64,
                        dx as i64,
                        dz as i64,
                        ht.upcast::<godot::classes::Texture2D>(),
                        coarse_tex.upcast::<godot::classes::Texture2D>(),
                        Vector2::new(po_x as f32, po_z as f32),                       // tile_origin (placement)
                        Vector2::new(span_l as f32, coarse_span as f32),             // spans (sample, coarse)
                        Vector2::new(span_l as f32, level_half_extent as f32),       // placement (tile_span, half_extent)
                        self.height_scale,
                        morph,
                        self.relief_ref,
                        Vector2::new(po_x as f32, po_z as f32),                       // sample_origin = own page corner
                        Vector2::new(cco_x as f32, cco_z as f32),                    // coarse (parent) origin
                        Vector2::new(level_center_x as f32, level_center_z as f32),  // this level's neighborhood centre
                    );
                }
            }
        }
    }

    #[func]
    pub fn stats(&self) -> Dictionary<GString, Variant> {
        match self.pool.as_ref() {
            Some(pool) => pool.bind().stats(),
            None => Dictionary::<GString, Variant>::new(),
        }
    }
}
