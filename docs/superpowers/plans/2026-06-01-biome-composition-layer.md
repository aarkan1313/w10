# Biome Composition Layer Implementation Plan (Fork B)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a deterministic, seam-exact biome composition layer where each biome KEEPS its own composition recipe (Fork B), all recipes are made seam-safe, and a tunable cross-recipe blend composes neighboring biomes at boundaries — proven offline in Python, render-first, before any Rust port.

**Architecture:** Fork B (decided by render-first probe — `keeper_v2` cannot express oriented-ridge mountains as presets, so "one engine" is dead). Each biome synth becomes a registered **recipe** behind one interface; per-window `zscore`/`norm01` are replaced with data-independent `affine_remap` (the v2 seam lesson); a `compose_biomes(window, grammar_weights, registry)` function blends recipe outputs with a tunable `blend_mode` (`height_favored` primary, `field` fallback). All offline Python; no Rust/GLSL (Slice 3).

**Tech Stack:** Python 3.12, numpy, scipy.ndimage, matplotlib (render review), pytest. All work in `tools/dem_pack/`. Run pytest **from the repo root** `D:/workflows/worldgen10` (recipe/fixture paths are repo-root-relative — running from inside `tools/dem_pack` breaks them; this bit two prior sessions).

**Spec:** `docs/superpowers/specs/2026-06-01-worldgen-biome-composition-layer-design.md` (read the two `★` REVISED banners at the top of §1 — they supersede the original "one engine / param-blend" framing with Fork B + the locked `blend_mode` decision).

**Commit convention (project rule):** stage files BY NAME (never `git add -A` — the repo has pre-existing dirty files that are NOT ours). Commit trailer: `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`. The owner pushes; do not push.

---

## File Structure

- Create: `tools/dem_pack/biome_compose.py` — the composition layer: `BlendConfig`, `compose_biomes`, the two blend modes (`_blend_height_favored`, `_blend_field`). One responsibility: turn (per-recipe height fields + a weight field) into one composed height. Knows nothing about which recipes exist.
- Create: `tools/dem_pack/biome_registry.py` — the recipe registry: `Recipe` (name + a `generate(wx,wz,seed,feature_span_m)->height` callable), `REGISTRY` dict, `get_recipe(name)`. One responsibility: name → recipe callable. The adapter that wraps each `*_synthesis.generate` into the uniform recipe signature (dropping the diagnostic masks, returning just height) lives here.
- Create: `tools/dem_pack/test_biome_compose.py` — tests for the compose layer (blend math, determinism, weight-field handling).
- Create: `tools/dem_pack/test_biome_registry.py` — tests for the registry (lookup, recipe signature uniformity, determinism).
- Modify (Slice C, seam-safe pass): `tools/dem_pack/mountain_synthesis.py`, `grassland_synthesis.py`, and the other 11 `*_synthesis.py` — replace the FINAL output `zscore(...)`/`norm01(...)` with a seam-safe `affine_remap`-style data-independent rescale. ONE biome per task.
- Create: `tools/dem_pack/probe_biome_blend_clash.py` — Slice A clash-pair probe (mountain↔desert dunes), throwaway-but-kept.
- Create: `tools/dem_pack/render_biome_compose.py` — render a composed multi-biome transition for owner review (the look gate).

Existing reference (read, do not modify): `keeper_v2.py` (`affine_remap` at line 28 — the seam-safe pattern), `probe_biome_blend.py` + `probe_v2_as_mountain.py` (the probes that decided Fork B), `geography_engine.py` (`grid` at line 54).

---

## Slice A — Clash-pair blend probe (de-risk the one open question first)

The mountain↔grassland probe passed because those recipes share primitives. Per spec pillar-4 caveat, validate the blend on a STRUCTURALLY CLASHING pair (mountain↔desert dunes) BEFORE building the layer. If `height_favored` fails here, we adjust the mechanism now, not after converting 13 biomes.

### Task A1: Clash-pair probe render

**Files:**
- Create: `tools/dem_pack/probe_biome_blend_clash.py`
- Output: `D:/tmp/wg10_biome_compose/probe_biome_blend_clash.png`

- [ ] **Step 1: Write the probe** (adapted from `probe_biome_blend.py`; swap grassland→desert, render the 3 blend modes)

