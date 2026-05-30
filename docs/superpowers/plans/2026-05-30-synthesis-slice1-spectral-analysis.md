# Kernel-DNA Synthesis Slice 1 — Spectral analysis + signature + fidelity gate (Implementation Plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove OFFLINE (no runtime change) that a real kernel DEM can be analyzed into a compact spectral signature (amplitude-per-octave) AND that synthesizing noise from that signature reproduces the kernel's spectral character — the core research bet behind kernel-DNA synthesis. Emit signatures into the pack additively.

**Architecture:** A new pure-Python `spectral.py` in `tools/dem_pack/` with two halves: `analyze_signature(dem)` (2D FFT → radially-averaged power spectrum → N-octave amplitude curve) and `synthesize_field(signature, size)` (value-noise fBm using those amplitudes — a Python MIRROR of the future Rust/GLSL synth, for the fidelity round-trip test only). A `fidelity` test asserts analyze→synthesize→re-analyze matches the source spectrum within tolerance. `build_pack_dict` gains an additive `signature` field per family (raw `kernel` ref kept — runtime still uses pixels until Slice 2).

**Tech Stack:** Python 3 + numpy (2.4.4, has `numpy.fft`) + pytest. NO Rust, NO GLSL, NO runtime/render change this slice — it all lives in `tools/dem_pack/`.

---

## File structure

- **Create:** `tools/dem_pack/spectral.py` — the signature analysis + a reference synthesizer. One responsibility: turn a 2D height array into a signature, and synthesize a field from a signature (for the round-trip test). Pure functions, numpy only.
- **Create:** `tools/dem_pack/test_spectral.py` — pytest: analysis shape/validation, the round-trip spectral-fidelity gate, determinism, degenerate-input rejection.
- **Modify:** `tools/dem_pack/dem_pack_lib.py` — `build_pack_dict` adds a `signature` to each family (additive; needs the DEM array, so it reads the `.npy` — see Task 4 for how the kernel pixels reach the function).
- **Modify:** `tools/dem_pack/test_dem_pack_lib.py` — assert the signature is present + well-formed in the built pack dict.

> **Additive, not destructive:** Slice 1 ADDS `signature` alongside the existing `kernel`/`relief_m`/`footprint_m`. The runtime (Rust/GLSL) is NOT touched and still reads pixels. Slice 2 makes the Rust core consume `signature`; Slice 3 removes the atlas. This keeps Slice 1 risk-free for the running engine.

> **N_OCTAVES = 8** (spec §3 "~6–10"): spans landform→detail over 8 octave-doublings. Fixed constant in `spectral.py`, used by both analyze and synthesize so they agree.

---

## Task 1: `analyze_signature` — DEM → amplitude-per-octave

**Files:**
- Create: `tools/dem_pack/spectral.py`
- Create: `tools/dem_pack/test_spectral.py`

- [ ] **Step 1: Write the failing test**

Create `tools/dem_pack/test_spectral.py`:

