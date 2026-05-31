# WorldGen10 — Tier-3 Guaranteed Regime-Aware Traversability Design

**Date:** 2026-05-31
**Milestone:** Phase 5 / Slice 2A-close → Slice 3 unblock (the keeper-fork follow-on: traversability is the real quality bar).
**Status:** design-ready; offline Python render-first; implementation gated by owner approval of THIS spec, then owner-eye acceptance of the corridors-on sheet. No Rust/GLSL port here.
**Parents:** `docs/plans/ROADMAP.md` Phase 5 Slice 3 + Phase 7B; `docs/plans/STATUS.md` "fork-resolution session update";
`docs/superpowers/specs/2026-05-31-worldgen-rough-highlands-keeper-v2-design.md` (the v2 substrate this layers on);
`docs/superpowers/specs/2026-05-31-worldgen-phase7b-drainage-skeleton-design.md` (the routed-skeleton subsystem this extends);
`docs/plans/LOOSE_ENDS_LEDGER.md` B8; memory `worldgen10-tier3-guaranteed-traversability`, `worldgen10-keeper-formula-fork`.

---

## 1. Purpose

Resolve the real quality bar that came out of the keeper-fork session. Owner reviewed A | B | v2 and the Tier-1
traversability report (`report_abv_traversability.py`): **A is too spiky for a play area** (no crossing
corridor at any scale, slope_p90 1.42), B is flatter but also has no crossing corridor, and **v2 is the only
variant that ever reaches a "candidate" WENS-crossing low corridor at play scales** — and even v2 only sometimes.
Measuring traversability (Tier-1) is done. It exposed that none of the three keepers *guarantees* you can get
through a mountain region; they only sometimes happen to leave a gap.

Tier-3 makes traversability a **guarantee**, not an accident, while staying regime-aware: passable terrain
(desert / plain / gentle foothill) is left completely untouched; **barrier terrain (high range_core / badlands
regime weight) gets a guaranteed route through or around it**; "totally unclimbable, no way through" becomes
**rare and a per-game opt-in**, not the default.

This is the precondition that unblocks Slice 3. The fork is "three kept variants + v2 is the traversability
front-runner"; the port target stays unfrozen until an owner-accepted **final stack** exists, and "final stack"
now means "a keeper variant **plus** the Tier-3 corridor layer, owner-accepted by eye." Tier-3 is that layer.

This is on-roadmap. Phase 5's thesis is "prove an owner-accepted offline geography read, then port only the
owner-accepted stack." Tier-3 is the connectivity guarantee that the existing Phase 7B `corridor_mask` is
explicitly *not* (it is decorative — see §3). It is offline Python, render-first, like all of Phase 5 / 7B.

### 1.1 Measured reality (drives the barrier definition — verified 2026-05-31)

A barrier is **not** a fixed regime-weight threshold, and "barrier" depends on **scale and relief**, not just
the region. Measured on the real stack (`keeper_v2` on `geography_skeleton_windows` facts):

- The v2 softmax regime weights are normalized across 6 regimes, so `range_core + badlands` together **never
  exceed ~0.32** in any of 54 scanned seed/origin windows (max 0.323, seed 1000). A fixed
  `barrier_weight_threshold` of ~0.45–0.5 (what reads intuitively as "a wall") would detect **zero barriers
  everywhere** — the guarantee gate would pass vacuously. Regime weight is therefore a *bias hint* (which
  regime a route prefers), **not** the barrier detector.
- At the **25.6 km play span with the default 260 m relief**, the composed v2 height has **no slope-impassable
  terrain at all** (max slope ≈ 0.21 < the 0.28 passable band); the window is fully crossable WE+NS. What
  Tier-1 (`report_abv_traversability.py`) flagged as "no crossing route" is the **low/valley corridor**
  (`low_corridor = passable & h ≤ median`) failing to connect, not impassable mountains.
