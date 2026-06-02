"""Glacial-only reference-kernel synthesis experiments.

This follows the mountain promotion pattern, but keeps glacial as a separate
biome problem. Glacial refs should read as smoothed ice-carved terrain: broad
U-shaped troughs, high ridge/icefield walls, and less noisy valley floors.

Seam-safety (apron_px > 0 path)
--------------------------------
When ``apron_px > 0``, ``generate`` expects ``wx``/``wz`` grids that are already
padded by ``apron_px`` cells of real world-coordinates on every side.  It
computes on the full padded array, then crops to the core before returning.

Rules that guarantee seam-exactness:
1. All ``gaussian_filter`` calls use ``mode='nearest'``.
2. Data-dependent normalisation (``zscore``, ``norm01``) is replaced by
   ``seam_safe.affine_remap`` with fixed constants (never per-window statistics).
3. Rotation uses a FIXED world origin (0, 0) -- not the per-window midpoint,
   which is data-dependent.
4. Trough carving uses REAL multiple-flow-direction (MFD) accumulation
   (``geography_skeleton._flow_accumulation_mfd``) with a FIXED-max normalisation
   (``log1p(acc) / log1p(acc.size)`` -- data-independent, NOT per-window max).
   Global flow accumulation is not bit-exact on a finite apron, but it CONVERGES
   to bit-exact as the apron grows: see mountain_synthesis docstring for the
   probe measurements that motivated apron 160.
5. A single crop ``height[a:-a, a:-a]`` is performed at the very end.

Required apron (computed as sum of chained kernel reaches along the deepest path):
  - _oriented_relief: norm blur sigma=1.25 -> reach 5; envelope blur sigma=5.8 -> reach 23;
    icefield blur sigma=7.0 -> reach 28; massif blur sigma=2.8 -> reach 11
    deepest chain: 5+28 = 33 (icefield reads from regional+envelope, envelope reads relief)
  - base -> primary flow: pre-blur sigma=1.85 -> reach 8; width blur sigma<=7.8 -> reach 31
    (MFD convergence dominates blur-reach); chain = 33+8+31 = 72
  - branch: reads primary_mask (depth 72); floor/ice blur sigma<=8.4 -> reach 34; chain = 72+34 = 106
  - final blend blur sigma=1.35 -> reach 6; total = 106+6 = 112 (blur-reach budget)

The blur-reach budget alone is ~112, BUT the dominant residual is the REAL
flow-accumulation convergence error, which is SCALE-DEPENDENT (same finding as
mountain). So: -> ``GLACIAL_APRON_PX = 160`` -- matches mountain/grassland's
calibrated floor (see mountain_synthesis docstring for the 7x7 / 175 km world
measurement). Larger troughs (ice_smooth_px up to 8.4) and thicker trough blurs
don't exceed 160 since blur-reach (112) is well under 160; the MFD residual at
apron 160 is the binding constraint (not blur-reach).
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from scipy.ndimage import gaussian_filter

import worldgen_proto as wg
import geography_skeleton as skel
import seam_safe as ss


# ---------------------------------------------------------------------------
# Apron constant -- how many cells of world-coord padding each side the caller
# must supply when calling generate(apron_px=GLACIAL_APRON_PX).
# See module docstring for the derivation.
# ---------------------------------------------------------------------------
GLACIAL_APRON_PX: int = 160

# ---------------------------------------------------------------------------
# Affine-remap constants (replace per-window zscore / norm01).
# Chosen from empirical statistics across seeds 0, 7, 42, 31, 99 on 96x96 / 90 km grids.
# Target: same STRUCTURE and similar tone to the original zscore/norm01 output.
#
# norm01(x) -> affine_remap(x, center=mean_min, scale=1/mean_ptp)
# zscore(x) -> affine_remap(x, center=mean, scale=1/std)
# ---------------------------------------------------------------------------

# regional fbm (norm01): min~-0.446, ptp~0.847
GLACIAL_REGIONAL_CENTER: float = -0.446
GLACIAL_REGIONAL_SCALE: float = 1.181

# _oriented_relief: norm01 of (0.60*long + 0.22*mid + 0.14*cross)
# combo min~-0.008, ptp~0.645
GLACIAL_RELIEF_CENTER: float = -0.008
GLACIAL_RELIEF_SCALE: float = 1.465

# massif inner: norm01 of (0.72*regional + 0.72*relief_envelope + 0.20*relief)
# min~0.154, ptp~1.270
GLACIAL_MASSIF_CENTER: float = 0.154
GLACIAL_MASSIF_SCALE: float = 0.787

# base inner: zscore of uplift*(1.34*massif + 0.22*relief - 0.16*(1-icefield))
# mean~0.758, std~0.402
GLACIAL_BASE_CENTER: float = 0.758
GLACIAL_BASE_SCALE: float = 2.487

# primary_combo: norm01 of (0.58*flow + 1.18*axial)
# min~0.003, ptp~1.452
GLACIAL_PRIMARY_CENTER: float = 0.003
GLACIAL_PRIMARY_SCALE: float = 0.690

# axial gate fbm (norm01 inside _axial_troughs):
# input is raw fbm; measure min/ptp
# fbm(seed+190): mean~0.004, min~-0.430, ptp~0.990
GLACIAL_AXIAL_GATE_CENTER: float = -0.430
GLACIAL_AXIAL_GATE_SCALE: float = 1.010

# relief zscore (inside branch_surface: zscore of the [0,1] norm01 relief output)
# relief output is post-blur [0,1], mean~0.503, std~0.196
GLACIAL_RELIEF_ZSCORE_CENTER: float = 0.503
GLACIAL_RELIEF_ZSCORE_SCALE: float = 5.102

# ridge_detail ridged_multifractal (zscore): mean~0.331, std~0.217
GLACIAL_RIDGE_DETAIL_CENTER: float = 0.331
GLACIAL_RIDGE_DETAIL_SCALE: float = 4.616

# close_detail fbm (zscore): mean~0.003, std~0.288
GLACIAL_CLOSE_DETAIL_CENTER: float = 0.003
GLACIAL_CLOSE_DETAIL_SCALE: float = 3.478

# striations combo (zscore): mean~0.001, std~0.222
GLACIAL_STRIATIONS_CENTER: float = 0.001
GLACIAL_STRIATIONS_SCALE: float = 4.516

# final blend: affine replaces trailing zscore; mean~-0.096, std~1.005
# Scale tuned slightly below 1/std (use 0.82) so overall amplitude stays near legacy.
# Mountain/grassland use 0.80/0.82; glacial's std is already ~1.0 so 0.82 keeps parity.
GLACIAL_FINAL_CENTER: float = -0.096
GLACIAL_FINAL_SCALE: float = 0.820


@dataclass(frozen=True)
class GlacialStyle:
    key: str
    label: str
    angle_rad: float
    uplift_gain: float = 1.0
    trough_gain: float = 1.0
    ridge_gain: float = 1.0
    branch_gain: float = 1.0
    trough_width_px: float = 5.0
    ice_smooth_px: float = 5.0
    detail_gain: float = 1.0
    striation_gain: float = 1.0
    anisotropy: float = 0.52


STYLES = (
    GlacialStyle(
        "fjorded_troughs",
        "fjorded troughs",
        angle_rad=0.56,
        uplift_gain=1.16,
        trough_gain=1.34,
        ridge_gain=1.02,
        branch_gain=0.82,
        trough_width_px=6.8,
        ice_smooth_px=6.2,
        detail_gain=0.40,
        striation_gain=0.82,
        anisotropy=0.72,
    ),
    GlacialStyle(
        "icefield_plateau",
        "icefield plateau",
        angle_rad=-0.18,
        uplift_gain=0.98,
        trough_gain=0.94,
        ridge_gain=0.74,
        branch_gain=0.56,
        trough_width_px=7.8,
        ice_smooth_px=8.4,
        detail_gain=0.28,
        striation_gain=0.68,
        anisotropy=0.48,
    ),
    GlacialStyle(
        "alpine_cirques",
        "alpine cirques",
        angle_rad=0.94,
        uplift_gain=1.10,
        trough_gain=1.12,
        ridge_gain=1.20,
        branch_gain=1.04,
        trough_width_px=4.8,
        ice_smooth_px=5.0,
        detail_gain=0.50,
        striation_gain=0.72,
        anisotropy=0.64,
    ),
    GlacialStyle(
        "arctic_valley_net",
        "arctic valley net",
        angle_rad=-0.62,
        uplift_gain=0.92,
        trough_gain=1.18,
        ridge_gain=0.82,
        branch_gain=1.22,
        trough_width_px=5.8,
        ice_smooth_px=7.0,
        detail_gain=0.34,
        striation_gain=0.96,
        anisotropy=0.38,
    ),
)


def grid(n: int, span_m: float, ox: float = 0.0, oz: float = 0.0) -> tuple[np.ndarray, np.ndarray]:
    ii = np.linspace(0.0, float(span_m), int(n))
    return np.meshgrid(ii + float(ox), ii + float(oz))


def zscore(a: np.ndarray) -> np.ndarray:
    a = np.asarray(a, dtype=np.float64)
    return (a - float(a.mean())) / (float(a.std()) + 1e-9)


def norm01(a: np.ndarray) -> np.ndarray:
    a = np.asarray(a, dtype=np.float64)
    return (a - float(a.min())) / (float(np.ptp(a)) + 1e-9)


def smoothstep(edge0: float, edge1: float, x: np.ndarray) -> np.ndarray:
    t = np.clip((x - float(edge0)) / (float(edge1) - float(edge0) + 1e-9), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def _rotated(
    wx: np.ndarray,
    wz: np.ndarray,
    angle_rad: float,
    *,
    cx: float | None = None,
    cz: float | None = None,
) -> tuple[np.ndarray, np.ndarray]:
    """Rotate ``(wx, wz)`` by ``angle_rad``.

    When ``cx``/``cz`` are ``None`` (legacy default), the rotation centre is the
    window midpoint -- data-dependent, NOT seam-safe.  Pass ``cx=0.0, cz=0.0``
    (or any fixed world-space centre) for seam-safe rotation.
    """
    if cx is None:
        cx = float(np.min(wx)) + float(np.ptp(wx)) * 0.5
    if cz is None:
        cz = float(np.min(wz)) + float(np.ptp(wz)) * 0.5
    x = wx - cx
    z = wz - cz
    c = np.cos(float(angle_rad))
    s = np.sin(float(angle_rad))
    return c * x + s * z, -s * x + c * z


def _oriented_relief(
    wx: np.ndarray,
    wz: np.ndarray,
    span_m: float,
    style: GlacialStyle,
    seed: int,
    *,
    seam_safe_mode: bool = False,
    blur_mode: str = "reflect",
) -> np.ndarray:
    rot_cx = 0.0 if seam_safe_mode else None
    rot_cz = 0.0 if seam_safe_mode else None
    rx, rz = _rotated(wx, wz, style.angle_rad, cx=rot_cx, cz=rot_cz)
    w_rx, w_rz = wg.recursive_domain_warp(
        rx,
        rz * style.anisotropy,
        warp_amount=span_m * 0.054,
        warp_freq=1.0 / (span_m * 0.68),
        seed=seed + 100,
        steps=3,
        decay=0.56,
        freq_mul=1.78,
    )
    long = wg.ridged_multifractal(w_rx, w_rz, 1.0 / (span_m * 0.44), 5, seed + 120, gain=0.56)
    mid = wg.ridged_multifractal(w_rx, w_rz, 1.0 / (span_m * 0.22), 4, seed + 130, gain=0.52)
    cross = wg.fbm(w_rx + 0.18 * w_rz, w_rz - 0.10 * w_rx, 1.0 / (span_m * 0.30), 5, seed + 140, gain=0.54)
    raw = 0.60 * long + 0.22 * mid + 0.14 * cross
    if seam_safe_mode:
        normed = np.clip(ss.affine_remap(raw, GLACIAL_RELIEF_CENTER, GLACIAL_RELIEF_SCALE), 0.0, 1.0)
    else:
        normed = norm01(raw)
    return gaussian_filter(normed, sigma=1.25, mode=blur_mode)


def _axial_troughs(
    wx: np.ndarray,
    wz: np.ndarray,
    span_m: float,
    style: GlacialStyle,
    seed: int,
    *,
    seam_safe_mode: bool = False,
    blur_mode: str = "reflect",
) -> np.ndarray:
    rot_cx = 0.0 if seam_safe_mode else None
    rot_cz = 0.0 if seam_safe_mode else None
    rx, rz = _rotated(wx, wz, style.angle_rad, cx=rot_cx, cz=rot_cz)
    long_noise = wg.fbm(rx, rz * 0.10, 1.0 / (span_m * 0.70), 5, seed + 170, gain=0.55)
    mid_noise = wg.fbm(rx + rz * 0.05, rz * 0.16, 1.0 / (span_m * 0.34), 4, seed + 180, gain=0.50)
    meander = (0.72 * long_noise + 0.28 * mid_noise) * span_m * 0.13
    offsets = (-0.24, 0.0, 0.25)
    trough = np.zeros_like(wx, dtype=np.float64)
    width = span_m * (0.030 + 0.010 * np.clip(style.trough_width_px / 7.0, 0.0, 1.4))
    for offset in offsets:
        center = meander + span_m * offset
        dist = np.abs(rz - center) / max(width, 1.0)
        trough = np.maximum(trough, np.exp(-(dist * dist)))
    gate_raw = wg.fbm(rx, rz, 1.0 / (span_m * 0.52), 4, seed + 190, gain=0.52)
    if seam_safe_mode:
        gate = smoothstep(0.28, 0.88, np.clip(ss.affine_remap(gate_raw, GLACIAL_AXIAL_GATE_CENTER, GLACIAL_AXIAL_GATE_SCALE), 0.0, 1.0))
    else:
        gate = smoothstep(0.28, 0.88, norm01(gate_raw))
    return gaussian_filter(np.clip(trough * (0.55 + 0.45 * gate), 0.0, 1.0), sigma=max(style.trough_width_px * 0.18, 0.8), mode=blur_mode)


def _trough_channels(surface: np.ndarray, width_px: float, power: float) -> np.ndarray:
    channels = wg.flow_accumulation_channels(gaussian_filter(surface, sigma=1.85), power=power)
    channels = gaussian_filter(channels, sigma=max(float(width_px), 0.1))
    return norm01(channels)


def _trough_channels_seam_safe(
    surface: np.ndarray,
    width_px: float,
    *,
    mode: str = "nearest",
    power: float = 0.58,
) -> np.ndarray:
    """Seam-safe CONNECTED drainage: real MFD flow accumulation + FIXED-max normalization.

    Mirrors mountain_synthesis._flow_channels_seam_safe exactly, using glacial's
    flow power (0.58 primary / 0.36 branch) and spread sigma (trough_width_px).
    See mountain_synthesis docstring for the probe measurements that confirm
    convergence at apron 160.

    Reach: pre-blur sigma=1.85 -> reach 8 px; MFD convergence (not blur-reach)
    sets the apron requirement; spread blur sigma=width_px<=7.8 -> reach 31 px.
    """
    pre = gaussian_filter(np.asarray(surface, dtype=np.float64), sigma=1.85, mode=mode)
    acc = skel._flow_accumulation_mfd(pre, power=float(power))
    # FIXED-max normalization (SEAM-SAFE: no per-window statistics).
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    # Spread channel extent with nearest-mode blur (glacial trough spread).
    return np.clip(
        gaussian_filter(discharge, sigma=max(float(width_px), 0.1), mode=mode),
        0.0,
        1.0,
    )


def _striations(
    wx: np.ndarray,
    wz: np.ndarray,
    span_m: float,
    style: GlacialStyle,
    seed: int,
    *,
    seam_safe_mode: bool = False,
    blur_mode: str = "reflect",
) -> np.ndarray:
    rot_cx = 0.0 if seam_safe_mode else None
    rot_cz = 0.0 if seam_safe_mode else None
    rx, rz = _rotated(wx, wz, style.angle_rad, cx=rot_cx, cz=rot_cz)
    long_scrape = wg.fbm(rx, rz * 0.18, 1.0 / (span_m * 0.030), 4, seed + 210, gain=0.48)
    fine_scrape = wg.fbm(rx + 0.18 * rz, rz * 0.12, 1.0 / (span_m * 0.014), 3, seed + 220, gain=0.44)
    raw = 0.72 * long_scrape + 0.28 * fine_scrape
    if seam_safe_mode:
        return ss.affine_remap(raw, GLACIAL_STRIATIONS_CENTER, GLACIAL_STRIATIONS_SCALE)
    return zscore(raw)


def generate(
    wx: np.ndarray,
    wz: np.ndarray,
    seed: int = 0,
    style: GlacialStyle = STYLES[0],
    feature_span_m: float | None = None,
    apron_px: int = 0,
) -> dict[str, np.ndarray | GlacialStyle]:
    """Generate one glacial-only candidate field.

    Parameters
    ----------
    wx, wz:
        World-coordinate grids.  When ``apron_px > 0`` these must be
        apron-padded (``apron_px`` cells of real world coords on each side);
        the returned ``height`` is cropped to the inner core.
    seed:
        RNG seed.
    style:
        One of ``STYLES``.
    feature_span_m:
        Override for the feature wavelength scale.  Defaults to the grid span.
        **REQUIRED when ``apron_px > 0``**: must be a fixed constant (e.g. the
        CORE span in metres), NOT derived from the padded ``wx``/``wz`` extent.
        Adjacent windows must pass the SAME value or their noise frequencies
        will differ and break seam-exactness.
    apron_px:
        When > 0, enables seam-safe mode.  ``wx``/``wz`` must include
        ``apron_px`` extra cells on every side.  The returned height (and all
        diagnostic fields) are cropped to the core before returning.
        Use ``GLACIAL_APRON_PX`` for the correct value.
    """
    a = int(apron_px)
    seam_safe_mode = a > 0
    blur_mode = "nearest" if seam_safe_mode else "reflect"

    if seam_safe_mode and feature_span_m is None:
        raise ValueError(
            "generate(): feature_span_m is required when apron_px > 0. "
            "Pass the CORE span in metres as a fixed constant shared by all "
            "adjacent windows (e.g. feature_span_m=60_000.0). "
            "Deriving span from np.ptp(wx) on a padded grid is data-dependent "
            "and will break seam-exactness."
        )

    span = max(float(np.ptp(wx)), float(np.ptp(wz)), 1.0)
    feature_span = max(float(feature_span_m) if feature_span_m is not None else span, 1.0)
    w_x, w_z = wg.recursive_domain_warp(
        wx,
        wz,
        warp_amount=feature_span * 0.044,
        warp_freq=1.0 / (feature_span * 0.78),
        seed=seed + 10,
        steps=3,
        decay=0.58,
        freq_mul=1.70,
    )

    if seam_safe_mode:
        regional = np.clip(ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.96), 5, seed + 20, gain=0.56),
            GLACIAL_REGIONAL_CENTER, GLACIAL_REGIONAL_SCALE,
        ), 0.0, 1.0)
        relief = _oriented_relief(w_x, w_z, feature_span, style, seed, seam_safe_mode=True, blur_mode=blur_mode)
        relief_envelope = smoothstep(0.22, 0.62, gaussian_filter(relief, sigma=5.8, mode=blur_mode))
        icefield = smoothstep(0.48, 0.78, gaussian_filter(0.56 * regional + 0.44 * relief_envelope, sigma=7.0, mode=blur_mode))
        massif = gaussian_filter(
            np.clip(ss.affine_remap(0.72 * regional + 0.72 * relief_envelope + 0.20 * relief, GLACIAL_MASSIF_CENTER, GLACIAL_MASSIF_SCALE), 0.0, 1.0),
            sigma=2.8, mode=blur_mode,
        )

        base = ss.affine_remap(
            style.uplift_gain * (1.34 * massif + 0.22 * relief - 0.16 * (1.0 - icefield)),
            GLACIAL_BASE_CENTER, GLACIAL_BASE_SCALE,
        )

        flow_primary = _trough_channels_seam_safe(base, width_px=style.trough_width_px, mode=blur_mode, power=0.58)
        axial = _axial_troughs(w_x, w_z, feature_span, style, seed, seam_safe_mode=True, blur_mode=blur_mode)
        primary = np.clip(ss.affine_remap(0.58 * flow_primary + 1.18 * axial, GLACIAL_PRIMARY_CENTER, GLACIAL_PRIMARY_SCALE), 0.0, 1.0)
        primary_mask = smoothstep(0.34, 0.84, primary)

        relief_z = ss.affine_remap(relief, GLACIAL_RELIEF_ZSCORE_CENTER, GLACIAL_RELIEF_ZSCORE_SCALE)
        branch_surface = base + 0.10 * relief_z - 0.18 * gaussian_filter(primary_mask, sigma=1.6, mode=blur_mode)
        tributary = _trough_channels_seam_safe(branch_surface, width_px=max(style.trough_width_px * 0.48, 0.8), mode=blur_mode, power=0.36)
        tributary_mask = smoothstep(0.54, 0.96, tributary) * (0.45 + 0.55 * relief_envelope)

        ridge_detail = ss.affine_remap(
            wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.060), 4, seed + 40, gain=0.50),
            GLACIAL_RIDGE_DETAIL_CENTER, GLACIAL_RIDGE_DETAIL_SCALE,
        )
        close_detail = ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.026), 4, seed + 50, gain=0.46),
            GLACIAL_CLOSE_DETAIL_CENTER, GLACIAL_CLOSE_DETAIL_SCALE,
        )
        scrapes = _striations(w_x, w_z, feature_span, style, seed, seam_safe_mode=True, blur_mode=blur_mode)
    else:
        regional = norm01(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.96), 5, seed + 20, gain=0.56))
        relief = _oriented_relief(w_x, w_z, feature_span, style, seed, seam_safe_mode=False)
        relief_envelope = smoothstep(0.22, 0.62, gaussian_filter(relief, sigma=5.8))
        icefield = smoothstep(0.48, 0.78, gaussian_filter(0.56 * regional + 0.44 * relief_envelope, sigma=7.0))
        massif = gaussian_filter(norm01(0.72 * regional + 0.72 * relief_envelope + 0.20 * relief), sigma=2.8)

        base = zscore(style.uplift_gain * (1.34 * massif + 0.22 * relief - 0.16 * (1.0 - icefield)))
        flow_primary = _trough_channels(base, width_px=style.trough_width_px, power=0.58)
        axial = _axial_troughs(w_x, w_z, feature_span, style, seed, seam_safe_mode=False)
        primary = norm01(0.58 * flow_primary + 1.18 * axial)
        primary_mask = smoothstep(0.34, 0.84, primary)

        branch_surface = base + 0.10 * zscore(relief) - 0.18 * gaussian_filter(primary_mask, sigma=1.6)
        tributary = _trough_channels(branch_surface, width_px=max(style.trough_width_px * 0.48, 0.8), power=0.36)
        tributary_mask = smoothstep(0.54, 0.96, tributary) * (0.45 + 0.55 * relief_envelope)

        ridge_detail = zscore(wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.060), 4, seed + 40, gain=0.50))
        close_detail = zscore(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.026), 4, seed + 50, gain=0.46))
        scrapes = _striations(w_x, w_z, feature_span, style, seed, seam_safe_mode=False)

    ridge_wall = smoothstep(0.48, 0.84, relief_envelope) * (1.0 - 0.52 * primary_mask)
    trough_floor = np.clip(0.90 * primary_mask + 0.44 * tributary_mask, 0.0, 1.0)
    high_ice = np.clip(icefield * (1.0 - 0.30 * primary_mask), 0.0, 1.0)

    height = base.copy()
    height += style.ridge_gain * (0.10 + 0.52 * ridge_wall) * (0.24 * ridge_detail)
    height += style.detail_gain * (0.04 + 0.18 * ridge_wall) * (0.18 * close_detail)
    height += style.striation_gain * (0.04 + 0.22 * (high_ice + trough_floor)) * (0.18 * scrapes)
    height -= style.trough_gain * (0.44 + 0.44 * high_ice + 0.16 * ridge_wall) * primary_mask
    height -= style.branch_gain * (0.12 + 0.34 * ridge_wall) * tributary_mask

    floor_mask = np.clip(smoothstep(0.36, 0.80, gaussian_filter(trough_floor, sigma=1.6, mode=blur_mode)), 0.0, 1.0)
    ice_mask = np.clip(smoothstep(0.50, 0.90, high_ice), 0.0, 1.0)
    floor = gaussian_filter(height, sigma=max(style.ice_smooth_px, 0.2), mode=blur_mode)
    ice_smooth = gaussian_filter(height, sigma=max(style.ice_smooth_px * 0.65, 0.2), mode=blur_mode)
    height = height * (1.0 - 0.52 * floor_mask) + floor * (0.52 * floor_mask)
    height = height * (1.0 - 0.28 * ice_mask) + ice_smooth * (0.28 * ice_mask)
    height -= 0.16 * floor_mask

    if seam_safe_mode:
        final_blend = 0.66 * height + 0.34 * gaussian_filter(height, sigma=1.35, mode=blur_mode)
        height = ss.affine_remap(final_blend, GLACIAL_FINAL_CENTER, GLACIAL_FINAL_SCALE)
    else:
        height = zscore(0.66 * height + 0.34 * gaussian_filter(height, sigma=1.35))

    # --- crop to core ---
    if seam_safe_mode and a > 0:
        height          = np.ascontiguousarray(height[a:-a, a:-a])
        relief          = np.ascontiguousarray(relief[a:-a, a:-a])
        relief_envelope = np.ascontiguousarray(relief_envelope[a:-a, a:-a])
        icefield        = np.ascontiguousarray(icefield[a:-a, a:-a])
        axial           = np.ascontiguousarray(axial[a:-a, a:-a])
        primary_mask    = np.ascontiguousarray(primary_mask[a:-a, a:-a])
        tributary_mask  = np.ascontiguousarray(tributary_mask[a:-a, a:-a])
        floor_mask      = np.ascontiguousarray(floor_mask[a:-a, a:-a])
        scrapes         = np.ascontiguousarray(scrapes[a:-a, a:-a])

    return {
        "height": height,
        "relief": relief,
        "relief_envelope": relief_envelope,
        "icefield": icefield,
        "axial_troughs": axial,
        "primary_troughs": primary_mask,
        "tributaries": tributary_mask,
        "trough_floor": floor_mask,
        "striations": scrapes,
        "style": style,
    }
