# Worldgen Core Slice 1 — Generator prototype (render-first) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove OFFLINE (no runtime/Rust/GLSL) that the warped-noise generator produces a CONTIGUOUS, STRUCTURED, non-repeating landmass that reads "like Google Maps explore" — by building `generate(x,z,params)` in Python and rendering hillshaded images of large areas + multiple hand-tuned biomes for the OWNER to judge by eye. This retires the core look-risk cheaply, before any runtime rebuild (the discipline that killed the spectral approach in half a day).

**Architecture:** A pure-numpy `worldgen_proto.py` implementing the generator stages from the spec §3 — domain warp → macro fBm landmass → ridged ridgelines → inverted-ridged valley carving — all driven by a `BiomeParams` dict. A `render_worldgen.py` script hillshades large generated areas + a biome-transition strip to PNGs. Tests prove the generator's invariants (deterministic, bounded, each stage does what it claims, no tiling-period auto-correlation). The DELIVERABLE is the images + the owner's eye verdict.

**Tech Stack:** Python 3 + numpy (2.4.4) + PIL (for PNG output) + pytest. NO Rust, NO GLSL, NO Godot, NO runtime/render-engine change — entirely in `tools/dem_pack/`.

---

## File structure

- **Create:** `tools/dem_pack/worldgen_proto.py` — the generator: `value_noise`, `fbm`, `ridged_fbm`, `domain_warp`, and `generate(wx, wz, params)` operating on numpy coordinate grids. Pure functions; the Python MIRROR of the future Rust/GLSL `generate`. One responsibility: turn (coords, params) → height field.
- **Create:** `tools/dem_pack/test_worldgen_proto.py` — pytest: determinism, bounded, stage behavior (warp changes the field; ridges add positive ridged structure; valleys subtract), and a NON-REPETITION auto-correlation check (no peak at a tiling period).
- **Create:** `tools/dem_pack/render_worldgen.py` — a runnable script (not a test) that generates large areas + a biome-transition strip and writes hillshaded PNGs to `D:\tmp\` for the owner to inspect. Reuses a hillshade helper.

> **`N_OCTAVES = 6`** for the prototype (spec §3 "N octaves"). **`BiomeParams` is a plain dict** in the prototype (the Rust struct comes in Slice 3): keys `relief_m, octave_amps (len N_OCTAVES), ridge_strength, valley_depth, warp_amount, base_freq, ridge_freq, valley_freq, warp_freq`. All scales are explicit params (the adaptable-scale principle).

> **This slice writes NO production code** — it's all in `tools/dem_pack/`, offline, render-first. Nothing touches the engine. The point is the IMAGES.

---

## Task 1: The noise toolkit (value_noise, fbm, ridged_fbm, domain_warp)

**Files:**
- Create: `tools/dem_pack/worldgen_proto.py`
- Create: `tools/dem_pack/test_worldgen_proto.py`

- [ ] **Step 1: Write the failing test**

Create `tools/dem_pack/test_worldgen_proto.py`:

```python
import numpy as np
import pytest
import worldgen_proto as wg


def _grid(n=64, span=4000.0, ox=0.0, oz=0.0):
    ii = np.linspace(0, span, n)
    return np.meshgrid(ii + ox, ii + oz)


def test_value_noise_range_and_determinism():
    wx, wz = _grid()
    a = wg.value_noise(wx, wz, seed=1)
    b = wg.value_noise(wx, wz, seed=1)
    assert a.shape == wx.shape
    assert np.allclose(a, b)                       # deterministic
    assert a.min() >= -1.0001 and a.max() <= 1.0001  # in [-1,1]
    assert float(a.max() - a.min()) > 0.1          # not flat


def test_fbm_is_multi_octave_and_bounded():
    wx, wz = _grid()
    h = wg.fbm(wx, wz, base_freq=1.0/2000.0, octaves=6, seed=2)
    assert h.shape == wx.shape
    assert np.all(np.isfinite(h))
    assert h.min() >= -1.5 and h.max() <= 1.5      # normalized fbm stays bounded


