# WorldGen10 — Roadmap

Ordered milestones. Mark `[x]` only when the item meets the definition of done
in DESIGN.md §7.3 (perf gate + visual gate + manual confirmation, as
applicable). Update this file in place; do not create new plan docs.

Last updated: 2026-05-29 (M3 slice 4 — clipmap rings — DONE & gated: ring_geometry (hollow ring bands) + Wg10ClipmapRings (Node3D, quantized recenter, page binding) + L↔L+1 geomorph; m3_rings_check passes WINDOWED (no holes, real relief, seam+morph continuity, recenter-no-rebuild); m3 suite 4 checks fail=0; 103 cargo tests green; M3 in progress)

Legend: `[x]` done · `[~]` partially done (note inline) · `[ ]` not started.

---

## Milestone 0 — Project skeleton & rules

- [x] Godot 4.6 project created (`wg-10/`, Forward+, D3D12, Jolt, .NET `wg10`).
- [x] Three living docs created (DESIGN / ROADMAP / STATUS).
- [ ] Addon/folder layout decided (drop-in boundary): one terrain node + one
      config resource, narrow public API.
- [x] Native backend toolchain set up (**Rust GDExtension**, carried forward
      from WG9) and loads in Godot 4.6. (`wg10_terrain` crate builds; `Wg10Hash`
      registers and is callable headlessly — verified 2026-05-28.)
- [x] Test/gate runner skeleton (headless), so gates exist before features.
      (`tools/gate.py --suite fast`; renderer-backed suites come with M3.)

## Milestone 1 — Worldgen core (CPU) + parity foundation

- [x] Port the deterministic formula: hash → noise → region/province → kernel →
      landform, as pure engine-agnostic math. **DONE: hash → value-noise → fbm →
      region/province + family grammar → kernel + landform** (`hash.rs`,
      `grammar.rs`, `npy.rs`, `height.rs`): `height(x,z,seed,pack)` pipeline
      green; bit-exact vs WG9 hash fixture + grammar + height gates green. First
      **real DEM pack** (`packs/dem_v1`) now wired (2026-05-29) — property gate
      + GPU-parity gate green on real 512×512 kernels.
- [x] Terrain-pack format defined and loadable (first pack = DEM/OpenTopo
      kernels). **DONE: format v1 + loader + validation** (`pack.rs`) + **kernel
      loading + `.npy` reader** (`npy.rs`, `pack.rs` loaders `load_pack_with_base`
      / `load_pack_dir`); rejects malformed packs; grammar reads in-memory `Pack`;
      `Pack` carries `FamilyKernel`. **First real DEM pack (`packs/dem_v1`) now
      wired and gated** (2026-05-29): 115-kernel approved map across 12 families,
      built by `tools/dem_pack/` from WG9 shortlist + metric inferences; loads
      through unchanged M1/M2 pipeline; property gate + GPU-parity gate green on
      real 512×512 kernels. Full-set streaming and visual relief/footprint tuning
      are M3 work.
- [~] Parity fixtures (hash, noise, provider decisions, sample grids) committed
      **to git**. **DONE: hash/noise fixture** (`hash_reference.json` vendored);
      provider-decision + sample-grid fixtures come with later layers.
- [x] Determinism gate (same coord → same value across callers/runs).
      (`determinism_check.gd`, in the fast suite.)
- [x] Seam gate including **x=0 / z=0 axis-crossing** exact-zero edges.
      (Rust `value_noise_is_continuous_across_zero_axis` locks floor semantics.)

## Milestone 2 — GPU formula + parity

- [x] GPU compute implementation of the same formula (no readback in production).
      Done: synthetic-kernel formula (hash→grammar→height) ported to GLSL compute
      (`height_field.glsl`), dispatched by `Wg10GpuCompute` (RenderingDevice,
      windowed). Readback exists ONLY in the parity gate (one-off compare), not in
      the eventual render path (M3).
