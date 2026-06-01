"""Slice 2A / 7B-lite skeleton-first geography prototype.

Offline only. This is the fork after the v5 review: build a coarse world-anchored
uplift/ridge skeleton, route flow on that skeleton, derive regimes from the routed
structure, then add local noise as material. Skeleton v2 replaces the coarse D8
accumulation with multi-flow routing and uses separate primary/tributary fields so
the 45 km view does not expose as many raster-aligned scars.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from scipy.ndimage import distance_transform_edt, gaussian_filter, maximum_filter, zoom

import geography_engine as geo
import worldgen_proto as wg


@dataclass(frozen=True)
class SkeletonScenario:
    key: str
    label: str
    uplift_gain: float = 1.0
    incision_gain: float = 1.0
    fill_gain: float = 1.0
    fan_gain: float = 1.0
    badlands_gain: float = 1.0
    range_texture: float = 1.0
    close_detail: float = 1.0
    channel_width: float = 1.0
    tributary_gain: float = 1.0
    basin_smoothing: float = 1.0
    range_spread: float = 1.0


SCENARIOS = (
    SkeletonScenario("skeleton_v2_base", "v2 base"),
    SkeletonScenario(
        "fan_aprons",
        "fan aprons",
        incision_gain=0.86,
        fill_gain=1.12,
        fan_gain=1.55,
        channel_width=1.34,
        basin_smoothing=1.22,
        range_spread=1.10,
    ),
    SkeletonScenario(
        "incised_badlands",
        "incised badlands",
        incision_gain=1.34,
        badlands_gain=1.62,
        close_detail=1.28,
        channel_width=0.78,
        tributary_gain=1.35,
    ),
    SkeletonScenario(
        "range_front",
        "range front",
        uplift_gain=1.24,
        fan_gain=1.18,
        range_texture=1.28,
        channel_width=0.92,
        range_spread=0.82,
    ),
    SkeletonScenario(
        "filled_basin",
        "filled basin",
        incision_gain=0.72,
        fill_gain=1.45,
        fan_gain=1.18,
        channel_width=1.22,
        basin_smoothing=1.42,
    ),
    SkeletonScenario(
        "rough_highlands",
        "rough highlands",
        uplift_gain=1.34,
        fill_gain=0.82,
        range_texture=1.58,
        close_detail=1.18,
        tributary_gain=0.84,
        range_spread=0.78,
    ),
)


def _resample(a: np.ndarray, shape: tuple[int, int], order: int = 3) -> np.ndarray:
    zy = shape[0] / a.shape[0]
    zx = shape[1] / a.shape[1]
    out = zoom(a, (zy, zx), order=order)
    return out[: shape[0], : shape[1]]


def _flow_accumulation_mfd(surface: np.ndarray, power: float = 1.45) -> np.ndarray:
    """Coarse-grid multiple-flow accumulation.

    D8 picked one neighbor and produced obvious diagonal/axis scars. This distributes flow across all
    downhill neighbors in proportion to slope, still on the coarse skeleton grid and never on final render
    pixels. It is not a full erosion model; it is just enough routed structure for render-first review.
    """
    # Plain-scalar inner loop (flat-indexed). ~2.5x faster than the prior per-cell numpy-array/np.sum version,
    # which spent its time in ~3M tiny np.sum/np.array allocations. Arithmetic differs only at float64 epsilon
    # (final composed height max diff ~6.7e-16, washed out by the downstream log1p/normalize/gaussian pipeline);
    # seam-exactness is unaffected (adjacent windows use the SAME function, so they still agree at the border).
    h = np.asarray(surface, dtype=np.float64)
    rows, cols = h.shape
    hflat = h.ravel()
    acc = np.ones(rows * cols, dtype=np.float64)
    order = np.argsort(-hflat)
    offsets = (
        (-1, -1, 1.41421356237),
        (-1, 0, 1.0),
        (-1, 1, 1.41421356237),
        (0, -1, 1.0),
        (0, 1, 1.0),
        (1, -1, 1.41421356237),
        (1, 0, 1.0),
        (1, 1, 1.41421356237),
    )
    p = float(power)
    for idx in order.tolist():
        y = idx // cols
        x = idx - y * cols
        hv = hflat[idx]
        targets: list[tuple[int, float]] = []
        total = 0.0
        for oy, ox, dist in offsets:
            ny = y + oy
            nx = x + ox
            if ny < 0 or ny >= rows or nx < 0 or nx >= cols:
                continue
            drop = (hv - hflat[ny * cols + nx]) / dist
            if drop > 0.0:
                wgt = drop ** p
                targets.append((ny * cols + nx, wgt))
                total += wgt
        if targets:
            scaled = acc[idx] / (total + 1e-12)
            for nidx, wgt in targets:
                acc[nidx] += wgt * scaled
    return acc.reshape(rows, cols)


def _skeleton_grid(wx: np.ndarray, wz: np.ndarray, coarse_n: int) -> tuple[np.ndarray, np.ndarray, float]:
    span = max(float(np.ptp(wx)), float(np.ptp(wz)), 1.0)
    ox = float(np.min(wx))
    oz = float(np.min(wz))
    cx, cz = geo.grid(coarse_n, span, ox=ox, oz=oz)
    spacing = span / max(coarse_n - 1, 1)
    return cx, cz, spacing


def build_coarse_skeleton(wx: np.ndarray, wz: np.ndarray, seed: int, coarse_n: int = 176) -> dict[str, np.ndarray | float]:
    cx, cz, spacing = _skeleton_grid(wx, wz, coarse_n)
    span = max(float(np.ptp(wx)), float(np.ptp(wz)), 1.0)
    x01 = (cx - float(np.min(cx))) / span
    z01 = (cz - float(np.min(cz))) / span

    w_x, w_z = wg.recursive_domain_warp(
        cx,
        cz,
        warp_amount=span * 0.16,
        warp_freq=1.0 / (span * 0.92),
        seed=seed + 700,
        steps=3,
        decay=0.56,
        freq_mul=1.8,
    )
    regional = geo.norm01(wg.fbm(w_x, w_z, 1.0 / (span * 0.95), 5, seed + 701, gain=0.57))
    ridge_long = wg.ridged_multifractal(w_x, w_z, 1.0 / (span * 0.52), 5, seed + 702, gain=0.56)
    ridge_mid = wg.ridged_multifractal(w_x, w_z, 1.0 / (span * 0.23), 4, seed + 703, gain=0.54)
    uplift = geo.norm01(0.55 * regional + 0.75 * ridge_long + 0.30 * ridge_mid)
    uplift = gaussian_filter(uplift, sigma=1.2)

    # A gentle world-anchored outlet potential prevents every coarse depression from becoming a terminal pit.
    outlet = -0.42 * x01 - 0.25 * z01 + 0.18 * wg.fbm(w_x, w_z, 1.0 / (span * 0.70), 3, seed + 704)
    basin_seed = geo.norm01((1.0 - uplift) * 0.80 + 0.35 * (1.0 - regional))
    routed_surface = geo.znorm(1.18 * uplift + 0.28 * ridge_mid - 0.46 * basin_seed + outlet)
    routed_surface = gaussian_filter(routed_surface, sigma=0.75)

    acc = _flow_accumulation_mfd(routed_surface)
    raw_discharge = geo.norm01(np.log1p(acc))
    discharge = geo.norm01(0.62 * gaussian_filter(raw_discharge, sigma=0.9) + 0.38 * gaussian_filter(raw_discharge, sigma=2.2))

    # Tributaries are lower-order flow corridors. They are softer than primary channels but still derived
    # from routed accumulation, not independent ridge noise.
    tributary = geo.norm01(gaussian_filter(geo.smoothstep(0.34, 0.78, discharge), sigma=1.6))

    crest_seed = geo.smoothstep(0.63, 0.88, uplift)
    local_max = uplift >= maximum_filter(uplift, size=7, mode="nearest") - 1e-6
    crest_mask = (crest_seed > 0.38) & local_max
    if not np.any(crest_mask):
        crest_mask = crest_seed > np.quantile(crest_seed, 0.88)
    channel_axis = geo.norm01(0.68 * geo.smoothstep(0.58, 0.96, discharge) + 0.32 * tributary)
    channel_centerline = channel_axis >= maximum_filter(channel_axis, size=5, mode="nearest") - 1e-6
    channel_mask = channel_centerline & (channel_axis > np.quantile(channel_axis, 0.58))
    if not np.any(channel_mask):
        channel_mask = channel_axis > np.quantile(channel_axis, 0.88)

    crest_dist = distance_transform_edt(~crest_mask) * spacing
    channel_dist = distance_transform_edt(~channel_mask) * spacing
    gy, gx = np.gradient(routed_surface, spacing, spacing)
    slope = geo.norm01(np.sqrt(gx * gx + gy * gy))
    drainage_density = geo.norm01(gaussian_filter(tributary, sigma=2.8))

    return {
        "uplift": uplift,
        "routed_surface": routed_surface,
        "discharge": discharge,
        "raw_discharge": raw_discharge,
        "flow_accum": acc,
        "tributary": tributary,
        "channel_axis": channel_axis,
        "crest_dist": crest_dist,
        "channel_dist": channel_dist,
        "slope": slope,
        "basin_seed": basin_seed,
        "drainage_density": drainage_density,
        "spacing": float(spacing),
        "span": float(span),
    }


def _fine_skeleton(wx: np.ndarray, wz: np.ndarray, seed: int, coarse_n: int) -> dict[str, np.ndarray | float]:
    coarse = build_coarse_skeleton(wx, wz, seed, coarse_n=coarse_n)
    shape = wx.shape
    out: dict[str, np.ndarray | float] = {"spacing": coarse["spacing"], "span": coarse["span"]}
    for key, value in coarse.items():
        if isinstance(value, np.ndarray):
            order = 1 if key in ("crest_dist", "channel_dist") else 3
            out[key] = _resample(value, shape, order=order)
    return out


def compose_height(
    wx: np.ndarray,
    wz: np.ndarray,
    seed: int,
    scenario: SkeletonScenario = SCENARIOS[0],
    coarse_n: int = 176,
) -> dict[str, np.ndarray | list[np.ndarray] | SkeletonScenario]:
    sk = _fine_skeleton(wx, wz, seed, coarse_n)
    span = float(sk["span"])
    uplift = np.asarray(sk["uplift"])
    discharge = np.asarray(sk["discharge"])
    crest_dist = np.asarray(sk["crest_dist"])
    channel_dist = np.asarray(sk["channel_dist"])
    slope = np.asarray(sk["slope"])
    basin_seed = np.asarray(sk["basin_seed"])
    drainage_density = np.asarray(sk["drainage_density"])
    tributary = np.asarray(sk["tributary"])
    channel_axis = np.asarray(sk["channel_axis"])

    crest_near = np.exp(-crest_dist / max(span * 0.105 * scenario.range_spread, 1.0))
    channel_near = np.exp(-channel_dist / max(span * 0.032 * scenario.channel_width, 1.0))
    basin = geo.smoothstep(0.42, 0.78, gaussian_filter(basin_seed, sigma=0.55 * scenario.basin_smoothing)) * (1.0 - 0.45 * crest_near)
    range_core = geo.smoothstep(0.58, 0.88, uplift) * (0.35 + 0.65 * crest_near)
    foothill = np.exp(-((crest_dist - span * 0.13 * scenario.range_spread) / max(span * 0.085, 1.0)) ** 2) * (0.45 + 0.55 * slope)
    plateau = geo.smoothstep(0.46, 0.78, uplift) * (1.0 - range_core) * (1.0 - 0.38 * basin)
    fan = channel_near * basin * geo.smoothstep(0.18, 0.58, slope) * (1.0 - geo.smoothstep(0.70, 0.94, uplift))
    badlands = drainage_density * scenario.tributary_gain * (0.35 + 0.65 * plateau + 0.35 * basin) * (1.0 - 0.35 * range_core)

    scores = [
        1.35 * basin * scenario.fill_gain,
        1.45 * fan * scenario.fan_gain,
        1.25 * foothill,
        1.18 * plateau,
        1.42 * range_core * scenario.uplift_gain,
        1.36 * badlands * scenario.badlands_gain,
    ]
    weights = geo.softmax(scores, temp=0.36)
    weights = geo.smooth_weights(weights, wx, wz)
    basin_w, fan_w, foothill_w, plateau_w, range_w, badlands_w = weights

    w_x, w_z = wg.recursive_domain_warp(
        wx,
        wz,
        warp_amount=span * 0.030,
        warp_freq=1.0 / (span * 0.45),
        seed=seed + 750,
        steps=2,
    )
    low = geo.znorm(wg.fbm(w_x, w_z, 1.0 / (span * 0.38), 4, seed + 751, gain=0.56))
    range_texture = geo.znorm(wg.ridged_multifractal(w_x, w_z, 1.0 / (span * 0.085), 5, seed + 752, gain=0.54))
    badland_texture = geo.znorm(wg.ridged_multifractal(w_x, w_z, 1.0 / (span * 0.040), 4, seed + 753, gain=0.50))
    fine = geo.znorm(wg.fbm(w_x, w_z, 1.0 / (span * 0.030), 4, seed + 754, gain=0.48))

    base = (
        1.45 * scenario.uplift_gain * uplift
        - 0.62 * scenario.fill_gain * basin
        + 0.26 * plateau
        + 0.10 * low
    )
    primary_width = max(span * 0.010 * scenario.channel_width, 1.0)
    tributary_width = max(span * 0.018 * scenario.channel_width, 1.0)
    primary_shape = np.exp(-(channel_dist / primary_width) ** 2)
    tributary_shape = np.exp(-(channel_dist / tributary_width) ** 2)
    primary = geo.smoothstep(0.56, 0.96, discharge) * (0.28 + 0.72 * primary_shape)
    tributary_cut = geo.smoothstep(0.34, 0.82, tributary) * (0.45 + 0.55 * tributary_shape) * (0.35 + 0.65 * slope)
    incision = scenario.incision_gain * (0.72 * primary + 0.34 * scenario.tributary_gain * tributary_cut)
    incision_context = np.clip(
        0.70 + 0.44 * badlands_w + 0.26 * foothill_w + 0.18 * range_w - 0.50 * basin_w - 0.35 * fan_w,
        0.18,
        1.18,
    )
    height = base - 0.38 * incision_context * incision

    # Regime material, added after the skeleton and causal incision.
    height += 0.32 * scenario.range_texture * range_w * range_texture
    height += 0.18 * foothill_w * range_texture
    height += 0.16 * scenario.fan_gain * fan_w * geo.znorm(gaussian_filter(channel_axis, sigma=3.0))
    height += 0.10 * plateau_w * low
    height += 0.28 * scenario.badlands_gain * badlands_w * (0.58 * badland_texture + 0.42 * fine)
    height += 0.10 * scenario.close_detail * (badlands_w + range_w + foothill_w) * fine
    height -= 0.06 * scenario.tributary_gain * (badlands_w + foothill_w + 0.35 * plateau_w) * tributary_cut
    if scenario.basin_smoothing > 1.0:
        basin_soft = np.clip(basin_w + 0.72 * fan_w, 0.0, 1.0)
        basin_fill = gaussian_filter(height, sigma=0.75 * scenario.basin_smoothing)
        mix = np.clip((scenario.basin_smoothing - 1.0) * 0.62 * basin_soft, 0.0, 0.48)
        height = height * (1.0 - mix) + basin_fill * mix

    height = np.tanh(height * 0.72)
    height = 0.72 * height + 0.28 * gaussian_filter(height, sigma=0.95)
    height = gaussian_filter(height, sigma=0.32)
    height = geo.znorm(height)

    return {
        "height": height,
        "weights": weights,
        "skeleton": sk,
        "scenario": scenario,
    }


def regime_rgb(weights: list[np.ndarray]) -> np.ndarray:
    return geo.regime_rgb(weights)


def straight_artifact_score(height: np.ndarray) -> float:
    return geo.straight_artifact_score(height)
