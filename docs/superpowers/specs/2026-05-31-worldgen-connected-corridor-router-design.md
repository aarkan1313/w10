# WorldGen10 — Connected-Corridor Router Design (Phase 7B pull-forward, unblocks the Tier-3 carve)

**Date:** 2026-05-31
**Milestone:** Phase 5 / Tier-3 traversability — the Phase 7B "connected drainage / pull-forward escape hatch"
(ROADMAP Phase 7B), pulled forward because the Tier-3 guarantee needs a *proven-connected* corridor.
**Status:** design-ready; offline Python render-first; implementation gated by owner approval of THIS spec, then
owner-eye acceptance of the corridors-on sheet. No Rust/GLSL port here (that is Slice 3+, owner-gated).
**Parents:** `docs/superpowers/specs/2026-05-31-worldgen-tier3-guaranteed-traversability-design.md` (§1.2 = the
carve-blocked finding this resolves); `docs/superpowers/specs/2026-05-31-worldgen-phase7b-drainage-skeleton-design.md`
(the runtime subsystem this is the connectivity half of); `docs/plans/ROADMAP.md` Phase 5 "YOU ARE HERE" + Phase
7B; `docs/plans/LOOSE_ENDS_LEDGER.md` B8; memory `worldgen10-tier3-seam-exact-carve`,
`worldgen10-tier3-barrier-measurements`, `worldgen10-tier3-guaranteed-traversability`.

---

## 1. Purpose

Tier-3 guarantees a route through barrier regions. Its detection + verify-first no-op + guarantee-check are
built and seam-safe, but the **carve is blocked** (Tier-3 spec §1.2): a globally-routed least-cost-path carve
cannot be seam-exact (adjacent windows route differently → border delta 0.62 ≠ 0), and no purely-local
seam-exact operator guarantees a *connected* crossing. The reconciliation is a **connected corridor that is
itself a seam-exact world fact** — the connectivity half of Phase 7B. This spec designs that router.

The router produces, per deterministic world-anchored window, a **connected corridor** (one spanning route or a
small network, tunable) that the Tier-3 carve follows. Because the corridor is anchored to **seam-identical edge
gates** and computed locally, the carve around it is bit-identical across neighbours (seam-exact), which a
global path could never be.

### 1.1 What is already proven (measured this session — the design rests on these, not on hope)

- **Edge gates are seam-identical.** Local minima of the composed-height line on a shared window edge are
  computed identically from either neighbour (the seam line itself has border delta **0.0** by keeper
  seam-exactness). Measured: both windows produced gates `[4, 16, 86, 121, 127]` on the shared seam, lowest at
  row 86 (= −0.421), agreeing exactly.
- **A gate-anchored carve is seam-exact.** A route ending at a shared-edge gate (row 86, agreed by both
  neighbours) with a local feathered carve gives a measured **seam border delta 0.0** — the exact thing the
  old global-path carve failed (0.62).
- **Cross-seam join already works** (`adjacent_corridor_continuity` match_frac = 1.00) and the flow-routing
  primitive exists (`geography_skeleton._flow_accumulation_mfd`).
- **Barrier fixtures** (memory `worldgen10-tier3-barrier-measurements`): low-corridor barrier = seed 1, spiky
  (`post_tanh_gain 2.4 / relief_amplitude 3.2`), 25.6 km; slope-wall-sever barrier = seed 42, gain 3.5, 4 km.

## 2. Non-Goals

- **No Rust/GLSL port.** Offline Python only. The runtime path is *designed* (§7) so the offline result is
  portable, but Slice 3+ does the port, owner-gated.
- **No global/world bake.** The corridor is `f(x, z, seed)` per apron-padded window — never a whole-world pass.
  (A per-authority-window off-frame route cache is an optimization, §7, not a correctness dependency.)
- **No always-carve.** Verify-first stays central: where the terrain already crosses, carve nothing.
- **No new keeper changes.** The router consumes the existing seam-exact keeper height + window facts unchanged.
- **No true global hydrology.** Corridors join across seams into effectively-infinite traversal; this is not a
  proof that any two arbitrary world points connect (same boundary as the Tier-3 spec).
- **No magic numbers.** Every gate/route/carve/density parameter is named config (pillar 1).

## 3. Architecture

New offline module `tools/dem_pack/corridor_router.py`, five units, each one responsibility, each independently
testable. It **replaces the blocked carve** inside `traverse_corridor.build_traverse_corridor` (detection,
no-op, and the `needs_route_core` guarantee-check are unchanged).

1. **`edge_gates(seam_line, p) -> list[int]`** — crossing points on one window edge = local minima of the
   composed-height line over a `gate_radius_px` window. Pure function of the seam-line values ⇒ both neighbours
   compute identical gates on a shared edge. Optionally filtered to the lowest `max_gates_per_edge`.
