//! Lifecycle, reset, and page texture helpers for `Wg10PagePool`.

use godot::classes::{
    rendering_device::{DataFormat, TextureUsageBits},
    RdTextureFormat, RdTextureView, RenderingDevice, RenderingServer, Texture2Drd,
};
use godot::prelude::*;

use crate::biome_page_compute;
use crate::gpu_compute::PackBuffers;
use crate::pack;
use crate::page_compute::{free_page_compute_context, PageComputeContext};
use crate::page_policy::PagePolicy;

use super::{BiomeWorldRuntime, Wg10PagePool};

impl Wg10PagePool {
    /// The actual teardown logic, shared by `free_all` and `Drop`.
    ///
    /// This is the only site that calls `rd.free_rid` on page textures.
    pub(super) fn free_all_impl(&mut self) {
        let rd_opt = RenderingServer::singleton().get_rendering_device();
        if rd_opt.is_none() {
            // No RenderingDevice: nothing to free on the GPU. Fully reset so
            // `acquire_page` sees the pool as unconfigured instead of finding a
            // half-cleared compute context.
            Self::reset_configured_state(
                &mut self.policy,
                &mut self.slot_tex,
                &mut self.slot_wrap,
                &mut self.slot_material_tex,
                &mut self.slot_material_wrap,
                &mut self.pack,
                &mut self.pack_buffers,
                &mut self.glsl_source,
                &mut self.compute_ctx,
                &mut self.use_biome_path,
                &mut self.biome_ctx,
                &mut self.biome_world,
                &mut self.static_ref,
            );
            return;
        }

        let mut rd = rd_opt.unwrap();
        if let Some(ctx) = self.compute_ctx.take() {
            free_page_compute_context(&mut rd, &ctx);
        }
        if let Some(bctx) = self.biome_ctx.take() {
            biome_page_compute::free_biome_page_context(&mut rd, &bctx);
        }
        if let Some(world) = self.biome_world.take() {
            for (_, bctx) in world.contexts {
                biome_page_compute::free_biome_page_context(&mut rd, &bctx);
            }
            biome_page_compute::free_biome_page_context(&mut rd, &world.compose_ctx);
        }
        self.static_ref = None;
        for rid_opt in self.slot_tex.iter_mut() {
            if let Some(rid) = rid_opt.take() {
                rd.free_rid(rid);
            }
        }
        for rid_opt in self.slot_material_tex.iter_mut() {
            if let Some(rid) = rid_opt.take() {
                rd.free_rid(rid);
            }
        }

        Self::reset_configured_state(
            &mut self.policy,
            &mut self.slot_tex,
            &mut self.slot_wrap,
            &mut self.slot_material_tex,
            &mut self.slot_material_wrap,
            &mut self.pack,
            &mut self.pack_buffers,
            &mut self.glsl_source,
            &mut self.compute_ctx,
            &mut self.use_biome_path,
            &mut self.biome_ctx,
            &mut self.biome_world,
            &mut self.static_ref,
        );
    }

    /// Pure, engine-free reset of all configured state to the unconfigured shape.
    ///
    /// Callers that own GPU resources must free them before calling this; here
    /// we only clear data handles and policy state.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn reset_configured_state(
        policy: &mut Option<PagePolicy>,
        slot_tex: &mut Vec<Option<Rid>>,
        slot_wrap: &mut Vec<Option<Gd<Texture2Drd>>>,
        slot_material_tex: &mut Vec<Option<Rid>>,
        slot_material_wrap: &mut Vec<Option<Gd<Texture2Drd>>>,
        pack: &mut Option<pack::Pack>,
        pack_buffers: &mut Option<PackBuffers>,
        glsl_source: &mut Option<String>,
        compute_ctx: &mut Option<PageComputeContext>,
        use_biome_path: &mut bool,
        biome_ctx: &mut Option<biome_page_compute::BiomePageComputeContext>,
        biome_world: &mut Option<BiomeWorldRuntime>,
        static_ref: &mut Option<super::StaticHeightRuntime>,
    ) {
        *policy = None;
        slot_tex.clear();
        slot_wrap.iter_mut().for_each(|w| *w = None);
        slot_wrap.clear();
        slot_material_tex.clear();
        slot_material_wrap.iter_mut().for_each(|w| *w = None);
        slot_material_wrap.clear();
        *pack = None;
        *pack_buffers = None;
        *glsl_source = None;
        *compute_ctx = None;
        *use_biome_path = false;
        *biome_ctx = None;
        *biome_world = None;
        *static_ref = None;
    }

    /// Exact predicate mirrored by the `acquire_page` guard.
    #[allow(dead_code)]
    pub(super) fn is_configured(&self) -> bool {
        self.policy.is_some()
            && ((self.pack.is_some()
                && self.pack_buffers.is_some()
                && self.glsl_source.is_some()
                && self.compute_ctx.is_some())
                || self.biome_ctx.is_some()
                || self.biome_world.is_some()
                || self.static_ref.is_some())
    }

    /// Create a new R32F STORAGE+SAMPLING texture of `page_px x page_px`.
    pub(super) fn create_page_texture(&self, rd: &mut Gd<RenderingDevice>) -> Option<Rid> {
        self.create_r32_texture(rd, "height")
    }

    /// Create a new R32F SAMPLING texture for static-reference material codes.
    pub(super) fn create_static_material_texture(&self, rd: &mut Gd<RenderingDevice>) -> Option<Rid> {
        self.create_r32_texture(rd, "static material")
    }

    fn create_r32_texture(&self, rd: &mut Gd<RenderingDevice>, label: &str) -> Option<Rid> {
        let px = self.page_px as u32;
        let mut fmt = RdTextureFormat::new_gd();
        fmt.set_width(px);
        fmt.set_height(px);
        fmt.set_format(DataFormat::R32_SFLOAT);
        fmt.set_usage_bits(
            TextureUsageBits::STORAGE_BIT
                | TextureUsageBits::SAMPLING_BIT
                | TextureUsageBits::CAN_COPY_FROM_BIT
                | TextureUsageBits::CAN_UPDATE_BIT,
        );

        let view = RdTextureView::new_gd();
        let tex_rid = rd.texture_create(&fmt, &view);
        if tex_rid.is_invalid() {
            godot_error!(
                "Wg10PagePool: {label} texture_create returned invalid RID (page_px={})",
                self.page_px
            );
            return None;
        }
        Some(tex_rid)
    }
}