```python
r"""PROBE (Slice A de-risk): does the height_favored blend survive a CLASHING biome pair?

mountain<->desert dunes — dune-train directionality vs ridge orientation is the stress test the
gentle mountain<->grassland probe did not cover. Renders field / feathered / height_favored side
by side for the owner's eye. If height_favored ghosts or fights here, adjust before building.

Run:   python tools/dem_pack/probe_biome_blend_clash.py
Writes: D:/tmp/wg10_biome_compose/probe_biome_blend_clash.png
"""
from __future__ import annotations
from pathlib import Path
import numpy as np
import matplotlib; matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.colors import LightSource
from scipy.ndimage import gaussian_filter
import geography_engine as geo
import mountain_synthesis as mountain
import desert_synthesis as desert

OUT = Path("D:/tmp/wg10_biome_compose/probe_biome_blend_clash.png")
N, SEED, SPAN_M, FEATURE_SPAN_M, BAND_FRAC = 320, 133, 60_000.0, 90_000.0, 0.16

def _smoothstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0 + 1e-9), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)

def _shade(ax, h, title):
    h = np.asarray(h, float); hn = (h - h.min()) / (np.ptp(h) + 1e-9)
    rgb = LightSource(azdeg=315, altdeg=45).shade(hn, cmap=plt.cm.terrain, vert_exag=2.0, blend_mode="soft")
    ax.imshow(rgb); ax.set_title(title, fontsize=9); ax.axis("off")
    ax.axvline(h.shape[1] * 0.5, color="red", lw=0.6, alpha=0.5)

def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    wx, wz = geo.grid(N, SPAN_M, ox=60_000.0, oz=36_000.0)
    mtn = np.asarray(mountain.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M)["height"], float)
    des = np.asarray(desert.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M)["height"], float)
    mtn = (mtn - mtn.mean()) / (mtn.std() + 1e-9)
    des = (des - des.mean()) / (des.std() + 1e-9)
    u = np.linspace(0.0, 1.0, N)[None, :].repeat(N, axis=0)
    w_mtn = 1.0 - _smoothstep(0.5 - BAND_FRAC, 0.5 + BAND_FRAC, u)
    field = w_mtn * mtn + (1.0 - w_mtn) * des
    thin = 0.045
    w_thin = 1.0 - _smoothstep(0.5 - thin, 0.5 + thin, u)
    feathered = w_thin * mtn + (1.0 - w_thin) * des
    relief_m = np.abs(mtn - gaussian_filter(mtn, sigma=6.0))
    relief_d = np.abs(des - gaussian_filter(des, sigma=6.0))
    favor = relief_m / (relief_m + relief_d + 1e-9)
    w_fav = np.clip(w_mtn + (favor - 0.5) * 0.9 * (1.0 - np.abs(2 * w_mtn - 1)), 0.0, 1.0)
    height_fav = w_fav * mtn + (1.0 - w_fav) * des
    fig, ax = plt.subplots(2, 3, figsize=(16, 11))
    _shade(ax[0, 0], mtn, "mountain"); _shade(ax[0, 1], des, "desert dunes"); _shade(ax[0, 2], w_mtn, "weight (white=mtn)")
    _shade(ax[1, 0], field, "1. field-blend"); _shade(ax[1, 1], feathered, "2. feathered"); _shade(ax[1, 2], height_fav, "3. height-favored")
    fig.suptitle("PROBE Slice A: CLASH pair mountain<->desert dunes. Red = boundary.", fontsize=12)
    fig.tight_layout(); fig.savefig(OUT, dpi=92); print(f"wrote {OUT}")

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Run it**

Run (from repo root): `python tools/dem_pack/probe_biome_blend_clash.py`
Expected: `wrote D:\tmp\wg10_biome_compose\probe_biome_blend_clash.png`

- [ ] **Step 3: OWNER GATE — show the render, get a verdict.**

Present the PNG. Ask: does `height_favored` (panel 3) read as a believable mountain→dune transition, or does it ghost/fight? **If acceptable → proceed to Slice B unchanged. If not → STOP; the blend mechanism needs adjustment (revise the spec's `blend_mode` decision before building the layer).** This is a hard checkpoint — do not build Slice B on an unvalidated blend.

- [ ] **Step 4: Commit the probe**

```bash
git add tools/dem_pack/probe_biome_blend_clash.py
git commit -m "probe: clash-pair (mountain<->dunes) blend de-risk for biome composition"
```

---

## Slice B — The composition layer (compose + registry + blend modes)

Build the layer with FAKE recipes first (synthetic height fields), so the blend math is tested in isolation before touching real synths.

### Task B1: `BlendConfig` + `_blend_field` (the simple fallback mode)

**Files:**
- Create: `tools/dem_pack/biome_compose.py`
- Test: `tools/dem_pack/test_biome_compose.py`

- [ ] **Step 1: Write the failing test**

```python
import numpy as np
import biome_compose as bc

