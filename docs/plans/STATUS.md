# WorldGen10 — Status

What is actually true right now. Update this whenever reality changes. If a
manual fly contradicts a claim here, fix this file immediately. (Separating
"what passed a counter gate" from "what is actually accepted" is the whole
point — see DESIGN §7.3.)

Last updated: 2026-05-29 (M3 slice 2 done; **slice 3 — stream-ahead scheduler — DESIGNED & spec'd** (`docs/superpowers/specs/2026-05-29-m3-slice3-design.md`), not yet built. Latest BUILT slice is still slice 2: page pool single RID owner, LRU+protected, zero-churn eviction; PagePolicy 11 headless tests; m3 suite 2 checks fail=0; 81 cargo tests green; M3 in progress)

---

## Current state

**Phase:** M0 toolchain green + M1 deterministic bedrock + grammar + height + first real DEM pack wired + M2 GPU formula + parity gate green + **M3 IN PROGRESS — slice 1 (first rendered page) + slice 2 (page pool) DONE**.

- Godot 4.6 project at `wg-10/` (Forward+, D3D12, Jolt, .NET `wg10`).
- Native `wg10_terrain` Rust GDExtension **builds and loads in Godot 4.6**.
  `Wg10Hash` (RefCounted) exposes `stable_hash_ints`, `hash_grid`, `value_noise`,
  `fbm`. `Wg10Grammar` (RefCounted) exposes `load_pack_json` + `family_ids` /
  `weight_values` (parallel packed arrays). `Wg10Height` (RefCounted) exposes
  `load_pack_dir` + `height` + `family_signature` queries.
- Deterministic core ported from WG9 into `wg-10/rust/src/hash.rs` (pure, no
  `godot` imports): FNV-1a `stable_hash`, `hash_grid`, `value_noise`, `fbm`,
  `fade`, `smoothstep_unit`. **Bit-exact vs WG9 `hash_reference.json`** (the
  fixture is vendored at `wg-10/worldgen_terrain/fixtures/`).
- **GPU-portable integer hash** `hash::stable_hash_ints(salt: u32, &[i64]) -> u32`
  (`hash.rs`): pure u32-wrapping FNV-1a fold, bit-identical on CPU and GLSL `uint`.
  Golden-value locked. Separate from the bedrock `hash_grid` (64-bit-multiply
  scheme, untouched).
- **Grammar rolls refactored** (`grammar.rs`): the 5 roll sites switched from
  string-join hashing to `stable_hash_ints` with distinct integer salts. New
  seed-space (accepted; WG10 grammar was never a WG9 parity contract). All grammar
  property tests pass unchanged; WG9-bit-exact bedrock untouched.
- **Terrain-pack v1 loader/validation** (`wg-10/rust/src/pack.rs`): schema
  `worldgen10.terrain_pack.v1`, validated on load, rejects malformed packs with
  descriptive errors, never silent defaults. `FAMILIES_PER_PALETTE = 3` fixed.
  `Pack` carries `family_kernels: BTreeMap<String, FamilyKernel>` via loaders
  `load_pack_with_base`/`load_pack_dir`.
- **Pure-Rust NumPy-v1.0 `.npy` reader** (`wg-10/rust/src/npy.rs`): parses
  C-order `<f4`/`<f8` 2-D arrays; rejects bad magic, version≠1, non-float dtype,
  Fortran order, non-2D shape, zero dims, overflowing shape. Descriptive errors,
  no silent defaults.
- **Grammar core** (`wg-10/rust/src/grammar.rs`): region/province locate (floor
  semantics), palette decision, `family_weights` corner blend — bounded, no heap
  allocation, normalized, deterministic, seam-continuous. Produces WEIGHTS ONLY —
  never reads kernel data.
- **Height core** (`wg-10/rust/src/height.rs`, pure, no godot): `sample_kernel`
  (tiled bilinear, scaled to `relief_m` — C0 across footprint seams; visible
  creases at footprint repeats are EXPECTED for naive tiling);
  `moderation` amplitude-only; `height(x,z,seed,&Pack)` = blend each
  grammar-selected family's moderated kernel sample by its weight.
- **First real DEM terrain pack** (`wg-10/worldgen_terrain/packs/dem_v1/`):
  115-kernel approved map across 12 families (coast, badlands, grassland, karst,
  glacial, mountain, rainforest, desert, volcanic, wetland, temperate, tundra),
  6–13 kernels each. Built by `tools/dem_pack/` (Python) from WG9's 602-kernel
  user shortlist + metric-driven family inferences. Rust crate **unchanged** — real
  pack loads through the existing M1/M2 loader/grammar/height interfaces.
  Temperate and tundra rebalanced from 1 kernel each (WG9) to 7 each via 12 new
  DEMs fetched from OpenTopo COP30 (0.5° bbox). Build-time spike filter dropped 3
  corrupt kernels (|Z|>12: Mekong delta z=44, Sahel Chad z=14, South Georgia z=12).
  Kernels are **Z-SCORE normalized** (mean 0, std 1) — height legitimately goes
  negative and can exceed `relief_m`; this is correct. `relief_m`=height_range_m
  (real elevation span ~990–2765 m); `footprint_m`=approx_sample_spacing_m×sample_px
  (~50 km); `footprint_scale` knob exists for M3 visual tuning. Committed gate
  subset only; full set generated on demand. Manual tag review deferred.
- **GPU compute shader** `height_field.glsl` (`wg-10/worldgen_terrain/shaders/`):
  hand-ported GLSL compute shader implementing hash→grammar→height end-to-end.
  Dispatched by `Wg10GpuCompute` (`gpu_compute.rs`), the only new
  RenderingDevice file; packs kernel atlas + coords as storage buffers, reads
  back height + family-signature buffers. **Runs WINDOWED** (headless
  RenderingDevice returns null on this D3D12 setup).
- **CPU/GPU parity gate** (`gpu_parity_check.gd`, `gpu` suite): verified on
  D3D12/RTX 5090 Laptop GPU over 576 coords with synthetic kernels. Tier 1:
  family-selection signatures EXACT (bit-exact). Tier 2: height within f32 epsilon
  (ABS_EPS=1e-2 m, observed max delta 7.67e-5 m — 130× headroom).
  `parity::family_signature` on CPU mirrors the GPU's signature;
  `Wg10Height::family_signature` exposes it.
- **DEM property gate** (`dem_pack_check.gd`, `fast` suite, HEADLESS): asserts
  finite output, bounded by `max_relief×12`, determinism, and height variety across
  a real DEM pack grid.
- **DEM GPU-parity gate** (`gpu_parity_dem_check.gd`, `gpu` suite, WINDOWED):
  dispatches real 512×512 kernels (~25 MB atlas) on D3D12/RTX 5090. Tier-1 family
  signatures EXACT; Tier-2 height maxd=0.040 m on ~6 km relief (within tolerance).
  **This validated the M2 kernel-atlas at real 512×512 scale — the named atlas-at-
  scale risk is closed.**
- **M3 slice 1 — `Wg10PageCompute` native class** (`page_compute.rs`,
  `height_page.glsl`): runs on the GLOBAL RenderingDevice (no readback); writes
  one DEM height page into an R32F `Texture2DRD`. Scene consts drive page
  origin/span/px, grid resolution, camera, height_scale — config-driven, no
  scattered magic numbers.
- **`ring_displace.gdshader`**: spatial shader sampling the `Texture2DRD` in
  `vertex()` to displace a flat ring mesh. Combined with `Wg10PageCompute`, the
  full compute → Texture2DRD → material → displaced-mesh path is proven.
- **`m3_slice1_check.gd`** (`m3` suite, WINDOWED): renders one static page +
  ring + frame, captures to `m3_slice1.png`, asserts real relief (distinct
  quantized colors ≥ 8; flat/black frames fail). Passes: distinct=18,
  nonblack_frac=1.0. Non-vacuous — a flat plane yields 2 buckets → fail.
  PNG inspected by eye: clear mountain/ridge/valley relief visible.
- **M3 slice 2 — `PagePolicy`** (`page_policy.rs`, pure Rust, no godot): the
  eviction bookkeeping — fixed-capacity slots, (level,origin)→slot map, LRU order,
  protected set. Returns DECISIONS (Reuse/Allocate/AllocateEvicting/Full); owns no
  RIDs. The WG9-killer rules proven headless (11 cargo tests): protected pages
  NEVER evicted, budget NEVER exceeded, cache hits reuse the slot,
  all-protected→Full (no panic, no wrong evict), release makes a slot evictable,
  re-acquire re-protects, deterministic, + `rollback(key)` (used on producer
  failure to keep policy/texture state consistent — no phantom slot, no panic, no
  stale content).
- **M3 slice 2 — `Wg10PagePool`** (`page_pool.rs`, godot): THE single owner of
  all page RIDs (the §5.2 anti-WG9 rule). Asks PagePolicy what to do. The ONLY
  texture_create/free_rid for pages live here (3 internal free sites: free_all
  teardown + two produce-failure cleanups). acquire_page/release_page/stats/
  configure/free_all. Eviction REUSES the slot's texture (same dims → zero
  mid-run RID churn).