def test_ridged_fbm_is_nonnegative_and_ridgey():
    # ridged = 1-|noise| -> in [0,1], biased high (ridge crests), distinct from plain fbm.
    wx, wz = _grid()
    r = wg.ridged_fbm(wx, wz, base_freq=1.0/2000.0, octaves=4, seed=3)
    assert r.min() >= -0.0001 and r.max() <= 1.0001
    assert float(r.mean()) > 0.3                    # ridged noise sits high (crest-biased)


def test_domain_warp_displaces_the_field():
    # warping the coords must CHANGE the sampled field (vs unwarped), proving warp is active.
    wx, wz = _grid()
    plain = wg.fbm(wx, wz, base_freq=1.0/2000.0, octaves=4, seed=4)
    wxx, wzz = wg.domain_warp(wx, wz, warp_amount=1500.0, warp_freq=1.0/6000.0, seed=4)
    warped = wg.fbm(wxx, wzz, base_freq=1.0/2000.0, octaves=4, seed=4)
    assert wxx.shape == wx.shape
    assert not np.allclose(plain, warped)           # warp actually bent space
    # warp_amount=0 is a no-op (back-compat / off switch)
    wx0, wz0 = wg.domain_warp(wx, wz, warp_amount=0.0, warp_freq=1.0/6000.0, seed=4)
    assert np.allclose(wx0, wx) and np.allclose(wz0, wz)
```

- [ ] **Step 2: Run to verify it FAILS**

```powershell
cd tools/dem_pack
python -m pytest test_worldgen_proto.py -q
```
Expected: FAIL — `worldgen_proto` module doesn't exist (ImportError).

- [ ] **Step 3: Implement the noise toolkit in `worldgen_proto.py`**

Create `tools/dem_pack/worldgen_proto.py`:

```python
"""WorldGen10 generator PROTOTYPE — pure numpy, offline, render-first (no engine/runtime).

The python MIRROR of the future Rust/GLSL `generate` (worldgen-core spec §3): domain warp ->
macro fBm landmass -> ridged ridgelines -> inverted-ridged valley carving, all driven by a
BiomeParams dict. This file exists to RENDER images the owner judges before any runtime rebuild
(the discipline that killed the spectral approach cheaply). NOTHING here runs at engine runtime."""

import numpy as np

N_OCTAVES = 6


def _hash2(ix, iz, seed):
    # integer lattice hash -> [0,1). Deterministic; wrapping int math.
    h = (ix.astype(np.int64) * 374761393 + iz.astype(np.int64) * 668265263 + int(seed) * 362437)
    h = (h ^ (h >> 13)) * 1274126177
    h = h & 0x7fffffff
    return h.astype(np.float64) / float(0x7fffffff)


def _fade(t):  # quintic smootherstep (C2) — smoother than cubic, fewer grid artifacts
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0)


def value_noise(wx, wz, seed=0):
    """Value noise on world-coord grids (cell size = 1 world unit at the given coords). [-1,1]."""
    x0 = np.floor(wx).astype(np.int64); z0 = np.floor(wz).astype(np.int64)
    tx = _fade(wx - x0); tz = _fade(wz - z0)
    c00 = _hash2(x0, z0, seed); c10 = _hash2(x0 + 1, z0, seed)
    c01 = _hash2(x0, z0 + 1, seed); c11 = _hash2(x0 + 1, z0 + 1, seed)
    top = c00 + (c10 - c00) * tx
    bot = c01 + (c11 - c01) * tx
    return (top + (bot - top) * tz) * 2.0 - 1.0


def fbm(wx, wz, base_freq, octaves, seed=0, gain=0.5, lacunarity=2.0):
    """Multi-octave value-noise fBm, normalized to ~[-1,1] (sum/Σgain^i). base_freq in 1/world-unit."""
    h = np.zeros_like(wx, dtype=np.float64)
    amp = 1.0; norm = 0.0; freq = base_freq
    for i in range(octaves):
        h += amp * value_noise(wx * freq, wz * freq, seed + i)
        norm += amp; amp *= gain; freq *= lacunarity
    return h / max(norm, 1e-9)


def ridged_fbm(wx, wz, base_freq, octaves, seed=0, gain=0.5, lacunarity=2.0):
    """Ridged fBm: each octave = 1 - |value_noise| (crest-biased -> linear ridges), summed+normalized -> [0,1]."""
    h = np.zeros_like(wx, dtype=np.float64)
    amp = 1.0; norm = 0.0; freq = base_freq
    for i in range(octaves):
        n = 1.0 - np.abs(value_noise(wx * freq, wz * freq, seed + i))
        h += amp * n
        norm += amp; amp *= gain; freq *= lacunarity
    return h / max(norm, 1e-9)


