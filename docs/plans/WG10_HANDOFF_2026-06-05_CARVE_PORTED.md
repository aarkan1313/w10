# WG10 Handoff — 2026-06-05 (carve ported, biome parity confirmed, bake_region assembled)

Single pickup point for the next session. Supersedes
`WG10_FINAL_HANDOFF_2026-06-05.md` (which predates this session's work).
Live truth source remains `docs/plans/STATUS.md` (top).

## TL;DR

This session closed the project's core divergence and the parity goal, and proved
the whole offline "baked look" pipeline in Rust. The ONE thing left to put the
carved look on screen — the producer integration — hit a hard measurement that
forces a GPU-macro design (see "The decisive measurement" below). Next session
builds that.

Branch `slice4-gpu-page-integration`, everything committed + pushed to origin
(`github.com/aarkan1313/w10`). cargo lib **251/251** green. The worktree has ~245
preexisting dirty files (an old backlog) — DO NOT clean/reset/broad-checkout them;
all this session's work was committed via scoped `git add` of only its own files.

## What this session accomplished (all parity-verified, pushed)

1. **Named + fixed THE divergence.** The connected pass-network CARVE — the feature
   that made the accepted "mountain chunk network" look — had ALWAYS lived only in
   ~4-second offline pure-Python and was never on the live path (the live seam-safe
   recipe dropped carving → "GPU but carveless"). That was why it "stopped looking
   good after the chunk networks."
2. **Carve fully ported to Rust** (`wg-10/rust/src/pass_network/`):
   - routing (Dijkstra): bit-exact vs Python, tamper-tested, ~19 ms (206× faster).
   - carve_ramp + EDT (`edt.rs`, brute-force-verified): now **fully bit-exact**.
3. **condition_world ported** (`wg-10/rust/src/condition_world.rs`): percentiles +
   interior bit-exact (residual only on the reflect-border ring).
4. **All 11 biomes confirmed at full parity with mountains, ON HARDWARE:** CPU recipe
   1e-9 (12/12), **GPU/GLSL 1e-6..1e-5 on RTX 5090** (`biome_page` suite ran green
   all 11), compose 1e-4. "Biome-to-biome parity" is DONE. The carve is
   mountain-only/world-layer — there is NO per-biome carve work.
5. **bake_region assembled** (`wg-10/rust/src/bake_region.rs`): the whole offline
   look-pipeline in Rust end-to-end — seam-safe macro → carve (on RAW) → condition.
   End-to-end parity vs the Python seam-safe oracle: RAW/carve_delta/condition-stats
   **bit-exact**, final height p99 0.09 m (condition border ring). The assembly gate
   also caught + fixed a latent carve_ramp gaussian-mode bug (nearest vs scipy
   reflect) → added `array_ops::gaussian_filter_reflect`.

Specs/plans (all committed):
- `docs/superpowers/specs/2026-06-04-wg10-connected-carve-to-live-path-design.md`
- `docs/superpowers/plans/2026-06-04-wg10-connected-carve-rust-port-plan.md`
- `docs/superpowers/plans/2026-06-05-wg10-carve-ramp-rust-port-plan.md`
- `docs/superpowers/specs/2026-06-05-wg10-bake-region-assembly-design.md`
- `docs/superpowers/plans/2026-06-05-wg10-bake-region-assembly-plan.md`
- (the un-intercept ladder spec/plan that started the arc: `...2026-06-04-wg10-unintercept-proving-ladder-*`)

## The decisive measurement (THE thing to design around next session)

Goal next: wire `bake_region` into the live producer so the carved look reaches the
screen (and closes the un-intercept ladder's Rung-1 gap — the live recipe read ~2×
the reference relief precisely because it lacked carve + condition_world, both now
ported).

The simplest design was "all-CPU bake_region, off-frame, cache on the page-pool LRU,
pages sample it (mirror `StaticHeightRuntime`)." **MEASURED and it does NOT fit:**

- all-CPU `bake_region` over a `region_size_m=32768` region (≈16 pages):
  **~961 ms at 513px, ~3319 ms at 1025px.**
- The **CPU macro (`mountain_seamsafe`) dominates** (it's the GPU recipe's CPU twin,
  run over the whole region+apron grid); carve is only ~19 ms, condition ~2 ms.
- Seconds-per-region is too slow for synchronous AND for a background bake (3 s of
  blank/coarse terrain crossing a region is unacceptable).

**This forces the architecture (and matches the standing GPU/Rust-first principle):**
the macro must run on the **GPU** (it already does, fast, per-page), and only the
carve (the genuinely CPU-bound graph work) runs CPU. So the integration shape is:

> **GPU macro (region) → ONE off-frame readback → CPU carve (~19 ms) + condition
> (~2 ms) → RegionFactRuntime (mirror of StaticHeightRuntime) → pages sample it.**

The `bake_collision_region` (facts_api.rs) off-frame-readback pattern is the model
for the GPU→CPU step. This likely drops a region bake from ~3 s to tens of ms.

## OWNER PRINCIPLE reaffirmed this session

**Anything still CPU-bound that can move to GPU or Rust appropriately MUST.** The
~3 s all-CPU bake is the live example: the macro is GPU-appropriate (pointwise/stencil,
already on GPU per-page) and was the bottleneck only because bake_region ran it on CPU.
Carve stays CPU (graph pathfinding is GPU-hostile, and it's already fast in Rust). When
scoping the next build, audit each step for GPU/Rust-appropriateness, not just "does it
work." (See memory `worldgen10-gpu-rust-first-principle` + `worldgen10-cpu-bound-audit`.)

## Next session — concrete first steps

1. **Confirm the bottleneck split** (cheap, do first): time bake_region's macro vs
   carve vs condition separately at region scale, to be 100% sure the macro is the
   ~3 s (the shape says so; verify before building the GPU path). If carve were the
   cost, GPU-macro wouldn't help — but it almost certainly is the macro.
2. **Build a GPU region-macro → CPU-readback path.** Either reuse
   `compute_biome_page_cached` over a region-sized texture (macro passes only, flow as
   appropriate) + `texture_get_data`, or a `Wg10GpuCompute.heights(xs,zs)`-style batch
   over the region grid. Off-frame only (the WG9 hot-path-readback rule).
3. **`RegionFactRuntime`** — near-copy of `StaticHeightRuntime` (hold the
   conditioned+carved region `Vec<f32>` + bounds; `sample()` bilinear;
   `write_page_texture()` per-page). Its grid comes from: GPU-macro readback → CPU
   `carve_routes`→`carve_ramp_delta`→`raw+delta`→`condition_world`.
4. **Producer dispatch arm + region cache.** Add a `ProducerKind`/path: if the page's
   region (`grammar::region_of`, region_size_m=32768) is baked+cached →
   `region_fact.write_page_texture(...)`; else trigger the off-frame bake (and show a
   coarse fallback meanwhile). The generic `PagePolicy` LRU is reusable keyed by region.
5. **Off-frame triggering.** There is NO async mechanism today (everything synchronous).
   Decide: explicit-trigger (GDScript/load-time, the bake_collision_region model) vs a
   background thread (note: RenderingDevice is per-thread — Godot thread-safety care).
   Start with the simplest that doesn't stall the hot frame.

## KNOWN BOUNDARY to validate when baking MULTIPLE regions

**Cross-region condition seam.** `condition_world` normalizes by percentiles computed
over THE REGION. Two adjacent baked regions have different percentile sets → their
shared border conditions slightly differently → a possible seam in the conditioned
height. The macro + carve are seam-exact; ONLY condition normalization varies by region.
Single-region bake_region doesn't expose it; the producer (multi-region) will. Options:
large regions (borders rare), overlap/blend at region seams, or a shared/quantized
percentile fact. Flagged in the bake_region spec; do not let it surprise the integration.

## What is explicitly NOT a problem / NOT to do

- Do NOT port the full-field `mountain.generate` branch (per-window zscore/norm01). The
  live runtime uses the seam-safe branch; bake_region targets seam-safe. The accepted
  JSON artifact uses full-field but the live path won't.
- Do NOT chase final terrain textures (still out of scope; the bar is geometry + the
  carved look + facts/collision).
- Do NOT treat "biome parity" as remaining work — it's done (CPU+GPU+compose, verified).
- Do NOT clean the noisy worktree.

## Pickup commands

From `D:\workflows\worldgen10` (PowerShell):
```powershell
git status --short    # ~245 preexisting dirty files — leave them
# Rust (isolated target, no editor needed):
$env:CARGO_TARGET_DIR='D:/tmp/wg10_check_target'; Push-Location 'D:\workflows\worldgen10\wg-10\rust'; cargo test -p wg10_terrain --lib; Pop-Location   # expect 251 green
# Confirm the carve/bake parity numbers:
#   pass_network::tests::routes_match_python_fixture        (routes bit-exact)
#   pass_network::tests::carve_ramp_matches_python...       (now bit-exact)
#   condition_world_tests::...                              (interior bit-exact)
#   bake_region_tests::bake_region_matches_python_seamsafe_pipeline  (RAW/carve/stats bit-exact, height p99 0.09m)
# All-11 biome GPU parity (windowed, RTX 5090, editor closed):
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'; python tools\gate.py --suite biome_page
```

## First files to read next session

1. `docs/plans/STATUS.md` (top — the live truth)
2. This handoff
3. `wg-10/rust/src/bake_region.rs` (the assembled pipeline to wire)
4. `wg-10/rust/src/page_pool/static_reference.rs` + `static_reference/sampling.rs` (the
   RegionFactRuntime template)
5. `wg-10/rust/src/page_pool/producer.rs` (the dispatch seam to add the region arm)
6. `wg-10/rust/src/facts_api.rs` `bake_collision_region` (the off-frame readback model)
7. `wg-10/rust/src/gpu_compute.rs` (`heights` batch readback — candidate GPU-macro path)