- **M3 slice 2 — `Wg10PageCompute` refactored to stateless producer:**
  `compute_into_texture` writes height into a pool-provided RID — no longer creates
  or owns textures. Dispatch byte-identical to slice 1 (parity-proven).
  Slice-1 regression-guarded: m3_slice1_check acquires its page via Wg10PagePool;
  still renders distinct=18 byte-identical PNG (rendering preserved).
- **`m3_pool_check.gd`** (`m3` suite, WINDOWED): drives acquire/release on a
  capacity-2 pool; asserts RIDs reuse on hit (created stays 2), budget never
  exceeded (resident≤2), protected page survives over-budget acquire, Full returns
  null (full_events≥1), eviction reuses slot (recomputed, not created), pooled page
  renders distinct=18. Pool driven by explicit acquire/release — NOT a frame loop.
- Gate runner: `python tools/gate.py --suite fast` → `[gate] suite=fast checks=5
  fail=0` (headless). `--suite gpu` → `[gate] suite=gpu checks=2 fail=0 skip=0`
  (windowed). `--suite m3` → `[gate] suite=m3 checks=2 fail=0` (windowed).
- Three living docs (DESIGN, ROADMAP, STATUS). Architecture locked — see DESIGN.

## What works

- **Deterministic hash/noise bedrock, proven bit-exact** against WG9 — at both
  the Rust unit level and through the Godot native boundary (hash parity +
  determinism gates).
