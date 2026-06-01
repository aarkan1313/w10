from __future__ import annotations
from dataclasses import dataclass
from typing import Callable
import numpy as np
import terrain_edits.apply as ap


@dataclass(frozen=True)
class TerrainEdit:
    """One tunable edit = a placement (geometry from facts) + a profile (geometry+terrain -> delta).
    axes: which crossings to place (one route per axis).
    combine_mode: RESERVED (not yet read by apply_edits; today all edits composite via min -- cuts).
    Two-level per-edit-vs-cross-edit compositing lands when fill edits (lakes) arrive."""
    placement: Callable
    placement_params: object
    profile: Callable
    profile_params: object
    axes: tuple[str, ...] = ("x",)
    combine_mode: str = "min"


def _as_routes(result):
    """Normalize a placement's return into a LIST OF ROUTES. A placement may return either a single route
    (a list of (row, col) tuples) or multiple routes (a list of routes). Detect by the first element: a
    (row, col) pair => single route; a list/route => already multiple."""
    if not result:
        return []
    first = result[0]
    # a single route's first element is a 2-element (row, col) of ints; a multi-route's first element is a list
    is_single = isinstance(first, tuple) and len(first) == 2 and not isinstance(first[0], (list, tuple))
    return [result] if is_single else list(result)


def _place_routes(ed, h, ctx, axis):
    """Return the list of routes this edit places on `axis`. If the placement params carry route_count > 1,
    place that many crossings at start positions spread evenly across the perpendicular extent (so trails
    thread different parts of the range, not just the one easiest edge). route_count == 1 (or absent) = a
    single sweeping crossing. A placement may itself return MULTIPLE routes (e.g. cross_waypoint's 4 arms) --
    those are flattened in. The placement must accept a `start_row` kwarg to support spread placement."""
    count = int(getattr(ed.placement_params, "route_count", 1) or 1)
    if count <= 1:
        return _as_routes(ed.placement(h, ctx, ed.placement_params, axis=axis))
    extent = h.shape[0] if axis == "x" else h.shape[1]   # rows for x-crossings, cols for z-crossings
    routes = []
    for k in range(count):
        start = int((k + 0.5) / count * extent)
        routes.extend(_as_routes(ed.placement(h, ctx, ed.placement_params, axis=axis, start_row=start)))
    return routes


def apply_edits(height: np.ndarray, ctx, edits) -> np.ndarray:
    """Run each edit's placement (per axis, route_count routes) + profile, composite all deltas. Returns the
    seam-exact world-local delta to add to base height. Deterministic."""
    h = np.asarray(height, dtype=np.float64)
    deltas = [np.zeros_like(h)]
    for ed in edits:
        for axis in ed.axes:
            for route in _place_routes(ed, h, ctx, axis):
                deltas.append(ed.profile(h, route, ctx, ed.profile_params))
    # min = deepest cut wins; per-edit combine_mode is reserved (see TerrainEdit) until fill edits exist
    return ap.combine(deltas, mode="min")