- Real slope-walls **do** appear at other relief/scale settings (a high-relief config — `post_tanh_gain 2.4,
  relief_amplitude 3.2` — produces ~11% slope-impassable terrain in the same window), and will appear in other
  world regions. The framework is infinite and the relief/span are **tunable** (owner: "we can adjust height
  and stuff"), so the guarantee must hold across all of them.

**Consequence:** the barrier is **measured from the composed height itself, scale-and-relief-aware** — it is
whatever actually severs a crossing on the surface the player walks: (a) a connected slope-impassable region
that blocks an edge-to-edge route, and/or (b) a broken low/valley corridor (the Tier-1 metric). Where the
terrain is already crossable at the active relief/scale, Tier-3 **no-ops** (verify-first). This single window is
one sample of an infinite world; correctness is proven across multiple seeds/regions **and** across low- and
high-relief configs (§6), never assumed from one place.

### 1.2 BUILD FINDING — seam-exactness vs connectivity is a hard tension (proven 2026-05-31; §8 risk realized)

Implementation surfaced the exact risk §8 named, and proved it with data. **Barrier detection (§4.1) and the
guarantee check work and are seam-safe.** The **carve mechanism (§4.2–4.3) as specified does not hold**, for a
fundamental reason:

- **A globally-routed least-cost-path carve CANNOT be seam-exact.** Adjacent windows compute different global
  paths (measured: seed 42 / 4 km, window A path = 3008 cells, window B = 2712), so the carve delta disagrees
  at the shared border (delta **0.62**; the hard gate requires **0.0**). The keeper substrate stays bit-exact
  (border 0.0) — the break is entirely the window-dependent global path. Verify-then-carve along a global path
  is irreducibly non-seam-safe.
- **Purely-local seam-exact operators don't guarantee connectivity.** A local apron-radius valley-deepening
  operator IS seam-exact (measured border ~4e-16) but does **not** reconnect the broken crossing (`needs_route`
  stays True) and over-carves (84–92% of the window — violates "passable untouched"). A channel-anchored carve
  is also seam-exact (border 0.0) but does **not** resolve the barrier either (the keeper's `channel_axis` is
  the *decorative* mask the 7B spec warns is "not proven-connected"; it does not cross edge-to-edge here).
- **Root cause:** seam-exactness demands **locality** (carve = f(world position, apron-radius data)); a
  connectivity *guarantee* is inherently **global** (a connected edge-to-edge route depends on the whole
  window). They pull opposite ways. No purely-local operator can guarantee a connected route; the only thing
  that guaranteed connectivity broke seams.

**Reconciliation (the real dependency):** the connected structure the carve follows must be **precomputed
deterministically per world-anchored window and stitched across seams** — i.e. a *proven-connected*,
cross-seam-joined corridor that is itself a seam-exact world fact. That is precisely the **unbuilt connectivity
half of Phase 7B** (the routed drainage skeleton with cross-seam join), which this spec said Tier-3 "builds on."
The existing `channel_axis` is the decorative half; the guaranteed-connected half does not exist yet. **So the
Tier-3 carve is blocked on a seam-stitched connected-corridor fact**; detection + guarantee-check + verify-first
no-op are done and seam-safe.

**Chosen direction + de-risk (owner "I trust you" → pillars choose option (i); de-risked before committing):**
build the 7B connected seam-joined corridor, then carve along it locally. The de-risk retired the scariest
unknown: **cross-seam JOIN already works** — `geography_skeleton_windows.adjacent_corridor_continuity` reports
`corridor_match_frac = 1.00` on all barrier fixtures (corridors entering a seam continue in the neighbor,
seam-exact), and `geography_skeleton._flow_accumulation_mfd` already produces connected, seam-joined drainage.
What is missing is only **edge-spanning**: the principal discharge network does not cross a window edge-to-edge
(largest connected high-discharge component ~2.5%, crosses neither WE nor NS — drainage exits to local/side
outlets). So option (i) is tractable, not a from-scratch subsystem: **extend/link the proven seam-joined
drainage segments into an EDGE-SPANNING connected corridor (reuse the flow + join primitives), then carve along
it locally (seam-exact).** Rejected alternatives stay on record: (ii) channel-where-available (seam-exact but
incomplete — fails the *full* guarantee pillar); (iii) Tier-2 param-bias (a softening, not a guarantee). Full
trace + measured numbers: memory `worldgen10-tier3-seam-exact-carve`. **Next concrete step:** spec the
connected-corridor *routing* (edge-spanning + seam-join requirement, deterministic, world-anchored) → build
offline → then the local seam-exact carve along it.

## 2. Non-Goals

- **No Rust/GLSL port.** Tier-3 is offline Python only. Slice 3 stays blocked until the owner accepts the
  keeper+corridor stack by eye. The runtime `traverse_corridor` fact shape is *designed* here (so the offline
  prototype does not paint itself into a non-portable corner), not implemented.
- **No provable global A-to-B traversal.** The guarantee is per-deterministic-window and joins across seams
  (effectively-infinite traversal). It is NOT a proof that any two arbitrary world points connect. Claiming
  global connectivity from bounded windows would be the same over-promise §2 of the 7B spec forbids.
- **No "always carve."** Carving terrain everywhere to force flatness was rejected in brainstorming as
  dishonest and visually disturbing (it would flatten the very ruggedness the owner liked). Tier-3 carves
  **only when the natural terrain has no under-budget route**, and then **minimally** (§4).
- **No drainage-only routing.** "Just use the channels as the route" was rejected as incomplete: channels do
  not always cross a range, and not every barrier has a channel through it. Channels *bias* the route (§4.2),
  they do not define it.
- **No water / hydrology realism.** Discharge/tributary connectivity is the other (drainage) half of Phase 7B
  and is out of scope. Tier-3 consumes the existing channel facts as a routing *hint* only.
- **No deleting A / B / v2.** All three keepers stay selectable variants (pillar 1). Tier-3 is a layer over
  whichever variant is active, not a fourth keeper.
- **No fixture re-freeze or doc "accepted" flip** until the owner accepts the corridors-on sheet by eye.
- **No magic numbers.** Every threshold / budget / width / bias gain is named config (pillar 1).

## 3. Substrate and prior art (reused, unchanged — the seam guarantee)

Tier-3 composes on the **existing** `geography_skeleton_windows.build_skeleton_window` facts and the keeper_v2
composition, both of which are already seam-exact:

- **Window model** (`geography_skeleton_windows.SkeletonWindowSpec`): world-anchored core span + apron, fixed
  `spacing_m`, no per-window normalization. `_core_slice` / `core_facts` crop the apron-padded window to the
  authoritative core. `adjacent_seam_deltas` already verifies border bit-identity (max delta 0.0 in B/v2).
- **Composed height** (`keeper_v2.compose_windowed_height_v2`): the surface the **barrier is measured from**
  (§4.1), the surface a route's slope is measured against, and the surface a carve modifies. Same conditioned
  grid `report_abv_traversability.py` already audits. This — not regime weight — is the barrier signal (§1.1).
- **Regime weights** (`keeper_v2._regime_weights`): the softmax `basin / fan / foothill / plateau / range_core
  / badlands` weights. These are a **route-bias hint only** (a route prefers a foothill/plateau saddle over a
  range_core spine, §4.2), NOT the barrier detector — §1.1 verified `range_core + badlands` never exceeds ~0.32,
  so a weight threshold would detect nothing.
- **Connectivity idioms** (`analyze_rough_world_traversability.component_stats`): 4-neighbour connected
  components with edge-touch flags (west/east/north/south). Tier-3 reuses this exact idiom to detect barrier
  *components* and to gate the guarantee (a component "has a route" iff a passable path reaches its required
  edges).
- **Decorative corridor** (`geography_skeleton_windows.corridor_mask` + `adjacent_corridor_continuity`): the
  current channel-derived mask. It marks where channels *are*; it does **not** guarantee a connected
  under-slope route through a range, and the 7B spec calls it a "seam/review heuristic, not a final gameplay
  route map." Tier-3 is the connectivity guarantee it lacks. The continuity *measurement* idiom
  (`_edge_match_count`, edge bands, row tolerance) is reused for the cross-seam join gate (§4.3).

Tier-3 does **not** modify the substrate facts or the regime weights. It adds one new world-anchored fact and
one bounded height delta. That is what keeps seams exact.

## 4. Method — verify-then-carve, per window

`build_traverse_corridor(window, seed, spec, params)` runs entirely on the **apron-padded** window and crops
its output (the carve delta + the route-distance fact) to the core with `_core_slice`, exactly like
`apron_blur_crop`. Pathfind and carve see apron samples (shared, deterministic world facts); the core is
cropped after; so the per-core delta is bit-identical from either neighbor.

### 4.1 Detect barriers from the composed height (verify step 1 — scale/relief-aware)

The barrier is measured from the **composed v2 height at the active relief/scale**, not from a fixed regime
threshold (which §1.1 verified never fires). Two barrier types, both computed on the apron-padded window's
composed height and connected-componented with the `component_stats` idiom:

1. **Slope-wall barrier:** `slope_grid(height, scene_width_m, height_scale_m) > slope_budget` — connected
   regions of terrain steeper than a route may climb, at the **active** `scene_width_m` / `height_scale_m`
   (so the same `slope_grid` / `height_scale_for` convention as the analyzer; barriers shrink at large spans
   and grow at high relief, exactly as measured). A slope-wall is a real barrier only if it **severs a
   crossing** — i.e. it separates one core edge from the opposite edge so the passable region no longer
   crosses WE (or NS).
2. **Low-corridor barrier (the Tier-1 case):** the largest `low_corridor` component
   (`passable & h ≤ percentile(h, low_corridor_pct)`) does **not** reach both opposite edges (no WE or NS
   crossing). This is the gentle-scale barrier that has no slope-wall but still leaves no natural valley route
   — the exact thing Tier-1 flags.

A window **needs a route** iff (1) a slope-wall severs the crossing, OR (2) the low corridor does not cross. If
the passable region already crosses AND the low corridor already crosses, the window is **crossable as-is →
no-op** (verify-first, §4.2 step 3). Regime weights are **not** the detector; they enter only as the route
**bias** (§4.2) — a route prefers to climb a foothill/plateau saddle over a range_core spine. Interior barriers
small enough to walk around (`min_barrier_component_frac` knob) are skipped — going around is already a route.

> **Seam-safety hazard (must-honor):** the `low_corridor` cutoff must be **data-independent**, not a raw
> per-core `percentile(h, ...)` — a global percentile differs between adjacent windows and would make the mask
> (and any carve keyed to it) disagree at the shared border. This is the same class of bug caught in keeper_v2
> review (`geo.norm01`'s per-window min/max broke seams, 0.0118 → 0.0). Use either a **fixed height cutoff
> constant** (a knob, same every window) or a cutoff computed over the **apron-padded** extent and applied to
> the core (shared apron samples → identical cutoff from either neighbor). Barrier *detection* may use a
> core-only percentile for the verify decision (detection does not write height, so it cannot break seams), but
> anything that influences the carve delta must be apron-computed or fixed.

### 4.2 Verify a natural route exists (verify step 2)

For each barrier component that must be crossed, run a **least-cost path** edge→edge across it on the
apron-padded grid:

- **Cost** = horizontal step distance × `(1 + slope_penalty * max(0, slope − slope_budget))`, where `slope` is
  the composed-height slope (the same `slope_grid` definition the analyzer uses: rise/run over the conditioned
  mesh at the active world scale). Steps at or under the slope budget are cheap; steeper steps cost more,
  steeply.
- **Channel/valley bias:** subtract a `drainage_bias` term where `channel_axis` is high or `channel_dist` is
  low, and prefer lower composed height (valley floors). This makes the search pick the **natural pass first**
  — it follows the existing river/valley through the range before it considers cutting a new line. (This is the
  "drainage biases, does not define" rule from §2.)
- Search is deterministic (Dijkstra / A* with a fixed tie-break on grid index — no `Math.random`, no
  floating-point-order nondeterminism), so the same window+seed yields the same route every run and from either
  neighbor's apron.

**If the cheapest path is already entirely under the slope budget → STOP. Leave the terrain alone.** Mark the
corridor fact (route distance) and carve **nothing**. This is the "verify-first" win: a range that already has
a natural pass is never disturbed. Brainstorming flagged this as the honesty pillar — most ranges with a valley
through them fall here.

### 4.3 Carve minimally (only if verify fails)

If and only if the cheapest path still exceeds the slope budget somewhere, carve along that path:

- Carve a **minimal** relief delta: lower the height along the route just enough that every step is at or under
  `slope_budget`, feathered over `corridor_width_m` so the cut reads as a pass/ramp, not a trench. Bounded by
  `carve_max_m` (a hard cap — if the budget cannot be met within the cap, that is reported, not silently
  exceeded; the per-game opt-in for "truly impassable" lives here).
- The carve is a **subtractive delta** sampled cheap online, composed into height **after** the keeper compose,
  so render / facts / collision all see the same post-carve surface (the visible==collision contract).
- **Seam-safe carve:** the path search and the feathered delta run on the apron-padded window; the delta is
  cropped to the core. Because the route geometry and depth depend only on shared apron facts (deterministic
  world fields), the carve delta on shared borders is **bit-identical** across neighbors. **Cross-seam join:** a
  route that reaches a core edge must continue in the neighbor — gated with the `adjacent_corridor_continuity`
  edge-match idiom applied to the new corridor fact (route entering an edge has a matching route entering the
  neighbor's shared edge within tolerance). This is what makes the guarantee *join* into effectively-infinite
  traversal rather than stopping at each window wall.

### 4.4 The new fact: `traverse_corridor`

A deterministic, world-anchored fact baked per window and sampled cheap online (designed for the future runtime
port, not ported here):

- `route_dist`: distance-to-nearest-guaranteed-route (like `channel_dist`, saturating to "far" outside the
  apron-valid band — same `max_fact_dist` discipline as `build_skeleton_window`).
- `carve_delta`: the (mostly zero) subtractive height delta. Zero everywhere verify-first succeeded.

Composed: `final_height = keeper_height + carve_delta`. `route_dist` is available to materials/scatter later
(a road/trail can follow it) and to the runtime collision path.

## 5. Tunable parameters (pillar 1)

All named in a Tier-3 params dataclass / config, no magic numbers. Barrier detection is height-derived
(§4.1), so the knobs are about *route quality and scale*, not a regime cutoff:
`slope_budget` (the grade a route must hold — the same threshold that defines a slope-wall),
`low_corridor_cutoff` (seam-safe fixed/apron-computed height cutoff for the valley-corridor test, **not** a raw
core percentile — see §4.1 hazard), `min_barrier_component_frac`, `slope_penalty`, `drainage_bias` (regime/
channel route bias), `corridor_width_m`, `carve_max_m`, per-regime `passable` / `impassable` opt-in map
(default: every regime gets a route), cross-seam `row_tolerance_px` / `band_px`, and the **active scale/relief**
(`scene_width_m`, `height_scale_m`) the barrier is measured at. Slope is measured in **absolute world metres**
at that active scale (same `slope_grid` / `height_scale_for` convention as the analyzer) so the guarantee is
scale-correct across clipmap levels and tracks the tunable relief (owner: "adjust height and stuff"). Defaults:
always-a-way-through, minimal carve, valley-biased.

## 6. Verification

Gates run alongside (not overriding) the owner eye. The headline is that **the guarantee itself becomes a
pass/fail gate**:

- **Connectivity guarantee (the headline gate):** the post-carve core **no longer needs a route** — i.e. the
  *same* crossing that `needs_route` flagged broken is reconnected. Operationally this is `needs_route(final)
  is False`: a slope-wall barrier requires the passable region to cross again; a low-corridor barrier requires
  the valley route to cross again. Verifying "the broken crossing reconnected" (not merely "some passable path
  exists") is what makes the gate non-vacuous — measured: for a low-corridor barrier the passable region
  already crosses pre-carve, so a passable-only check would pass without the carve doing anything. Asserted
  with the `component_stats` idiom on the post-carve surface. Fail = Tier-3 is wrong, full stop.
- **Seam-exactness:** max shared-border `carve_delta` (and final height) delta == 0.0 across adjacent windows
  (`adjacent_seam_deltas` idiom). Hard gate.
- **Cross-seam join:** routes entering a seam continue in the neighbor (`adjacent_corridor_continuity` idiom on
  the new corridor fact) above a match-fraction floor.
- **Minimal disturbance:** carved-cell fraction and max `|carve_delta|` are bounded and reported; verify-first
  windows carve **0**; passable-regime cells carve **0** (assert `carve_delta == 0` wherever barrier mask is
  false).
- **Did-real-work guard** (anti-fooling, per `worldgen10-profiling-must-be-real`): the gate must prove it
  actually found/needed routes on terrain that genuinely blocks — a crossable window trivially "passes." §1.1
  verified the **default 25.6 km / 260 m config has no slope-wall**, so the gate must construct a real barrier
  rather than hope to find one: run on a **high-relief config** (e.g. `post_tanh_gain`/`relief_amplitude` raised
  until ≥1 slope-wall severs a crossing — verified to produce ~11% impassable) AND/OR a **smaller span**, on
  **multiple seeds/origins**, and assert `barrier_components_crossed ≥ 1` *before* carve and the connectivity
  guarantee holds *after*. Also assert the verify-first no-op path fires on the default (gentle) config (carve
  == 0 where already crossable). A guarantee gate that only ran where there was nothing to do is worthless.
- **Determinism:** same seed+coords → same route, same carve; same coord independent of which window requested
  it.
- **Still-rugged guard:** Tier-3 must NOT flatten the variant into a pancake — `slope_p90` of the post-carve
  surface stays ≥ `MIN_STRUCTURAL_SLOPE_P90` (the analyzer's flat-everywhere floor). The whole point is to keep
  the ruggedness and add *a* way through, not to gentle everything.

### 6.1 Owner gate (the acceptance authority)

A **corridors-on review**: the existing A | B | v2 switcher scene (`rough_world_abv_review.tscn`) extended with
a corridor overlay (route line + carved cells highlighted) and the `report_abv_traversability.py` verdict
re-run **with Tier-3 applied**, so the owner sees (a) the route drawn on the terrain, (b) that passable areas
are visually untouched, and (c) the Tier-1 grade flipping to a guaranteed crossing. The owner judges whether
the routes read as natural passes (not trenches) and whether the untouched terrain still reads right. Owner eye
decides; metrics only prove the guarantee/seams/minimality held.

## 7. Slice plan

Offline Python; TDD; do not re-freeze any fixture or flip docs to "accepted" until the owner accepts the
corridors-on sheet. (Detailed task breakdown comes from `writing-plans` against this spec.)

1. **Barrier detection (height-derived, scale/relief-aware):** slope-wall mask (`slope_grid > slope_budget` at
   active scale/relief) + low-corridor mask (seam-safe cutoff, not raw core percentile) → connected components
   → "needs a route" = slope-wall severs crossing OR low corridor doesn't cross; skip-if-walk-around. Test: a
   **high-relief config** yields ≥1 crossing barrier; the default gentle config yields none (no-op path).
2. **Verify (least-cost path):** deterministic slope-penalized, channel-biased edge→edge search on the
   apron-padded grid; returns route + whether it is already under budget. Test: determinism + apron-symmetry
   (same route from either neighbor's apron).
3. **Carve (only on verify-fail):** minimal feathered subtractive delta to hit `slope_budget`, bounded by
   `carve_max_m`; cropped to core. Test: post-carve route ≤ budget; `carve_delta == 0` where verify-first
   succeeded and where regime is passable.
4. **Seam + join gates:** `carve_delta` border delta 0.0; cross-seam route continuity; did-real-work guard.
5. **`traverse_corridor` fact + compose:** `route_dist` + `carve_delta` baked; `final = keeper + carve_delta`;
   determinism + visible==(would-be)collision parity on the offline grid.
6. **Corridors-on sheet + Tier-1 re-run:** overlay on the A|B|v2 scene; `report_abv_traversability.py` with
   Tier-3 on; relief/seam/guarantee numbers labeled.
7. **Owner eye.** Accept → mark the keeper+corridor stack as the candidate Slice 3 port target; flip the
   traversability blocker on Slice 3; (later, owner-gated) port. Reject/iterate → tune knobs on the same sheet.

## 8. Boundary / honest risk

- Tier-3 is design-direction until the owner accepts the corridors-on sheet. Passed guarantee/seam gates ≠
  owner acceptance (DESIGN §7.3).
- **Cost risk (pillar 2, but offline):** per-window fine-grid least-cost path is the expensive offline step.
  Mitigations in priority order: verify-first skips carving (and most of the expense) on the common case; the
  search can run on a **coarsened** barrier grid (skeleton spacing, not fine page spacing) and the carve
  feathered onto the fine grid; bounded barrier components cap the search area. If still too slow, fall back to
  routing on the coarse skeleton grid only — explicitly `log()`ged, never silently down-sampled
  (`worldgen10-profiling-must-be-real`). This is offline cost; the runtime cost is the cheap `traverse_corridor`
  sample, designed in §4.4 but not built here.
- **Carve-vs-tone risk:** even a minimal carve changes the surface. If a route reads as an unnatural slot, that
  is an owner-eye reject → widen `corridor_width_m` / raise `slope_budget` / strengthen `drainage_bias` so it
  follows a real valley. The expectation is valley-biased verify-first routes rarely carve at all.
- **If the guarantee cannot hold with exact seams** (e.g. a cross-seam join is irreducibly global, or a carve
  needed to meet budget exceeds the apron reach), that is a real finding: report it; fall back to a wider apron
  for the route layer, or to the owner explicitly opting that regime into impassable. The expectation is the
  apron-cropped approach holds, exactly as it did for keeper_v2's blurs.
- **Downstream unblocked on acceptance, not done here:** the Slice 3 port target (keeper variant + corridor
  layer), the runtime `traverse_corridor` fact, the collision-path integration, Tier-2 bias knobs.