- **Grammar property gate** (`grammar_check.gd`, fast suite): asserts sum=1,
  determinism, id/weight array parallelism, and family variety across a region
  grid (no single-palette collapse).
- **Height property gate** (`height_check.gd`, fast suite): asserts finite output,
  determinism across two independent calls, bounded output within pack relief
  range, and variety across a grid (no flat-collapse).
- **DEM property gate** (`dem_pack_check.gd`, fast suite): finite, bounded
  (max_relief×12), deterministic, varied — on real DEM pack kernels. HEADLESS.
- **Fast suite: 5 checks, fail=0** (headless).
- **GPU parity gate** (`gpu_parity_check.gd`, `gpu` suite): family selection EXACT
  + height within f32 epsilon on D3D12/RTX 5090; runs windowed. Returns SKIP code 2
  on no-GPU/headless box.
- **DEM GPU-parity gate** (`gpu_parity_dem_check.gd`, `gpu` suite): real 512×512
  kernels (~25 MB atlas) dispatched + read back on D3D12/RTX 5090. Tier-1 EXACT,
  Tier-2 maxd=0.040 m on ~6 km relief. Validates M2 atlas at real scale — atlas-
  at-scale risk closed.
- **GPU suite: 2 checks, fail=0** (windowed).
- **M3 slice-1 gate** (`m3_slice1_check.gd`, `m3` suite, WINDOWED): distinct=18,
  nonblack_frac=1.0, fail=0. One static page, one ring, one frame — Texture2DRD→
  material→displaced-mesh path proven. PNG inspected: real DEM mountain/ridge/
  valley relief visible. (Regression-guarded through slice 2: still passes
  distinct=18 after the pool refactor.)
