# WorldGen10 — Slice 4: GPU Page Integration (biome composition on the render path)

**Date:** 2026-06-02
**Milestone:** Phase 5 Slice 4. Replaces the legacy kernel-tiling page formula with the accepted 11-biome
composition stack ON THE GPU, in the grid-shaped page path, and removes the 25 MB kernel atlas. The biggest
single integration milestone in the project — sequenced incrementally to de-risk it.
**Status:** design-ready; owner-approved architecture (this brainstorm). Implementation gated by owner approval
of THIS spec, then the plan.
**Parents:** `2026-06-01-slice3-rust-port-plan.md` (the CPU port this builds on — all 11 recipes + `compose_biomes`
+ `recipe_noise` + `array_ops` ported to Rust, parity machine-exact, the f64 oracle), the GPU-flow gate
(`flow_spike.rs`/`flow_accum_spike.glsl`, commit 4b392b6 — proved iterative-relaxation flow fits budget at 256²),
`2026-06-01-worldgen-biome-composition-layer-design.md` (Fork B), `2026-06-02-worldgen-scale-contract-design.md`,
memory `worldgen10-gpu-rust-first-principle`, `worldgen10-biome-composition-layer`.

---

## 1. Purpose

The CPU port (Slice 3) put the entire accepted biome height stack in Rust, parity-exact vs Python. But the LIVE
runtime terrain is still the legacy `sample_kernel` kernel-tiling formula (in `height_page.glsl` + the 25 MB atlas),
marked LEGACY/SCAFFOLDING in-source. Slice 4 makes the engine actually GENERATE the biome terrain: mirror the
accepted stack to GLSL, run it in the page path (which is grid-shaped — the natural home for the apron-grid
recipes, per the Slice-3 pillar decision that a per-point swap would run flow-accumulation per collision query),
parity-gate it against the CPU oracle, flip the runtime, and delete the atlas.

## 2. Non-Goals

- **No per-point `height.rs` swap.** The recipes are apron-GRID pipelines; the per-point facts path stays on the
  legacy formula until/unless a consumer needs fine per-point drainage (the coarse-cached-fact split, designed then —
  YAGNI). Slice 4 is the PAGE (render) path only. `visible==collision` is preserved by the parity gate, not by
  forcing facts through the grid path.
- **No new biome look tuning.** Port the accepted recipes faithfully; the display "scale look" open item is judged
  in-engine after the runtime is live (separate, owner-gated).
- **No bit-exact-vs-f64 claim.** GPU is f32 and the flow pass is an iterative approximation of the exact sweep —
  parity is the two-tier bar (§4), not 1e-15.

## 3. Architecture — three sub-slices (incremental de-risk)

The recipe-port template proved one biome before generalizing; Slice 4 does the same on the GPU.

### Slice 4a — prove the architecture on ONE biome (mountain), behind a flag
1. **MEASURE the real per-page cost FIRST** (the design's first build step — validate, don't assume; GPU/Rust-first
   measure-real principle). Extend the flow spike to the TRUE per-page dimensions: a page of `page_px` (256) needs an
   apron of `apron_px` (≈160 for mountain) → a `page_px + 2·apron` ≈ 576² working grid, the full 128-pass flow
   relaxation, AND the mountain recipe work (warp/ridge/fbm/gaussians). Measure real GPU time (wall-differential
   method — RD compute timestamps are unreliable on this box, per the flow-gate finding). This MEASUREMENT DECIDES
   the page pipeline (§3.1).
2. **Port the GLSL primitives:** mirror `recipe_noise` (hash2/value_noise/fbm/ridged_fbm/ridged_multifractal/
   domain_warp/recursive_domain_warp/cellular_edges/etc) + `array_ops` (gaussian_filter_nearest, the flow
   relaxation) to GLSL. f32-parity-gated against the f64 Rust oracle (§4).
3. **Port the mountain recipe to GLSL** + build the apron page pipeline (§3.1) per the measured decision.
4. **Two-tier parity gate (windowed):** GPU mountain page vs CPU `recipes::mountain_seamsafe` (§4).
5. **Wired behind a flag** — old atlas path stays the runtime default. Nothing flips.

### Slice 4b — generalize
The other 10 biome recipes + `compose_biomes` + the grammar biome-weight field, each GLSL-ported + parity-gated,
reusing 4a's primitives + pipeline. (Mirrors the CPU recipe-port fan-out.)

### Slice 4c — flip + clean
Make the new path the runtime default; **remove the 25 MB kernel atlas** (audit gate: no active shader samples
`KData`, no atlas buffer created on the new path, legacy code clearly marked + not called by render); re-run the
hardened GPU-time perf gate (p99 < 6 ms at ~1000 m/s with did-real-work assertions: streamed + nonblack + recipe
work contributed + no atlas path used); owner fly review (the live biome-composed terrain).

### 3.1 The page pipeline (decided by the 4a measurement)