- [x] CPU/GPU parity gate (bit-close; documented epsilon only if profiled).
      Done: Tier-1 family selection EXACT (bit-exact `family_signature` over 576
      coords); Tier-2 height within f32 epsilon (ABS_EPS=1e-2 m, observed max
      delta 7.67e-5 m — 130× headroom). Verified on D3D12/RTX 5090 Laptop GPU.
      `gpu` gate suite runs windowed; `fast` stays headless (now 5 checks,
      fail=0); `gpu` suite now 2 checks, fail=0 (synthetic parity + DEM
      parity). 67 Rust unit/property tests green. M2 is a CPU-math + parity
      milestone — its definition of done is the parity gate, not a visual/fly-test
      gate (that applies to the render pipeline, M3).

## Milestone 3 — Render pipeline at speed (the hard part)

**[~] Slice 1 DONE (2026-05-29):** `Wg10PageCompute` (native Rust class, global
RenderingDevice) runs `height_page.glsl` to write one DEM height page into an
R32F `Texture2DRD` (no readback). `ring_displace.gdshader` samples it in
`vertex()` to displace a flat ring mesh. Result captured to `m3_slice1.png` and
gated by `m3_slice1_check.gd` (`m3` suite, WINDOWED): distinct quantized colors
= 18, nonblack_frac = 1.0. Clear mountain/ridge/valley relief visible. The
Texture2DRD → material → displaced-mesh path is proven. ONE static page, ONE
ring, ONE frame — no streaming, no movement, no multi-ring. M3 milestone OPEN.

**[~] Slice 2 DONE (2026-05-29):** `PagePolicy` (pure Rust, no godot) — the
eviction bookkeeping: fixed-capacity slots, (level,origin)→slot map, LRU order,
protected set. Returns DECISIONS (Reuse/Allocate/AllocateEvicting/Full); owns no
RIDs. 11 headless cargo tests: protected pages NEVER evicted, budget NEVER
exceeded, cache hits reuse the slot, all-protected→Full (no panic), release makes
slot evictable, re-acquire re-protects, `rollback(key)` on producer failure (no
phantom slot, no stale content). `Wg10PagePool` (godot) — THE single owner of all
page RIDs; asks PagePolicy what to do; the ONLY texture_create/free_rid for pages
(3 internal free sites). Eviction reuses the slot's texture (same dims → zero
mid-run RID churn). `Wg10PageCompute` refactored to a stateless producer:
`compute_into_texture` writes height into a pool-provided RID — no longer creates
or owns textures. Slice-1 regression-guarded: m3_slice1_check acquires via the
pool; distinct=18 byte-identical PNG (rendering preserved). New
`m3_pool_check.gd` (`m3` suite, WINDOWED): drives acquire/release on a
capacity-2 pool, asserts RIDs reuse on hit (created stays 2), budget never exceeded
(resident≤2), protected page survives over-budget acquire, Full returns null
(full_events≥1), eviction reuses slot, pooled page renders (distinct=18). m3 suite
now 2 checks, fail=0. Cargo tests: was 70, now 81 (+9 PagePolicy +2 rollback).
Pool driven by explicit acquire/release — NOT a live frame loop. M3 OPEN.

Remaining slices (NOT done):

- [x] `page_scheduler`: velocity-aware stream-ahead, bounded computes/frame,
      coarser-page fallback (never black, never stall). **DONE (2026-05-29, slice 3).**
      `SchedulePolicy` (pure Rust, no godot: `coverage` velocity-led multi-level ring,
      `coarser_fallback` never-black ancestor walk, `plan_frame` bounded
      **coarsest-first** acquire/release — 14 cargo tests incl. a 2000-sample
      never-black property test) + `Wg10Streamer` (godot §5.4 frame-loop driver,
      delegates all math, owns no RIDs) + `Wg10PagePool::resident_keys()` (only pool
      change) + `m3_stream_check.gd` (m3 suite → 3 checks, WINDOWED). Gate passes over
      a 60-frame 6000 m/s sweep: bounded, budget-safe, never-black, deterministic,
      non-vacuous (fallback genuinely fires). Coarsest-first priority + lead/budget
      tuning make never-black STRUCTURAL — the windowed gate falsified the original
      finest-first design (see spec §2.3). Synchronous produce this slice; the
      scheduler↔pool seam is async-ready (zero scheduler change when background
      production lands — trigger = heavy multi-pass pages, M5–M7).
