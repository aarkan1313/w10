from __future__ import annotations
from dataclasses import dataclass
import numpy as np
from scipy.ndimage import distance_transform_edt, gaussian_filter

import traverse_corridor as tc
import keeper_v2 as v2
import geography_skeleton_windows as win


@dataclass(frozen=True)
class CorridorParams:
    gate_radius_px: int = 3            # local-minima window for edge gates (pure f(seam line) -> seam-identical)
    max_gates_per_edge: int = 5        # keep only the lowest N gates per edge
    corridor_density: int = 2          # gate pairs to link: 1 = single spanning route, higher = network
    corridor_width_m: float = 1200.0   # carve feather half-width
    carve_max_m: float = 700.0         # hard cap on |carve_delta| (world metres); deep enough to lower a
                                       # saddle to the valley cutoff on high-relief terrain (220 was too shallow)
    low_corridor_cutoff: float = 0.0   # FIXED seam-safe cutoff (NOT np.percentile)
    slope_budget: float = 0.28         # grade a route must hold (rise/run); matches TraverseParams.slope_budget
    # --- ramp carve (slope-wall barriers / mountains): cut a walkable VALLEY through a steep wall ---
    ramp_floor_grade_frac: float = 0.35  # valley floor descends at slope_budget*this ALONG the route. < 1 leaves
                                         # margin for the cross-slope so the COMBINED 2D gradient stays <= budget
                                         # (a floor graded at full budget reads ~budget*sqrt2 -> impassable).
    ramp_wall_grade_frac: float = 0.55   # band walls rise at slope_budget*this away from the flat floor
    ramp_flat_half_m: float = 2000.0     # half-width of the flat valley bottom
    ramp_half_width_m: float = 5000.0    # total half-width of the carved band (flat floor + graded walls)
    ramp_floor_smooth_px: float = 5.0    # smoothing of the floor field (kills zigzag-route bumpiness)
    ramp_carve_max_m: float = 3500.0     # cap for the ramp carve (mountains need a deep valley; bigger than the
                                         # gentle-saddle carve_max_m)


def edge_gates(seam_line: np.ndarray, p: CorridorParams) -> list[int]:
    """Crossing points on one window edge = local minima of the composed-height line over a gate_radius
    window. Pure function of the line values -> two neighbours sharing this edge compute identical gates.
    Returns indices sorted by height (lowest first), truncated to max_gates_per_edge."""
    line = np.asarray(seam_line, dtype=np.float64)
    k = int(p.gate_radius_px)
    n = line.size
    gates = [i for i in range(n) if line[i] == np.min(line[max(0, i - k): min(n, i + k + 1)])]
    gates.sort(key=lambda i: float(line[i]))
    return gates[: int(p.max_gates_per_edge)]


def _core(full: np.ndarray, spec) -> np.ndarray:
    cs = win._core_slice(spec)
    return np.asarray(full)[cs, cs]


def window_gates(full: np.ndarray, spec, p: CorridorParams) -> dict:
    """Gates on all four CORE edges. Returns {"w":[(r,0)...], "e":[(r,n-1)...], "n":[(0,c)...], "s":[(n-1,c)...]}.
    Each edge's gates are seam-identical with the matching neighbour edge (edge_gates is pure f(edge line))."""
    core = _core(full, spec)
    n = core.shape[0]
    return {
        "w": [(r, 0) for r in edge_gates(core[:, 0], p)],
        "e": [(r, n - 1) for r in edge_gates(core[:, -1], p)],
        "n": [(0, c) for c in edge_gates(core[0, :], p)],
        "s": [(n - 1, c) for c in edge_gates(core[-1, :], p)],
    }


def route_between_gates(full: np.ndarray, a: tuple[int, int], b: tuple[int, int], spec, tp) -> dict:
    """Valley-biased least-cost path between two CORE-edge gates, reusing the tier3 cost model + Dijkstra
    core. Single-source (a) -> single-target (b). Returns {path, max_step_slope, natural}."""
    core_h = _core(full, spec)
    slopes = tc.trav.slope_grid(core_h, scene_width_m=float(tp.scene_width_m), height_scale_m=float(tp.height_scale_m))
    channel = np.zeros_like(core_h)   # valley pull comes from height in _step_cost; channel bias optional here
    cell_m = float(spec.spacing_m)
    rows, cols = core_h.shape
    ar, acol = a
    bt = (int(b[0]), int(b[1]))
    prev, dist, target = tc._dijkstra_cost_field(
        slopes, core_h, channel, cell_m, tp, [(int(ar), int(acol))], lambda r, c: (r, c) == bt
    )
    if target < 0:
        raise RuntimeError("route_between_gates: no path")
    path = tc._reconstruct_path(prev, target, cols)
    max_step_slope = max((float(slopes[r, c]) for r, c in path), default=0.0)
    natural = bool(max_step_slope <= float(tp.slope_budget))
    return {"path": path, "max_step_slope": float(max_step_slope), "natural": natural}


