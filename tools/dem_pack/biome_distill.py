"""WorldGen10 biome distillation (Slice 2) — pure numpy, offline, render-first.

Measures STRUCTURAL metrics (NOT a power spectrum — the refuted path) on a real DEM in REAL-WORLD
units, and maps them to the warped-noise generator's knobs (worldgen_proto.generate). Structural
descriptors drive structure-GENERATING machinery (ridged noise, warp, carving). Pure functions
(arrays/dicts in, dicts out); distill_biomes.py does the file I/O. Nothing here runs at engine runtime.

Metric SOURCE (data-driven, surveyed across all 12 families): RELIEF + SLOPE come from the vetted
kernel.json metadata (height_range_m, mean_slope_deg). STRUCTURE (amp profile, ridge, incision,
anisotropy, wavelength) is COMPUTED from the z-score DEM array. WG9's ridge_density/valley_density are
a dead-constant 0.100 for every kernel — NEVER read them (they would collapse every biome to identical).
See docs/superpowers/specs/2026-05-30-worldgen-slice2-biome-distillation-design.md."""
from __future__ import annotations
import numpy as np
from scipy.ndimage import gaussian_filter, sobel

N_OCTAVES = 6

# --- transform constants (named config — no magic numbers in function bodies, pillar 1) ---
RIDGE_FREQ_RATIO = 2.0       # ridge_freq = RIDGE_FREQ_RATIO * base_freq
VALLEY_FREQ_RATIO = 1.2      # valley_freq = VALLEY_FREQ_RATIO * base_freq
WARP_FREQ_K = 2.7            # warp_freq = 1 / (WARP_FREQ_K * dominant_wavelength_m)
RIDGE_STRENGTH_MAX = 1.0     # clamp ceiling for ridge_strength
VALLEY_DEPTH_MAX = 1.0       # clamp ceiling for valley_depth
WARP_AMOUNT_FRAC = 0.35      # warp_amount = WARP_AMOUNT_FRAC * dominant_wavelength_m * flow
UPPER_MASK_PCTL = 60.0       # ridge_linearity measured on the upper-elevation mask (top 40%)
BASE_BLUR_SIGMA_PX = 1.0     # smallest octave-band blur sigma in pixels

# --- blur-sigma knobs (tuned by eye in later tasks; named so tuning is one-place) ---
INCISION_REGIONAL_SIGMA_PX = 6.0  # regional smoothing window for incision_depth baseline surface
CURVATURE_GATE_SIGMA_PX = 1.0     # light blur before Laplacian concavity gate in incision_depth
TENSOR_SMOOTH_SIGMA_PX = 2.0      # structure-tensor component smoothing for coherence estimation


def to_metres(z, height_range_m):
    """Rescale a z-score DEM so its min->max span equals the real height_range_m (metres)."""
    z = np.asarray(z, dtype=np.float64)
    span = float(z.max() - z.min())
    if span <= 0.0:
        return np.zeros_like(z)
    return z * (float(height_range_m) / span)


def bandpass_amp_profile(z, n_octaves=N_OCTAVES):
    """Difference-of-Gaussian-blur octave bands; each band's std = its AMPLITUDE (not phase).
    Returned profile is normalized so band 0 == 1.0 for any non-degenerate field; a
    flat/degenerate field returns all-zeros. Amplitude-only (the spectral lesson)."""
    z = np.asarray(z, dtype=np.float64)
    prev = z.copy()
    amps = []
    sigma = BASE_BLUR_SIGMA_PX
    for _ in range(n_octaves):
        blurred = gaussian_filter(z, sigma=sigma, mode="reflect")
        band = prev - blurred           # the detail removed at this scale = this octave's content
        amps.append(float(band.std()))
        prev = blurred
        sigma *= 2.0
    amps = np.asarray(amps, dtype=np.float64)
    a0 = amps[0] if amps[0] > 1e-12 else 1.0
    return (amps / a0).tolist()


def _structure_tensor_coherence(z):
    """Coherence of the gradient structure tensor: (l1-l2)/(l1+l2) in [0,1].
    High = one dominant gradient direction (linear/anisotropic); low = isotropic."""
    z = np.asarray(z, dtype=np.float64)
    gx = sobel(z, axis=1, mode="reflect")
    gz = sobel(z, axis=0, mode="reflect")
    # smooth the tensor components so coherence reflects regional, not per-pixel, structure
    jxx = gaussian_filter(gx * gx, TENSOR_SMOOTH_SIGMA_PX, mode="reflect")
    jzz = gaussian_filter(gz * gz, TENSOR_SMOOTH_SIGMA_PX, mode="reflect")
    jxz = gaussian_filter(gx * gz, TENSOR_SMOOTH_SIGMA_PX, mode="reflect")
    tr = jxx + jzz
    disc = np.sqrt(np.maximum((jxx - jzz) ** 2 + 4.0 * jxz * jxz, 0.0))
    l1 = 0.5 * (tr + disc)
    l2 = 0.5 * (tr - disc)
    denom = l1 + l2
    coh = np.where(denom > 1e-12, (l1 - l2) / denom, 0.0)
    return float(np.clip(coh.mean(), 0.0, 1.0))