def domain_warp(wx, wz, warp_amount, warp_freq, seed=0):
    """Bend the coords by a low-freq fbm vector field. warp_amount in world units (0 = no warp).
    This is the anti-grid/anti-repeat spine: downstream sampling at warped coords never reads as a tile."""
    if warp_amount == 0.0:
        return wx, wz
    dx = fbm(wx, wz, warp_freq, 3, seed + 17)
    dz = fbm(wx, wz, warp_freq, 3, seed + 43)
    return wx + warp_amount * dx, wz + warp_amount * dz
```

- [ ] **Step 4: Run to verify PASS**

```powershell
python -m pytest test_worldgen_proto.py -q
```
Expected: all 4 toolkit tests pass (range/determinism, fbm bounded, ridged nonneg+crest-biased, warp displaces + zero-is-noop).

- [ ] **Step 5: Commit**

```bash
git add tools/dem_pack/worldgen_proto.py tools/dem_pack/test_worldgen_proto.py
git commit -m "worldgen s1: noise toolkit (value_noise/fbm/ridged_fbm/domain_warp) — prototype, render-first"
```

---

## Task 2: `generate(wx,wz,params)` — the full generator + invariant tests

**Files:**
- Modify: `tools/dem_pack/worldgen_proto.py`
- Modify: `tools/dem_pack/test_worldgen_proto.py`

- [ ] **Step 1: Write the failing test**

Add to `tools/dem_pack/test_worldgen_proto.py`:

```python
MOUNTAIN = {
    "relief_m": 1200.0,
    "octave_amps": [1.0, 0.5, 0.25, 0.12, 0.06, 0.03],
    "ridge_strength": 0.9, "valley_depth": 0.5, "warp_amount": 2500.0,
    "base_freq": 1.0/3000.0, "ridge_freq": 1.0/1500.0,
    "valley_freq": 1.0/2500.0, "warp_freq": 1.0/8000.0,
}
PLAINS = {
    "relief_m": 180.0,
    "octave_amps": [1.0, 0.4, 0.18, 0.08, 0.03, 0.01],
    "ridge_strength": 0.05, "valley_depth": 0.15, "warp_amount": 1200.0,
    "base_freq": 1.0/4000.0, "ridge_freq": 1.0/1500.0,
    "valley_freq": 1.0/3000.0, "warp_freq": 1.0/9000.0,
}


def test_generate_deterministic_finite_relief_scaled():
    wx, wz = _grid(96, span=20000.0)
    a = wg.generate(wx, wz, MOUNTAIN, seed=5)
    b = wg.generate(wx, wz, MOUNTAIN, seed=5)
    assert a.shape == wx.shape
    assert np.allclose(a, b)                          # deterministic
    assert np.all(np.isfinite(a))
    # mountain (relief 1200, ridge_strength 0.9) has much more vertical range than plains
    p = wg.generate(wx, wz, PLAINS, seed=5)
    assert float(np.ptp(a)) > 3.0 * float(np.ptp(p))


def test_generate_bounded_by_closed_form():
    # |h| before relief <= Σoctave_amps + ridge_strength + valley_depth ; ×relief is the ceiling.
    wx, wz = _grid(96, span=20000.0)
    a = wg.generate(wx, wz, MOUNTAIN, seed=5)
    ceiling = (sum(MOUNTAIN["octave_amps"]) + MOUNTAIN["ridge_strength"] + MOUNTAIN["valley_depth"]) * MOUNTAIN["relief_m"]
    assert np.all(np.abs(a) <= ceiling * 1.01)


