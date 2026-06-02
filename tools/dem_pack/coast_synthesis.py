"""Coast-only reference-kernel synthesis experiments.

This is a terrain-only setup pass. It marks low shelf/sea zones in masks and
palette, but does not solve runtime water, tides, shore materials, or sea-level
integration.

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
4. Channel carving uses REAL multiple-flow-direction (MFD) accumulation
   (``geography_skeleton._flow_accumulation_mfd``) with a FIXED-max normalisation
   (``log1p(acc) / log1p(acc.size)`` -- data-independent, NOT per-window max).
   Global flow accumulation is not bit-exact on a finite apron, but it CONVERGES
   to bit-exact as the apron grows: see mountain_synthesis docstring for the
   probe measurements that motivated apron 160.
5. A single crop ``height[a:-a, a:-a]`` is performed at the very end.

Required apron (computed as sum of chained kernel reaches along the deepest path):
  - channels: gaussian_filter(channels_raw, sigma=1.9) -> reach 8 px
  - islands: gaussian_filter(islands_seed, sigma=2.0) -> reach 8 px
  - sea smoothing: gaussian_filter(height, sigma=3.0) -> reach 12 px; chain = 12+8 = 20
  - flow pre-blur sigma=1.15 -> reach 5 px (MFD convergence dominates)
  - final blend sigma=0.9 -> reach 4 px; total blur-reach = 20+4 = 24

The blur-reach budget alone is ~24, BUT the dominant residual is the REAL
flow-accumulation convergence error, which is SCALE-DEPENDENT (same finding as
mountain). So: -> ``COAST_APRON_PX = 160`` -- matches mountain's calibrated
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
# must supply when calling generate(apron_px=COAST_APRON_PX).
# See module docstring for the derivation.
# ---------------------------------------------------------------------------
COAST_APRON_PX: int = 160

# ---------------------------------------------------------------------------
# Affine-remap constants (replace per-window zscore / norm01).
# Chosen from empirical statistics across seeds 0, 7, 42, 81 on 96x96 / 90 km grids.
# Target: same STRUCTURE and similar tone to the original zscore/norm01 output.
#
# norm01(x) -> affine_remap(x, center=mean_min, scale=1/mean_ptp)
# zscore(x) -> affine_remap(x, center=mean, scale=1/std)
# ---------------------------------------------------------------------------

# inland fbm (norm01): mean_min=-0.551, ptp=1.084
COAST_INLAND_CENTER: float = -0.551
COAST_INLAND_SCALE: float = 0.923

# ridge_source (zscore): mean=0.500, std=0.224
COAST_RIDGE_SOURCE_CENTER: float = 0.500
COAST_RIDGE_SOURCE_SCALE: float = 4.474

# texture ridged multifractal (zscore): mean=0.350, std=0.225
COAST_TEXTURE_CENTER: float = 0.350
COAST_TEXTURE_SCALE: float = 4.437

# sea_floor fbm (norm01): mean_min=-0.708, ptp=1.402
COAST_SEA_FLOOR_CENTER: float = -0.708
COAST_SEA_FLOOR_SCALE: float = 0.713

# inland used in land_height (zscore): mean=-0.045, std=0.222
COAST_INLAND_ZSCORE_CENTER: float = -0.045
COAST_INLAND_ZSCORE_SCALE: float = 4.499

# final blend (replaces trailing zscore): mean=-0.518, std=0.602
# FINAL_SCALE tuned to keep post-blend amplitude near legacy std~1.
COAST_FINAL_CENTER: float = -0.518
COAST_FINAL_SCALE: float = 1.662


@dataclass(frozen=True)
class CoastStyle:
    key: str
    label: str
    angle_rad: float
    scarp_gain: float = 1.0
    fjord_gain: float = 1.0
    island_gain: float = 1.0
    shelf_gain: float = 1.0
    headland_gain: float = 1.0
    texture_gain: float = 1.0
    coastline_warp: float = 1.0
    seed_offset: int = 0


STYLES = (
    CoastStyle(
        "cliffed_headlands",
        "cliffed headlands",
        angle_rad=0.12,
        scarp_gain=1.28,
        fjord_gain=0.28,
        island_gain=0.34,
        shelf_gain=0.82,
        headland_gain=1.14,
        texture_gain=0.72,
        coastline_warp=0.92,
        seed_offset=0,
    ),
    CoastStyle(
        "fjord_coast",
        "fjord coast",
        angle_rad=-0.52,
        scarp_gain=1.10,
        fjord_gain=1.45,
        island_gain=0.52,
        shelf_gain=0.74,
        headland_gain=0.86,
        texture_gain=0.58,
        coastline_warp=1.18,
        seed_offset=1000,
    ),
    CoastStyle(
        "ria_island_shelf",
        "ria island shelf",
        angle_rad=0.64,
        scarp_gain=0.58,
        fjord_gain=0.82,
        island_gain=1.28,
        shelf_gain=1.22,
        headland_gain=0.82,
        texture_gain=0.38,
        coastline_warp=1.34,
        seed_offset=2000,
    ),
    CoastStyle(
        "desert_coastal_scarp",
        "desert coastal scarp",
        angle_rad=-0.20,
        scarp_gain=1.36,
        fjord_gain=0.22,
        island_gain=0.18,
        shelf_gain=0.96,
        headland_gain=0.70,
        texture_gain=0.52,
        coastline_warp=0.58,
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


def _flow_channels_seam_safe(
    surface: np.ndarray,
    *,
    mode: str = "nearest",
    power: float = 0.47,
) -> np.ndarray:
    """Seam-safe CONNECTED drainage: real MFD flow accumulation + FIXED-max normalization.

    Mirrors mountain_synthesis._flow_channels_seam_safe exactly, using coast's
    flow power (0.47) and spread sigma (1.9).  See mountain_synthesis docstring for
    the probe measurements that confirm convergence at apron 160.

    Reach: pre-blur sigma=1.15 -> reach 5 px; MFD convergence (not blur-reach) sets
    the apron requirement; spread blur sigma=1.9 -> reach 8 px.
    """
    pre = gaussian_filter(np.asarray(surface, dtype=np.float64), sigma=1.15, mode=mode)
    acc = skel._flow_accumulation_mfd(pre, power=float(power))
    # FIXED-max normalization (SEAM-SAFE: no per-window statistics).
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    # Spread channel extent with nearest-mode blur (coast spread sigma=1.9).
    return np.clip(
        gaussian_filter(discharge, sigma=1.9, mode=mode),
        0.0,
        1.0,
    )


def generate(
    wx: np.ndarray,
    wz: np.ndarray,
    seed: int = 0,
    style: CoastStyle = STYLES[0],
    feature_span_m: float | None = None,
    apron_px: int = 0,
) -> dict[str, np.ndarray | CoastStyle]:
    """Generate one coast setup candidate with diagnostic masks.

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
        Use ``COAST_APRON_PX`` for the correct value.
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

    # Rotation: seam-safe uses fixed world origin (0, 0); legacy uses per-window midpoint
    rot_cx = 0.0 if seam_safe_mode else None
    rot_cz = 0.0 if seam_safe_mode else None
    rx, rz = _rotated(wx, wz, style.angle_rad, cx=rot_cx, cz=rot_cz)

    w_x, w_z = wg.recursive_domain_warp(
        wx,
        wz,
        warp_amount=feature_span * 0.026,
        warp_freq=1.0 / (feature_span * 0.82),
        seed=sseed + 10,
        steps=3,
        decay=0.55,
        freq_mul=1.72,
    )

    coast_warp = wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.42), 5, sseed + 30, gain=0.56)
    signed = rx + coast_warp * feature_span * 0.15 * style.coastline_warp
    sea = smoothstep(feature_span * 0.030, -feature_span * 0.030, signed)
    land = 1.0 - sea
    nearshore = np.exp(-((signed / (feature_span * 0.045)) ** 2))
    shelf = smoothstep(feature_span * 0.20, -feature_span * 0.060, signed)

    inland_raw = wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.72), 5, sseed + 60, gain=0.58)
    headlands_raw = wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.22), 4, sseed + 80, gain=0.52)
    headlands = smoothstep(0.50, 0.84, headlands_raw)
    scarp = nearshore * land * (0.55 + 0.75 * headlands)

    if seam_safe_mode:
        inland = np.clip(ss.affine_remap(inland_raw, COAST_INLAND_CENTER, COAST_INLAND_SCALE), 0.0, 1.0)
        ridge_source = ss.affine_remap(
            inland + 0.36 * headlands + 0.18 * scarp,
            COAST_RIDGE_SOURCE_CENTER, COAST_RIDGE_SOURCE_SCALE,
        )
        # Seam-safe flow: MFD accumulation with fixed-max normalization
        channels_raw = _flow_channels_seam_safe(ridge_source, mode=blur_mode, power=0.47)
        channels = smoothstep(0.53, 0.92, channels_raw) * land
    else:
        inland = norm01(inland_raw)
        ridge_source = zscore(inland + 0.36 * headlands + 0.18 * scarp)
        channels_raw = wg.flow_accumulation_channels(ridge_source, power=0.47)
        channels = smoothstep(0.53, 0.92, gaussian_filter(channels_raw, sigma=1.9)) * land

    fjords = channels * nearshore * smoothstep(0.20, 0.80, land)
    fjord_grooves = wg.ridged_multifractal(rz, rx * 0.24, 1.0 / (feature_span * 0.11), 4, sseed + 120, gain=0.50)
    fjord_grooves = smoothstep(0.52, 0.88, fjord_grooves) * land * smoothstep(feature_span * 0.25, -feature_span * 0.01, signed)
    channel_relief = np.clip(
        channels * (0.34 + 0.34 * style.fjord_gain)
        + fjords * style.fjord_gain
        + fjord_grooves * max(style.fjord_gain - 0.30, 0.0) * 0.44,
        0.0,
        1.0,
    )

    islands_seed = wg.cellular_edges(w_x, w_z, 1.0 / (feature_span * 0.18), sseed + 160, sharpness=1.30)
    islands = smoothstep(0.50, 0.86, gaussian_filter(islands_seed, sigma=2.0, mode=blur_mode)) * sea
    islands *= smoothstep(feature_span * 0.18, -feature_span * 0.02, signed)

    texture_raw = wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.050), 4, sseed + 220, gain=0.44)
    sea_floor_raw = wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.34), 4, sseed + 260, gain=0.55)

    if seam_safe_mode:
        texture = ss.affine_remap(texture_raw, COAST_TEXTURE_CENTER, COAST_TEXTURE_SCALE)
        sea_floor = -0.74 - 0.22 * np.clip(ss.affine_remap(sea_floor_raw, COAST_SEA_FLOOR_CENTER, COAST_SEA_FLOOR_SCALE), 0.0, 1.0)
        land_height = 0.68 * ss.affine_remap(inland_raw, COAST_INLAND_ZSCORE_CENTER, COAST_INLAND_ZSCORE_SCALE) + 0.26 * style.headland_gain * headlands
    else:
        texture = zscore(texture_raw)
        sea_floor = -0.74 - 0.22 * norm01(sea_floor_raw)
        land_height = 0.68 * zscore(inland_raw) + 0.26 * style.headland_gain * headlands

    land_height += 0.48 * style.scarp_gain * scarp
    land_height -= 0.48 * channel_relief
    land_height += style.texture_gain * 0.09 * texture * (0.35 + 0.65 * land)

    height = land * land_height + sea * sea_floor
    height += style.island_gain * 0.62 * islands
    height -= style.shelf_gain * 0.22 * shelf * sea
    smoothed_sea = gaussian_filter(height, sigma=3.0, mode=blur_mode)
    height = height * (1.0 - 0.34 * sea) + smoothed_sea * (0.34 * sea)

    if seam_safe_mode:
        final_blend = 0.86 * height + 0.14 * gaussian_filter(height, sigma=0.9, mode=blur_mode)
        height = ss.affine_remap(final_blend, COAST_FINAL_CENTER, COAST_FINAL_SCALE)
    else:
        height = zscore(0.86 * height + 0.14 * gaussian_filter(height, sigma=0.9))

    # --- crop to core ---
    if seam_safe_mode and a > 0:
        height        = np.ascontiguousarray(height[a:-a, a:-a])
        sea           = np.ascontiguousarray(sea[a:-a, a:-a])
        shelf         = np.ascontiguousarray(shelf[a:-a, a:-a])
        scarp         = np.ascontiguousarray(scarp[a:-a, a:-a])
        channel_relief = np.ascontiguousarray(channel_relief[a:-a, a:-a])
        islands       = np.ascontiguousarray(islands[a:-a, a:-a])
        headlands     = np.ascontiguousarray(headlands[a:-a, a:-a])

    return {
        "height": height,
        "sea": sea,
        "shelf": shelf,
        "scarp": np.clip(scarp * style.scarp_gain, 0.0, 1.0),
        "channels": channel_relief,
        "islands": np.clip(islands * style.island_gain, 0.0, 1.0),
        "headlands": np.clip(headlands * style.headland_gain, 0.0, 1.0),
        "style": style,
    }