def _gate_pairs(g: dict, density: int) -> list[tuple[tuple[int, int], tuple[int, int]]]:
    """Ordered opposite-edge gate pairs, lowest-first. density caps how many we link.
    Pair order: W<->E lowest, N<->S lowest, then next-lowest pairs, alternating axes."""
    pairs: list[tuple[tuple[int, int], tuple[int, int]]] = []
    we = list(zip(g["w"], g["e"]))      # gates are already lowest-first per edge
    ns = list(zip(g["n"], g["s"]))
    i = 0
    while len(pairs) < density and (i < len(we) or i < len(ns)):
        if i < len(we):
            pairs.append(we[i])
        if len(pairs) < density and i < len(ns):
            pairs.append(ns[i])
        i += 1
    return pairs[:density]


def _seam_stub_band_px(spec, p: CorridorParams, n: int | None = None) -> int:
    """How many cells in from each edge the corridor runs STRAIGHT (perpendicular) into its gate.
    Must cover the carve feather reach so the near-seam carve reads only the gate-anchored stub, not the
    window-dependent route interior (the seam-exactness fix). reach = corridor_width / spacing + 1 px margin.
    Clamped to < n/2 - 1 so the interior anchor stays inside the grid even for a very wide corridor."""
    band = int(np.ceil(float(p.corridor_width_m) / float(spec.spacing_m))) + 1
    if n is not None:
        band = max(1, min(band, n // 2 - 2))
    return band


def _interior_anchor(gate: tuple[int, int], n: int, band: int) -> tuple[int, int]:
    """The point `band` cells in from a gate, perpendicular to its edge. The interior route runs anchor->anchor
    (window-dependent, but >= band from every edge); the perpendicular stub runs anchor->gate (identical both
    sides of a seam). This keeps the corridor CONNECTED and seam-exact near the edge."""
    gr, gc = gate
    if gc == 0:
        return (gr, band)
    if gc == n - 1:
        return (gr, n - 1 - band)
    if gr == 0:
        return (band, gc)
    if gr == n - 1:
        return (n - 1 - band, gc)
    return gate


def _stub_cells(gate: tuple[int, int], n: int, band: int) -> list[tuple[int, int]]:
    """Straight perpendicular cells from the gate inward to its interior anchor (inclusive). Identical from
    both neighbours sharing the gate's edge -> the near-seam corridor is seam-exact."""
    gr, gc = gate
    if gc == 0:
        return [(gr, k) for k in range(band + 1)]
    if gc == n - 1:
        return [(gr, n - 1 - k) for k in range(band + 1)]
    if gr == 0:
        return [(k, gc) for k in range(band + 1)]
    if gr == n - 1:
        return [(n - 1 - k, gc) for k in range(band + 1)]
    return [gate]


def build_corridor(full: np.ndarray, spec, tp, p: CorridorParams) -> dict:
    """Link `corridor_density` opposite-edge gate pairs into a connected corridor. Returns {mask, corridor_dist,
    routes} on the CORE grid. corridor_dist saturates to apron*0.68 (same 'far' discipline as channel_dist).

    SEAM-EXACTNESS: each route's near-edge cells are straightened to a perpendicular stub through its gate
    (_straighten_route_ends) over the carve feather reach, so the near-seam corridor depends ONLY on the
    seam-identical gate, not the window-dependent route interior. This makes carve_corridor seam-exact."""
    core = _core(full, spec)
    n = core.shape[0]
    g = window_gates(full, spec, p)
    pairs = _gate_pairs(g, int(p.corridor_density))
    band = _seam_stub_band_px(spec, p, n)
    mask = np.zeros_like(core, dtype=bool)
    routes = []
    for a, b in pairs:
        # interior route runs anchor->anchor (window-dependent, but kept >= band from every edge); the
        # perpendicular stub runs anchor->gate (seam-identical). Union = connected AND seam-exact near edges.
        anchor_a = _interior_anchor(a, n, band)
        anchor_b = _interior_anchor(b, n, band)
        r = route_between_gates(full, anchor_a, anchor_b, spec, tp)
        routes.append(r)
        cells = list(r["path"]) + _stub_cells(a, n, band) + _stub_cells(b, n, band)
        for (rr, cc) in cells:
            if 0 <= rr < n and 0 <= cc < n:
                mask[rr, cc] = True
    far = float(spec.apron_m) * 0.68
    if mask.any():
        corridor_dist = np.minimum(distance_transform_edt(~mask) * float(spec.spacing_m), far)
    else:
        corridor_dist = np.full_like(core, far)
    return {"mask": mask, "corridor_dist": corridor_dist, "routes": routes}


def carve_seam_safe(spec, p: CorridorParams, n: int) -> bool:
    """Seam-exactness precondition: with a network (density>1), a route perpendicular to one seam can have
    interior cells within the carve feather reach of an ADJACENT seam, breaking seam-exactness. A single route
    (density==1) only touches each seam at its gate-anchored stub, so it is always seam-safe. For density>1 the
    feather reach (px) must be small relative to the window so no cross-route interior reaches a perpendicular
    seam. Mirrors keeper_v2's 'blur reach must fit the apron' discipline."""
    if int(p.corridor_density) <= 1:
        return True
    feather_px = float(p.corridor_width_m) / float(spec.spacing_m)
    return feather_px <= float(n) * 0.10   # cross-route interior stays clear of perpendicular seams


def carve_corridor(full: np.ndarray, corridor: dict, spec, p: CorridorParams, height_scale_m: float = 260.0) -> np.ndarray:
    """Local feathered subtractive (<=0) carve toward low_corridor_cutoff around the corridor, on the CORE grid.
    Seam-exact: the corridor is gate-anchored (seam-identical near the seam) and the carve is local, so the
    core border is bit-identical between neighbours. carve_max_m is a world-metre cap (-> height units).

    Rejects (does not silently produce) a configuration that is not seam-safe (carve_seam_safe) -- no shortcuts:
    validate and reject rather than emit a seam-breaking carve."""
    core = _core(full, spec)
    n = core.shape[0]
    if not carve_seam_safe(spec, p, n):
        raise ValueError(
            f"carve_corridor: corridor_density={p.corridor_density} with feather "
            f"{p.corridor_width_m/spec.spacing_m:.0f}px on a {n}px window is not seam-safe "
            f"(feather too large relative to window for a network). Use corridor_density=1 or a larger window."
        )
    dist_m = np.asarray(corridor["corridor_dist"], dtype=np.float64)   # 0 on corridor, metres elsewhere (core)
    feather = np.clip(1.0 - dist_m / max(float(p.corridor_width_m), 1.0), 0.0, 1.0)
    cutoff = float(p.low_corridor_cutoff)
    cap_h = float(p.carve_max_m) / float(height_scale_m)
    delta = -np.clip(core - cutoff, 0.0, cap_h) * feather
    return np.ascontiguousarray(delta)


def carve_ramp(full: np.ndarray, corridor: dict, spec, p: CorridorParams, height_scale_m: float = 1700.0) -> np.ndarray:
    """Cut a walkable VALLEY through a steep slope-wall (mountain) barrier, on the CORE grid. Unlike
    carve_corridor (a feathered cut toward a cutoff -- good for gentle valley reconnection), this builds a
    slope-FEASIBLE valley: a smooth floor that descends gently ALONG the route + graded walls rising away from
    it, so a connected passable BAND crosses the wall.

    KEY (the hard-won fix): the floor's along-route grade must be slope_budget * ramp_floor_grade_frac (< 1),
    NOT full budget -- a floor graded at full budget plus any cross-slope gives a COMBINED 2D gradient ~budget*
    sqrt2 > budget (impassable). The reduced along-grade leaves margin so the combined gradient stays <= budget.
    The floor field is smoothed to remove the zigzag-route bumpiness that otherwise re-introduces slope.

    Subtractive (<=0), bounded by ramp_carve_max_m. Seam-exactness: same gate-anchored basis as carve_corridor
    (the route ends at seam-identical gates); the floor/wall grading is a local function of distance-to-route."""
    core = _core(full, spec)
    n = core.shape[0]
    cell_m = float(spec.spacing_m)
    budget = float(p.slope_budget)
    core_m = core * float(height_scale_m)

    routes = corridor.get("routes", [])
    if not routes:
        return np.ascontiguousarray(np.zeros_like(core))

    # union the floor target across all routes (network); each route contributes a slope-feasible valley
    delta_m = np.zeros_like(core_m)
    for route in routes:
        path = route["path"]
        if not path:
            continue
        idx = np.asarray(path, dtype=np.int64)
        # 1) slope-feasible floor ALONG the route at the REDUCED grade (margin for cross-slope)
        along = core_m[idx[:, 0], idx[:, 1]].astype(np.float64)
        prof = along.copy()
        step = budget * float(p.ramp_floor_grade_frac) * cell_m
        for i in range(1, prof.size):
            prof[i] = min(prof[i], prof[i - 1] + step)
        for i in range(prof.size - 2, -1, -1):
            prof[i] = min(prof[i], prof[i + 1] + step)
        # 2) scatter to a floor field, smooth it (kill zigzag bumps), grade walls up away from the route
        on_path = np.zeros((n, n), dtype=bool)
        on_path[idx[:, 0], idx[:, 1]] = True
        prof_field = np.full((n, n), np.inf)
        prof_field[idx[:, 0], idx[:, 1]] = prof
        distpx, (iy, ix) = distance_transform_edt(~on_path, return_indices=True)
        floor = gaussian_filter(prof_field[iy, ix], sigma=float(p.ramp_floor_smooth_px))
        d_m = distpx * cell_m
        wall_rise = np.clip(d_m - float(p.ramp_flat_half_m), 0.0, None) * (budget * float(p.ramp_wall_grade_frac))
        target = floor + wall_rise
        band = d_m <= float(p.ramp_half_width_m)
        this = np.where(band, np.minimum(target - core_m, 0.0), 0.0)
        delta_m = np.minimum(delta_m, this)   # deepest carve wins where routes overlap

    delta_m = np.clip(delta_m, -float(p.ramp_carve_max_m), 0.0)
    return np.ascontiguousarray(delta_m / float(height_scale_m))