def test_generate_no_tiling_autocorrelation():
    # NON-REPETITION (the owner's "no chunks/squares/lines" bar): sample a long 1-D world transect
    # and assert its autocorrelation has NO strong peak at any candidate tiling period (e.g. the old
    # 8192 m page span or the kernel footprints). A tiled field would spike at its period.
    n = 4096
    span = 400000.0                                   # 400 km transect
    xs = np.linspace(0, span, n)
    wx = xs.reshape(1, -1); wz = np.zeros_like(wx)
    line = wg.generate(wx, wz, MOUNTAIN, seed=5).ravel()
    line = line - line.mean()
    ac = np.correlate(line, line, mode="full")[n-1:]  # autocorr, lags 0..n-1
    ac = ac / ac[0]
    step = span / n                                   # metres per lag
    # check candidate tiling periods (page span 8192 m, kernel footprints ~50-220 km)
    for period_m in (8192.0, 16384.0, 50000.0, 100000.0):
        lag = int(round(period_m / step))
        if 2 <= lag < n:
            assert ac[lag] < 0.5, f"autocorr spike {ac[lag]:.2f} at {period_m} m -> tiling/repeat!"
```

- [ ] **Step 2: Run to verify it FAILS**

```powershell
python -m pytest test_worldgen_proto.py -q -k generate
```
Expected: FAIL — `generate` not defined.

- [ ] **Step 3: Implement `generate` in `worldgen_proto.py`**

Add to `tools/dem_pack/worldgen_proto.py`:

```python
def generate(wx, wz, params, seed=0):
    """The worldgen generator (spec §3): warp -> macro fBm landmass -> ridged ridgelines ->
    inverted-ridged valley carving -> ×relief. params is a BiomeParams dict. Pure function of
    world coords -> seamless/contiguous by construction; the python mirror of the runtime synth."""
    amps = params["octave_amps"]
    # 1. DOMAIN WARP — the anti-grid spine.
    w_x, w_z = domain_warp(wx, wz, params["warp_amount"], params["warp_freq"], seed)
    # 2. MACRO LANDMASS — multi-octave fBm at the biome's base freq + octave amplitudes.
    h = np.zeros_like(wx, dtype=np.float64)
    freq = params["base_freq"]
    for i in range(len(amps)):
        h += amps[i] * value_noise(w_x * freq, w_z * freq, seed + i)
        freq *= 2.0
    # 3. RIDGES — linear ridgelines, amplified in uplands (where h is already high).
    upland = np.clip((h - (-0.1)) / 0.6, 0.0, 1.0)    # smoothstep-ish 0..1 over the upper range
    upland = upland * upland * (3.0 - 2.0 * upland)
    h = h + params["ridge_strength"] * upland * ridged_fbm(w_x, w_z, params["ridge_freq"], 4, seed + 100)
    # 4. VALLEYS — inverted ridged noise carves connected drainage.
    h = h - params["valley_depth"] * ridged_fbm(w_x, w_z, params["valley_freq"], 4, seed + 200)
    # 5. relief
    return h * params["relief_m"]
```

- [ ] **Step 4: Run to verify PASS**

```powershell
python -m pytest test_worldgen_proto.py -q
```
Expected: ALL pass, including `test_generate_no_tiling_autocorrelation` (no autocorr spike at any tiling period — PROVES the warped field doesn't repeat). If the autocorr test fails (a spike appears), that's a REAL finding — investigate whether a freq is accidentally periodic; do NOT relax the 0.5 threshold to pass.

- [ ] **Step 5: Commit**

```bash
git add tools/dem_pack/worldgen_proto.py tools/dem_pack/test_worldgen_proto.py
git commit -m "worldgen s1: generate(x,z,params) — warp+macro+ridges+valleys; deterministic/bounded/non-repeating"
```

---

## Task 3: Render the images (the deliverable for the owner's eye)

**Files:**
- Create: `tools/dem_pack/render_worldgen.py`

- [ ] **Step 1: Create the render script**

Create `tools/dem_pack/render_worldgen.py`:

```python
"""Render hillshaded PNGs of the worldgen prototype for the owner to judge by eye (render-first).
Writes to D:\\tmp\\. NOT a test — a runnable inspection tool. Run: python render_worldgen.py"""
import numpy as np
from PIL import Image
import worldgen_proto as wg

OUT = r"D:\tmp"

MOUNTAIN = {"relief_m": 1200.0, "octave_amps": [1.0,0.5,0.25,0.12,0.06,0.03],
            "ridge_strength": 0.9, "valley_depth": 0.5, "warp_amount": 2500.0,
            "base_freq": 1.0/3000.0, "ridge_freq": 1.0/1500.0, "valley_freq": 1.0/2500.0, "warp_freq": 1.0/8000.0}
