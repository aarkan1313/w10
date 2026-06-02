"""Karst-only reference-kernel synthesis experiments.

Karst needs a different shape from mountain/glacial terrain: limestone plateaus,
rounded residual towers, cockpit/doline depressions, dry valleys, and lineament
control. This is intentionally a narrow biome pass, not a generic family mapper.

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
4. Dry-valley carving uses REAL multiple-flow-direction (MFD) accumulation
   (``geography_skeleton._flow_accumulation_mfd``) with a FIXED-max normalisation
   (``log1p(acc) / log1p(acc.size)`` -- data-independent, NOT per-window max).
   Global flow accumulation is not bit-exact on a finite apron, but it CONVERGES
   to bit-exact as the apron grows: see mountain_synthesis docstring for the
   probe measurements that motivated apron 160.
5. A single crop ``height[a:-a, a:-a]`` is performed at the very end.

Required apron (computed as sum of chained kernel reaches along the deepest path):
  - regional blur sigma=5.8 -> reach 23 px (plateau from regional)
  - cellular blur sigma=3.8 -> reach 15 px (parallel, not chained deeper)
  - tower blur sigma<=3.2 -> reach 13 px (parallel)
  - doline blur sigma<=4.0 -> reach 16 px (parallel)
  - flow pre-blur sigma=1.15 -> reach 5 px (MFD convergence dominates over blur-reach)
  - dry_valleys spread blur sigma=2.6 -> reach 11 px; MFD dominates; chain = 23+11 = 34
  - floor smooth blur max sigma=5.0 -> reach 20 px; input depth=34: chain = 34+20 = 54
  - final blend sigma=0.95 -> reach 4 px; total = 54+4 = 58 (blur-reach budget)

The blur-reach budget alone is ~58, BUT the dominant residual is the REAL
flow-accumulation convergence error, which is SCALE-DEPENDENT (same finding as
mountain). So: -> ``KARST_APRON_PX = 160`` -- matches mountain's calibrated
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
# must supply when calling generate(apron_px=KARST_APRON_PX).
# See module docstring for the derivation.
# ---------------------------------------------------------------------------
KARST_APRON_PX: int = 160

# ---------------------------------------------------------------------------
# Affine-remap constants (replace per-window zscore / norm01).
# Chosen from empirical statistics across seeds 0, 7, 42, 71 on 96x96 / 90 km grids,
# all four styles combined.
# Target: same STRUCTURE and similar tone to the original zscore/norm01 output.
#
# norm01(x) -> affine_remap(x, center=mean_min, scale=1/mean_ptp)
# zscore(x) -> affine_remap(x, center=mean, scale=1/std)
# ---------------------------------------------------------------------------

# regional fbm raw (norm01): min=-0.673, ptp=1.472
KARST_REGIONAL_CENTER: float = -0.673
KARST_REGIONAL_SCALE: float = 0.679

# tower cone+local combo (norm01 inside _tower_field): min=0.0005, ptp=0.906
KARST_TOWER_CONE_CENTER: float = 0.0005
KARST_TOWER_CONE_SCALE: float = 1.104

# tower blurred-sparse output (norm01 at end of _tower_field): min=0.0, ptp=0.696
KARST_TOWER_FINAL_CENTER: float = 0.00
KARST_TOWER_FINAL_SCALE: float = 1.437

# doline pits combo (norm01 inside _doline_field): min=0.0003, ptp=0.924
KARST_DOLINE_PITS_CENTER: float = 0.0003
KARST_DOLINE_PITS_SCALE: float = 1.082

# doline blurred-bowls output (norm01 at end of _doline_field): min=0.0, ptp=0.234
KARST_DOLINE_BOWLS_CENTER: float = 0.00
KARST_DOLINE_BOWLS_SCALE: float = 4.274

# lineament combo (norm01 on 0.68*lineA + 0.32*lineB): min=0.001, ptp=0.916
KARST_LINEAMENT_CENTER: float = 0.001
KARST_LINEAMENT_SCALE: float = 1.092

# cockpit_noise fbm raw (norm01): min=-0.880, ptp=1.769
KARST_COCKPIT_NOISE_CENTER: float = -0.880
KARST_COCKPIT_NOISE_SCALE: float = 0.565

# cockpit combo (norm01 on 0.50*dolines+0.26*(1-cellular)+0.24*cockpit_noise):
# min=0.072, ptp=0.735
KARST_COCKPIT_CENTER: float = 0.072
KARST_COCKPIT_SCALE: float = 1.360

# base raw (zscore on plateau_gain*(1.06*plateau+0.18*regional)):
# mean=0.560, std=0.479
KARST_BASE_CENTER: float = 0.560
KARST_BASE_SCALE: float = 2.090

# fine fbm raw (zscore): mean=-0.0003, std=0.283
KARST_FINE_CENTER: float = 0.00
KARST_FINE_SCALE: float = 3.539

# karren ridged raw (zscore): mean=0.356, std=0.235
KARST_KARREN_CENTER: float = 0.356
KARST_KARREN_SCALE: float = 4.257

# final height before trailing zscore: mean=0.0805, std=1.038
# affine replaces trailing zscore; scale tuned to keep amplitude near legacy std~1.
KARST_FINAL_CENTER: float = 0.08
KARST_FINAL_SCALE: float = 0.964


@dataclass(frozen=True)
class KarstStyle:
    key: str
    label: str
    angle_rad: float
    plateau_gain: float = 1.0
    tower_gain: float = 1.0
    cockpit_gain: float = 1.0
    doline_gain: float = 1.0
    valley_gain: float = 1.0
    lineament_gain: float = 1.0
    tower_width_px: float = 2.4
    doline_width_px: float = 2.8
    floor_smooth_px: float = 3.2
    detail_gain: float = 1.0
    anisotropy: float = 0.55
    seed_offset: int = 0


STYLES = (
    KarstStyle(
        "tower_karst",
        "tower karst",
        angle_rad=0.42,
        plateau_gain=0.86,
        tower_gain=1.45,
        cockpit_gain=1.02,
        doline_gain=0.82,
        valley_gain=0.62,
        lineament_gain=0.74,
        tower_width_px=2.0,
        doline_width_px=2.6,
        floor_smooth_px=2.8,
        detail_gain=0.54,
        anisotropy=0.48,
        seed_offset=0,
    ),
    KarstStyle(
        "cockpit_hills",
        "cockpit hills",
        angle_rad=-0.30,
        plateau_gain=1.02,
        tower_gain=0.92,
        cockpit_gain=1.36,
        doline_gain=1.18,
        valley_gain=0.80,
        lineament_gain=0.58,
        tower_width_px=2.8,
        doline_width_px=3.4,
        floor_smooth_px=3.6,
        detail_gain=0.46,
        anisotropy=0.42,
        seed_offset=1000,
    ),
    KarstStyle(
        "linear_valley_karst",
        "linear valley karst",
        angle_rad=0.92,
        plateau_gain=1.12,
        tower_gain=0.72,
        cockpit_gain=0.78,
        doline_gain=0.82,
        valley_gain=1.34,
        lineament_gain=1.16,
        tower_width_px=3.2,
        doline_width_px=3.0,
        floor_smooth_px=4.4,
        detail_gain=0.40,
        anisotropy=0.24,
        seed_offset=2000,
    ),
    KarstStyle(
        "mogote_plain",
        "mogote plain",
        angle_rad=-0.74,
        plateau_gain=0.78,
        tower_gain=1.22,
        cockpit_gain=0.66,
        doline_gain=0.72,
        valley_gain=0.46,
        lineament_gain=0.50,
        tower_width_px=2.2,
        doline_width_px=4.0,
        floor_smooth_px=5.0,
        detail_gain=0.32,
        anisotropy=0.60,
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


def _lineaments(
    wx: np.ndarray,
    wz: np.ndarray,
    feature_span_m: float,
    style: KarstStyle,
    seed: int,
    *,
    seam_safe_mode: bool = False,
    blur_mode: str = "reflect",
) -> np.ndarray:
    rot_cx = 0.0 if seam_safe_mode else None
    rot_cz = 0.0 if seam_safe_mode else None
    rx, rz = _rotated(wx, wz, style.angle_rad, cx=rot_cx, cz=rot_cz)
    line_a = wg.ridged_multifractal(
        rx,
        rz * style.anisotropy,
        1.0 / (feature_span_m * 0.18),
        4,
        seed + 100,
        gain=0.54,
    )
    line_b = wg.ridged_multifractal(
        rx * 0.58 - rz * 0.32,
        rz * 0.58 + rx * 0.32,
        1.0 / (feature_span_m * 0.11),
        3,
        seed + 130,
        gain=0.48,
    )
    combo = 0.68 * line_a + 0.32 * line_b
    if seam_safe_mode:
        return smoothstep(0.46, 0.82, np.clip(ss.affine_remap(combo, KARST_LINEAMENT_CENTER, KARST_LINEAMENT_SCALE), 0.0, 1.0))
    return smoothstep(0.46, 0.82, norm01(combo))


def _tower_field(
    wx: np.ndarray,
    wz: np.ndarray,
    feature_span_m: float,
    style: KarstStyle,
    seed: int,
    *,
    seam_safe_mode: bool = False,
    blur_mode: str = "reflect",
) -> np.ndarray:
    cone = wg.ridged_multifractal(
        wx,
        wz,
        1.0 / (feature_span_m * 0.055),
        5,
        seed + 210,
        gain=0.52,
        weight_gain=1.62,
    )
    local = wg.ridged_multifractal(wx, wz, 1.0 / (feature_span_m * 0.026), 3, seed + 240, gain=0.45)
    combo = 0.78 * cone + 0.22 * local
    if seam_safe_mode:
        sparse = smoothstep(0.46, 0.84, np.clip(ss.affine_remap(combo, KARST_TOWER_CONE_CENTER, KARST_TOWER_CONE_SCALE), 0.0, 1.0))
        towers = gaussian_filter(np.power(sparse, 1.20), sigma=max(style.tower_width_px, 0.2), mode=blur_mode)
        return np.clip(ss.affine_remap(towers, KARST_TOWER_FINAL_CENTER, KARST_TOWER_FINAL_SCALE), 0.0, 1.0)
    sparse = smoothstep(0.46, 0.84, norm01(combo))
    towers = gaussian_filter(np.power(sparse, 1.20), sigma=max(style.tower_width_px, 0.2))
    return norm01(towers)


def _doline_field(
    wx: np.ndarray,
    wz: np.ndarray,
    feature_span_m: float,
    style: KarstStyle,
    seed: int,
    *,
    seam_safe_mode: bool = False,
    blur_mode: str = "reflect",
) -> np.ndarray:
    pits_a = wg.ridged_multifractal(wx, wz, 1.0 / (feature_span_m * 0.040), 4, seed + 310, gain=0.50)
    pits_b = wg.ridged_multifractal(wx + 0.31 * wz, wz - 0.17 * wx, 1.0 / (feature_span_m * 0.022), 3, seed + 330, gain=0.46)
    combo = 0.66 * pits_a + 0.34 * pits_b
    if seam_safe_mode:
        pits = smoothstep(0.55, 0.90, np.clip(ss.affine_remap(combo, KARST_DOLINE_PITS_CENTER, KARST_DOLINE_PITS_SCALE), 0.0, 1.0))
        bowls = gaussian_filter(np.power(pits, 1.45), sigma=max(style.doline_width_px, 0.2), mode=blur_mode)
        return np.clip(ss.affine_remap(bowls, KARST_DOLINE_BOWLS_CENTER, KARST_DOLINE_BOWLS_SCALE), 0.0, 1.0)
    pits = smoothstep(0.55, 0.90, norm01(combo))
    bowls = gaussian_filter(np.power(pits, 1.45), sigma=max(style.doline_width_px, 0.2))
    return norm01(bowls)


def _dry_valleys_seam_safe(
    surface: np.ndarray,
    *,
    mode: str = "nearest",
    power: float = 0.54,
) -> np.ndarray:
    """Seam-safe CONNECTED drainage: real MFD flow accumulation + FIXED-max normalization.

    Mirrors mountain_synthesis._flow_channels_seam_safe exactly, using karst's
    flow power (0.54) and spread sigma (2.6).  See mountain_synthesis docstring for
    the probe measurements that confirm convergence at apron 160.

    Reach: pre-blur sigma=1.15 -> reach 5 px; MFD convergence (not blur-reach) sets
    the apron requirement; spread blur sigma=2.6 -> reach 11 px.
    """
    pre = gaussian_filter(np.asarray(surface, dtype=np.float64), sigma=1.15, mode=mode)
    acc = skel._flow_accumulation_mfd(pre, power=float(power))
    # FIXED-max normalization (SEAM-SAFE: no per-window statistics).
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    # Spread channel extent with nearest-mode blur (karst spread sigma=2.6).
    return np.clip(
        gaussian_filter(discharge, sigma=2.6, mode=mode),
        0.0,
        1.0,
    )


def generate(
    wx: np.ndarray,
    wz: np.ndarray,
    seed: int = 0,
    style: KarstStyle = STYLES[0],
    feature_span_m: float | None = None,
    apron_px: int = 0,
) -> dict[str, np.ndarray | KarstStyle]:
    """Generate one karst-only candidate with diagnostic masks.

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
        Use ``KARST_APRON_PX`` for the correct value.
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
        warp_amount=feature_span * 0.035,
        warp_freq=1.0 / (feature_span * 0.62),
        seed=sseed + 10,
        steps=3,
        decay=0.55,
        freq_mul=1.82,
    )

    if seam_safe_mode:
        regional = np.clip(ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.74), 5, sseed + 30, gain=0.56),
            KARST_REGIONAL_CENTER, KARST_REGIONAL_SCALE,
        ), 0.0, 1.0)
        plateau = smoothstep(0.30, 0.72, gaussian_filter(regional, sigma=5.8, mode=blur_mode))
        towers = _tower_field(w_x, w_z, feature_span, style, sseed, seam_safe_mode=True, blur_mode=blur_mode)
        dolines = _doline_field(w_x, w_z, feature_span, style, sseed, seam_safe_mode=True, blur_mode=blur_mode)
        lineaments = _lineaments(w_x, w_z, feature_span, style, sseed, seam_safe_mode=True, blur_mode=blur_mode)
        cellular = gaussian_filter(
            wg.cellular_edges(w_x, w_z, 1.0 / (feature_span * 0.145), sseed + 160, sharpness=1.45),
            sigma=3.8,
            mode=blur_mode,
        )
        cockpit_noise = np.clip(ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.052), 4, sseed + 180, gain=0.54),
            KARST_COCKPIT_NOISE_CENTER, KARST_COCKPIT_NOISE_SCALE,
        ), 0.0, 1.0)
        cockpit = smoothstep(0.52, 0.90, np.clip(ss.affine_remap(
            0.50 * dolines + 0.26 * (1.0 - cellular) + 0.24 * cockpit_noise,
            KARST_COCKPIT_CENTER, KARST_COCKPIT_SCALE,
        ), 0.0, 1.0))

        base = ss.affine_remap(
            style.plateau_gain * (1.06 * plateau + 0.18 * regional),
            KARST_BASE_CENTER, KARST_BASE_SCALE,
        )
        dry_valleys = _dry_valleys_seam_safe(
            base - 0.30 * lineaments - 0.10 * dolines,
            mode=blur_mode,
            power=0.54,
        )
        dry_valleys = smoothstep(0.58, 0.92, dry_valleys)
        dry_valleys = np.clip(dry_valleys * (0.72 + 0.28 * style.valley_gain), 0.0, 1.0)

        tower_mask = smoothstep(0.22, 0.74, towers) * (0.50 + 0.50 * plateau)
        cockpit_mask = smoothstep(0.46, 0.86, cockpit) * (0.35 + 0.65 * plateau)
        doline_mask = smoothstep(0.46, 0.88, dolines) * (0.30 + 0.70 * plateau)
        lineament_mask = np.clip(style.lineament_gain * lineaments * (0.35 + 0.65 * plateau), 0.0, 1.0)
        tower_mask = tower_mask * (1.0 - 0.50 * doline_mask) * (1.0 - 0.30 * dry_valleys)

        fine = ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.018), 4, sseed + 410, gain=0.48),
            KARST_FINE_CENTER, KARST_FINE_SCALE,
        )
        karren = ss.affine_remap(
            wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.016), 3, sseed + 430, gain=0.46),
            KARST_KARREN_CENTER, KARST_KARREN_SCALE,
        )
    else:
        regional = norm01(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.74), 5, sseed + 30, gain=0.56))
        plateau = smoothstep(0.30, 0.72, gaussian_filter(regional, sigma=5.8))
        towers = _tower_field(w_x, w_z, feature_span, style, sseed)
        dolines = _doline_field(w_x, w_z, feature_span, style, sseed)
        lineaments = _lineaments(w_x, w_z, feature_span, style, sseed)
        cellular = gaussian_filter(
            wg.cellular_edges(w_x, w_z, 1.0 / (feature_span * 0.145), sseed + 160, sharpness=1.45),
            sigma=3.8,
        )
        cockpit_noise = norm01(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.052), 4, sseed + 180, gain=0.54))
        cockpit = smoothstep(0.52, 0.90, norm01(0.50 * dolines + 0.26 * (1.0 - cellular) + 0.24 * cockpit_noise))

        base = zscore(style.plateau_gain * (1.06 * plateau + 0.18 * regional))
        dry_valleys = wg.flow_accumulation_channels(base - 0.30 * lineaments - 0.10 * dolines, power=0.54)
        dry_valleys = smoothstep(0.58, 0.92, gaussian_filter(dry_valleys, sigma=2.6))
        dry_valleys = np.clip(dry_valleys * (0.72 + 0.28 * style.valley_gain), 0.0, 1.0)

        tower_mask = smoothstep(0.22, 0.74, towers) * (0.50 + 0.50 * plateau)
        cockpit_mask = smoothstep(0.46, 0.86, cockpit) * (0.35 + 0.65 * plateau)
        doline_mask = smoothstep(0.46, 0.88, dolines) * (0.30 + 0.70 * plateau)
        lineament_mask = np.clip(style.lineament_gain * lineaments * (0.35 + 0.65 * plateau), 0.0, 1.0)
        tower_mask = tower_mask * (1.0 - 0.50 * doline_mask) * (1.0 - 0.30 * dry_valleys)

        fine = zscore(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.018), 4, sseed + 410, gain=0.48))
        karren = zscore(wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.016), 3, sseed + 430, gain=0.46))

    height = base.copy()
    height += style.tower_gain * (0.84 * tower_mask + 0.20 * tower_mask * karren)
    height += style.lineament_gain * 0.20 * lineament_mask
    height -= style.cockpit_gain * 0.26 * cockpit_mask
    height -= style.doline_gain * 0.72 * doline_mask
    height -= style.valley_gain * 0.40 * dry_valleys
    height += style.detail_gain * (0.08 + 0.24 * tower_mask + 0.10 * lineament_mask) * fine

    floor_mask = np.clip(0.72 * doline_mask + 0.56 * cockpit_mask + 0.48 * dry_valleys, 0.0, 1.0)
    smoothed_floor = gaussian_filter(height, sigma=max(style.floor_smooth_px, 0.2), mode=blur_mode)
    height = height * (1.0 - 0.34 * floor_mask) + smoothed_floor * (0.34 * floor_mask)

    if seam_safe_mode:
        final_blend = 0.80 * height + 0.20 * gaussian_filter(height, sigma=0.95, mode=blur_mode)
        height = ss.affine_remap(final_blend, KARST_FINAL_CENTER, KARST_FINAL_SCALE)
    else:
        height = zscore(0.80 * height + 0.20 * gaussian_filter(height, sigma=0.95))

    # --- crop to core ---
    if seam_safe_mode and a > 0:
        height        = np.ascontiguousarray(height[a:-a, a:-a])
        plateau       = np.ascontiguousarray(plateau[a:-a, a:-a])
        lineament_mask = np.ascontiguousarray(lineament_mask[a:-a, a:-a])
        tower_mask    = np.ascontiguousarray(tower_mask[a:-a, a:-a])
        cockpit_mask  = np.ascontiguousarray(cockpit_mask[a:-a, a:-a])
        doline_mask   = np.ascontiguousarray(doline_mask[a:-a, a:-a])
        dry_valleys   = np.ascontiguousarray(dry_valleys[a:-a, a:-a])

    return {
        "height": height,
        "plateau": plateau,
        "lineaments": lineament_mask,
        "towers": tower_mask,
        "cockpit": cockpit_mask,
        "dolines": doline_mask,
        "dry_valleys": dry_valleys,
        "style": style,
    }