```python
import numpy as np
import pytest
import spectral


def test_analyze_signature_shape_and_keys():
    rng = np.random.default_rng(0)
    dem = rng.standard_normal((64, 64)).astype(np.float32)
    sig = spectral.analyze_signature(dem, spacing_m=90.0)
    assert set(sig.keys()) == {"amp_octaves", "base_freq_per_m", "relief_m"}
    assert len(sig["amp_octaves"]) == spectral.N_OCTAVES
    assert all(np.isfinite(a) and a >= 0.0 for a in sig["amp_octaves"])
    assert sig["base_freq_per_m"] > 0.0


def test_analyze_signature_smooth_has_low_high_octaves():
    # A smooth (low-frequency) DEM should have most amplitude in the LOW octaves
    # and near-zero in the HIGH octaves; a rough one the opposite. This proves the
    # spectrum actually discriminates roughness (the whole point of the signature).
    xs = np.linspace(0, 2 * np.pi, 128)
    smooth = np.outer(np.sin(xs), np.sin(xs)).astype(np.float32)        # one low frequency
    rng = np.random.default_rng(1)
    rough = rng.standard_normal((128, 128)).astype(np.float32)          # white noise = all freqs
    s_smooth = spectral.analyze_signature(smooth, spacing_m=90.0)["amp_octaves"]
    s_rough = spectral.analyze_signature(rough, spacing_m=90.0)["amp_octaves"]
    # smooth: low octaves dominate -> ratio(last/first) small; rough: high octaves present -> larger
    smooth_hi_ratio = s_smooth[-1] / max(s_smooth[0], 1e-9)
    rough_hi_ratio = s_rough[-1] / max(s_rough[0], 1e-9)
    assert rough_hi_ratio > smooth_hi_ratio


def test_analyze_signature_rejects_degenerate():
    with pytest.raises(ValueError):
        spectral.analyze_signature(np.zeros((32, 32), dtype=np.float32), spacing_m=90.0)
    with pytest.raises(ValueError):
        bad = np.full((32, 32), np.nan, dtype=np.float32)
        spectral.analyze_signature(bad, spacing_m=90.0)
    with pytest.raises(ValueError):
        spectral.analyze_signature(np.ones((32, 32), dtype=np.float32), spacing_m=0.0)
```

- [ ] **Step 2: Run to verify it FAILS**

Run (from `tools/dem_pack/`):
```powershell
cd tools/dem_pack
python -m pytest test_spectral.py -q
```
Expected: FAIL — `spectral` module doesn't exist (ImportError).

- [ ] **Step 3: Implement `analyze_signature` in `spectral.py`**

Create `tools/dem_pack/spectral.py`:

```python
"""Kernel-DNA spectral analysis + a reference synthesizer (offline, pure numpy).

analyze_signature(dem) turns a 2-D height array into a compact "terrain signature":
an N-octave amplitude curve (the kernel's radially-averaged power spectrum binned into
octaves), a base spatial frequency, and the relief. synthesize_field(signature) grows a
non-repeating field from a signature using a value-noise fBm whose per-octave amplitudes
are the signature's — a Python MIRROR of the future Rust/GLSL runtime synth, used by the
fidelity round-trip test. NOTHING here runs at engine runtime (Slice 1 is offline-only)."""

import numpy as np

N_OCTAVES = 8


def analyze_signature(dem: np.ndarray, spacing_m: float) -> dict:
    """DEM (2-D float array, metres) + sample spacing (m) -> signature dict:
      amp_octaves[N_OCTAVES] : relative amplitude per octave (radial power spectrum, binned)
      base_freq_per_m        : spatial frequency (1/m) of octave 0 (the largest feature scale)
      relief_m               : peak-to-peak vertical range
    Raises ValueError on non-finite / flat / bad-spacing input (no silent defaults)."""
    if spacing_m <= 0.0:
        raise ValueError(f"spacing_m must be > 0, got {spacing_m}")
    a = np.asarray(dem, dtype=np.float64)
    if a.ndim != 2 or a.shape[0] < 4 or a.shape[1] < 4:
        raise ValueError(f"dem must be 2-D >=4x4, got shape {a.shape}")
    if not np.all(np.isfinite(a)):
        raise ValueError("dem has non-finite values")
    relief = float(a.max() - a.min())
    if relief <= 0.0:
        raise ValueError("dem is flat (relief == 0) -> no spectrum")

    # 2-D FFT power spectrum (de-mean so DC doesn't dominate).
    a = a - a.mean()
    n = min(a.shape)
    a = a[:n, :n]                       # square crop for clean radial bins
    f = np.fft.fftshift(np.fft.fft2(a))
    power = np.abs(f) ** 2

    # radial frequency of each pixel (cycles per sample), 0 at centre.
    cy, cx = n // 2, n // 2
    yy, xx = np.mgrid[0:n, 0:n]
    r = np.sqrt((yy - cy) ** 2 + (xx - cx) ** 2)        # radius in pixels
    r_norm = r / (n / 2.0)                                # 0..~1 (1 = Nyquist)

    # bin radial power into N_OCTAVES log-spaced frequency bands (octaves).
    # octave i covers normalized-frequency [2^-(N-i), 2^-(N-1-i)).
    amp = np.zeros(N_OCTAVES, dtype=np.float64)
    for i in range(N_OCTAVES):
        lo = 2.0 ** (-(N_OCTAVES - i))
        hi = 2.0 ** (-(N_OCTAVES - 1 - i))
        mask = (r_norm >= lo) & (r_norm < hi)
        if np.any(mask):
            # amplitude ~ sqrt(mean power) in the band
            amp[i] = float(np.sqrt(power[mask].mean()))

    # normalize the curve to unit max (the SHAPE is the DNA; relief sets absolute scale).
    peak = amp.max()
    if peak <= 0.0:
        raise ValueError("dem produced an all-zero spectrum")
    amp = amp / peak

    # base_freq = the spatial frequency (1/m) at octave 0's center band.
    # octave 0 center normalized-freq ~ 2^-(N) ... map to cycles/m: norm_freq * (0.5/spacing).
    base_norm = 2.0 ** (-(N_OCTAVES - 0.5))
    base_freq_per_m = base_norm * (0.5 / spacing_m)

    return {
        "amp_octaves": [float(x) for x in amp],
        "base_freq_per_m": float(base_freq_per_m),
        "relief_m": relief,
    }
```

