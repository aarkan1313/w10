from __future__ import annotations
from dataclasses import dataclass
import numpy as np
from scipy.ndimage import gaussian_filter

import geography_engine as geo
import geography_skeleton_windows as win
import worldgen_proto as wg


def apron_blur_crop(field_with_apron: np.ndarray, apron_px: int, sigma: float, truncate: float = 4.0) -> np.ndarray:
    """Gaussian-blur an apron-padded window, then crop to the authoritative core (all axes).

    The blur reads only samples inside the apron-padded extent, and `mode='nearest'` invents no
    out-of-window samples (unlike 'wrap'/'reflect'/'constant'), so the cropped CORE is bit-identical
    across adjacent windows that share those samples — as long as the kernel's half-width fits in the
    apron. scipy's kernel half-width is int(truncate*sigma + 0.5); we pass the SAME truncate to the
    filter and the guard so they cannot disagree.
    """
    a = int(apron_px)
    reach = int(np.floor(float(truncate) * float(sigma) + 0.5))  # matches gaussian_filter's kernel half-width
    if reach > a:
        raise ValueError(f"apron_blur_crop: kernel reach {reach}px (truncate {truncate}*sigma {sigma}) exceeds apron {a}px (would break seams)")
    blurred = gaussian_filter(np.asarray(field_with_apron, dtype=np.float64), sigma=float(sigma),
                              mode="nearest", truncate=float(truncate))
    return blurred[a:-a, a:-a] if a > 0 else blurred

def affine_remap(field: np.ndarray, center: float, scale: float) -> np.ndarray:
    """Data-independent remap (replaces znorm). Same (center,scale) every window => shared
    borders stay bit-identical. center/scale are tunable constants, NOT per-array statistics."""
    return (np.asarray(field, dtype=np.float64) - float(center)) * float(scale)

@dataclass(frozen=True)
class KeeperV2Params:
    softmax_temp: float = 0.36          # A's regime softmax temperature
    relief_amplitude: float = 2.0       # overall vertical gain; flat-guard gate floor (slope_p90 >= MIN_STRUCTURAL_SLOPE_P90 at 200x preset) sets this default
    incision_gain: float = 1.0
    range_texture_gain: float = 0.32
    badland_gain: float = 0.28
    fine_gain: float = 0.10
    blur_radius_m: float = 950.0        # final shaping blur
    weight_blur_m: float = 1700.0       # smooth_weights blur radius
    remap_center: float = 0.0           # affine remap (replaces znorm); tune to match A's tone
    remap_scale: float = 1.0
    slope_norm_scale: float = 2941.0    # routed-surface slope -> ~[0,1] without per-window stats (seam-safe; see Task 5)


def apron_blur_crop_full(field_full: np.ndarray, apron_px: int, sigma: float, truncate: float = 4.0) -> np.ndarray:
    """Apron-aware blur returning FULL extent (no crop). Reach must fit the apron so the core region
    is seam-identical once cropped at the end of compose. Guard uses scipy's ACTUAL kernel half-width
    int(truncate*sigma+0.5) and passes the same truncate to the filter (NOT 3*sigma — scipy truncates
    at 4*sigma by default; a loose guard silently breaks seams)."""
    reach = int(np.floor(float(truncate) * float(sigma) + 0.5))
    if reach > int(apron_px):
        raise ValueError(f"apron_blur_crop_full: kernel reach {reach}px exceeds apron {apron_px}px")
    return gaussian_filter(np.asarray(field_full, dtype=np.float64), sigma=float(sigma), mode="nearest", truncate=float(truncate))


def _regime_weights(facts, spec, p, apron_px):
    span = float(spec.core_span_m); spacing = float(spec.spacing_m)
    uplift = facts["uplift"]; discharge = facts["discharge"]; tributary = facts["tributary"]
    channel_axis = facts["channel_axis"]; crest_dist = facts["crest_dist"]; channel_dist = facts["channel_dist"]
    routed = facts["routed_surface"]
    gy, gx = np.gradient(routed, spacing, spacing)
    # SEAM-EXACTNESS: these three normalizations must be DATA-INDEPENDENT. geo.norm01 uses a
    # per-window global min/max (a.min()/ptp) which differs between adjacent windows, breaking the
    # shared-border bit-identity that the apron design guarantees. Use fixed clip/affine instead.
    # slope: routed-surface gradient magnitude (tiny, ~<=3.4e-4 over a chunk); a fixed scale maps the
    # typical max to ~1 and clips, preserving the prior [0,1] dynamic range without reading window stats.
    slope = np.clip(np.sqrt(gx * gx + gy * gy) * p.slope_norm_scale, 0.0, 1.0)
    basin_seed = np.clip(1.0 - uplift, 0.0, 1.0)              # uplift is already ~[0,1]
    drainage_density = np.clip(apron_blur_crop_full(tributary, apron_px, 2.8), 0.0, 1.0)  # blurred tributary already ~[0,1]
    crest_near = np.exp(-crest_dist / max(span * 0.105, 1.0))
    channel_near = np.exp(-channel_dist / max(span * 0.032, 1.0))
    basin = geo.smoothstep(0.42, 0.78, basin_seed) * (1.0 - 0.45 * crest_near)
    range_core = geo.smoothstep(0.58, 0.88, uplift) * (0.35 + 0.65 * crest_near)
    foothill = np.exp(-((crest_dist - span * 0.13) / max(span * 0.085, 1.0)) ** 2) * (0.45 + 0.55 * slope)
    plateau = geo.smoothstep(0.46, 0.78, uplift) * (1.0 - range_core) * (1.0 - 0.38 * basin)
    fan = channel_near * basin * geo.smoothstep(0.18, 0.58, slope) * (1.0 - geo.smoothstep(0.70, 0.94, uplift))
    badlands = drainage_density * (0.35 + 0.65 * plateau + 0.35 * basin) * (1.0 - 0.35 * range_core)
    scores = [1.35 * basin, 1.45 * fan, 1.25 * foothill, 1.18 * plateau, 1.42 * range_core, 1.36 * badlands]
    weights = geo.softmax(scores, temp=p.softmax_temp)
    sigma = max(p.weight_blur_m / spacing, 0.1)
    sm = [apron_blur_crop_full(w_, apron_px, sigma) for w_ in weights]
    total = np.sum(np.stack(sm, axis=0), axis=0) + 1e-9
    weights = [np.clip(w_ / total, 0.0, 1.0) for w_ in sm]
    return weights, slope, basin, range_core, plateau, channel_axis


