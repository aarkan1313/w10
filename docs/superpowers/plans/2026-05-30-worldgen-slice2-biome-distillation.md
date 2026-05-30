# Worldgen Core Slice 2 — Biome Distillation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Distill the 115 real WG9 DEMs (12 biome families) into per-family STRUCTURAL parameter-sets that drive the Slice-1 warped-noise `generate`, and render real-vs-synth side-by-side hillshades for the owner to judge per-family character match — all offline Python, no engine/Rust/GLSL touched.

**Architecture:** A pure-numpy `biome_distill.py` measures interpretable STRUCTURAL metrics (relief, octave-amplitude profile, ridge linearity, valley incision, anisotropy, dominant wavelength, slope) on each DEM in REAL-WORLD units (un-normalizing the z-score field via `height_range_m`, horizontal via `approx_sample_spacing_m`), aggregates per family by MEDIAN, and maps each metric to exactly one generator knob via documented simple transforms (all constants named config). A `render_biomes.py` renders real-vs-synth side-by-sides. A pack-writer adds a validated per-family `biome_params` table (additive; existing per-kernel entries + pixels untouched). Metrics are structural descriptors driving structure-generating machinery — NOT a power spectrum (the refuted path).

**Tech Stack:** Python 3 + numpy + scipy.ndimage (gaussian_filter, sobel — already a numpy-stack dep) + PIL + pytest. Entirely in `tools/dem_pack/`. NO Rust, NO GLSL, NO Godot.

---

## Ground truth (verified against real files — use these exact facts)

