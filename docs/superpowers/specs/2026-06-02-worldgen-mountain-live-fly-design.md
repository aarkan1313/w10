# WorldGen10 — Mountain Live-Fly (first biome wired into the streaming runtime)

**Date:** 2026-06-02
**Milestone:** The first slice of 4c, scoped to ONE biome. Take the parity-proven mountain GPU recipe
out of its readback-only test harness and run it in the real `page_pool` streaming path on the global
RenderingDevice, behind a flag, so the owner can FLY it at speed and judge the surfaced/in-motion look
that no offline render can show.
**Status:** design-ready; owner-approved direction (sections A/B/C approved 2026-06-02). Owner review of
THIS spec gates the plan.
**Parents:** the Slice-4 audit (`worldgen10-slice4-audit-2026-06-02`) — which established that the biome
GPU path is currently a parity TEST HARNESS, not a runtime; the M3 streaming pipeline (`page_pool.rs`,
`page_compute.rs`); the production-convergence finding (`worldgen10-flow-convergence-production`).

---

## 1. Purpose & motivation

All 11 biome recipes + compose are parity-proven on the GPU — but on a throwaway LOCAL RenderingDevice,
recompiled per call, readback-only (`Wg10BiomePageCompute::run_inner`, `biome_page_compute.rs:1973`).
**Nothing is wired into the streaming runtime.** The legacy kernel-atlas producer (`page_compute.rs`) is
still the only thing `page_pool` runs. So the owner has not seen any biome terrain LIVE — only fixture
parity numbers and offline matplotlib renders.

Offline renders (this session: `render_mountain_review.py`) proved the mountain STRUCTURE is good enough
to build on (organized ridgelines, connected carved drainage, honest scale). But the remaining look
questions — surfacing, M3 shader detail, behavior under motion — are **in-engine properties** that only a
live fly can answer. This slice builds the minimum runtime integration to fly mountain.

## 2. Non-goals (deliberate cut-lines)

- **The other 10 biomes.** Mountain-only. The seam built here is general; the other 10 follow once mountain
  is proven live. (Pillar 1: prove the seam on one first.)
- **PART B (grammar / multi-biome per page).** The whole world is mountain. Per-pixel biome selection +
  compose is its own later slice. An all-mountain world is exactly right for judging the mountain look in
  motion without confounds.
- **Atlas removal.** The legacy kernel buffers stay allocated; the legacy producer stays constructible
  (flag off) for A/B + zero-risk rollback. Removing the 25 MB atlas is a later cleanup slice.
- **Drainage off-frame bake.** Mountain runs flow INLINE per page for now. The live fly is itself the
  measurement that decides whether the drainage bake is urgent (§5).
- **Compose real-recipe-regime parity gate** (audit gap #7). Compose is OUT of this slice (all-mountain, no
  blending), so that gate correctly moves to the PART B slice. Recorded here so it is not silently dropped.

## 3. Architecture

### 3.1 The seam to mirror
`page_pool` holds an `Option<PageComputeContext>` (`page_pool.rs:51`), built once by `configure()`
(`:170` via `build_page_compute_context`), and `acquire_page` produces each page by calling
`compute_page_cached(rd, ctx, …, target_rid, origin_x, origin_z, world_span, page_px, seed)`
(`page_compute.rs:169`) — which binds the page texture at image-binding 0 and dispatches on the GLOBAL rd
passed in, writing the core into the pool-owned R32F texture.

The biome path is the OPPOSITE shape (local rd, per-call compile, readback). The core work is to give the
biome producer the SAME shape as `PageComputeContext` + `compute_page_cached`.

### 3.2 New: `BiomePageComputeContext` (global RD, built once)
A context mirroring `PageComputeContext`:
- Compiled-once mountain shader (`recipe_primitives.glsl` + `biome_page.glsl` + `biome_mountain.glsl`,
  concatenated via `concat_glsl_hoist_version` — the proven path).
- Persistent apron field buffers + the per-sigma gaussian kernels (the same set `run_inner` builds today),
  allocated once, reused per page.
