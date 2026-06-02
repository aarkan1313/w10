# WorldGen10 — Player-to-World Scale Contract

**Date:** 2026-06-01
**Milestone:** Phase 5. Resolves the project-wide scale-contract gap the mountain-promotion doc flagged
(`docs/plans/MOUNTAIN_BIOME_PROMOTION_2026-05-31.md` §"Scale Contract Needed Project-Wide" / §"Player-To-World
Scale Policy"). Replaces eye-tuned multipliers with one coherent chain of REAL-METRE numbers anchored on the
on-foot player, so "is it tall enough?" becomes a measurable slope ratio, not a vibe.
**Status:** owner-accepted direction (this session). The contract is the numbers below; presentation/runtime
knobs are named + decoupled. Applies to the offline biome-composition stack now; ported with the rest at Slice 3.
**Parents:** `MOUNTAIN_BIOME_PROMOTION_2026-05-31.md` (the 7 tangled scales + vocabulary), memory
`worldgen10-scale-traversability-decoupling` (why fixed-relief-over-large-span reads flat),
`2026-06-01-worldgen-biome-composition-layer-design.md` (the biomes this scales).

---

## 1. Why this exists

Reviewing terrain from a fly camera at tens of km made "tall enough" perpetually ambiguous — every attempt to
fix it by multiplying relief/feature-span by eye failed to converge, because **content scale, display scale,
and viewing distance were tangled** (the mountain-promotion doc's 7 tangled scales). The fix is to anchor
everything on the on-foot player in real metres, so the look falls out of coherent numbers.

## 2. The anchored numbers (on-foot default, real metres)

WG10 is a framework supporting multiple player modes (on-foot / vehicle / top-down) via these knobs; the
DEFAULT we tune against is **on-foot human** (most demanding; the others rescale down).

| Knob | Value | Meaning |
|---|---|---|
| `player_meter` | 1.0 m (≈1.8 m character) | the anchor — everything is relative to this |
| `feature_span` (per biome) | mountain ~3.5 km … wetland ~9 km | base width of a biome's signature feature |
| `relief_meter` (per biome) | mountain ~1000 m … wetland ~110 m | peak height of a biome's signature feature, REAL metres |
| `region_span` | ~30 km | how far you travel through one biome (~8 features → a real range) |
| `world_span` | N regions × region_span | total world |

**Per-biome feature_span / relief_meter (the validated table):**
mountain 3.5km/1000m (slope ~0.29) · volcanic 4.0/850 · glacial 5.0/700 · karst 3.0/550 · rainforest 5.0/450 ·
temperate 6.0/380 · desert 5.0/300 · tundra 7.0/280 · grassland 8.0/220 · coast 6.0/200 · wetland 9.0/110.

**Key ratios (what was tangled, now fixed):**
- **slope ratio = relief_meter / feature_span** — "dramatic" is now a NUMBER. Mountain ~0.29; lowlands ~0.01–0.06.
- **region : feature ≈ 8 : 1** — a biome holds ~8 of its features → reads as a real land, not one bump.
- **feature : player ≈ thousands : 1** — features are km, player is metres.

## 3. The honest scale truth (resolved this session)

Rendering at TRUE proportions (real metres, no fake vertical exaggeration), from an on-foot oblique camera in a
valley, showed: **real mountains at slope ~0.29 are BROAD SWELLS, not towering walls — and that is correct.**
Real mountains are not walls; the "towering" felt in games comes from EITHER much steeper local slopes (cliffs,
which are local features, not whole-mountain) OR fine-scale steep faces/crags ON the broad form. The current
smooth ridged-noise generator produces the broad form correctly.

**Owner decision:** accept broad-at-true-scale as realistic/correct. Do NOT fudge slope to fake "towering."
The dramatic up-close read is a FUTURE detail-layer concern (local cliffs/crags — Phase 6 surfacing / a detail
pass), not a scale-contract number. The persistent "not tall enough" feeling was an overview-vs-on-foot illusion:
1000 m mountains genuinely look like small bumps from 120 km up because they ARE, at that distance.

## 4. Decoupled scales (the rule that prevents the past confusion)

The 7 tangled scales become independent named knobs. **The rule: content scale is authoritative and in real
metres; presentation scales are explicit, display-only, and NEVER feed back into content.**

- **Content (authoritative, real metres):** feature_span, relief_meter, region_span, world_span (§2).
- **Presentation (display-only, never corrupts content):**
  - `review_scale` — display multiplier to fit a world into a review scene. THIS was the silent corrupter (the
    old `rough_world_review.gd` squished any grid into a fixed box → "more chunks looked denser not bigger").
    Every render/scene must state its review_scale so "looks gentle" is never confused with "is gentle."
  - `runtime_page_span` / page-px / LOD — the clipmap streaming scale (M3); samples the content field, independent.
  - camera / frustum / fog / speed — viewing knobs; NEVER used to tune terrain shape (don't fix a mountain by
    changing camera speed).
  - collision / physics — derived from `player_meter`; gated by visible==collision parity.

## 5. How it plugs into the biomes

Each biome's seam-safe `generate(wx, wz, seed, feature_span_m=...)` already takes feature_span; the contract just
supplies the per-biome value from §2, and the composed height is scaled to real metres (peak-to-peak ≈ relief_m).
The biome-composition layer (`compose_biomes`) blends real-metre fields. No generator change needed — the
contract is a set of numbers the caller supplies + the decoupling rule.

## 6. Verification

- Slope ratio per biome matches §2 (measurable, not eyeballed).
- A render states its review_scale + vert_exag; a TRUE-scale (vert_exag=1.0, real dx/dz) render shows honest slopes.
- On-foot oblique render shows features at human-relatable proportions (the 1.8 m reference).
- Owner eye: accepts the world reads coherent at its real scale (NOT "towering" from overview — that's understood
  as an illusion).

## 7. Boundary / what's deferred

- **"Towering" up-close drama = a future detail layer** (local cliffs/crags), Phase 6 surfacing or a detail pass.
  NOT a scale number. Explicitly out of scope here.
- Vehicle / top-down mode rescalings: the knobs support them; tuning those modes is per-game/per-mode, later.
- Runtime/Rust application of the contract: Slice 3+ (the port supplies these numbers to `height.rs`/GLSL).
- The numbers in §2 are setup-grade defaults (a starting coherent set); per-game tuning expected, but now from a
  coherent baseline instead of from scratch.
