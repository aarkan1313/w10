# WorldGen10 — Scale-Invariant Biome Producer (world-blurs + near-field-only flow)

**Date:** 2026-06-03
**Milestone:** Fixes the LOD geomorph "ground shifts weirdly" warp the owner hit flying the live mountain
biome, AND attacks the per-page flow hitch. Both are pillar blockers (P3 quality: no warp; P2 perf: no
stall). Sibling to the mountain-live-fly work (commit `a381717`).
**Status:** design-ready; owner-approved direction (sections A/B/C approved 2026-06-03, all decided by the
four pillars). Owner review of THIS spec gates the plan.
**Parents:** the geomorph root-cause (memory `worldgen10-mountain-live-working` + the level-surface-diff
finding: a level-1 parent page differs from its level-0 children by peak 73% of relief); the 576 parity
residual (`worldgen10-576-parity-residual`, the discrete-flow near-tie lesson); the mountain-live-fly spec.

---

## 1. Problem

Each clipmap level bakes a DIFFERENT surface, not a coarse version of its children. Measured (renderer-
bypassed, two world-overlapping pages sampled at identical world XZ): level-1 vs level-0 `peak_abs_diff =
3971 m = 73% of the level-0 relief`. The LOD geomorph `mix(h_fine, h_coarse, t)` then blends two mismatched
surfaces, and the terrain visibly warps as `t` animates with camera motion ("ground shifts weirdly").

Two coupled causes, both confirmed in source:
1. **The recipe's whole-field operators work in GRID-CELL space, not world-metres.** Gaussian blurs use
   sigmas in CELLS (1.15, 5.0, 7.0, 1.8, 2.0, 1.2, valley_width_px=2.4, floor_smooth_px=4.0). At a 2×
   coarser level the same cell-sigma covers 2× the WORLD distance → massifs broaden, valleys widen/shift.
   The recipe never receives the page SPACING (m/px), so it cannot compensate. (This is the dominant
   contributor — the macro blur structure is what diverges most.)
2. **Flow accumulation is grid-discrete + fixed-iter.** 8-neighbour ±1-cell pull, fixed 192 iters, drops in
   cell units → the drainage network reaches DIFFERENT world positions on a 2× coarser grid, and (per the
   576 residual) discrete near-tie routing cannot bit-match across grids anyway.

The legacy kernel-atlas path masked this (tiled / level-scale-invariant); real mountain relief exposed it.
It affects ALL 11 biomes, not just mountain — every recipe shares these cell-space operators.

## 2. Goal & non-goals

**Goal:** a coarse page is a true LOW-FREQUENCY version of its children in WORLD space, so the geomorph
blends matching surfaces (no warp). Secondarily, make coarse pages cheap (attack the hitch).

**Non-goals:**
- NOT bit-exact cross-level flow (proven impossible — discrete near-tie routing, the 576-residual lesson).
- NOT changing the EXISTING 344/576 parity (it stays valid by construction — §5).
- NOT the other 10 biomes in this slice (mountain first; the others inherit the same world-anchoring pattern
  later, like the original port fan-out).
- NOT the drainage off-frame bake (the hitch's full answer; this slice's flow-off-at-coarse is a large down
  payment, the bake is a later slice if still needed).

## 3. Architecture

### 3.1 World-anchor every gaussian sigma (Section A)
Pass the page **spacing** (m/px) into the producer (it already has `world_span` + `page_px` at the pool, so
`spacing = world_span / (page_px - 1)`, the texel-corner denominator). Define each blur's intended WORLD
extent once via a fixed REFERENCE spacing `S_ref`: `sigma_world_m = sigma_cell × S_ref` (where `sigma_cell`
is the current hardcoded value). At bake time each level computes `sigma_cells = sigma_world_m / spacing`, so
the blur covers the SAME metres at every level. `S_ref` is a documented constant; its VALUE doesn't change
the cross-level invariance (it just sets the absolute world-extent of each blur) — but choosing `S_ref` =
the value that keeps the LOOK the owner already accepted (the captured 3500-feature-span mountain at the
near level's spacing) means the near level is unchanged. (The parity fixtures stay valid regardless of
`S_ref` because they reduce per-page — see §4.1.) The per-sigma
gaussian kernels (already built CPU-side per distinct sigma + uploaded) are rebuilt per-level from the
world-anchored cell sigmas. The flow pre-blur (1.15) and the discharge spreads (valley_width, floor_smooth)
world-anchor identically. Result: the macro structure (regional/ranges/massif/envelope/lowland) is identical
in world space across levels.

### 3.2 Near-field-only flow (Section B — the keystone)
Flow (the carved valleys) runs ONLY on the finest N levels — a tunable `flow_max_level` threshold (start:
flow on levels 0–1, off on 2–4). Coarse levels (≥ threshold) SKIP the flow passes entirely and bake just the
world-scaled blurred MACRO assembly (base + massif/envelope/lowland) WITHOUT the primary/tributary carve
subtraction.