- [ ] **Step 4: Run to verify analysis tests PASS**

```powershell
python -m pytest test_spectral.py -q -k "analyze"
```
Expected: 3 analyze tests pass (shape/keys, smooth-vs-rough discrimination, degenerate rejection).

- [ ] **Step 5: Commit**

```bash
git add tools/dem_pack/spectral.py tools/dem_pack/test_spectral.py
git commit -m "synthesis s1: analyze_signature — DEM -> amplitude-per-octave spectral DNA (analysis tests green)"
```

---

## Task 2: `synthesize_field` — signature → non-repeating field (the reference synth)

**Files:**
- Modify: `tools/dem_pack/spectral.py`
- Modify: `tools/dem_pack/test_spectral.py`

- [ ] **Step 1: Write the failing test (the spectral-fidelity gate + determinism)**

Add to `tools/dem_pack/test_spectral.py`:

```python
def test_synthesize_field_deterministic_and_nonflat():
    sig = {"amp_octaves": [1.0, 0.5, 0.25, 0.12, 0.06, 0.03, 0.015, 0.007],
           "base_freq_per_m": 1.0e-4, "relief_m": 1000.0}
    a = spectral.synthesize_field(sig, size=64, spacing_m=90.0, seed=7)
    b = spectral.synthesize_field(sig, size=64, spacing_m=90.0, seed=7)
    assert a.shape == (64, 64)
    assert np.allclose(a, b)                      # deterministic
    assert float(a.max() - a.min()) > 0.0         # not flat


def test_spectral_fidelity_roundtrip():
    # THE core research proof: a kernel-like DEM -> signature -> synthesized field ->
    # re-analyzed spectrum should MATCH the source signature's octave shape. This is the
    # "synthesis behaves like the real place (spectrally)" gate. We use a synthetic DEM
    # with a KNOWN spectral falloff so the round-trip is checkable without a real .npy.
    rng = np.random.default_rng(3)
    # build a DEM with a 1/f-ish spectrum (pink-ish): sum of decaying-amplitude sine grids
    n = 128
    xs = np.linspace(0, 2 * np.pi, n)
    dem = np.zeros((n, n), dtype=np.float64)
    for k, amp in [(1, 1.0), (2, 0.5), (4, 0.25), (8, 0.12), (16, 0.06)]:
        ph = rng.uniform(0, 2 * np.pi)
        dem += amp * np.outer(np.sin(k * xs + ph), np.sin(k * xs + ph))
    src = spectral.analyze_signature(dem, spacing_m=90.0)
    field = spectral.synthesize_field(src, size=256, spacing_m=90.0, seed=11)
    syn = spectral.analyze_signature(field, spacing_m=90.0)
    # compare the NORMALIZED octave curves: cosine similarity high (>0.9) => same spectral shape.
    u = np.array(src["amp_octaves"]); v = np.array(syn["amp_octaves"])
    cos = float(np.dot(u, v) / (np.linalg.norm(u) * np.linalg.norm(v) + 1e-12))
    assert cos > 0.9, f"spectral fidelity too low: cos={cos:.3f} src={u} syn={v}"
```

