"""Offline Phase 7B-lite windowing spike for routed skeleton facts.

This is not runtime code. It tests the subsystem shape needed if the accepted
Slice 2A keeper depends on a routed coarse skeleton: fixed world-anchored
windows, apron/stitch behavior, and deterministic facts that fine pages can
sample later. The current review generator remains in geography_skeleton.py.
"""

from __future__ import annotations

from dataclasses import dataclass
from math import floor

import numpy as np
from scipy.ndimage import distance_transform_edt, gaussian_filter

import geography_engine as geo
import geography_skeleton as skel
import worldgen_proto as wg


@dataclass(frozen=True)
class SkeletonWindowSpec:
    core_span_m: float = 90000.0
    apron_m: float = 30000.0
    spacing_m: float = 1500.0
    route_power: float = 1.45


FACT_FIELDS = (
    "uplift",
    "routed_surface",
    "discharge",
    "tributary",
    "channel_axis",
    "crest_dist",
    "channel_dist",
)


def window_origin_for(x: float, z: float, spec: SkeletonWindowSpec = SkeletonWindowSpec()) -> tuple[float, float]:
    span = float(spec.core_span_m)
    return floor(float(x) / span) * span, floor(float(z) / span) * span


def _axis(start: float, spec: SkeletonWindowSpec) -> np.ndarray:
    total = float(spec.core_span_m) + 2.0 * float(spec.apron_m)
    count = int(round(total / float(spec.spacing_m))) + 1
    return float(start) - float(spec.apron_m) + np.arange(count, dtype=np.float64) * float(spec.spacing_m)


def _core_slice(spec: SkeletonWindowSpec) -> slice:
    start = int(round(float(spec.apron_m) / float(spec.spacing_m)))
    count = int(round(float(spec.core_span_m) / float(spec.spacing_m))) + 1
    return slice(start, start + count)


def _stable01(a: np.ndarray, gain: float = 1.0) -> np.ndarray:
    return np.clip(0.5 + 0.5 * np.tanh(np.asarray(a, dtype=np.float64) * float(gain)), 0.0, 1.0)


def _world_skeleton_surface(wx: np.ndarray, wz: np.ndarray, seed: int, spec: SkeletonWindowSpec) -> dict[str, np.ndarray]:
    """Build world-coordinate fields without per-window min/max normalization."""
    core = float(spec.core_span_m)
    w_x, w_z = wg.recursive_domain_warp(
        wx,
        wz,
        warp_amount=core * 0.13,
        warp_freq=1.0 / (core * 1.85),
        seed=seed + 9100,
        steps=3,
        decay=0.58,
        freq_mul=1.75,
    )
    regional = _stable01(wg.fbm(w_x, w_z, 1.0 / (core * 2.15), 5, seed + 9101), gain=1.12)
    ridge_long = wg.ridged_multifractal(w_x, w_z, 1.0 / (core * 0.72), 5, seed + 9102, gain=0.56)
    ridge_mid = wg.ridged_multifractal(w_x, w_z, 1.0 / (core * 0.34), 4, seed + 9103, gain=0.54)
    basin_seed = _stable01(-0.85 * regional + 0.45 * wg.fbm(w_x, w_z, 1.0 / (core * 1.10), 4, seed + 9104), gain=1.0)
    uplift = np.clip(0.38 * regional + 0.48 * ridge_long + 0.22 * ridge_mid - 0.14 * basin_seed, 0.0, 1.0)
    uplift = np.clip(gaussian_filter(uplift, sigma=1.0), 0.0, 1.0)

    # Absolute world-coordinate outlet potential keeps routed flow from depending purely on local window
    # edges. The period is intentionally much larger than one window.
    outlet = (
        -0.20 * (wx / max(core * 5.0, 1.0))
        - 0.14 * (wz / max(core * 5.0, 1.0))
        + 0.12 * wg.fbm(w_x, w_z, 1.0 / (core * 2.80), 3, seed + 9105)
    )
    routed_surface = 1.18 * uplift + 0.22 * ridge_mid - 0.34 * basin_seed + outlet
    routed_surface = gaussian_filter(routed_surface, sigma=0.70)
    return {
        "uplift": uplift,
        "routed_surface": routed_surface,
        "basin_seed": basin_seed,
    }


def build_skeleton_window(
    origin_x: float,
    origin_z: float,
    seed: int,
    spec: SkeletonWindowSpec = SkeletonWindowSpec(),
) -> dict[str, np.ndarray | float | tuple[float, float]]:
    xs = _axis(origin_x, spec)
    zs = _axis(origin_z, spec)
    wx, wz = np.meshgrid(xs, zs)
    base = _world_skeleton_surface(wx, wz, seed, spec)
    acc = skel._flow_accumulation_mfd(base["routed_surface"], power=spec.route_power)
    # Fixed scaling by maximum possible accumulation in this window, not local max. This avoids one window's
    # wettest basin remapping all discharges relative to its neighbor.
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    discharge = np.clip(0.62 * gaussian_filter(discharge, sigma=0.8) + 0.38 * gaussian_filter(discharge, sigma=2.0), 0.0, 1.0)
    tributary = np.clip(gaussian_filter(geo.smoothstep(0.12, 0.46, discharge), sigma=1.35), 0.0, 1.0)

    # For the window seam spike, centerline masks must be threshold-derived from world-coordinate facts.
    # Local maximum picking can choose different representatives on each side of a window boundary.
    crest_seed = geo.smoothstep(0.48, 0.74, base["uplift"])
    crest_mask = crest_seed > 0.28
    if not np.any(crest_mask):
        crest_mask = crest_seed > np.quantile(crest_seed, 0.88)

    channel_axis = np.clip(0.72 * geo.smoothstep(0.16, 0.62, discharge) + 0.28 * tributary, 0.0, 1.0)
    channel_center = channel_axis > 0.22
    if not np.any(channel_center):
        channel_center = channel_axis > np.quantile(channel_axis, 0.88)

    # Distance facts are only authoritative inside the apron-valid band. Outside that band, the fact should
    # saturate to "far" instead of leaking incomplete-window context into fine pages.
    max_fact_dist = float(spec.apron_m) * 0.68
    crest_dist = np.minimum(distance_transform_edt(~crest_mask) * float(spec.spacing_m), max_fact_dist)
    channel_dist = np.minimum(distance_transform_edt(~channel_center) * float(spec.spacing_m), max_fact_dist)
    return {
        "origin": (float(origin_x), float(origin_z)),
        "wx": wx,
        "wz": wz,
        "uplift": base["uplift"],
        "routed_surface": base["routed_surface"],
        "discharge": discharge,
        "tributary": tributary,
        "channel_axis": channel_axis,
        "crest_dist": crest_dist,
        "channel_dist": channel_dist,
    }


