//! Page acquisition compute and rollback helpers for `Wg10PagePool`.

use godot::classes::RenderingDevice;
use godot::prelude::*;

use crate::page_compute::compute_page_cached;
use crate::page_policy::PageKey;

use super::Wg10PagePool;

impl Wg10PagePool {
    /// Dispatch the active producer path into an already-owned page texture RID.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn dispatch_page_compute(
        &self,
        rd: &mut Gd<RenderingDevice>,
        tex_rid: Rid,
        origin_x: f64,
        origin_z: f64,
        world_span: f64,
        page_px: i64,
        seed: i64,
        flow_on: bool,
    ) -> Result<(), String> {
        if self.use_biome_path {
            let bctx = self.biome_ctx.as_ref().unwrap();
            crate::biome_page_compute::compute_biome_page_cached(
                rd,
                bctx,
                tex_rid,
                origin_x,
                origin_z,
                world_span,
                page_px,
                self.biome_feature_span_m,
                seed,
                flow_on,
            )
        } else {
            let ctx = self.compute_ctx.as_ref().unwrap();
            let num_palettes = self.pack_buffers.as_ref().unwrap().num_palettes;
            compute_page_cached(
                rd,
                ctx,
                &self.pack.as_ref().unwrap().grammar_constants,
                num_palettes,
                tex_rid,
                origin_x,
                origin_z,
                world_span,
                page_px,
                seed,
            )
        }
    }

    /// Roll back a failed compute into a newly-created texture.
    pub(super) fn rollback_failed_allocate(
        &mut self,
        rd: &mut Gd<RenderingDevice>,
        key: PageKey,
        tex_rid: Rid,
    ) {
        rd.free_rid(tex_rid);
        self.policy.as_mut().unwrap().rollback(key);
    }

    /// Roll back a failed recompute into an evicted slot.
    pub(super) fn rollback_failed_eviction(
        &mut self,
        rd: &mut Gd<RenderingDevice>,
        key: PageKey,
        slot: usize,
    ) {
        if let Some(old_rid) = self.slot_tex[slot].take() {
            rd.free_rid(old_rid);
        }
        self.slot_wrap[slot] = None;
        self.policy.as_mut().unwrap().rollback(key);
    }
}
