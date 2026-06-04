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

use godot::prelude::*;
use godot::classes::{RenderingServer, Texture2Drd};
use crate::pack;
use crate::gpu_compute::{build_pack_buffers, PackBuffers};
use crate::page_policy::{PagePolicy, PageKey, Decision};
use crate::page_compute::{PageComputeContext, build_page_compute_context};
use crate::biome_page_compute;
use std::path::Path;

mod acquire;
mod configure;
mod lifecycle;

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
    policy:       Option<PagePolicy>,
    slot_tex:     Vec<Option<Rid>>,
    slot_wrap:    Vec<Option<Gd<Texture2Drd>>>,
    pack:         Option<pack::Pack>,
    pack_buffers: Option<PackBuffers>,
    glsl_source:  Option<String>,
    compute_ctx:  Option<PageComputeContext>,

    // Biome GPU producer path (Slice-4, live mountain fly). Flag-gated; legacy kernel path is the
    // DEFAULT (use_biome_path=false) for A/B + rollback. When true, acquire_page routes both
    // producer sites through `compute_biome_page_cached` against `biome_ctx` instead of
    // `compute_page_cached`. The biome path has NO pack/pack_buffers/glsl_source/compute_ctx.
    use_biome_path:      bool,
    biome_ctx:           Option<biome_page_compute::BiomePageComputeContext>,
    biome_feature_span_m: f64,
    /// SCALE-INVARIANCE: the FIRST clipmap level (0 = finest) that bakes WITHOUT the drainage carve.
    /// A page at `level` runs `flow_on = level < biome_flow_max_level`. Default 2 => flow on levels
    /// 0,1 (near camera, where carved valleys read), off 2.. (coarse, where the macro surface
    /// suffices and the two flow passes are too costly). Set by `configure_biome`.
    biome_flow_max_level: i64,

    page_px:      i64,
    world_span:   f64,
    seed:         i64,

    // stats
    created:      i64,
    reused:       i64,
    recomputed:   i64,
    full_events:  i64,

    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10PagePool {
    fn init(base: Base<RefCounted>) -> Self {
        Self {
            policy:       None,
            slot_tex:     Vec::new(),
            slot_wrap:    Vec::new(),
            pack:         None,
            pack_buffers: None,
            glsl_source:  None,
            compute_ctx:  None,
            use_biome_path:       false,
            biome_ctx:            None,
            biome_feature_span_m: 90000.0,
            biome_flow_max_level: 2,
            page_px:      256,
            world_span:   1000.0,
            seed:         0,
            created:      0,
            reused:       0,
            recomputed:   0,
            full_events:  0,
            base,
        }
    }
}

#[godot_api]
impl Wg10PagePool {
    // -----------------------------------------------------------------------
    // configure
    // -----------------------------------------------------------------------

