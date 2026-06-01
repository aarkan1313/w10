from __future__ import annotations
from dataclasses import dataclass
import numpy as np
from scipy.ndimage import distance_transform_edt
import terrain_edits.apply as ap


@dataclass(frozen=True)
class ThinTrailParams:
    floor_grade_frac: float = 0.5     # trail floor climbs at slope_budget*this ALONG the route (<1 leaves
                                      # cross-slope margin so the combined 2D gradient stays walkable)
    trail_width_m: float = 300.0      # flat trail half-width (thin)
    blend_width_m: float = 400.0      # smoothstep taper width beyond the flat trail (no cliffs)
    depth_cap_m: float = 4000.0       # max lowering below a cell's own raw height (preserve peaks; tune by eye)


def thin_climbing_trail(height: np.ndarray, route: list[tuple[int, int]], ctx, p: ThinTrailParams) -> np.ndarray:
    """A thin, gently-climbing walkable ledge along `route` that PRESERVES the surrounding terrain.
    PROVEN recipe: re-grade the route's RAW height monotone at slope_budget*floor_grade_frac (NOT a smoothed
    version -- smoothing re-steepens it toward surrounding peaks), carve a thin ledge to it, blend, bound."""
    n0, n1 = height.shape
    hm = np.asarray(height, dtype=np.float64) * float(ctx.height_scale_m)
    if len(route) < 2:
        return np.zeros_like(hm) / float(ctx.height_scale_m)
    idx = np.asarray(route, dtype=np.int64)
    raw = hm[idx[:, 0], idx[:, 1]].astype(np.float64)
    prof = raw.copy()
    s = float(ctx.slope_budget) * float(p.floor_grade_frac) * float(ctx.cell_m)
    for i in range(1, prof.size):
        prof[i] = min(prof[i], prof[i - 1] + s)
    for i in range(prof.size - 2, -1, -1):
        prof[i] = min(prof[i], prof[i + 1] + s)
    prof = np.maximum(prof, raw - float(p.depth_cap_m))
    on_path = np.zeros((n0, n1), dtype=bool)
    on_path[idx[:, 0], idx[:, 1]] = True
    prof_field = np.full((n0, n1), np.inf)
    prof_field[idx[:, 0], idx[:, 1]] = prof
    dpx, (iy, ix) = distance_transform_edt(~on_path, return_indices=True)
    floor = prof_field[iy, ix]
    dist_m = dpx * float(ctx.cell_m)
    raw_cut = np.minimum(floor - hm, 0.0)
    blended = ap.blend_edges(raw_cut, dist_m, flat_to=float(p.trail_width_m), blend_to=float(p.trail_width_m) + float(p.blend_width_m))
    bounded = ap.bound_depth(blended, cap_m=float(p.depth_cap_m))
    return np.ascontiguousarray(bounded / float(ctx.height_scale_m))


@dataclass(frozen=True)
class GradedValleyParams:
    floor_grade_frac: float = 0.35
    trail_width_m: float = 1200.0
    blend_width_m: float = 1500.0
    depth_cap_m: float = 6000.0


def graded_valley(height, route, ctx, p: GradedValleyParams):
    """A wider graded valley (vs the thin trail). Same machinery, wider knobs."""
    return thin_climbing_trail(height, route, ctx, ThinTrailParams(
        floor_grade_frac=p.floor_grade_frac, trail_width_m=p.trail_width_m,
        blend_width_m=p.blend_width_m, depth_cap_m=p.depth_cap_m))