def test_field_blend_is_weighted_lerp():
    a = np.full((4, 4), 2.0)
    b = np.full((4, 4), 0.0)
    w = np.full((4, 4), 0.25)          # 0.25 weight on 'a'
    cfg = bc.BlendConfig(mode="field")
    out = bc._blend_field(a, b, w)
    assert np.allclose(out, 0.25 * 2.0 + 0.75 * 0.0)  # = 0.5 everywhere
```

- [ ] **Step 2: Run it, verify it fails**

Run (repo root): `python -m pytest tools/dem_pack/test_biome_compose.py::test_field_blend_is_weighted_lerp -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'biome_compose'`

- [ ] **Step 3: Write minimal implementation**

```python
r"""Biome composition layer (Fork B): blend the OUTPUTS of distinct biome recipes at boundaries.

Knows nothing about which recipes exist — it takes per-recipe height fields + a weight field and
composes one height. blend_mode is tunable: 'height_favored' (primary; bias toward the locally
higher-relief recipe so structure stays crisp through the band) or 'field' (cheap lerp fallback).
Decided by render-first probes — see the spec's BLEND PROBE RESULT banner.
"""
from __future__ import annotations
from dataclasses import dataclass
import numpy as np
from scipy.ndimage import gaussian_filter


@dataclass(frozen=True)
class BlendConfig:
    mode: str = "height_favored"     # 'height_favored' | 'field'
    relief_sigma_px: float = 6.0     # blur radius for the local-relief proxy (height_favored)
    favor_strength: float = 2.0      # how hard to bias toward the higher-relief recipe in the band
    # NOTE (validated via clash tuning sweep, D:/tmp/wg10_biome_compose/probe_biome_blend_clash_tune.png):
    # favor_strength=2.0 + a NARROW transition band (band_frac~=0.05, owned by the grammar/weight-field, NOT
    # this config) gives the crispest NATURAL mountain<->dune transition without a hard-seam look. AAA goal:
    # natural transitions, not visible seams. Band width is per-PAIR tunable (wider for gentle pairs like
    # mountain<->grassland, narrow for clashing pairs) and lives in how the weight field is built.


def _blend_field(a: np.ndarray, b: np.ndarray, w_a: np.ndarray) -> np.ndarray:
    """Plain weighted lerp: w_a on a, (1-w_a) on b."""
    a = np.asarray(a, dtype=np.float64); b = np.asarray(b, dtype=np.float64)
    w = np.asarray(w_a, dtype=np.float64)
    return w * a + (1.0 - w) * b
```

- [ ] **Step 4: Run it, verify it passes**

Run (repo root): `python -m pytest tools/dem_pack/test_biome_compose.py::test_field_blend_is_weighted_lerp -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add tools/dem_pack/biome_compose.py tools/dem_pack/test_biome_compose.py
git commit -m "feat(biome-compose): BlendConfig + field-blend fallback mode"
```

### Task B2: `_blend_height_favored` (the primary mode)

**Files:**
- Modify: `tools/dem_pack/biome_compose.py`
- Test: `tools/dem_pack/test_biome_compose.py`

- [ ] **Step 1: Write the failing test**

```python
def test_height_favored_biases_toward_higher_relief_in_band():
    # 'a' has strong local relief (a stripe), 'b' is flat. In the transition band (w_a=0.5),
    # height_favored should pull the result toward 'a' more than a plain 50/50 lerp would.
    a = np.zeros((8, 8)); a[:, ::2] = 3.0       # high-frequency stripes = strong local relief
    b = np.zeros((8, 8))                        # flat = no relief
    w = np.full((8, 8), 0.5)                    # neutral band weight
    cfg = bc.BlendConfig(mode="height_favored")
    favored = bc._blend_height_favored(a, b, w, cfg)
    plain = bc._blend_field(a, b, w)
    # favored result should be closer to 'a' (higher mean abs) than the plain lerp where a has relief
    assert float(np.mean(np.abs(favored))) > float(np.mean(np.abs(plain)))