    /// Load the terrain pack + GLSL source and initialise the policy/slot vectors.
    ///
    /// Returns `""` on success, or an error string on failure (leaves the pool
    /// in a not-ready state).
    ///
    /// `pack_dir`   — OS path to the terrain-pack directory
    /// `pack_file`  — filename within `pack_dir`, e.g. `"terrain_pack.json"`
    /// `glsl_path`  — OS path to `height_page.glsl`
    /// `capacity`   — maximum number of resident page textures
    /// `page_px`    — page resolution in pixels (width == height, multiple of 16)
    /// `world_span` — world-space size of one page in metres
    /// `seed`       — grammar seed
    #[func]
    pub fn configure(
        &mut self,
        pack_dir:   GString,
        pack_file:  GString,
        glsl_path:  GString,
        capacity:   i64,
        page_px:    i64,
        world_span: f64,
        seed:       i64,
    ) -> GString {
        // --- F8: free-before-reconfigure ---
        // A second configure() would otherwise overwrite slot_tex / slot_wrap /
        // compute_ctx with new GPU resources WITHOUT releasing the old ones, leaking
        // the previous textures' RIDs + compute context on the device. Tear down any
        // existing configuration first (fully resets state per the F7 fix above).
        // Idempotent: a no-op on a fresh, never-configured pool (empty vecs + None
        // Options), so this is safe on the first configure() too.
        self.free_before_reconfigure();

        // --- load pack ---
        let pack = match pack::load_pack_dir(
            Path::new(&pack_dir.to_string()),
            &pack_file.to_string(),
        ) {
            Ok(p)  => p,
            Err(e) => return GString::from(&format!("pack: {e}")),
        };

        // --- build pack buffers ---
        let pb = build_pack_buffers(&pack);

        // --- load GLSL ---
        let glsl = match std::fs::read_to_string(glsl_path.to_string()) {
            Ok(s)  => s,
            Err(e) => return GString::from(&format!("glsl: {e}")),
        };

        // --- build the cached compute context ONCE (slice 7) ---
        // Compile the shader + pipeline + upload the 6 pack buffers (incl. the ~25 MB kernel
        // atlas) here, reused for every page — so per-page production never recompiles/re-uploads
        // (the 90 ms boundary-crossing spike the M3 p99 gate caught). Needs the global RD; the
        // pool is only meaningfully configured windowed (like every pool user).
        let mut rd0 = match RenderingServer::singleton().get_rendering_device() {
            Some(r) => r,
            None    => return GString::from("configure: global RenderingDevice unavailable (windowed-only)"),
        };
        let ctx = match build_page_compute_context(&mut rd0, &pb, &glsl) {
            Ok(c)  => c,
            Err(e) => return GString::from(&format!("compute context: {e}")),
        };

        self.install_legacy_configuration(
            pack, pb, glsl, ctx, capacity, page_px, world_span, seed,
        );

        GString::new()
    }

    // -----------------------------------------------------------------------
    // configure_biome  (Slice-4 GPU biome producer path)
    // -----------------------------------------------------------------------

    /// Configure the pool to produce pages via the GPU biome path (mountain, Slice-4 live-fly)
    /// instead of the legacy kernel atlas. Sets `use_biome_path=true` and builds the biome compute
    /// context on the global rd. Legacy `configure` stays the default path (flag off) for A/B +
    /// rollback. Windowed-only (needs the global RenderingDevice), like `configure`.
    ///
    /// Returns `""` on success, or an error string on failure (leaving the pool not-ready).
    #[func]
    #[allow(clippy::too_many_arguments)]
    pub fn configure_biome(
        &mut self,
        primitives_glsl_path: GString,   // res://.../recipe_primitives.glsl
        machine_glsl_path:    GString,   // res://.../biome_page.glsl
        mountain_glsl_path:   GString,   // res://.../biome_mountain.glsl  (the fragment)
        capacity:   i64,
        page_px:    i64,                 // core px (256) — the apron is added internally
        apron_px:   i64,                 // 160 for mountain
        world_span: f64,                 // world metres per page
        feature_span_m: f64,             // 90000.0 for mountain
        flow_iters: i64,                 // production convergence count (192 per memory)
        relief_m:   f64,                 // VERTICAL SCALE (metres): normalized recipe height * this -> metres
                                         // before the page texture write (the render shader expects metres).
                                         // The tunable vertical-scale knob (~1000 for mountain).
        flow_max_level: i64,             // SCALE-INVARIANCE: first level (0=finest) baked WITHOUT the
                                         // drainage carve. A page at `level` runs flow_on = level <
                                         // flow_max_level. 2 => flow on levels 0,1; off 2.. (coarse).
        seed:       i64,
    ) -> GString {
        // --- F8: free-before-reconfigure (mirror `configure`) ---
        self.free_before_reconfigure();

        // --- read the 3 GLSL sources ---
        let prim = match std::fs::read_to_string(primitives_glsl_path.to_string()) {
            Ok(s)  => s,
            Err(e) => return GString::from(&format!("configure_biome: glsl: {e}")),
        };
        let machine = match std::fs::read_to_string(machine_glsl_path.to_string()) {
            Ok(s)  => s,
            Err(e) => return GString::from(&format!("configure_biome: glsl: {e}")),
        };
        let mountain = match std::fs::read_to_string(mountain_glsl_path.to_string()) {
            Ok(s)  => s,
            Err(e) => return GString::from(&format!("configure_biome: glsl: {e}")),
        };

        // --- global RenderingDevice (windowed-only) ---
        let mut rd0 = match RenderingServer::singleton().get_rendering_device() {
            Some(r) => r,
            None    => return GString::from("configure_biome: global RenderingDevice unavailable (windowed-only)"),
        };

        // --- build the cached biome compute context on the global rd ---
        let ctx = match biome_page_compute::build_biome_page_context(
            &mut rd0,
            &prim,
            &machine,
            &mountain,
            page_px as usize,
            apron_px as usize,
            flow_iters as usize,
            relief_m as f32,
        ) {
            Ok(c)  => c,
            Err(e) => return GString::from(&format!("configure_biome: context: {e}")),
        };

        self.install_biome_configuration(
            ctx,
            capacity,
            page_px,
            world_span,
            feature_span_m,
            flow_max_level,
            seed,
        );

        GString::new()
    }

