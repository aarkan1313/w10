# WorldGen10 — Terrain-Edit Framework Design

**Date:** 2026-06-01
**Milestone:** Phase 5. Generalizes the Tier-3 mountain-pass carve into a reusable, tunable terrain-edit
subsystem (owner directive: "make it tunable, it won't just be used for this" — mountains now; roads, POIs,
rivers, lakes later all edit terrain).
**Status:** design-ready; offline Python render-first; implementation gated by owner approval of THIS spec.
Build scope = framework core + mountain-trail config (owner choice), with road/river/lake config sketches to
prove the abstractions against >1 use. No Rust/GLSL port here (Slice 3+, owner-gated).
**Parents:** `docs/superpowers/specs/2026-05-31-worldgen-connected-corridor-router-design.md` (carve_ramp /
seam discipline this generalizes), `docs/plans/MOUNTAIN_BIOME_PROMOTION_2026-05-31.md` (the mountain terrain +
scale findings), the M4 Facts API edit-provider seam (`Wg10Facts.get_height = base + edit-provider.delta`),
`docs/plans/ROADMAP.md` Phase 5, memory `worldgen10-tier3-corridor-built-mountain-gap` (the full carve/routing
trace + all negative results).

---

## 1. Purpose

The Tier-3 mountain-pass work produced real machinery (seam-exact routing + carve) but it was a one-off recipe.
The owner's goal reframes it: WorldGen10 needs a **tunable, reusable terrain-EDIT subsystem** where "carve a
mountain pass" is ONE configured use, and the same bones later serve roads, POIs, rivers, lakes — everything
that authors/modifies terrain on top of the natural generated world. This spec defines that subsystem.

The immediate deliverable is the mountain pass/trail solved as the first config; the structural deliverable is
the framework it's a config of, with the routing strategy and carve profile as swappable, tunable components.

## 2. Non-Goals

- **No Rust/GLSL port.** Offline Python, render-first (all of Phase 5). The runtime/bake split is *designed*
  here, ported at Slice 3+.
- **No new world facts.** Edits READ the existing facts (channel_axis, discharge, regime, height); they do not
  mutate facts or become facts. (Whether a specific edit later also exposes a queryable fact is a per-type
  decision deferred to when a consumer needs it — §3.4.)
- **No full road/river/lake implementations.** Those get *config sketches* (instantiate the interfaces, emit a
  stub delta) to prove the abstractions; real implementations land when each is actually needed (YAGNI).
- **No global state / no whole-world pass.** Every edit is a seam-exact, world-local delta provider (the
  performance pillar; fits the M4 edit-provider seam). An edit that needs the whole world is rejected.
- **No magic numbers.** Every placement/profile parameter is named config (pillar 1: adaptable/tunable).
- **No silent gouging.** Where an edit cannot meet its goal within its bounds (e.g. a trail that can't both
  preserve a peak AND stay walkable), it REPORTS it (per-game opt-in), never silently bulldozes.

## 3. Architecture

A terrain edit is a **tunable `(Placement + Profile)` pair that emits a seam-exact, world-local height delta**,
composed into height at the M4 edit-provider seam (`get_height = base + Σ edit deltas`). Three layers, each one
responsibility, each independently testable.

### 3.1 Placement (WHERE) — geometry from facts
Deterministic geometry from world facts + the window. Produces a **route polyline** (path edits) or a **region
mask** (area edits), world-anchored and seam-safe. READS facts (channel_axis, discharge, regime, height), never
mutates them. Pluggable strategies (interface: `place(field, window, facts, params) -> geometry`):
- `low_corridor_route` — sparse least-cost route biased hard to low ground (mountain trails, passes). PROVEN.
- `contour_sweep` — traverse along contours, climbing gently (Fellowship sweeping look, few wide switchbacks).
- `spline` / `point` / `flow_trace` / `basin_fill` — roads / POIs / rivers / lakes (sketches now).