```

- [ ] **Step 2: Run it, verify it fails**

Run (repo root): `python -m pytest tools/dem_pack/test_biome_compose.py::test_height_favored_biases_toward_higher_relief_in_band -v`
Expected: FAIL — `AttributeError: module 'biome_compose' has no attribute '_blend_height_favored'`

- [ ] **Step 3: Write minimal implementation** (append to `biome_compose.py`)

```python
def _blend_height_favored(a: np.ndarray, b: np.ndarray, w_a: np.ndarray, cfg: "BlendConfig") -> np.ndarray:
    """Bias the blend weight toward whichever recipe has stronger LOCAL relief inside the
    transition band, so structured terrain (e.g. mountain ridges) is not ghost-flattened into a
    low mound by a neutral average. Outside the band (w_a at 0 or 1) this reduces to the field blend."""
    a = np.asarray(a, dtype=np.float64); b = np.asarray(b, dtype=np.float64)
    w = np.asarray(w_a, dtype=np.float64)
    relief_a = np.abs(a - gaussian_filter(a, sigma=cfg.relief_sigma_px))
    relief_b = np.abs(b - gaussian_filter(b, sigma=cfg.relief_sigma_px))
    favor = relief_a / (relief_a + relief_b + 1e-9)            # ~1 where a has the structure
    band = 1.0 - np.abs(2.0 * w - 1.0)                         # 1 at band center, 0 at the pure ends
    w_adj = np.clip(w + (favor - 0.5) * cfg.favor_strength * band, 0.0, 1.0)
    return w_adj * a + (1.0 - w_adj) * b
```

- [ ] **Step 4: Run it, verify it passes**

Run (repo root): `python -m pytest tools/dem_pack/test_biome_compose.py::test_height_favored_biases_toward_higher_relief_in_band -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add tools/dem_pack/biome_compose.py tools/dem_pack/test_biome_compose.py
git commit -m "feat(biome-compose): height_favored primary blend mode"
```

### Task B3: `compose_biomes` dispatcher (2-recipe blend via a weight field)

**Files:**
- Modify: `tools/dem_pack/biome_compose.py`
- Test: `tools/dem_pack/test_biome_compose.py`

- [ ] **Step 1: Write the failing test**

```python
def test_compose_biomes_two_recipes_reduces_to_pure_at_ends():
    a = np.full((4, 4), 5.0); b = np.full((4, 4), 1.0)
    w_a = np.array([[1.0, 1.0, 0.0, 0.0]] * 4)   # left 2 cols pure a, right 2 cols pure b
    cfg = bc.BlendConfig(mode="height_favored")
    out = bc.compose_biomes([a, b], [w_a, 1.0 - w_a], cfg)
    assert np.allclose(out[:, 0], 5.0)            # pure a
    assert np.allclose(out[:, -1], 1.0)           # pure b

def test_compose_biomes_determinism():
    a = np.full((4, 4), 5.0); b = np.full((4, 4), 1.0)
    w_a = np.full((4, 4), 0.5)
    cfg = bc.BlendConfig()
    o1 = bc.compose_biomes([a, b], [w_a, 1.0 - w_a], cfg)
    o2 = bc.compose_biomes([a, b], [w_a, 1.0 - w_a], cfg)
    assert np.array_equal(o1, o2)
```

- [ ] **Step 2: Run it, verify it fails**

Run (repo root): `python -m pytest tools/dem_pack/test_biome_compose.py -k compose_biomes -v`
Expected: FAIL — `AttributeError: module 'biome_compose' has no attribute 'compose_biomes'`

- [ ] **Step 3: Write minimal implementation** (append to `biome_compose.py`)

```python
def compose_biomes(fields: list[np.ndarray], weights: list[np.ndarray], cfg: "BlendConfig") -> np.ndarray:
    """Compose N per-recipe height fields by their per-pixel weights into one height.

    Weights are expected to be a partition of unity (sum to ~1 per pixel) from the grammar.
    For N=2 we use the pairwise blend mode (height_favored | field). For N>2 we fold pairwise
    in weight order (the dominant pair blended first), which keeps the 2-recipe behavior the
    probes validated and degrades gracefully where 3+ biomes meet (rare; a triple point).
    """
    if len(fields) != len(weights):
        raise ValueError(f"fields/weights length mismatch: {len(fields)} vs {len(weights)}")
    if not fields:
        raise ValueError("compose_biomes requires at least one recipe field")
    fields = [np.asarray(f, dtype=np.float64) for f in fields]
    weights = [np.asarray(w, dtype=np.float64) for w in weights]
    if len(fields) == 1:
        return fields[0]
    # accumulate: start from recipe 0, fold each next recipe in by its relative weight
    acc = fields[0]
    acc_w = weights[0].copy()
    for f, w in zip(fields[1:], weights[1:]):
        denom = acc_w + w + 1e-12
        w_acc = acc_w / denom                       # weight on the accumulator vs the new recipe
        if cfg.mode == "field":
            acc = _blend_field(acc, f, w_acc)
        else:
            acc = _blend_height_favored(acc, f, w_acc, cfg)
        acc_w = acc_w + w
    return acc