- **M3 pool gate** (`m3_pool_check.gd`, `m3` suite, WINDOWED): capacity-2 pool,
  explicit acquire/release. created=2 (RID reuse on hit), resident≤2 (budget
  enforced), full_events≥1 (Full path exercised), pooled page distinct=18.
- **m3 suite: 2 checks, fail=0** (windowed). fast=5, gpu=2 unchanged.
- **81 Rust unit/property tests green** (70 prior + 9 PagePolicy + 2 rollback).
  One exact-value anchor: all-flat pack yields `height == 500.0` at any coord.
- **Verification shape for M3:** windowed + visual. The m3 gate proves the render
  path only. Value-correctness leans on the M2 gpu_parity gate (same formula).
  Global RenderingDevice is null under --headless on this D3D12 box — same
  constraint as the gpu suite. SKIP code 2 returned on no-GPU/headless box.
- Pool driven by explicit acquire/release. No scheduler, no rings, no streaming
  loop, no movement, no perf number, no fly-test. M3 milestone OPEN.
  (Honest baseline — slice 2 proves "a bounded pool owns all page RIDs, enforces
  budget, never evicts protected pages, and reuses slot textures"; driving it under
  motion with the scheduler is slice 3.)

## What's next

1. **M3 slice 3 — stream-ahead scheduler:** velocity-aware, bounded
   computes/frame, coarser-page fallback (never black, never stall). This is the
   first slice that USES the pool's acquire/Full under MOTION — the first live
   frame loop. **DESIGNED & spec'd** (`docs/superpowers/specs/2026-05-29-m3-slice3-design.md`,
   approved) — NOT yet built. Shape: `SchedulePolicy` (pure Rust, no godot —
   `coverage`/`plan_frame`/`coarser_fallback`, multi-level, bounded acquires,
   never-black property), `Wg10Streamer` (godot frame-loop driver, synchronous
   produce this slice), `page_pool.rs` gains `resident_keys()` (only pool change),
   `m3_stream_check.gd` (m3 suite → 3 checks). Page production is **synchronous**
   this slice but the scheduler↔pool seam is **async-ready** (scheduler never
   assumes same-frame residency) so background production drops in later with zero
   scheduler change. Next: writing-plans → subagent-driven execution → audit.
   Then: clipmap rings (concentric, persistent meshes, recenter on
   move, L↔L+1 morph), modular harness components (camera/movement,
   diagnostics/profiling, UI overlay), manual fly-test scene, and the real M3
   acceptance gate (p99 < 6 ms + no-black, manually confirmed at ~1000 m/s).
   M3 milestone remains OPEN.
2. **Visual tuning of `relief_m` / `footprint_m`** (deferred to M3): physical
   ground-truth values in place; visual feel needs the renderer. `footprint_scale`
   knob exists for then.
3. **Full-pack streaming** (deferred to M3): gate-committed subset loads now;
   full ~115-kernel set is generated on demand but not yet streamed.
4. **Anti-repetition / kernel variety tuning**: naive single-kernel tiling
   visibly creases at footprint seam boundaries (C0 not C1); deferred until the
   renderer can show it.

## Decisions locked

- Native backend: **Rust GDExtension** (carried forward from WG9).
- Renderer acceptance budget: **frame p99 < 6 ms at ~1000 m/s**.
- Finest-ring spacing / ring count: **config-driven, value deliberately not
  locked** — tune against real assets later.

## Known risks / watch-items

- OpenTopo kernel methodology REVIEWED 2026-05-28 (see DESIGN §9): sound, cache
  is sufficient, no blocking issues. Two follow-ups for future packs: mask NoData
  holes properly; improve family tagging (591/703 WG9 kernels were `uncategorized`;
  dem_v1 approved map covers 115 across 12 families, tag accuracy unreviewed).
