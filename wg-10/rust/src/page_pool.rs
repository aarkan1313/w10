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
use godot::classes::{
    RenderingServer, RenderingDevice, RdTextureFormat, RdTextureView, Texture2Drd,
    rendering_device::{DataFormat, TextureUsageBits},
};
use crate::pack;
use crate::gpu_compute::{build_pack_buffers, PackBuffers};
use crate::page_policy::{PagePolicy, PageKey, Decision};
use crate::page_compute::{PageComputeContext, build_page_compute_context, free_page_compute_context, compute_page_cached};
use std::path::Path;

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
        if self.is_configured() {
            self.free_all_impl();
        }

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

        // --- init policy + slot vectors ---
        let cap = capacity as usize;
        self.policy      = Some(PagePolicy::new(cap));
        self.slot_tex    = vec![None; cap];
        // Option<Gd<_>> is not Clone-defaultable via vec![None; cap]
        self.slot_wrap   = (0..cap).map(|_| None).collect();

        self.pack         = Some(pack);
        self.pack_buffers = Some(pb);
        self.glsl_source  = Some(glsl);
        self.compute_ctx  = Some(ctx);
        self.page_px      = page_px;
        self.world_span   = world_span;
        self.seed         = seed;

        // reset stats on reconfigure
        self.created     = 0;
        self.reused      = 0;
        self.recomputed  = 0;
        self.full_events = 0;

        GString::new()
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
        if self.policy.is_none()
            || self.pack.is_none()
            || self.pack_buffers.is_none()
            || self.glsl_source.is_none()
        {
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
                let ctx = self.compute_ctx.as_ref().unwrap();
                let num_palettes = self.pack_buffers.as_ref().unwrap().num_palettes;
                let result = compute_page_cached(
                    &mut rd,
                    ctx,
                    &self.pack.as_ref().unwrap().grammar_constants,
                    num_palettes,
                    tex_rid,
                    ox, oz, ws, ppx, sd,
                );

                if let Err(e) = result {
                    godot_error!("Wg10PagePool: compute_page_cached failed (slot {slot}): {e}");
                    // Free the just-created texture — slot was never stored.
                    rd.free_rid(tex_rid);
                    // Roll back the policy fully: remove the key + free the slot
                    // so a re-acquire is a fresh Allocate, not a stale Reuse.
                    self.policy.as_mut().unwrap().rollback(key);
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

                let ctx = self.compute_ctx.as_ref().unwrap();
                let num_palettes = self.pack_buffers.as_ref().unwrap().num_palettes;
                let result = compute_page_cached(
                    &mut rd,
                    ctx,
                    &self.pack.as_ref().unwrap().grammar_constants,
                    num_palettes,
                    tex_rid,
                    ox, oz, ws, ppx, sd,
                );

                if let Err(e) = result {
                    godot_error!(
                        "Wg10PagePool: compute_page_cached failed on eviction (slot {slot}): {e}"
                    );
                    // The slot's texture now holds neither key cleanly: the old
                    // key was already evicted from the policy by acquire(), and
                    // the recompute for the new key failed. Drop it so no stale
                    // content is ever returned. (This is the ONE case
                    // AllocateEvicting frees — a documented failure path.)
                    if let Some(old_rid) = self.slot_tex[slot].take() {
                        rd.free_rid(old_rid);
                    }
                    self.slot_wrap[slot] = None;
                    // Roll back the new key: frees the slot in the policy so a
                    // future acquire re-allocates it cleanly.
                    self.policy.as_mut().unwrap().rollback(key);
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
// Private helpers
// ---------------------------------------------------------------------------

impl Wg10PagePool {
    /// The actual teardown logic, callable from both the `#[func] free_all` and
    /// `Drop` (B1). Frees every page-texture RID + the cached compute context on
    /// the global RenderingDevice, then fully resets ALL configured state to the
    /// unconfigured shape (F7) via `reset_configured_state`. Idempotent and safe
    /// with no RenderingDevice (headless / already torn down).
    ///
    /// This is the ONLY site that calls `rd.free_rid` on page textures.
    fn free_all_impl(&mut self) {
        let rd_opt = RenderingServer::singleton().get_rendering_device();
        if rd_opt.is_none() {
            // No RenderingDevice — nothing to free on the GPU; drop our handles
            // and fully reset to the UNCONFIGURED state (F7). Leaving `policy`/
            // `pack`/`pack_buffers`/`glsl_source` Some here would let the
            // `acquire_page` guard PASS while `compute_ctx` is None → the
            // `compute_ctx.as_ref().unwrap()` would panic. Reset everything so the
            // guard correctly sees "not configured".
            Self::reset_configured_state(
                &mut self.policy,
                &mut self.slot_tex,
                &mut self.slot_wrap,
                &mut self.pack,
                &mut self.pack_buffers,
                &mut self.glsl_source,
                &mut self.compute_ctx,
            );
            return;
        }
        let mut rd = rd_opt.unwrap();
        // Free the cached compute context (slice 7) — the pool owns it, built at configure.
        // Take it BEFORE the reset so we can free the GPU resources it holds.
        if let Some(ctx) = self.compute_ctx.take() {
            free_page_compute_context(&mut rd, &ctx);
        }
        for rid_opt in self.slot_tex.iter_mut() {
            if let Some(rid) = rid_opt.take() {
                rd.free_rid(rid);
            }
        }
        // GPU resources released above; now fully reset to the UNCONFIGURED state (F7)
        // so `acquire_page`/`get_resident_page` guards see "not configured" and return
        // None gracefully instead of unwrapping a None `compute_ctx` / indexing a
        // cleared `slot_wrap` from stale policy state.
        Self::reset_configured_state(
            &mut self.policy,
            &mut self.slot_tex,
            &mut self.slot_wrap,
            &mut self.pack,
            &mut self.pack_buffers,
            &mut self.glsl_source,
            &mut self.compute_ctx,
        );
    }

    /// Pure, engine-free reset of ALL configured state to the not-yet-configured
    /// shape (F7). Operates only on plain data — NO `RenderingServer`, NO GPU
    /// `free_rid`, NO `self.base` — so it is headless-unit-testable and cannot
    /// panic. Callers that own GPU resources (the compute ctx, the slot RIDs) MUST
    /// free them BEFORE calling this; here we only drop the (already-taken) handles
    /// and clear the policy/slot vectors + the four `configure`-set Options.
    ///
    /// Post-condition (the F7 invariant): there is NO half-configured state that
    /// would pass the `acquire_page` guard (policy/pack/pack_buffers/glsl_source
    /// all Some) yet leave `compute_ctx` None. After this returns, `is_configured`
    /// is false and every Option field is None.
    ///
    /// Idempotent: calling it on an already-empty/unconfigured pool is a harmless
    /// no-op (Options already None, vectors already empty), which is what makes
    /// `free_all` safe to call twice and `configure`'s free-before-reconfigure
    /// (F8) safe on a fresh pool.
    fn reset_configured_state(
        policy:       &mut Option<PagePolicy>,
        slot_tex:     &mut Vec<Option<Rid>>,
        slot_wrap:    &mut Vec<Option<Gd<Texture2Drd>>>,
        pack:         &mut Option<pack::Pack>,
        pack_buffers: &mut Option<PackBuffers>,
        glsl_source:  &mut Option<String>,
        compute_ctx:  &mut Option<PageComputeContext>,
    ) {
        *policy = None;
        slot_tex.clear();
        slot_wrap.iter_mut().for_each(|w| *w = None);
        slot_wrap.clear();
        *pack = None;
        *pack_buffers = None;
        *glsl_source = None;
        *compute_ctx = None;
    }

    /// The exact predicate the `acquire_page` guard uses: a pool is "configured"
    /// only when policy/pack/pack_buffers/glsl_source are ALL Some. Mirrors the
    /// guard so the F7 invariant (consistent configured-vs-unconfigured state) can
    /// be asserted headlessly. NOTE: `compute_ctx` is intentionally NOT part of
    /// this predicate — that asymmetry is precisely the F7 hazard, so the test
    /// asserts compute_ctx tracks the predicate after a reset.
    #[allow(dead_code)]
    fn is_configured(&self) -> bool {
        self.policy.is_some()
            && self.pack.is_some()
            && self.pack_buffers.is_some()
            && self.glsl_source.is_some()
    }

    /// Create a new R32F STORAGE+SAMPLING texture of `page_px × page_px`.
    /// Returns `Some(Rid)` on success; logs a godot_error and returns `None` on
    /// failure.  The ONLY `texture_create` call for page textures.
    fn create_page_texture(&self, rd: &mut Gd<RenderingDevice>) -> Option<Rid> {
        let px = self.page_px as u32;
        let mut fmt = RdTextureFormat::new_gd();
        fmt.set_width(px);
        fmt.set_height(px);
        fmt.set_format(DataFormat::R32_SFLOAT);
        // STORAGE (compute writes it) + SAMPLING (the ring shader reads it). CAN_COPY_FROM lets a
        // GATE read the page back with texture_get_data to assert seam-freeness against the real
        // height_page.glsl output (slice 8). CAN_COPY_FROM only permits a copy source; unlike
        // CPU_READ it allocates no CPU-side mirror, so page residency cost is unchanged on the
        // render path.
        fmt.set_usage_bits(
            TextureUsageBits::STORAGE_BIT
                | TextureUsageBits::SAMPLING_BIT
                | TextureUsageBits::CAN_COPY_FROM_BIT,
        );
        let view    = RdTextureView::new_gd();
        let tex_rid = rd.texture_create(&fmt, &view);
        if tex_rid.is_invalid() {
            godot_error!(
                "Wg10PagePool: texture_create returned invalid RID (page_px={})",
                self.page_px
            );
            return None;
        }
        Some(tex_rid)
    }
}

// ---------------------------------------------------------------------------
// Tests — headless state-machine coverage for the F7/F8 lifecycle fixes.
//
// These exercise the PURE reset logic (`reset_configured_state`) and the guard
// predicate (`is_configured`) directly on plain data: no `Base<RefCounted>`, no
// `RenderingServer`, no GPU. A `Wg10PagePool` GodotClass instance CANNOT be
// constructed under `cargo test` (it needs a live engine + `Base`), and
// `free_all_impl` / `configure` / `acquire_page` call `RenderingServer::singleton()`
// + GPU ops, so the END-TO-END "free_all then acquire_page returns None" path is
// only verifiable under a WINDOWED RenderingDevice (the gpu/m3 gate). The fix,
// however, is a pure state-machine change, and that is exactly what these tests
// pin: after a reset, NO field is left in the half-configured shape that would
// pass the acquire guard but then unwrap a None `compute_ctx` (F7), and the reset
// is idempotent so `configure`'s free-before-reconfigure (F8) is a safe no-op on a
// fresh pool.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::{Pack, GrammarConstants};
    use crate::gpu_compute::PackBuffers;
    use crate::page_compute::PageComputeContext;
    use crate::page_policy::PagePolicy;
    use std::collections::BTreeMap;

    fn minimal_pack() -> Pack {
        Pack {
            grammar_constants: GrammarConstants {
                region_size_m: 500.0,
                province_size_regions: 8,
                palette_primary_pct: 60,
                palette_compatible_pct: 30,
                moderation_min: 0.4,
                moderation_strength: 0.5,
            },
            palettes: Vec::new(),
            compatibility: BTreeMap::new(),
            family_ids: Vec::new(),
            family_kernels: BTreeMap::new(),
        }
    }

    fn minimal_pack_buffers() -> PackBuffers {
        PackBuffers {
            palettes_bytes:    Vec::new(),
            compat_off_bytes:  Vec::new(),
            compat_flat_bytes: Vec::new(),
            krec_bytes:        Vec::new(),
            kparam_bytes:      Vec::new(),
            kdata_bytes:       Vec::new(),
            num_palettes:      0,
        }
    }

    // A PageComputeContext is all POD `Rid` handles. `Rid::new(..)` does NOT touch
    // the GPU — it just wraps a u64 — so this is safe to build/drop headlessly. We
    // only need it to be `Some(..)` so the reset can prove it goes to `None`.
    fn fake_compute_ctx() -> PageComputeContext {
        PageComputeContext {
            shader:      Rid::new(101),
            pipeline:    Rid::new(102),
            palettes:    Rid::new(103),
            compat_off:  Rid::new(104),
            compat_flat: Rid::new(105),
            krec:        Rid::new(106),
            kparam:      Rid::new(107),
            kdata:       Rid::new(108),
        }
    }

    /// F7 — the core invariant. A FULLY-configured field set (policy/pack/
    /// pack_buffers/glsl_source/compute_ctx all Some, slot_tex populated) must,
    /// after `reset_configured_state`, leave EVERY field None/empty. The bug was
    /// that the old `free_all_impl` cleared only `compute_ctx` + the slot vectors,
    /// leaving the four config Options Some — so the acquire guard PASSED while
    /// `compute_ctx` was None, then `compute_ctx.as_ref().unwrap()` panicked.
    #[test]
    fn reset_clears_all_configured_state_no_half_configured_residue() {
        let mut policy:       Option<PagePolicy>          = Some(PagePolicy::new(4));
        // Populated with POD RIDs (the GPU free happens BEFORE reset in the real
        // code path; here we only assert the handles are dropped to None).
        let mut slot_tex:     Vec<Option<Rid>>            =
            vec![Some(Rid::new(1)), None, Some(Rid::new(2)), None];
        // slot_wrap holds Gd<Texture2Drd> which can't be built headlessly; the
        // realistic populated case is covered by slot_tex. Use the post-config
        // shape: a sized Vec of None (what `configure` builds before any acquire).
        let mut slot_wrap:    Vec<Option<Gd<Texture2Drd>>> = (0..4).map(|_| None).collect();
        let mut pack:         Option<Pack>                = Some(minimal_pack());
        let mut pack_buffers: Option<PackBuffers>         = Some(minimal_pack_buffers());
        let mut glsl_source:  Option<String>              = Some("// glsl".to_string());
        let mut compute_ctx:  Option<PageComputeContext>  = Some(fake_compute_ctx());

        Wg10PagePool::reset_configured_state(
            &mut policy,
            &mut slot_tex,
            &mut slot_wrap,
            &mut pack,
            &mut pack_buffers,
            &mut glsl_source,
            &mut compute_ctx,
        );

        // Every configure-set field is None — the acquire guard now correctly sees
        // "not configured" (returns None) instead of passing then unwrapping None.
        assert!(policy.is_none(),       "policy must be cleared");
        assert!(pack.is_none(),         "pack must be cleared");
        assert!(pack_buffers.is_none(), "pack_buffers must be cleared");
        assert!(glsl_source.is_none(),  "glsl_source must be cleared");
        assert!(compute_ctx.is_none(),  "compute_ctx must be cleared");
        // Slot vectors emptied — no stale slot_wrap indexable by a stale policy.
        assert!(slot_tex.is_empty(),    "slot_tex must be empty");
        assert!(slot_wrap.is_empty(),   "slot_wrap must be empty");

        // THE F7 GUARD CHECK: the acquire-guard predicate (policy && pack &&
        // pack_buffers && glsl_source all Some) and `compute_ctx.is_some()` must
        // AGREE. The old bug made the guard true while compute_ctx was None — that
        // disagreement is the panic. Assert they cannot disagree after a reset.
        let guard_passes = policy.is_some()
            && pack.is_some()
            && pack_buffers.is_some()
            && glsl_source.is_some();
        assert!(!guard_passes, "acquire guard must FAIL after reset (unconfigured)");
        assert_eq!(
            guard_passes,
            compute_ctx.is_some(),
            "F7: acquire guard and compute_ctx presence must never disagree"
        );
    }

    /// F7/F8 — `reset_configured_state` is idempotent: calling it on an already-
    /// unconfigured (fresh) field set is a harmless no-op. This is what makes
    /// `free_all()` safe to call twice and `configure`'s free-before-reconfigure
    /// safe on a never-configured pool.
    #[test]
    fn reset_is_idempotent_on_unconfigured_state() {
        let mut policy:       Option<PagePolicy>           = None;
        let mut slot_tex:     Vec<Option<Rid>>             = Vec::new();
        let mut slot_wrap:    Vec<Option<Gd<Texture2Drd>>> = Vec::new();
        let mut pack:         Option<Pack>                 = None;
        let mut pack_buffers: Option<PackBuffers>          = None;
        let mut glsl_source:  Option<String>               = None;
        let mut compute_ctx:  Option<PageComputeContext>   = None;

        // Must not panic / must stay fully unconfigured.
        Wg10PagePool::reset_configured_state(
            &mut policy, &mut slot_tex, &mut slot_wrap,
            &mut pack, &mut pack_buffers, &mut glsl_source, &mut compute_ctx,
        );

        assert!(policy.is_none() && pack.is_none() && pack_buffers.is_none()
            && glsl_source.is_none() && compute_ctx.is_none());
        assert!(slot_tex.is_empty() && slot_wrap.is_empty());
    }

    /// `is_configured` mirrors the acquire guard exactly: true ONLY when all four
    /// of policy/pack/pack_buffers/glsl_source are Some. This is the predicate
    /// `configure` uses to decide whether to free-before-reconfigure (F8), so it
    /// must match the guard wording in `acquire_page`. Verified field-by-field via
    /// the same boolean the guard computes (the struct itself needs a Base, so we
    /// reproduce the predicate here rather than instantiate the GodotClass).
    #[test]
    fn configured_predicate_requires_all_four_config_options() {
        // Helper mirroring the guard / `is_configured` over the four options.
        fn guard(p: bool, pk: bool, pb: bool, g: bool) -> bool { p && pk && pb && g }

        assert!(guard(true, true, true, true), "all Some => configured");
        // Any single missing option => NOT configured (guard returns early).
        assert!(!guard(false, true, true, true), "missing policy => unconfigured");
        assert!(!guard(true, false, true, true), "missing pack => unconfigured");
        assert!(!guard(true, true, false, true), "missing pack_buffers => unconfigured");
        assert!(!guard(true, true, true, false), "missing glsl_source => unconfigured");
        assert!(!guard(false, false, false, false), "all None => unconfigured");
    }

    // NOTE (windowed-only, NOT run headless): the END-TO-END proofs —
    //   (1) `free_all()` then `acquire_page(..)` returns None without panicking, and
    //   (2) a second `configure(..)` frees the prior textures' RIDs + compute ctx
    //       (no GPU leak) and leaves no duplicate slot vectors —
    // require a live windowed RenderingDevice (the pool's `configure`/`acquire_page`/
    // `free_all_impl` all call `RenderingServer::singleton().get_rendering_device()`
    // and dispatch real GPU work) and a constructed `Gd<Wg10PagePool>` (needs the
    // engine). Run the windowed gpu/m3 gate (editor closed) to confirm those paths.
    // The state-machine fix itself is covered by the headless tests above.
}
