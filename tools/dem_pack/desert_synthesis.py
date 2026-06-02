"""Desert-only reference-kernel synthesis experiments.

Desert terrain is not one thing. This setup pass keeps separate modes for dune
seas, yardang/deflation basins, rocky basin-range terrain, and wadi/erg margins.

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
4. Wash carving uses REAL multiple-flow-direction (MFD) accumulation
   (``geography_skeleton._flow_accumulation_mfd``) with a FIXED-max normalisation
   (``log1p(acc) / log1p(acc.size)`` -- data-independent, NOT per-window max).
   Global flow accumulation is not bit-exact on a finite apron, but it CONVERGES
   to bit-exact as the apron grows: see mountain_synthesis docstring for the
   probe measurements that motivated apron 160.
5. A single crop ``height[a:-a, a:-a]`` is performed at the very end.

Required apron (computed as sum of chained kernel reaches along the deepest path):
  - basin blur sigma=6.2 -> reach 25 px
  - playa blur sigma=5.0 -> reach 20 px; runs on basin (depth=25): chain = 25+20 = 45
  - block_cores blur sigma=3.2 -> reach 13 px (parallel, not chained deeper than basin)
  - mesa blur sigma=2.2 -> reach 9 px (runs on block_cores, but starts fresh)
  - flow pre-blur sigma=1.15 -> reach 5 px (MFD convergence dominates over blur-reach)
  - washes spread blur sigma=1.8 -> reach 7 px; MFD depth dominates; chain = 45+7 = 52
  - floor smooth blur max sigma=5.2 -> reach 21 px; input depth=52: chain = 52+21 = 73
  - final blend sigma=0.95 -> reach 4 px; total = 73+4 = 77 (blur-reach budget)

The blur-reach budget alone is ~77, BUT the dominant residual is the REAL
flow-accumulation convergence error, which is SCALE-DEPENDENT (same finding as
mountain). So: -> ``DESERT_APRON_PX = 160`` -- matches mountain's calibrated
floor (see mountain_synthesis docstring for the 7x7 / 175 km world measurement).
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
# must supply when calling generate(apron_px=DESERT_APRON_PX).
# See module docstring for the derivation.
# ---------------------------------------------------------------------------
DESERT_APRON_PX: int = 160

# ---------------------------------------------------------------------------
# Affine-remap constants (replace per-window zscore / norm01).
# Chosen from empirical statistics across seeds 0, 7, 42 on 96x96 / 90 km grids,
# all four styles combined.
# Target: same STRUCTURE and similar tone to the original zscore/norm01 output.
#
# norm01(x) -> affine_remap(x, center=mean_min, scale=1/mean_ptp)
# zscore(x) -> affine_remap(x, center=mean, scale=1/std)
# ---------------------------------------------------------------------------

# regional fbm (norm01): mean_min=-0.668, ptp=1.397
DESERT_REGIONAL_CENTER: float = -0.668
DESERT_REGIONAL_SCALE: float = 0.716

# dune blurred (norm01 of gaussian_filter(dunes_raw, sigma=0.70)):
# min=0.018, ptp=0.626
DESERT_DUNE_CENTER: float = 0.018
DESERT_DUNE_SCALE: float = 1.596

# yardang combo (norm01 of 0.72*ridges + 0.28*fine_y):
# min=0.001, ptp=0.915
DESERT_YARDANG_CENTER: float = 0.001
DESERT_YARDANG_SCALE: float = 1.093

# base_surface_raw (zscore of 0.72*regional + 0.24*mesas - 0.62*basin):
# mean=0.113, std=0.433
DESERT_BASE_CENTER: float = 0.113
DESERT_BASE_SCALE: float = 2.312

# fine fbm (zscore): mean=-0.002~=0.0, std=0.282
DESERT_FINE_CENTER: float = 0.000
DESERT_FINE_SCALE: float = 3.543

# salt ridged multifractal (zscore): mean=0.365, std=0.239
DESERT_SALT_CENTER: float = 0.365
DESERT_SALT_SCALE: float = 4.185

# final blend (replaces trailing zscore): mean=0.171, std=1.143
# Tuned to 0.85 so post-floor-blend amplitude lands near legacy std~1.
DESERT_FINAL_CENTER: float = 0.000
DESERT_FINAL_SCALE: float = 0.85


@dataclass(frozen=True)
class DesertStyle:
    key: str
    label: str
    angle_rad: float
    dune_gain: float = 1.0
    yardang_gain: float = 1.0
    wash_gain: float = 1.0
    mesa_gain: float = 1.0
    playa_gain: float = 1.0
    basin_gain: float = 1.0
    dune_spacing_m: float = 3100.0
    dune_width: float = 0.44
    yardang_anisotropy: float = 0.20
    floor_smooth_px: float = 4.0
    detail_gain: float = 1.0
    seed_offset: int = 0


STYLES = (
    DesertStyle(
        "dune_sea",
        "dune sea",
        angle_rad=0.48,
        dune_gain=1.42,
        yardang_gain=0.28,
        wash_gain=0.34,
        mesa_gain=0.20,
        playa_gain=0.52,
        basin_gain=0.92,
        dune_spacing_m=2400.0,
        dune_width=0.36,
        yardang_anisotropy=0.30,
        floor_smooth_px=5.2,
        detail_gain=0.24,
        seed_offset=0,
    ),
    DesertStyle(
        "yardang_basin",
        "yardang basin",
        angle_rad=-0.18,
        dune_gain=0.08,
        yardang_gain=1.42,
        wash_gain=0.42,
        mesa_gain=0.16,
        playa_gain=0.86,
        basin_gain=1.05,
        dune_spacing_m=4200.0,
        dune_width=0.48,
        yardang_anisotropy=0.12,
        floor_smooth_px=4.6,
        detail_gain=0.38,
        seed_offset=1000,
    ),
    DesertStyle(
        "rocky_basin_range",
        "rocky basin-range",
        angle_rad=0.86,
        dune_gain=0.03,
        yardang_gain=0.14,
        wash_gain=0.98,
        mesa_gain=1.30,
        playa_gain=0.58,
        basin_gain=1.22,
        dune_spacing_m=5000.0,
        dune_width=0.52,
        yardang_anisotropy=0.26,
        floor_smooth_px=3.8,
        detail_gain=0.64,
        seed_offset=2000,
    ),
    DesertStyle(
        "wadi_erg_margin",
        "wadi erg margin",
        angle_rad=-0.72,
        dune_gain=0.34,
        yardang_gain=0.20,
        wash_gain=1.12,
        mesa_gain=0.82,
        playa_gain=0.40,
        basin_gain=0.92,
        dune_spacing_m=3300.0,
        dune_width=0.40,
        yardang_anisotropy=0.22,
        floor_smooth_px=3.4,
        detail_gain=0.58,
        seed_offset=3000,
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


def _dune_field(
    wx: np.ndarray,
    wz: np.ndarray,
    feature_span_m: float,
    style: DesertStyle,
    seed: int,
    *,
    seam_safe_mode: bool = False,
    blur_mode: str = "reflect",
) -> np.ndarray:
    rot_cx = 0.0 if seam_safe_mode else None
    rot_cz = 0.0 if seam_safe_mode else None
    rx, rz = _rotated(wx, wz, style.angle_rad, cx=rot_cx, cz=rot_cz)
    warp = wg.fbm(wx, wz, 1.0 / (feature_span_m * 0.20), 4, seed + 120, gain=0.52) * style.dune_spacing_m * 0.72
    phase = (rx + warp) / max(style.dune_spacing_m, 1.0) * np.pi * 2.0
    crest = 1.0 - np.abs(np.sin(phase))
    secondary = 1.0 - np.abs(np.sin(
        (rx * 0.62 + rz * 0.16 + warp * 0.35) / max(style.dune_spacing_m * 1.75, 1.0) * np.pi * 2.0
    ))
    dunes_raw = np.power(np.clip(0.78 * crest + 0.22 * secondary, 0.0, 1.0), 1.0 + 1.8 * style.dune_width)
    blurred = gaussian_filter(dunes_raw, sigma=0.70, mode=blur_mode)
    if seam_safe_mode:
        return np.clip(ss.affine_remap(blurred, DESERT_DUNE_CENTER, DESERT_DUNE_SCALE), 0.0, 1.0)
    return norm01(blurred)


def _yardang_field(
    wx: np.ndarray,
    wz: np.ndarray,
    feature_span_m: float,
    style: DesertStyle,
    seed: int,
    *,
    seam_safe_mode: bool = False,
) -> np.ndarray:
    rot_cx = 0.0 if seam_safe_mode else None
    rot_cz = 0.0 if seam_safe_mode else None
    rx, rz = _rotated(wx, wz, style.angle_rad, cx=rot_cx, cz=rot_cz)
    ridges = wg.ridged_multifractal(rx, rz * style.yardang_anisotropy, 1.0 / (feature_span_m * 0.075), 5, seed + 210, gain=0.50)
    fine = wg.ridged_multifractal(rx + 0.22 * rz, rz * 0.18, 1.0 / (feature_span_m * 0.038), 3, seed + 230, gain=0.46)
    combo = 0.72 * ridges + 0.28 * fine
    if seam_safe_mode:
        return smoothstep(0.42, 0.86, np.clip(ss.affine_remap(combo, DESERT_YARDANG_CENTER, DESERT_YARDANG_SCALE), 0.0, 1.0))
    return smoothstep(0.42, 0.86, norm01(combo))


def _wash_channels_seam_safe(
    surface: np.ndarray,
    *,
    mode: str = "nearest",
    power: float = 0.43,
) -> np.ndarray:
    """Seam-safe CONNECTED drainage: real MFD flow accumulation + FIXED-max normalization.

    Mirrors mountain_synthesis._flow_channels_seam_safe exactly, using desert's
    flow power (0.43) and spread sigma (1.8).  See mountain_synthesis docstring for
    the probe measurements that confirm convergence at apron 160.

    Reach: pre-blur sigma=1.15 -> reach 5 px; MFD convergence (not blur-reach) sets
    the apron requirement; spread blur sigma=1.8 -> reach 7 px.
    """
    pre = gaussian_filter(np.asarray(surface, dtype=np.float64), sigma=1.15, mode=mode)
    acc = skel._flow_accumulation_mfd(pre, power=float(power))
    # FIXED-max normalization (SEAM-SAFE: no per-window statistics).
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    # Spread channel extent with nearest-mode blur (desert wash spread sigma=1.8).
    return np.clip(
        gaussian_filter(discharge, sigma=1.8, mode=mode),
        0.0,
        1.0,
    )


def generate(
    wx: np.ndarray,
    wz: np.ndarray,
    seed: int = 0,
    style: DesertStyle = STYLES[0],
    feature_span_m: float | None = None,
    apron_px: int = 0,
) -> dict[str, np.ndarray | DesertStyle]:
    """Generate one desert setup candidate with diagnostic masks.

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
        Use ``DESERT_APRON_PX`` for the correct value.
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
    sseed = int(seed) + int(style.seed_offset)
    w_x, w_z = wg.recursive_domain_warp(
        wx,
        wz,
        warp_amount=feature_span * 0.030,
        warp_freq=1.0 / (feature_span * 0.72),
        seed=sseed + 10,
        steps=3,
        decay=0.52,
        freq_mul=1.78,
    )

    if seam_safe_mode:
        regional = np.clip(
            ss.affine_remap(
                wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.86), 5, sseed + 30, gain=0.58),
                DESERT_REGIONAL_CENTER, DESERT_REGIONAL_SCALE,
            ),
            0.0, 1.0,
        )
        basin = smoothstep(0.34, 0.78, 1.0 - gaussian_filter(regional, sigma=6.2, mode=blur_mode))
        playa = smoothstep(0.56, 0.90, gaussian_filter(basin, sigma=5.0, mode=blur_mode))
        dunes = _dune_field(w_x, w_z, feature_span, style, sseed, seam_safe_mode=True, blur_mode=blur_mode)
        yardangs = _yardang_field(w_x, w_z, feature_span, style, sseed, seam_safe_mode=True)

        rot_cx, rot_cz = 0.0, 0.0
        rx, rz = _rotated(w_x, w_z, style.angle_rad + 0.78, cx=rot_cx, cz=rot_cz)
        block_edges = wg.cellular_edges(rx, rz, 1.0 / (feature_span * 0.210), sseed + 310, sharpness=1.25)
        block_cores = smoothstep(0.22, 0.76, gaussian_filter(1.0 - block_edges, sigma=3.2, mode=blur_mode))
        mesa_blocks = (
            smoothstep(0.52, 0.82, gaussian_filter(regional, sigma=2.2, mode=blur_mode))
            * block_cores
            * (1.0 - 0.68 * basin)
        )
        rocky_relief = smoothstep(
            0.36, 0.84,
            wg.ridged_multifractal(rx, rz * 0.42, 1.0 / (feature_span * 0.18), 4, sseed + 330, gain=0.52),
        )
        mesas = np.clip(0.68 * mesa_blocks + 0.32 * rocky_relief * (1.0 - 0.42 * basin), 0.0, 1.0)

        base_surface = ss.affine_remap(
            0.72 * regional + 0.24 * mesas - 0.62 * basin,
            DESERT_BASE_CENTER, DESERT_BASE_SCALE,
        )
        washes = _wash_channels_seam_safe(base_surface + 0.16 * mesas, mode=blur_mode, power=0.43)
        washes = smoothstep(0.57, 0.94, washes) * (0.35 + 0.65 * (1.0 - playa))

        fine = ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.018), 4, sseed + 410, gain=0.48),
            DESERT_FINE_CENTER, DESERT_FINE_SCALE,
        )
        salt = ss.affine_remap(
            wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.025), 3, sseed + 430, gain=0.42),
            DESERT_SALT_CENTER, DESERT_SALT_SCALE,
        )
    else:
        regional = norm01(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.86), 5, sseed + 30, gain=0.58))
        basin = smoothstep(0.34, 0.78, 1.0 - gaussian_filter(regional, sigma=6.2))
        playa = smoothstep(0.56, 0.90, gaussian_filter(basin, sigma=5.0))
        dunes = _dune_field(w_x, w_z, feature_span, style, sseed)
        yardangs = _yardang_field(w_x, w_z, feature_span, style, sseed)

        rx, rz = _rotated(w_x, w_z, style.angle_rad + 0.78)
        block_edges = wg.cellular_edges(rx, rz, 1.0 / (feature_span * 0.210), sseed + 310, sharpness=1.25)
        block_cores = smoothstep(0.22, 0.76, gaussian_filter(1.0 - block_edges, sigma=3.2))
        mesa_blocks = (
            smoothstep(0.52, 0.82, gaussian_filter(regional, sigma=2.2))
            * block_cores
            * (1.0 - 0.68 * basin)
        )
        rocky_relief = smoothstep(
            0.36, 0.84,
            wg.ridged_multifractal(rx, rz * 0.42, 1.0 / (feature_span * 0.18), 4, sseed + 330, gain=0.52),
        )
        mesas = np.clip(0.68 * mesa_blocks + 0.32 * rocky_relief * (1.0 - 0.42 * basin), 0.0, 1.0)

        base_surface = zscore(0.72 * regional + 0.24 * mesas - 0.62 * basin)
        washes = wg.flow_accumulation_channels(base_surface + 0.16 * mesas, power=0.43)
        washes = smoothstep(0.57, 0.94, gaussian_filter(washes, sigma=1.8)) * (0.35 + 0.65 * (1.0 - playa))

        fine = zscore(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.018), 4, sseed + 410, gain=0.48))
        salt = zscore(wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.025), 3, sseed + 430, gain=0.42))

    sand_mask = np.clip((0.42 + 0.58 * basin) * (1.0 - 0.42 * mesas), 0.0, 1.0)
    dune_mask = dunes * sand_mask * (0.25 + 0.75 * basin)
    yardang_mask = yardangs * (0.45 + 0.55 * basin) * (1.0 - 0.35 * dune_mask)
    wash_mask = washes * (0.45 + 0.55 * (1.0 - basin + 0.35 * mesas))
    playa_mask = playa * (1.0 - 0.45 * dune_mask)

    dune_relief = dune_mask * style.dune_gain
    yardang_relief = yardang_mask * style.yardang_gain
    wash_relief = wash_mask * style.wash_gain
    playa_relief = playa_mask * style.playa_gain
    mesa_relief = mesas * style.mesa_gain

    height = base_surface.copy()
    height += style.basin_gain * 0.24 * (1.0 - basin)
    height += 0.50 * mesa_relief + 0.14 * mesa_relief * fine
    height += 0.44 * dune_relief + 0.10 * dune_relief * fine
    height += 0.34 * yardang_relief + 0.08 * yardang_relief * salt
    height -= 0.36 * wash_relief
    height -= 0.38 * playa_relief
    height += style.detail_gain * (0.08 + 0.12 * mesas + 0.12 * yardang_mask) * fine

    floor_mask = np.clip(0.68 * playa_relief + 0.46 * basin + 0.34 * wash_relief, 0.0, 1.0)
    smooth_floor = gaussian_filter(height, sigma=max(style.floor_smooth_px, 0.2), mode=blur_mode)
    height = height * (1.0 - 0.34 * floor_mask) + smooth_floor * (0.34 * floor_mask)

    if seam_safe_mode:
        final_blend = 0.82 * height + 0.18 * gaussian_filter(height, sigma=0.95, mode=blur_mode)
        height = ss.affine_remap(final_blend, DESERT_FINAL_CENTER, DESERT_FINAL_SCALE)
    else:
        height = zscore(0.82 * height + 0.18 * gaussian_filter(height, sigma=0.95))

    # --- crop to core ---
    if seam_safe_mode and a > 0:
        height       = np.ascontiguousarray(height[a:-a, a:-a])
        dunes        = np.ascontiguousarray(dunes[a:-a, a:-a])
        yardangs     = np.ascontiguousarray(yardangs[a:-a, a:-a])
        washes       = np.ascontiguousarray(washes[a:-a, a:-a])
        playa        = np.ascontiguousarray(playa[a:-a, a:-a])
        mesas        = np.ascontiguousarray(mesas[a:-a, a:-a])
        basin        = np.ascontiguousarray(basin[a:-a, a:-a])
        dune_relief  = np.ascontiguousarray(dune_relief[a:-a, a:-a])
        yardang_relief = np.ascontiguousarray(yardang_relief[a:-a, a:-a])
        wash_relief  = np.ascontiguousarray(wash_relief[a:-a, a:-a])
        playa_relief = np.ascontiguousarray(playa_relief[a:-a, a:-a])
        mesa_relief  = np.ascontiguousarray(mesa_relief[a:-a, a:-a])

    return {
        "height": height,
        "dunes": np.clip(dune_relief, 0.0, 1.0),
        "yardangs": np.clip(yardang_relief, 0.0, 1.0),
        "washes": np.clip(wash_relief, 0.0, 1.0),
        "playa": np.clip(playa_relief, 0.0, 1.0),
        "mesas": np.clip(mesa_relief, 0.0, 1.0),
        "basin": basin,
        "style": style,
    }
