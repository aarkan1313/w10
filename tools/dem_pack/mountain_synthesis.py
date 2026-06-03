"""Mountain-only reference-kernel synthesis experiments.

This is intentionally not a generic biome mapper. The failed all-family pass
showed that pushing every family through one skeleton recipe produces blocky
generic terrain. This file is the next narrow pass: make mountain kernels read
as mountain-like procedural terrain before moving to another family.

Seam-safety (apron_px > 0 path)
--------------------------------
When ``apron_px > 0``, ``generate`` expects ``wx``/``wz`` grids that are already
padded by ``apron_px`` cells of real world-coordinates on every side.  It
computes on the full padded array, then crops to the core before returning.

Rules that guarantee seam-exactness:
1. All ``gaussian_filter`` calls use ``mode='nearest'``.
2. Data-dependent normalisation (``zscore``, ``norm01``) is replaced by
   ``seam_safe.affine_remap`` with fixed constants (never per-window statistics).
3. Channel carving uses REAL multiple-flow-direction (MFD) accumulation
   (``geography_skeleton._flow_accumulation_mfd``) with a FIXED-max normalisation
   (``log1p(acc) / log1p(acc.size)`` — data-independent, NOT per-window max).
   Global flow accumulation is not bit-exact on a finite apron, but it CONVERGES
   to bit-exact as the apron grows: the probe (``probe_flow_seam_real.py``) measured
   final-height border delta 1.7e-10 at apron 80, 5.6e-17 at 128, 0.0 at 200.
   1.7e-10 is far below float32 epsilon (~1e-7), so on the GPU (float32) it is
   bit-identical at apron 80.  This replaces the earlier local DoG proxy, which was
   literally seam-exact (delta 0.0) but produced disconnected/soft valleys; the owner
   judged that too soft at scale — real flow accumulation gives connected drainage.
4. A single crop ``height[a:-a, a:-a]`` is performed at the very end.

Required apron (computed as sum of chained kernel reaches along the deepest path):
  - ``_lowland_mask``: blur σ=7.0 → reach 28 px
  - flow pre-blur σ=1.15 → reach 5; channel width blur σ≤4.0 → reach 16; chain = 28+5+16 = 49
  - floor blur σ≤5.6 → reach 23; input depth max(28, 49) = 49; chain = 49+23 = 72
  - final blend blur σ=1.2 → reach 5; total = 72+5 = 77 (blur-reach budget)

The blur-reach budget alone is ~77, BUT the dominant residual is the REAL flow-accumulation
convergence error, which is SCALE-DEPENDENT: a border cell's drainage depends on upstream area
that grows with world size, so no fixed apron is ever bit-exact for an arbitrarily large world.
Measured (export_godot_mountain_seamsafe_chunks): worst adjacent-chunk seam delta over a 7x7 /
175 km world was ~1e-4 normalized at apron 160 (≈0.17 m at base_height_scale 1700 — invisible);
at apron 80 the same many-window world hit ~1.3e-2 (a VISIBLE ~22 m step). An earlier single-seam
probe at apron 80 read 1.7e-10, which was misleadingly optimistic — it did not cross sensitive
drainage. So:
  → ``MOUNTAIN_APRON_PX = 160`` — holds the VISUALLY-SEAMLESS bar (seam << relief) across a
    many-window world. Seam-safety here means "no visible/trippable seam", NOT bit-exact: global
    flow-accumulation connected drainage cannot be bit-exact across arbitrary windows (a fundamental
    limit, owner-accepted: "doesn't have to be exact if it's seamless and looks good"). Apron 160 is
    the perf/quality balance (vs ~80 for a small world, or ever-larger for an ever-bigger one).
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from scipy.ndimage import gaussian_filter

import geography_engine as geo
import geography_skeleton as skel
import worldgen_proto as wg
import seam_safe as ss

# ---------------------------------------------------------------------------
# Apron constant — how many cells of world-coord padding each side the caller
# must supply when calling generate(apron_px=MOUNTAIN_APRON_PX).
# See module docstring for the derivation.
# ---------------------------------------------------------------------------
MOUNTAIN_APRON_PX: int = 160

# ---------------------------------------------------------------------------
# Affine-remap constants (replace per-window zscore / norm01).
# Chosen from empirical statistics across seeds 0, 7, 42 on 96×96 / 90 km grids.
# Target: same STRUCTURE and similar tone to the original zscore/norm01 output.
# ---------------------------------------------------------------------------

# norm01 → affine_remap targets [0, 1] equivalent
# regional: fbm_raw typically mean≈0, range≈[-0.5 .. 0.5]
REGIONAL_CENTER: float = -0.50
REGIONAL_SCALE: float = 1.00

# ridges (sum of ridged_multifractal combos): mean≈0.43, range≈[0.10 .. 0.97]
RIDGES_CENTER: float = 0.10
RIDGES_SCALE: float = 1.15

# massif_inner: mean≈0.83, range≈[0.12 .. 1.56]
# SCALE pushed 0.70→0.72 during LOOK tuning to lift massif contrast toward legacy.
MASSIF_CENTER: float = 0.12
MASSIF_SCALE: float = 0.72

# norm01 on flow channels output: range≈[0.32 .. 1.0]
# (only used in non-apron path; apron path uses DoG which is already [0,1]-ish)
CHANNELS_CENTER: float = 0.32
CHANNELS_SCALE: float = 1.47

# zscore → affine_remap targets mean≈0, std≈1 equivalent
# base_inner: mean≈0.83, std≈0.45
# SCALE pushed 2.22→2.28 during LOOK tuning to lift base relief toward legacy.
BASE_CENTER: float = 0.83
BASE_SCALE: float = 2.28

# zscore(ranges) used in rough_surface: mean≈0.42, std≈0.14
RANGES_ZSCORE_CENTER: float = 0.42
RANGES_ZSCORE_SCALE: float = 7.00

# ridge_detail raw: mean≈0.31, std≈0.21
# SCALE pushed 4.76→4.85 during LOOK tuning to sharpen ridge crests toward legacy.
RIDGE_DETAIL_CENTER: float = 0.31
RIDGE_DETAIL_SCALE: float = 4.85

# near_detail (fbm) raw: mean≈0.0, std≈0.28
NEAR_DETAIL_CENTER: float = 0.00
NEAR_DETAIL_SCALE: float = 3.60

# final blend: upstream is already ≈mean=0, std≈1 by construction.
# Legacy ends with a per-window zscore (forces std=1.0); the seam-safe path
# cannot (data-dependent), so FINAL_SCALE is the overall-amplitude knob. Tuned
# to 0.80 so the post-incision std lands near legacy's ~1.0 (the carve/ridge
# gains below add relief that this scale then normalizes back down).
FINAL_CENTER: float = 0.00
FINAL_SCALE: float = 0.80

# ---------------------------------------------------------------------------
# LOOK levers (seam-safe path only) — recover the accepted legacy look.
# All are DATA-INDEPENDENT fixed constants → seams stay bit-exact.
# ---------------------------------------------------------------------------
# Channel mask thresholds (seam-safe path), calibrated for the real flow-accumulation
# discharge field (log1p(acc)/log1p(acc.size)), which is concentrated around
# mean≈0.21, p90≈0.31, max≈0.47 — much tighter than the old DoG proxy.  These
# thresholds carve the top ~30% discharge (connected main valleys + tributaries),
# matching legacy relief (ptp/std/vdepth within ~10%).
PRIMARY_THRESH_LO: float = 0.26
PRIMARY_THRESH_HI: float = 0.40
TRIBUTARY_THRESH_LO: float = 0.24
TRIBUTARY_THRESH_HI: float = 0.40
# Extra incision gain applied to the carve/branch terms in the seam-safe path
# (multiplies the per-style carve_gain/branch_gain). >1 deepens valleys.
# FINAL_SCALE then normalizes overall amplitude back near legacy std.
SEAMSAFE_CARVE_GAIN: float = 2.00
SEAMSAFE_BRANCH_GAIN: float = 1.70
# Extra ridge/detail gain in the seam-safe path (multiplies per-style ridge_gain
# /detail_gain). >1 sharpens ridge crests. Kept modest (1.12/1.05) so ridges read
# crisp without over-roughening relative to legacy.
SEAMSAFE_RIDGE_GAIN: float = 1.12
SEAMSAFE_DETAIL_GAIN: float = 1.05

# ---------------------------------------------------------------------------
# Scale-invariant blur sigmas.
# Reference spacing (metres/pixel) for world-anchored blur sigmas. A blur whose
# sigma is `sc` CELLS at this spacing covers `sc * S_REF` METRES; at any other
# spacing the cell-sigma is rescaled so the blur covers the SAME world distance
# -> macro structure identical across clipmap levels.
# MUST equal the Rust S_REF (recipes.rs, added in a later task). 32.0 = the live
# scene's L0 spacing.
# ---------------------------------------------------------------------------
S_REF: float = 32.0


def _sigma_cells(sigma_cell_ref: float, spacing_m: float) -> float:
    """Reference CELL sigma -> cell sigma at `spacing_m` (fixed WORLD extent).
    sigma_world_m = sigma_cell_ref * S_REF;  returns sigma_world_m / spacing_m."""
    return (sigma_cell_ref * S_REF) / max(spacing_m, 1e-6)


@dataclass(frozen=True)
class MountainStyle:
    key: str
    label: str
    angle_rad: float
    uplift_gain: float = 1.0
    ridge_gain: float = 1.0
    carve_gain: float = 1.0
    branch_gain: float = 1.0
    valley_width_px: float = 2.0
    floor_smooth_px: float = 3.0
    detail_gain: float = 1.0
    anisotropy: float = 0.34


STYLES = (
    MountainStyle(
        "alpine_branching",
        "alpine branching",
        angle_rad=0.42,
        uplift_gain=1.12,
        ridge_gain=1.18,
        carve_gain=1.08,
        branch_gain=1.18,
        valley_width_px=2.4,
        floor_smooth_px=4.0,
        detail_gain=0.72,
        anisotropy=0.72,
    ),
    MountainStyle(
        "sierra_block",
        "sierra block",
        angle_rad=1.05,
        uplift_gain=1.04,
        ridge_gain=0.92,
        carve_gain=0.84,
        branch_gain=0.86,
        valley_width_px=4.0,
        floor_smooth_px=5.6,
        detail_gain=0.52,
        anisotropy=0.52,
    ),
    MountainStyle(
        "pamir_chains",
        "pamir chains",
        angle_rad=-0.28,
        uplift_gain=1.28,
        ridge_gain=1.34,
        carve_gain=0.98,
        branch_gain=0.92,
        valley_width_px=2.8,
        floor_smooth_px=4.4,
        detail_gain=0.62,
        anisotropy=0.30,
    ),
    MountainStyle(
        "dissected_highlands",
        "dissected highlands",
        angle_rad=0.72,
        uplift_gain=0.96,
        ridge_gain=1.05,
        carve_gain=1.32,
        branch_gain=1.42,
        valley_width_px=2.0,
        floor_smooth_px=3.4,
        detail_gain=0.78,
        anisotropy=0.62,
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
    window midpoint — data-dependent, NOT seam-safe.  Pass ``cx=0.0, cz=0.0``
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


def _oriented_ridges(
    wx: np.ndarray,
    wz: np.ndarray,
    span_m: float,
    style: MountainStyle,
    seed: int,
    *,
    seam_safe_mode: bool = False,
) -> np.ndarray:
    # In seam-safe mode, rotate around the world origin so all windows share
    # the same rotation reference — not the window midpoint (data-dependent).
    rot_cx = 0.0 if seam_safe_mode else None
    rot_cz = 0.0 if seam_safe_mode else None
    rx, rz = _rotated(wx, wz, style.angle_rad, cx=rot_cx, cz=rot_cz)
    # Compress the cross-range coordinate so ridged noise forms bent range chains
    # instead of isotropic blotches.
    w_rx, w_rz = wg.recursive_domain_warp(
        rx,
        rz * style.anisotropy,
        warp_amount=span_m * 0.065,
        warp_freq=1.0 / (span_m * 0.58),
        seed=seed + 100,
        steps=3,
        decay=0.54,
        freq_mul=1.85,
    )
    long = wg.ridged_multifractal(w_rx, w_rz, 1.0 / (span_m * 0.34), 5, seed + 120, gain=0.58)
    mid = wg.ridged_multifractal(w_rx, w_rz, 1.0 / (span_m * 0.15), 4, seed + 130, gain=0.54)
    organic = wg.ridged_multifractal(w_x := w_rx + 0.28 * w_rz, w_z := w_rz - 0.18 * w_rx, 1.0 / (span_m * 0.22), 5, seed + 140, gain=0.56)
    cross = wg.ridged_multifractal(w_x, w_z, 1.0 / (span_m * 0.095), 3, seed + 150, gain=0.50)
    raw = 0.42 * long + 0.24 * mid + 0.48 * organic + 0.18 * cross
    if seam_safe_mode:
        return np.clip(ss.affine_remap(raw, RIDGES_CENTER, RIDGES_SCALE), 0.0, 1.0)
    return norm01(raw)


def _flow_channels(surface: np.ndarray, width_px: float, power: float) -> np.ndarray:
    # Legacy non-seam-safe twin of _flow_channels_seam_safe; intentionally NOT
    # scale-anchored (single-window diagnostic path only, never feeds the clipmap).
    channels = wg.flow_accumulation_channels(gaussian_filter(surface, sigma=1.15), power=power)
    channels = gaussian_filter(channels, sigma=max(float(width_px), 0.1))
    return norm01(channels)


def _flow_channels_seam_safe(
    surface: np.ndarray,
    width_px: float,
    *,
    mode: str = "nearest",
    power: float = 0.48,
    spacing_m: float = S_REF,
) -> np.ndarray:
    """Seam-safe CONNECTED drainage: real MFD flow accumulation + FIXED-max normalization.

    Computes real multiple-flow-direction accumulation
    (``geography_skeleton._flow_accumulation_mfd``) on a lightly pre-smoothed surface,
    then normalises with a DATA-INDEPENDENT fixed max — ``log1p(acc) / log1p(acc.size)``
    rather than per-window ``acc.max()`` — so adjacent windows agree at the border to
    float epsilon on an adequate apron (probe: 1.7e-10 at apron 80, below float32
    epsilon; see module docstring).  A final nearest-mode blur spreads the channel
    extent.

    Replaces the earlier local DoG proxy, which was literally seam-exact (delta 0.0)
    but produced disconnected/soft valleys (owner judged too soft at scale).  Real
    flow accumulation routes connected drainage natively.

    Reach (single chained path on ``surface``): pre-blur σ=1.15 → reach 5 px; the MFD
    pass itself is global (its convergence error, not kernel reach, sets the apron);
    width blur σ=width_px≤4.0 → reach 16 px.
    """
    pre = gaussian_filter(np.asarray(surface, dtype=np.float64), sigma=_sigma_cells(1.15, spacing_m), mode=mode)
    acc = skel._flow_accumulation_mfd(pre, power=float(power))
    # FIXED-max normalization (SEAM-SAFE: no per-window statistics).
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    # Spread channel extent (seam-safe nearest-mode blur).
    return np.clip(
        gaussian_filter(discharge, sigma=_sigma_cells(max(float(width_px), 0.1), spacing_m), mode=mode),
        0.0,
        1.0,
    )


def _lowland_mask(
    range_field: np.ndarray,
    regional: np.ndarray,
    *,
    blur_mode: str = "reflect",
    spacing_m: float = S_REF,
) -> np.ndarray:
    """Broad non-range floor mask.

    The failed mountain pass put detail everywhere. Mountain refs have quiet
    low pockets and valley floors; use a soft inverse range envelope, modulated
    by regional lows, to protect those areas from ridge/detail noise.
    """
    broad_range = gaussian_filter(range_field, sigma=_sigma_cells(7.0, spacing_m), mode=blur_mode)
    low = smoothstep(0.48, 0.84, 1.0 - broad_range)
    regional_low = smoothstep(0.44, 0.78, 1.0 - regional)
    return np.clip(low * (0.35 + 0.65 * regional_low), 0.0, 1.0)


def generate(
    wx: np.ndarray,
    wz: np.ndarray,
    seed: int = 0,
    style: MountainStyle = STYLES[0],
    feature_span_m: float | None = None,
    apron_px: int = 0,
    spacing_m: float | None = None,
    flow_on: bool = True,
) -> dict[str, np.ndarray | MountainStyle]:
    """Generate one mountain-only candidate.

    Returns normalized height plus diagnostic channel/ridge masks for review
    sheets and tests.

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
        Use ``MOUNTAIN_APRON_PX`` for the correct value.
    spacing_m:
        Metres-per-pixel of this grid.  All seam-safe gaussian blur sigmas
        (specified as reference CELL sigmas) are rescaled via ``_sigma_cells``
        so each blur covers the SAME world distance at any spacing -> macro
        structure is identical across clipmap levels.  ``None`` (default)
        resolves to ``S_REF`` (the reference spacing), giving byte-identical
        output to the pre-scale-invariant recipe at the reference level.
    flow_on:
        When ``True`` (default) the expensive flow-carved drainage is computed.
        When ``False`` the two ``_flow_channels_seam_safe`` passes are skipped
        (primary/tributary masks set to zero) -> the MACRO surface with no
        drainage carve, for coarse clipmap levels where drainage is near-field
        detail only.
    """
    a = int(apron_px)
    seam_safe_mode = a > 0
    blur_mode = "nearest" if seam_safe_mode else "reflect"
    spacing_m = float(spacing_m) if spacing_m is not None else S_REF

    if seam_safe_mode and feature_span_m is None:
        raise ValueError(
            "generate(): feature_span_m is required when apron_px > 0. "
            "Pass the CORE span in metres as a fixed constant shared by all "
            "adjacent windows (e.g. feature_span_m=90_000.0). "
            "Deriving span from np.ptp(wx) on a padded grid is data-dependent "
            "and will break seam-exactness."
        )

    span = max(float(np.ptp(wx)), float(np.ptp(wz)), 1.0)
    feature_span = max(float(feature_span_m) if feature_span_m is not None else span, 1.0)
    w_x, w_z = wg.recursive_domain_warp(
        wx,
        wz,
        warp_amount=feature_span * 0.050,
        warp_freq=1.0 / (feature_span * 0.72),
        seed=seed + 10,
        steps=3,
        decay=0.58,
        freq_mul=1.75,
    )

    if seam_safe_mode:
        regional = np.clip(ss.affine_remap(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.88), 5, seed + 20, gain=0.56), REGIONAL_CENTER, REGIONAL_SCALE), 0.0, 1.0)
        ranges = _oriented_ridges(w_x, w_z, feature_span, style, seed, seam_safe_mode=True)

        range_envelope = smoothstep(0.24, 0.58, gaussian_filter(ranges, sigma=_sigma_cells(5.0, spacing_m), mode=blur_mode))
        lowland = _lowland_mask(ranges, regional, blur_mode=blur_mode, spacing_m=spacing_m)

        massif_inner = 0.58 * regional + 0.86 * range_envelope + 0.28 * gaussian_filter(ranges, sigma=_sigma_cells(1.8, spacing_m), mode=blur_mode)
        massif = np.clip(ss.affine_remap(massif_inner, MASSIF_CENTER, MASSIF_SCALE), 0.0, 1.0)
        massif = gaussian_filter(massif, sigma=_sigma_cells(2.0, spacing_m), mode=blur_mode)

        base = ss.affine_remap(style.uplift_gain * (1.50 * massif + 0.18 * ranges - 0.46 * lowland), BASE_CENTER, BASE_SCALE)

        if flow_on:
            primary = _flow_channels_seam_safe(base, width_px=style.valley_width_px, mode=blur_mode, power=0.48, spacing_m=spacing_m)
            # Real flow accumulation (fixed-max normalized) fires at a different scale than
            # the legacy per-window-max flow channels, so use data-independent thresholds.
            primary_mask = smoothstep(PRIMARY_THRESH_LO, PRIMARY_THRESH_HI, primary)

            rough_surface = base + 0.18 * ss.affine_remap(ranges, RANGES_ZSCORE_CENTER, RANGES_ZSCORE_SCALE)
            tributary = _flow_channels_seam_safe(rough_surface, width_px=max(style.valley_width_px * 0.42, 0.6), mode=blur_mode, power=0.34, spacing_m=spacing_m)
            tributary_mask = smoothstep(TRIBUTARY_THRESH_LO, TRIBUTARY_THRESH_HI, tributary)
        else:
            # Coarse-level: skip the expensive flow-carved drainage entirely.
            # The two carve terms below (height -= ...) then vanish -> MACRO surface.
            primary_mask = np.zeros_like(base)
            tributary_mask = np.zeros_like(base)

        ridge_detail = ss.affine_remap(wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.045), 5, seed + 40, gain=0.52), RIDGE_DETAIL_CENTER, RIDGE_DETAIL_SCALE)
        near_detail = ss.affine_remap(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.020), 4, seed + 50, gain=0.48), NEAR_DETAIL_CENTER, NEAR_DETAIL_SCALE)
    else:
        # Non-seam-safe (single-window diagnostic) path: bare sigmas, NOT scale-anchored
        # by design -- never feeds the clipmap, so no spacing_m threading.
        regional = norm01(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.88), 5, seed + 20, gain=0.56))
        ranges = _oriented_ridges(w_x, w_z, feature_span, style, seed, seam_safe_mode=False)

        range_envelope = smoothstep(0.24, 0.58, gaussian_filter(ranges, sigma=5.0))
        lowland = _lowland_mask(ranges, regional)
        massif = norm01(0.58 * regional + 0.86 * range_envelope + 0.28 * gaussian_filter(ranges, sigma=1.8))
        massif = gaussian_filter(massif, sigma=2.0)

        base = zscore(style.uplift_gain * (1.50 * massif + 0.18 * ranges - 0.46 * lowland))

        if flow_on:
            primary = _flow_channels(base, width_px=style.valley_width_px, power=0.48)
            primary_mask = smoothstep(0.54, 0.94, primary)

            rough_surface = base + 0.18 * zscore(ranges)
            tributary = _flow_channels(rough_surface, width_px=max(style.valley_width_px * 0.42, 0.6), power=0.34)
            tributary_mask = smoothstep(0.44, 0.88, tributary)
        else:
            # Coarse-level: skip the expensive flow-carved drainage (parallel to the
            # seam-safe branch). The two carve terms below then vanish -> MACRO surface.
            primary_mask = np.zeros_like(base)
            tributary_mask = np.zeros_like(base)

        ridge_detail = zscore(wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.045), 5, seed + 40, gain=0.52))
        near_detail = zscore(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.020), 4, seed + 50, gain=0.48))

    high_mask = smoothstep(0.48, 0.86, massif) * (1.0 - 0.38 * lowland)
    valley_mask = np.clip(0.72 * primary_mask + 0.46 * tributary_mask, 0.0, 1.0)

    # LOOK: in the seam-safe path the DoG channel proxy + conservative affine
    # constants read too smooth; apply fixed (data-independent) gain multipliers
    # to sharpen ridges and deepen valleys back toward the accepted legacy look.
    ridge_g = style.ridge_gain * (SEAMSAFE_RIDGE_GAIN if seam_safe_mode else 1.0)
    detail_g = style.detail_gain * (SEAMSAFE_DETAIL_GAIN if seam_safe_mode else 1.0)
    carve_g = style.carve_gain * (SEAMSAFE_CARVE_GAIN if seam_safe_mode else 1.0)
    branch_g = style.branch_gain * (SEAMSAFE_BRANCH_GAIN if seam_safe_mode else 1.0)

    height = base
    height += ridge_g * (0.08 + 0.58 * high_mask) * (0.24 * ridge_detail)
    height += detail_g * (0.04 + 0.34 * high_mask) * (0.34 * near_detail)
    height -= carve_g * (0.42 + 0.58 * high_mask) * primary_mask
    height -= branch_g * (0.18 + 0.42 * high_mask) * tributary_mask

    floor_mask = np.clip(
        smoothstep(0.48, 0.86, gaussian_filter(valley_mask, sigma=_sigma_cells(1.2, spacing_m), mode=blur_mode)) + 0.24 * lowland,
        0.0,
        1.0,
    )
    floor = gaussian_filter(height, sigma=_sigma_cells(max(style.floor_smooth_px, 0.2), spacing_m), mode=blur_mode)
    height = height * (1.0 - 0.38 * floor_mask) + floor * (0.38 * floor_mask)
    height -= 0.18 * floor_mask

    if seam_safe_mode:
        final_blend = 0.74 * height + 0.26 * gaussian_filter(height, sigma=_sigma_cells(1.20, spacing_m), mode=blur_mode)
        height = ss.affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE)
    else:
        height = zscore(0.74 * height + 0.26 * gaussian_filter(height, sigma=1.20))

    # --- crop to core ---
    if seam_safe_mode and a > 0:
        height      = np.ascontiguousarray(height[a:-a, a:-a])
        ranges      = np.ascontiguousarray(ranges[a:-a, a:-a])
        range_envelope = np.ascontiguousarray(range_envelope[a:-a, a:-a])
        lowland     = np.ascontiguousarray(lowland[a:-a, a:-a])
        primary_mask = np.ascontiguousarray(primary_mask[a:-a, a:-a])
        tributary_mask = np.ascontiguousarray(tributary_mask[a:-a, a:-a])

    return {
        "height": height,
        "ranges": ranges,
        "range_envelope": range_envelope,
        "lowland": lowland,
        "primary_channels": primary_mask,
        "tributaries": tributary_mask,
        "style": style,
    }
