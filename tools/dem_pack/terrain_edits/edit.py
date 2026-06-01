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


def apply_edits(height: np.ndarray, ctx, edits) -> np.ndarray:
    """Run each edit's placement (per axis) + profile, composite all deltas. Returns the seam-exact world-local
    delta to add to base height. Deterministic."""
    h = np.asarray(height, dtype=np.float64)
    deltas = [np.zeros_like(h)]
    for ed in edits:
        for axis in ed.axes:
            route = ed.placement(h, ctx, ed.placement_params, axis=axis)
            deltas.append(ed.profile(h, route, ctx, ed.profile_params))
    # min = deepest cut wins; per-edit combine_mode is reserved (see TerrainEdit) until fill edits exist
    return ap.combine(deltas, mode="min")
