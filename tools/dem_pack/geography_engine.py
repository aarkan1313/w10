"""Slice 2A geography-engine prototype.

Offline only. This module deliberately separates coarse landform organization from local
roughness. Noise is used as material inside landform regimes, not as the whole terrain story.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from scipy.ndimage import gaussian_filter

import worldgen_proto as wg


@dataclass(frozen=True)
class GeographyScenario:
    key: str
    label: str
    range_gain: float = 1.0
    basin_gain: float = 1.0
    plateau_gain: float = 1.0
    foothill_gain: float = 1.0
    fan_gain: float = 1.0
    badlands_gain: float = 0.35
    channel_gain: float = 0.45
    detail_gain: float = 0.065


SCENARIOS = (
    GeographyScenario("basin_range", "basin/range v0", range_gain=1.25, basin_gain=1.05, fan_gain=1.15),
    GeographyScenario("range_front", "range front v0", range_gain=1.55, basin_gain=0.85, foothill_gain=1.25),
    GeographyScenario("incised_plateau", "incised plateau v0", range_gain=0.55, plateau_gain=1.55, badlands_gain=1.1, channel_gain=0.62),
    GeographyScenario("foothill_fans", "foothill fans v0", range_gain=1.05, foothill_gain=1.45, fan_gain=1.55, basin_gain=1.1),
    GeographyScenario("soft_lowlands", "soft lowlands v0", range_gain=0.35, basin_gain=1.55, plateau_gain=0.55, channel_gain=0.25, detail_gain=0.03),
    GeographyScenario("badlands_mix", "badlands mix v0", range_gain=0.75, plateau_gain=1.25, badlands_gain=1.55, channel_gain=0.78, detail_gain=0.10),
)

REGIME_NAMES = ("basin", "fan", "foothill", "plateau", "range", "badlands")
REGIME_COLORS = np.array(
    [
        [45, 82, 145],
        [92, 148, 74],
        [181, 144, 74],
        [126, 102, 66],
        [174, 174, 174],
        [158, 86, 54],
    ],
    dtype=np.float64,
)


def grid(n: int, span_m: float, ox: float = 0.0, oz: float = 0.0) -> tuple[np.ndarray, np.ndarray]:
    ii = np.linspace(0.0, float(span_m), int(n))
    return np.meshgrid(ii + float(ox), ii + float(oz))


def norm01(a: np.ndarray) -> np.ndarray:
    a = np.asarray(a, dtype=np.float64)
    return (a - float(a.min())) / (float(np.ptp(a)) + 1e-9)


def znorm(a: np.ndarray) -> np.ndarray:
    a = np.asarray(a, dtype=np.float64)
    return (a - float(a.mean())) / (float(a.std()) + 1e-9)


def smoothstep(edge0: float, edge1: float, x: np.ndarray) -> np.ndarray:
    t = np.clip((x - float(edge0)) / (float(edge1) - float(edge0) + 1e-9), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def softmax(scores: list[np.ndarray], temp: float = 0.35) -> list[np.ndarray]:
    stack = np.stack(scores, axis=0) / float(temp)
    stack = stack - np.max(stack, axis=0, keepdims=True)
    e = np.exp(stack)
    e = e / (np.sum(e, axis=0, keepdims=True) + 1e-9)
    return [e[i] for i in range(e.shape[0])]


def _spacing_m(wx: np.ndarray) -> float:
    if wx.shape[1] < 2:
        return 1.0
    return max(float(wx[0, 1] - wx[0, 0]), 1.0)


def _span_m(wx: np.ndarray, wz: np.ndarray) -> float:
    return max(float(np.ptp(wx)), float(np.ptp(wz)), 1.0)


def _sigma_for(wx: np.ndarray, metres: float) -> float:
    return max(float(metres) / _spacing_m(wx), 0.1)


def _warped_world(wx: np.ndarray, wz: np.ndarray, seed: int) -> tuple[np.ndarray, np.ndarray]:
    return wg.recursive_domain_warp(
        wx,
        wz,
        warp_amount=26000.0,
        warp_freq=1.0 / 185000.0,
        seed=seed + 1100,
        steps=3,
        decay=0.58,
        freq_mul=1.75,
    )


def coarse_fields(wx: np.ndarray, wz: np.ndarray, seed: int) -> dict[str, np.ndarray]:
    """Build irregular world-scale fields without explicit straight segment primitives."""
    w_x, w_z = _warped_world(wx, wz, seed)

    regional = norm01(wg.fbm(w_x, w_z, 1.0 / 210000.0, 5, seed + 1, gain=0.56))
    range_ridge = wg.ridged_multifractal(w_x, w_z, 1.0 / 105000.0, 5, seed + 2, gain=0.57)
    range_ridge = norm01(0.70 * range_ridge + 0.30 * wg.ridged_multifractal(w_x, w_z, 1.0 / 65000.0, 4, seed + 3, gain=0.55))
    broad_range = smoothstep(0.46, 0.82, range_ridge)
    broad_range = norm01(gaussian_filter(broad_range, sigma=_sigma_for(wx, 9000.0)))
    sharp_range = smoothstep(0.72, 0.94, range_ridge)

    plateau_seed = norm01(wg.fbm(w_x, w_z, 1.0 / 125000.0, 5, seed + 4, gain=0.58))
    plateau = smoothstep(0.55, 0.82, plateau_seed) * (1.0 - 0.45 * broad_range)
    plateau = norm01(gaussian_filter(plateau, sigma=_sigma_for(wx, 6000.0)))

    basin_seed = norm01((1.0 - regional) * 0.70 + (1.0 - broad_range) * 0.55 + 0.18 * wg.fbm(w_x, w_z, 1.0 / 80000.0, 3, seed + 5))
    basin = smoothstep(0.45, 0.82, basin_seed)
    basin = norm01(gaussian_filter(basin, sigma=_sigma_for(wx, 8500.0)))

    edge = smoothstep(0.18, 0.50, broad_range) * (1.0 - smoothstep(0.62, 0.90, broad_range))
    edge = norm01(gaussian_filter(edge, sigma=_sigma_for(wx, 4200.0)))

    coarse = znorm(1.35 * broad_range + 0.55 * sharp_range + 0.48 * plateau - 0.72 * basin + 0.20 * (regional - 0.5))
    span = _span_m(wx, wz)
    flow_seed = gaussian_filter(coarse, sigma=_sigma_for(wx, 1600.0 if span > 70000.0 else 1100.0))
    flow_seed = flow_seed + 0.030 * wg.fbm(wx, wz, 1.0 / 5200.0, 3, seed + 6)
    flow = wg.flow_accumulation_channels(flow_seed, power=0.58)
    channel_blur_m = 1450.0 if span > 70000.0 else 500.0
    flow_smooth = gaussian_filter(flow, sigma=_sigma_for(wx, channel_blur_m))
    valley_texture = wg.ridged_multifractal(w_x, w_z, 1.0 / 32000.0, 5, seed + 7, gain=0.55)
    flow_mix = 0.28 if span > 70000.0 else 0.18
    channels = norm01(flow_mix * flow_smooth + (1.0 - flow_mix) * valley_texture * (0.55 + 0.45 * basin))
    channels = smoothstep(0.48, 0.88, gaussian_filter(channels, sigma=_sigma_for(wx, 420.0 if span <= 70000.0 else 900.0)))
    alluvial = norm01(gaussian_filter(channels * (0.55 * edge + 0.45 * basin), sigma=_sigma_for(wx, 4500.0)))
    badlands = norm01(gaussian_filter(channels * (0.45 + 0.55 * plateau), sigma=_sigma_for(wx, 1300.0)))

    return {
        "regional": regional,
        "range": broad_range,
        "sharp_range": sharp_range,
        "plateau": plateau,
        "basin": basin,
        "foothill": edge,
        "channels": channels,
        "alluvial": alluvial,
        "badlands": badlands,
        "coarse": coarse,
        "warped_x": w_x,
        "warped_z": w_z,
    }


def regime_weights(fields: dict[str, np.ndarray], scenario: GeographyScenario) -> list[np.ndarray]:
    basin = fields["basin"]
    fan = fields["alluvial"]
    foothill = fields["foothill"]
    plateau = fields["plateau"]
    ranges = fields["range"]
    badlands = fields["badlands"]
    channels = fields["channels"]

    scores = [
        scenario.basin_gain * (1.25 * basin + 0.18 * (1.0 - ranges)),
        scenario.fan_gain * (1.35 * fan + 0.32 * foothill + 0.15 * channels),
        scenario.foothill_gain * (1.25 * foothill + 0.22 * ranges),
        scenario.plateau_gain * (1.12 * plateau + 0.20 * (1.0 - basin)),
        scenario.range_gain * (1.35 * ranges + 0.55 * fields["sharp_range"]),
        scenario.badlands_gain * (1.20 * badlands + 0.28 * channels + 0.25 * plateau),
    ]
    return softmax(scores, temp=0.42)


def smooth_weights(weights: list[np.ndarray], wx: np.ndarray, wz: np.ndarray) -> list[np.ndarray]:
    span = _span_m(wx, wz)
    sigma = _sigma_for(wx, 1700.0 if span <= 70000.0 else 4200.0)
    smoothed = [gaussian_filter(w, sigma=sigma) for w in weights]
    total = np.sum(np.stack(smoothed, axis=0), axis=0) + 1e-9
    return [np.clip(w / total, 0.0, 1.0) for w in smoothed]


def compose_height(
    wx: np.ndarray,
    wz: np.ndarray,
    seed: int,
    scenario: GeographyScenario = SCENARIOS[0],
) -> dict[str, np.ndarray | list[np.ndarray] | GeographyScenario]:
    fields = coarse_fields(wx, wz, seed)
    weights = smooth_weights(regime_weights(fields, scenario), wx, wz)
    basin_w, fan_w, foothill_w, plateau_w, range_w, badlands_w = weights
    w_x = fields["warped_x"]
    w_z = fields["warped_z"]

    low = znorm(wg.fbm(w_x, w_z, 1.0 / 90000.0, 5, seed + 20, gain=0.56))
    mid = znorm(wg.fbm(w_x, w_z, 1.0 / 28000.0, 4, seed + 21, gain=0.52))
    fine = znorm(wg.fbm(w_x, w_z, 1.0 / 6200.0, 4, seed + 22, gain=0.50))
    near = znorm(wg.fbm(w_x, w_z, 1.0 / 2400.0, 3, seed + 24, gain=0.46))
    ridge_detail = znorm(wg.ridged_multifractal(w_x, w_z, 1.0 / 18000.0, 5, seed + 23, gain=0.55))
    small_ridges = znorm(wg.ridged_multifractal(w_x, w_z, 1.0 / 5200.0, 4, seed + 25, gain=0.52))

    ranges = fields["range"]
    sharp = fields["sharp_range"]
    plateau = fields["plateau"]
    basin = fields["basin"]
    channels = fields["channels"]
    alluvial = fields["alluvial"]
    badlands = fields["badlands"]

    range_h = 1.55 * ranges + 0.78 * sharp + 0.30 * ridge_detail + 0.12 * mid
    foothill_h = 0.62 * ranges + 0.35 * fields["foothill"] + 0.18 * ridge_detail + 0.12 * low
    fan_h = -0.20 + 0.36 * fields["foothill"] + 0.30 * alluvial + 0.06 * low - 0.16 * channels
    basin_h = -0.58 + 0.24 * low + 0.08 * fine - 0.24 * basin - 0.12 * channels
    plateau_h = 0.50 + 0.45 * plateau + 0.12 * low - 0.22 * channels
    badlands_h = 0.24 * plateau + 0.15 * mid - 0.82 * badlands - 0.40 * channels + 0.10 * ridge_detail

    height = (
        basin_w * basin_h
        + fan_w * fan_h
        + foothill_w * foothill_h
        + plateau_w * plateau_h
        + range_w * range_h
        + badlands_w * badlands_h
    )
    carve_mask = 0.24 + 0.50 * badlands_w + 0.22 * plateau_w + 0.12 * range_w
    height = height - scenario.channel_gain * carve_mask * channels
    height = height + 0.18 * fan_w * alluvial
    detail_mask = 0.18 + 0.55 * range_w + 0.32 * plateau_w + 0.22 * badlands_w + 0.18 * foothill_w
    height = height + scenario.detail_gain * detail_mask * (0.50 * ridge_detail + 0.28 * small_ridges + 0.22 * fine)

    # Secondary close-scale incision: use warped ridge/valley texture, not D8 raster flow. The first v1/v2
    # flow-accumulation branch made some convincing detail, but it also exposed vertical/diagonal raster scars.
    span = _span_m(wx, wz)
    close_weight = 1.0 - smoothstep(70000.0, 135000.0, np.array(span))
    branch_x = w_x + 900.0 * wg.fbm(w_x, w_z, 1.0 / 9000.0, 3, seed + 60)
    branch_z = w_z + 900.0 * wg.fbm(w_x, w_z, 1.0 / 9000.0, 3, seed + 61)
    branch_texture = wg.ridged_multifractal(branch_x, branch_z, 1.0 / 3600.0, 4, seed + 62, gain=0.50)
    branches = norm01(0.68 * branch_texture + 0.20 * norm01(near) + 0.12 * norm01(small_ridges))
    branches = smoothstep(0.52, 0.88, gaussian_filter(branches, sigma=_sigma_for(wx, 120.0)))
    branch_mask = 0.08 + 0.38 * badlands_w + 0.18 * plateau_w + 0.16 * foothill_w + 0.10 * range_w
    height = height - close_weight * (0.11 + 0.06 * scenario.channel_gain) * branch_mask * branches
    height = height + close_weight * (0.065 + scenario.detail_gain * 0.55) * detail_mask * (0.55 * near + 0.45 * small_ridges)

    # Soft-limit extreme prototype cliffs before normalization. This is not hiding a seam; it is a cheap
    # version of the slope/relief moderation a runtime geography stack will need before scene review.
    height = np.tanh(height * 0.78)

    smooth_m = 170.0 if _span_m(wx, wz) <= 70000.0 else 320.0
    height = gaussian_filter(height, sigma=_sigma_for(wx, smooth_m))
    height = znorm(height)

    return {
        "height": height,
        "weights": weights,
        "fields": fields,
        "branches": branches,
        "scenario": scenario,
    }


def regime_rgb(weights: list[np.ndarray]) -> np.ndarray:
    rgb = np.zeros((*weights[0].shape, 3), dtype=np.float64)
    for i, w in enumerate(weights):
        rgb += w[..., None] * REGIME_COLORS[i]
    return np.clip(rgb, 0, 255).astype(np.uint8)


def straight_artifact_score(height: np.ndarray) -> float:
    """Cheap red-flag score for axis-aligned/ruler-like gradients.

    This is intentionally conservative: it catches obvious grid/segment artifacts, but cannot prove terrain is
    natural. The owner image review remains authoritative.
    """
    gy, gx = np.gradient(np.asarray(height, dtype=np.float64))
    mag = np.sqrt(gx * gx + gy * gy)
    if float(np.max(mag)) <= 1e-12:
        return 1.0
    strong = mag > np.quantile(mag, 0.90)
    axisish = (np.abs(gx) < 0.08 * (np.abs(gy) + 1e-9)) | (np.abs(gy) < 0.08 * (np.abs(gx) + 1e-9))
    return float(np.mean(axisish[strong])) if np.any(strong) else 0.0