PLAINS   = {"relief_m": 180.0, "octave_amps": [1.0,0.4,0.18,0.08,0.03,0.01],
            "ridge_strength": 0.05, "valley_depth": 0.15, "warp_amount": 1200.0,
            "base_freq": 1.0/4000.0, "ridge_freq": 1.0/1500.0, "valley_freq": 1.0/3000.0, "warp_freq": 1.0/9000.0}
BADLANDS = {"relief_m": 400.0, "octave_amps": [1.0,0.6,0.4,0.25,0.15,0.08],
            "ridge_strength": 0.4, "valley_depth": 0.9, "warp_amount": 1800.0,
            "base_freq": 1.0/2200.0, "ridge_freq": 1.0/900.0, "valley_freq": 1.0/700.0, "warp_freq": 1.0/6000.0}


def hillshade(z, exaggeration=1.0, az=315.0, alt=45.0):
    zn = (z - z.min()) / (np.ptp(z) + 1e-9)
    gy, gx = np.gradient(zn * 80.0 * exaggeration)
    slope = np.pi/2.0 - np.arctan(np.sqrt(gx*gx + gy*gy))
    aspect = np.arctan2(-gx, gy)
    azr = np.radians(360 - az + 90); altr = np.radians(alt)
    sh = np.sin(altr)*np.sin(slope) + np.cos(altr)*np.cos(slope)*np.cos(azr - aspect)
    return np.clip(sh, 0, 1)


def grid(n, span, ox=0.0, oz=0.0):
    ii = np.linspace(0, span, n)
    return np.meshgrid(ii + ox, ii + oz)


def save(name, sh):
    Image.fromarray((sh*255).astype(np.uint8), mode="L").save(rf"{OUT}\{name}.png")
    print(f"wrote {OUT}\\{name}.png")


def main():
    # 1. Each biome over a LARGE area (200 km) — judge contiguity + structure + no-repeat.
    for nm, p in [("mountain", MOUNTAIN), ("plains", PLAINS), ("badlands", BADLANDS)]:
        wx, wz = grid(1024, 200000.0)
        save(f"worldgen_{nm}_200km", hillshade(wg.generate(wx, wz, p, seed=7)))
    # 2. A close-up (10 km) of mountains — judge near-field detail.
    wx, wz = grid(1024, 10000.0, ox=120000.0, oz=80000.0)
    save("worldgen_mountain_10km", hillshade(wg.generate(wx, wz, MOUNTAIN, seed=7), exaggeration=2.0))
    # 3. A BIOME-TRANSITION strip: mountains (left) blending to plains (right) by linear param lerp,
    #    to eyeball that transitions are SEAMLESS (no hard line). (Slice-3 uses real grammar weights;
    #    this is a linear lerp stand-in just to SEE the blend.)
    n = 1024; span = 200000.0
    wx, wz = grid(n, span)
    t = np.linspace(0.0, 1.0, n).reshape(1, -1)       # 0=mountain .. 1=plains across X
    blended = {k: (np.array(MOUNTAIN[k])*(1-t) + np.array(PLAINS[k])*t) if k == "octave_amps"
               else (MOUNTAIN[k] if k.endswith("freq") else MOUNTAIN[k]*(1-t) + PLAINS[k]*t)
               for k in MOUNTAIN}
    # octave_amps blends per-column; build height column-aware (simple: generate both, lerp result for the strip view)
    hm = wg.generate(wx, wz, MOUNTAIN, seed=7); hp = wg.generate(wx, wz, PLAINS, seed=7)
    strip = hm*(1-t) + hp*t                            # param-lerp approximated by result-lerp for the eyeball
    save("worldgen_transition_strip", hillshade(strip, exaggeration=1.5))


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Run the render script**

