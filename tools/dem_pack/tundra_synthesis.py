"""Tundra-only reference-kernel synthesis experiments.

This setup pass keeps tundra low and cold: broad Arctic plains, patterned
ground, glacial fringe terrain, and sparse polar foothills. It is terrain-only;
snow/ice/permafrost materials are later work.

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
  - plain blur sigma=5.8 -> reach 23 px
  - pattern_combo blur sigma=1.2 -> reach 5 px (parallel, depth 23)
  - fringe blur sigma=1.8 -> reach 7 px (parallel, depth 23)
  - flow pre-blur sigma=1.15 -> reach 5 px (MFD convergence dominates over blur-reach)
  - drainage spread blur sigma=2.0 -> reach 8 px; MFD depth dominates; chain = 23+8 = 31
  - base blur sigma=max_smoothing_px=5.0 -> reach 20 px; input depth=31: chain = 31+20 = 51
  - final blend sigma=1.1 -> reach 5 px; total = 51+5 = 56 (blur-reach budget)

The blur-reach budget alone is ~56, BUT the dominant residual is the REAL
flow-accumulation convergence error, which is SCALE-DEPENDENT (same finding as
mountain). So: -> ``TUNDRA_APRON_PX = 160`` -- matches mountain's calibrated
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
# must supply when calling generate(apron_px=TUNDRA_APRON_PX).
# See module docstring for the derivation.
# ---------------------------------------------------------------------------
TUNDRA_APRON_PX: int = 160

# ---------------------------------------------------------------------------
# Affine-remap constants (replace per-window zscore / norm01).
# Chosen from empirical statistics across seeds 0, 7, 42 on 96x96 / 90 km grids,
# all four styles combined.
# Target: same STRUCTURE and similar tone to the original zscore/norm01 output.
#
# norm01(x) -> affine_remap(x, center=mean_min, scale=1/mean_ptp)
# zscore(x) -> affine_remap(x, center=mean, scale=1/std)
# ---------------------------------------------------------------------------

# macro fbm (norm01): mean_min=-0.668, ptp=1.398
TUNDRA_MACRO_CENTER: float = -0.668
TUNDRA_MACRO_SCALE: float = 0.715

# macro normed (second-stage zscore for height term):
# norm01(macro_raw) output mean=0.497, std=0.236 -> zscore: center=0.497, scale=1/0.236
TUNDRA_MACRO_ZSCORE_CENTER: float = 0.497
TUNDRA_MACRO_ZSCORE_SCALE: float = 4.24

# flow_source_inner (zscore): mean=0.1525, std=0.1761
TUNDRA_FLOW_SOURCE_CENTER: float = 0.153
TUNDRA_FLOW_SOURCE_SCALE: float = 5.68

# fine fbm (zscore): mean=-0.001, std=0.309
TUNDRA_FINE_CENTER: float = 0.000
TUNDRA_FINE_SCALE: float = 3.24

# base_inner (0.74*macro+0.26*foothills) -> zscore then gaussian_filter:
# zscore: mean=0.405, std=0.185 -> center=0.405, scale=1/0.185
TUNDRA_BASE_CENTER: float = 0.405
TUNDRA_BASE_SCALE: float = 5.41

# final blend (replaces trailing zscore): pre-final mean=0.037, std=0.424
# FINAL_SCALE tuned to 0.82 to keep amplitude near legacy std~1 after non-linear mixing.
TUNDRA_FINAL_CENTER: float = 0.000
TUNDRA_FINAL_SCALE: float = 0.82


@dataclass(frozen=True)
class TundraStyle:
    key: str
    label: str
    angle_rad: float
    plain_gain: float = 1.0
    pattern_gain: float = 1.0
    fringe_gain: float = 1.0
    foothill_gain: float = 1.0
    drainage_gain: float = 1.0
    texture_gain: float = 1.0
    smoothing_px: float = 3.0
    seed_offset: int = 0


STYLES = (
    TundraStyle(
        "arctic_plain",
        "arctic plain",
        angle_rad=0.10,
        plain_gain=1.30,
        pattern_gain=0.32,
        fringe_gain=0.18,
        foothill_gain=0.22,
        drainage_gain=0.48,
        texture_gain=0.22,
        smoothing_px=5.0,
        seed_offset=0,
    ),
    TundraStyle(
        "patterned_ground",
        "patterned ground",
        angle_rad=-0.38,
        plain_gain=0.96,
        pattern_gain=1.28,
        fringe_gain=0.22,
        foothill_gain=0.26,
        drainage_gain=0.52,
        texture_gain=0.30,
        smoothing_px=4.4,
        seed_offset=1000,
    ),
    TundraStyle(
        "glacial_fringe",
        "glacial fringe",
        angle_rad=0.52,
        plain_gain=0.78,
        pattern_gain=0.44,
        fringe_gain=1.22,
        foothill_gain=0.52,
        drainage_gain=0.82,
        texture_gain=0.34,
        smoothing_px=3.7,
        seed_offset=2000,
    ),
    TundraStyle(
        "polar_foothills",
        "polar foothills",
        angle_rad=-0.78,
        plain_gain=0.58,
        pattern_gain=0.34,
        fringe_gain=0.48,
        foothill_gain=1.28,
        drainage_gain=0.72,
        texture_gain=0.42,
        smoothing_px=3.0,
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


def _drainage_channels_seam_safe(
    surface: np.ndarray,
    *,
    mode: str = "nearest",
    power: float = 0.48,
) -> np.ndarray:
    """Seam-safe CONNECTED drainage: real MFD flow accumulation + FIXED-max normalization.

    Mirrors mountain_synthesis._flow_channels_seam_safe exactly, using tundra's
    flow power (0.48) and spread sigma (2.0).  See mountain_synthesis docstring for
    the probe measurements that confirm convergence at apron 160.

    Reach: pre-blur sigma=1.15 -> reach 5 px; MFD convergence (not blur-reach) sets
    the apron requirement; spread blur sigma=2.0 -> reach 8 px.
    """
    pre = gaussian_filter(np.asarray(surface, dtype=np.float64), sigma=1.15, mode=mode)
    acc = skel._flow_accumulation_mfd(pre, power=float(power))
    # FIXED-max normalization (SEAM-SAFE: no per-window statistics).
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    # Spread channel extent with nearest-mode blur (tundra drainage spread sigma=2.0).
    return np.clip(
        gaussian_filter(discharge, sigma=2.0, mode=mode),
        0.0,
        1.0,
    )


def generate(
    wx: np.ndarray,
    wz: np.ndarray,
    seed: int = 0,
    style: TundraStyle = STYLES[0],
    feature_span_m: float | None = None,
    apron_px: int = 0,
) -> dict[str, np.ndarray | TundraStyle]:
    """Generate one tundra setup candidate with diagnostic masks.

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
        Use ``TUNDRA_APRON_PX`` for the correct value.
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
        warp_amount=feature_span * 0.020,
        warp_freq=1.0 / (feature_span * 0.86),
        seed=sseed + 10,
        steps=3,
        decay=0.54,
        freq_mul=1.72,
    )

    if seam_safe_mode:
        macro_raw = wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.94), 5, sseed + 30, gain=0.58)
        macro = np.clip(ss.affine_remap(macro_raw, TUNDRA_MACRO_CENTER, TUNDRA_MACRO_SCALE), 0.0, 1.0)
        plain = smoothstep(0.36, 0.76, gaussian_filter(1.0 - np.abs(macro - 0.46), sigma=5.8, mode=blur_mode))
        rx, rz = _rotated(w_x, w_z, style.angle_rad, cx=0.0, cz=0.0)
        polygons = wg.cellular_edges(rx, rz, 1.0 / (feature_span * 0.030), sseed + 70, sharpness=1.70)
        stripes = wg.ridged_multifractal(rx, rz * 0.18, 1.0 / (feature_span * 0.055), 4, sseed + 90, gain=0.48)
        pattern = smoothstep(0.46, 0.86, gaussian_filter(0.56 * polygons + 0.44 * stripes, sigma=1.2, mode=blur_mode)) * plain

        fringe_ridges = wg.ridged_multifractal(w_x, w_z * 0.48, 1.0 / (feature_span * 0.16), 5, sseed + 130, gain=0.52)
        fringe = smoothstep(0.42, 0.84, gaussian_filter(fringe_ridges, sigma=1.8, mode=blur_mode))
        foothills = smoothstep(0.40, 0.80, wg.ridged_multifractal(rx, rz * 0.48, 1.0 / (feature_span * 0.22), 5, sseed + 160, gain=0.52))

        flow_source_inner = 0.62 * macro + 0.26 * foothills + 0.22 * fringe - 0.22 * plain
        flow_source = ss.affine_remap(flow_source_inner, TUNDRA_FLOW_SOURCE_CENTER, TUNDRA_FLOW_SOURCE_SCALE)
        channels = _drainage_channels_seam_safe(flow_source, mode=blur_mode, power=0.48)
        drainage = smoothstep(0.58, 0.94, channels)

        fine = ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.026), 3, sseed + 220, gain=0.44),
            TUNDRA_FINE_CENTER, TUNDRA_FINE_SCALE,
        )

        base_inner = 0.74 * macro + 0.26 * foothills
        base = gaussian_filter(
            ss.affine_remap(base_inner, TUNDRA_BASE_CENTER, TUNDRA_BASE_SCALE),
            sigma=style.smoothing_px,
            mode=blur_mode,
        )

        macro_zsc = ss.affine_remap(macro, TUNDRA_MACRO_ZSCORE_CENTER, TUNDRA_MACRO_ZSCORE_SCALE)
        height = 0.24 * style.plain_gain * macro_zsc
        height += 0.10 * style.pattern_gain * pattern
        height += 0.34 * style.fringe_gain * fringe
        height += 0.40 * style.foothill_gain * foothills
        height -= 0.22 * style.drainage_gain * drainage
        height += 0.045 * style.texture_gain * fine * (0.45 + 0.55 * pattern)
        height = 0.72 * height + 0.28 * base
        final_blend = 0.86 * height + 0.14 * gaussian_filter(height, sigma=1.1, mode=blur_mode)
        height = ss.affine_remap(final_blend, TUNDRA_FINAL_CENTER, TUNDRA_FINAL_SCALE)
    else:
        macro = norm01(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.94), 5, sseed + 30, gain=0.58))
        plain = smoothstep(0.36, 0.76, gaussian_filter(1.0 - np.abs(macro - 0.46), sigma=5.8))
        rx, rz = _rotated(w_x, w_z, style.angle_rad)
        polygons = wg.cellular_edges(rx, rz, 1.0 / (feature_span * 0.030), sseed + 70, sharpness=1.70)
        stripes = wg.ridged_multifractal(rx, rz * 0.18, 1.0 / (feature_span * 0.055), 4, sseed + 90, gain=0.48)
        pattern = smoothstep(0.46, 0.86, gaussian_filter(0.56 * polygons + 0.44 * stripes, sigma=1.2)) * plain

        fringe_ridges = wg.ridged_multifractal(w_x, w_z * 0.48, 1.0 / (feature_span * 0.16), 5, sseed + 130, gain=0.52)
        fringe = smoothstep(0.42, 0.84, gaussian_filter(fringe_ridges, sigma=1.8))
        foothills = smoothstep(0.40, 0.80, wg.ridged_multifractal(rx, rz * 0.48, 1.0 / (feature_span * 0.22), 5, sseed + 160, gain=0.52))

        flow_source = zscore(0.62 * macro + 0.26 * foothills + 0.22 * fringe - 0.22 * plain)
        channels = wg.flow_accumulation_channels(flow_source, power=0.48)
        drainage = smoothstep(0.58, 0.94, gaussian_filter(channels, sigma=2.0))

        fine = zscore(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.026), 3, sseed + 220, gain=0.44))
        base = gaussian_filter(zscore(0.74 * macro + 0.26 * foothills), sigma=style.smoothing_px)

        height = 0.24 * style.plain_gain * zscore(macro)
        height += 0.10 * style.pattern_gain * pattern
        height += 0.34 * style.fringe_gain * fringe
        height += 0.40 * style.foothill_gain * foothills
        height -= 0.22 * style.drainage_gain * drainage
        height += 0.045 * style.texture_gain * fine * (0.45 + 0.55 * pattern)
        height = 0.72 * height + 0.28 * base
        height = zscore(0.86 * height + 0.14 * gaussian_filter(height, sigma=1.1))

    # --- crop to core ---
    if seam_safe_mode and a > 0:
        height   = np.ascontiguousarray(height[a:-a, a:-a])
        plain    = np.ascontiguousarray(plain[a:-a, a:-a])
        pattern  = np.ascontiguousarray(pattern[a:-a, a:-a])
        fringe   = np.ascontiguousarray(fringe[a:-a, a:-a])
        foothills = np.ascontiguousarray(foothills[a:-a, a:-a])
        drainage = np.ascontiguousarray(drainage[a:-a, a:-a])

    return {
        "height": height,
        "plain": np.clip(plain * style.plain_gain, 0.0, 1.0),
        "pattern": np.clip(pattern * style.pattern_gain, 0.0, 1.0),
        "fringe": np.clip(fringe * style.fringe_gain, 0.0, 1.0),
        "foothills": np.clip(foothills * style.foothill_gain, 0.0, 1.0),
        "drainage": np.clip(drainage * style.drainage_gain, 0.0, 1.0),
        "style": style,
    }