**Placement is the EXPENSIVE part** (least-cost routing, flow tracing) → baked off-frame (§4). The bake is a
pure optimization (deterministic → recompute = identical), never a correctness dependency. A placement cheap
enough to be pure `f(x,z)` (straight road, point) may skip the bake (§4).

### 3.2 Profile (WHAT) — geometry + terrain → delta
Turns geometry + local terrain into the height delta. Pluggable cross-section/surface profiles (interface:
`profile(field, geometry, params) -> raw_delta`):
- `thin_climbing_trail` — a thin, gently-climbing walkable ledge that PRESERVES the surrounding terrain (the
  mountain-trail profile; §5).
- `graded_valley` — the wider graded valley (= refactored `carve_ramp`; for cases that want a real valley).
- `flat_road` / `incised_channel` / `lake_surface` / `level_pad` — roads / rivers / lakes / POI pads (sketches).

**Profile is CHEAP** → procedural, sampled per-pixel at runtime in the height-compose (like carve_ramp today).

### 3.3 Apply (HOW) — seam-exact compositing (shared, once)
The proven discipline, owned in one place and reused by every edit type:
- **Edge blend:** smoothstep taper to the surrounding terrain (NO cliffs — fixes "straight drops").
- **Depth/extent bound:** cap the edit magnitude (NO peak-gouging — fixes "messes up high elevations").
- **Combine:** multiple edits composited deterministically (min for cuts, max for fills, or ordered).
- **Seam-exactness:** apron-local + deterministic so adjacent windows agree at the border (carve-the-big-field-
  then-slice for the mountain 9x9; gate-anchored discipline for independent-window streaming).

### 3.4 Edits read facts, stay separate (owner-chosen)
Facts describe the natural world; edits author on top, reading facts but living as their own delta provider on
base height. Keeps facts pure and edits composable (matches carve_ramp + the M4 stamp edits today). If a future
edit must be queryable by other systems (a road POIs snap to, a river materials react to), promoting THAT edit
to a fact is a per-type decision made when the consumer exists — not designed in now (YAGNI).

## 4. Offline / runtime split ("procedural is ideal; test and figure it out")

- **Procedural is the default wherever feasible.** Profile + Apply are pure, cheap `f(x,z)`-style sampling,
  recomputed per window in the height-compose — no bake, no cache to invalidate.
- **Placement bakes a compact edit-fact** (route polyline + profile params — kilobytes, not a height field),
  off-frame per world-anchored authority window, like the existing M3/7B cache. Deterministic ⇒ the bake is a
  pure optimization, never a correctness dependency. This holds the no-stall pillar for expensive routing.
- **Escape hatch toward fully-procedural:** the interface lets any placement cheap enough (straight road, point
  POI) SKIP the bake and be pure `f(x,z)`. We test per-placement which side it lands on — "procedural ideal"
  realized wherever the cost allows, bake only for genuinely-expensive routing.
- **Phase-5 discipline:** prove it all OFFLINE in Python (render/fly review) first; design the runtime
  sample/bake split here; port at Slice 3+ (like every WG10 subsystem).

## 5. Tunability (pillar 1, first-class)

Every edit is a config object: `placement_strategy` + a params dataclass, `profile` + a params dataclass — no
magic numbers. Swap strategy, swap profile, dial knobs. Named configs are the named uses:
- **Mountain trail** = `low_corridor_route` (sparse, valley-following; knobs `route_count`, `low_pref`,
  `valley_bias`) + `thin_climbing_trail` (knobs `floor_grade_frac`, `trail_width`, `blend_width`, `depth_cap`).
- **Road** (sketch) = `spline`/`low_corridor_route` + `flat_road`.
- **River** (sketch) = `flow_trace` + `incised_channel`.
- **Lake** (sketch) = `basin_fill` + `lake_surface`.
- **POI pad** (sketch) = `point` + `level_pad`.

