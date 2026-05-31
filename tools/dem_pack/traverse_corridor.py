from __future__ import annotations
from dataclasses import dataclass
import heapq
import numpy as np

import keeper_v2 as v2
import geography_skeleton_windows as win
import analyze_rough_world_traversability as trav


@dataclass(frozen=True)
class TraverseParams:
    # Route must hold this grade (rise/run) on the conditioned mesh; also defines a slope-wall.
    slope_budget: float = 0.28          # == trav.PASSABLE_SLOPE, the play passability band
    # Seam-safe fixed height cutoff for the low/valley-corridor test (NOT a per-core percentile).
    low_corridor_cutoff: float = 0.0    # composed height is ~tanh-centered near 0; <= cutoff == "low"
    min_barrier_component_frac: float = 0.02   # interior barriers smaller than this -> walk around
    slope_penalty: float = 24.0         # cost multiplier per unit slope over budget
    drainage_bias: float = 0.55         # route bias toward channels/valleys (0 = none)
    corridor_width_m: float = 1200.0    # feather half-width of the carve
    carve_max_m: float = 220.0          # hard cap on |carve_delta| (world metres); exceed => report, not silent
    row_tolerance_px: int = 2           # cross-seam join tolerance
    band_px: int = 2                    # cross-seam join band width in pixels (paired with row_tolerance_px)
    # Active review scale/relief the barrier + slope are measured at (the analyzer convention).
    scene_width_m: float = 25600.0      # the 25.6 km play span (== spec.core_span_m at chunk scale)
    height_scale_m: float = trav.BASE_HEIGHT_SCALE_M   # 260 m default relief; raise to test/sim higher relief


def _padded_world_width_m(grid: np.ndarray, spec) -> float:
    """World width spanned by the apron-padded grid, so cell_m == core cell_m (= spacing_m)."""
    return float(spec.spacing_m) * float(grid.shape[0] - 1)


def padded_slope(height_full: np.ndarray, spec, p: TraverseParams) -> np.ndarray:
    """Slope magnitude over the padded composed height at the active relief, reusing the analyzer's
    slope_grid so the route shares the same rise/run convention as the Tier-1 report."""
    width_m = _padded_world_width_m(height_full, spec)
    return trav.slope_grid(np.asarray(height_full, dtype=np.float64), scene_width_m=width_m,
                           height_scale_m=float(p.height_scale_m))


def passable_mask(height_full: np.ndarray, spec, p: TraverseParams) -> np.ndarray:
    return padded_slope(height_full, spec, p) <= float(p.slope_budget)


def _core(grid_full: np.ndarray, spec) -> np.ndarray:
    cs = win._core_slice(spec)
    return np.asarray(grid_full)[cs, cs]


def needs_route_core(core_height: np.ndarray, spec, p: TraverseParams) -> dict:
    """Decide if an already-CORE composed-height grid needs a guaranteed route, with diagnostics.

    A window needs a route iff a slope-wall SEVERS the crossing OR the low/valley corridor does not cross.
    Detection writes no height, so the core-only masks here cannot break seams (only carve inputs must be
    seam-safe). This is the canonical decision: the Tier-3 guarantee is `needs_route_core(final) is False`."""
    core_h = np.asarray(core_height, dtype=np.float64)
    slopes_core = trav.slope_grid(core_h, scene_width_m=float(p.scene_width_m), height_scale_m=float(p.height_scale_m))
    passable = slopes_core <= float(p.slope_budget)
    slope_wall = ~passable
    pc = trav.component_stats(passable)
    passable_crosses = bool(pc["largest_crosses_we"] or pc["largest_crosses_ns"])

    low = passable & (core_h <= float(p.low_corridor_cutoff))
    lc = trav.component_stats(low)
    low_crosses = bool(lc["largest_crosses_we"] or lc["largest_crosses_ns"])

    slope_wall_frac = float(np.mean(slope_wall))
    sw = trav.component_stats(slope_wall)
    slope_wall_severs = (slope_wall_frac > 0.0) and (not passable_crosses) and \
                        (float(sw["largest_frac"]) >= float(p.min_barrier_component_frac))

    needs = bool(slope_wall_severs or (not low_crosses))
    return {
        "needs_route": needs,
        "slope_wall_frac": slope_wall_frac,
        "slope_wall_severs": slope_wall_severs,
        "passable_crosses": passable_crosses,
        "low_corridor_crosses": low_crosses,
    }