def ridge_linearity(z):
    """How linear/ridgey the UPLANDS are (vs scattered bumps): structure-tensor coherence on the
    upper-elevation mask. [0,1]. Drives ridge_strength."""
    z = np.asarray(z, dtype=np.float64)
    thr = np.percentile(z, UPPER_MASK_PCTL)
    upper = np.where(z >= thr, z, thr)     # flatten the lowlands so coherence reflects ridge structure
    return _structure_tensor_coherence(upper)


def anisotropy_flow(z):
    """Whole-field directional coherence (flowing/meandering vs blocky). [0,1]. Drives warp_amount."""
    return _structure_tensor_coherence(z)


def incision_depth(z_m, spacing_m):
    """Drainage incision in REAL metres: how far concave/low areas sit below their local surroundings.
    local_relief = (regional mean) - z in concave spots; report the high-incision quantile."""
    z = np.asarray(z_m, dtype=np.float64)
    regional = gaussian_filter(z, sigma=INCISION_REGIONAL_SIGMA_PX, mode="reflect")
    below = np.clip(regional - z, 0.0, None)      # how far below the regional surface (valleys positive)
    # curvature gate: keep concave (valley) areas (laplacian > 0 for pits/channels)
    lap = (gaussian_filter(z, CURVATURE_GATE_SIGMA_PX, mode="reflect") - z)
    valley = below * (lap > 0)
    if not np.any(valley > 0):
        return 0.0
    return float(np.percentile(valley[valley > 0], 90))    # metres of typical deep incision


def dominant_wavelength_from_profile(profile, spacing_m):
    """Return dominant wavelength in metres given an already-computed bandpass amp profile.
    Pure helper: argmax band -> sigma_px -> period in metres. Avoids recomputing the profile."""
    prof = np.asarray(profile, dtype=np.float64)
    band = int(np.argmax(prof))
    sigma_px = BASE_BLUR_SIGMA_PX * (2 ** band)
    return float(sigma_px * float(spacing_m) * 2.0)


def dominant_wavelength_m(z, spacing_m, n_octaves=N_OCTAVES):
    """Characteristic feature size in metres: the octave band (from bandpass_amp_profile) with the
    most amplitude -> its centre wavelength = (BASE_BLUR_SIGMA_PX * 2^band) * spacing_m * 2 (period).
    Calls bandpass_amp_profile then dominant_wavelength_from_profile; use the latter directly when
    the profile is already in hand (e.g. inside metrics_for_dem) to avoid duplicate filtering."""
    prof = bandpass_amp_profile(z, n_octaves)
    return dominant_wavelength_from_profile(prof, spacing_m)


def metrics_for_dem(z, meta):
    """Measure all structural metrics for ONE DEM. RELIEF + SLOPE come from the vetted kernel.json meta
    (height_range_m, mean_slope_deg — they separate families cleanly); STRUCTURE (amp profile, ridge,
    incision, anisotropy, wavelength) is COMPUTED from the z-score array (WG9's ridge_density/valley_density
    are dead-constant 0.100 — never read them). Returns a plain dict of floats/lists. Vertical converted to
    real metres for incision; horizontal via approx_sample_spacing_m for wavelengths."""
    z = np.asarray(z, dtype=np.float64)
    height_range_m = float(meta["height_range_m"])
    spacing_m = float(meta["approx_sample_spacing_m"])
    z_m = to_metres(z, height_range_m)
    # compute profile once; reuse for both amp_profile and dominant_wavelength (avoids 6 extra blurs)
    profile = bandpass_amp_profile(z, N_OCTAVES)
    return {
        "relief_real_m": height_range_m,                    # META (vetted)
        "slope_bias_deg": float(meta["mean_slope_deg"]),    # META (vetted)
        "amp_profile": profile,                             # COMPUTED
        "ridge_linearity": ridge_linearity(z),              # COMPUTED (meta ridge_density is dead)
        "incision_depth_m": incision_depth(z_m, spacing_m), # COMPUTED (meta valley_density is dead)
        "anisotropy": anisotropy_flow(z),                   # COMPUTED (meta anisotropy_score too weak)
        "dominant_wavelength_m": dominant_wavelength_from_profile(profile, spacing_m),  # COMPUTED
    }