    /// True when the pool is producing pages via the GPU biome path (Slice-4). A perf/parity gate
    /// asserts this to PROVE the biome producer is the one actually running (anti-fooling).
    #[func]
    pub fn uses_biome_path(&self) -> bool {
        self.use_biome_path
    }

    // -----------------------------------------------------------------------
    // acquire_page
    // -----------------------------------------------------------------------

    /// Acquire (or compute) the page texture for `(level, origin_x, origin_z)`.
    ///
    /// On a cache hit the existing `Texture2Drd` is returned immediately.
    /// On a miss a new R32F texture is created (or an existing slot is reused
    /// for eviction) and the compute shader is dispatched into it.
    ///
    /// Returns `None` when:
    ///   - `configure()` has not been called (or failed);
    ///   - the global `RenderingDevice` is not available (windowed-only mode);
    ///   - texture creation or shader dispatch fails;
    ///   - all slots are protected (`Decision::Full`).
    #[func]
    pub fn acquire_page(
        &mut self,
        level:    i64,
        origin_x: f64,
        origin_z: f64,
    ) -> Option<Gd<Texture2Drd>> {
        // --- guards ---
        // Accept EITHER configured path: legacy kernel (pack+pack_buffers+glsl_source+compute_ctx)
        // OR biome (policy + biome_ctx). `is_configured()` is the single source of truth so the
        // producer call sites' `.as_ref().unwrap()` can never unwrap a None ctx (the F7 lesson).
        if !self.is_configured() {
            godot_error!("Wg10PagePool: acquire_page called before configure()");
            return None;
        }

        // --- global RenderingDevice ---
        let mut rd = match RenderingServer::singleton().get_rendering_device() {
            Some(r) => r,
            None    => {
                godot_error!("Wg10PagePool: global RenderingDevice unavailable (windowed-only mode)");
                return None;
            }
        };

        let key = PageKey {
            level:    level as i32,
            origin_x: origin_x as i64,
            origin_z: origin_z as i64,
        };

        let decision = self.policy.as_mut().unwrap().acquire(key);

        // Extract scalar parameters before the match so later arms can take
        // references to pack/pack_buffers without fighting the borrow checker.
        // glsl is cloned once (cheap vs. GPU dispatch cost); all other reads
        // are scalars copied out of self before any mutable access to
        // slot_tex/slot_wrap.  No unsafe required.
        // Per-level page span: a level-L page covers world_span * 2^level (Fix #1, slice 5a).
        // A flat world_span was only correct at level 0; coarser levels must cover 2^L more
        // ground so the page matches the clipmap band (scheduler/RingLayout level_span).
        let span_l = self.world_span * 2f64.powi(level as i32);
        let (ox, oz, ws, ppx, sd) =
            (origin_x, origin_z, span_l, self.page_px, self.seed);
        // SCALE-INVARIANCE: this level's drainage-carve flag. `compute_biome_page_cached` derives its
        // own spacing from (ws=span_l, ppx) -> each level bakes its blurs world-anchored to its span;
        // flow_on gates the carve off on coarse levels (cheaper, macro surface). (Inert on the legacy
        // kernel path, which ignores flow_on.)
        let flow_on = level < self.biome_flow_max_level;

        match decision {
            // ----------------------------------------------------------------
            Decision::Reuse(slot) => {
                self.reused += 1;
                self.slot_wrap[slot].clone()
            }

            // ----------------------------------------------------------------
            Decision::Allocate(slot) => {
                // Create a new R32F texture — the ONLY texture_create call in
                // the whole crate for page textures.
                let tex_rid = self.create_page_texture(&mut rd);
                let tex_rid = match tex_rid {
                    Some(r) => r,
                    None    => {
                        // texture_create failed; no texture exists to free.
                        // Roll back the policy so it has no phantom-protected
                        // slot (which would later panic an eviction .expect).
                        self.policy.as_mut().unwrap().rollback(key);
                        return None; // error already reported inside helper
                    }
                };

                // Dispatch into the new texture using the CACHED compute context (slice 7) —
                // no shader recompile, no buffer re-upload; just a uniform set + push + dispatch.
                // Branch on the producer path: biome (Slice-4) vs legacy kernel atlas (default).
                let result = self.dispatch_page_compute(
                    &mut rd, tex_rid, ox, oz, ws, ppx, sd, flow_on,
                );

                if let Err(e) = result {
                    godot_error!("Wg10PagePool: compute_page_cached failed (slot {slot}): {e}");
                    self.rollback_failed_allocate(&mut rd, key, tex_rid);
                    return None;
                }

                // Wrap and store — mutable slot access after immutable refs end.
                let mut wrap = Texture2Drd::new_gd();
                wrap.set_texture_rd_rid(tex_rid);

                self.slot_tex[slot]  = Some(tex_rid);
                self.slot_wrap[slot] = Some(wrap.clone());
                self.created += 1;
                Some(wrap)
            }

            // ----------------------------------------------------------------
            Decision::AllocateEvicting { slot, evicted: _ } => {
                // Reuse the EXISTING texture RID — zero-churn eviction.
                // No free_rid, no texture_create; recompute into the slot RID.
                let tex_rid = self.slot_tex[slot]
                    .expect("AllocateEvicting: slot must be occupied");

                // Branch on the producer path: biome (Slice-4) vs legacy kernel atlas (default).
                let result = self.dispatch_page_compute(
                    &mut rd, tex_rid, ox, oz, ws, ppx, sd, flow_on,
                );

                if let Err(e) = result {
                    godot_error!(
                        "Wg10PagePool: compute_page_cached failed on eviction (slot {slot}): {e}"
                    );
                    self.rollback_failed_eviction(&mut rd, key, slot);
                    return None;
                }

                // The Texture2Drd wrapper still points at the same RID; content
                // was recomputed in place.  Clone increments the Godot refcount.
                self.recomputed += 1;
                self.slot_wrap[slot].clone()
            }

            // ----------------------------------------------------------------
            Decision::Full => {
                self.full_events += 1;
                godot_warn!(
                    "Wg10PagePool: all slots protected, returning null (Full)"
                );
                None
            }
        }
    }

