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

        let num = self.num_levels;
        for level in 0..num {
            let span_l = self.base_span * 2f64.powi(level);
            let center_x = (led_x / span_l).floor() * span_l;
            let center_z = (led_z / span_l).floor() * span_l;

            // slice 8: the 3x3 neighborhood's world center is the MIDDLE tile's center
            // (page-origin `center` + half a page); the geomorph normalizes to half the
            // neighborhood width (3 tiles of span_l -> half-extent 1.5*span_l) so it engages
            // only at the level's true outer ring, not at every interior tile edge.
            let level_center_x = center_x + span_l * 0.5;
            let level_center_z = center_z + span_l * 0.5;
            let level_half_extent = 1.5 * span_l;

            let span_c = if level < num - 1 {
                self.base_span * 2f64.powi(level + 1)
            } else {
                span_l
            };
            let coarse_level = if level < num - 1 { level + 1 } else { level };

            for dz in -1..=1 {
                for dx in -1..=1 {
                    let po_x = center_x + dx as f64 * span_l;
                    let po_z = center_z + dz as f64 * span_l;

                    let tc_x = po_x + span_l * 0.5;
                    let tc_z = po_z + span_l * 0.5;
                    let co_x = (tc_x / span_c).floor() * span_c;
                    let co_z = (tc_z / span_c).floor() * span_c;

                    let (tex, coarse_tex) = {
                        let pool = self.pool.as_ref().unwrap().bind();
                        let tex = pool.get_resident_page(level as i64, po_x, po_z);
                        let coarse_tex = pool.get_resident_page(coarse_level as i64, co_x, co_z);
                        (tex, coarse_tex)
                    };

                    // Tile PLACEMENT is invariant: it always sits in its fine slot, covering
                    // world [po, po+span_l]. Only what it SAMPLES varies (self-consistent
                    // texture+origin+span). The slice-8 flicker bug was a fallback tile sampling
                    // the coarse texture with the FINE origin+span -> wrong UV. Three cases:
                    //   - fine resident: sample the fine page (po, span_l); morph to coarse if
                    //     the coarse parent is resident, else morph OFF.
                    //   - fine missing, coarse resident: sample the COARSE page in ITS OWN frame
                    //     (co, span_c) over this tile's footprint; morph OFF. Correct lower-detail.
                    //   - neither resident: skip (the coarser level's own 3x3 covers this area).
                    // Tuple: (height_tex, coarse_tex, sample_span, coarse_span, sample_ox, sample_oz, coarse_ox, coarse_oz, morph)
                    let sample = if let Some(ht) = tex {
                        match coarse_tex.clone() {
                            Some(ct) => {
                                let m = if level < num - 1 { self.morph_region } else { 0.0 };
                                Some((ht, ct, span_l, span_c, po_x, po_z, co_x, co_z, m))
                            }
                            None => Some((ht.clone(), ht, span_l, span_l, po_x, po_z, po_x, po_z, 0.0)),
                        }
                    } else if let Some(ct) = coarse_tex.clone() {
                        Some((ct.clone(), ct, span_c, span_c, co_x, co_z, co_x, co_z, 0.0))
                    } else {
                        None
                    };

                    if let Some((ht, ct, sample_span, coarse_span_b, so_x, so_z, cco_x, cco_z, morph)) = sample {
                        let mut rings = self.rings.as_ref().unwrap().clone();
                        rings.bind_mut().bind_tile(
                            level as i64,
                            dx as i64,
                            dz as i64,
                            ht.upcast::<godot::classes::Texture2D>(),
                            ct.upcast::<godot::classes::Texture2D>(),
                            Vector2::new(po_x as f32, po_z as f32), // tile_origin (invariant placement)
                            Vector2::new(sample_span as f32, coarse_span_b as f32), // spans
                            Vector2::new(span_l as f32, level_half_extent as f32),  // placement: (tile_span, level_half_extent)
                            self.height_scale,
                            morph,
                            self.relief_ref,
                            Vector2::new(so_x as f32, so_z as f32), // sample_origin
                            Vector2::new(cco_x as f32, cco_z as f32),
                            Vector2::new(level_center_x as f32, level_center_z as f32),
                        );
                    }
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