```

- [ ] **Step 4: Run it, verify it passes**

Run (repo root): `python -m pytest tools/dem_pack/test_biome_compose.py -k compose_biomes -v`
Expected: PASS (both tests)

- [ ] **Step 5: Commit**

```bash
git add tools/dem_pack/biome_compose.py tools/dem_pack/test_biome_compose.py
git commit -m "feat(biome-compose): compose_biomes N-recipe dispatcher (pairwise fold)"
```

### Task B4: Biome registry (name → uniform recipe callable)

**Files:**
- Create: `tools/dem_pack/biome_registry.py`
- Test: `tools/dem_pack/test_biome_registry.py`

- [ ] **Step 1: Write the failing test**

```python
import numpy as np
import biome_registry as br
import geography_engine as geo

def test_registry_has_mountain_and_grassland():
    assert "mountain" in br.REGISTRY
    assert "grassland" in br.REGISTRY

def test_recipe_returns_bare_height_array():
    wx, wz = geo.grid(48, 60_000.0, ox=60_000.0, oz=36_000.0)
    h = br.get_recipe("mountain").generate(wx, wz, seed=133, feature_span_m=90_000.0)
    assert isinstance(h, np.ndarray)
    assert h.shape == (48, 48)

def test_recipe_is_deterministic():
    wx, wz = geo.grid(32, 60_000.0, ox=60_000.0, oz=36_000.0)
    r = br.get_recipe("grassland")
    a = r.generate(wx, wz, seed=7, feature_span_m=90_000.0)
    b = r.generate(wx, wz, seed=7, feature_span_m=90_000.0)
    assert np.array_equal(a, b)
```

- [ ] **Step 2: Run it, verify it fails**

Run (repo root): `python -m pytest tools/dem_pack/test_biome_registry.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'biome_registry'`

- [ ] **Step 3: Write minimal implementation**

```python
r"""Biome recipe registry (Fork B): name -> a uniform recipe callable.

Each biome keeps its own *_synthesis.generate recipe; this adapts them to ONE signature
generate(wx, wz, seed, feature_span_m) -> height ndarray (dropping the diagnostic masks the
synths return for review). The composition layer (biome_compose) consumes these; it never
imports the synths directly. Adding a biome = one REGISTRY entry, no compose-layer change.
"""
from __future__ import annotations
from dataclasses import dataclass
from typing import Callable
import numpy as np

import mountain_synthesis as mountain
import grassland_synthesis as grassland
import desert_synthesis as desert
import glacial_synthesis as glacial
import karst_synthesis as karst
import volcanic_synthesis as volcanic
import temperate_synthesis as temperate
import tundra_synthesis as tundra
import rainforest_synthesis as rainforest
import coast_synthesis as coast
import wetland_synthesis as wetland


@dataclass(frozen=True)
class Recipe:
    name: str
    generate: Callable[..., np.ndarray]


def _adapt(mod) -> Callable[..., np.ndarray]:
    """Wrap a *_synthesis module's generate so it returns the bare height array."""
    def gen(wx, wz, seed: int = 0, feature_span_m: float | None = None) -> np.ndarray:
        out = mod.generate(wx, wz, seed=int(seed), feature_span_m=feature_span_m)
        return np.asarray(out["height"], dtype=np.float64)
    return gen


REGISTRY: dict[str, Recipe] = {
    name: Recipe(name, _adapt(mod))
    for name, mod in (
        ("mountain", mountain), ("grassland", grassland), ("desert", desert),
        ("glacial", glacial), ("karst", karst), ("volcanic", volcanic),
        ("temperate", temperate), ("tundra", tundra), ("rainforest", rainforest),
        ("coast", coast), ("wetland", wetland),
    )
}


def get_recipe(name: str) -> Recipe:
    if name not in REGISTRY:
        raise KeyError(f"unknown biome recipe {name!r}; known: {sorted(REGISTRY)}")
    return REGISTRY[name]
```

> NOTE: the registry lists 11 biomes (the `*_synthesis.py` modules that exist). "badlands" is generated via `biome_synthesis.generate_family_height` (no standalone module) and "rough-highlands" is `keeper_v2` — both are added in Slice D once the 11 standalone synths are proven, since they have different call shapes.

- [ ] **Step 4: Run it, verify it passes**

Run (repo root): `python -m pytest tools/dem_pack/test_biome_registry.py -v`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add tools/dem_pack/biome_registry.py tools/dem_pack/test_biome_registry.py
git commit -m "feat(biome-registry): name->uniform recipe adapter for 11 synth biomes"
```

---

## Slice C — Seam-safety pass (replace per-window normalization, one biome at a time)

Each synth ends with a per-window `zscore`/`norm01` (data-DEPENDENT — differs between adjacent windows → broken seams). Replace the FINAL normalization with a data-independent rescale so adjacent windows share borders bit-exactly. Do ONE biome per task; gate each with an adjacent-window seam test. Start with the two probe biomes (mountain, grassland), then the other 9.

