"""Connected pass NETWORK through a steep mountain range (Tier-3, mountain scale).

A single pass can't make a 96%-impassable 270 km range traversable, and a per-cell grid / valley-mask carve
reads artificial or as scattered blobs (rejected, see memory worldgen10-tier3-corridor-built-mountain-gap).
This builds a CONNECTED network: a sparse set of WE + NS least-cost crossings (routed on a coarse grid for
speed, following the natural low ground via the slope-penalized Dijkstra), then carve_ramp a walkable valley
along each. The routes branch + connect into a believable trail/pass network through the range.

Seam-exactness for the 9x9 review comes from carving the ONE big field then slicing into chunks (the mountain
9x9 already slices a single generated field), so no per-window gate-anchoring is needed here.

Success criterion is NETWORK-aware: does the carved walkable band connect edge-to-edge? (needs_route_core's
single-largest-component valley metric is too strict for a network whose passability is split across several
wide pass-corridors.)
"""

from __future__ import annotations

from dataclasses import dataclass
import types

import numpy as np
from scipy.ndimage import zoom, binary_dilation

import traverse_corridor as tc
import corridor_router as cr
import analyze_rough_world_traversability as trav


@dataclass(frozen=True)
class PassNetworkParams:
    n_we: int = 4                 # west-east crossings
    n_ns: int = 4                 # north-south crossings
    coarse_n: int = 193           # routing grid resolution (coarse = fast; carve is on the full grid)
    ramp_half_frac: float = 0.020  # pass half-width as a fraction of the field span (span-relative)
    ramp_flat_frac: float = 0.006  # flat valley-floor half-width fraction
    carve_max_m: float = 3500.0


def _routes(height: np.ndarray, span_m: float, height_scale_m: float, p_trav, pp: PassNetworkParams):
    """Sparse WE + NS least-cost crossings on a coarse grid, mapped back to full-res index space.
    Each crossing is seeded at an evenly-spaced start row/col and follows the natural low ground."""
    n = height.shape[0]
    sc = pp.coarse_n / n
    hc = zoom(height, (sc, sc), order=1)
    nc = hc.shape[0]
    slc = trav.slope_grid(hc, scene_width_m=span_m, height_scale_m=height_scale_m)
    ch = np.zeros_like(hc)
    cm = span_m / (nc - 1)
    routes = []
    for k in range(int(pp.n_we)):
        r0 = int((k + 0.5) / pp.n_we * nc)
        prev, _, tgt = tc._dijkstra_cost_field(slc, hc, ch, cm, p_trav, [(r0, 0)], lambda r, c: c == nc - 1)
        if tgt >= 0:
            routes.append([(int(rr / sc), int(cc / sc)) for rr, cc in tc._reconstruct_path(prev, tgt, nc)])
    for k in range(int(pp.n_ns)):
        c0 = int((k + 0.5) / pp.n_ns * nc)
        prev, _, tgt = tc._dijkstra_cost_field(slc.T, hc.T, ch.T, cm, p_trav, [(c0, 0)], lambda r, c: c == nc - 1)
        if tgt >= 0:
            routes.append([(int(cc / sc), int(rr / sc)) for rr, cc in tc._reconstruct_path(prev, tgt, nc)])
    return routes


def carve_pass_network(height: np.ndarray, span_m: float, height_scale_m: float,
                       p_trav=None, pp: PassNetworkParams = PassNetworkParams()) -> dict:
    """Route + carve a connected pass network on `height` (a single big field). Returns
    {delta, final, routes, network_crosses, carved_frac}. delta/final are full-res; carving the big field then
    slicing into chunks keeps the network seam-exact across chunks."""
    n = height.shape[0]
    cell_m = span_m / (n - 1)
    p_trav = p_trav if p_trav is not None else __import__("dataclasses").replace(
        tc.TraverseParams(), scene_width_m=span_m, height_scale_m=height_scale_m)

    # the corridor router needs a no-apron single-window shim (this is one continuous field, not a keeper window)
    import geography_skeleton_windows as win
    saved_core, saved_slice = cr._core, win._core_slice
    cr._core = lambda full, spec: np.asarray(full)
    win._core_slice = lambda spec: slice(0, n)
    try:
        routes = _routes(height, span_m, height_scale_m, p_trav, pp)
        spec = types.SimpleNamespace(spacing_m=cell_m, apron_m=0.0, core_span_m=span_m)
        p_cor = cr.CorridorParams(
            corridor_density=1, slope_budget=float(p_trav.slope_budget),
            ramp_half_width_m=span_m * float(pp.ramp_half_frac),
            ramp_flat_half_m=span_m * float(pp.ramp_flat_frac),
            ramp_carve_max_m=float(pp.carve_max_m),
        )
        delta = cr.carve_ramp(height, {"routes": [{"path": rt} for rt in routes]}, spec, p_cor,
                              height_scale_m=height_scale_m)
    finally:
        cr._core, win._core_slice = saved_core, saved_slice

    final = height + delta
    # NETWORK-aware crossing: does the carved walkable band connect edge-to-edge? Build the route band, intersect
    # with the passable mask, and check the largest component crosses.
    rmask = np.zeros((n, n), dtype=bool)
    for rt in routes:
        if rt:
            idx = np.asarray(rt, dtype=np.int64)
            rmask[idx[:, 0], idx[:, 1]] = True
    half_px = max(1, int(span_m * float(pp.ramp_half_frac) / cell_m))
    band = binary_dilation(rmask, iterations=half_px)
    slopes = trav.slope_grid(final, scene_width_m=span_m, height_scale_m=height_scale_m)
    walkable_net = (slopes <= float(p_trav.slope_budget)) & band
    st = trav.component_stats(walkable_net)
    network_crosses = bool(st["largest_crosses_we"] or st["largest_crosses_ns"])
    return {
        "delta": np.ascontiguousarray(delta),
        "final": np.ascontiguousarray(final),
        "routes": routes,
        "network_crosses": network_crosses,
        "band_passable_frac": float((slopes[band] <= float(p_trav.slope_budget)).mean()) if band.any() else 0.0,
        "carved_frac": float(np.mean(delta != 0.0)),
    }
