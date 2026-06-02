"""Rainforest-only reference-kernel synthesis experiments.

Rainforest terrain is mostly a humid-process family: rounded dissected hills,
shield/plateau surfaces, foothill ridges, and dense drainage. Vegetation is not
modeled here; this pass builds the landform read.

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
4. Drainage carving uses REAL multiple-flow-direction (MFD) accumulation
   (``geography_skeleton._flow_accumulation_mfd``) with a FIXED-max normalisation
   (``log1p(acc) / log1p(acc.size)`` -- data-independent, NOT per-window max).
   Global flow accumulation is not bit-exact on a finite apron, but it CONVERGES
   to bit-exact as the apron grows: see mountain_synthesis docstring for the
   probe measurements that motivated apron 160.
5. A single crop ``height[a:-a, a:-a]`` is performed at the very end.

Required apron (computed as sum of chained kernel reaches along the deepest path):
  - lowland blur sigma=5.4 -> reach 22 px
  - plateau blur sigma=4.5 -> reach 18 px (parallel path; not deeper than lowland)
  - flow pre-blur sigma=1.15 -> reach 5 px (MFD convergence dominates over blur-reach)
  - tributary spread blur sigma=1.15 -> reach 5 px; trunk spread sigma=2.2 -> reach 9 px
    MFD depth dominates; chain drainage depth = 22+9 = 31
  - wet_rounding blur max sigma=3.7 -> reach 15 px; input depth=31; chain = 31+15 = 46
  - final blend sigma=1.0 -> reach 4 px; total = 46+4 = 50 (blur-reach budget)

The blur-reach budget alone is ~50, BUT the dominant residual is the REAL
flow-accumulation convergence error, which is SCALE-DEPENDENT (same finding as
mountain). So: -> ``RAINFOREST_APRON_PX = 160`` -- matches mountain's calibrated
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
# must supply when calling generate(apron_px=RAINFOREST_APRON_PX).
# See module docstring for the derivation.
# ---------------------------------------------------------------------------
RAINFOREST_APRON_PX: int = 160

# ---------------------------------------------------------------------------
# Affine-remap constants (replace per-window zscore / norm01).
# Chosen from empirical statistics across seeds 0, 7, 42, 71 on 96x96 / 90 km grids,
# all four styles combined.
# Target: same STRUCTURE and similar tone to the original zscore/norm01 output.
#
# norm01(x) -> affine_remap(x, center=mean_min, scale=1/mean_ptp)
# zscore(x) -> affine_remap(x, center=mean, scale=1/std)
# ---------------------------------------------------------------------------

# macro fbm (norm01): mean_min=-0.6668, ptp=1.3956
RAINFOREST_MACRO_CENTER: float = -0.667
RAINFOREST_MACRO_SCALE: float = 0.717

# hills = norm01(gaussian_filter(ridged_mf, sigma=1.7)):
# blurred_raw min=0.0001, ptp=0.8339
RAINFOREST_HILLS_CENTER: float = 0.000
RAINFOREST_HILLS_SCALE: float = 1.199

# plateau_seed fbm (norm01): mean_min=-0.8466, ptp=1.5972
RAINFOREST_PLATEAU_SEED_CENTER: float = -0.847
RAINFOREST_PLATEAU_SEED_SCALE: float = 0.626

# flow_source (zscore of combo): mean=0.4806, std=0.3269
RAINFOREST_FLOW_CENTER: float = 0.481
RAINFOREST_FLOW_SCALE: float = 3.059

# close fbm (zscore): mean=-0.0033~=0.0, std=0.2910
RAINFOREST_CLOSE_CENTER: float = 0.000
RAINFOREST_CLOSE_SCALE: float = 3.436

# wet_rounding_raw (zscore input = 0.62*macro + 0.36*hills + 0.26*plateau):
# mean=0.5025, std=0.1974
RAINFOREST_WET_ROUNDING_CENTER: float = 0.503
RAINFOREST_WET_ROUNDING_SCALE: float = 5.066

# hills_norm01 used in zscore(hills) for height: mean=0.3855, std=0.2525
RAINFOREST_HILLS_ZSCORE_CENTER: float = 0.386
RAINFOREST_HILLS_ZSCORE_SCALE: float = 3.960

# final blend (replaces trailing zscore): pre_final mean=-0.061, std=0.566
# Scale tuned to 1.70 so amplitude stays near legacy std~1 after the blend.
RAINFOREST_FINAL_CENTER: float = 0.000
RAINFOREST_FINAL_SCALE: float = 1.70


@dataclass(frozen=True)
class RainforestStyle:
    key: str
    label: str
    angle_rad: float
    hill_gain: float = 1.0
    ridge_gain: float = 1.0
    drainage_gain: float = 1.0
    plateau_gain: float = 1.0
    lowland_gain: float = 1.0
    texture_gain: float = 1.0
    smoothing_px: float = 2.0
    seed_offset: int = 0


STYLES = (
    RainforestStyle(
        "humid_dissected_hills",
        "humid dissected hills",
        angle_rad=0.42,
        hill_gain=1.18,
        ridge_gain=0.78,
        drainage_gain=1.18,
        plateau_gain=0.36,
        lowland_gain=0.30,
        texture_gain=0.58,
        smoothing_px=2.6,
        seed_offset=0,
    ),
    RainforestStyle(
        "shield_plateau",
        "shield plateau",
        angle_rad=-0.18,
        hill_gain=0.82,
        ridge_gain=0.54,
        drainage_gain=0.92,
        plateau_gain=1.28,
        lowland_gain=0.38,
        texture_gain=0.42,
        smoothing_px=3.2,
        seed_offset=1000,
    ),
    RainforestStyle(
        "jungle_foothills",
        "jungle foothills",
        angle_rad=0.86,
        hill_gain=1.06,
        ridge_gain=1.26,
        drainage_gain=1.02,
        plateau_gain=0.28,
        lowland_gain=0.30,
        texture_gain=0.66,
        smoothing_px=2.1,
        seed_offset=2000,
    ),
    RainforestStyle(
        "river_lowland",
        "river lowland",
        angle_rad=-0.64,
        hill_gain=0.58,
        ridge_gain=0.34,
        drainage_gain=1.34,
        plateau_gain=0.22,
        lowland_gain=1.80,
        texture_gain=0.30,
        smoothing_px=3.7,
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


def _drainage_seam_safe(
    surface: np.ndarray,
    *,
    mode: str = "nearest",
    power: float = 0.38,
) -> tuple[np.ndarray, np.ndarray]:
    """Seam-safe CONNECTED drainage: real MFD flow accumulation + FIXED-max normalization.

    Mirrors mountain_synthesis._flow_channels_seam_safe exactly, using rainforest's
    flow power (0.38) and the two spread sigmas (1.15 for tributaries, 2.2 for trunk).
    See mountain_synthesis docstring for the probe measurements that confirm convergence
    at apron 160.

    Returns (tributaries, trunk) both in [0, 1].
    """
    pre = gaussian_filter(np.asarray(surface, dtype=np.float64), sigma=1.15, mode=mode)
    acc = skel._flow_accumulation_mfd(pre, power=float(power))
    # FIXED-max normalization (SEAM-SAFE: no per-window statistics).
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    tributaries = smoothstep(0.42, 0.88, gaussian_filter(discharge, sigma=1.15, mode=mode))
    trunk = smoothstep(0.68, 0.95, gaussian_filter(discharge, sigma=2.2, mode=mode))
    return tributaries, trunk


def generate(
    wx: np.ndarray,
    wz: np.ndarray,
    seed: int = 0,
    style: RainforestStyle = STYLES[0],
    feature_span_m: float | None = None,
    apron_px: int = 0,
) -> dict[str, np.ndarray | RainforestStyle]:
    """Generate one rainforest setup candidate with diagnostic masks.

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
        Use ``RAINFOREST_APRON_PX`` for the correct value.
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
        warp_amount=feature_span * 0.034,
        warp_freq=1.0 / (feature_span * 0.72),
        seed=sseed + 10,
        steps=4,
        decay=0.54,
        freq_mul=1.74,
    )

    if seam_safe_mode:
        macro = np.clip(ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.78), 5, sseed + 30, gain=0.58),
            RAINFOREST_MACRO_CENTER, RAINFOREST_MACRO_SCALE,
        ), 0.0, 1.0)
        hills = np.clip(ss.affine_remap(
            gaussian_filter(wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.24), 5, sseed + 60, gain=0.52), sigma=1.7, mode=blur_mode),
            RAINFOREST_HILLS_CENTER, RAINFOREST_HILLS_SCALE,
        ), 0.0, 1.0)
        # Rotation uses fixed world origin (0, 0) -- seam-safe.
        rx, rz = _rotated(w_x, w_z, style.angle_rad, cx=0.0, cz=0.0)
        ridges = smoothstep(0.42, 0.83, wg.ridged_multifractal(rx, rz * 0.42, 1.0 / (feature_span * 0.16), 5, sseed + 90, gain=0.50))
        plateau_seed = np.clip(ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.44), 4, sseed + 130, gain=0.55),
            RAINFOREST_PLATEAU_SEED_CENTER, RAINFOREST_PLATEAU_SEED_SCALE,
        ), 0.0, 1.0)
        plateau = smoothstep(0.54, 0.80, gaussian_filter(plateau_seed, sigma=4.5, mode=blur_mode)) * (1.0 - 0.38 * ridges)
        lowland_source = gaussian_filter(1.0 - macro, sigma=5.4, mode=blur_mode)
        lowland = smoothstep(0.57 - 0.10 * style.lowland_gain, 0.88 - 0.06 * style.lowland_gain, lowland_source)

        flow_source = ss.affine_remap(
            0.66 * macro + 0.46 * hills + 0.28 * ridges + 0.20 * plateau - 0.36 * lowland,
            RAINFOREST_FLOW_CENTER, RAINFOREST_FLOW_SCALE,
        )
        tributaries, trunk = _drainage_seam_safe(flow_source, mode=blur_mode, power=0.38)
        drainage = np.clip(0.68 * tributaries + 0.58 * trunk, 0.0, 1.0)

        close = ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.030), 4, sseed + 210, gain=0.45),
            RAINFOREST_CLOSE_CENTER, RAINFOREST_CLOSE_SCALE,
        )
        wet_rounding = gaussian_filter(
            ss.affine_remap(0.62 * macro + 0.36 * hills + 0.26 * plateau, RAINFOREST_WET_ROUNDING_CENTER, RAINFOREST_WET_ROUNDING_SCALE),
            sigma=style.smoothing_px, mode=blur_mode,
        )

        height = 0.46 * style.hill_gain * ss.affine_remap(hills, RAINFOREST_HILLS_ZSCORE_CENTER, RAINFOREST_HILLS_ZSCORE_SCALE)
        height += 0.34 * style.ridge_gain * ridges
        height += 0.30 * style.plateau_gain * plateau
        height -= 0.38 * style.lowland_gain * lowland
        height -= 0.34 * style.drainage_gain * drainage
        height += style.texture_gain * (0.055 * close + 0.045 * close * ridges)
        height = 0.72 * height + 0.28 * wet_rounding
        final_blend = 0.84 * height + 0.16 * gaussian_filter(height, sigma=1.0, mode=blur_mode)
        height = ss.affine_remap(final_blend, RAINFOREST_FINAL_CENTER, RAINFOREST_FINAL_SCALE)
    else:
        macro = norm01(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.78), 5, sseed + 30, gain=0.58))
        hills = norm01(gaussian_filter(wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.24), 5, sseed + 60, gain=0.52), sigma=1.7))
        rx, rz = _rotated(w_x, w_z, style.angle_rad)
        ridges = smoothstep(0.42, 0.83, wg.ridged_multifractal(rx, rz * 0.42, 1.0 / (feature_span * 0.16), 5, sseed + 90, gain=0.50))
        plateau_seed = norm01(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.44), 4, sseed + 130, gain=0.55))
        plateau = smoothstep(0.54, 0.80, gaussian_filter(plateau_seed, sigma=4.5)) * (1.0 - 0.38 * ridges)
        lowland_source = gaussian_filter(1.0 - macro, sigma=5.4)
        lowland = smoothstep(0.57 - 0.10 * style.lowland_gain, 0.88 - 0.06 * style.lowland_gain, lowland_source)

        flow_source = zscore(0.66 * macro + 0.46 * hills + 0.28 * ridges + 0.20 * plateau - 0.36 * lowland)
        channels = wg.flow_accumulation_channels(flow_source, power=0.38)
        tributaries = smoothstep(0.42, 0.88, gaussian_filter(channels, sigma=1.15))
        trunk = smoothstep(0.68, 0.95, gaussian_filter(channels, sigma=2.2))
        drainage = np.clip(0.68 * tributaries + 0.58 * trunk, 0.0, 1.0)

        close = zscore(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.030), 4, sseed + 210, gain=0.45))
        wet_rounding = gaussian_filter(zscore(0.62 * macro + 0.36 * hills + 0.26 * plateau), sigma=style.smoothing_px)

        height = 0.46 * style.hill_gain * zscore(hills)
        height += 0.34 * style.ridge_gain * ridges
        height += 0.30 * style.plateau_gain * plateau
        height -= 0.38 * style.lowland_gain * lowland
        height -= 0.34 * style.drainage_gain * drainage
        height += style.texture_gain * (0.055 * close + 0.045 * close * ridges)
        height = 0.72 * height + 0.28 * wet_rounding
        height = zscore(0.84 * height + 0.16 * gaussian_filter(height, sigma=1.0))

    # --- crop to core ---
    if seam_safe_mode and a > 0:
        height     = np.ascontiguousarray(height[a:-a, a:-a])
        hills      = np.ascontiguousarray(hills[a:-a, a:-a])
        ridges     = np.ascontiguousarray(ridges[a:-a, a:-a])
        drainage   = np.ascontiguousarray(drainage[a:-a, a:-a])
        plateau    = np.ascontiguousarray(plateau[a:-a, a:-a])
        lowland    = np.ascontiguousarray(lowland[a:-a, a:-a])

    return {
        "height": height,
        "hills": np.clip(hills * style.hill_gain, 0.0, 1.0),
        "ridges": np.clip(ridges * style.ridge_gain, 0.0, 1.0),
        "drainage": np.clip(drainage * style.drainage_gain, 0.0, 1.0),
        "plateau": np.clip(plateau * style.plateau_gain, 0.0, 1.0),
        "lowland": np.clip(lowland * style.lowland_gain, 0.0, 1.0),
        "style": style,
    }