- [ ] **Step 2: Run to verify it FAILS**

```powershell
python -m pytest test_spectral.py -q -k "synthesize or fidelity"
```
Expected: FAIL — `synthesize_field` not defined.

- [ ] **Step 3: Implement `synthesize_field` in `spectral.py`**

Add to `tools/dem_pack/spectral.py`:

```python
def _value_noise_2d(gx: np.ndarray, gz: np.ndarray, seed: int) -> np.ndarray:
    """Vectorized value noise on a grid of world coords (gx,gz arrays, same shape). Hash-based
    lattice + smoothstep bilinear, output in [-1,1]. Mirrors the runtime value-noise SHAPE
    (this is the offline reference; the Rust/GLSL runtime is built in Slice 2/3). Deterministic
    in seed + integer lattice -> non-repeating across world space."""
    def hashf(ix, iz):
        # integer hash -> [0,1); wrapping uint math (numpy int64, mask to 32 bits).
        h = (ix.astype(np.int64) * 374761393 + iz.astype(np.int64) * 668265263 + seed * 362437)
        h = (h ^ (h >> 13)) * 1274126177
        h = h & 0x7fffffff
        return (h.astype(np.float64) / float(0x7fffffff))

    x0 = np.floor(gx).astype(np.int64); z0 = np.floor(gz).astype(np.int64)
    tx = gx - x0; tz = gz - z0
    sx = tx * tx * (3.0 - 2.0 * tx); sz = tz * tz * (3.0 - 2.0 * tz)
    c00 = hashf(x0, z0); c10 = hashf(x0 + 1, z0)
    c01 = hashf(x0, z0 + 1); c11 = hashf(x0 + 1, z0 + 1)
    top = c00 + (c10 - c00) * sx
    bot = c01 + (c11 - c01) * sx
    return (top + (bot - top) * sz) * 2.0 - 1.0      # [-1,1]


def synthesize_field(signature: dict, size: int, spacing_m: float, seed: int = 0) -> np.ndarray:
    """Grow a size×size height field (metres) from a signature: value-noise fBm whose per-octave
    amplitudes are signature['amp_octaves'], starting at base_freq_per_m, lacunarity 2, scaled to
    relief. Continuous over world space -> non-repeating (the runtime synth mirrors this)."""
    amp = signature["amp_octaves"]
    base_freq = float(signature["base_freq_per_m"])
    relief = float(signature["relief_m"])
    if len(amp) != N_OCTAVES:
        raise ValueError(f"signature amp_octaves len {len(amp)} != {N_OCTAVES}")
    # world coords for the grid (metres): a patch at an arbitrary world origin.
    ii = np.arange(size, dtype=np.float64) * spacing_m
    wx, wz = np.meshgrid(ii, ii)
    h = np.zeros((size, size), dtype=np.float64)
    freq = base_freq
    for i in range(N_OCTAVES):
        h += amp[i] * _value_noise_2d(wx * freq, wz * freq, seed + i)
        freq *= 2.0
    # scale to unit RMS then to relief (so relief_m is the controlling vertical scale).
    rms = float(np.sqrt(np.mean(h * h)))
    if rms > 0.0:
        h = h / rms
    return (h * (relief / 6.0)).astype(np.float64)    # /6 ~ unit-RMS -> peak ~relief over the octaves
```