2. **`window_gates(full, spec, p) -> dict`** — gates on all four CORE edges (W/E/N/S) of the apron-padded
   window, as (row/col, height) on the core grid.
3. **`route_between_gates(full, a, b, spec, p) -> route`** — connect two gates with a valley-biased least-cost
   path. Reuses the built cost model + Dijkstra core (`traverse_corridor._step_cost` + the deterministic
   tie-broken heap loop): the existing `least_cost_crossing` is edge-to-edge (multi-source left column →
   right column), gate-to-gate is single-source → single-target, so the shared inner solver is **extracted into
   a `_dijkstra_cost_field(...)` helper** that both call (a pure refactor of the existing loop, guarded by the
   existing `least_cost_crossing` tests — no behavior change). Verify-first: if the gates already connect under
   budget along low ground, the route is marked "natural" (no carve owed).
4. **`build_corridor(window, seed, spec, p) -> corridor`** — choose gate pairs to link per `corridor_density`
   (1 → single lowest-cost spanning route; higher → link more pairs into a connected network reaching more
   edges), route each, union into a corridor mask + a seam-exact `corridor_dist` fact (distance-to-corridor,
   apron-cropped, saturating to "far" like `channel_dist`).
5. **`carve_corridor(full, corridor, spec, p) -> carve_delta`** — local feathered carve toward
   `low_corridor_cutoff` around the corridor, bounded by `carve_max_m`, cropped to core. Seam-exact because the
   corridor is gate-anchored (§4). Valley-first: cells already low/walkable get ~0 carve.

## 4. Seam-exactness contract (the one hard part — proven, §1.1)

Every value that influences `carve_delta` must depend only on shared world-coordinate data within apron reach.
The chain that guarantees it:

- **Gates** are local minima of the composed-height line on a CORE edge. Adjacent windows share that edge line
  exactly (keeper border delta 0.0), so they derive the **same gates** on the shared edge. Proven.
- **Routes** end at gates. A route reaching the shared edge ends at the seam-identical gate, so the route's
  geometry *near the seam* is the same from both sides. (The route's interior may differ between windows — that
  is fine, because the carve is **local**: a core cell's carve depends only on corridor cells within
  `corridor_width_m` + apron reach, and near the seam those are the shared gate-anchored cells.)
- **Carve** is a local feathered function of distance-to-corridor + composed height, computed on the
  apron-padded window and cropped to core. Near the seam it reads only shared, gate-anchored data ⇒ border
  delta **0.0** (proven on the seed-1 barrier).