- Real DEMs: `D:/workflows/worldgen9/factory/kernels/<kernel_id>/normalized_height.npy` — **512×512 float32**, z-score normalized (std == 1.0, e.g. range −3.09..+4.25 sigma). WG9 is **READ-ONLY**.
- Meta: `D:/workflows/worldgen9/factory/kernels/<kernel_id>/kernel.json` — keys: `region, kernel_id, family_hint, sample_px (512), height_range_m (e.g. 1801.0), approx_sample_spacing_m (e.g. 90.0 m/px)`.
- Family map: `tools/dem_pack/kernel_family_map.approved.json` → `{"map": {kernel_id: family}, "excluded": [...]}`; 12 families, 115 kernels: coast 13 · badlands 12 · grassland 11 · karst 11 · glacial 10 · mountain 10 · rainforest 10 · desert 9 · volcanic 9 · temperate 7 · tundra 7 · wetland 6.
- **Real-scale conversion (exact):** vertical `metres = z * (height_range_m / (z.max() - z.min()))` (rescales the z-score span to the real elevation range); horizontal `metres = pixels * approx_sample_spacing_m`. A 512px×90m kernel ≈ 46 km footprint.
- Spike guard (reuse `build_pack.py`'s value): drop a kernel if `max(|z|) > 12.0` before measuring.
- The generator (`worldgen_proto.generate`) consumes a params dict with keys: `relief_m, octave_amps (len 6), ridge_strength, valley_depth, warp_amount, base_freq, ridge_freq, valley_freq, warp_freq`. We additionally produce `slope_bias` (stored, NOT consumed by current `generate`).
- **METRIC SOURCE (data-driven, surveyed across all 12 families — see spec §4):** `kernel.json` already has vetted metrics. USE the metadata for `height_range_m` (relief) + `mean_slope_deg` (slope_bias) — they cleanly separate families. COMPUTE from the raw DEM: `amp_profile[6]`, `dominant_wavelength_m`, `ridge_linearity`, `incision_depth`, `anisotropy`. **CRITICAL: do NOT use metadata `ridge_density`/`valley_density` — they are a CONSTANT 0.100 for every kernel (WG9's detector is degenerate); trusting them collapses every biome to identical ridge/valley.** So `metrics_for_dem` takes the META dict (for relief+slope) AND the z array (for the computed structure). The computed metrics are fixture-gated; the trusted metadata is range/finite-asserted.
- Run tests with `python -m pytest` from `tools/dem_pack/` (cwd matters — modules import by bare name; the existing suite does this). Renders write to `D:\tmp\`.

---

## File structure

- **Create:** `tools/dem_pack/biome_distill.py` — pure metric functions (`bandpass_amp_profile`, `ridge_linearity`, `incision_depth`, `anisotropy_flow`, `dominant_wavelength_m`, `to_metres`) + `metrics_for_dem(z, meta)` (one DEM + its kernel.json meta → metrics dict; relief/slope FROM meta, structure COMPUTED from z) + `params_from_metrics(metrics)` (metrics → BiomeParams dict via documented transforms) + `aggregate_median(list_of_metrics)`. Named config constants at top. Pure (arrays/dicts in, dicts out) — no file I/O. **Does NOT read `ridge_density`/`valley_density` from meta (dead-constant).**
- **Create:** `tools/dem_pack/distill_biomes.py` — the I/O orchestrator (runnable): loads the family map + WG9 kernels, applies the spike guard, calls `biome_distill`, writes a `biome_params.json` (the 12-family table) to `tools/dem_pack/` + injects it into the pack via the pack-writer (Task 5). Mirrors `build_pack.py`'s I/O role.
- **Create:** `tools/dem_pack/test_biome_distill.py` — pytest: real-scale conversion; fixture monotonicity (ridge/valley/anisotropy metrics measure what they claim); determinism/finite; `params_from_metrics` produces in-domain, bounded, finite, parity-ready params; non-repetition (reuse worldgen_proto) on a distilled-param field.
- **Create:** `tools/dem_pack/render_biomes.py` — runnable: real-vs-synth side-by-side hillshades, captioned with distilled metrics, to `D:\tmp\`. Reuses `worldgen_proto.generate` + a shared hillshade helper.
- **Modify:** `tools/dem_pack/dem_pack_lib.py` — add a pure `attach_biome_params(pack_dict, biome_params)` that validates + inserts the per-family table (additive). Keep it pure (dict in, dict out) per the file's contract.
- **Modify:** `tools/dem_pack/test_dem_pack_lib.py` — tests for `attach_biome_params` (valid insert; rejects NaN/out-of-domain naming the family).

> **N_OCTAVES = 6** (matches `worldgen_proto`). **All transform constants** (freq ratios, warp k, clamp ranges, blur sigmas) are module-level named config in `biome_distill.py` — no magic numbers in function bodies (pillar 1).

---

## Task 1: Real-scale conversion + the metric toolkit (pure)

**Files:**
- Create: `tools/dem_pack/biome_distill.py`
- Create: `tools/dem_pack/test_biome_distill.py`

- [ ] **Step 1: Write the failing test**

Create `tools/dem_pack/test_biome_distill.py`:

```python
import numpy as np
import pytest
import biome_distill as bd


def _ridged(n=128, period=16):
    # parallel linear ridges along z (1-|sin|) -> strongly linear/anisotropic, high crests
    x = np.arange(n)
    line = 1.0 - np.abs(np.sin(2 * np.pi * x / period))
    return np.tile(line.reshape(1, -1), (n, 1)).astype(np.float32)


def _flat_noise(n=128, seed=0):
    rng = np.random.default_rng(seed)
    return rng.standard_normal((n, n)).astype(np.float32) * 0.01  # near-flat, isotropic


def _carved(n=128):
    # a single deep valley trench down the middle, flat elsewhere -> high incision
    a = np.zeros((n, n), dtype=np.float32)
    a[:, n // 2 - 2:n // 2 + 2] = -1.0
    return a


def test_to_metres_rescales_span_to_height_range():
    z = np.array([[-2.0, 0.0], [1.0, 3.0]], dtype=np.float32)  # span = 5 sigma
    m = bd.to_metres(z, height_range_m=1000.0)
    assert np.isclose(float(m.max() - m.min()), 1000.0)        # span == real range
    assert np.all(np.isfinite(m))


def test_ridge_linearity_high_for_ridges_low_for_noise():
    r = bd.ridge_linearity(_ridged())
    f = bd.ridge_linearity(_flat_noise())
    assert 0.0 <= f <= 1.0 and 0.0 <= r <= 1.0
    assert r > f + 0.2                                          # ridges read as more linear


def test_incision_depth_high_for_carved_low_for_flat():
    c = bd.incision_depth(_carved(), spacing_m=90.0)
    fl = bd.incision_depth(np.zeros((128, 128), np.float32), spacing_m=90.0)
    assert c > fl                                               # carved trench has incision, flat has ~none


def test_anisotropy_high_for_directional_low_for_isotropic():
    a = bd.anisotropy_flow(_ridged())
    i = bd.anisotropy_flow(_flat_noise())
    assert 0.0 <= i <= 1.0 and 0.0 <= a <= 1.0
    assert a > i + 0.2                                          # directional terrain is more anisotropic


def test_bandpass_amp_profile_is_len6_normalized_finite():
    p = bd.bandpass_amp_profile(_ridged(), n_octaves=6)
    assert len(p) == 6
    assert np.all(np.isfinite(p))
    assert np.isclose(p[0], 1.0)                                # normalized so band 0 == 1.0
    assert np.all(np.asarray(p) >= 0.0)


_META = {"height_range_m": 1801.0, "approx_sample_spacing_m": 90.0, "mean_slope_deg": 12.5}


def test_metrics_deterministic_and_finite():
    z = _ridged()
    m1 = bd.metrics_for_dem(z, _META)
    m2 = bd.metrics_for_dem(z, _META)
    assert m1 == m2                                             # deterministic (pure)
    for k, v in m1.items():
        arr = np.asarray(v, dtype=float)
        assert np.all(np.isfinite(arr)), f"{k} not finite"


def test_metrics_use_metadata_for_relief_and_slope():
    # relief + slope come straight from the vetted metadata; structure is computed from z.
    z = _ridged()
    m = bd.metrics_for_dem(z, _META)
    assert m["relief_real_m"] == 1801.0          # from meta height_range_m, not computed
    assert m["slope_bias_deg"] == 12.5           # from meta mean_slope_deg, not computed
    # structure metrics ARE computed (present + in range)
    assert 0.0 <= m["ridge_linearity"] <= 1.0
    assert len(m["amp_profile"]) == bd.N_OCTAVES
```

- [ ] **Step 2: Run to verify it FAILS**

```powershell
cd tools/dem_pack
python -m pytest test_biome_distill.py -q
```
Expected: FAIL — `biome_distill` module doesn't exist (ImportError).

- [ ] **Step 3: Implement the metric toolkit in `biome_distill.py`**

Create `tools/dem_pack/biome_distill.py`:

```python
"""WorldGen10 biome distillation (Slice 2) — pure numpy, offline, render-first.

Measures STRUCTURAL metrics (NOT a power spectrum — the refuted path) on a real DEM in REAL-WORLD
units, and maps them to the warped-noise generator's knobs (worldgen_proto.generate). Structural
descriptors drive structure-GENERATING machinery (ridged noise, warp, carving). Pure functions
(arrays/dicts in, dicts out); distill_biomes.py does the file I/O. Nothing here runs at engine runtime.
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


def to_metres(z, height_range_m):
    """Rescale a z-score DEM so its min->max span equals the real height_range_m (metres)."""
    z = np.asarray(z, dtype=np.float64)
    span = float(z.max() - z.min())
    if span <= 0.0:
        return np.zeros_like(z)
    return z * (float(height_range_m) / span)


def bandpass_amp_profile(z, n_octaves=N_OCTAVES):
    """Difference-of-Gaussian-blur octave bands; each band's std = its AMPLITUDE (not phase).
    Returned profile is normalized so band 0 == 1.0. Amplitude-only (the spectral lesson)."""
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
    jxx = gaussian_filter(gx * gx, 2.0, mode="reflect")
    jzz = gaussian_filter(gz * gz, 2.0, mode="reflect")
    jxz = gaussian_filter(gx * gz, 2.0, mode="reflect")
    tr = jxx + jzz
    det = jxx * jzz - jxz * jxz
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
    regional = gaussian_filter(z, sigma=6.0, mode="reflect")
    below = np.clip(regional - z, 0.0, None)      # how far below the regional surface (valleys positive)
    # curvature gate: keep concave (valley) areas (laplacian > 0 for pits/channels)
    lap = (gaussian_filter(z, 1.0, mode="reflect") - z)
    valley = below * (lap > 0)
    if not np.any(valley > 0):
        return 0.0
    return float(np.percentile(valley[valley > 0], 90))    # metres of typical deep incision


def dominant_wavelength_m(z, spacing_m, n_octaves=N_OCTAVES):
    """Characteristic feature size in metres: the octave band (from bandpass_amp_profile) with the
    most amplitude -> its centre wavelength = (BASE_BLUR_SIGMA_PX * 2^band) * spacing_m * 2 (period)."""
    prof = np.asarray(bandpass_amp_profile(z, n_octaves), dtype=np.float64)
    band = int(np.argmax(prof))
    sigma_px = BASE_BLUR_SIGMA_PX * (2 ** band)
    return float(sigma_px * float(spacing_m) * 2.0)


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
    return {
        "relief_real_m": height_range_m,                    # META (vetted)
        "slope_bias_deg": float(meta["mean_slope_deg"]),    # META (vetted)
        "amp_profile": bandpass_amp_profile(z, N_OCTAVES),  # COMPUTED
        "ridge_linearity": ridge_linearity(z),              # COMPUTED (meta ridge_density is dead)
        "incision_depth_m": incision_depth(z_m, spacing_m), # COMPUTED (meta valley_density is dead)
        "anisotropy": anisotropy_flow(z),                   # COMPUTED (meta anisotropy_score too weak)
        "dominant_wavelength_m": dominant_wavelength_m(z, spacing_m),  # COMPUTED
    }
```

- [ ] **Step 4: Run to verify PASS**

```powershell
python -m pytest test_biome_distill.py -q
```
Expected: all toolkit tests pass (to_metres span, ridge>noise, incision carved>flat, anisotropy directional>isotropic, amp profile len6/normalized, deterministic+finite).

> If a monotonicity test fails (e.g. ridge_linearity not higher for ridges), that's a REAL finding about the metric — fix the metric, do NOT relax the threshold to pass. The whole point is the metric measures what it claims.

- [ ] **Step 5: Commit**

```bash
git add tools/dem_pack/biome_distill.py tools/dem_pack/test_biome_distill.py
git commit -m "worldgen s2: structural metric toolkit (real-scale, ridge/valley/anisotropy/amp/wavelength/slope) — pure, fixture-gated

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `params_from_metrics` + `aggregate_median` (metrics → generator knobs)

**Files:**
- Modify: `tools/dem_pack/biome_distill.py`
- Modify: `tools/dem_pack/test_biome_distill.py`

- [ ] **Step 1: Write the failing test**

Add to `tools/dem_pack/test_biome_distill.py`:

```python
import worldgen_proto as wg  # for the bounds + non-repetition checks


def _metrics(relief=1200.0, ridge=0.8, incis=300.0, aniso=0.7, wl=6000.0, slope=20.0):
    return {
        "relief_real_m": relief,
        "amp_profile": [1.0, 0.5, 0.25, 0.12, 0.06, 0.03],
        "ridge_linearity": ridge,
        "incision_depth_m": incis,
        "anisotropy": aniso,
        "dominant_wavelength_m": wl,
        "slope_bias_deg": slope,
    }


def test_params_from_metrics_has_all_generator_keys():
    p = bd.params_from_metrics(_metrics())
    for k in ("relief_m", "octave_amps", "ridge_strength", "valley_depth", "warp_amount",
              "base_freq", "ridge_freq", "valley_freq", "warp_freq", "slope_bias"):
        assert k in p, f"missing {k}"
    assert len(p["octave_amps"]) == bd.N_OCTAVES
    assert np.isclose(p["octave_amps"][0], 1.0)


def test_params_from_metrics_in_domain_and_finite():
    p = bd.params_from_metrics(_metrics())
    assert 0.0 <= p["ridge_strength"] <= bd.RIDGE_STRENGTH_MAX
    assert 0.0 <= p["valley_depth"] <= bd.VALLEY_DEPTH_MAX
    assert p["base_freq"] > 0 and p["ridge_freq"] > 0 and p["valley_freq"] > 0 and p["warp_freq"] > 0
    assert p["relief_m"] > 0
    for k, v in p.items():
        assert np.all(np.isfinite(np.asarray(v, dtype=float))), f"{k} not finite"
    # parity-readiness: every scalar f32-representable (round-trip stable)
    for k, v in p.items():
        if isinstance(v, (int, float)):
            assert float(np.float32(v)) == pytest.approx(v, rel=1e-5), f"{k} not f32-representable"


def test_freqs_derive_from_dominant_wavelength():
    p = bd.params_from_metrics(_metrics(wl=6000.0))
    assert np.isclose(p["base_freq"], 1.0 / 6000.0)
    assert np.isclose(p["ridge_freq"], bd.RIDGE_FREQ_RATIO / 6000.0)
    assert np.isclose(p["valley_freq"], bd.VALLEY_FREQ_RATIO / 6000.0)


def test_more_ridged_metrics_give_more_ridge_strength():
    lo = bd.params_from_metrics(_metrics(ridge=0.1))
    hi = bd.params_from_metrics(_metrics(ridge=0.9))
    assert hi["ridge_strength"] > lo["ridge_strength"]


def test_aggregate_median_is_per_metric_median():
    ms = [_metrics(relief=1000.0, ridge=0.2), _metrics(relief=2000.0, ridge=0.8),
          _metrics(relief=1500.0, ridge=0.5)]
    agg = bd.aggregate_median(ms)
    assert agg["relief_real_m"] == 1500.0          # median of [1000,2000,1500]
    assert agg["ridge_linearity"] == 0.5
    assert len(agg["amp_profile"]) == bd.N_OCTAVES  # per-band median


def test_generated_params_are_bounded():
    # the produced params must satisfy worldgen_proto's closed-form ceiling
    p = bd.params_from_metrics(_metrics())
    ii = np.linspace(0, 40000.0, 96)
    wx, wz = np.meshgrid(ii, ii)
    h = wg.generate(wx, wz, p, seed=5)
    ceiling = (sum(p["octave_amps"]) + p["ridge_strength"] + p["valley_depth"]) * p["relief_m"]
    assert np.all(np.abs(h) <= ceiling * 1.01)


def test_distilled_params_do_not_tile():
    # non-repetition (the owner's "no chunks/squares/lines" bar) on a REAL distilled-param field
    p = bd.params_from_metrics(_metrics())
    n = 4096
    span = 400000.0
    xs = np.linspace(0, span, n)
    wx = xs.reshape(1, -1); wz = np.zeros_like(wx)
    line = wg.generate(wx, wz, p, seed=5).ravel()
    line = line - line.mean()
    ac = np.correlate(line, line, mode="full")[n - 1:]
    ac = ac / ac[0]
    step = span / n
    for period_m in (8192.0, 16384.0, 50000.0, 100000.0):
        lag = int(round(period_m / step))
        if 2 <= lag < n:
            assert ac[lag] < 0.5, f"autocorr spike {ac[lag]:.2f} at {period_m} m -> tiling!"
```

- [ ] **Step 2: Run to verify it FAILS**

```powershell
python -m pytest test_biome_distill.py -q -k "params or aggregate or freqs or ridged or bounded or tile"
```
Expected: FAIL — `params_from_metrics` / `aggregate_median` not defined.

- [ ] **Step 3: Implement in `biome_distill.py`**

Add to `tools/dem_pack/biome_distill.py`:

```python
def aggregate_median(metrics_list):
    """Median of each metric across a family's per-kernel metrics dicts. amp_profile is per-band median."""
    if not metrics_list:
        raise ValueError("aggregate_median: empty metrics list")
    keys = metrics_list[0].keys()
    out = {}
    for k in keys:
        if k == "amp_profile":
            stacked = np.asarray([m[k] for m in metrics_list], dtype=np.float64)  # (kernels, N_OCTAVES)
            out[k] = np.median(stacked, axis=0).tolist()
        else:
            out[k] = float(np.median([float(m[k]) for m in metrics_list]))
    return out


def _f32(x):
    """Round a python float to f32 precision so distilled values are parity-ready (CPU==GPU later)."""
    return float(np.float32(x))


def params_from_metrics(metrics):
    """Map structural metrics -> the warped-noise generator's knobs via documented simple transforms.
    Every constant is named config above. Returns a BiomeParams dict (worldgen_proto.generate-compatible
    + slope_bias). All values finite, in-domain, f32-representable (parity-ready for Slice 3/4)."""
    wl = max(float(metrics["dominant_wavelength_m"]), 1.0)
    base_freq = 1.0 / wl
    # amp_profile normalized so band0==1.0 (already normalized by bandpass; re-assert defensively)
    amps = np.asarray(metrics["amp_profile"], dtype=np.float64)
    a0 = amps[0] if abs(amps[0]) > 1e-12 else 1.0
    amps = (amps / a0)
    ridge_strength = float(np.clip(metrics["ridge_linearity"], 0.0, 1.0)) * RIDGE_STRENGTH_MAX
    # valley_depth: incision normalized by relief -> a 0..1 fraction, clamped
    relief = max(float(metrics["relief_real_m"]), 1.0)
    valley_depth = float(np.clip(metrics["incision_depth_m"] / relief, 0.0, 1.0)) * VALLEY_DEPTH_MAX
    warp_amount = WARP_AMOUNT_FRAC * wl * float(np.clip(metrics["anisotropy"], 0.0, 1.0))
    p = {
        "relief_m": _f32(relief),
        "octave_amps": [_f32(a) for a in amps],
        "ridge_strength": _f32(ridge_strength),
        "valley_depth": _f32(valley_depth),
        "warp_amount": _f32(warp_amount),
        "base_freq": _f32(base_freq),
        "ridge_freq": _f32(RIDGE_FREQ_RATIO * base_freq),
        "valley_freq": _f32(VALLEY_FREQ_RATIO * base_freq),
        "warp_freq": _f32(1.0 / (WARP_FREQ_K * wl)),
        "slope_bias": _f32(metrics["slope_bias_deg"]),  # STORED; not yet consumed by generate()
    }
    return p
```

- [ ] **Step 4: Run to verify PASS**

```powershell
python -m pytest test_biome_distill.py -q
```
Expected: ALL pass (toolkit + params keys/domain/f32, freq derivation, ridge monotonicity, median aggregation, generator bounds, non-repetition).

- [ ] **Step 5: Commit**

```bash
git add tools/dem_pack/biome_distill.py tools/dem_pack/test_biome_distill.py
git commit -m "worldgen s2: params_from_metrics + aggregate_median — metrics->generator knobs, in-domain/f32/bounded/non-repeating

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: The I/O orchestrator (`distill_biomes.py`) — real DEMs → per-family table

**Files:**
- Create: `tools/dem_pack/distill_biomes.py`

- [ ] **Step 1: Create the orchestrator**

Create `tools/dem_pack/distill_biomes.py`:

```python
#!/usr/bin/env python3
"""Slice 2 orchestrator: distill the real WG9 DEMs (by family) into a per-family biome_params table.
WG9 is READ-ONLY. Run from repo root (or tools/dem_pack). Writes tools/dem_pack/biome_params.json.

  python tools/dem_pack/distill_biomes.py                 # all 12 families
  python tools/dem_pack/distill_biomes.py --families mountain grassland badlands   # subset (prove-on-3)
"""
from __future__ import annotations
import argparse
import collections
import json
import os
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import biome_distill as bd  # noqa: E402

WG9_KERNELS = "D:/workflows/worldgen9/factory/kernels"
MAP_PATH = os.path.join(HERE, "kernel_family_map.approved.json")
OUT_PATH = os.path.join(HERE, "biome_params.json")
MAX_ABS_ZSCORE = 12.0   # reuse build_pack.py's spike guard


def load_family_map():
    return dict(json.load(open(MAP_PATH))["map"])


def load_kernel(kid):
    z = np.load(f"{WG9_KERNELS}/{kid}/normalized_height.npy")
    meta = json.load(open(f"{WG9_KERNELS}/{kid}/kernel.json"))
    return z, meta


def distill(families=None):
    """Return {family: biome_params} for the requested families (default all)."""
    fam_of = load_family_map()
    by_fam = collections.defaultdict(list)
    for kid, fam in fam_of.items():
        by_fam[fam].append(kid)
    if families:
        by_fam = {f: by_fam[f] for f in families if f in by_fam}
        for f in families:
            if f not in by_fam:
                raise SystemExit(f"[distill] unknown family {f!r}")
    out = {}
    for fam in sorted(by_fam):
        metrics_list = []
        used = 0
        for kid in sorted(by_fam[fam]):
            z, meta = load_kernel(kid)
            if max(abs(float(z.min())), abs(float(z.max()))) > MAX_ABS_ZSCORE:
                print(f"[distill] {fam}: dropped {kid} (z-score spike)")
                continue
            metrics_list.append(bd.metrics_for_dem(z, meta))
            used += 1
        if not metrics_list:
            raise SystemExit(f"[distill] family {fam!r}: all kernels dropped — nothing to distill")
        agg = bd.aggregate_median(metrics_list)
        out[fam] = bd.params_from_metrics(agg)
        print(f"[distill] {fam}: {used} kernels -> "
              f"relief={out[fam]['relief_m']:.0f} ridge={out[fam]['ridge_strength']:.2f} "
              f"valley={out[fam]['valley_depth']:.2f} warp={out[fam]['warp_amount']:.0f} "
              f"base_wl={1.0/out[fam]['base_freq']:.0f}m")
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--families", nargs="*", default=None)
    args = ap.parse_args()
    params = distill(args.families)
    with open(OUT_PATH, "w") as f:
        json.dump(params, f, indent=1)
        f.write("\n")
    print(f"[distill] wrote {len(params)} families -> {OUT_PATH}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 2: Run on the prove-on-3 subset**

```powershell
cd tools/dem_pack
python distill_biomes.py --families mountain grassland badlands
```
Expected: prints one `[distill] <fam>: N kernels -> relief=… ridge=… valley=… warp=… base_wl=…m` line per family, writes `tools/dem_pack/biome_params.json` with 3 entries. No errors. Sanity-eyeball: mountain should show higher ridge/relief than grassland.

- [ ] **Step 3: Commit**

```bash
git add tools/dem_pack/distill_biomes.py
git commit -m "worldgen s2: distill_biomes.py — I/O orchestrator, real DEMs by family -> biome_params.json (spike-guarded, median)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Real-vs-synth render (the deliverable for the owner's eye) — prove on 3

**Files:**
- Create: `tools/dem_pack/render_biomes.py`

- [ ] **Step 1: Create the render script**

Create `tools/dem_pack/render_biomes.py`:

```python
"""Render real-vs-synth side-by-side hillshades for the owner's eye (render-first, Slice 2).
Left = the family's real DEM (a representative kernel), right = synth from its distilled params,
at MATCHED metres/pixel. Captioned with the distilled metrics. Writes to D:\\tmp\\.
  python render_biomes.py --families mountain grassland badlands
  python render_biomes.py                 # all families in biome_params.json
NOT a test — a runnable inspection tool. Character match (same KIND of terrain), not pixel copy."""
from __future__ import annotations
import argparse
import collections
import json
import os
import sys

import numpy as np
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import worldgen_proto as wg  # noqa: E402
import biome_distill as bd   # noqa: E402

WG9_KERNELS = "D:/workflows/worldgen9/factory/kernels"
MAP_PATH = os.path.join(HERE, "kernel_family_map.approved.json")
PARAMS_PATH = os.path.join(HERE, "biome_params.json")
OUT = r"D:\tmp"
TILE = 512  # render size per panel


def hillshade(z, az=315.0, alt=45.0):
    zn = (z - z.min()) / (np.ptp(z) + 1e-9)
    gy, gx = np.gradient(zn * 80.0)
    slope = np.pi / 2.0 - np.arctan(np.sqrt(gx * gx + gy * gy))
    aspect = np.arctan2(-gx, gy)
    azr = np.radians(360 - az + 90); altr = np.radians(alt)
    sh = np.sin(altr) * np.sin(slope) + np.cos(altr) * np.cos(slope) * np.cos(azr - aspect)
    return np.clip(sh, 0, 1)


def representative_kernel(fam):
    fam_of = dict(json.load(open(MAP_PATH))["map"])
    ids = sorted([k for k, f in fam_of.items() if f == fam])
    return ids[0]  # deterministic; the family's first id by sort


def real_panel(fam):
    kid = representative_kernel(fam)
    z = np.load(f"{WG9_KERNELS}/{kid}/normalized_height.npy")
    meta = json.load(open(f"{WG9_KERNELS}/{kid}/kernel.json"))
    return hillshade(z.astype(np.float64)), meta["approx_sample_spacing_m"], kid


def synth_panel(params, spacing_m):
    # match the real tile's metres/pixel so the comparison is at the same scale
    span = TILE * float(spacing_m)
    ii = np.linspace(0, span, TILE)
    wx, wz = np.meshgrid(ii + 123456.0, ii + 654321.0)  # arbitrary world offset (not origin)
    return hillshade(wg.generate(wx, wz, params, seed=7))


def to_img(sh):
    return Image.fromarray((np.asarray(sh) * 255).astype(np.uint8), mode="L").resize((TILE, TILE)).convert("RGB")


def render_family(fam, params):
    real_sh, spacing, kid = real_panel(fam)
    synth_sh = synth_panel(params, spacing)
    pad = 24
    canvas = Image.new("RGB", (TILE * 2 + pad * 3, TILE + pad * 3), (20, 20, 20))
    canvas.paste(to_img(real_sh), (pad, pad))
    canvas.paste(to_img(synth_sh), (pad * 2 + TILE, pad))
    d = ImageDraw.Draw(canvas)
    d.text((pad, 4), f"{fam}  REAL: {kid} ({spacing:.0f} m/px)", fill=(220, 220, 220))
    d.text((pad * 2 + TILE, 4), f"{fam}  SYNTH (distilled params)", fill=(220, 220, 220))
    cap = (f"relief={params['relief_m']:.0f}m ridge={params['ridge_strength']:.2f} "
           f"valley={params['valley_depth']:.2f} warp={params['warp_amount']:.0f}m "
           f"base_wl={1.0/params['base_freq']:.0f}m slope={params['slope_bias']:.1f}deg")
    d.text((pad, TILE + pad + 6), cap, fill=(180, 200, 180))
    path = rf"{OUT}\biome_{fam}.png"
    canvas.save(path)
    print(f"wrote {path}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--families", nargs="*", default=None)
    args = ap.parse_args()
    params_all = json.load(open(PARAMS_PATH))
    fams = args.families or sorted(params_all)
    for fam in fams:
        if fam not in params_all:
            raise SystemExit(f"[render] {fam!r} not in biome_params.json — run distill_biomes.py first")
        render_family(fam, params_all[fam])


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Run the render on the prove-on-3 subset**

```powershell
cd tools/dem_pack
python render_biomes.py --families mountain grassland badlands
```
Expected: writes `D:\tmp\biome_mountain.png`, `biome_grassland.png`, `biome_badlands.png` — each a real|synth side-by-side captioned with metrics. No errors.

- [ ] **Step 3: Commit**

```bash
git add tools/dem_pack/render_biomes.py
git commit -m "worldgen s2: render_biomes.py — real-vs-synth side-by-side hillshades (character match), for owner eye

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 4: OWNER EYE VERDICT — prove-on-3 (the make-or-break gate)**

The controller opens the 3 PNGs (`Start-Process`) and sends them to the owner. Owner judges per family:
- **Character match:** does the SYNTH read as the same KIND of terrain as the REAL (ridge spacing, valley density, roughness, relief feel)? (NOT same place.)
- **Mountain** should look ridged/high-relief; **grassland** soft/low-relief; **badlands** heavily incised.

Record the verdict verbatim. **If the mapping reads wrong, refine the transforms in `biome_distill.py` (the named config constants) and re-render — cheap, before fanning to 12.** Do NOT proceed to Task 6 until the owner accepts the 3.

---

## Task 5: Pack-writer — attach the validated per-family table (additive)

**Files:**
- Modify: `tools/dem_pack/dem_pack_lib.py`
- Modify: `tools/dem_pack/test_dem_pack_lib.py`

- [ ] **Step 1: Write the failing test**

Add to `tools/dem_pack/test_dem_pack_lib.py`:

```python
def test_attach_biome_params_adds_table():
    pack = {"schema": lib.SCHEMA, "version": 1, "families": {"k1": {"kernel": "kernels/k1.npy"}}}
    bp = {"mountain": {"relief_m": 1200.0, "octave_amps": [1.0, 0.5, 0.25, 0.12, 0.06, 0.03],
                       "ridge_strength": 0.8, "valley_depth": 0.3, "warp_amount": 2000.0,
                       "base_freq": 1.0/6000, "ridge_freq": 2.0/6000, "valley_freq": 1.2/6000,
                       "warp_freq": 1.0/16200, "slope_bias": 20.0}}
    out = lib.attach_biome_params(pack, bp)
    assert "biome_params" in out
    assert out["biome_params"]["mountain"]["relief_m"] == 1200.0
    assert out["families"] == pack["families"]   # per-kernel entries untouched (additive)


def test_attach_biome_params_rejects_nan_naming_family():
    pack = {"schema": lib.SCHEMA, "version": 1, "families": {}}
    bad = {"badlands": {"relief_m": float("nan"), "octave_amps": [1.0]*6, "ridge_strength": 0.4,
                        "valley_depth": 0.9, "warp_amount": 1800.0, "base_freq": 1.0/2200,
                        "ridge_freq": 2.0/2200, "valley_freq": 1.2/2200, "warp_freq": 1.0/5940,
                        "slope_bias": 30.0}}
    with pytest.raises(ValueError, match="badlands"):
        lib.attach_biome_params(pack, bad)


def test_attach_biome_params_rejects_out_of_domain_freq():
    pack = {"schema": lib.SCHEMA, "version": 1, "families": {}}
    bad = {"coast": {"relief_m": 100.0, "octave_amps": [1.0]*6, "ridge_strength": 0.1,
                     "valley_depth": 0.1, "warp_amount": 500.0, "base_freq": 0.0,  # invalid: freq must be >0
                     "ridge_freq": 0.0, "valley_freq": 0.0, "warp_freq": 0.0, "slope_bias": 5.0}}
    with pytest.raises(ValueError, match="coast"):
        lib.attach_biome_params(pack, bad)
```

> Ensure `import pytest` is at the top of `test_dem_pack_lib.py` (add it if absent).

- [ ] **Step 2: Run to verify it FAILS**

```powershell
cd tools/dem_pack
python -m pytest test_dem_pack_lib.py -q -k biome_params
```
Expected: FAIL — `attach_biome_params` not defined.

- [ ] **Step 3: Implement `attach_biome_params` in `dem_pack_lib.py`**

Add to `tools/dem_pack/dem_pack_lib.py` (after the imports / `SCHEMA`):

```python
import math

REQUIRED_BIOME_PARAM_KEYS = (
    "relief_m", "octave_amps", "ridge_strength", "valley_depth", "warp_amount",
    "base_freq", "ridge_freq", "valley_freq", "warp_freq", "slope_bias",
)
N_OCTAVE_AMPS = 6


def _validate_biome_params(family, bp):
    """Reject NaN/degenerate/out-of-domain params with a descriptive error NAMING the family
    (pillar 4 — no silent default; parity-readiness — finite, f32-representable, in-domain)."""
    for k in REQUIRED_BIOME_PARAM_KEYS:
        if k not in bp:
            raise ValueError(f"biome_params[{family!r}]: missing key {k!r}")
    amps = bp["octave_amps"]
    if not isinstance(amps, (list, tuple)) or len(amps) != N_OCTAVE_AMPS:
        raise ValueError(f"biome_params[{family!r}]: octave_amps must be length {N_OCTAVE_AMPS}")
    scalars = {k: bp[k] for k in REQUIRED_BIOME_PARAM_KEYS if k != "octave_amps"}
    for k, v in list(scalars.items()) + [(f"octave_amps[{i}]", a) for i, a in enumerate(amps)]:
        fv = float(v)
        if not math.isfinite(fv):
            raise ValueError(f"biome_params[{family!r}]: {k} not finite ({v})")
    for fk in ("base_freq", "ridge_freq", "valley_freq", "warp_freq"):
        if float(bp[fk]) <= 0.0:
            raise ValueError(f"biome_params[{family!r}]: {fk} must be > 0 (got {bp[fk]})")
    if float(bp["relief_m"]) <= 0.0:
        raise ValueError(f"biome_params[{family!r}]: relief_m must be > 0 (got {bp['relief_m']})")
    if not (0.0 <= float(bp["ridge_strength"]) <= 1.0):
        raise ValueError(f"biome_params[{family!r}]: ridge_strength out of [0,1] ({bp['ridge_strength']})")
    if not (0.0 <= float(bp["valley_depth"]) <= 1.0):
        raise ValueError(f"biome_params[{family!r}]: valley_depth out of [0,1] ({bp['valley_depth']})")


def attach_biome_params(pack_dict, biome_params):
    """Additively attach a per-FAMILY biome_params table to a pack dict (validated). Returns a NEW dict;
    the existing per-kernel `families` entries + kernels are untouched (atlas removal is Slice 4)."""
    for family, bp in biome_params.items():
        _validate_biome_params(family, bp)
    out = dict(pack_dict)
    out["biome_params"] = {f: dict(bp) for f, bp in biome_params.items()}
    return out
```

- [ ] **Step 4: Run to verify PASS**

```powershell
python -m pytest test_dem_pack_lib.py -q
```
Expected: all dem_pack_lib tests pass, including the 3 new biome_params tests (additive insert; rejects NaN naming family; rejects zero freq naming family).

- [ ] **Step 5: Commit**

```bash
git add tools/dem_pack/dem_pack_lib.py tools/dem_pack/test_dem_pack_lib.py
git commit -m "worldgen s2: attach_biome_params — additive validated per-family table (pillar4 reject naming family, parity-ready)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Fan to all 12 + write into the pack + full render (after owner accepts the 3)

**Files:**
- Modify: `tools/dem_pack/build_pack.py` (wire the table into the emitted pack)

- [ ] **Step 1: Distill all 12 families**

```powershell
cd tools/dem_pack
python distill_biomes.py
```
Expected: 12 `[distill] <fam>: …` lines, writes `biome_params.json` with 12 entries. No errors.

- [ ] **Step 2: Render all 12 real-vs-synth panels**

```powershell
python render_biomes.py
```
Expected: writes `D:\tmp\biome_<fam>.png` for all 12 families. No errors.

- [ ] **Step 3: Wire the table into the pack emitter**

In `tools/dem_pack/build_pack.py`, after `pack = lib.build_pack_dict(...)` (around line 100), add:

```python
    # Slice 2: attach the distilled per-family biome_params table (additive) if present.
    bp_path = os.path.join(HERE, "biome_params.json")
    if os.path.exists(bp_path):
        with open(bp_path) as bpf:
            pack = lib.attach_biome_params(pack, json.load(bpf))
        print(f"[build] attached biome_params for {len(pack['biome_params'])} families")
```

- [ ] **Step 4: Rebuild the gate-subset pack (proves the wiring + validation on real data)**

```powershell
cd /d/workflows/worldgen10
python tools/dem_pack/build_pack.py --gate-subset 24 --validate
```
Expected: `[build] attached biome_params for N families` + `[build] validate OK` + `[build] wrote terrain_pack.gate.json`. No validation error (proves the distilled params are all in-domain/finite — pillar 4 on real data).

- [ ] **Step 5: Full dem_pack suite (nothing regressed)**

```powershell
cd tools/dem_pack
python -m pytest -q
```
Expected: all green. Record the count (was 22; this slice adds test_biome_distill.py's tests + 3 in test_dem_pack_lib.py → new total).

- [ ] **Step 6: Commit**

```bash
git add tools/dem_pack/build_pack.py tools/dem_pack/biome_params.json wg-10/worldgen_terrain/packs/dem_v1/terrain_pack.gate.json
git commit -m "worldgen s2: distill all 12 families + wire biome_params into pack emitter (validated on real data)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 7: OWNER EYE VERDICT — all 12 (acceptance).** Controller opens the 12 PNGs + sends them. Owner judges per-family character match. Record verbatim. If some families read wrong, refine their mapping (config constants) / metrics and re-render those — the v1 metric set is explicitly not claimed final.

---

## Task 7: Update living docs + final verification

**Files:**
- Modify: `docs/plans/STATUS.md`
- Modify: `docs/plans/ROADMAP.md`

- [ ] **Step 1: Verification evidence (before any "done" claim)**

```powershell
cd tools/dem_pack
python -m pytest -q
```
Record the exact pass count + the `[build] validate OK` line from Task 6 Step 4. (Gates prove invariants; the LOOK is the owner verdicts recorded in Tasks 4 & 6.)

- [ ] **Step 2: Update STATUS.md** — under the "CURRENT DIRECTION: Worldgen Core" section, add a "Slice 2 — biome distillation" entry: per-family structural params distilled from the 115 real DEMs in real units (median aggregation; structural metrics → generator knobs, NOT a spectrum); `biome_params.json` + pack table (validated, parity-ready); real-vs-synth renders for all 12 families; the OWNER eye verdict (verbatim). State explicitly: gates prove determinism/bounds/metric-validity/non-repetition; the look is owner-judged; NEXT = close ledger B1/B2/B3 + then Slice 3 (Rust generator core, the first runtime build). No Rust/GLSL/engine touched.

- [ ] **Step 3: Update ROADMAP.md** — mark Slice 2 done (with the owner verdict summary); Slice 3 precondition = B1/B2/B3.

- [ ] **Step 4: Commit + push**

```bash
cd /d/workflows/worldgen10
git add docs/plans/STATUS.md docs/plans/ROADMAP.md
git commit -m "worldgen s2: STATUS/ROADMAP — biome distillation done, owner verdict recorded; next = B1/B2/B3 then Slice 3

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
GIT_TERMINAL_PROMPT=0 git push origin main
```

---

## Self-review notes (planner)

- **Metric source (spec §4, data-driven):** relief + slope_bias FROM vetted `kernel.json` metadata; amp_profile/ridge_linearity/incision/anisotropy/wavelength COMPUTED from the DEM; `ridge_density`/`valley_density` metadata DELIBERATELY UNUSED (dead-constant 0.100 → would collapse all biomes). `metrics_for_dem(z, meta)`.
- **Spec coverage:** §3 architecture → Tasks 1-6; §4 metrics+mapping → Tasks 1-2 (each metric→one knob, config constants); §5 real-scale (z-score trap) → Task 1 `to_metres` + metres-based incision/slope/wavelength + spike guard in Task 3; §6 pack storage (additive per-family table, validated) → Task 5 + Task 6 wiring; §7 verification (determinism/fixture-monotonicity/bounds/non-repetition + render-first + owner eye) → Tasks 1-2 tests + Tasks 4/6 renders+verdicts; §9 slice plan (prove-on-3 then 12) → Task 4 (3) → Task 6 (12); parity-readiness constraint → `_f32` in Task 2 + `_validate_biome_params` in Task 5; §11 DoD → Task 7.
- **Placeholder scan:** no TBD/TODO; every code step shows complete code; every command has expected output. Transform constants are concrete named config (refinable by eye, not placeholders).
- **Type/name consistency:** metric dict keys (`relief_real_m, amp_profile, ridge_linearity, incision_depth_m, anisotropy, dominant_wavelength_m, slope_bias_deg`) consistent across `metrics_for_dem`/`aggregate_median`/`params_from_metrics`; param dict keys match `worldgen_proto.generate`'s consumed keys + `slope_bias`; `attach_biome_params`/`_validate_biome_params`/`REQUIRED_BIOME_PARAM_KEYS` consistent Task 5↔6.
- **Render-first discipline:** the make-or-break is Task 4's owner verdict on 3 families BEFORE fanning to 12 (Task 6) — same as S1/spectral; refine cheap offline if wrong.
- **No engine touch:** entirely `tools/dem_pack/` + docs; cannot break the running engine; B1/B2/B3 explicitly deferred to Slice 3.
- **Honesty:** `slope_bias` documented as stored-not-consumed (Task 2 comment + spec); gates prove invariants only, look is owner-judged (Tasks 4/6/7); the v1 metric set is explicitly refinable.
- **scipy dependency:** uses `scipy.ndimage` (gaussian_filter/sobel) — part of the standard numpy scientific stack; if absent, Task 1 Step 4 will ImportError → `pip install scipy` (note for the implementer).
```
