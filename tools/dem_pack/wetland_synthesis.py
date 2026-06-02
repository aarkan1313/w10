"""Wetland-only reference-kernel synthesis experiments.

This setup pass is terrain/mask only: deltas, floodplains, peat bog lowlands,
and swamp/backwater channels. Actual water rendering, flooding, and materials
belong to later runtime phases.

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
4. Channel routing uses REAL multiple-flow-direction (MFD) accumulation
   (``geography_skeleton._flow_accumulation_mfd``) with a FIXED-max normalisation
   (``log1p(acc) / log1p(acc.size)`` -- data-independent, NOT per-window max).
   Global flow accumulation is not bit-exact on a finite apron, but it CONVERGES
   to bit-exact as the apron grows: see mountain_synthesis docstring for the
   probe measurements that motivated apron 160.
5. A single crop ``height[a:-a, a:-a]`` is performed at the very end.

Required apron (computed as sum of chained kernel reaches along the deepest path):
  - macro blur (via gaussian_filter on 1-macro): sigma=5.8 -> reach 23 px
  - floodplain blur: sigma=5.2 -> reach 21 px; parallel path, depth capped at 23
  - fine_flow MFD: convergence dominates over blur-reach (same as mountain)
  - fine_flow spread blur: sigma=1.8 -> reach 7 px; chain = 23+7 = 30
  - flat_base blur: sigma=max_smoothing_px=6.0 -> reach 24; depth=30: chain = 30+24 = 54
  - final blend sigma=1.2 -> reach 5; total = 54+5 = 59 (blur-reach budget)

The blur-reach budget alone is ~59, BUT the dominant residual is the REAL
flow-accumulation convergence error, which is SCALE-DEPENDENT (same finding as
mountain). So: -> ``WETLAND_APRON_PX = 160`` -- matches mountain's calibrated
floor (see mountain_synthesis docstring for the 7x7 / 175 km world measurement).
Wetland is flat/low-relief with connected channels -- the MFD drainage matters here
exactly as much as in mountain (connected floodplain routing), so the same apron
floor applies.
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
# must supply when calling generate(apron_px=WETLAND_APRON_PX).
# See module docstring for the derivation.
# ---------------------------------------------------------------------------
WETLAND_APRON_PX: int = 160

# ---------------------------------------------------------------------------
# Affine-remap constants (replace per-window zscore / norm01).
# Chosen from empirical statistics across seeds 0, 7, 42 on 96x96 / 90 km grids,
# delta + meander styles.
# Target: same STRUCTURE and similar tone to the original zscore/norm01 output.
#
# norm01(x) -> affine_remap(x, center=mean_min, scale=1/mean_ptp)
# zscore(x) -> affine_remap(x, center=mean, scale=1/std)
# ---------------------------------------------------------------------------

# macro fbm (norm01): mean_min=-0.38, mean_ptp=0.876
WETLAND_MACRO_CENTER: float = -0.38
WETLAND_MACRO_SCALE: float = 1.14

# flow_input = macro - 0.34*basin (zscore input for fine_flow):
# mean~0.28, std~0.33 -> scale=1/0.33=3.00
WETLAND_FLOW_INPUT_CENTER: float = 0.28
WETLAND_FLOW_INPUT_SCALE: float = 3.00

# micro fbm (zscore): mean~-0.007, std~0.304 -> scale=1/0.304=3.29
WETLAND_MICRO_CENTER: float = 0.00
WETLAND_MICRO_SCALE: float = 3.29

# flat_base_input = 0.42*macro - 0.58*basin + 0.20*floodplain (zscore):
# mean~0.13, std~0.287 -> scale=1/0.287=3.49
WETLAND_FLAT_BASE_CENTER: float = 0.13
WETLAND_FLAT_BASE_SCALE: float = 3.49

# macro for height zscore: macro is [0,1] after norm01, mean~0.5, std~0.25
# zscore(macro) -> center=0.5, scale=4.0 (generous to recover full range)
WETLAND_MACRO_ZSCORE_CENTER: float = 0.50
WETLAND_MACRO_ZSCORE_SCALE: float = 4.00

# final blend: affine replaces trailing zscore; tuned to keep amplitude near legacy.
# Upstream signal hovers ~std 1 by construction; 0.82 scale gives similar amplitude.
WETLAND_FINAL_CENTER: float = 0.00
WETLAND_FINAL_SCALE: float = 0.82


@dataclass(frozen=True)
class WetlandStyle:
    key: str
    label: str
    angle_rad: float
    channel_gain: float = 1.0
    floodplain_gain: float = 1.0
    levee_gain: float = 1.0
    basin_gain: float = 1.0
    texture_gain: float = 1.0
    smoothing_px: float = 4.0
    seed_offset: int = 0


STYLES = (
    WetlandStyle(
        "delta_distributary",
        "delta distributary",
        angle_rad=0.08,
        channel_gain=1.32,
        floodplain_gain=1.08,
        levee_gain=0.90,
        basin_gain=0.74,
        texture_gain=0.32,
        smoothing_px=4.4,
        seed_offset=0,
    ),
    WetlandStyle(
        "meander_floodplain",
        "meander floodplain",
        angle_rad=-0.44,
        channel_gain=1.02,
        floodplain_gain=1.28,
        levee_gain=1.10,
        basin_gain=0.62,
        texture_gain=0.26,
        smoothing_px=4.8,
        seed_offset=1000,
    ),
    WetlandStyle(
        "peat_bog_lowland",
        "peat bog lowland",
        angle_rad=0.62,
        channel_gain=0.46,
        floodplain_gain=0.86,
        levee_gain=0.26,
        basin_gain=1.34,
        texture_gain=0.18,
        smoothing_px=6.0,
        seed_offset=2000,
    ),
    WetlandStyle(
        "swamp_backwater",
        "swamp backwater",
        angle_rad=-0.18,
        channel_gain=0.86,
        floodplain_gain=1.12,
        levee_gain=0.52,
        basin_gain=1.05,
        texture_gain=0.24,
        smoothing_px=5.2,
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


def _meander_field(
    wx: np.ndarray,
    wz: np.ndarray,
    feature_span_m: float,
    style: WetlandStyle,
    seed: int,
    *,
    seam_safe_mode: bool = False,
) -> np.ndarray:
    rot_cx = 0.0 if seam_safe_mode else None
    rot_cz = 0.0 if seam_safe_mode else None
    rx, rz = _rotated(wx, wz, style.angle_rad, cx=rot_cx, cz=rot_cz)
    meander = wg.fbm(rx, rz, 1.0 / (feature_span_m * 0.24), 5, seed + 120, gain=0.55) * feature_span_m * 0.050
    trunk_phase = (rz + meander) / max(feature_span_m * 0.090, 1.0) * np.pi * 2.0
    trunk = np.exp(-((np.sin(trunk_phase) / 0.18) ** 2))
    distributary = wg.ridged_multifractal(rx + meander, rz * 0.38, 1.0 / (feature_span_m * 0.13), 4, seed + 140, gain=0.50)
    return np.clip(0.62 * trunk + 0.58 * smoothstep(0.50, 0.88, distributary), 0.0, 1.0)


def _fine_flow_seam_safe(
    surface: np.ndarray,
    *,
    mode: str = "nearest",
    power: float = 0.44,
) -> np.ndarray:
    """Seam-safe CONNECTED drainage: real MFD flow accumulation + FIXED-max normalization.

    Mirrors mountain_synthesis._flow_channels_seam_safe exactly, using wetland's
    flow power (0.44) and spread sigma (1.8).  See mountain_synthesis docstring for
    the probe measurements that confirm convergence at apron 160.

    Wetland is flat/low-relief with connected channels -- the MFD flow matters here
    exactly as much as in mountain (connected floodplain drainage), so we keep REAL
    MFD rather than a proxy.

    Reach: pre-blur sigma=1.15 -> reach 5 px; MFD convergence (not blur-reach) sets
    the apron requirement; spread blur sigma=1.8 -> reach 7 px.
    """
    pre = gaussian_filter(np.asarray(surface, dtype=np.float64), sigma=1.15, mode=mode)
    acc = skel._flow_accumulation_mfd(pre, power=float(power))
    # FIXED-max normalization (SEAM-SAFE: no per-window statistics).
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    # Spread channel extent with nearest-mode blur (wetland spread sigma=1.8).
    return np.clip(
        gaussian_filter(discharge, sigma=1.8, mode=mode),
        0.0,
        1.0,
    )


def generate(
    wx: np.ndarray,
    wz: np.ndarray,
    seed: int = 0,
    style: WetlandStyle = STYLES[0],
    feature_span_m: float | None = None,
    apron_px: int = 0,
) -> dict[str, np.ndarray | WetlandStyle]:
    """Generate one wetland setup candidate with diagnostic masks.

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
        Use ``WETLAND_APRON_PX`` for the correct value.
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
        warp_amount=feature_span * 0.018,
        warp_freq=1.0 / (feature_span * 0.88),
        seed=sseed + 10,
        steps=3,
        decay=0.54,
        freq_mul=1.68,
    )

    if seam_safe_mode:
        macro = np.clip(ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.96), 5, sseed + 30, gain=0.58),
            WETLAND_MACRO_CENTER, WETLAND_MACRO_SCALE,
        ), 0.0, 1.0)
        basin = smoothstep(0.48, 0.86, gaussian_filter(1.0 - macro, sigma=5.8, mode=blur_mode))
        floodplain = smoothstep(0.36, 0.78, gaussian_filter(1.0 - np.abs(macro - 0.42), sigma=5.2, mode=blur_mode))
        channels = _meander_field(w_x, w_z, feature_span, style, sseed, seam_safe_mode=True) * floodplain
        # Seam-safe fine_flow: replace zscore(flow_input) + legacy flow_accumulation_channels
        # with affine_remap + real MFD accumulation (fixed-max normalized).
        flow_input = ss.affine_remap(macro - 0.34 * basin, WETLAND_FLOW_INPUT_CENTER, WETLAND_FLOW_INPUT_SCALE)
        fine_flow = _fine_flow_seam_safe(flow_input, mode=blur_mode, power=0.44)
        channels = np.clip(0.68 * channels + 0.50 * smoothstep(0.56, 0.94, fine_flow), 0.0, 1.0)
        # Levees: DoG on channels; channels is already [0,1]; gaussian blurs are seam-safe with nearest
        levees = gaussian_filter(channels, sigma=2.2, mode=blur_mode) - gaussian_filter(channels, sigma=5.2, mode=blur_mode)
        levees = smoothstep(0.02, 0.18, levees)
        levees *= 1.0 - smoothstep(0.42, 0.86, channels)
        backwater = smoothstep(0.52, 0.88, gaussian_filter(basin + 0.34 * floodplain - 0.42 * channels, sigma=3.4, mode=blur_mode))

        micro = ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.026), 3, sseed + 220, gain=0.44),
            WETLAND_MICRO_CENTER, WETLAND_MICRO_SCALE,
        )
        flat_base = gaussian_filter(
            ss.affine_remap(
                0.42 * macro - 0.58 * basin + 0.20 * floodplain,
                WETLAND_FLAT_BASE_CENTER, WETLAND_FLAT_BASE_SCALE,
            ),
            sigma=style.smoothing_px, mode=blur_mode,
        )

        height = ss.affine_remap(macro, WETLAND_MACRO_ZSCORE_CENTER, WETLAND_MACRO_ZSCORE_SCALE) * 0.18
        height -= 0.32 * style.basin_gain * basin
        height -= 0.28 * style.floodplain_gain * floodplain
        height -= 0.30 * style.channel_gain * channels
        height += 0.54 * style.levee_gain * levees
        height += 0.045 * style.texture_gain * micro * (0.30 + 0.70 * floodplain)
        height = 0.66 * height + 0.34 * flat_base
        final_blend = 0.88 * height + 0.12 * gaussian_filter(height, sigma=1.2, mode=blur_mode)
        height = ss.affine_remap(final_blend, WETLAND_FINAL_CENTER, WETLAND_FINAL_SCALE)
    else:
        macro = norm01(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.96), 5, sseed + 30, gain=0.58))
        basin = smoothstep(0.48, 0.86, gaussian_filter(1.0 - macro, sigma=5.8))
        floodplain = smoothstep(0.36, 0.78, gaussian_filter(1.0 - np.abs(macro - 0.42), sigma=5.2))
        channels = _meander_field(w_x, w_z, feature_span, style, sseed) * floodplain
        fine_flow = wg.flow_accumulation_channels(zscore(macro - 0.34 * basin), power=0.44)
        channels = np.clip(0.68 * channels + 0.50 * smoothstep(0.56, 0.94, gaussian_filter(fine_flow, sigma=1.8)), 0.0, 1.0)
        levees = gaussian_filter(channels, sigma=2.2) - gaussian_filter(channels, sigma=5.2)
        levees = smoothstep(0.02, 0.18, levees)
        levees *= 1.0 - smoothstep(0.42, 0.86, channels)
        backwater = smoothstep(0.52, 0.88, gaussian_filter(basin + 0.34 * floodplain - 0.42 * channels, sigma=3.4))

        micro = zscore(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.026), 3, sseed + 220, gain=0.44))
        flat_base = gaussian_filter(zscore(0.42 * macro - 0.58 * basin + 0.20 * floodplain), sigma=style.smoothing_px)

        height = 0.18 * zscore(macro)
        height -= 0.32 * style.basin_gain * basin
        height -= 0.28 * style.floodplain_gain * floodplain
        height -= 0.30 * style.channel_gain * channels
        height += 0.54 * style.levee_gain * levees
        height += 0.045 * style.texture_gain * micro * (0.30 + 0.70 * floodplain)
        height = 0.66 * height + 0.34 * flat_base
        height = zscore(0.88 * height + 0.12 * gaussian_filter(height, sigma=1.2))

    # --- crop to core ---
    if seam_safe_mode and a > 0:
        height     = np.ascontiguousarray(height[a:-a, a:-a])
        channels   = np.ascontiguousarray(channels[a:-a, a:-a])
        floodplain = np.ascontiguousarray(floodplain[a:-a, a:-a])
        levees     = np.ascontiguousarray(levees[a:-a, a:-a])
        basin      = np.ascontiguousarray(basin[a:-a, a:-a])
        backwater  = np.ascontiguousarray(backwater[a:-a, a:-a])

    return {
        "height": height,
        "channels": np.clip(channels * style.channel_gain, 0.0, 1.0),
        "floodplain": np.clip(floodplain * style.floodplain_gain, 0.0, 1.0),
        "levees": np.clip(levees * style.levee_gain, 0.0, 1.0),
        "basin": np.clip(basin * style.basin_gain, 0.0, 1.0),
        "backwater": np.clip(backwater * max(style.basin_gain, style.floodplain_gain), 0.0, 1.0),
        "style": style,
    }
