# WG10 — Seam-Exact Smooth-Percentile-Field Conditioning (sub-design)

**Date:** 2026-06-05
**Branch:** `slice4-gpu-page-integration`
**Status:** design (owner directives applied; pending spec review)
**Parent:** `2026-06-05-wg10-region-fact-producer-integration-design.md` (this resolves that
spec's "condition seam" component, now that G-seam measured the seam at ~1090 m).

## Problem (measured, not hypothetical)

The G-seam gate measured two ABUTTING 270 km regions: their RAW+carve fields match at the
shared border to ~2 m (seam-safe holds), but `condition_world`'s **per-region scalar
percentiles** differ (A p50 = −0.6, B p50 = −1.0) → the same border RAW conditions to a
**~1090 m height seam**. The whole-region scalar-percentile normalization is the sole
remaining seam source (macro + carve are already seam-exact).

## Owner directives this design obeys (standing, 2026-06-05)

1. **Pillars + long-term AAA quality** — seam-exact BY CONSTRUCTION, no blend/softening,
   no loss of local adaptivity.
2. **GPU/Rust-first** — pointwise/stencil percentile sampling runs on GPU on the live path;
   Rust CPU for the off-frame region bake. Python is look-proving scaffolding only.
3. **Modularity (WG10 is an ENGINE, not a game)** — conditioning takes a swappable
   percentile **provider** interface, not a hardcoded source.
4. **Parity-safe + owner-visual** — objective numeric gates in-CI now; deferred owner
   visual A/B before the look is declared shipped.

## Core idea

Replace the per-region **scalar** percentiles with a **smooth per-position percentile
field** `p05(x,z) / p50(x,z) / p95(x,z)` that is a continuous, deterministic function of
world position. Two abutting regions evaluate the SAME percentile values at their shared
border → **seam-exact by construction** — while the field still varies smoothly across the
world, so conditioning stays locally adaptive (the property that made the look good).

The conditioning robust step becomes elementwise:
```
robust[i] = (z[i] - p50_field[i]) / (p95_field[i] - p05_field[i] + 1e-9) * 2.10
shaped    = tanh(gaussian(robust, 0.55))
```
Mechanically a scalar→field swap; the design weight is the field SOURCE + the provider
interface + the gates.

## Component 1 — `PercentileProvider` interface (modularity)

`condition_world` becomes a pure transform parameterized by a provider that yields, for a
region's grid, the p05/p50/p95 **per cell** (or a single triple, as a degenerate field).

```
trait PercentileProvider {
    /// Percentile fields for a region grid: returns (p05, p50, p95), each either length 1
    /// (scalar, broadcast) or length n*n (per-cell field), evaluated at the region's world coords.
    fn percentiles(&self, region_world_x0, region_world_z0, span_m, n) -> PercentileFields;
}
```

Two implementations, interchangeable (engine consumers can supply their own):
- **`ScalarRegionPercentiles`** — today's behavior: percentiles over the region's own field
  (one triple, broadcast). KEPT for single-region bakes + the existing parity tests
  (so `bake_region`'s end-to-end Python gate stays bit-exact).
- **`SmoothFieldPercentiles`** — the seam-exact engine default (Component 2).

`condition_world_with_percentiles` (built in Task 1) generalizes to accept fields (scalar =
length-1 broadcast). The existing scalar tests pass a length-1 field → identical math.

## Component 2 — `SmoothFieldPercentiles` source

The field is computed from a **coarse evaluation of the same seam-safe macro** the regions
use, with percentiles over a **smooth spatial window**, sampled bilinearly per cell:

1. Evaluate the seam-safe macro on a COARSE world lattice covering the region + a window
   margin (coarse = low resolution; cheap — a fraction of the full macro cost). This is the
   same `mountain_seamsafe` / GPU macro, just at coarse spacing — so it carries the real
   terrain distribution, not a fabricated approximation.
2. For each coarse node, compute p05/p50/p95 over a fixed-world-size window centered on that
   node (the window spans ≥ 1 region so the stats are stable + locally representative).
   Because the window is a continuous function of world position and the coarse macro is
   continuous, the resulting coarse percentile nodes are continuous across region borders.
3. Bilinearly interpolate the coarse percentile nodes to the region's full-res grid → smooth
   per-cell `p05/p50/p95` fields. Adjacent regions share the coarse lattice nodes at their
   border → identical interpolated values there → **seam-exact**.

Why this is look-faithful (the coarse-drainage-refuted guard): where a region is internally
near-uniform in distribution, its windowed percentiles ≈ its own per-region percentiles, so
conditioning ≈ today's accepted result THERE; the field only *varies* where the world's
distribution genuinely varies — smoothly, never stepped. The numeric interior-match gate
(below) proves this.

**GPU/Rust split:** the coarse macro + windowed percentiles + bilinear sample are
pointwise/stencil → **GPU-appropriate on the live path**; for the off-frame region bake they
run in **Rust CPU** (cheap at coarse resolution). The coarse macro reuses the existing
GPU/Rust macro path (no new macro formula). Window percentiles are a Rust reduction (sort +
`percentile_linear`, already ported); a GPU histogram/selection is a later optimization, not
required for the off-frame bake.

## Data flow (one region bake)

```
region (x0,z0,span,n) →
  SmoothFieldPercentiles.percentiles():
    coarse macro over region+window (coarse lattice)         [reuse macro path]
    → per-node windowed p05/p50/p95                          [Rust reduction]
    → bilinear upsample to n*n                                [smooth field]
  → condition_world_with_percentiles(raw_carved, n, p05_field, p50_field, p95_field)
  → conditioned region (seam-exact at every border)
```

## Gates (parity-safe; numeric now, visual deferred)

1. **Seam-exactness (by construction):** re-run the G-seam fixture through
   `SmoothFieldPercentiles` conditioning for both regions; assert the shared-border
   conditioned-height delta is ~0 (≪ the 0.15 m budget). This is the gate the ~1090 m
   measurement demanded.
2. **Interior look-parity:** on a single real region, assert `SmoothFieldPercentiles`
   conditioning matches today's `ScalarRegionPercentiles` (per-region) conditioning in the
   region INTERIOR within a tight bar where the region's distribution is internally uniform
   (the smooth field ≈ the region's own percentiles there). Guards the coarse-stats-drift
   failure mode ([[worldgen10-coarse-drainage-refuted]]) objectively.
3. **Scalar-provider refactor safety:** `condition_world` via `ScalarRegionPercentiles`
   stays BIT-EXACT to the current `condition_world` (the existing Python-oracle + bake_region
   gates remain green) — proves the field generalization changed nothing on the scalar path.
4. **Deferred owner visual A/B (before "look shipped"):** render smooth-field-conditioned vs
   per-region-conditioned terrain for the owner's eye. Flagged, not blocking the integration.

## Modularity boundaries (engine, not game)

- `condition_world` = pure transform (field in → conditioned out). No knowledge of where
  percentiles come from.
- `PercentileProvider` = the seam strategy, swappable. A downstream game can supply a
  different provider (e.g. a gameplay-driven normalization) without touching conditioning.
- `SmoothFieldPercentiles` = one provider impl; its coarse-macro source is injected (reuses
  the engine's macro path), not duplicated.
- Each unit independently testable: provider fields (pure), conditioning (pure), seam
  (fixture), interior parity (fixture).

## Explicitly NOT in scope (YAGNI)

- No GPU histogram for window percentiles yet (Rust reduction is fine for the off-frame
  bake; GPU is a live-path optimization to revisit when the live conditioning path is built).
- No new macro formula — the coarse field is the SAME seam-safe macro at coarse spacing.
- No per-biome percentile logic (conditioning is world-layer, like the carve).

## Open risks to watch

- **Coarse spacing vs look fidelity:** too-coarse a lattice could under-resolve a real
  distribution gradient → interior-parity gate catches it; pick the coarsest spacing that
  passes gate 2.
- **Window size vs adaptivity:** too-large a window → global-constant-like flattening;
  too-small → noisy/over-adaptive. The interior-parity gate + the deferred visual bound this;
  start at window = 1 region and widen only if gate 2 fails.
