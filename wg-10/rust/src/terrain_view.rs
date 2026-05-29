//! Wg10TerrainView (DESIGN §5.5) — the drop-in live-loop coordinator. A thin Node3D that
//! owns Gd handles to the pool, the streamer, and the ring meshes, and on each `update`
//! ticks the scheduler then per level: acquires the resident page (coarser fallback when
//! the pool is Full), acquires its coarser neighbor (for the morph), binds both into the
//! rings, and finally recenters the rings on the camera.
//!
//! Owns NO RIDs, NO meshes, and contains NO scheduling math — every responsibility is
//! delegated to the three wired classes. The only arithmetic here is the page-corner
//! convention, which MUST match `Wg10ClipmapRings::recenter`'s mesh-center quantization:
//! a level-L band is centered at `floor(cam / cell_L) * cell_L` (per axis), so the page's
//! lower-XZ world corner is `center_L - span_L/2`.

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
    grid_res: i32,
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
            grid_res: 0,
            height_scale: 0.0,
            morph_region: 0.0,
            relief_ref: 0.0,
            base,
        }
    }
}

#[godot_api]
impl Wg10TerrainView {
    /// Wire the view with the three already-configured classes and the shared tunables.
    /// `grid_res` must match the rings' grid_res so the page corner matches the rings'
    /// mesh-center quantization (see the CORNER convention in the module docs).
    #[func]
    #[allow(clippy::too_many_arguments)]
    pub fn configure(
        &mut self,
        pool: Gd<Wg10PagePool>,
        streamer: Gd<Wg10Streamer>,
        rings: Gd<Wg10ClipmapRings>,
        num_levels: i64,
        base_span: f64,
        grid_res: i64,
        height_scale: f64,
        morph_region: f64,
        relief_ref: f64,
    ) {
        self.pool = Some(pool);
        self.streamer = Some(streamer);
        self.rings = Some(rings);
        self.num_levels = num_levels as i32;
        self.base_span = base_span;
        self.grid_res = grid_res as i32;
        self.height_scale = height_scale;
        self.morph_region = morph_region;
        self.relief_ref = relief_ref;
    }

    /// One frame of the live loop (DESIGN §5.5): tick the streamer, then per level acquire
    /// the resident page (+ coarser neighbor for the morph) and bind it into the rings,
    /// finally recenter the rings on the camera.
    #[func]
    pub fn update(&mut self, camera_x: f64, camera_z: f64, vel_x: f64, vel_z: f64) {
        if self.pool.is_none() || self.streamer.is_none() || self.rings.is_none() {
            godot_error!("Wg10TerrainView: update called before configure()");
            return;
        }

        // Advance the scheduler. Clone the Gd handle before bind_mut so we don't hold a
        // borrow of self.streamer while mutably borrowing the same object.
        {
            let mut streamer = self.streamer.as_ref().unwrap().clone();
            streamer.bind_mut().update(camera_x, camera_z, vel_x, vel_z);
        }

        for level in 0..self.num_levels {
            let span_l = self.base_span * 2f64.powi(level);
            let (ox_l, oz_l) = corner(camera_x, camera_z, span_l, self.grid_res);

            // Coarser neighbor for the morph. The coarsest level is its own coarse
            // neighbor (morph disabled).
            let (coarse_level, span_c) = if level < self.num_levels - 1 {
                (level + 1, self.base_span * 2f64.powi(level + 1))
            } else {
                (level, span_l)
            };
            let (ox_c, oz_c) = corner(camera_x, camera_z, span_c, self.grid_res);

            // Acquire this level's page and its coarser neighbor (both expected cache
            // hits — the streamer already produced residency this frame).
            let (tex_l, coarse_tex) = {
                let mut pool = self.pool.as_ref().unwrap().clone();
                let tex_l = pool.bind_mut().acquire_page(level as i64, ox_l as f64, oz_l as f64);
                let coarse_tex =
                    pool.bind_mut().acquire_page(coarse_level as i64, ox_c as f64, oz_c as f64);
                (tex_l, coarse_tex)
            };

            // Never-black fallback: if this level isn't resident, degenerate to the flat
            // coarse page with the morph disabled. If BOTH are missing, skip — the gate's
            // never-black assertion catches a true coverage gap.
            let (height_tex, morph_l): (Option<Gd<godot::classes::Texture2Drd>>, f64) =
                if tex_l.is_some() {
                    let m = if level < self.num_levels - 1 { self.morph_region } else { 0.0 };
                    (tex_l, m)
                } else {
                    // tex_l is None: use the coarse page as height with morph 0.
                    (coarse_tex.clone(), 0.0)
                };

            if let (Some(height_tex), Some(coarse_tex)) = (height_tex, coarse_tex) {
                let mut rings = self.rings.as_ref().unwrap().clone();
                rings.bind_mut().bind_page(
                    level as i64,
                    height_tex.upcast::<godot::classes::Texture2D>(),
                    coarse_tex.upcast::<godot::classes::Texture2D>(),
                    span_l,
                    span_c,
                    self.height_scale,
                    morph_l,
                    self.relief_ref,
                    ox_c as f64,
                    oz_c as f64,
                );
            }
        }

        // Recenter the rings on the camera (quantized translate, never a rebuild).
        {
            let mut rings = self.rings.as_ref().unwrap().clone();
            rings.bind_mut().recenter(camera_x, camera_z);
        }
    }

    /// Pass-through of the pool's residency/budget stats (the gate's + overlay's view in).
    #[func]
    pub fn stats(&self) -> Dictionary<GString, Variant> {
        match self.pool.as_ref() {
            Some(pool) => pool.bind().stats(),
            None => Dictionary::<GString, Variant>::new(),
        }
    }
}

/// Lower-XZ world corner of the level's camera-centered band. MUST match
/// `Wg10ClipmapRings::recenter`'s mesh-center quantization: the band is centered at
/// `floor(cam / cell) * cell` (per axis) and spans `[center - span/2, center + span/2]`,
/// so the page key (lower corner) is `center - span/2`.
fn corner(camera_x: f64, camera_z: f64, span: f64, grid_res: i32) -> (i64, i64) {
    let cell = span / grid_res as f64;
    let center_x = (camera_x / cell).floor() * cell;
    let center_z = (camera_z / cell).floor() * cell;
    (((center_x - span * 0.5)) as i64, ((center_z - span * 0.5)) as i64)
}