- [x] `clipmap_rings`: fixed concentric rings, persistent meshes, recenter on
      move, shader displace + L↔L+1 morph. **DONE (2026-05-29, slice 4).**
      `ring_geometry` (pure Rust: `RingLayout` level spans + `band_mesh` filled grid /
      hollow ring bands, gapless tiling, 7 cargo tests incl. consistent-winding +
      grid_res%4 guard) + `Wg10ClipmapRings` (godot Node3D — first non-RefCounted class:
      N persistent ArrayMesh children, quantized `recenter` that never rebuilds,
      `bind_page` for per-level height + coarser-neighbor textures; owns no RIDs) +
      L↔L+1 **geomorph** in `ring_displace.gdshader` (blend finer edge toward the coarser
      surface at the same world point, `t=1` at the seam → crack-free; backward-compatible
      no-morph default keeps slice-1/2 gates passing). `m3_rings_check.gd` (m3 suite →
      4 checks, WINDOWED): top-down ortho asserts no holes, real relief, seam continuity,
      morph continuity, recenter-no-rebuild; PNG eyeballed. One-band-one-page binding
      (scheduler radius_pages=0); transient Texture2DRD-second-sampler startup warning is
      benign (render correct, not per-frame).
- [ ] Modular harness components: camera/movement, diagnostics/profiling, UI
      overlay (live fps/stats).
- [ ] Manual fly-test scene: WASD + Shift speed + mouse look + Space/C vertical,
      free-fly (+ optional ground-follow).
- [ ] Renderer-backed acceptance gate: no large black/missing component AND
      **renderer frame p99 < 6 ms**, in motion at ~1000 m/s.
- [ ] Tune finest-ring spacing + ring count against the review scene (config;
      not a locked constant — revisit when real assets exist).
- [ ] **MANUAL ACCEPTANCE:** owner flies it at full speed and confirms no
      stalls and no black/holes. (Gate green is necessary, not sufficient.)

## Milestone 4 — Facts API (authoritative, sparse)

- [ ] `get_height(x, z)` authoritative sparse query.
- [ ] `get_collision_field(area)` + Jolt `HeightMapShape3D` integration.
- [ ] Save/edit layer hook (composition over base height).

## Milestone 5 — Detail & masks (GPU, render-only)

- [ ] Detail/displacement layer (bounded, shader-only, edge-safe).
- [ ] Slope/curvature/debug + world-space masks.

## Milestone 6 — Biomes & textures (data-driven)

- [ ] Stable world-space biome/material masks driven by terrain-family rules.
- [ ] Texture/material packs (swappable, like terrain packs).

## Milestone 7 — Erosion & hydrology

- [ ] River/pass routing facts.
- [ ] Erosion/hydrology, integrated without breaking determinism/parity.

---

## Pre-work follow-up (not blocking M0/M1 doc work)

- [x] **Review OpenTopo kernel-extraction methodology** (done 2026-05-28,
      conclusion in DESIGN §9): methodology sound, cache sufficient. Pack-build
      follow-ups: mask NoData holes; improve family tagging (591/703
      uncategorized).
- [ ] **Async/background page production** (deferred pool-layer follow-up, tracked
      from M3 slice 3): scheduler is async-ready; build the background producer
      behind `Wg10PagePool::acquire_page` when synchronous N-per-frame computes blow
      the frame budget. **Trigger:** heavy multi-pass pages — M5 (detail/normals),
      M6 (biome masks), M7 (erosion/hydrology). Zero scheduler change required.
