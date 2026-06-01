from __future__ import annotations
from dataclasses import dataclass
import numpy as np


def blend_edges(raw_delta: np.ndarray, dist_m: np.ndarray, flat_to: float, blend_to: float) -> np.ndarray:
    """Smoothstep-taper a delta to 0 as distance-to-edit grows from flat_to..blend_to. Full delta within
    flat_to, smoothly 0 by blend_to (NO cliffs). dist_m = metres to the edit geometry."""
    d = np.asarray(dist_m, dtype=np.float64)
    span = max(float(blend_to) - float(flat_to), 1e-9)
    t = np.clip((d - float(flat_to)) / span, 0.0, 1.0)
    blend = t * t * (3.0 - 2.0 * t)                 # smoothstep
    return np.asarray(raw_delta, dtype=np.float64) * (1.0 - blend)


def bound_depth(raw_delta: np.ndarray, cap_m: float) -> np.ndarray:
    """Bound a CUT delta to [-cap_m, 0] (cuts only; positive clamped to 0). Preserves peaks (no gouge past cap)."""
    return np.clip(np.asarray(raw_delta, dtype=np.float64), -float(cap_m), 0.0)


@dataclass(frozen=True)
class EditContext:
    """The window geometry every edit needs: world span (m), cell size (m), height scale (m), slope budget.
    No global state -- everything an edit needs to be world-local + seam-exact is here."""
    span_m: float
    cell_m: float
    height_scale_m: float
    slope_budget: float = 0.28


def combine(deltas, mode: str = "min") -> np.ndarray:
    """Composite multiple edit deltas. 'min' = deepest cut wins (carves), 'max' = highest fill wins (mounds),
    'sum' = additive. Deterministic."""
    stack = np.stack([np.asarray(d, dtype=np.float64) for d in deltas], axis=0)
    if mode == "min":
        return np.min(stack, axis=0)
    if mode == "max":
        return np.max(stack, axis=0)
    if mode == "sum":
        return np.sum(stack, axis=0)
    raise ValueError(f"combine: unknown mode {mode!r}")