- Grammar↔kernel coupling RESOLVED 2026-05-29 (see DESIGN §9): moderation is
  amplitude-only in the height layer; grammar never reads kernel data.
- **GPU kernel-atlas for varied sizes — CLOSED 2026-05-29** (see DESIGN §9):
  validated on real 512×512 kernels at ~25 MB atlas; no redesign needed.
- **DEM kernel Z-score normalization:** height is NOT [0,1]; goes negative; can
  exceed `relief_m`. Build-time filter drops |Z|>12 spikes. Normal behavior —
  document clearly for any M3 shader work that consumes the pages.
- **Manual tag review deferred:** dem_v1 approved map seeded from confidence≥0.7
  metric inferences; no human thumbnail review done. Tooling ready for when it is.
- Naive kernel tiling creases at footprint seam boundaries (C0, not C1) — expected
  behavior; deferred until the renderer can show it.
- Finest-ring spacing affects near-detail radius and interacts with future
  asset/texture scale; tune against real assets in M3.
- **GPU compute is windowed-only:** `Wg10GpuCompute`, `Wg10PageCompute`, and all
  `gpu`/`m3` gates require a windowed run; headless returns null RenderingDevice
  on this D3D12 setup. SKIP code 2 is returned on no-GPU/headless box — never
  miscounted as a pass.
- **Texture RID ownership — RESOLVED slice 2:** `Wg10PagePool` is now the single
  owner of all page RIDs (DESIGN §5.2). free_all/teardown + two produce-failure
  cleanup sites cover every allocation. The slice-1 one-shot is regression-gated
  via the pool path.
- **Async page production — DEFERRED (tracked, not a gap):** M3 slice 3 produces
  pages SYNCHRONOUSLY inside the streamer's `update` (≤ N/frame, so still bounded).
  The scheduler↔pool seam is deliberately **async-ready** — the scheduler reads only
  the *observed* resident set and always has a coarser fallback, so it never assumes
  a page is resident the same frame it was requested. **Trigger to actually build
  background production:** when a single page compute becomes heavy enough that N
  synchronous computes blow the frame budget — i.e. multi-pass pages from M5
  (detail/normals), M6 (biome masks), or M7 (erosion/hydrology). At that point it is
  a pool/streamer-layer change behind `acquire_page` with ZERO scheduler change.
  (Spec §1.1, §7.)

## Build / run gotchas (learned 2026-05-28 wiring the toolchain)

- **`CARGO_TARGET_DIR` is set globally on this machine** (to
  `D:\cargo-target-kalshi`). It OVERRIDES `wg-10/rust/.cargo/config.toml`'s
  `target-dir`, so `cargo build`/`cargo test` send output to the global dir and
  the `.gdextension` can't find the dll. **Unset it per-invocation** when
  building/testing this crate: `$env:CARGO_TARGET_DIR=$null; cargo build`. The
  committed `.cargo/config.toml` makes the local layout correct on a clean
  machine (no global var) — it's only this machine that needs the unset.
- **`.gdextension` library path is `res://rust/target/debug/wg10_terrain.dll`** —
  resolved from the PROJECT ROOT, not relative to the `.gdextension` file.
  Godot `res://` cannot escape the project root with `..`.
- **GDExtension only loads after an editor import pass** writes
  `.godot/extension_list.cfg`. A bare `--headless --script` run on a clean
  checkout will NOT register `Wg10Hash`. `tools/gate.py` runs
  `--headless --import` first to handle this; do the same for any new check.
- **`--quit` without a main scene pops a blocking ALERT dialog** (even headless).
  Use `--script` (SceneTree) for checks, never `--quit`, in automated runs.
- Headless is fine for this pure-CPU layer; GPU work (M2+) won't run headless.

## Reference

- Predecessor: `d:/workflows/worldgen9` — read for knowledge (formulas,
  contracts, lessons); do not copy code. Its render layer is the cautionary
  tale (per-chunk synchronous GPU pages → 128 ms/chunk → black slabs + 5 fps at
  speed).
- Godot binary used for gates:
  `C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe`