def compose_windowed_height_v2(window, seed, spec, p):
    apron_px = int(round(float(spec.apron_m) / float(spec.spacing_m)))
    span = float(spec.core_span_m); spacing = float(spec.spacing_m)
    facts = {k: np.asarray(window[k], dtype=np.float64) for k in
             ("uplift","routed_surface","discharge","tributary","channel_axis","crest_dist","channel_dist")}
    weights, slope, basin, range_core, plateau, channel_axis = _regime_weights(facts, spec, p, apron_px)
    basin_w, fan_w, foothill_w, plateau_w, range_w, badlands_w = weights
    uplift = facts["uplift"]; discharge = facts["discharge"]; tributary = facts["tributary"]
    channel_dist = facts["channel_dist"]
    wx = np.asarray(window["wx"]); wz = np.asarray(window["wz"])
    w_x, w_z = wg.recursive_domain_warp(wx, wz, warp_amount=span*0.030, warp_freq=1.0/(span*0.45), seed=seed+750, steps=2)
    low = affine_remap(wg.fbm(w_x, w_z, 1.0/(span*0.38), 4, seed+751, gain=0.56), p.remap_center, 1.0)
    range_texture = affine_remap(wg.ridged_multifractal(w_x, w_z, 1.0/(span*0.085), 5, seed+752, gain=0.54), 0.5, 1.0)
    badland_texture = affine_remap(wg.ridged_multifractal(w_x, w_z, 1.0/(span*0.040), 4, seed+753, gain=0.50), 0.5, 1.0)
    fine = affine_remap(wg.fbm(w_x, w_z, 1.0/(span*0.030), 4, seed+754, gain=0.48), p.remap_center, 1.0)
    base = 1.45*uplift - 0.62*basin + 0.26*plateau + 0.10*low
    primary_shape = np.exp(-(channel_dist / max(span*0.010, 1.0))**2)
    tributary_shape = np.exp(-(channel_dist / max(span*0.018, 1.0))**2)
    primary = geo.smoothstep(0.56, 0.96, discharge) * (0.28 + 0.72*primary_shape)
    tributary_cut = geo.smoothstep(0.34, 0.82, tributary) * (0.45 + 0.55*tributary_shape) * (0.35 + 0.65*slope)
    incision = p.incision_gain * (0.72*primary + 0.34*tributary_cut)
    incision_context = np.clip(0.70 + 0.44*badlands_w + 0.26*foothill_w + 0.18*range_w - 0.50*basin_w - 0.35*fan_w, 0.18, 1.18)
    height = base - 0.38*incision_context*incision
    height = height + p.range_texture_gain * range_w * range_texture
    height = height + 0.18 * foothill_w * range_texture
    height = height + 0.16 * fan_w * apron_blur_crop_full(channel_axis, apron_px, 3.0)
    height = height + 0.10 * plateau_w * low
    height = height + p.badland_gain * badlands_w * (0.58*badland_texture + 0.42*fine)
    height = height + p.fine_gain * (badlands_w + range_w + foothill_w) * fine
    height = height - 0.06 * (badlands_w + foothill_w + 0.35*plateau_w) * tributary_cut
    height = height * p.relief_amplitude
    height = np.tanh(height * 0.72)
    sigma_final = max(p.blur_radius_m / spacing, 0.1)
    height = 0.72*height + 0.28*apron_blur_crop_full(height, apron_px, sigma_final)
    height = apron_blur_crop_full(height, apron_px, max(0.32*sigma_final/0.95, 0.1))
    height = affine_remap(height, p.remap_center, p.remap_scale)
    core = win._core_slice(spec)
    return np.ascontiguousarray(height[core, core])