### Task C1: Mountain seam-safety + seam test

**Files:**
- Modify: `tools/dem_pack/mountain_synthesis.py` (the final `zscore(...)` at the end of `generate`, ~line 221)
- Test: `tools/dem_pack/test_biome_registry.py` (add a seam test)

- [ ] **Step 1: Write the failing seam test**

```python
def test_mountain_recipe_seam_exact_adjacent_windows():
    # Two adjacent 65x65 windows sharing a vertical border, same world coords on the seam.
    import geography_engine as geo
    span = 25_600.0; n = 65; step = span * (n - 1) / n  # window B starts one window-width right
    wxa, wza = geo.grid(n, span, ox=60_000.0, oz=36_000.0)
    wxb, wzb = geo.grid(n, span, ox=60_000.0 + span, oz=36_000.0)
    r = br.get_recipe("mountain")
    a = r.generate(wxa, wza, seed=133, feature_span_m=90_000.0)
    b = r.generate(wxb, wzb, seed=133, feature_span_m=90_000.0)
    # NOTE: this test as written checks INDEPENDENT-window seam behavior. A standalone synth that
    # self-normalizes per window will FAIL until the final normalization is made data-independent.
    border_delta = float(np.max(np.abs(a[:, -1] - b[:, 0])))
    assert border_delta < 1e-9, f"mountain not seam-safe: border delta {border_delta}"
```

> IMPORTANT for the implementer: the two windows above do NOT actually share a coordinate column unless `ox_b == ox_a + span*(n-1)/n` (texel-corner sharing). Use the EXACT shared-coordinate construction the repo already uses for seam tests — copy the window builder pattern from `test_keeper_v2.py::test_*_seam*` (it uses `ex._window_spec` + `win.build_skeleton_window` with `ox + CHUNK_SPAN_M`). For a standalone synth (no skeleton window), construct two grids whose adjacent columns are the SAME world x: `wxb = wxa + (span - span/(n-1))` so column `-1` of A equals column `0` of B. Verify the shared column matches in `wx` before asserting on height.

- [ ] **Step 2: Run it, verify it fails**

Run (repo root): `python -m pytest tools/dem_pack/test_biome_registry.py::test_mountain_recipe_seam_exact_adjacent_windows -v`
Expected: FAIL — border delta is large (per-window `zscore` differs between windows).

- [ ] **Step 3: Make the final normalization data-independent**

In `mountain_synthesis.py`, the final line of `generate` is:
```python
    height = zscore(0.74 * height + 0.26 * gaussian_filter(height, sigma=1.20))
```
Replace the per-window `zscore` with a data-independent affine rescale (fixed center/scale constants, NOT array statistics). Add a module-level helper mirroring `keeper_v2.affine_remap`:
```python
def _affine_remap(field, center: float, scale: float):
    """Data-independent rescale (replaces zscore at the seam): same center/scale every window
    => adjacent windows share borders bit-exactly. center/scale are tunable constants."""
    return (np.asarray(field, dtype=np.float64) - float(center)) * float(scale)
```
and change the final line to (the gaussian_filter is window-local — it reads neighbors within the window; for true seam-exactness it must operate on an APRON or be dropped at the seam. For this standalone-synth pass, drop the final blur term and rescale the raw composed height):
```python
    height = _affine_remap(height, center=MOUNTAIN_REMAP_CENTER, scale=MOUNTAIN_REMAP_SCALE)
```
Add tunable constants near the top of the module:
```python
MOUNTAIN_REMAP_CENTER = 0.0    # tune so the typical mid-height maps near 0 (was the zscore mean)
MOUNTAIN_REMAP_SCALE = 1.0     # tune so typical relief matches the prior zscore std-1 range
```

> The `gaussian_filter(height, sigma=1.20)` final-smoothing term is window-local and breaks seams at the border. Removing it changes the look slightly (a touch more fine detail). If the owner wants the smoothing back, it must be reintroduced as an APRON-cropped blur (copy `keeper_v2.apron_blur_crop_full`) in a later task — out of scope for this seam-safety pass.

- [ ] **Step 4: Run it, verify it passes**

Run (repo root): `python -m pytest tools/dem_pack/test_biome_registry.py::test_mountain_recipe_seam_exact_adjacent_windows -v`
Expected: PASS (border delta < 1e-9)

- [ ] **Step 5: Run the existing mountain synth tests, confirm still green**

Run (repo root): `python -m pytest tools/dem_pack/test_mountain_synthesis.py -v`
Expected: PASS. If any test asserted on the old zscore range, update its expected values (the rescale changed the tone, not the structure).

- [ ] **Step 6: Commit**