The mountain-trail's hard tension (depth-cap vs walkable) becomes **tunable knobs the owner dials by eye**, not
a hidden recipe: the proven insight (carve to the route's re-graded RAW height — NOT a smoothed version, which
re-steepens it toward surrounding peaks; that was the repeated bug) gives 0%-over-budget along the trail; the
depth-cap-vs-preserve-peak tradeoff is exposed as `depth_cap`. Where no setting is both pristine AND walkable on
the steepest faces, the profile REPORTS it (per-game opt-in), never gouges silently (§2).

## 6. Verification

- **Interface contracts:** each placement returns deterministic world-anchored geometry; each profile returns a
  bounded `<=0`/`>=0` delta of the field shape; Apply returns a seam-exact composite. Unit-tested per piece.
- **Mountain config end-to-end:** sparse thin trails that (a) read as Fellowship-style mountain paths (owner
  eye), (b) are walkable along their length (0% over-budget on the carved trail), (c) PRESERVE the mountain (no
  gouged peaks, no cliffs — bounded `depth_cap`, blended edges), (d) seam-exact across chunks.
- **Abstraction holds vs >1 use:** the road/river/lake config sketches instantiate the same interfaces and emit
  a (stub) delta through Apply — proving the seams are real, not guessed.
- **Determinism + seam-exactness (hard gate):** same window+seed → same delta; adjacent-window border delta
  0.0 (carve-then-slice for the 9x9; gate-anchored for independent windows).
- **Tunability:** changing a named knob measurably changes the result; no magic numbers in the edit path.
- **Owner-eye gate (acceptance authority):** the mountain 9x9 chunk scene with trails carved in — flown +
  walked. Passed unit gates ≠ owner acceptance (DESIGN §7.3).

## 7. Slice plan

Offline Python; TDD; render-first.
1. **Apply (shared seam-exact compositor):** edge blend + depth bound + combine + seam test. The proven
   discipline, isolated first (everything depends on it).
2. **Placement interface + `low_corridor_route` + `contour_sweep`:** deterministic geometry from facts; bake
   shape (polyline edit-fact); determinism + apron tests.
3. **Profile interface + `thin_climbing_trail` (+ `graded_valley` from carve_ramp):** geometry+terrain→delta;
   the raw-height-not-smoothed fix; depth_cap/blend knobs; along-trail-walkable test.
4. **`TerrainEdit` config + `apply_edits` + mountain_pass_config:** the mountain trail as a config; end-to-end
   on the mountain field (sparse, thin, walkable, preserves the mountain).
5. **Road/river/lake/POI config sketches:** instantiate the interfaces with stub placement/profile; prove the
   abstraction emits a delta through Apply.
6. **Wire into the mountain 9x9 chunk scene** (carve-big-field-then-slice = seam-exact) + render/fly review.
7. **Owner eye.** Accept → the framework + mountain config stand; trails tunable. Reject/iterate → dial knobs.

## 8. Boundary / honest risk

- Design-direction until the owner accepts the flown mountain trails. Passed gates ≠ acceptance.
- **The depth-cap vs walkable tension is real** (proven across many iterations, memory
  `worldgen10-tier3-corridor-built-mountain-gap`): there may be no knob setting that is BOTH perfectly pristine
  AND fully walkable on the steepest faces. The framework's honest outcome is "owner picks the tradeoff per
  game via knobs," not a magic solve — and it reports where a trail can't satisfy both, rather than gouging.
- **Abstraction risk:** designing road/river/lake interfaces before those are built can guess wrong. Mitigation:
  build them only as thin sketches now (prove the seams), keep the interfaces minimal, expand when the real use
  arrives — don't over-fit the abstraction to the mountain case.
- **Scale coupling:** how DENSE trails should be (how far between passes) is genuinely part of the player-to-
  world scale contract the mountain promotion doc calls for — exposed here as a knob (`route_count`/spacing),
  with the real density policy deferred to that scale slice.
- Downstream unblocked on acceptance, not done here: the runtime sample/bake split, the Rust port, and the
  full road/river/lake/POI editors.
