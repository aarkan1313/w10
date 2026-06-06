# WG10 Handoff — 2026-06-05 (region-fact producer ON SCREEN; unified review scene)

Single pickup point for the next session. **Supersedes
`WG10_HANDOFF_2026-06-05_CARVE_PORTED.md`** (which predates the producer integration
this session built). Live truth source remains `docs/plans/STATUS.md` (top).

## TL;DR

The carved "baked look" now **reaches the screen** through the live `Wg10PagePool`, via
an off-frame async super-region bake. The un-intercept Rung-1 gap is **CLOSED**. The
project's core arc — "the baked look, procedurally, on screen, seamless" — is delivered
for the producer path; what remains is one owner **visual A/B** and a short list of
honestly-deferred items.

Branch `slice4-gpu-page-integration`, everything committed + pushed to origin
(`github.com/aarkan1313/w10`). cargo lib **268/268** green. Worktree has ~245 preexisting
dirty files (old backlog) — DO NOT clean/reset/broad-checkout; all this session's work was
committed via scoped `git add` of only its own files.

## What this session built (all gated; on hardware where windowed)

1. **Region-fact producer integration** — `bake_region` wired into the live pool. Shape:
   **GPU super-macro readback → CPU carve (~19 ms) + condition (~2 ms) → sliced region
   facts → live page sampling.** Forced by measurement (all-CPU bake ~3 s/region). The
   async bake worker (`region_bake/worker.rs`) is a dedicated thread with its **own
   RenderingDevice** (per-thread; never shares the pool's RD); never-black coarse fallback
   while a region bakes; `Drop` closes tx-then-joins (no deadlock).
2. **Seam-exactness, decomposed + fixed in layers** (the measured ~1090 m "condition seam"
   was actually THREE sources):
   - **Percentiles** → `SmoothFieldPercentiles` (world-keyed smooth percentile FIELD behind
     a swappable `PercentileProvider` interface — engine modularity). **0-ULP seam-exact**,
     including across super-region OUTER borders (`outer_seam_tests.rs`).
   - **Carve + conditioning gaussian** → `bake_super_region`: bake a k×k SUPER-region as ONE
     field, then SLICE into region facts. Internal borders **0-ULP seam-exact BY
     CONSTRUCTION** (the global Dijkstra carve runs once over the super-field; the
     edge-clamped gaussian likewise). `k` is a modular knob (k=1 = single region).
   - `condition_world` is now a pure transform over per-cell percentile fields (length-1 =
     scalar broadcast, **bit-exact** to the old path → all Python-parity + bake_region gates
     stay green).
3. **Non-visual gates added** (post-integration): outer-border percentile seam (0-ULP across
   super-keys; measures the ~5.9 m conditioned outer residual — the k-knob tradeoff, printed
   not hidden), slice→sample bit-faithfulness (max_err 0.0), and a `region_fact_stats()`
   getter for the HUD.
4. **Unified review scene** (see below).

### KEY FINDING (corrects a prior spec assumption — carry this forward)
The bake-region assembly spec claimed "the seam-safe macro + carve are seam-exact." **Only
the MACRO is.** The carve (`pass_network/routes.rs`) runs a GLOBAL edge-to-edge Dijkstra, so
INDEPENDENT per-region carves seam by ~3500 m. The fix is super-region bake-then-slice (the
proven carve-big-then-slice model). The genuinely-infinite-per-region answer
(core-local-anchored carve) remains correctly deferred behind the player-to-world SCALE
CONTRACT — do not brute-force it before the scale contract is settled.

## Verification (RTX 5090, editor-closed unless noted)
- cargo lib **268/268**; isolated build via `CARGO_TARGET_DIR=D:/tmp/wg10_check_target`.
- Windowed gates GREEN: `region_macro` (GPU super-macro readback), `bake_worker` (worker
  round-trips 2 super-regions, own RD), `region_rung1` (ON-SCREEN: a baked region page
  upgrades past the never-black fallback, finite + non-degenerate, and two adjacent pages
  agree to ~1 mm at their shared **INTERNAL** super-region border).
- Regression GREEN: `gpu` 4/4, `biome_page` 3/3 (no GPU-parity disturbance).
- Run all: `$env:GODOT_BIN='C:\Godot\v4.6.2\...console.exe'; python tools\gate.py --suite <name>`.

## THE REVIEW SCENE (this is what you asked to "test every feature")

**`wg-10/worldgen_terrain/harness/feature_review.tscn`** — run WINDOWED. ONE scene, every
shippable terrain feature, sequentially, on the same clipmap pipeline. 8 steps:

| # | Step | What it shows |
|---|------|---------------|
| 1 | Accepted reference baseline | static mountain-network payload (the accepted look) |
| 2 | Live procedural mountain (macro+flow) | raw GPU recipe, NOT reference-bound — the genuine live look |
| 3 | Reference-backed mountain bridge | live producer bound to the payload — MATCHES step 1 by design (a bridge) |
| 4 | **CARVED BAKED LOOK (region-fact)** | **this session's feature — its first fly scene; the owner A/B lives here** |
| 5 | World composition (diagnostic) | grammar route/weight overlay (compose hitches → diagnostic) |
| 6 | Legacy DEM atlas | regression baseline |
| 7 | **FACTS / COLLISION field** | `get_collision_field` sampled around the camera, drawn as a height-colored point cloud (shipped subsystem's FIRST visual surface) |
| 8 | **TERRAIN EDITS** | `apply_edit` — F=crater, G=mound, X=clear; the collision overlay updates live (M4 edit API's FIRST visual surface) |

Controls: `]`/`[` next/prev · `1-0` jump · WASD+Shift fly (~1000 m/s) · M morph heatmap ·
N detail · R reframe · P profiling snapshot · (facts steps) F crater / G mound / X clear.
HUD: fps / frame p99 / **real GPU p99** (viewport_get_measured_render_time_gpu) / pool stats /
region-fact bake progress / facts status. Composes the existing harness (Wg10FlyCamera,
Wg10Profiler, the runtime-config helper, the producer helpers, TerrainView/ClipmapRings) — no
duplication. Smoke-verified windowed: every step configures clean, no script errors; the
facts/collision + edit API verified working (crater drops the sampled height by the edit depth,
clear restores it).

**What the scene covers vs the feature set:** all live page producers (reference / live mountain
/ bridge / region-fact / world / legacy) + the facts/collision subsystem + terrain edits. NOT
flyable (by architecture, not omission): individual non-mountain biomes — the live pool only
streams **mountain** (SingleBiome) and **all-11 via WORLD compose**; per-biome review is the
static-mesh `*_world_review.tscn` scenes. Material/AAA surfacing is Phase-6 (current = debug
height/slope palette).

## NEXT SESSION — the open items (in priority order)

1. **OWNER VISUAL A/B (the one thing blocking "look shipped"):** fly Step 4 of the review scene.
   Does the smooth-field-conditioned carved look match the accepted per-region look? Numeric
   gates are all green; this is the eye-check. If it reads right → the carved baked-look is
   shipped. If not → the tuning knobs are `SmoothFieldPercentiles { coarse_stride_m,
   window_radius_m, window_samples }` (currently span/16-ish stride, 33 samples).
2. **Super-region OUTER-border seam** (~5.9 m conditioned residual, measured): raise `k` (more
   internal borders, fewer outer) OR, long-term, the core-local-anchored carve once the SCALE
   CONTRACT is settled. The percentile layer is already 0-ULP there; only carve+gaussian residual.
3. **Worker GPU-context reuse:** the worker rebuilds RD+context per super-bake (correct +
   isolated). If super-bakes become frequent on the live path, cache the ctx/RD on the worker
   thread (the back-to-back gate already proves reuse is safe).
4. **Region-fact at production region_size_m:** the review scene tiles region facts at BASE_SPAN
   (8192 m) for a clean clipmap demo. The pack's real `region_size_m` is 32768 m; confirm the
   producer behaves at production region size + tune `k` for the bake-unit cost there.

## What is explicitly NOT a problem / NOT to do
- Do NOT clean the noisy worktree.
- Do NOT chase final terrain textures (Phase 6; bar is geometry + carved look + facts/collision).
- Do NOT force the core-local-anchored carve before the scale contract (it defeated ~10 prior
  iterations; super-region slice is the proven seam-exact model meanwhile).
- "Biome parity" is DONE (CPU 1e-9, GPU 1e-6, compose 1e-4) — not remaining work.

## First files to read next session
1. `docs/plans/STATUS.md` (top — the live truth)
2. This handoff
3. `wg-10/worldgen_terrain/harness/feature_review.gd` (the review scene — fly it)
4. `wg-10/rust/src/region_bake/` (mod.rs bake_super_region, percentile_provider.rs, worker.rs)
5. `wg-10/rust/src/page_pool/region_producer.rs` + `region_fact.rs` (the producer + cache)
6. Memories: `worldgen10-condition-seam-measured`, `worldgen10-standing-build-directives`,
   `worldgen10-carve-ported-to-rust`, `worldgen10-gpu-readback-bare-pool`.