def core_facts(window: dict[str, np.ndarray | float | tuple[float, float]], spec: SkeletonWindowSpec = SkeletonWindowSpec()) -> dict[str, np.ndarray]:
    core = _core_slice(spec)
    return {field: np.asarray(window[field])[core, core] for field in FACT_FIELDS}


def corridor_mask(
    facts: dict[str, np.ndarray],
    spec: SkeletonWindowSpec = SkeletonWindowSpec(),
    channel_axis_threshold: float = 0.22,
    channel_distance_m: float | None = None,
) -> np.ndarray:
    """Return a coarse routed-corridor mask from window facts.

    This is a seam/review heuristic, not a final gameplay route map. It follows
    the same world-anchored channel facts that a future runtime fine page would
    sample.
    """
    distance_m = float(channel_distance_m) if channel_distance_m is not None else float(spec.spacing_m) * 2.0
    return (np.asarray(facts["channel_axis"]) >= float(channel_axis_threshold)) | (np.asarray(facts["channel_dist"]) <= distance_m)


def _edge_match_count(source_edge: np.ndarray, target_band: np.ndarray, row_tolerance: int) -> int:
    source = np.asarray(source_edge, dtype=bool)
    target = np.asarray(target_band, dtype=bool)
    matches = 0
    for row, enters in enumerate(source):
        if not enters:
            continue
        lo = max(0, row - int(row_tolerance))
        hi = min(target.shape[0], row + int(row_tolerance) + 1)
        if bool(np.any(target[lo:hi, :])):
            matches += 1
    return matches


def adjacent_corridor_continuity(
    seed: int,
    spec: SkeletonWindowSpec = SkeletonWindowSpec(),
    origin_x: float = 0.0,
    origin_z: float = 0.0,
    axis: str = "x",
    band_px: int = 2,
    row_tolerance_px: int = 1,
) -> dict[str, float | int]:
    """Measure whether routed corridors entering a seam continue in the neighbor."""
    if axis not in ("x", "z"):
        raise ValueError("axis must be 'x' or 'z'")
    a = core_facts(build_skeleton_window(origin_x, origin_z, seed, spec), spec)
    if axis == "x":
        b = core_facts(build_skeleton_window(origin_x + spec.core_span_m, origin_z, seed, spec), spec)
        ma = corridor_mask(a, spec)
        mb = corridor_mask(b, spec)
        a_edge = ma[:, -1]
        b_edge = mb[:, 0]
        a_matches = _edge_match_count(a_edge, mb[:, : band_px + 1], row_tolerance_px)
        b_matches = _edge_match_count(b_edge, ma[:, -band_px - 1 :], row_tolerance_px)
    else:
        b = core_facts(build_skeleton_window(origin_x, origin_z + spec.core_span_m, seed, spec), spec)
        ma = corridor_mask(a, spec).T
        mb = corridor_mask(b, spec).T
        a_edge = ma[:, -1]
        b_edge = mb[:, 0]
        a_matches = _edge_match_count(a_edge, mb[:, : band_px + 1], row_tolerance_px)
        b_matches = _edge_match_count(b_edge, ma[:, -band_px - 1 :], row_tolerance_px)
    entering = int(np.count_nonzero(a_edge) + np.count_nonzero(b_edge))
    matched = int(a_matches + b_matches)
    unmatched = int(max(0, entering - matched))
    return {
        "corridor_entering_count": entering,
        "corridor_matched_count": matched,
        "corridor_unmatched_count": unmatched,
        "corridor_match_frac": float(matched / entering) if entering else 1.0,
    }


def adjacent_seam_deltas(
    seed: int,
    spec: SkeletonWindowSpec = SkeletonWindowSpec(),
    origin_x: float = 0.0,
    origin_z: float = 0.0,
    axis: str = "x",
) -> dict[str, float]:
    if axis not in ("x", "z"):
        raise ValueError("axis must be 'x' or 'z'")
    a = core_facts(build_skeleton_window(origin_x, origin_z, seed, spec), spec)
    if axis == "x":
        b = core_facts(build_skeleton_window(origin_x + spec.core_span_m, origin_z, seed, spec), spec)
        return {field: float(np.max(np.abs(a[field][:, -1] - b[field][:, 0]))) for field in FACT_FIELDS}
    b = core_facts(build_skeleton_window(origin_x, origin_z + spec.core_span_m, seed, spec), spec)
    return {field: float(np.max(np.abs(a[field][-1, :] - b[field][0, :]))) for field in FACT_FIELDS}
