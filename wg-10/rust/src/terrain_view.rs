//! Wg10TerrainView (DESIGN §6.2) — the drop-in terrain Node3D. Owns Gd handles to the page
//! pool, the stream-ahead scheduler, and the 3x3 clipmap rings, and ticks the live loop:
//! streamer.update -> per level per tile (3x3) fetch the resident page via the READ-ONLY
//! get_resident_page (NEVER computes on the render path), coarser fallback on a miss ->
//! rings.bind_tile. Owns no RIDs, no meshes, no scheduling math.

use crate::clipmap_rings::Wg10ClipmapRings;
use crate::page_pool::Wg10PagePool;
use crate::streamer::Wg10Streamer;
use godot::prelude::*;

#[derive(GodotClass)]
#[class(base=Node3D)]
pub struct Wg10TerrainView {
    pool: Option<Gd<Wg10PagePool>>,
    streamer: Option<Gd<Wg10Streamer>>,
    rings: Option<Gd<Wg10ClipmapRings>>,
    num_levels: i32,
    base_span: f64,
    relief_scale: f64,
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
            relief_scale: 1.0,
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
        relief_scale: f64,
        morph_region: f64,
        relief_ref: f64,
        _lead_seconds: f64, // kept for call-signature stability; the view now reads the clamped
                            // led centre from the streamer (coverage_center) instead of leading itself.
    ) {
        self.pool = Some(pool);
        self.streamer = Some(streamer);
        self.rings = Some(rings);
        self.num_levels = num_levels as i32;
        self.base_span = base_span;
        self.relief_scale = relief_scale;
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

        // B2 (structural never-black): clear last frame's display pins, then re-pin exactly the pages
        // we bind below. A pinned page can't be evicted/recycled while displayed, so the held coarse
        // blanket can never show page-A geometry with page-B pixels under capacity pressure.
        {
            let mut pool = self.pool.as_ref().unwrap().clone();
            pool.bind_mut().clear_display_pins();
        }

        // Display the ring around the camera. The streamer maintains this display ring plus a
        // velocity-led prefetch ring, so new pages can stream before the display boundary reaches
        // them without exposing the prefetch ring itself.
        let display_x = camera_x;
        let display_z = camera_z;

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
            let center_x = (display_x / span_l).floor() * span_l;
            let center_z = (display_z / span_l).floor() * span_l;
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
                    let tex = self.pool.as_ref().unwrap().bind().get_resident_page(
                        level as i64,
                        po_x,
                        po_z,
                    );
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
                            rings.bind_mut().set_tile_visible(
                                level as i64,
                                dx as i64,
                                dz as i64,
                                false,
                            );
                        } else {
                            // B2: HOLD LAST-GOOD on the coarsest level. The tile keeps showing the
                            // page it last bound (tracked by the rings). RE-VALIDATE that page is
                            // still resident as ITSELF and PIN it so eviction can't recycle the slot
                            // underneath it (page-A geometry + page-B pixels). If the held page is no
                            // longer resident-as-itself, there is nothing safe to show — fall through
                            // to hide rather than display a recycled RID.
                            let bk =
                                rings
                                    .bind()
                                    .bound_page_key(level as i64, dx as i64, dz as i64);
                            let held_ox = bk.x as f64;
                            let held_oz = bk.y as f64;
                            let still_there = self
                                .pool
                                .as_ref()
                                .unwrap()
                                .bind()
                                .get_resident_page(level as i64, held_ox, held_oz)
                                .is_some();
                            if still_there {
                                // pin the held page so the streamer can't evict/recycle it while shown.
                                let mut pool = self.pool.as_ref().unwrap().clone();
                                pool.bind_mut()
                                    .pin_displayed_page(level as i64, held_ox, held_oz);
                            } else {
                                // held page gone (recycled under capacity pressure) — don't show wrong
                                // pixels; hide. (Default-capacity flying never hits this; it's the
                                // structural guard the capacity-pressure gate exercises.)
                                rings.bind_mut().set_tile_visible(
                                    level as i64,
                                    dx as i64,
                                    dz as i64,
                                    false,
                                );
                            }
                        }
                        continue;
                    };

                    // B2: pin the page we are ACTUALLY binding this frame so it can't be evicted/
                    // recycled while on screen. Pins are cleared at the top of update() and re-set
                    // here, so a page that stops being displayed becomes evictable again next frame.
                    {
                        let mut pool = self.pool.as_ref().unwrap().clone();
                        pool.bind_mut().pin_displayed_page(level as i64, po_x, po_z);
                    }

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
                        let ptex = self.pool.as_ref().unwrap().bind().get_resident_page(
                            (level + 1) as i64,
                            p_ox,
                            p_oz,
                        );
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
                        Vector2::new(po_x as f32, po_z as f32), // tile_origin (placement)
                        Vector2::new(span_l as f32, coarse_span as f32), // spans (sample, coarse)
                        Vector2::new(span_l as f32, level_half_extent as f32), // placement (tile_span, half_extent)
                        self.relief_scale,
                        morph,
                        self.relief_ref,
                        Vector2::new(po_x as f32, po_z as f32), // sample_origin = own page corner
                        Vector2::new(cco_x as f32, cco_z as f32), // coarse (parent) origin
                        Vector2::new(level_center_x as f32, level_center_z as f32), // this level's neighborhood centre
                    );
                    let static_material_tex = self
                        .pool
                        .as_ref()
                        .unwrap()
                        .bind()
                        .get_resident_static_material_page(level as i64, po_x, po_z);
                    if let Some(material_tex) = static_material_tex {
                        rings.bind_mut().set_tile_static_material(
                            level as i64,
                            dx as i64,
                            dz as i64,
                            material_tex.upcast::<godot::classes::Texture2D>(),
                            0.58,
                        );
                    }
                    let debug_color =
                        debug_color_for_page(self.pool.as_ref().unwrap(), level as i64, po_x, po_z);
                    let material_mix = biome_material_mix_for_page(
                        self.pool.as_ref().unwrap(),
                        level as i64,
                        po_x,
                        po_z,
                    );
                    rings.bind_mut().set_tile_debug_color(
                        level as i64,
                        dx as i64,
                        dz as i64,
                        debug_color,
                        material_mix,
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

fn debug_color_for_page(
    pool: &Gd<Wg10PagePool>,
    level: i64,
    origin_x: f64,
    origin_z: f64,
) -> Color {
    let pool_ref = pool.bind();
    let mode = pool_ref.biome_runtime_mode().to_string();
    if mode == "world" {
        let biome = pool_ref
            .debug_world_biome_for_page(level, origin_x, origin_z)
            .to_string();
        biome_route_color(&biome)
    } else if mode == "static_reference" {
        if let Some((low_pass, floor, rock, snow)) =
            pool_ref.static_reference_material_hint_means_for_page(level, origin_x, origin_z, 17)
        {
            return static_reference_material_color(low_pass, floor, rock, snow);
        }
        let corridor_frac = pool_ref
            .static_reference_corridor_fraction_for_page(level, origin_x, origin_z, 17)
            .unwrap_or(0.0);
        if corridor_frac > 0.02 {
            Color::from_rgba(0.24, 0.48, 0.35, 1.0)
        } else {
            biome_route_color("mountain")
        }
    } else if mode == "single" {
        if let Some((low_pass, floor, rock, snow)) = pool_ref
            .mountain_world_layer_material_hint_means_for_page(level, origin_x, origin_z, 17)
        {
            return static_reference_material_color(low_pass, floor, rock, snow);
        }
        let corridor_frac = pool_ref
            .mountain_world_layer_corridor_fraction_for_page(level, origin_x, origin_z, 17)
            .unwrap_or(0.0);
        if corridor_frac > 0.02 {
            return Color::from_rgba(0.24, 0.48, 0.35, 1.0);
        }
        biome_route_color("mountain")
    } else {
        Color::from_rgba(0.34, 0.38, 0.43, 1.0)
    }
}

fn biome_material_mix_for_page(
    pool: &Gd<Wg10PagePool>,
    level: i64,
    origin_x: f64,
    origin_z: f64,
) -> f64 {
    let pool_ref = pool.bind();
    let mode = pool_ref.biome_runtime_mode().to_string();
    if mode == "world" {
        if pool_ref.has_world_preview_reference() {
            // WORLD preview uses accepted reference material pages in normal mode. Keep the route
            // colors available only through wg_dbg_mode=2 so the preview is not mistaken for raw
            // composed WORLD terrain quality.
            0.0
        } else {
            0.34
        }
    } else if mode == "static_reference" {
        // Static-reference mode now binds per-texel material/corridor pages. Keep the older
        // page-average debug tint out of normal material mode so it does not mute the accepted
        // payload's local floor/rock/snow/corridor story.
        let _ = (level, origin_x, origin_z);
        0.0
    } else {
        0.0
    }
}

fn static_reference_material_color(low_pass: f64, floor: f64, rock: f64, snow: f64) -> Color {
    let floorish = floor.max(low_pass);
    if snow >= rock && snow >= floorish && snow > 0.02 {
        Color::from_rgba(0.74, 0.78, 0.72, 1.0)
    } else if rock >= floorish && rock > 0.02 {
        Color::from_rgba(0.44, 0.45, 0.40, 1.0)
    } else if floorish > 0.02 {
        Color::from_rgba(0.24, 0.48, 0.35, 1.0)
    } else {
        biome_route_color("mountain")
    }
}

fn biome_route_color(name: &str) -> Color {
    match name {
        "coast" => Color::from_rgba(0.16, 0.46, 0.68, 1.0),
        "desert" => Color::from_rgba(0.78, 0.62, 0.25, 1.0),
        "glacial" => Color::from_rgba(0.78, 0.88, 0.92, 1.0),
        "grassland" => Color::from_rgba(0.32, 0.60, 0.24, 1.0),
        "karst" => Color::from_rgba(0.52, 0.52, 0.40, 1.0),
        "mountain" => Color::from_rgba(0.42, 0.43, 0.38, 1.0),
        "rainforest" => Color::from_rgba(0.10, 0.42, 0.24, 1.0),
        "temperate" => Color::from_rgba(0.24, 0.52, 0.32, 1.0),
        "tundra" => Color::from_rgba(0.55, 0.62, 0.58, 1.0),
        "volcanic" => Color::from_rgba(0.42, 0.22, 0.18, 1.0),
        "wetland" => Color::from_rgba(0.18, 0.38, 0.34, 1.0),
        _ => Color::from_rgba(0.45, 0.45, 0.45, 1.0),
    }
}
