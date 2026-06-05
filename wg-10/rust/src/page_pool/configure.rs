//! Configuration state helpers for `Wg10PagePool`.

use crate::biome_page_compute;
use crate::gpu_compute::PackBuffers;
use crate::pack;
use crate::page_compute::PageComputeContext;
use crate::page_policy::PagePolicy;
use std::collections::BTreeMap;

use super::{BiomeWorldRuntime, StaticHeightRuntime, Wg10PagePool};

impl Wg10PagePool {
    /// Release any existing configured GPU state before applying a new configuration.
    pub(super) fn free_before_reconfigure(&mut self) {
        if self.is_configured() {
            self.free_all_impl();
        }
    }

    pub(super) fn init_policy_slots(&mut self, capacity: i64) {
        let cap = capacity as usize;
        self.policy = Some(PagePolicy::new(cap));
        self.slot_tex = vec![None; cap];
        self.slot_wrap = (0..cap).map(|_| None).collect();
        self.slot_material_tex = vec![None; cap];
        self.slot_material_wrap = (0..cap).map(|_| None).collect();
    }

    pub(super) fn reset_stats(&mut self) {
        self.created = 0;
        self.reused = 0;
        self.recomputed = 0;
        self.full_events = 0;
    }

    pub(super) fn reset_biome_source_transform(&mut self) {
        self.biome_source_scale = 1.0;
        self.biome_source_offset_x_m = 0.0;
        self.biome_source_offset_z_m = 0.0;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn install_legacy_configuration(
        &mut self,
        pack: pack::Pack,
        pack_buffers: PackBuffers,
        glsl_source: String,
        compute_ctx: PageComputeContext,
        capacity: i64,
        page_px: i64,
        world_span: f64,
        seed: i64,
    ) {
        self.init_policy_slots(capacity);
        self.pack = Some(pack);
        self.pack_buffers = Some(pack_buffers);
        self.glsl_source = Some(glsl_source);
        self.compute_ctx = Some(compute_ctx);
        self.biome_ctx = None;
        self.biome_world = None;
        self.static_ref = None;
        self.mountain_layer_ref = None;
        self.world_preview_ref = None;
        self.analytic = None;
        self.reset_biome_source_transform();
        self.page_px = page_px;
        self.world_span = world_span;
        self.seed = seed;
        self.reset_stats();
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn install_biome_configuration(
        &mut self,
        biome_ctx: biome_page_compute::BiomePageComputeContext,
        capacity: i64,
        page_px: i64,
        world_span: f64,
        feature_span_m: f64,
        flow_max_level: i64,
        seed: i64,
    ) {
        self.init_policy_slots(capacity);
        self.pack = None;
        self.pack_buffers = None;
        self.glsl_source = None;
        self.compute_ctx = None;
        self.biome_ctx = Some(biome_ctx);
        self.biome_world = None;
        self.static_ref = None;
        self.mountain_layer_ref = None;
        self.world_preview_ref = None;
        self.analytic = None;
        self.biome_feature_span_m = feature_span_m;
        self.reset_biome_source_transform();
        self.biome_flow_max_level = flow_max_level;
        self.page_px = page_px;
        self.world_span = world_span;
        self.seed = seed;
        self.reset_stats();
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn install_biome_world_configuration(
        &mut self,
        pack: pack::Pack,
        contexts: BTreeMap<String, biome_page_compute::BiomePageComputeContext>,
        compose_ctx: biome_page_compute::BiomePageComputeContext,
        capacity: i64,
        page_px: i64,
        world_span: f64,
        feature_span_m: f64,
        flow_max_level: i64,
        seed: i64,
    ) {
        self.init_policy_slots(capacity);
        self.pack = None;
        self.pack_buffers = None;
        self.glsl_source = None;
        self.compute_ctx = None;
        self.biome_ctx = None;
        self.biome_world = Some(BiomeWorldRuntime::new(pack, contexts, compose_ctx));
        self.static_ref = None;
        self.mountain_layer_ref = None;
        self.world_preview_ref = None;
        self.analytic = None;
        self.biome_feature_span_m = feature_span_m;
        self.reset_biome_source_transform();
        self.biome_flow_max_level = flow_max_level;
        self.page_px = page_px;
        self.world_span = world_span;
        self.seed = seed;
        self.reset_stats();
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn install_static_reference_configuration(
        &mut self,
        static_ref: StaticHeightRuntime,
        capacity: i64,
        page_px: i64,
        world_span: f64,
        seed: i64,
    ) {
        let feature_span_m = static_ref.feature_span_m;
        self.init_policy_slots(capacity);
        self.pack = None;
        self.pack_buffers = None;
        self.glsl_source = None;
        self.compute_ctx = None;
        self.biome_ctx = None;
        self.biome_world = None;
        self.static_ref = Some(static_ref);
        self.mountain_layer_ref = None;
        self.world_preview_ref = None;
        self.analytic = None;
        self.biome_feature_span_m = feature_span_m;
        self.reset_biome_source_transform();
        self.biome_flow_max_level = 0;
        self.page_px = page_px;
        self.world_span = world_span;
        self.seed = seed;
        self.reset_stats();
    }

    /// Install the RegionFact producer: clears every other producer Option (so `active_producer_kind`
    /// resolves to `RegionFact`), stores the pack (for `region_of`), the spawned bake worker, and the
    /// per-super-region bake config.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn install_region_fact_configuration(
        &mut self,
        pack: pack::Pack,
        worker: crate::region_bake::BakeWorker,
        cfg: super::RegionFactConfig,
        capacity: i64,
        page_px: i64,
        world_span: f64,
        seed: i64,
    ) {
        self.init_policy_slots(capacity);
        self.pack = Some(pack);
        self.pack_buffers = None;
        self.glsl_source = None;
        self.compute_ctx = None;
        self.biome_ctx = None;
        self.biome_world = None;
        self.static_ref = None;
        self.mountain_layer_ref = None;
        self.world_preview_ref = None;
        self.analytic = None;
        self.region_worker = Some(worker);
        self.region_cache.clear();
        self.region_baking.clear();
        self.biome_feature_span_m = cfg.feature_span_m;
        self.biome_flow_max_level = 0;
        self.region_cfg = Some(cfg);
        self.reset_biome_source_transform();
        self.page_px = page_px;
        self.world_span = world_span;
        self.seed = seed;
        self.reset_stats();
    }

    /// Ladder Rung 0: install the analytic closed-form plumbing producer. Clears every other
    /// producer Option so `active_producer_kind` resolves to `Analytic`.
    pub(super) fn install_analytic_configuration(
        &mut self,
        params: super::producer::AnalyticParams,
        capacity: i64,
        page_px: i64,
        world_span: f64,
    ) {
        self.init_policy_slots(capacity);
        self.pack = None;
        self.pack_buffers = None;
        self.glsl_source = None;
        self.compute_ctx = None;
        self.biome_ctx = None;
        self.biome_world = None;
        self.static_ref = None;
        self.mountain_layer_ref = None;
        self.world_preview_ref = None;
        self.analytic = Some(params);
        self.reset_biome_source_transform();
        self.biome_flow_max_level = 0;
        self.page_px = page_px;
        self.world_span = world_span;
        self.seed = 0;
        self.reset_stats();
    }
}