    // -----------------------------------------------------------------------
    // release_page
    // -----------------------------------------------------------------------

    /// Unprotect a page (marks it LRU-eligible for eviction).  Idempotent.
    #[func]
    pub fn release_page(&mut self, level: i64, origin_x: f64, origin_z: f64) {
        if self.policy.is_none() {
            godot_error!("Wg10PagePool: release_page called before configure()");
            return;
        }
        let key = PageKey {
            level:    level as i32,
            origin_x: origin_x as i64,
            origin_z: origin_z as i64,
        };
        self.policy.as_mut().unwrap().release(key);
    }

    // -----------------------------------------------------------------------
    // stats
    // -----------------------------------------------------------------------

    /// Return a Dictionary with pool statistics:
    ///   "created"      — textures allocated from scratch
    ///   "reused"       — cache hits
    ///   "recomputed"   — eviction-reuse (recomputed into existing texture)
    ///   "full_events"  — times all slots were protected (Full)
    ///   "resident"     — pages currently resident in the policy
    #[func]
    pub fn stats(&self) -> Dictionary<GString, Variant> {
        let resident = self.policy
            .as_ref()
            .map(|p| p.resident_count() as i64)
            .unwrap_or(0);

        let mut d = Dictionary::<GString, Variant>::new();
        d.set("created",     self.created);
        d.set("reused",      self.reused);
        d.set("recomputed",  self.recomputed);
        d.set("full_events", self.full_events);
        d.set("resident",    resident);
        d
    }

