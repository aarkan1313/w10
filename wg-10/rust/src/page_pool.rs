//! Single owner of all page-texture RIDs (DESIGN §5.2).
//!
//! `Wg10PagePool` is the ONLY place that calls `texture_create` and `free_rid`
//! on page textures.  It asks `PagePolicy` what to do (reuse / allocate /
//! evict), then executes by calling `compute_into_texture` from `page_compute`.
//!
//! Anti-WG9 rule enforced here: one place creates, one place frees.
//! `compute_into_texture` frees only ITS own transient buffers/pipeline/shader —
//! never the page texture RID.  All `free_rid` on page textures is pool-internal,
//! at exactly three sites: (a) `free_all` (teardown), (b) Allocate compute-failure
//! cleanup (free the just-created texture), (c) AllocateEvicting compute-failure
//! cleanup (drop the now-stale slot texture).  Single-owner discipline holds: only
//! the pool ever frees page textures.
//!
//! On any producer failure (texture_create or compute_into_texture) the pool calls
//! `PagePolicy::rollback(key)` so policy state matches reality — no phantom-resident
//! key (which could later panic an eviction `.expect`), no stale mapping returning
//! wrong/null content on re-acquire.

use crate::biome_page_compute;
use crate::gpu_compute::PackBuffers;
use crate::pack;
use crate::page_compute::PageComputeContext;
use crate::page_policy::PagePolicy;
use godot::classes::Texture2Drd;
use godot::prelude::*;

mod acquire;
mod config_api;
mod configure;
mod lifecycle;
mod producer;
mod state_api;
mod static_reference;
mod static_reports;
mod world_layer_bindings;
mod world_layer_contract;
mod world_layer_reference;
mod world_producer;
mod world_reports;
mod world_route;
mod world_runtime;

use static_reference::StaticHeightRuntime;
use world_layer_reference::BoundWorldLayerReference;
use world_runtime::BiomeWorldRuntime;

// ---------------------------------------------------------------------------
// Wg10PagePool
// ---------------------------------------------------------------------------

/// Single owner of all page-texture RIDs.
///
/// Call order:
///   1. `configure(...)` — load pack + GLSL, set policy capacity.
///   2. `acquire_page(level, origin_x, origin_z)` — get (or compute) a page.
///   3. `release_page(level, origin_x, origin_z)` — unprotect (LRU-eligible).
///   4. `free_all()` — teardown; frees ALL page texture RIDs.
#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10PagePool {
    policy: Option<PagePolicy>,
    slot_tex: Vec<Option<Rid>>,
    slot_wrap: Vec<Option<Gd<Texture2Drd>>>,
    slot_material_tex: Vec<Option<Rid>>,
    slot_material_wrap: Vec<Option<Gd<Texture2Drd>>>,
    pack: Option<pack::Pack>,
    pack_buffers: Option<PackBuffers>,
    glsl_source: Option<String>,
    compute_ctx: Option<PageComputeContext>,

    // Biome GPU producer path (Slice-4). Active producer identity is derived from exactly one of
    // the producer contexts below. The original `biome_ctx` is the proven single recipe path used
    // by parity/perf gates. `biome_world` is the grammar-routed runtime path: it owns grammar-only
    // pack data and a cached context per compiled recipe.
    biome_ctx: Option<biome_page_compute::BiomePageComputeContext>,
    biome_world: Option<BiomeWorldRuntime>,
    static_ref: Option<StaticHeightRuntime>,
    mountain_layer_ref: Option<BoundWorldLayerReference>,
    world_preview_ref: Option<BoundWorldLayerReference>,
    /// Ladder Rung 0 plumbing producer: writes a CLOSED-FORM height so a gate can predict every
    /// texel. De-risks the un-intercept flip (produce->stream->read) independent of biome content.
    /// `Some` => active producer is `Analytic` (checked first in `active_producer_kind`).
    analytic: Option<producer::AnalyticParams>,
    biome_feature_span_m: f64,
    biome_source_scale: f64,
    biome_source_offset_x_m: f64,
    biome_source_offset_z_m: f64,
    /// SCALE-INVARIANCE: the FIRST clipmap level (0 = finest) that bakes WITHOUT the drainage carve.
    /// A page at `level` runs `flow_on = level < biome_flow_max_level`. Default 2 => flow on levels
    /// 0,1 (near camera, where carved valleys read), off 2.. (coarse, where the macro surface
    /// suffices and the two flow passes are too costly). Set by `configure_biome`.
    biome_flow_max_level: i64,

    page_px: i64,
    world_span: f64,
    seed: i64,

    // stats
    created: i64,
    reused: i64,
    recomputed: i64,
    full_events: i64,

    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10PagePool {
    fn init(base: Base<RefCounted>) -> Self {
        Self {
            policy: None,
            slot_tex: Vec::new(),
            slot_wrap: Vec::new(),
            slot_material_tex: Vec::new(),
            slot_material_wrap: Vec::new(),
            pack: None,
            pack_buffers: None,
            glsl_source: None,
            compute_ctx: None,
            biome_ctx: None,
            biome_world: None,
            static_ref: None,
            mountain_layer_ref: None,
            world_preview_ref: None,
            analytic: None,
            biome_feature_span_m: 90000.0,
            biome_source_scale: 1.0,
            biome_source_offset_x_m: 0.0,
            biome_source_offset_z_m: 0.0,
            biome_flow_max_level: 2,
            page_px: 256,
            world_span: 1000.0,
            seed: 0,
            created: 0,
            reused: 0,
            recomputed: 0,
            full_events: 0,
            base,
        }
    }
}

#[godot_api]
impl Wg10PagePool {
    // -----------------------------------------------------------------------
    // free_all  (the ONLY place that frees page texture RIDs)
    // -----------------------------------------------------------------------

    /// Free all page texture RIDs on the global RenderingDevice and clear the
    /// slot vectors.  Safe to call during scene teardown; idempotent.
    ///
    /// As of the B1 fix this is ALSO called automatically from `Drop` (below), so
    /// leak-freedom is structural — a GDScript owner that forgets to call it no
    /// longer leaks. Calling it explicitly is still fine (the second call is a
    /// no-op: the slot vectors are already cleared and all configured Options are
    /// `None`).
    ///
    /// As of the F7 fix this fully resets the pool to the UNCONFIGURED state:
    /// after `free_all()` the `acquire_page`/`get_resident_page` guards correctly
    /// see "not configured" and return None instead of panicking on a stale-but-
    /// half-cleared state. To use the pool again, call `configure()`.
    ///
    /// This is the ONLY site (via `free_all_impl`) that calls `rd.free_rid` on
    /// page textures.
    #[func]
    pub fn free_all(&mut self) {
        self.free_all_impl();
    }
}

// ---------------------------------------------------------------------------
// Drop — structural leak-freedom (B1)
// ---------------------------------------------------------------------------

impl Drop for Wg10PagePool {
    /// Release all page-texture RIDs + the cached compute context when the pool
    /// is dropped, regardless of whether the GDScript owner called `free_all()`.
    ///
    /// A Godot `Rid` is a POD handle — dropping the Rust struct does NOT free the
    /// underlying GPU resource, so without this the RIDs orphan on the device
    /// (the B1 leak). `free_all_impl` guards for "no RenderingDevice" (headless /
    /// already-torn-down), so this is safe at any drop time, and idempotent with
    /// an explicit `free_all()` call.
    fn drop(&mut self) {
        self.free_all_impl();
    }
}

// ---------------------------------------------------------------------------
// Tests - headless state-machine coverage for lifecycle fixes.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod page_pool_tests;