- [ ] **Step 4: Run to verify synth + fidelity tests PASS**

```powershell
python -m pytest test_spectral.py -q
```
Expected: ALL tests pass, including `test_spectral_fidelity_roundtrip` (cos > 0.9 — the synthesized field's spectrum matches the source's octave shape). **This is the core research proof: spectral DNA round-trips.**

IF `test_spectral_fidelity_roundtrip` FAILS (cos <= 0.9): this is a REAL FINDING, not a bug to force. It means binning/normalization between analyze and synthesize disagree. Investigate: are analyze's octave bands and synthesize's octave frequencies aligned (both N_OCTAVES, both lacunarity 2, base_freq consistent)? Tune the band/frequency alignment so a field synthesized at amplitude curve C re-analyzes to ~C. Do NOT lower the 0.9 threshold to pass — fix the alignment. If after genuine effort the spectral round-trip can't exceed ~0.9, STOP and report — it means pure spectral may not be faithful enough and the design's shaping-seam discussion is needed earlier.

- [ ] **Step 5: Commit**

```bash
git add tools/dem_pack/spectral.py tools/dem_pack/test_spectral.py
git commit -m "synthesis s1: synthesize_field + spectral-fidelity round-trip gate (cos>0.9 — DNA round-trips)"
```

---

## Task 3: Run the fidelity gate on a REAL kernel (the honest proof)

The synthetic-DEM round-trip proves the math; this proves it on actual COP30 data.

**Files:**
- Modify: `tools/dem_pack/test_spectral.py`

- [ ] **Step 1: Add a real-kernel fidelity test**

Add to `tools/dem_pack/test_spectral.py`:

```python
import os

REAL_KERNEL = os.path.join(
    os.path.dirname(__file__), "..", "..",
    "wg-10", "worldgen_terrain", "packs", "dem_v1", "kernels",
    "badlands__cop30_badlands_grand_canyon_112_1_36_1.npy")


@pytest.mark.skipif(not os.path.exists(REAL_KERNEL), reason="real kernel .npy not present")
def test_fidelity_on_real_kernel():
    dem = np.load(REAL_KERNEL).astype(np.float64)
    src = spectral.analyze_signature(dem, spacing_m=90.0)
    field = spectral.synthesize_field(src, size=256, spacing_m=90.0, seed=5)
    syn = spectral.analyze_signature(field, spacing_m=90.0)
    u = np.array(src["amp_octaves"]); v = np.array(syn["amp_octaves"])
    cos = float(np.dot(u, v) / (np.linalg.norm(u) * np.linalg.norm(v) + 1e-12))
    print(f"\n[real-kernel fidelity] grand_canyon cos={cos:.3f}\n  src={np.round(u,3)}\n  syn={np.round(v,3)}")
    assert cos > 0.85, f"real-kernel spectral fidelity cos={cos:.3f} (src={u} syn={v})"
```

(Threshold 0.85 vs the synthetic 0.9 — real DEM spectra are noisier than a clean synthetic falloff, so slightly looser is honest. If a real kernel scores well under 0.85, that's the signal that pure spectral isn't capturing the kernel — report it; it informs whether the shaping seam is needed before the runtime rebuild.)

- [ ] **Step 2: Run + EYEBALL the printed octave curves**

```powershell
python -m pytest test_spectral.py -q -s -k real_kernel
```
Expected: pass (cos > 0.85). The `-s` prints the src vs syn octave curves — confirm they're similar shapes (the synthesized field reproduces the Grand Canyon's spectral falloff). Record the cos value + the curves in the commit message.

- [ ] **Step 3: Commit**

```bash
git add tools/dem_pack/test_spectral.py
git commit -m "synthesis s1: spectral fidelity proven on a REAL COP30 kernel (grand_canyon cos=<value>)"
```

---

## Task 4: Emit `signature` into the pack (additive)

Wire the analysis into `build_pack_dict` so built packs carry signatures alongside the existing kernel refs.