The seam math: fine height = macro − carve; coarse height = macro. At the morph band
`mix(macro − carve, macro, t) = macro − (1−t)·carve` — the carve smoothly scales to zero crossing into
coarse. So the carved valleys become a NEAR-FIELD ADDITIVE DETAIL that FADES IN as you approach (which is
exactly what geomorph is for: detail appears, nothing warps), PROVIDED the macro parts match (§3.1 ensures
that). A and B are coupled: A makes macro match; B makes carve a pure near-field delta.

Three pillar wins: P3 (coarse = true low-pass → no warp), P2 (coarse pages skip the 192-iter flow → cheap →
attacks the hitch), P4 (drainage is genuinely a near-field feature — principled LOD).

### 3.3 Where the changes live
- `recipes.rs::mountain::generate_seamsafe` (+ `flow_channels_seam_safe`/`array_ops` callers): take a
  `spacing` (or scale factor) arg; world-anchor the sigmas; gate the carve behind a `flow_on: bool`.
- The GLSL machine (`biome_page.glsl`) + `biome_page_compute.rs` schedule: per-level kernel build from
  world sigmas; a flow-on/off branch in `schedule_mountain` (skip the flow + carve passes when off).
- `page_pool.rs`: pass per-level `spacing` + the `flow_on` decision (level vs `flow_max_level`) into
  `compute_biome_page_cached`. `configure_biome` gains `flow_max_level` (tunable).

## 4. Verification / parity bar

### 4.1 Parity is RE-ESTABLISHED Rust↔Python (the fixtures regenerate from the updated oracle)
HONEST CORRECTION (caught in spec self-review): a SINGLE reference spacing `S_ref` canNOT make both existing
fixtures byte-identical — the 344 fixture is at 3913 m/px, the 576 oracle at 351 m/px, so `sigma_cell × S_ref
/ spacing` equals the old `sigma_cell` for only ONE of them. The current recipe is spacing-AGNOSTIC (cell-
sigmas regardless of spacing); the world-anchored recipe is NOT — by design — so its output at a given
spacing differs from the old output UNLESS `spacing == S_ref`.

Therefore parity is re-established the way it always actually worked: **Rust world-anchored == Python world-
anchored.** The fixtures are GENERATED by the same Python recipe the Rust mirrors (`mountain_synthesis.py` /
the exporters). World-anchor the PYTHON recipe identically (one sigma-scaling change, shared definition),
REGENERATE the 344 fixture + the 576 oracle from it, and re-assert Rust↔Python parity to the SAME tight
bar (~1e-9..1e-12). This proves the Rust GPU/CPU path still matches its oracle — the recipe MATH is preserved
across the port — even though the absolute pixel values changed (because the recipe is now spacing-aware).
The 576 GPU gate likewise re-runs against the regenerated oracle (the f32 routing residual stays Tier-2).
The no-shortcut guard is the TIGHT Rust↔Python bar on the regenerated fixtures, NOT "identical to the old
bytes" (which would be wrong to demand — the recipe legitimately changed). At `S_ref` chosen so the near
level matches the accepted 3500-feature look, the LOOK the owner accepted is preserved at the near level.

### 4.2 NEW cross-level macro-agreement gate (did we fix the warp?)
Bake level L and level L+1 over the same world region (FLOW OFF on both, or comparing only the macro
component); assert their world-resampled heights agree within a small bar (the seed harness
`biome_level_surface_diff_check.gd` already measures the 73% — this gate asserts it drops to e.g. < few % of
relief). Can't false-pass: a regression to mismatched surfaces trips it.

### 4.3 NEW flow-off macro-only parity
The flow-off page is a new output mode. Gate it against a Python oracle running the recipe with the carve
terms zeroed — proving "flow-off" == "macro without carve", not some other divergence.

### 4.4 Owner visual gate
After build: the FINE-vs-COARSE morph capture shows coarse converging to a low-pass of fine; a real fly shows
valleys FADING IN cleanly (not warping). Owner judges. The hitch should also drop (coarse pages skip flow).

## 5. Boundary / honest risks
- **The flow-off transition level may be subtly visible** (carve full on the fine side, zero on the coarse
  side). Far better than warping, and tunable (`flow_max_level` + the morph band width). Watch it in the fly.
- **Re-validating parity** is real work — but it stays valid by construction (§4.1); the risk is a sigma-
  world-anchor arithmetic slip, which the unchanged 344/576 gates catch immediately.
- **Mountain-first.** The other 10 biomes still bake cell-sigma (will warp) until they inherit the pattern.
  Acceptable: the live fly is all-mountain. Note it so it's not forgotten.
- **The flow that DOES run (near levels) still won't bit-match across the near levels** (near-tie). But near
  levels have small morph bands + the carve detail is small relative to macro → residual warp tiny vs 73%.
  Measured by §4.2 extended to the near levels.

## 6. Out of scope / deferred
- The other 10 biomes' world-anchoring (later fan-out).
- The drainage off-frame bake (the hitch's full answer if flow-off-at-coarse isn't enough; later slice).
- Per-level seam coherence beyond the morph (same-level page seams are already apron-seam-safe).
