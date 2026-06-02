"""Temperate-only reference-kernel synthesis experiments.

Temperate terrain is intentionally broad and subtle compared with the more
distinctive families. This pass keeps separate modes for folded Appalachian-like
ridges, rounded forest hills, glaciated uplands, and rugged wet highlands.

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
4. Valley carving uses REAL multiple-flow-direction (MFD) accumulation
   (``geography_skeleton._flow_accumulation_mfd``) with a FIXED-max normalisation
   (``log1p(acc) / log1p(acc.size)`` -- data-independent, NOT per-window max).
   Global flow accumulation is not bit-exact on a finite apron, but it CONVERGES
   to bit-exact as the apron grows: see mountain_synthesis docstring for the
   probe measurements that motivated apron 160.
5. A single crop ``height[a:-a, a:-a]`` is performed at the very end.

Required apron (computed as sum of chained kernel reaches along the deepest path):
  - macro blur sigma=4.2 -> reach 17 px
  - flow pre-blur sigma=1.15 -> reach 5 px (MFD convergence dominates over blur-reach)
  - valleys spread blur sigma=1.8 -> reach 7 px; broad_valleys sigma=4.2 -> reach 17 px;
    chain from macro: 17+17 = 34 px (blur-reach budget; MFD convergence >> this)
  - rounded blur max sigma=4.0 -> reach 16 px (parallel path from macro: 17+16 = 33 px)
  - final blend sigma=1.0 -> reach 4 px; total = max(34,33)+4 = 38 px (blur-reach budget)

The blur-reach budget alone is ~38, BUT the dominant residual is the REAL
flow-accumulation convergence error, which is SCALE-DEPENDENT (same finding as
mountain). So: -> ``TEMPERATE_APRON_PX = 160`` -- matches mountain's calibrated
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
# must supply when calling generate(apron_px=TEMPERATE_APRON_PX).
# See module docstring for the derivation.
# ---------------------------------------------------------------------------
TEMPERATE_APRON_PX: int = 160

# ---------------------------------------------------------------------------
# Affine-remap constants (replace per-window zscore / norm01).
# Chosen from empirical statistics across seeds 0, 7, 42 on 96x96 / 90 km grids,
# all four styles combined.
# Target: same STRUCTURE and similar tone to the original zscore/norm01 output.
#
# norm01(x) -> affine_remap(x, center=mean_min, scale=1/mean_ptp)
# zscore(x) -> affine_remap(x, center=mean, scale=1/std)
# ---------------------------------------------------------------------------

# macro fbm raw (norm01): mean_min=-0.428, mean_ptp=0.942
TEMPERATE_MACRO_CENTER: float = -0.428
TEMPERATE_MACRO_SCALE: float = 1.061

# folded ridged_multifractal raw (norm01): mean_min=0.004, mean_ptp=0.921
TEMPERATE_FOLDED_CENTER: float = 0.004
TEMPERATE_FOLDED_SCALE: float = 1.085

# hills = norm01(gaussian_filter(ridged_mf, sigma=2.4)): mean_min=0.008, mean_ptp=0.747
TEMPERATE_HILLS_CENTER: float = 0.008
TEMPERATE_HILLS_SCALE: float = 1.339

# flow_source raw (zscore): mean=0.583, std=0.257
TEMPERATE_FLOW_SRC_CENTER: float = 0.583
TEMPERATE_FLOW_SRC_SCALE: float = 3.895

# fine fbm raw (zscore): mean=-0.001~=0.0, std=0.291
TEMPERATE_FINE_CENTER: float = 0.000
TEMPERATE_FINE_SCALE: float = 3.436

# rounded inner (0.52*macro + 0.48*hills) before zscore: mean=0.458, std=0.157
TEMPERATE_ROUNDED_CENTER: float = 0.458
TEMPERATE_ROUNDED_SCALE: float = 6.390

# final blend replaces trailing zscore: mean of pre-final=0.079, std=0.501
# Tuned so post-blend amplitude lands near legacy std~1.
TEMPERATE_FINAL_CENTER: float = 0.079
TEMPERATE_FINAL_SCALE: float = 1.995

# MFD valley channel thresholds (seam-safe path).
# log1p(acc)/log1p(acc.size) discharge field: concentrated around low values.
# These thresholds select the top-discharge channels analogous to the legacy
# smoothstep(0.52, 0.92) on per-window normalized channels.
TEMPERATE_VALLEY_THRESH_LO: float = 0.24
TEMPERATE_VALLEY_THRESH_HI: float = 0.40
TEMPERATE_BROAD_VALLEY_THRESH_LO: float = 0.20
TEMPERATE_BROAD_VALLEY_THRESH_HI: float = 0.36


@dataclass(frozen=True)
class TemperateStyle:
    key: str
    label: str
    angle_rad: float
    ridge_gain: float = 1.0
    hill_gain: float = 1.0
    valley_gain: float = 1.0
    upland_gain: float = 1.0
    smoothing_px: float = 2.0
    texture_gain: float = 1.0
    seed_offset: int = 0


STYLES = (
    TemperateStyle(
        "appalachian_ridges",
        "appalachian ridges",
        angle_rad=0.78,
        ridge_gain=1.55,
        hill_gain=0.72,
        valley_gain=1.12,
        upland_gain=0.62,
        smoothing_px=1.8,
        texture_gain=0.58,
        seed_offset=0,
    ),
    TemperateStyle(
        "rounded_forest_hills",
        "rounded forest hills",
        angle_rad=-0.24,
        ridge_gain=0.25,
        hill_gain=1.18,
        valley_gain=0.62,
        upland_gain=0.72,
        smoothing_px=3.5,
        texture_gain=0.34,
        seed_offset=1000,
    ),
    TemperateStyle(
        "glaciated_upland",
        "glaciated upland",
        angle_rad=0.18,
        ridge_gain=0.64,
        hill_gain=0.82,
        valley_gain=1.18,
        upland_gain=1.22,
        smoothing_px=4.0,
        texture_gain=0.26,
        seed_offset=2000,
    ),
    TemperateStyle(
        "rugged_wet_highland",
        "rugged wet highland",
        angle_rad=-0.66,
        ridge_gain=1.12,
        hill_gain=0.94,
        valley_gain=1.04,
        upland_gain=1.06,
        smoothing_px=2.4,
        texture_gain=0.50,
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


def _valley_channels_seam_safe(
    surface: np.ndarray,
    *,
    mode: str = "nearest",
    power: float = 0.43,
) -> np.ndarray:
    """Seam-safe CONNECTED drainage: real MFD flow accumulation + FIXED-max normalization.

    Mirrors mountain_synthesis._flow_channels_seam_safe exactly, using temperate's
    flow power (0.43) and two spread sigmas (1.8 for valleys, 4.2 for broad_valleys).
    Returns the raw discharge field (caller applies smoothstep thresholds separately).

    Reach: pre-blur sigma=1.15 -> reach 5 px; MFD convergence (not blur-reach) sets
    the apron requirement. See mountain_synthesis docstring for probe measurements.
    """
    pre = gaussian_filter(np.asarray(surface, dtype=np.float64), sigma=1.15, mode=mode)
    acc = skel._flow_accumulation_mfd(pre, power=float(power))
    # FIXED-max normalization (SEAM-SAFE: no per-window statistics).
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    return discharge


def generate(
    wx: np.ndarray,
    wz: np.ndarray,
    seed: int = 0,
    style: TemperateStyle = STYLES[0],
    feature_span_m: float | None = None,
    apron_px: int = 0,
) -> dict[str, np.ndarray | TemperateStyle]:
    """Generate one temperate setup candidate with diagnostic masks.

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
        Use ``TEMPERATE_APRON_PX`` for the correct value.
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
        warp_freq=1.0 / (feature_span * 0.76),
        seed=sseed + 10,
        steps=3,
        decay=0.55,
        freq_mul=1.72,
    )

    if seam_safe_mode:
        # Seam-safe rotation: fixed world origin (not per-window midpoint).
        rot_cx, rot_cz = 0.0, 0.0

        macro = np.clip(ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.84), 5, sseed + 30, gain=0.58),
            TEMPERATE_MACRO_CENTER, TEMPERATE_MACRO_SCALE,
        ), 0.0, 1.0)

        rx, rz = _rotated(w_x, w_z, style.angle_rad, cx=rot_cx, cz=rot_cz)
        folded = wg.ridged_multifractal(rx, rz * 0.22, 1.0 / (feature_span * 0.13), 5, sseed + 60, gain=0.54)
        ridges = smoothstep(0.40, 0.82, gaussian_filter(
            np.clip(ss.affine_remap(folded, TEMPERATE_FOLDED_CENTER, TEMPERATE_FOLDED_SCALE), 0.0, 1.0),
            sigma=1.1, mode=blur_mode,
        ))

        hills_raw = wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.28), 5, sseed + 90, gain=0.52)
        hills = np.clip(ss.affine_remap(
            gaussian_filter(hills_raw, sigma=2.4, mode=blur_mode),
            TEMPERATE_HILLS_CENTER, TEMPERATE_HILLS_SCALE,
        ), 0.0, 1.0)

        upland = smoothstep(0.50, 0.82, gaussian_filter(macro, sigma=4.2, mode=blur_mode))

        # Flow source: affine_remap replaces zscore.
        flow_source = ss.affine_remap(
            0.72 * macro + 0.32 * ridges + 0.28 * hills + 0.26 * upland,
            TEMPERATE_FLOW_SRC_CENTER, TEMPERATE_FLOW_SRC_SCALE,
        )
        # Valley channels: seam-safe MFD, fixed-max normalized discharge.
        discharge = _valley_channels_seam_safe(flow_source, mode=blur_mode, power=0.43)
        valleys = smoothstep(
            TEMPERATE_VALLEY_THRESH_LO, TEMPERATE_VALLEY_THRESH_HI,
            gaussian_filter(discharge, sigma=1.8, mode=blur_mode),
        )
        broad_valleys = smoothstep(
            TEMPERATE_BROAD_VALLEY_THRESH_LO, TEMPERATE_BROAD_VALLEY_THRESH_HI,
            gaussian_filter(discharge, sigma=4.2, mode=blur_mode),
        )

        fine = ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.035), 4, sseed + 150, gain=0.45),
            TEMPERATE_FINE_CENTER, TEMPERATE_FINE_SCALE,
        )
        rounded = gaussian_filter(
            ss.affine_remap(
                0.52 * macro + 0.48 * hills,
                TEMPERATE_ROUNDED_CENTER, TEMPERATE_ROUNDED_SCALE,
            ),
            sigma=max(style.smoothing_px, 0.2), mode=blur_mode,
        )

        height = 0.42 * style.hill_gain * ss.affine_remap(hills, 0.5, 2.0)
        height += 0.42 * style.ridge_gain * ridges
        height += 0.30 * style.upland_gain * upland
        height -= 0.30 * style.valley_gain * valleys
        height -= 0.16 * style.valley_gain * broad_valleys
        height += 0.060 * style.texture_gain * fine * (0.45 + 0.55 * ridges)
        height = 0.76 * height + 0.24 * rounded

        final_blend = 0.85 * height + 0.15 * gaussian_filter(height, sigma=1.0, mode=blur_mode)
        height = ss.affine_remap(final_blend, TEMPERATE_FINAL_CENTER, TEMPERATE_FINAL_SCALE)

    else:
        macro = norm01(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.84), 5, sseed + 30, gain=0.58))
        rx, rz = _rotated(w_x, w_z, style.angle_rad)
        folded = wg.ridged_multifractal(rx, rz * 0.22, 1.0 / (feature_span * 0.13), 5, sseed + 60, gain=0.54)
        ridges = smoothstep(0.40, 0.82, gaussian_filter(folded, sigma=1.1))
        hills = norm01(gaussian_filter(wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.28), 5, sseed + 90, gain=0.52), sigma=2.4))
        upland = smoothstep(0.50, 0.82, gaussian_filter(macro, sigma=4.2))

        flow_source = zscore(0.72 * macro + 0.32 * ridges + 0.28 * hills + 0.26 * upland)
        channels = wg.flow_accumulation_channels(flow_source, power=0.43)
        valleys = smoothstep(0.52, 0.92, gaussian_filter(channels, sigma=1.8))
        broad_valleys = smoothstep(0.48, 0.86, gaussian_filter(channels, sigma=4.2))

        fine = zscore(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.035), 4, sseed + 150, gain=0.45))
        rounded = gaussian_filter(zscore(0.52 * macro + 0.48 * hills), sigma=style.smoothing_px)

        height = 0.42 * style.hill_gain * zscore(hills)
        height += 0.42 * style.ridge_gain * ridges
        height += 0.30 * style.upland_gain * upland
        height -= 0.30 * style.valley_gain * valleys
        height -= 0.16 * style.valley_gain * broad_valleys
        height += 0.060 * style.texture_gain * fine * (0.45 + 0.55 * ridges)
        height = 0.76 * height + 0.24 * rounded
        height = zscore(0.85 * height + 0.15 * gaussian_filter(height, sigma=1.0))

    # --- crop to core ---
    if seam_safe_mode and a > 0:
        height     = np.ascontiguousarray(height[a:-a, a:-a])
        ridges     = np.ascontiguousarray(ridges[a:-a, a:-a])
        hills      = np.ascontiguousarray(hills[a:-a, a:-a])
        valleys    = np.ascontiguousarray(valleys[a:-a, a:-a])
        broad_valleys = np.ascontiguousarray(broad_valleys[a:-a, a:-a])
        upland     = np.ascontiguousarray(upland[a:-a, a:-a])

    return {
        "height": height,
        "ridges": np.clip(ridges * style.ridge_gain, 0.0, 1.0),
        "hills": np.clip(hills * style.hill_gain, 0.0, 1.0),
        "valleys": np.clip((valleys + 0.5 * broad_valleys) * style.valley_gain, 0.0, 1.0),
        "upland": np.clip(upland * style.upland_gain, 0.0, 1.0),
        "style": style,
    }