**Files:**
- Modify: `tools/dem_pack/dem_pack_lib.py`
- Modify: `tools/dem_pack/test_dem_pack_lib.py`

- [ ] **Step 1: Write the failing test**

Add to `tools/dem_pack/test_dem_pack_lib.py`:

```python
import numpy as np


def test_build_pack_dict_includes_signature(tmp_path):
    # build_pack_dict gains a kernel_arrays param {kid -> 2D np.array}; each family entry
    # gets a `signature` (additive — `kernel` ref stays). Signature is well-formed.
    import spectral
    fam_of = {"m1": "mountain", "m2": "mountain", "m3": "mountain"}
    rng = np.random.default_rng(0)
    meta = {k: {"height_range_m": 1000.0, "approx_sample_spacing_m": 90.0, "sample_px": 64}
            for k in fam_of}
    kernel_arrays = {k: rng.standard_normal((64, 64)).astype(np.float32) for k in fam_of}
    pack = lib.build_pack_dict(fam_of, meta, kernel_arrays=kernel_arrays)
    fam = pack["families"]["m1"]
    assert "kernel" in fam and "relief_m" in fam and "footprint_m" in fam   # unchanged
    assert "signature" in fam
    sig = fam["signature"]
    assert len(sig["amp_octaves"]) == spectral.N_OCTAVES
    assert sig["base_freq_per_m"] > 0.0 and sig["relief_m"] > 0.0


def test_build_pack_dict_without_arrays_omits_signature():
    # back-compat: no kernel_arrays -> no signature (existing callers unaffected).
    fam_of = {"m1": "mountain", "m2": "mountain", "m3": "mountain"}
    meta = {k: {"height_range_m": 1000.0, "approx_sample_spacing_m": 90.0, "sample_px": 64}
            for k in fam_of}
    pack = lib.build_pack_dict(fam_of, meta)
    assert "signature" not in pack["families"]["m1"]
```

- [ ] **Step 2: Run to verify it FAILS**

```powershell
python -m pytest test_dem_pack_lib.py -q -k signature
```
Expected: FAIL — `build_pack_dict` has no `kernel_arrays` param / no signature emitted.

- [ ] **Step 3: Add signature emission to `build_pack_dict`**

In `tools/dem_pack/dem_pack_lib.py`, at the top add `import spectral` (next to the other imports). Change the `build_pack_dict` signature + the family-entry construction:

```python
def build_pack_dict(fam_of, meta, footprint_scale=1.0, kernel_arrays=None):
    """... (existing docstring) ... If kernel_arrays {kernel_id -> 2D np.array} is given, each
    family also gets a `signature` (spectral DNA) via spectral.analyze_signature — additive; the
    raw `kernel` ref is kept (runtime still reads pixels until the synthesis runtime lands)."""
    if footprint_scale <= 0.0:
        raise ValueError(f"footprint_scale must be > 0, got {footprint_scale}")
    families = {}
    for kid, fam in fam_of.items():
        m = meta.get(kid)
        if m is None:
            raise ValueError(f"kernel {kid!r}: no metadata")
        relief = float(m.get("height_range_m") or 0.0)
        spacing = float(m.get("approx_sample_spacing_m") or 0.0)
        px = int(m.get("sample_px") or 0)
        if relief <= 0.0:
            raise ValueError(f"kernel {kid!r}: relief (height_range_m) must be > 0, got {relief}")
        if spacing <= 0.0 or px <= 0:
            raise ValueError(f"kernel {kid!r}: footprint inputs must be > 0 (spacing={spacing}, px={px})")
        entry = {
            "kernel": f"kernels/{kid}.npy",
            "relief_m": relief,
            "footprint_m": spacing * px * footprint_scale,
        }
        if kernel_arrays is not None:
            arr = kernel_arrays.get(kid)
            if arr is None:
                raise ValueError(f"kernel {kid!r}: kernel_arrays given but missing this kernel")
            entry["signature"] = spectral.analyze_signature(arr, spacing_m=spacing)
        families[kid] = entry
    palettes = compose_palettes(fam_of)
    if not palettes:
        raise ValueError("no palettes composed (empty family map)")
    compatibility = _compose_compatibility(palettes)
    return {
        "schema": SCHEMA,
        "version": 1,
        "grammar_constants": dict(DEFAULT_GRAMMAR_CONSTANTS),
        "palettes": palettes,
        "compatibility": compatibility,
        "families": families,
    }
```
(Read the actual current `return {...}` block and keep its exact keys — the snippet above mirrors the structure seen in the file; match what's really there.)

- [ ] **Step 4: Run to verify PASS**

```powershell
python -m pytest test_dem_pack_lib.py -q
```
Expected: all pass — the new signature tests + all existing dem_pack tests (the `kernel_arrays=None` default keeps every existing caller working).

- [ ] **Step 5: Commit**

```bash
git add tools/dem_pack/dem_pack_lib.py tools/dem_pack/test_dem_pack_lib.py
git commit -m "synthesis s1: build_pack_dict emits additive `signature` when kernel arrays supplied (back-compat)"
```

---

## Task 5: Full test run + STATUS

- [ ] **Step 1: Run the whole dem_pack test suite**

```powershell
cd tools/dem_pack
python -m pytest -q
```
Expected: all green (test_spectral.py + test_dem_pack_lib.py). Record the count.

- [ ] **Step 2: Update STATUS.md** — add a "Kernel-DNA synthesis Slice 1" entry: spectral analysis + reference synth landed OFFLINE in `tools/dem_pack/spectral.py`; the spectral-fidelity round-trip gate passes on a synthetic DEM (cos>0.9) AND a real COP30 kernel (grand_canyon cos=<value>) — the core research bet (spectral DNA round-trips) is PROVEN offline; signatures emitted additively into the pack (raw kernel refs kept; runtime untouched). Note explicitly: this proves spectral ROUGHNESS fidelity, NOT human-judged structure — that's the owner fly after the runtime lands. NEXT = Slice 2 (Rust synthesis core). No Rust/GLSL/render touched.

- [ ] **Step 3: Commit STATUS.**

```bash
git add docs/plans/STATUS.md
git commit -m "synthesis s1: STATUS — spectral DNA proven offline (synthetic + real kernel fidelity), signatures in pack"
```

---

## Self-review notes (planner)

- **Spec coverage (Slice 1):** spec §3 (signature = radial power spectrum → amp-per-octave + base_freq + relief, validated) → Task 1; spec §6 NEW spectral-fidelity gate (analyze→synth→re-analyze matches) → Tasks 2+3; spec §3 storage (additive signature in pack) → Task 4. Runtime synth (§4), GPU parity, atlas removal are Slices 2-4, correctly absent.
- **The core risk is retired in Tasks 2-3** (does spectral synth capture a kernel) — OFFLINE, before any runtime, exactly per the spec's slice rationale. The "if fidelity fails, report don't force" guard is explicit (Task 2 Step 4, Task 3 Step 1).
- **Additive/back-compat:** `kernel_arrays=None` default → existing `build_pack_dict` callers + all existing tests unaffected; `kernel` ref kept; NO runtime change. Slice 1 cannot break the running engine.
- **Placeholder note:** `<value>` in the real-kernel commit/STATUS = the measured cos (filled at run time); the 0.85/0.9 thresholds are concrete. N_OCTAVES=8 concrete.
- **Name consistency:** `analyze_signature`, `synthesize_field`, `N_OCTAVES`, `amp_octaves`, `base_freq_per_m`, `relief_m`, `kernel_arrays`, `signature` used identically across spectral.py + dem_pack_lib.py + both test files.
- **Honesty:** the gate proves spectral roughness fidelity (cos of octave curves), NOT "looks like the Alps to a human" — stated in Task 5 STATUS + the spec §6. The real-kernel test + `-s` eyeball is the strongest offline evidence; the human judgment is the post-runtime fly.
