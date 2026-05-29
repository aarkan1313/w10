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
use crate::page_compute::compute_into_texture;
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

        // --- init policy + slot vectors ---
        let cap = capacity as usize;
        self.policy      = Some(PagePolicy::new(cap));
        self.slot_tex    = vec![None; cap];
        // Option<Gd<_>> is not Clone-defaultable via vec![None; cap]
        self.slot_wrap   = (0..cap).map(|_| None).collect();

        self.pack         = Some(pack);
        self.pack_buffers = Some(pb);
        self.glsl_source  = Some(glsl);
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
        let (ox, oz, ws, ppx, sd) =
            (origin_x, origin_z, self.world_span, self.page_px, self.seed);

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

                // Borrow pack/pb/glsl immutably; no mutable slot access yet.
                let glsl = self.glsl_source.as_deref().unwrap().to_owned();
                let result = compute_into_texture(
                    &mut rd,
                    self.pack.as_ref().unwrap(),
                    self.pack_buffers.as_ref().unwrap(),
                    tex_rid,
                    &glsl,
                    ox, oz, ws, ppx, sd,
                );

                if let Err(e) = result {
                    godot_error!("Wg10PagePool: compute_into_texture failed (slot {slot}): {e}");
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

                let glsl = self.glsl_source.as_deref().unwrap().to_owned();
                let result = compute_into_texture(
                    &mut rd,
                    self.pack.as_ref().unwrap(),
                    self.pack_buffers.as_ref().unwrap(),
                    tex_rid,
                    &glsl,
                    ox, oz, ws, ppx, sd,
                );

                if let Err(e) = result {
                    godot_error!(
                        "Wg10PagePool: compute_into_texture failed on eviction (slot {slot}): {e}"
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
    // free_all  (the ONLY place that frees page texture RIDs)
    // -----------------------------------------------------------------------

    /// Free all page texture RIDs on the global RenderingDevice and clear the
    /// slot vectors.  Must be called during scene teardown; the GDScript owner
    /// is responsible for calling this before the pool goes out of scope.
    ///
    /// This is the ONLY site that calls `rd.free_rid` on page textures.
    #[func]
    pub fn free_all(&mut self) {
        let rd_opt = RenderingServer::singleton().get_rendering_device();
        if rd_opt.is_none() {
            // Windowed mode — no RIDs to free.
            self.slot_tex  = Vec::new();
            self.slot_wrap = Vec::new();
            return;
        }
        let mut rd = rd_opt.unwrap();
        for rid_opt in self.slot_tex.iter_mut() {
            if let Some(rid) = rid_opt.take() {
                rd.free_rid(rid);
            }
        }
        self.slot_tex.clear();
        self.slot_wrap.iter_mut().for_each(|w| *w = None);
        self.slot_wrap.clear();
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

impl Wg10PagePool {
    /// Create a new R32F STORAGE+SAMPLING texture of `page_px × page_px`.
    /// Returns `Some(Rid)` on success; logs a godot_error and returns `None` on
    /// failure.  The ONLY `texture_create` call for page textures.
    fn create_page_texture(&self, rd: &mut Gd<RenderingDevice>) -> Option<Rid> {
        let px = self.page_px as u32;
        let mut fmt = RdTextureFormat::new_gd();
        fmt.set_width(px);
        fmt.set_height(px);
        fmt.set_format(DataFormat::R32_SFLOAT);
        fmt.set_usage_bits(
            TextureUsageBits::STORAGE_BIT | TextureUsageBits::SAMPLING_BIT,
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