```powershell
cd tools/dem_pack
python render_worldgen.py
```
Expected: writes 5 PNGs to `D:\tmp\` (`worldgen_mountain_200km`, `worldgen_plains_200km`, `worldgen_badlands_200km`, `worldgen_mountain_10km`, `worldgen_transition_strip`). No errors.

- [ ] **Step 3: Commit the render script**

```bash
git add tools/dem_pack/render_worldgen.py
git commit -m "worldgen s1: render_worldgen.py — hillshaded PNGs (large area + closeup + transition) for owner eye"
```

---

## Task 4: Full test run + owner eye verdict + STATUS

- [ ] **Step 1: Run the whole dem_pack suite (nothing regressed)**

```powershell
cd tools/dem_pack
python -m pytest -q
```
Expected: all green (test_worldgen_proto.py + test_spectral.py + test_dem_pack_lib.py). Record the count.

- [ ] **Step 2: OWNER eye verdict (the real acceptance — render-first).** Open the 5 PNGs in `D:\tmp\`
  (the controller should `Start-Process` them + send them to the owner). The owner judges:
  - **Contiguity:** does each 200 km image read as ONE continuous landmass (no chunks/squares/lines)?
  - **Structure:** real ridgelines + valleys/drainage (not blobs, not pure noise)?
  - **No repetition:** nothing visibly tiles/repeats across the 200 km?
  - **Near-field (10 km):** does the closeup hold up (toward the 1-10m-detail goal)?
  - **Transition:** does mountain→plains blend seamlessly (no hard line)?
  Record the verdict verbatim. **This is the make-or-break — if it doesn't read as Google-Maps-ish terrain,
  that's the cheap offline finding (like spectral), and we refine the toolkit/params BEFORE any runtime.**

- [ ] **Step 3: Update STATUS.md** — add a "Worldgen Core Slice 1" entry: the warped-noise generator
  prototype landed OFFLINE in `tools/dem_pack/worldgen_proto.py` (warp+macro+ridges+valleys, deterministic/
  bounded/non-repeating — autocorr gate green at the old tiling periods); rendered hillshades for 3 biomes
  (200 km + 10 km closeup + transition strip); the OWNER eye verdict (verbatim). Note explicitly: this
  proves the generator OFFLINE; the look judgment is the owner's; NEXT = S2 (biome distillation) IF the
  owner accepts the look, else refine the toolkit/params first. No Rust/GLSL/render touched.

- [ ] **Step 4: Commit STATUS.**

```bash
git add docs/plans/STATUS.md
git commit -m "worldgen s1: STATUS — generator prototype rendered offline, owner eye verdict recorded"
```

---

## Self-review notes (planner)

- **Spec coverage (Slice 1):** spec §3 generator (warp/macro/ridges/valleys) → Tasks 1-2; spec §7 non-
  repetition gate → Task 2 autocorr test; spec §7 render-images-first → Task 3; spec §8 S1 "render-first,
  owner judges, retire look-risk offline" → Task 4. Biome distillation (§4) is S2; Rust/GLSL (§3 runtime),
  grammar blend (§5) are S3-S5, correctly absent.
- **The core risk is retired in Task 4** (does warped-noise LOOK contiguous/structured) — OFFLINE, owner-
  judged, before any runtime. The "if it doesn't read right, refine before runtime" guard is explicit.
- **No engine touch:** entirely `tools/dem_pack/`, pure numpy/PIL/pytest. Cannot break the running engine.
- **Placeholder note:** the biome param VALUES (MOUNTAIN/PLAINS/BADLANDS dicts) are HAND-TUNED starting
  guesses for the prototype (the spec says S1 hand-tunes; S2 distills real ones) — concrete numbers, not
  TBD; they get refined by the owner's eye verdict + S2 distillation.
- **Name consistency:** `value_noise`, `fbm`, `ridged_fbm`, `domain_warp`, `generate`, `N_OCTAVES`,
  the BiomeParams dict keys (`relief_m`, `octave_amps`, `ridge_strength`, `valley_depth`, `warp_amount`,
  `base_freq`, `ridge_freq`, `valley_freq`, `warp_freq`) used identically across worldgen_proto.py +
  both consumers (test + render).
- **The transition strip** uses a result-lerp approximation (generate both, lerp) just for the eyeball — the
  REAL grammar param-blend is S3-S5; noted in the code comment so it's not mistaken for the real blend.
- **Honesty:** the autocorr gate proves NO tiling-period repeat (a real metric for the owner's complaint);
  it does NOT prove "looks like Google Maps" — that's the owner's eye (Task 4). Stated in STATUS.
