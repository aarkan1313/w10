"""Volcanic-only reference-kernel synthesis experiments.

Volcanic terrain needs explicit vents, cones, craters/calderas, rift chains,
ash/lava plains, and lobate flows. This is a setup-level biome pass, not a
generic family mapper.

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
4. Vent positions are derived from ``feature_span_m`` (a caller-supplied FIXED
   constant) and a FIXED world origin (0, 0) rather than per-window ``np.min/max``
   extents, so adjacent windows share identical vent locations.
5. Gully carving uses REAL multiple-flow-direction (MFD) accumulation
   (``geography_skeleton._flow_accumulation_mfd``) with a FIXED-max normalisation
   (``log1p(acc) / log1p(acc.size)`` -- data-independent, NOT per-window max).
   Global flow accumulation is not bit-exact on a finite apron, but it CONVERGES
   to bit-exact as the apron grows: see mountain_synthesis docstring for the
   probe measurements that motivated apron 160.
6. A single crop ``height[a:-a, a:-a]`` is performed at the very end.

Required apron (computed as sum of chained kernel reaches along the deepest path):
  - flows gaussian_filter sigma=1.1 -> reach 5 px
  - caldera_bowl: gaussian_filter(shields+cones, sigma=2.6) -> reach 11 px
  - ash_plain: gaussian_filter(max(cones,flows), sigma=3.0) -> reach 12 px
  - smoothed_plain: gaussian_filter(height, sigma=2.6) -> reach 11 px (runs on height)
  - gullies: MFD convergence dominates; pre-blur sigma=1.15 -> reach 5 px;
    spread blur sigma=1.2 -> reach 5 px
  - final blend: gaussian_filter(height, sigma=0.85) -> reach 4 px
  - total blur-reach budget: ~77 px (dominated by MFD convergence error)

The blur-reach budget alone is ~77, BUT the dominant residual is the REAL
flow-accumulation convergence error, which is SCALE-DEPENDENT (same finding as
mountain). So: -> ``VOLCANIC_APRON_PX = 160`` -- matches mountain's calibrated
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
# must supply when calling generate(apron_px=VOLCANIC_APRON_PX).
# See module docstring for the derivation.
# ---------------------------------------------------------------------------
VOLCANIC_APRON_PX: int = 160

# ---------------------------------------------------------------------------
# Affine-remap constants (replace per-window zscore / norm01).
# Derived from empirical statistics across seeds 0, 7, 42, 51, 52 on 96x96
# / 90 km grids, all four styles combined.
# Target: same STRUCTURE and similar tone to the original zscore/norm01 output.
#
# norm01(x) -> affine_remap(x, center=mean_min, scale=1/mean_ptp)
# zscore(x) -> affine_remap(x, center=mean, scale=1/std)
# ---------------------------------------------------------------------------

# regional fbm (norm01): mean_min=-0.492, ptp=0.996
VOLCANIC_REGIONAL_CENTER: float = -0.492
VOLCANIC_REGIONAL_SCALE: float = 1.004

# rift ridged_multifractal raw (smoothstep input, no normalisation needed):
# no affine constant -- smoothstep thresholds handle it directly

# cones_raw (norm01): mean_min=0.003, mean_ptp=1.405
VOLCANIC_CONES_CENTER: float = 0.003
VOLCANIC_CONES_SCALE: float = 0.712

# craters_raw (norm01): mean_min=0.000, mean_ptp=1.114
VOLCANIC_CRATERS_CENTER: float = 0.000
VOLCANIC_CRATERS_SCALE: float = 0.898

# shields_raw (norm01): mean_min=0.010, mean_ptp=2.302
VOLCANIC_SHIELDS_CENTER: float = 0.010
VOLCANIC_SHIELDS_SCALE: float = 0.434

# flows_blurred (norm01 of gaussian_filter(flows_raw, sigma=1.1)): mean_min=0.003, ptp=0.686
VOLCANIC_FLOWS_CENTER: float = 0.003
VOLCANIC_FLOWS_SCALE: float = 1.459

# vents_raw (norm01): mean_min=0.000, ptp=0.992 (maximum field, so range is [0, ~1])
VOLCANIC_VENTS_CENTER: float = 0.000
VOLCANIC_VENTS_SCALE: float = 1.008

# base_inner = 0.58*regional + 0.52*shields*shield_gain + 0.22*rift (zscore):
# mean=0.459, std=0.189 -> scale=1/0.189=5.30
VOLCANIC_BASE_CENTER: float = 0.459
VOLCANIC_BASE_SCALE: float = 5.30

# lava_texture fbm (zscore): mean=-0.002, std=0.276 -> scale=1/0.276=3.63
VOLCANIC_LAVA_TEXTURE_CENTER: float = -0.002
VOLCANIC_LAVA_TEXTURE_SCALE: float = 3.63

# rough_aa ridged_multifractal (zscore): mean=0.335, std=0.224 -> scale=1/0.224=4.47
VOLCANIC_ROUGH_AA_CENTER: float = 0.335
VOLCANIC_ROUGH_AA_SCALE: float = 4.47

# final blend (replaces trailing zscore): mean=0.376, std=1.211
# Tuned to 0.82 so post-floor-blend amplitude lands near legacy std~1.
VOLCANIC_FINAL_CENTER: float = 0.376
VOLCANIC_FINAL_SCALE: float = 0.82


@dataclass(frozen=True)
class VolcanicStyle:
    key: str
    label: str
    angle_rad: float
    vent_count: int
    cone_gain: float = 1.0
    shield_gain: float = 1.0
    caldera_gain: float = 1.0
    flow_gain: float = 1.0
    rift_gain: float = 1.0
    gully_gain: float = 1.0
    cone_width_m: float = 7600.0
    crater_width_m: float = 1900.0
    flow_length_m: float = 31000.0
    detail_gain: float = 1.0
    seed_offset: int = 0


STYLES = (
    VolcanicStyle(
        "stratovolcano_cluster",
        "stratovolcano cluster",
        angle_rad=0.35,
        vent_count=4,
        cone_gain=1.28,
        shield_gain=0.62,
        caldera_gain=0.72,
        flow_gain=0.78,
        rift_gain=0.34,
        gully_gain=1.12,
        cone_width_m=6700.0,
        crater_width_m=1500.0,
        flow_length_m=27000.0,
        detail_gain=0.58,
        seed_offset=0,
    ),
    VolcanicStyle(
        "shield_lava_field",
        "shield lava field",
        angle_rad=-0.15,
        vent_count=3,
        cone_gain=0.55,
        shield_gain=1.42,
        caldera_gain=0.58,
        flow_gain=1.32,
        rift_gain=0.46,
        gully_gain=0.46,
        cone_width_m=12500.0,
        crater_width_m=3300.0,
        flow_length_m=46000.0,
        detail_gain=0.36,
        seed_offset=1000,
    ),
    VolcanicStyle(
        "caldera_complex",
        "caldera complex",
        angle_rad=0.82,
        vent_count=5,
        cone_gain=0.76,
        shield_gain=0.86,
        caldera_gain=1.50,
        flow_gain=0.66,
        rift_gain=0.38,
        gully_gain=0.72,
        cone_width_m=8600.0,
        crater_width_m=6200.0,
        flow_length_m=25000.0,
        detail_gain=0.44,
        seed_offset=2000,
    ),
    VolcanicStyle(
        "rift_cone_chain",
        "rift cone chain",
        angle_rad=-0.72,
        vent_count=7,
        cone_gain=0.88,
        shield_gain=0.76,
        caldera_gain=0.44,
        flow_gain=1.02,
        rift_gain=1.34,
        gully_gain=0.62,
        cone_width_m=5600.0,
        crater_width_m=1300.0,
        flow_length_m=33000.0,
        detail_gain=0.50,
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


def _angle_delta(a: np.ndarray, b: float) -> np.ndarray:
    return np.arctan2(np.sin(a - float(b)), np.cos(a - float(b)))


def _vent_points(
    wx: np.ndarray,
    wz: np.ndarray,
    style: VolcanicStyle,
    seed: int,
    *,
    feature_span_m: float | None = None,
    seam_safe_mode: bool = False,
) -> list[tuple[float, float, float]]:
    """Return vent positions as (x, z, amplitude) tuples.

    When ``seam_safe_mode=True``, vent positions are derived from
    ``feature_span_m`` and a FIXED world origin (0, 0) so that all adjacent
    windows compute the SAME vent coordinates -- making the cone/crater/shield
    distance fields seam-safe.  The legacy path derives extents from
    ``np.min/max(wx)`` (data-dependent, NOT seam-safe).
    """
    rng = np.random.default_rng(int(seed) + int(style.seed_offset) + 500)
    if seam_safe_mode and feature_span_m is not None:
        # Fixed world-origin centre; span from caller-supplied constant.
        span = float(feature_span_m)
        cx = 0.0
        cz = 0.0
        min_x = cx - span * 0.5
        min_z = cz - span * 0.5
    else:
        min_x = float(np.min(wx))
        max_x = float(np.max(wx))
        min_z = float(np.min(wz))
        max_z = float(np.max(wz))
        cx = (min_x + max_x) * 0.5
        cz = (min_z + max_z) * 0.5
        span = max(max_x - min_x, max_z - min_z, 1.0)
    vents: list[tuple[float, float, float]] = []
    if style.key == "rift_cone_chain":
        c = np.cos(style.angle_rad)
        s = np.sin(style.angle_rad)
        for i in range(style.vent_count):
            t = (float(i) / max(float(style.vent_count - 1), 1.0) - 0.5) * span * 0.74
            lateral = rng.normal(0.0, span * 0.045)
            x = cx + c * t - s * lateral
            z = cz + s * t + c * lateral
            amp = 0.72 + 0.46 * rng.random()
            vents.append((x, z, amp))
    elif style.key == "caldera_complex":
        vents.append((cx + rng.normal(0.0, span * 0.035), cz + rng.normal(0.0, span * 0.035), 1.20))
        for i in range(style.vent_count - 1):
            a = 2.0 * np.pi * float(i) / max(float(style.vent_count - 1), 1.0) + rng.normal(0.0, 0.24)
            r = span * (0.17 + 0.06 * rng.random())
            vents.append((cx + np.cos(a) * r, cz + np.sin(a) * r, 0.58 + 0.34 * rng.random()))
    else:
        vents.append((cx + rng.normal(0.0, span * 0.08), cz + rng.normal(0.0, span * 0.08), 1.08))
        for _ in range(style.vent_count - 1):
            x = min_x + span * (0.18 + 0.64 * rng.random())
            z = min_z + span * (0.18 + 0.64 * rng.random())
            amp = 0.48 + 0.52 * rng.random()
            vents.append((x, z, amp))
    return vents


def _vent_fields(
    wx: np.ndarray,
    wz: np.ndarray,
    feature_span_m: float,
    style: VolcanicStyle,
    seed: int,
    *,
    seam_safe_mode: bool = False,
    blur_mode: str = "reflect",
) -> dict[str, np.ndarray]:
    cones = np.zeros_like(wx, dtype=np.float64)
    craters = np.zeros_like(wx, dtype=np.float64)
    shields = np.zeros_like(wx, dtype=np.float64)
    flows = np.zeros_like(wx, dtype=np.float64)
    vents = np.zeros_like(wx, dtype=np.float64)
    flow_dirs_rng = np.random.default_rng(int(seed) + int(style.seed_offset) + 900)
    vent_list = _vent_points(
        wx, wz, style, seed,
        feature_span_m=feature_span_m,
        seam_safe_mode=seam_safe_mode,
    )
    for vx, vz, amp in vent_list:
        dx = wx - float(vx)
        dz = wz - float(vz)
        r = np.sqrt(dx * dx + dz * dz)
        cone = np.exp(-r / max(style.cone_width_m, 1.0))
        shield = np.exp(-((r / max(style.cone_width_m * 2.65, 1.0)) ** 2))
        crater = np.exp(-((r / max(style.crater_width_m, 1.0)) ** 2))
        rim = np.exp(-(((r - style.crater_width_m * 1.55) / max(style.crater_width_m * 0.34, 1.0)) ** 2))
        cones += amp * cone
        shields += amp * shield
        craters += amp * crater
        cones += 0.18 * amp * rim
        vents = np.maximum(vents, crater)

        angle = np.arctan2(dz, dx)
        local_flows = np.zeros_like(wx, dtype=np.float64)
        for _ in range(4):
            direction = flow_dirs_rng.uniform(-np.pi, np.pi)
            angular = np.exp(-((_angle_delta(angle, direction) / 0.25) ** 2))
            downstream = smoothstep(style.crater_width_m * 1.8, style.cone_width_m * 1.4, r)
            lobe = angular * np.exp(-r / max(style.flow_length_m, 1.0)) * downstream
            local_flows = np.maximum(local_flows, lobe)
        flows = np.maximum(flows, amp * local_flows)

    if seam_safe_mode:
        cones_out = np.clip(ss.affine_remap(cones, VOLCANIC_CONES_CENTER, VOLCANIC_CONES_SCALE), 0.0, 1.0)
        craters_out = np.clip(ss.affine_remap(craters, VOLCANIC_CRATERS_CENTER, VOLCANIC_CRATERS_SCALE), 0.0, 1.0)
        shields_out = np.clip(ss.affine_remap(shields, VOLCANIC_SHIELDS_CENTER, VOLCANIC_SHIELDS_SCALE), 0.0, 1.0)
        flows_blurred = gaussian_filter(flows, sigma=1.1, mode=blur_mode)
        flows_out = np.clip(ss.affine_remap(flows_blurred, VOLCANIC_FLOWS_CENTER, VOLCANIC_FLOWS_SCALE), 0.0, 1.0)
        vents_out = np.clip(ss.affine_remap(vents, VOLCANIC_VENTS_CENTER, VOLCANIC_VENTS_SCALE), 0.0, 1.0)
    else:
        cones_out = norm01(cones)
        craters_out = norm01(craters)
        shields_out = norm01(shields)
        flows_out = norm01(gaussian_filter(flows, sigma=1.1))
        vents_out = norm01(vents)

    return {
        "cones": cones_out,
        "craters": craters_out,
        "shields": shields_out,
        "flows": flows_out,
        "vents": vents_out,
    }


def _gully_channels_seam_safe(
    surface: np.ndarray,
    *,
    mode: str = "nearest",
    power: float = 0.40,
) -> np.ndarray:
    """Seam-safe gully carving: real MFD flow accumulation + FIXED-max normalization.

    Mirrors mountain_synthesis._flow_channels_seam_safe exactly, using volcanic's
    flow power (0.40) and spread sigma (1.2).  See mountain_synthesis docstring for
    the probe measurements that confirm convergence at apron 160.

    Reach: pre-blur sigma=1.15 -> reach 5 px; MFD convergence (not blur-reach) sets
    the apron requirement; spread blur sigma=1.2 -> reach 5 px.
    """
    pre = gaussian_filter(np.asarray(surface, dtype=np.float64), sigma=1.15, mode=mode)
    acc = skel._flow_accumulation_mfd(pre, power=float(power))
    # FIXED-max normalization (SEAM-SAFE: no per-window statistics).
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    # Spread gully extent (seam-safe nearest-mode blur).
    return np.clip(
        gaussian_filter(discharge, sigma=1.2, mode=mode),
        0.0,
        1.0,
    )


def generate(
    wx: np.ndarray,
    wz: np.ndarray,
    seed: int = 0,
    style: VolcanicStyle = STYLES[0],
    feature_span_m: float | None = None,
    apron_px: int = 0,
) -> dict[str, np.ndarray | VolcanicStyle]:
    """Generate one volcanic setup candidate with diagnostic masks.

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
        Use ``VOLCANIC_APRON_PX`` for the correct value.
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
        warp_amount=feature_span * 0.026,
        warp_freq=1.0 / (feature_span * 0.72),
        seed=sseed + 10,
        steps=3,
        decay=0.52,
        freq_mul=1.82,
    )

    if seam_safe_mode:
        regional = np.clip(
            ss.affine_remap(
                wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.84), 5, sseed + 30, gain=0.56),
                VOLCANIC_REGIONAL_CENTER, VOLCANIC_REGIONAL_SCALE,
            ),
            0.0, 1.0,
        )
        # Rotation around world origin (0, 0) -- seam-safe (fixed, not window midpoint)
        rx, rz = _rotated(w_x, w_z, style.angle_rad, cx=0.0, cz=0.0)
        rift_raw = wg.ridged_multifractal(rx, rz * 0.22, 1.0 / (feature_span * 0.16), 4, sseed + 80, gain=0.52)
        rift = np.clip(
            smoothstep(0.40, 0.88, rift_raw) * style.rift_gain,
            0.0, 1.0,
        )
        fields = _vent_fields(w_x, w_z, feature_span, style, sseed, seam_safe_mode=True, blur_mode=blur_mode)
    else:
        regional = norm01(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.84), 5, sseed + 30, gain=0.56))
        # Legacy: rotation around per-window midpoint (data-dependent)
        rx, rz = _rotated(w_x, w_z, style.angle_rad)
        rift_raw = smoothstep(
            0.40,
            0.88,
            wg.ridged_multifractal(rx, rz * 0.22, 1.0 / (feature_span * 0.16), 4, sseed + 80, gain=0.52),
        )
        rift = np.clip(rift_raw * style.rift_gain, 0.0, 1.0)
        fields = _vent_fields(w_x, w_z, feature_span, style, sseed, seam_safe_mode=False, blur_mode=blur_mode)

    cones = np.asarray(fields["cones"], dtype=np.float64)
    craters = np.asarray(fields["craters"], dtype=np.float64)
    shields = np.asarray(fields["shields"], dtype=np.float64)
    flows = np.asarray(fields["flows"], dtype=np.float64)
    vents = np.asarray(fields["vents"], dtype=np.float64)

    if seam_safe_mode:
        lava_texture = ss.affine_remap(
            wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.020), 5, sseed + 210, gain=0.48),
            VOLCANIC_LAVA_TEXTURE_CENTER, VOLCANIC_LAVA_TEXTURE_SCALE,
        )
        rough_aa = ss.affine_remap(
            wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.027), 4, sseed + 240, gain=0.48),
            VOLCANIC_ROUGH_AA_CENTER, VOLCANIC_ROUGH_AA_SCALE,
        )
        base = ss.affine_remap(
            0.58 * regional + 0.52 * shields * style.shield_gain + 0.22 * rift,
            VOLCANIC_BASE_CENTER, VOLCANIC_BASE_SCALE,
        )
    else:
        lava_texture = zscore(wg.fbm(w_x, w_z, 1.0 / (feature_span * 0.020), 5, sseed + 210, gain=0.48))
        rough_aa = zscore(wg.ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.027), 4, sseed + 240, gain=0.48))
        base = zscore(0.58 * regional + 0.52 * shields * style.shield_gain + 0.22 * rift)

    radial_surface = base + 1.12 * cones - 0.78 * craters

    if seam_safe_mode:
        gullies_discharge = _gully_channels_seam_safe(radial_surface, mode=blur_mode, power=0.40)
        gullies = smoothstep(0.52, 0.92, gullies_discharge) * (0.30 + 0.70 * cones)
    else:
        gullies = wg.flow_accumulation_channels(radial_surface, power=0.40)
        gullies = smoothstep(0.52, 0.92, gaussian_filter(gullies, sigma=1.2)) * (0.30 + 0.70 * cones)

    caldera_bowl = craters * smoothstep(0.52, 0.88, gaussian_filter(shields + cones, sigma=2.6, mode=blur_mode))
    caldera_rim = smoothstep(0.38, 0.78, cones) * (1.0 - smoothstep(0.25, 0.72, craters))
    cone_lift = cones * (1.0 - 0.88 * smoothstep(0.12, 0.78, craters))
    height = base.copy()
    height += style.cone_gain * (1.08 * cone_lift + 0.20 * cone_lift * rough_aa)
    height += style.shield_gain * 0.54 * shields
    height += 0.22 * rift
    height += style.flow_gain * (0.42 * flows + 0.13 * flows * lava_texture)
    height += style.caldera_gain * 0.22 * caldera_rim
    height -= style.caldera_gain * 1.48 * caldera_bowl
    height -= style.gully_gain * 0.30 * gullies
    height += style.detail_gain * (0.10 + 0.18 * flows + 0.20 * cones) * lava_texture

    ash_plain = smoothstep(0.52, 0.86, 1.0 - gaussian_filter(np.maximum(cones, flows), sigma=3.0, mode=blur_mode))
    smoothed_plain = gaussian_filter(height, sigma=2.6, mode=blur_mode)
    height = height * (1.0 - 0.30 * ash_plain) + smoothed_plain * (0.30 * ash_plain)

    if seam_safe_mode:
        final_blend = 0.82 * height + 0.18 * gaussian_filter(height, sigma=0.85, mode=blur_mode)
        height = ss.affine_remap(final_blend, VOLCANIC_FINAL_CENTER, VOLCANIC_FINAL_SCALE)
    else:
        height = zscore(0.82 * height + 0.18 * gaussian_filter(height, sigma=0.85))

    # --- crop to core ---
    if seam_safe_mode and a > 0:
        height      = np.ascontiguousarray(height[a:-a, a:-a])
        cones       = np.ascontiguousarray(cones[a:-a, a:-a])
        craters     = np.ascontiguousarray(craters[a:-a, a:-a])
        shields     = np.ascontiguousarray(shields[a:-a, a:-a])
        flows       = np.ascontiguousarray(flows[a:-a, a:-a])
        rift        = np.ascontiguousarray(rift[a:-a, a:-a])
        gullies     = np.ascontiguousarray(gullies[a:-a, a:-a])
        ash_plain   = np.ascontiguousarray(ash_plain[a:-a, a:-a])
        vents       = np.ascontiguousarray(vents[a:-a, a:-a])

    return {
        "height": height,
        "cones": cones,
        "craters": craters,
        "shields": shields,
        "flows": flows,
        "rift": rift,
        "gullies": gullies,
        "ash_plain": ash_plain,
        "vents": vents,
        "style": style,
    }