- Built on the GLOBAL rd (the pool's rd), NOT a local throwaway rd.

This is a REFACTOR of the existing `run_inner`: split "build the context (compile + allocate)" from
"dispatch one page". Today they are fused because each call is throwaway. The proven `schedule_mountain`
dispatch sequence is REUSED verbatim — the math does not change; only its host (cached global-RD context
vs per-call local rd) changes.

### 3.3 New: `compute_biome_page_cached(rd, ctx, target_rid, origin_x, origin_z, world_span, page_px, apron_px, feature_span_m, seed, flow_iters)`
Mirrors `compute_page_cached`'s role: dispatch the mountain schedule on the global rd, write the CORE into
`target_rid` at image-binding 0, using the IDENTICAL texel-CORNER pixel→world mapping the legacy producer
uses (`height_page.glsl:183-195` convention — texel 0 → origin, N-1 → origin+span, `denom = page_px-1`).
Seam-exactness of the clipmap depends on this mapping being byte-identical to the legacy one. `flow_iters`
defaults to the measured production convergence count — the value the `flow_converge` gate reports for
mountain at the 576² apron (the `page_flow_convergence_check.gd` CONVERGED_AT number), NOT the 344² fixture's
128.

### 3.4 `page_pool` flag
`configure` gains a `use_biome_path: bool`. When set, it builds a `BiomePageComputeContext` instead of the
kernel `PageComputeContext`, and `acquire_page` / the eviction-recompute path call
`compute_biome_page_cached` instead of `compute_page_cached`. Default the flag ON for the new fly scene;
the legacy producer stays constructible (flag OFF) for A/B + rollback. The pool's LRU / eviction / pinning
/ never-black machinery is UNCHANGED — only the producer swaps.

### 3.5 The fly scene
Reuse the existing M3 streaming scene; flip `use_biome_path` on. The owner flies an all-mountain world at
the ~1000 m/s target. Present A/B (legacy flag-off vs mountain flag-on) so the owner can compare.

## 4. Verification / parity bar

### 4.1 576² cross-oracle parity (audit gap #6 — FOLDED IN, gates this slice)
The existing biome parity is cross-oracle ONLY at the 344² fixture; the 576² convergence check is
SELF-convergence (no CPU oracle). This slice runs the producer at the REAL 576² apron / 256 core, so we
gate THAT against an independent Python f64 oracle generated at 256-core (the exact `flow_accumulation_mfd`
sweep, `array_ops.rs:172`, is the oracle). A scale-dependent math divergence that 344² could not catch
surfaces HERE — before the owner flies a possibly-wrong world. Deliverable: one 256-core mountain fixture
record (`export_recipe_mountain_fixture.py` extended, or a sibling exporter) + one parity assertion in the
windowed gate at `NORM_EPS = 1e-4` (the proven bar; record achieved maxd, tighten/justify per the
established discipline).

### 4.2 Live did-real-work perf gate (anti-fooling baked in)
A hardened gate (model on `m5_perf_hardened_check.gd`, memory `worldgen10-real-gpu-time` +
`worldgen10-profiling-must-be-real`) flies the biome-path scene and asserts ALL of:
- real GPU-time p99 (via `RenderingServer.viewport_get_measured_render_time_gpu`, NOT wall),
- **pages actually streamed under motion (count > 0)** — a green number with zero streamed pages is the
  exact false-pass to forbid,
- **frame non-black + terrain-vs-sky** (B3's discipline),
- **the biome path was actually used** (not a silent fallback to legacy) — this assertion is what proves
  the flip is real.

The p99 threshold is RECORDED, not asserted-pass blindly: inline mountain flow at 576² was measured ~6.45
ms, OVER the 3 ms half-budget. So this gate may legitimately report over-budget under fast motion — that is
DATA (§5), not a build failure. The gate's job is an honest number with did-real-work proof, like
`page_measure`.

### 4.3 No-regression
`facts_collision_parity_check.gd` and the `m3` moving-camera suite must stay green with the biome path live
(the facts path is unchanged; the clipmap seam convention must survive the producer swap — §3.3).

### 4.4 Look (owner-judged, no self-approval)
The owner flies the A/B scene and judges the surfaced/in-motion mountain look. This is the acceptance
authority (DESIGN §7.3). Claude does not self-approve the look.

## 5. The live fly IS a measurement (honest risk)

Inline 576² flow at ~6.45 ms/page is over the half-budget. Under fast motion with many fresh pages, the
live fly may STALL or drop p99. **This is expected and is the point:** the spikes proxied per-page cost;
the live fly measures the REAL streamed cost (with the pool's caching amortizing stationary/slow motion).
Outcomes + responses:
- **Tolerable** (caching + lead prefetch absorb it at realistic motion) → inline flow is fine for now;
  drainage-bake stays deferred.
- **Stalls** → we see exactly where, and THAT data drives the next slice (drainage off-frame bake per the
  drainage spec, or a cheaper caching/prefetch tweak). We do NOT pre-build the bake on a spike's say-so.

So this slice de-risks the drainage-priority question with real runtime data instead of assumption.

## 6. Boundary / what could break

- **Texel-corner mapping mismatch** → clipmap seams/cracks. Mitigated by reusing the legacy convention
  byte-identically (§3.3) and the m3 moving-camera gate (§4.3).
- **Global-RD resource lifecycle** — the biome context allocates persistent buffers/shader on the global
  rd; must `free` them on reconfigure/teardown (the B1 pool-RID-leak lesson). The context owns its RIDs and
  frees on drop/reconfigure, mirroring `free_page_compute_context`.
- **flow_iters at 576²** — must use the measured production convergence count (the `flow_converge` gate's
  mountain CONVERGED_AT at 576²), NOT the 128 the 344² fixture path uses, or live drainage under-converges.
  The cross-oracle gate (§4.1) at 576² catches under-convergence.
- **Windowed-only** — the cross-oracle gate + live fly need the editor closed + rebuilt dll. Claude builds
  + verifies via the isolated cargo target; windowed gates are owner-run or Claude-run with the editor
  closed. No windowed result is claimed unwatched.

## 7. Out of scope / deferred (tracked, not dropped)

- The other 10 biomes live (same seam, later).
- PART B grammar/multi-biome/compose + the compose real-recipe-regime parity gate (audit gap #7).
- 25 MB atlas removal (cleanup slice).
- Drainage off-frame bake (gated on §5's live measurement).