    // -----------------------------------------------------------------------
    // resident_keys
    // -----------------------------------------------------------------------

    /// Resident page keys as a flat array of (level, origin_x, origin_z) triples.
    /// Read-only — the pool stays the sole RID owner; this only reports residency.
    #[func]
    pub fn resident_keys(&self) -> PackedInt64Array {
        let mut out = PackedInt64Array::new();
        if let Some(policy) = self.policy.as_ref() {
            for key in policy.resident_keys() {
                out.push(key.level as i64);
                out.push(key.origin_x);
                out.push(key.origin_z);
            }
        }
        out
    }

    // -----------------------------------------------------------------------
    // get_resident_page  (read-only — NEVER computes; the anti-WG9 render-path rule)
    // -----------------------------------------------------------------------

    /// Return the page texture for `(level, origin_x, origin_z)` IFF it is already resident,
    /// else `None`. **Read-only: it NEVER allocates, evicts, or dispatches a compute** — it
    /// is a pure slot lookup. A CONSUMER (e.g. `Wg10TerrainView`) uses this to fetch a page
    /// to display without triggering synchronous page production on the render path (the
    /// WG9 disease). Only the scheduler's `acquire_page` may compute; the view queries.
    /// On a miss the caller falls back to a coarser resident page (never black).
    #[func]
    pub fn get_resident_page(
        &self,
        level:    i64,
        origin_x: f64,
        origin_z: f64,
    ) -> Option<Gd<Texture2Drd>> {
        let policy = self.policy.as_ref()?;
        let key = PageKey {
            level:    level as i32,
            origin_x: origin_x as i64,
            origin_z: origin_z as i64,
        };
        let slot = policy.slot_of(&key)?;
        self.slot_wrap[slot].clone()
    }

    // -----------------------------------------------------------------------
    // Display pinning (B2 — structural never-black; the view drives this)
    // -----------------------------------------------------------------------

    /// Clear all display pins, then the view re-pins exactly what it binds this frame.
    /// Call at the START of the view's per-frame bind pass.
    #[func]
    pub fn clear_display_pins(&mut self) {
        if let Some(p) = self.policy.as_mut() {
            p.clear_display_pins();
        }
    }

    /// Pin the page `(level, origin_x, origin_z)` as currently-displayed so it can NEVER be evicted/
    /// recycled while on screen (B2). The view calls this for every page it binds — especially the
    /// held coarse blanket — so a streamer `release` can't recycle the slot under a visible tile
    /// (the page-A-geometry-with-page-B-pixels corruption). No-op if the page isn't resident.
    #[func]
    pub fn pin_displayed_page(&mut self, level: i64, origin_x: f64, origin_z: f64) {
        if let Some(p) = self.policy.as_mut() {
            p.pin_displayed(PageKey {
                level: level as i32,
                origin_x: origin_x as i64,
                origin_z: origin_z as i64,
            });
        }
    }

    /// True if `(level, origin_x, origin_z)` is resident AND display-pinned. Gate/test introspection.
    #[func]
    pub fn is_displayed_pinned(&self, level: i64, origin_x: f64, origin_z: f64) -> bool {
        self.policy.as_ref().map_or(false, |p| p.is_pinned(&PageKey {
            level: level as i32,
            origin_x: origin_x as i64,
            origin_z: origin_z as i64,
        }))
    }

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
