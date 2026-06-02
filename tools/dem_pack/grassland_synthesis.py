"""Grassland-only reference-kernel synthesis experiments.

This setup pass keeps grassland as low-to-moderate relief terrain with broad
swells, shallow draws, grass-stabilized dunes, steppe pans, and occasional
chapada/savanna plateau edges. It should not collapse into a flat color plane or
turn into small mountains.

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
4. Draw carving uses REAL multiple-flow-direction (MFD) accumulation
   (``geography_skeleton._flow_accumulation_mfd``) with a FIXED-max normalisation
   (``log1p(acc) / log1p(acc.size)`` -- data-independent, NOT per-window max).
   Global flow accumulation is not bit-exact on a finite apron, but it CONVERGES
   to bit-exact as the apron grows: see mountain_synthesis docstring for the
   probe measurements that motivated apron 160.
5. A single crop ``height[a:-a, a:-a]`` is performed at the very end.

Required apron (computed as sum of chained kernel reaches along the deepest path):
  - Swells blur sigma=max_smoothing_px=5.0 -> reach 20 px
  - Pans blur sigma=5.2 -> reach 21 px; runs on swells (same path): chain = 20+21 = 41
  - Draw spread blur sigma=2.1 -> reach 9 px; flow pre-blur sigma=1.15 -> reach 5 px
    (MFD convergence dominates over blur-reach); chain draw depth = 41+9 = 50
  - Smoothing blur sigma=5.0 -> reach 20; input depth=50; chain = 50+20 = 70
  - Final blend sigma=1.1 -> reach 5; total = 70+5 = 75 (blur-reach budget)

The blur-reach budget alone is ~75, BUT the dominant residual is the REAL
flow-accumulation convergence error, which is SCALE-DEPENDENT (same finding as
mountain). So: -> ``GRASSLAND_APRON_PX = 160`` -- matches mountain's calibrated
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
# must supply when calling generate(apron_px=GRASSLAND_APRON_PX).
# See module docstring for the derivation.
# ---------------------------------------------------------------------------
GRASSLAND_APRON_PX: int = 160

# ---------------------------------------------------------------------------
# Affine-remap constants (replace per-window zscore / norm01).
# Chosen from empirical statistics across seeds 0, 7, 42, 71 on 96x96 / 90 km grids.
# Target: same STRUCTURE and similar tone to the original zscore/norm01 output.
#
# norm01(x) -> affine_remap(x, center=mean_min, scale=1/mean_ptp)
# zscore(x) -> affine_remap(x, center=mean, scale=1/std)
# ---------------------------------------------------------------------------

# macro fbm (norm01): mean=-0.042, min=-0.500, max=0.381 -> ptp~0.881
GRASSLAND_MACRO_CENTER: float = -0.50
GRASSLAND_MACRO_SCALE: float = 1.14

# secondary fbm (norm01): mean=-0.056, min=-0.686, max=0.712 -> ptp~1.398
GRASSLAND_SECONDARY_CENTER: float = -0.69
GRASSLAND_SECONDARY_SCALE: float = 0.72

# swells = norm01(gaussian_filter(0.74*macro_n + 0.26*secondary_n)):
# pre-blur combo mean=0.499, blurred mean=0.499, blurred min=0.132, blurred max=0.861 -> ptp~0.729
GRASSLAND_SWELLS_CENTER: float = 0.13
GRASSLAND_SWELLS_SCALE: float = 1.37

# swells zscore (center on swells output [0,1]): swells output mean~0.507, std~0.223
GRASSLAND_SWELLS_ZSCORE_CENTER: float = 0.507
GRASSLAND_SWELLS_ZSCORE_SCALE: float = 4.49

# base_for_flow zscore: mean~0.503, std~0.196
GRASSLAND_BASE_FLOW_CENTER: float = 0.503
GRASSLAND_BASE_FLOW_SCALE: float = 5.11

# fine_grain fbm (zscore): mean~-0.001, std~0.288
GRASSLAND_FINE_GRAIN_CENTER: float = 0.00
GRASSLAND_FINE_GRAIN_SCALE: float = 3.47

# low_ripple ridged multifractal (zscore): mean~0.353, std~0.234
GRASSLAND_LOW_RIPPLE_CENTER: float = 0.353
GRASSLAND_LOW_RIPPLE_SCALE: float = 4.27

# sandhill envelope fbm (norm01): min~-0.381, ptp~0.989
GRASSLAND_SH_ENVELOPE_CENTER: float = -0.38
GRASSLAND_SH_ENVELOPE_SCALE: float = 1.01

# sandhill broken fbm (norm01): min~-0.871, ptp~1.731
GRASSLAND_SH_BROKEN_CENTER: float = -0.87
GRASSLAND_SH_BROKEN_SCALE: float = 0.58

# sandhill final (norm01 of blurred composite): output is [0,1]-ish; empirical min~0.0, ptp~1.0
# Use a gentle affine that maps 0->0, 1->1 (identity -- the combination is self-normalizing).
GRASSLAND_SH_FINAL_CENTER: float = 0.00
GRASSLAND_SH_FINAL_SCALE: float = 1.00

# escarpment plateau fbm (norm01): min~-0.509, ptp~1.110
GRASSLAND_ESC_PLATEAU_CENTER: float = -0.51
GRASSLAND_ESC_PLATEAU_SCALE: float = 0.90

# escarpment final (norm01 of blurred edge*plateau): min~0.0, ptp~1.0 -- identity
GRASSLAND_ESC_FINAL_CENTER: float = 0.00
GRASSLAND_ESC_FINAL_SCALE: float = 1.00

# final blend: affine replaces trailing zscore; tuned to keep amplitude near legacy std~1.
# Upstream signal by construction hovers around std~1; 0.82 scale gives similar amplitude.
GRASSLAND_FINAL_CENTER: float = 0.00
GRASSLAND_FINAL_SCALE: float = 0.82


@dataclass(frozen=True)
class GrasslandStyle:
    key: str
    label: str
    angle_rad: float
    swell_gain: float = 1.0
    draw_gain: float = 1.0
    sandhill_gain: float = 1.0
    pan_gain: float = 1.0
    escarpment_gain: float = 1.0
    texture_gain: float = 1.0
    smoothing_px: float = 3.0
    seed_offset: int = 0


STYLES = (
    GrasslandStyle(
        "rolling_prairie",
        "rolling prairie",
        angle_rad=0.34,
        swell_gain=1.18,
        draw_gain=0.72,
        sandhill_gain=0.00,
        pan_gain=0.18,
        escarpment_gain=0.18,
        texture_gain=0.42,
        smoothing_px=3.7,
        seed_offset=0,
    ),
    GrasslandStyle(
        "sandhill_steppe",
        "sandhill steppe",
        angle_rad=-0.46,
        swell_gain=0.86,
        draw_gain=0.42,
        sandhill_gain=1.22,
        pan_gain=0.20,
        escarpment_gain=0.08,
        texture_gain=0.34,
        smoothing_px=4.4,
        seed_offset=1000,
    ),
    GrasslandStyle(
        "dry_steppe_basin",
        "dry steppe basin",
        angle_rad=0.92,
        swell_gain=0.72,
        draw_gain=0.48,
        sandhill_gain=0.00,
        pan_gain=1.18,
        escarpment_gain=0.18,
        texture_gain=0.24,
        smoothing_px=5.0,
        seed_offset=2000,
    ),
    GrasslandStyle(
        "chapada_savanna",
        "chapada savanna",
        angle_rad=-0.12,
        swell_gain=0.92,
        draw_gain=0.82,
        sandhill_gain=0.00,
        pan_gain=0.24,
        escarpment_gain=1.18,
        texture_gain=0.46,
        smoothing_px=3.5,
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


def _sandhill_field(
    wx: np.ndarray,
    wz: np.ndarray,
    feature_span_m: float,
    style: GrasslandStyle,
    seed: int,
    *,
    seam_safe_mode: bool = False,
    blur_mode: str = "reflect",
) -> np.ndarray:
    rot_cx = 0.0 if seam_safe_mode else None
    rot_cz = 0.0 if seam_safe_mode else None
    rx, rz = _rotated(wx, wz, style.angle_rad, cx=rot_cx, cz=rot_cz)
    spacing = feature_span_m * 0.030
    warp = wg.fbm(wx, wz, 1.0 / (feature_span_m * 0.30), 4, seed + 120, gain=0.52) * spacing * 1.20
    cross = wg.fbm(wx + rz * 0.18, wz + rx * 0.08, 1.0 / (feature_span_m * 0.12), 3, seed + 126, gain=0.50)
    phase = (rx + warp + cross * spacing * 0.42) / max(spacing, 1.0) * np.pi * 2.0
    secondary = (rx * 0.74 + rz * 0.18 + warp * 0.30) / max(spacing * 1.65, 1.0) * np.pi * 2.0
    ridges = 0.74 * (1.0 - np.abs(np.sin(phase))) + 0.26 * (1.0 - np.abs(np.sin(secondary)))
    softened = np.power(np.clip(ridges, 0.0, 1.0), 1.55)
    if seam_safe_mode:
        envelope_raw = wg.fbm(wx, wz, 1.0 / (feature_span_m * 0.76), 4, seed + 130)
        envelope = smoothstep(0.48, 0.80, np.clip(ss.affine_remap(envelope_raw, GRASSLAND_SH_ENVELOPE_CENTER, GRASSLAND_SH_ENVELOPE_SCALE), 0.0, 1.0))
        broken_raw = wg.fbm(wx, wz, 1.0 / (feature_span_m * 0.055), 3, seed + 136, gain=0.46)
        broken = 0.55 + 0.45 * np.clip(ss.affine_remap(broken_raw, GRASSLAND_SH_BROKEN_CENTER, GRASSLAND_SH_BROKEN_SCALE), 0.0, 1.0)
        blurred = gaussian_filter(softened * envelope * broken, sigma=1.55, mode=blur_mode)
        return np.clip(ss.affine_remap(blurred, GRASSLAND_SH_FINAL_CENTER, GRASSLAND_SH_FINAL_SCALE), 0.0, 1.0)
    else:
        envelope = smoothstep(0.48, 0.80, norm01(wg.fbm(wx, wz, 1.0 / (feature_span_m * 0.76), 4, seed + 130)))
        broken = 0.55 + 0.45 * norm01(wg.fbm(wx, wz, 1.0 / (feature_span_m * 0.055), 3, seed + 136, gain=0.46))
        return norm01(gaussian_filter(softened * envelope * broken, sigma=1.55))


def _escarpment_field(
    wx: np.ndarray,
    wz: np.ndarray,
    feature_span_m: float,
    style: GrasslandStyle,
    seed: int,
    *,
    seam_safe_mode: bool = False,
    blur_mode: str = "reflect",
) -> np.ndarray:
    rot_cx = 0.0 if seam_safe_mode else None
    rot_cz = 0.0 if seam_safe_mode else None
    rx, rz = _rotated(wx, wz, style.angle_rad + 0.58, cx=rot_cx, cz=rot_cz)
    bands = wg.fault_block_field(rx, rz, cell_size=feature_span_m * 0.54, width=feature_span_m * 0.040, seed=seed + 210)
    if seam_safe_mode:
        plateau_raw = wg.fbm(wx, wz, 1.0 / (feature_span_m * 0.64), 4, seed + 230)
        plateau = smoothstep(0.44, 0.78, np.clip(ss.affine_remap(plateau_raw, GRASSLAND_ESC_PLATEAU_CENTER, GRASSLAND_ESC_PLATEAU_SCALE), 0.0, 1.0))
        edge = smoothstep(0.18, 0.62, np.abs(bands)) * plateau
        blurred = gaussian_filter(edge, sigma=1.4, mode=blur_mode)
        return np.clip(ss.affine_remap(blurred, GRASSLAND_ESC_FINAL_CENTER, GRASSLAND_ESC_FINAL_SCALE), 0.0, 1.0)
    else:
        plateau = smoothstep(0.44, 0.78, norm01(wg.fbm(wx, wz, 1.0 / (feature_span_m * 0.64), 4, seed + 230)))
        edge = smoothstep(0.18, 0.62, np.abs(bands)) * plateau
        return norm01(gaussian_filter(edge, sigma=1.4))


def _draw_channels_seam_safe(
    surface: np.ndarray,
    *,
    mode: str = "nearest",
    power: float = 0.50,
) -> np.ndarray:
    """Seam-safe CONNECTED drainage: real MFD flow accumulation + FIXED-max normalization.

    Mirrors mountain_synthesis._flow_channels_seam_safe exactly, using grassland's
    flow power (0.50) and spread sigma (2.1).  See mountain_synthesis docstring for
    the probe measurements that confirm convergence at apron 160.

    Reach: pre-blur sigma=1.15 -> reach 5 px; MFD convergence (not blur-reach) sets
    the apron requirement; spread blur sigma=2.1 -> reach 9 px.
    """
    pre = gaussian_filter(np.asarray(surface, dtype=np.float64), sigma=1.15, mode=mode)
    acc = skel._flow_accumulation_mfd(pre, power=float(power))
    # FIXED-max normalization (SEAM-SAFE: no per-window statistics).
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    # Spread channel extent with nearest-mode blur (grassland draw spread sigma=2.1).
    return np.clip(
        gaussian_filter(discharge, sigma=2.1, mode=mode),
        0.0,
        1.0,
    )


def generate(
    wx: np.ndarray,
    wz: np.ndarray,
    seed: int = 0,
    style: GrasslandStyle = STYLES[0],
    feature_span_m: float | None = None,
    apron_px: int = 0,
) -> dict[str, np.ndarray | GrasslandStyle]:
    """Generate one grassland setup candidate with diagnostic masks.

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
        Use ``GRASSLAND_APRON_PX`` for the correct value.
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
        warp_freq=1.0 / (feature_span * 0.78),
        seed=sseed + 10,
        steps=3,
        decay=0.55,
        freq_mul=1.70,
    )

    if seam_safe_mode:
        macro = np.clip(ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.92), 5, sseed + 30, gain=0.58),
            GRASSLAND_MACRO_CENTER, GRASSLAND_MACRO_SCALE,
        ), 0.0, 1.0)
        secondary = np.clip(ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.34), 4, sseed + 50, gain=0.55),
            GRASSLAND_SECONDARY_CENTER, GRASSLAND_SECONDARY_SCALE,
        ), 0.0, 1.0)
        # Swells: blur the combo, then affine_remap in place of norm01
        pre_swells = gaussian_filter(0.74 * macro + 0.26 * secondary, sigma=style.smoothing_px, mode=blur_mode)
        swells = np.clip(ss.affine_remap(pre_swells, GRASSLAND_SWELLS_CENTER, GRASSLAND_SWELLS_SCALE), 0.0, 1.0)

        pans = smoothstep(0.54, 0.88, gaussian_filter(1.0 - swells, sigma=5.2, mode=blur_mode))
        sandhills = _sandhill_field(w_x, w_z, feature_span, style, sseed, seam_safe_mode=True, blur_mode=blur_mode)
        escarpments = _escarpment_field(w_x, w_z, feature_span, style, sseed, seam_safe_mode=True, blur_mode=blur_mode)

        base_for_flow = ss.affine_remap(
            0.82 * swells + 0.28 * escarpments - 0.34 * pans,
            GRASSLAND_BASE_FLOW_CENTER, GRASSLAND_BASE_FLOW_SCALE,
        )
        draws = _draw_channels_seam_safe(base_for_flow, mode=blur_mode, power=0.50)
        draws = smoothstep(0.60, 0.94, draws)
        draws *= 0.42 + 0.58 * (1.0 - pans)

        # fine_grain and low_ripple: seam-safe rotation (fixed world origin)
        rx, rz = _rotated(w_x, w_z, style.angle_rad + 1.10, cx=0.0, cz=0.0)
        fine_grain = ss.affine_remap(
            wg.fbm(rx, rz, 1.0 / (feature_span * 0.032), 4, sseed + 310, gain=0.46),
            GRASSLAND_FINE_GRAIN_CENTER, GRASSLAND_FINE_GRAIN_SCALE,
        )
        low_ripple = ss.affine_remap(
            wg.ridged_multifractal(rx, rz * 0.34, 1.0 / (feature_span * 0.075), 3, sseed + 330, gain=0.44),
            GRASSLAND_LOW_RIPPLE_CENTER, GRASSLAND_LOW_RIPPLE_SCALE,
        )

        height = ss.affine_remap(swells, GRASSLAND_SWELLS_ZSCORE_CENTER, GRASSLAND_SWELLS_ZSCORE_SCALE) * (0.52 * style.swell_gain)
        height += 0.16 * style.sandhill_gain * sandhills
        height += 0.34 * style.escarpment_gain * escarpments
        height -= 0.28 * style.pan_gain * pans
        height -= 0.24 * style.draw_gain * draws
        height += style.texture_gain * (0.050 * fine_grain + 0.050 * low_ripple * (0.35 + 0.65 * sandhills))

        smooth = gaussian_filter(height, sigma=max(style.smoothing_px, 0.5), mode=blur_mode)
        open_floor = np.clip(0.62 * pans + 0.26 * (1.0 - escarpments), 0.0, 1.0)
        height = height * (1.0 - 0.28 * open_floor) + smooth * (0.28 * open_floor)
        final_blend = 0.86 * height + 0.14 * gaussian_filter(height, sigma=1.1, mode=blur_mode)
        height = ss.affine_remap(final_blend, GRASSLAND_FINAL_CENTER, GRASSLAND_FINAL_SCALE)
    else:
        macro = norm01(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.92), 5, sseed + 30, gain=0.58))
        secondary = norm01(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.34), 4, sseed + 50, gain=0.55))
        swells = norm01(gaussian_filter(0.74 * macro + 0.26 * secondary, sigma=style.smoothing_px))

        pans = smoothstep(0.54, 0.88, gaussian_filter(1.0 - swells, sigma=5.2))
        sandhills = _sandhill_field(w_x, w_z, feature_span, style, sseed)
        escarpments = _escarpment_field(w_x, w_z, feature_span, style, sseed)

        base_for_flow = zscore(0.82 * swells + 0.28 * escarpments - 0.34 * pans)
        draws = wg.flow_accumulation_channels(base_for_flow, power=0.50)
        draws = smoothstep(0.60, 0.94, gaussian_filter(draws, sigma=2.1))
        draws *= 0.42 + 0.58 * (1.0 - pans)

        rx, rz = _rotated(w_x, w_z, style.angle_rad + 1.10)
        fine_grain = zscore(wg.fbm(rx, rz, 1.0 / (feature_span * 0.032), 4, sseed + 310, gain=0.46))
        low_ripple = zscore(wg.ridged_multifractal(rx, rz * 0.34, 1.0 / (feature_span * 0.075), 3, sseed + 330, gain=0.44))

        height = zscore(swells) * (0.52 * style.swell_gain)
        height += 0.16 * style.sandhill_gain * sandhills
        height += 0.34 * style.escarpment_gain * escarpments
        height -= 0.28 * style.pan_gain * pans
        height -= 0.24 * style.draw_gain * draws
        height += style.texture_gain * (0.050 * fine_grain + 0.050 * low_ripple * (0.35 + 0.65 * sandhills))

        smooth = gaussian_filter(height, sigma=max(style.smoothing_px, 0.5))
        open_floor = np.clip(0.62 * pans + 0.26 * (1.0 - escarpments), 0.0, 1.0)
        height = height * (1.0 - 0.28 * open_floor) + smooth * (0.28 * open_floor)
        height = zscore(0.86 * height + 0.14 * gaussian_filter(height, sigma=1.1))

    # --- crop to core ---
    if seam_safe_mode and a > 0:
        height        = np.ascontiguousarray(height[a:-a, a:-a])
        swells        = np.ascontiguousarray(swells[a:-a, a:-a])
        draws         = np.ascontiguousarray(draws[a:-a, a:-a])
        sandhills     = np.ascontiguousarray(sandhills[a:-a, a:-a])
        pans          = np.ascontiguousarray(pans[a:-a, a:-a])
        escarpments   = np.ascontiguousarray(escarpments[a:-a, a:-a])

    return {
        "height": height,
        "swells": swells,
        "draws": np.clip(draws * style.draw_gain, 0.0, 1.0),
        "sandhills": np.clip(sandhills * style.sandhill_gain, 0.0, 1.0),
        "pans": np.clip(pans * style.pan_gain, 0.0, 1.0),
        "escarpments": np.clip(escarpments * style.escarpment_gain, 0.0, 1.0),
        "style": style,
    }