def needs_route(height_full: np.ndarray, spec, p: TraverseParams) -> dict:
    """Decide if the CORE window needs a guaranteed route, from an apron-padded composed height (crops first)."""
    return needs_route_core(_core(height_full, spec), spec, p)


def _step_cost(slope_b: float, h_b: float, chan_b: float, cell_m: float, p: TraverseParams) -> float:
    over = max(0.0, float(slope_b) - float(p.slope_budget))
    base = float(cell_m) * (1.0 + float(p.slope_penalty) * over)
    reward = float(p.drainage_bias) * (0.6 * float(chan_b) + 0.4 * float(np.clip(-h_b, 0.0, 1.0)))
    return max(base * (1.0 - reward), float(cell_m) * 0.05)


def least_cost_crossing(
    slopes: np.ndarray,
    height_full: np.ndarray,
    channel_full: np.ndarray,
    spec,
    p: TraverseParams,
    axis: str = "x",
) -> dict:
    """Deterministic Dijkstra crossing over the apron-padded grid.

    axis='x' crosses west->east; axis='z' crosses north->south. Ties are broken by
    flattened index through heap ordering and fixed neighbour order.
    """
    if axis not in ("x", "z"):
        raise ValueError("axis must be 'x' or 'z'")
    s = np.asarray(slopes, dtype=np.float64)
    h = np.asarray(height_full, dtype=np.float64)
    ch = np.asarray(channel_full, dtype=np.float64)
    if s.shape != h.shape or s.shape != ch.shape:
        raise ValueError("slopes, height_full, and channel_full must have identical shapes")
    work_s, work_h, work_ch = (s, h, ch) if axis == "x" else (s.T, h.T, ch.T)
    rows, cols = work_s.shape
    cell_m = float(spec.spacing_m)
    dist = np.full(rows * cols, float("inf"), dtype=np.float64)
    prev = np.full(rows * cols, -1, dtype=np.int64)
    pq: list[tuple[float, int]] = []
    for r in range(rows):
        idx = r * cols
        cost = _step_cost(work_s[r, 0], work_h[r, 0], work_ch[r, 0], cell_m, p)
        dist[idx] = cost
        heapq.heappush(pq, (cost, idx))

    target = -1
    while pq:
        d, idx = heapq.heappop(pq)
        if d > dist[idx]:
            continue
        r, c = divmod(idx, cols)
        if c == cols - 1:
            target = idx
            break
        for dr, dc in ((-1, 0), (1, 0), (0, 1), (0, -1)):
            nr, nc = r + dr, c + dc
            if not (0 <= nr < rows and 0 <= nc < cols):
                continue
            nidx = nr * cols + nc
            nd = d + _step_cost(work_s[nr, nc], work_h[nr, nc], work_ch[nr, nc], cell_m, p)
            if nd < dist[nidx]:
                dist[nidx] = nd
                prev[nidx] = idx
                heapq.heappush(pq, (nd, nidx))

    if target < 0:
        raise RuntimeError("least_cost_crossing: no path found across grid")
    path_work: list[tuple[int, int]] = []
    node = target
    while node != -1:
        path_work.append(divmod(int(node), cols))
        node = int(prev[node])
    path_work.reverse()
    path = [(r, c) if axis == "x" else (c, r) for (r, c) in path_work]
    max_step_slope = max((float(s[r, c]) for r, c in path), default=0.0)
    return {"path": path, "max_step_slope": float(max_step_slope), "total_cost": float(dist[target]), "axis": axis}