Hard gate: max shared-border `carve_delta` (and final height) delta == 0.0 across adjacent windows. If any
operation cannot be made seam-exact this way, it does not go in (same discipline as keeper_v2's apron blur).

> **Cross-seam route JOIN.** A route exiting a core edge at a gate must continue from the neighbour's matching
> gate (the same world point). Because both windows route *to/from the same shared gate*, the corridors meet at
> the seam by construction. Verified with the existing `adjacent_corridor_continuity` edge-match idiom on the
> `corridor_dist` fact (match-fraction floor; the decorative-corridor version already hits 1.00).

## 5. The guarantee (unchanged contract, now deliverable)

The Tier-3 guarantee stays exactly as built: `needs_route_core(keeper_core + carve_delta) is False` — the
post-carve core no longer needs a route (the broken crossing, slope-wall OR low-corridor, is reconnected). The
router's job is to make that true: `build_corridor` must produce a corridor whose carve resolves the barrier; if
it cannot within `carve_max_m`, that window is reported `resolved=False` (the per-game-impassable case), never
silently claimed. `crossing_holds` / `compose_with_corridor` are unchanged.

## 6. Tunable parameters (pillar 1)

Named in a `CorridorParams` dataclass (or folded into `TraverseParams`), no magic numbers:
`gate_radius_px` (local-minima window for gates), `max_gates_per_edge`, `corridor_density` (1 = single spanning
route; higher = network — default leans network per owner "more fun"), `route_slope_penalty` /
`route_drainage_bias` (reuse the `least_cost_crossing` knobs), `corridor_width_m` (carve feather),
`carve_max_m`, `low_corridor_cutoff` (seam-safe fixed cutoff, NOT a per-core percentile — the §4.1 hazard from
the Tier-3 spec). Defaults: network-leaning, valley-first, minimal carve.

## 7. Runtime / live path (designed in, not an afterthought)

The corridor is `f(x, z, seed)` by construction (gate-anchored, apron-local), so it is procedural/infinite-ready
— that is the whole point of the seam discipline. Two layers, matching the existing M3 + 7B shapes:

- **Cheap online sample (no bake):** once a window's corridor exists, sampling `corridor_dist` and `carve_delta`
  is per-pixel cheap, exactly like `channel_dist` / `get_height`. `final = keeper_height + carve_delta` rides
  the existing M3 page path; render and collision both call it ⇒ visible==collision parity by construction.
- **Off-frame route solve + cache (the "bake"):** the route solve (`least_cost_crossing`, Dijkstra over a
  window) is too heavy for a frame, so the runtime routes on the **coarse world-anchored authority window
  off-frame and caches the corridor fact** (the Phase 7B authority-window cache), and fine pages sample the
  cached result. Determinism + world-anchoring make the cache a pure optimization (recompute ⇒ same answer),
  never a correctness dependency — no global bake, no camera-relative state.

This offline spec proves the `f(x,z,seed)` + seam contract first (Phase 5 discipline): if it doesn't hold
offline, it can't go live. The Rust/GLSL + authority-window-cache + off-frame-route port is Slice 3+, owner-gated
— the same CPU/facts/collision story the 7B roadmap entry requires, defined up front.

## 8. Verification

Gates run alongside (not overriding) the owner eye:
- **Seam-exactness (hard gate):** max shared-border `carve_delta` and final-height delta == 0.0 across adjacent
  windows, on both barrier fixtures (the `adjacent_seam_deltas` idiom). Proven achievable (§1.1).
- **Gate identity:** `edge_gates` of a shared edge identical from both neighbours (proven; locked as a test).
- **Connectivity guarantee:** `needs_route_core(final_core) is False` on both barrier fixtures (low-corridor +
  wall-sever) — the barrier is actually resolved, not vacuously passed.
- **Cross-seam join:** `corridor_dist` continuity across the seam above a match-fraction floor
  (`adjacent_corridor_continuity` idiom).
- **Verify-first / minimal:** gentle default carves 0; passable cells carve 0; carve bounded by `carve_max_m`;
  carved fraction reported.
- **Still-rugged:** post-carve `slope_p90 ≥ MIN_STRUCTURAL_SLOPE_P90` (no pancaking).
- **Did-real-work guard:** the gate runs on the measured barrier fixtures (seed 1 / 25.6 km low-corridor, seed
  42 / 4 km wall-sever) and asserts a real barrier existed pre-carve — never validated on crossable terrain.
- **Determinism + density knob:** same seed+coords → same corridor; `corridor_density=1` → single spanning
  route, higher → more edges reached (both asserted).

### 8.1 Owner gate (acceptance authority)
A corridors-on sheet: the A|B|v2 switcher / barrier fixtures with the corridor + carved cells overlaid, before/
after, plus the Tier-1 verdict flipping to a guaranteed crossing. Owner judges whether routes read as natural
valleys/passes (not trenches) and untouched terrain still reads right. Owner eye decides; metrics only prove
seams/guarantee/minimality held.

## 9. Slice plan

Offline Python; TDD; do not flip docs to "accepted" until the owner accepts the sheet.
1. `edge_gates` + gate-identity test (shared-edge gates identical between neighbours).
2. `window_gates` (four core edges).
3. `route_between_gates` (reuse `least_cost_crossing`, gate-to-gate, verify-first natural-route flag).
4. `build_corridor` + `corridor_dist` fact + `corridor_density` knob (single route ↔ network).
5. `carve_corridor` (local feathered, seam-exact) + the hard seam gate (border 0.0 on both fixtures).
6. Wire into `traverse_corridor.build_traverse_corridor`: replace `carve_pending` with the real corridor carve;
   guarantee (`needs_route_core(final) False`), still-rugged, minimal, cross-seam-join gates.
7. Corridors-on sheet + owner review. Accept → unblock Tier-3 / the Slice-3 port candidate. Reject → tune knobs.

## 10. Boundary / honest risk

- Design-direction until the owner accepts the sheet. Passed gates ≠ owner acceptance (DESIGN §7.3).
- **Route-interior-vs-seam:** the carve is seam-exact because it is *local* and the near-seam cells are
  gate-anchored. If a chosen `corridor_width_m` is so wide that a core cell's carve reaches corridor cells whose
  position differs between windows (interior route divergence within feather reach of the seam), seams could
  break. Mitigation: the feather reach must stay within the gate-anchored near-seam band; verified by the hard
  seam gate on both fixtures. If a wide corridor genuinely needs interior route agreement, anchor interior
  waypoints to world coordinates too (more gates) — reported, not silently widened.
- **Density vs disturbance:** a dense network carves more. The `corridor_density` knob + the still-rugged guard
  bound it; owner eye is the final arbiter of "too carved."
- **Cost (offline):** routing N gate pairs per window is the offline expense; verify-first skips carving on
  natural routes; the runtime routes coarse + off-frame + cached (§7). If too slow offline, route on the coarse
  skeleton grid — `log()`ged, never silently downsampled.
- Downstream unblocked on acceptance, not done here: the Slice-3 port target, the authority-window cache, the
  runtime off-frame route.