```bash
git add tools/dem_pack/mountain_synthesis.py tools/dem_pack/test_biome_registry.py tools/dem_pack/test_mountain_synthesis.py
git commit -m "feat(mountain): seam-safe final rescale (data-independent affine, drop window-local blur)"
```

### Task C2: Grassland seam-safety + seam test

**Files:**
- Modify: `tools/dem_pack/grassland_synthesis.py` (final `zscore`/`norm01` of `generate`)
- Test: `tools/dem_pack/test_biome_registry.py`

- [ ] **Step 1: Write the failing seam test** (same construction as C1, recipe `"grassland"`)

```python
def test_grassland_recipe_seam_exact_adjacent_windows():
    import geography_engine as geo
    span = 25_600.0; n = 65
    wxa, wza = geo.grid(n, span, ox=60_000.0, oz=36_000.0)
    wxb, wzb = geo.grid(n, span, ox=60_000.0 + span - span / (n - 1), oz=36_000.0)
    assert np.allclose(wxa[:, -1], wxb[:, 0])   # shared column same world x
    r = br.get_recipe("grassland")
    a = r.generate(wxa, wza, seed=133, feature_span_m=90_000.0)
    b = r.generate(wxb, wzb, seed=133, feature_span_m=90_000.0)
    border_delta = float(np.max(np.abs(a[:, -1] - b[:, 0])))
    assert border_delta < 1e-9, f"grassland not seam-safe: border delta {border_delta}"
```

- [ ] **Step 2: Run it, verify it fails**

Run (repo root): `python -m pytest tools/dem_pack/test_biome_registry.py::test_grassland_recipe_seam_exact_adjacent_windows -v`
Expected: FAIL — border delta large.

- [ ] **Step 3: Make grassland's final normalization data-independent** — same pattern as C1: add `_affine_remap` + `GRASSLAND_REMAP_CENTER`/`GRASSLAND_REMAP_SCALE`, replace the final `zscore`/`norm01`, drop or apron-ize any window-local final blur.

- [ ] **Step 4: Run it, verify it passes**

Run (repo root): `python -m pytest tools/dem_pack/test_biome_registry.py::test_grassland_recipe_seam_exact_adjacent_windows -v`
Expected: PASS

- [ ] **Step 5: Existing grassland tests still green**

Run (repo root): `python -m pytest tools/dem_pack/test_grassland_synthesis.py -v`
Expected: PASS (update expected values if they asserted the old normalized range).

- [ ] **Step 6: Commit**

```bash
git add tools/dem_pack/grassland_synthesis.py tools/dem_pack/test_biome_registry.py tools/dem_pack/test_grassland_synthesis.py
git commit -m "feat(grassland): seam-safe final rescale (data-independent affine)"
```

### Tasks C3–C11: remaining 9 biomes (desert, glacial, karst, volcanic, temperate, tundra, rainforest, coast, wetland)

Each is the SAME 6-step pattern as C2 (seam test → fails → `_affine_remap` + remap constants → passes → existing synth tests green → commit). Repeat per biome. ONE biome per task, ONE commit per biome.

> Do not batch these. Each biome's synth has its own final-normalization shape and may have biome-specific window-local terms (e.g. a `gaussian_filter` pass, a `norm01` inside a mask). Read the synth's final ~10 lines, replace the data-dependent normalization, drop/apron-ize window-local blurs, gate with the adjacent-window seam test, keep the existing per-biome tests green. Commit message per biome: `feat(<biome>): seam-safe final rescale`.

---

## Slice D — Whole-world compose + owner review (the look gate)

### Task D1: Render a grammar-driven multi-biome compose

**Files:**
- Create: `tools/dem_pack/render_biome_compose.py`
- Output: `D:/tmp/wg10_biome_compose/biome_compose_world.png`

- [ ] **Step 1: Write the renderer** (uses `biome_registry` + `biome_compose`; a simple smooth grammar-weight field placing 3–4 seam-safe biomes across one world, rendered shaded for the owner)