> **▶ MEASURED 2026-06-02 (RTX 5090 Laptop / D3D12, windowed) → DECISION: `coarse-drainage-fact-fallback`.**
> At the REAL per-page apron dimension (256 core + 2×160 apron = **576²**) with 128 flow-relaxation iterations,
> the marginal flow cost is **flow_marginal_ms = 4.30 ms** (0.0336 ms/iter; wall_hi 4.61, wall_lo 0.57), which
> EXCEEDS the half-budget threshold of 3.00 ms. This confirms the spec §6 #1 risk: the 256² flow spike (~1.9 ms)
> understated the real per-page cost because the 576² apron grid is ~5× the pixels. So a per-page LIVE flow pass
> does NOT fit the frame budget → the FALLBACK (coarse-drainage-fact + fine per-page detail) is the indicated
> RUNTIME pipeline. (Gate: `python tools/gate.py --suite page_measure`, `[wg10-page-measure] ... PIPELINE=coarse-drainage-fact-fallback`.)
> NOTE: this does NOT affect the PARITY-PROVEN 4a mountain page — that pipeline ran the live flow at the small
> FIXTURE dims (344²) and matched the f64 oracle to **overall_maxd = 1.89e-6** (gate `biome_page` green). The
> recipe transcription + flow approximation are CORRECT; the open question 4b/4c must answer is how DRAINAGE is
> DELIVERED at the 576² production page (live-per-page is too slow → coarse cached drainage fact), not whether the
> recipe math is right. The coarse-fact bake/cache is now a REQUIRED 4b/4c design item, not optional. PRIMITIVE
> parity on real hardware: maxd 1.86e-4 (warp_x, within the 2e-4 budget) — the i64-emulated hash holds on D3D12.

- **DEFAULT (if 4a fits budget): per-page live pipeline** — each page dispatch is a mini-pipeline on an
  apron-padded buffer: generate biome fields + warp/ridge → run the N ping-pong flow-relaxation passes →
  `compose_biomes` → crop the core into the output R32F image. Procedural/infinite-pure, no cache. This is the
  GPU-first preferred path.
- **FALLBACK (if 4a is over budget): coarse-drainage-fact + fine per-page detail** — flow/drainage computed on a
  coarse world-grid off-frame (cached fact texture), pages sample it + add cheap local biome detail. Avoids
  per-page flow passes. Needs the coarse-fact bake/cache infra. The GPU-flow gate (256²) suggests live fits, but
  the real per-page is ~5× bigger — hence the measurement decides.
- **Adaptable:** per-page-live vs coarse-fact MAY be exposed as a config knob (different games/hardware); the
  measurement picks the default.

## 4. Parity bar (two-tier, pillar-decided)

GPU (f32) cannot bit-match the f64 oracle, and the flow relaxation approximates the exact sweep. So:
- **Tier 1 — EXACT structural decisions:** grammar/biome-weight selection, recipe dispatch, palette/region
  decisions (all flow-FREE integer/threshold logic) must match the CPU bit-for-bit (or to the existing M2
  integer-decision standard). Catches a wrong-biome/structural divergence that a metres tolerance would mask.
- **Tier 2 — composed HEIGHT within a documented f32 tolerance, relief-relative:** GPU vs CPU composed height
  agree within a small delta expressed in METRES relative to relief (start from the existing M2 gpu_parity ~1e-2 m
  budget; widen only with justification from observed f32 + flow-approximation error). The flow CONTRIBUTION falls
  under Tier 2 (it's the approximated part). This is the `visible==collision` contract.
- Committed parity fixtures: explicit world-coord samples, per recipe + composed, the same fixtures the CPU port
  uses, extended with GPU readback in a gate (readback ONLY in the gate, never the render hot path).

## 5. Verification / gates

- **4a measurement gate:** real per-page GPU time recorded; pipeline decision (live vs coarse) documented with the
  number. A `gpu_flow`-style suite entry, truthful (nonzero if over budget).
- **GLSL primitive parity:** GPU vs CPU primitives within f32 epsilon (documented).
- **Per-biome two-tier parity** (4a mountain, then 4b each): Tier-1 exact + Tier-2 metres tolerance, windowed gate.
- **compose parity** (4b): GPU composed page vs CPU `compose_biomes` within Tier-2.
- **Atlas-removal audit (4c):** grep/runtime gate — no active render shader samples `KData`, no atlas buffer
  created on the new path.
- **Hardened perf gate (4c):** real GPU-time p99 < 6 ms at ~1000 m/s, did-real-work assertions, no atlas path used.
- **visible==collision (4c):** facts/collision still agree with the rendered base within the accepted epsilon (the
  facts path is unchanged legacy until a later slice; the gate confirms no regression).
- **Owner fly (4c):** the live biome-composed terrain — acceptance authority (DESIGN §7.3).

## 6. Boundary / honest risk

- **Per-page flow cost is the #1 unknown** — the 256² spike fit, but the real per-page apron grid is ~5× the pixels
  + the recipe work. 4a measures it before the pipeline is committed; if it's over budget the coarse-fact fallback
  is the design's built-in answer (not a surprise).
- **GLSL port surface is large** (15 primitives + 11 recipes + compose + grammar). Mitigation: 4a proves the
  primitives + pipeline + one biome end-to-end before the 10-biome fan-out; each piece parity-gated.
- **f32 vs f64 + flow approximation** — handled by the two-tier bar; the risk is a Tier-2 tolerance hiding a real
  structural bug, which Tier-1 (exact decisions) guards against.
- **Atlas removal must not break M0–M4** — the legacy path stays flagged until 4c; the audit gate confirms removal
  is clean.
- The page shader's existing texel-CORNER seam convention + custom AABB + coarsest-hold-last-good (M3 lessons) must
  be preserved through the rewrite.

## 7. Out of scope / deferred

- Per-point facts drainage (coarse-cached-fact split) — when a consumer needs it.
- Display scale-look tuning — in-engine, post-flip, owner-gated.
- The Runevision local erosion DETAIL layer — Phase 6/7A (memory `worldgen10-runevision-erosion-candidate`).