# NOTE: the seam-exact CONNECTED carve is intentionally not implemented here. Both a globally-routed
# least-cost-path carve (breaks seams) and purely-local seam-exact operators (don't guarantee a connected
# crossing) were prototyped and rejected — see spec §1.2 BUILD FINDING and memory
# worldgen10-tier3-seam-exact-carve. The carve is owed a cross-seam-stitched connected-corridor fact (the
# unbuilt connectivity half of Phase 7B). `least_cost_crossing` above stays as the verify-step building block
# for when that fact exists. `build_traverse_corridor` below reports `carve_pending` for real barriers rather
# than emitting a carve that is not seam-exact.


def build_traverse_corridor(window: dict, seed: int, spec, p: TraverseParams, keeper_params) -> dict:
    """Verify(-then-carve) for one window. Returns core-cropped `carve_delta` + `route_dist` + diagnostics.

    STATUS (see spec §1.2 BUILD FINDING, memory worldgen10-tier3-seam-exact-carve): detection + the
    verify-first no-op are done and seam-safe. The CARVE for a real barrier is BLOCKED: a globally-routed
    least-cost-path carve cannot be seam-exact (adjacent windows route differently -> border delta != 0), and
    no purely-local seam-exact operator guarantees a *connected* crossing. The seam-exact connected carve
    depends on a cross-seam-stitched connected-corridor fact (the unbuilt connectivity half of Phase 7B).

    So: if the window does NOT need a route -> no-op, zero delta, resolved=True (correct, seam-safe). If it DOES
    need a route -> emit a ZERO (seam-safe) delta and report resolved=False, carve_pending=True. We never emit a
    seam-breaking carve nor claim a route we cannot deliver seam-exactly (pillar: no shortcuts)."""
    full = v2.compose_windowed_height_v2_full(window, seed, spec, keeper_params)
    decision = needs_route(full, spec, p)
    cs = win._core_slice(spec)
    n = cs.stop - cs.start
    zero = np.zeros((n, n), dtype=np.float64)
    far = np.full((n, n), float(spec.apron_m) * 0.68, dtype=np.float64)

    if not decision["needs_route"]:
        return {
            "carve_delta": np.ascontiguousarray(zero),
            "route_dist": np.ascontiguousarray(far),
            "carved": False,
            "resolved": True,            # already crossable -> guarantee holds with no work
            "carve_pending": False,
            "route_axis": "",
            **decision,
        }

    # Real barrier, but the seam-exact connected carve is blocked (spec §1.2). Report honestly; emit no carve.
    return {
        "carve_delta": np.ascontiguousarray(zero),   # zero = seam-safe; NOT a resolved route
        "route_dist": np.ascontiguousarray(far),
        "carved": False,
        "resolved": False,
        "carve_pending": True,                       # a seam-exact connected-corridor carve is owed here
        "route_axis": "",
        **decision,
    }


def compose_with_corridor(window: dict, seed: int, spec, p: TraverseParams, keeper_params) -> tuple[np.ndarray, dict]:
    """Final composed height = keeper core height + Tier-3 carve delta. Render and collision both call this,
    so visible==collision parity holds by construction."""
    keeper = v2.compose_windowed_height_v2(window, seed, spec, keeper_params)
    res = build_traverse_corridor(window, seed, spec, p, keeper_params)
    return np.ascontiguousarray(keeper + res["carve_delta"]), res


def crossing_holds(core_height: np.ndarray, spec, p: TraverseParams) -> bool:
    """The guarantee: True iff the post-carve CORE no longer needs a route (the broken crossing -- slope-wall
    OR low-corridor, whichever tripped needs_route -- is reconnected). Thin wrapper over needs_route_core, so a
    low-corridor barrier cannot pass vacuously via an already-true passable crossing."""
    return not needs_route_core(np.asarray(core_height, dtype=np.float64), spec, p)["needs_route"]