```python
r"""Render a grammar-driven multi-biome compose for owner review (the Fork-B look gate).

Places a few SEAM-SAFE biome recipes across one world via a smooth weight field (a stand-in for
the real grammar's biome weights), composes them with biome_compose.compose_biomes, and shades
the result. This is the owner-eye acceptance render for the composition layer.

Run:   python tools/dem_pack/render_biome_compose.py
Writes: D:/tmp/wg10_biome_compose/biome_compose_world.png
"""
from __future__ import annotations
from pathlib import Path
import numpy as np
import matplotlib; matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.colors import LightSource
import geography_engine as geo
import biome_registry as br
import biome_compose as bc

OUT = Path("D:/tmp/wg10_biome_compose/biome_compose_world.png")
N, SEED, SPAN_M, FEATURE_SPAN_M = 384, 133, 90_000.0, 90_000.0
BIOMES = ["mountain", "grassland", "desert", "glacial"]   # 4 quadrants, smoothly blended

def _quadrant_weights(n):
    u = np.linspace(0, 1, n)[None, :].repeat(n, 0)
    v = np.linspace(0, 1, n)[:, None].repeat(n, 1)
    def ss(e0, e1, x):
        t = np.clip((x - e0) / (e1 - e0 + 1e-9), 0, 1); return t * t * (3 - 2 * t)
    wl = 1 - ss(0.4, 0.6, u); wt = 1 - ss(0.4, 0.6, v)
    w = [wl * wt, (1 - wl) * wt, wl * (1 - wt), (1 - wl) * (1 - wt)]  # TL TR BL BR
    s = sum(w) + 1e-9
    return [x / s for x in w]

def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    wx, wz = geo.grid(N, SPAN_M, ox=60_000.0, oz=36_000.0)
    fields = []
    for name in BIOMES:
        h = br.get_recipe(name).generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M)
        fields.append((h - h.mean()) / (h.std() + 1e-9))   # comparable scale for review
    weights = _quadrant_weights(N)
    composed = bc.compose_biomes(fields, weights, bc.BlendConfig(mode="height_favored"))
    hn = (composed - composed.min()) / (np.ptp(composed) + 1e-9)
    rgb = LightSource(azdeg=315, altdeg=45).shade(hn, cmap=plt.cm.terrain, vert_exag=2.0, blend_mode="soft")
    fig, ax = plt.subplots(figsize=(11, 11))
    ax.imshow(rgb); ax.axis("off")
    ax.set_title(f"Biome compose (height_favored): {', '.join(BIOMES)}", fontsize=11)
    fig.tight_layout(); fig.savefig(OUT, dpi=100); print(f"wrote {OUT}")

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Run it**

Run (repo root): `python tools/dem_pack/render_biome_compose.py`
Expected: `wrote D:\tmp\wg10_biome_compose\biome_compose_world.png`

- [ ] **Step 3: OWNER GATE — present the render.** Ask: do the four biomes read as themselves AND transition believably (no ghosts, no hard seams, no grid)? Accept → the composition layer stands. Iterate → dial `BlendConfig` / weight bands.

- [ ] **Step 4: Commit**

```bash
git add tools/dem_pack/render_biome_compose.py
git commit -m "feat(biome-compose): grammar-driven multi-biome compose render (owner look gate)"
```

### Task D2: Full-suite gate + STATUS/ROADMAP update

- [ ] **Step 1: Run the full dem_pack suite from repo root**

Run (repo root): `python -m pytest tools/dem_pack/ -q`
Expected: all green (the existing 209 + the new compose/registry/seam tests). NOTE: run from REPO ROOT — running inside `tools/dem_pack` breaks fixture paths (false failures).

- [ ] **Step 2: Update STATUS.md + ROADMAP.md** — record the biome composition layer built + owner-accepted (Fork B; seam-safe recipes + tunable cross-recipe blend); note Slice 3 (Rust port) target is now `compose_biomes` + the seam-safe recipes + `keeper_v2`/grammar, not a single engine.

- [ ] **Step 3: Commit (stage by name)**

```bash
git add docs/plans/STATUS.md docs/plans/ROADMAP.md
git commit -m "docs: biome composition layer (Fork B) built + accepted; update living docs"
```

---

## Notes on later/deferred work (NOT in this plan)

- **badlands + rough-highlands(keeper_v2) recipes:** added to the registry after the 11 standalone synths are proven (different call shapes — `biome_synthesis.generate_family_height` and `keeper_v2.compose_windowed_height_v2`). One task each, same registry pattern.
- **Apron-cropped final blur:** Slice C drops window-local final-smoothing blurs for seam-safety. If the owner wants the smoothing back, reintroduce it as an apron-cropped blur (`keeper_v2.apron_blur_crop_full` pattern). Per-biome, owner-driven.
- **Real grammar weights:** Slice D uses a stand-in smooth weight field. Wiring the actual grammar's biome placement into `compose_biomes` is its own slice (needs the grammar to emit a smooth per-(x,z) weight field; spec §8).
- **Triple-point blend quality:** `compose_biomes` folds pairwise for N>2; if 3-biome meeting points look wrong on review, revisit the fold order / a simultaneous N-way blend.
- **Rust port (Slice 3):** unblocked by this; ports the seam-safe recipes + `compose_biomes` to `height.rs`.
