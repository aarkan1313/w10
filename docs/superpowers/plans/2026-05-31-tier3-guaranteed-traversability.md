# Tier-3 Guaranteed Regime-Aware Traversability Implementation Plan

> **⚠ STATUS 2026-05-31 — partially built, CARVE BLOCKED (read before executing).** Tasks 1–6 (keeper extract,
> params, slope/passability, barrier detection, least-cost path, seam precondition) are DONE and seam-safe in
> `tools/dem_pack/traverse_corridor.py` (9 tests green). **Tasks 7–10 (the CARVE) are BLOCKED by a proven
> design finding** (spec §1.2, memory `worldgen10-tier3-seam-exact-carve`): a globally-routed least-cost-path
> carve cannot be seam-exact (adjacent windows route differently → border delta 0.62 ≠ 0), and no purely-local
> seam-exact operator guarantees a *connected* crossing. The seam-exact connected carve needs a
> cross-seam-stitched **connected-corridor fact = the unbuilt connectivity half of Phase 7B**. The shipped
> module is honest: real barriers are reported `carve_pending` (never falsely "resolved"). **Do NOT execute
> Tasks 7–10 as written** (they assume a carve-along-global-path that breaks seams). They are kept below as the
> historical design; the real next step is the owner decision recorded in STATUS / LEDGER B8 (pull 7B connected
> corridor forward / scope to channel-where-available / Tier-2 param-bias).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an offline Python layer that *guarantees* a connected, edge-reaching, under-slope-budget route through any barrier region of a keeper_v2 window — verify-then-carve, seam-exact, scale/relief-aware — so the keeper+corridor stack becomes an owner-acceptable Slice-3 port candidate.

**Architecture:** Layer over the existing seam-exact stack (`geography_skeleton_windows` facts + `keeper_v2` composition). Detect barriers **from the composed height at the active relief/scale** (slope-walls that sever a crossing, and/or a disconnected low/valley corridor). Run a deterministic slope-penalized, valley-biased least-cost path on the **apron-padded** composed height; if the cheapest crossing is already under budget, carve nothing (no-op); else carve a minimal feathered subtractive delta, crop it to the core (bit-identical seams). Output a world-anchored `traverse_corridor` fact (`route_dist` + `carve_delta`); `final = keeper_height + carve_delta`.

**Tech Stack:** Python 3, NumPy, SciPy (`scipy.ndimage`), pytest. Pure offline — no Rust/GLSL/Godot. Mirrors `tools/dem_pack/keeper_v2.py` conventions.

**Spec:** `docs/superpowers/specs/2026-05-31-worldgen-tier3-guaranteed-traversability-design.md`. Read §1.1 (measured reality), §4 (method), §6 (gates) before starting.

---

## Conventions (read once)

- **All commands run from `d:/workflows/worldgen10/tools/dem_pack`** (that dir is on `sys.path` for the sibling imports `import keeper_v2 as v2`, etc. — every existing test does this).
- Run a single test: `python -m pytest test_traverse_corridor.py::test_name -v`
- Run the file: `python -m pytest test_traverse_corridor.py -v`
- **Canonical test window** (used everywhere, matches `test_keeper_v2.py`):
  ```python
  import export_godot_rough_world_chunks as ex
  import geography_skeleton_windows as win
  spec = ex._window_spec(129, ex.CHUNK_SPAN_M)          # core_span 25600, apron 25600, spacing 200
  w = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 133, spec)  # seed 133 = rough_anchor
  ```
  Verified constants: `spacing_m = 200.0`, `apron_px = 128`, `_core_slice(spec) = slice(128, 257)`, core grid `129×129`.
- **High-relief barrier config** (verified to produce ≈11% slope-impassable terrain — the default gentle config has none):
  ```python
  import dataclasses
  spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
  ```
- **Seam rule (non-negotiable):** anything that influences `carve_delta` must depend only on shared apron facts — pathfind + carve on the **apron-padded** composed height, crop to core with `win._core_slice`. No raw per-core `np.percentile`/`min`/`max` may key a carve (same bug class as keeper_v2's `geo.norm01`, 0.0118 → 0.0).
- **Commit after every task** (frequent commits). Stage by name — `git add <files>`, **never** `git add -A` (repo has unrelated dirty files). Do not commit unless the human says; this plan's commit steps are written as the intended granularity — the executor confirms commit policy with the human.

---

## File Structure

- **Create** `tools/dem_pack/traverse_corridor.py` — the whole Tier-3 layer: `TraverseParams`, barrier detection, least-cost path, carve, `build_traverse_corridor`, `compose_with_corridor`.
- **Create** `tools/dem_pack/test_traverse_corridor.py` — all unit/seam/guarantee tests.
- **Modify** `tools/dem_pack/keeper_v2.py` — extract `compose_windowed_height_v2_full` (pre-crop, apron-padded) so Tier-3 can detect/carve on the padded grid; existing `compose_windowed_height_v2` calls it and crops (behavior unchanged, guarded by existing seam test).
- **Create** `tools/dem_pack/report_tier3_traversability.py` — runs the guarantee/seam/minimality gates + prints a verdict (the Tier-3 analogue of `report_abv_traversability.py`).
- **Create** `tools/dem_pack/render_tier3_corridor.py` — corridors-on overlay sheet for the owner eye (Task 11).

---

## Task 1: Extract apron-padded composition from keeper_v2 (no behavior change)

**Files:**
- Modify: `tools/dem_pack/keeper_v2.py:92-131` (`compose_windowed_height_v2`)
- Test: `tools/dem_pack/test_keeper_v2.py` (existing seam test is the guard)

- [ ] **Step 1: Run the existing keeper_v2 seam test to capture the green baseline**

Run: `python -m pytest test_keeper_v2.py::test_v2_adjacent_window_seams_are_exact -v`
Expected: PASS (border delta == 0.0).

- [ ] **Step 2: Refactor — split compose into full + crop**

In `keeper_v2.py`, replace the tail of `compose_windowed_height_v2` so the full padded grid is produced by a new function and the public function crops it. The full function returns the grid **before** `height[core, core]`:

```python
def compose_windowed_height_v2_full(window, seed, spec, p):
    """Apron-padded composed height (pre-crop). Tier-3 detects/carves on this, then crops to core.
    Identical math to compose_windowed_height_v2 up to the final core crop."""
    apron_px = int(round(float(spec.apron_m) / float(spec.spacing_m)))
    span = float(spec.core_span_m); spacing = float(spec.spacing_m)
    facts = {k: np.asarray(window[k], dtype=np.float64) for k in
             ("uplift","routed_surface","discharge","tributary","channel_axis","crest_dist","channel_dist")}
    weights, slope, basin, range_core, plateau, channel_axis = _regime_weights(facts, spec, p, apron_px)
    basin_w, fan_w, foothill_w, plateau_w, range_w, badlands_w = weights
    uplift = facts["uplift"]; discharge = facts["discharge"]; tributary = facts["tributary"]
    channel_dist = facts["channel_dist"]
    wx = np.asarray(window["wx"]); wz = np.asarray(window["wz"])
    w_x, w_z = wg.recursive_domain_warp(wx, wz, warp_amount=span*0.030, warp_freq=1.0/(span*0.45), seed=seed+750, steps=2)
    low = affine_remap(wg.fbm(w_x, w_z, 1.0/(span*0.38), 4, seed+751, gain=0.56), p.remap_center, 1.0)
    range_texture = affine_remap(wg.ridged_multifractal(w_x, w_z, 1.0/(span*0.085), 5, seed+752, gain=0.54), 0.5, 1.0)
    badland_texture = affine_remap(wg.ridged_multifractal(w_x, w_z, 1.0/(span*0.040), 4, seed+753, gain=0.50), 0.5, 1.0)
    fine = affine_remap(wg.fbm(w_x, w_z, 1.0/(span*0.030), 4, seed+754, gain=0.48), p.remap_center, 1.0)
    base = 1.45*uplift - 0.62*basin + 0.26*plateau + 0.10*low
    primary_shape = np.exp(-(channel_dist / max(span*0.010, 1.0))**2)
    tributary_shape = np.exp(-(channel_dist / max(span*0.018, 1.0))**2)
    primary = geo.smoothstep(0.56, 0.96, discharge) * (0.28 + 0.72*primary_shape)
    tributary_cut = geo.smoothstep(0.34, 0.82, tributary) * (0.45 + 0.55*tributary_shape) * (0.35 + 0.65*slope)
    incision = p.incision_gain * (0.72*primary + 0.34*tributary_cut)
    incision_context = np.clip(0.70 + 0.44*badlands_w + 0.26*foothill_w + 0.18*range_w - 0.50*basin_w - 0.35*fan_w, 0.18, 1.18)
    height = base - 0.38*incision_context*incision
    height = height + p.range_texture_gain * range_w * range_texture
    height = height + 0.18 * foothill_w * range_texture
    height = height + 0.16 * fan_w * apron_blur_crop_full(channel_axis, apron_px, 3.0)
    height = height + 0.10 * plateau_w * low
    height = height + p.badland_gain * badlands_w * (0.58*badland_texture + 0.42*fine)
    height = height + p.fine_gain * (badlands_w + range_w + foothill_w) * fine
    height = height - 0.06 * (badlands_w + foothill_w + 0.35*plateau_w) * tributary_cut
    height = height * p.relief_amplitude
    height = np.tanh(height * 0.72)
    height = height * p.post_tanh_gain
    sigma_final = max(p.blur_radius_m / spacing, 0.1)
    mix = float(np.clip(p.final_blur_mix, 0.0, 1.0))
    if mix > 0.0:
        height = (1.0 - mix) * height + mix * apron_blur_crop_full(height, apron_px, sigma_final)
    height = affine_remap(height, p.remap_center, p.remap_scale)
    return height


def compose_windowed_height_v2(window, seed, spec, p):
    height = compose_windowed_height_v2_full(window, seed, spec, p)
    core = win._core_slice(spec)
    return np.ascontiguousarray(height[core, core])
```

(This is a pure extract: the old function body verbatim, with the final two lines moved into the wrapper.)

- [ ] **Step 3: Write a test that full[core] == public compose, and full is the padded shape**

Add to `test_keeper_v2.py`:

```python
def test_compose_full_core_matches_public_and_is_padded():
    import keeper_v2 as v2
    w, spec = _window()
    p = v2.KeeperV2Params()
    full = v2.compose_windowed_height_v2_full(w, 133, spec, p)
    core = v2.compose_windowed_height_v2(w, 133, spec, p)
    cs = win._core_slice(spec)
    assert full.shape == w["uplift"].shape          # apron-padded (385x385 at this spec)
    assert np.array_equal(full[cs, cs], core)        # crop of full == public output
```

- [ ] **Step 4: Run both tests**

Run: `python -m pytest test_keeper_v2.py::test_v2_adjacent_window_seams_are_exact test_keeper_v2.py::test_compose_full_core_matches_public_and_is_padded -v`
Expected: PASS, PASS.

- [ ] **Step 5: Commit**

```bash
git add keeper_v2.py test_keeper_v2.py
git commit -m "refactor(keeper_v2): extract compose_windowed_height_v2_full for Tier-3 apron-padded access"
```

---

## Task 2: TraverseParams dataclass (all knobs, no magic numbers)

**Files:**
- Create: `tools/dem_pack/traverse_corridor.py`
- Test: `tools/dem_pack/test_traverse_corridor.py`

- [ ] **Step 1: Write the failing test**

```python
import dataclasses
import numpy as np
import traverse_corridor as tc
import keeper_v2 as v2
import geography_skeleton_windows as win
import export_godot_rough_world_chunks as ex


def test_params_defaults_present_and_overridable():
    p = tc.TraverseParams()
    for name in ("slope_budget", "low_corridor_cutoff", "min_barrier_component_frac",
                 "slope_penalty", "drainage_bias", "corridor_width_m", "carve_max_m",
                 "row_tolerance_px", "band_px", "scene_width_m", "height_scale_m"):
        assert hasattr(p, name)
    p2 = dataclasses.replace(p, slope_budget=0.40)
    assert p2.slope_budget == 0.40 and p.slope_budget != 0.40
```

- [ ] **Step 2: Run it to verify it fails**

Run: `python -m pytest test_traverse_corridor.py::test_params_defaults_present_and_overridable -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'traverse_corridor'`.

- [ ] **Step 3: Write minimal implementation**

Create `traverse_corridor.py`:

```python
from __future__ import annotations
from dataclasses import dataclass
import heapq
import numpy as np

import keeper_v2 as v2
import geography_skeleton_windows as win
import analyze_rough_world_traversability as trav


@dataclass(frozen=True)
class TraverseParams:
    # Route must hold this grade (rise/run) on the conditioned mesh; also defines a slope-wall.
    slope_budget: float = 0.28          # == trav.PASSABLE_SLOPE, the play passability band
    # Seam-safe fixed height cutoff for the low/valley-corridor test (NOT a per-core percentile).
    low_corridor_cutoff: float = 0.0    # composed height is ~tanh-centered near 0; <= cutoff == "low"
    min_barrier_component_frac: float = 0.02   # interior barriers smaller than this -> walk around
    slope_penalty: float = 24.0         # cost multiplier per unit slope over budget
    drainage_bias: float = 0.55         # route bias toward channels/valleys (0 = none)
    corridor_width_m: float = 1200.0    # feather half-width of the carve
    carve_max_m: float = 220.0          # hard cap on |carve_delta| (world metres); exceed => report, not silent
    row_tolerance_px: int = 2           # cross-seam join tolerance
    band_px: int = 2
    # Active review scale/relief the barrier + slope are measured at (the analyzer convention).
    scene_width_m: float = 25600.0      # the 25.6 km play span (== spec.core_span_m at chunk scale)
    height_scale_m: float = trav.BASE_HEIGHT_SCALE_M   # 260 m default relief; raise to test/sim higher relief
```

- [ ] **Step 4: Run it to verify it passes**

Run: `python -m pytest test_traverse_corridor.py::test_params_defaults_present_and_overridable -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add traverse_corridor.py test_traverse_corridor.py
git commit -m "feat(tier3): TraverseParams with all route/scale knobs"
```

---

## Task 3: Slope + passability helpers on a padded grid

**Files:**
- Modify: `tools/dem_pack/traverse_corridor.py`
- Test: `tools/dem_pack/test_traverse_corridor.py`

The barrier/route are measured at the active scale. The padded grid spans `core_span + 2*apron` metres, so its per-cell metre size equals the core's. We compute slope over the **padded** grid at the active `height_scale_m`, using the padded grid's own world width so `cell_m` matches the core.

- [ ] **Step 1: Write the failing test**

```python
def _window():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    w = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 133, spec)
    return w, spec


def test_padded_slope_matches_core_cell_size():
    w, spec = _window()
    p = tc.TraverseParams()
    full = v2.compose_windowed_height_v2_full(w, 133, spec, p_v2())  # helper below
    slopes = tc.padded_slope(full, spec, p)
    # padded grid same shape as facts; finite; cell size == spacing-derived core cell
    assert slopes.shape == full.shape
    assert np.all(np.isfinite(slopes))


def p_v2():
    return v2.KeeperV2Params()
```

- [ ] **Step 2: Run it to verify it fails**

Run: `python -m pytest test_traverse_corridor.py::test_padded_slope_matches_core_cell_size -v`
Expected: FAIL — `AttributeError: module 'traverse_corridor' has no attribute 'padded_slope'`.

- [ ] **Step 3: Write minimal implementation**

Append to `traverse_corridor.py`:

```python
def _padded_world_width_m(grid: np.ndarray, spec) -> float:
    """World width spanned by the apron-padded grid, so cell_m == core cell_m (= spacing_m)."""
    return float(spec.spacing_m) * float(grid.shape[0] - 1)


def padded_slope(height_full: np.ndarray, spec, p: TraverseParams) -> np.ndarray:
    """Slope magnitude over the padded composed height at the active relief, reusing the analyzer's
    slope_grid so the route shares the same rise/run convention as the Tier-1 report."""
    width_m = _padded_world_width_m(height_full, spec)
    return trav.slope_grid(np.asarray(height_full, dtype=np.float64), scene_width_m=width_m,
                           height_scale_m=float(p.height_scale_m))


def passable_mask(height_full: np.ndarray, spec, p: TraverseParams) -> np.ndarray:
    return padded_slope(height_full, spec, p) <= float(p.slope_budget)
```

- [ ] **Step 4: Run it**

Run: `python -m pytest test_traverse_corridor.py::test_padded_slope_matches_core_cell_size -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add traverse_corridor.py test_traverse_corridor.py
git commit -m "feat(tier3): padded slope + passability at active relief/scale"
```

---

## Task 4: Barrier detection — needs-a-route decision (height-derived, scale/relief-aware)

**Files:**
- Modify: `tools/dem_pack/traverse_corridor.py`
- Test: `tools/dem_pack/test_traverse_corridor.py`

Decision (spec §4.1): a window **needs a route** iff a slope-wall severs the crossing OR the low corridor doesn't cross. Verified: default config (260 m, 25.6 km) needs **no** route; the spiky config needs one.

- [ ] **Step 1: Write the failing test**

```python
import dataclasses


def test_default_config_is_crossable_spiky_needs_route():
    w, spec = _window()
    p = tc.TraverseParams()
    full_default = v2.compose_windowed_height_v2_full(w, 133, spec, v2.KeeperV2Params())
    nd = tc.needs_route(full_default, spec, p)
    assert nd["needs_route"] is False          # measured: gentle default crosses WE+NS, no slope-wall

    spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    full_spiky = v2.compose_windowed_height_v2_full(w, 133, spec, spiky)
    ns = tc.needs_route(full_spiky, spec, p)
    assert ns["slope_wall_frac"] > 0.0         # spiky has real impassable terrain
    # a barrier that severs a crossing OR a broken low corridor must trip needs_route on spiky terrain
    assert ns["needs_route"] is True
```

- [ ] **Step 2: Run it to verify it fails**

Run: `python -m pytest test_traverse_corridor.py::test_default_config_is_crossable_spiky_needs_route -v`
Expected: FAIL — `AttributeError: ... 'needs_route'`.

- [ ] **Step 3: Write minimal implementation**

Append to `traverse_corridor.py`:

```python
def _core(grid_full: np.ndarray, spec) -> np.ndarray:
    cs = win._core_slice(spec)
    return np.asarray(grid_full)[cs, cs]


def needs_route(height_full: np.ndarray, spec, p: TraverseParams) -> dict:
    """Decide if the CORE window needs a guaranteed route. Detection reads the core composed height
    (detection writes no height, so a core-only percentile here cannot break seams — only carve inputs
    must be seam-safe, see Task 7). Returns the decision + diagnostics."""
    core_h = _core(height_full, spec)
    slopes_core = trav.slope_grid(core_h, scene_width_m=float(p.scene_width_m), height_scale_m=float(p.height_scale_m))
    passable = slopes_core <= float(p.slope_budget)
    slope_wall = ~passable
    pc = trav.component_stats(passable)
    passable_crosses = bool(pc["largest_crosses_we"] or pc["largest_crosses_ns"])

    # low/valley corridor with a SEAM-SAFE fixed cutoff (NOT np.percentile) — see spec §4.1 hazard.
    low = passable & (core_h <= float(p.low_corridor_cutoff))
    lc = trav.component_stats(low)
    low_crosses = bool(lc["largest_crosses_we"] or lc["largest_crosses_ns"])

    slope_wall_frac = float(np.mean(slope_wall))
    sw = trav.component_stats(slope_wall)
    # a slope-wall "severs" the crossing if the passable region no longer crosses at all
    slope_wall_severs = (slope_wall_frac > 0.0) and (not passable_crosses) and \
                        (float(sw["largest_frac"]) >= float(p.min_barrier_component_frac))

    needs = bool(slope_wall_severs or (not low_crosses))
    return {
        "needs_route": needs,
        "slope_wall_frac": slope_wall_frac,
        "slope_wall_severs": slope_wall_severs,
        "passable_crosses": passable_crosses,
        "low_corridor_crosses": low_crosses,
    }
```

- [ ] **Step 4: Run it**

Run: `python -m pytest test_traverse_corridor.py::test_default_config_is_crossable_spiky_needs_route -v`
Expected: PASS.

> If the default `low_corridor_cutoff=0.0` makes the gentle default report `needs_route=True` (because the
> low corridor doesn't cross even when the whole window is passable — which is the Tier-1 finding), that is
> **correct behavior**, not a bug: the gentle default *does* lack a connected valley route. In that case the
> test's `nd["needs_route"] is False` assertion is wrong about the default and must be relaxed to
> `nd["slope_wall_severs"] is False` (no wall) while the spiky assertions stand. Decide which by running
> `needs_route` once and reading `low_corridor_crosses` for the default; keep the test honest to the measurement.

- [ ] **Step 5: Commit**

```bash
git add traverse_corridor.py test_traverse_corridor.py
git commit -m "feat(tier3): height-derived scale/relief-aware barrier detection (needs_route)"
```

---

## Task 5: Deterministic slope-penalized, valley-biased least-cost path

**Files:**
- Modify: `tools/dem_pack/traverse_corridor.py`
- Test: `tools/dem_pack/test_traverse_corridor.py`

Edge→edge least-cost path on the **padded** grid. Cost per step = horizontal distance × `(1 + slope_penalty·max(0, slope − budget))`, minus a `drainage_bias` reward where channels are strong / valleys are low. Deterministic: Dijkstra with a fixed tie-break on flattened index.

- [ ] **Step 1: Write the failing test**

```python
def test_least_cost_path_is_deterministic_and_edge_to_edge():
    w, spec = _window()
    p = tc.TraverseParams()
    spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    full = v2.compose_windowed_height_v2_full(w, 133, spec, spiky)
    slopes = tc.padded_slope(full, spec, p)
    channel = np.asarray(w["channel_axis"], dtype=np.float64)
    r1 = tc.least_cost_crossing(slopes, full, channel, spec, p, axis="x")
    r2 = tc.least_cost_crossing(slopes, full, channel, spec, p, axis="x")
    assert r1["path"] == r2["path"]                      # determinism
    rows = [pt[0] for pt in r1["path"]]; cols = [pt[1] for pt in r1["path"]]
    assert min(cols) == 0 and max(cols) == full.shape[1] - 1   # spans west->east on the padded grid
    assert isinstance(r1["max_step_slope"], float)
```

- [ ] **Step 2: Run it to verify it fails**

Run: `python -m pytest test_traverse_corridor.py::test_least_cost_path_is_deterministic_and_edge_to_edge -v`
Expected: FAIL — `AttributeError: ... 'least_cost_crossing'`.

- [ ] **Step 3: Write minimal implementation**

Append to `traverse_corridor.py`:

```python
def _step_cost(slope_b, h_b, chan_b, cell_m, p: TraverseParams) -> float:
    over = max(0.0, float(slope_b) - float(p.slope_budget))
    base = cell_m * (1.0 + float(p.slope_penalty) * over)
    # valley/channel reward: lower cost where a channel is strong or the floor is low.
    reward = float(p.drainage_bias) * (0.6 * float(chan_b) + 0.4 * float(np.clip(-h_b, 0.0, 1.0)))
    return max(base * (1.0 - reward), cell_m * 0.05)     # never non-positive


def least_cost_crossing(slopes, height_full, channel_full, spec, p: TraverseParams, axis: str = "x") -> dict:
    """Dijkstra edge->edge across the padded grid. axis='x' crosses west->east, 'z' north->south.
    Deterministic: ties broken by flattened index. Returns path + the worst step slope along it."""
    s = np.asarray(slopes, dtype=np.float64)
    h = np.asarray(height_full, dtype=np.float64)
    ch = np.asarray(channel_full, dtype=np.float64)
    H, W = s.shape
    cell_m = float(spec.spacing_m)
    work_s, work_h, work_ch = (s, h, ch) if axis == "x" else (s.T, h.T, ch.T)
    Hh, Ww = work_s.shape
    INF = float("inf")
    dist = np.full(Hh * Ww, INF)
    prev = np.full(Hh * Ww, -1, dtype=np.int64)
    pq: list[tuple[float, int]] = []
    for r in range(Hh):                                   # all left-column cells are sources
        idx = r * Ww + 0
        c = _step_cost(work_s[r, 0], work_h[r, 0], work_ch[r, 0], cell_m, p)
        dist[idx] = c
        heapq.heappush(pq, (c, idx))
    target = -1
    while pq:
        d, idx = heapq.heappop(pq)
        if d > dist[idx]:
            continue
        r, c = divmod(idx, Ww)
        if c == Ww - 1:
            target = idx
            break
        for dr, dc in ((-1, 0), (1, 0), (0, 1), (0, -1)):  # fixed neighbour order = deterministic ties
            nr, nc = r + dr, c + dc
            if 0 <= nr < Hh and 0 <= nc < Ww:
                nidx = nr * Ww + nc
                step = _step_cost(work_s[nr, nc], work_h[nr, nc], work_ch[nr, nc], cell_m, p)
                nd = d + step
                if nd < dist[nidx]:
                    dist[nidx] = nd
                    prev[nidx] = idx
                    heapq.heappush(pq, (nd, nidx))
    path_work: list[tuple[int, int]] = []
    node = target
    while node != -1:
        path_work.append(divmod(node, Ww))
        node = int(prev[node])
    path_work.reverse()
    # map back to (row, col) of the original (non-transposed) grid
    path = [(r, c) if axis == "x" else (c, r) for (r, c) in path_work]
    max_step_slope = 0.0
    for (r, c) in path:
        max_step_slope = max(max_step_slope, float(s[r, c]))
    return {"path": path, "max_step_slope": float(max_step_slope), "total_cost": float(dist[target])}
```

- [ ] **Step 4: Run it**

Run: `python -m pytest test_traverse_corridor.py::test_least_cost_path_is_deterministic_and_edge_to_edge -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add traverse_corridor.py test_traverse_corridor.py
git commit -m "feat(tier3): deterministic slope-penalized valley-biased least-cost crossing"
```

---

## Task 6: Apron-symmetry of the path (seam precondition)

**Files:**
- Test: `tools/dem_pack/test_traverse_corridor.py`

The carve will be seam-exact only if the path's geometry within the shared region is identical from either neighbor's apron. Verify the path through the overlapping band matches between this window and its east neighbor.

- [ ] **Step 1: Write the failing test**

```python
def test_path_overlap_band_matches_between_neighbors():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    p = tc.TraverseParams()
    spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    ox, oz = ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M
    wa = win.build_skeleton_window(ox, oz, 133, spec)
    wb = win.build_skeleton_window(ox + ex.CHUNK_SPAN_M, oz, 133, spec)
    fa = v2.compose_windowed_height_v2_full(wa, 133, spec, spiky)
    fb = v2.compose_windowed_height_v2_full(wb, 133, spec, spiky)
    # The east apron of A and the west apron+core of B sample the same world coords -> identical composed height.
    cs = win._core_slice(spec); a = int(round(spec.apron_m / spec.spacing_m))
    # A's eastern apron columns overlap B's western core columns at the same world x.
    overlap_a = fa[:, cs.stop:cs.stop + a]      # A core-east + into apron
    overlap_b = fb[:, a - (fa.shape[1] - cs.stop): a]  # corresponding world cols in B
    assert overlap_a.shape == overlap_b.shape
    assert np.allclose(overlap_a, overlap_b, atol=1e-9)   # shared world coords => identical composed height
```

- [ ] **Step 2: Run it**

Run: `python -m pytest test_traverse_corridor.py::test_path_overlap_band_matches_between_neighbors -v`
Expected: PASS if the overlap indexing is right; if the slice arithmetic mismatches shapes, adjust the band indices until `overlap_a`/`overlap_b` address the same world columns (the invariant being tested is *composed height at a world coord is window-independent* — already guaranteed by keeper_v2's seam-exactness; this test just locates the shared band the carve will live in).

> This test pins the world-coordinate band the carve must be computed on. If it cannot be made to pass, the
> carve cannot be seam-exact and that is a real finding (spec §8) — surface it, do not weaken the carve test.

- [ ] **Step 3: Commit**

```bash
git add test_traverse_corridor.py
git commit -m "test(tier3): pin world-coord overlap band for seam-exact carve"
```

---

## Task 7: Minimal feathered carve, cropped to core (seam-exact)

**Files:**
- Modify: `tools/dem_pack/traverse_corridor.py`
- Test: `tools/dem_pack/test_traverse_corridor.py`

Carve only if the barrier is real. The carve's job is to make **`needs_route` go False** on the final surface — i.e. reconnect *whichever* crossing was broken (a wall-sever needs the passable crossing back; a low-corridor break needs the valley route back). Lower height along the least-cost path enough that, after carving, `needs_route(final) is False`, feathered over `corridor_width_m`, capped at `carve_max_m`. Compute on the **padded** grid (path + distance-to-path are world-coordinate functions of shared apron facts), crop the delta to core.

> **MEASURED — barrier fixtures (memory `worldgen10-tier3-barrier-measurements`):** seed 133 is a BAD barrier fixture — it stays fully traversable even spiky (low-dominated terrain), so a seed-133-spiky carve test would be vacuous (carve == 0). Use **seed 1, spiky, at the 25.6 km span** for a real **low-corridor barrier** (`needs_route` via `low_corridor_crosses=False`), and the gentle default (seed 133) for the verify-first no-op. The carve target is `needs_route(final) is False`, NOT merely `max_step_slope <= budget` (that was the original, vacuous-for-low-corridor framing).

- [ ] **Step 1: Write the failing test**

```python
def test_carve_zero_on_crossable_and_resolves_barrier_seam_exact():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    p = tc.TraverseParams()
    # gentle default (seed 133) is fully crossable -> verify-first no-op, carve all zero
    wa = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 133, spec)
    res = tc.build_traverse_corridor(wa, 133, spec, p, v2.KeeperV2Params())
    assert res["needs_route"] is False
    assert np.count_nonzero(res["carve_delta"]) == 0          # verify-first no-op

    # REAL low-corridor barrier: seed 1, spiky, 25.6 km (measured needs_route=True via broken valley route)
    spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    ox, oz = ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M
    ra = tc.build_traverse_corridor(win.build_skeleton_window(ox, oz, 1, spec), 1, spec, p, spiky)
    rb = tc.build_traverse_corridor(win.build_skeleton_window(ox + ex.CHUNK_SPAN_M, oz, 1, spec), 1, spec, p, spiky)
    assert ra["needs_route"] is True and ra["carved"] is True   # did-real-work: a barrier was carved
    # the carve must RESOLVE the barrier: needs_route_core is False on the final CORE surface
    keeper_core_a = v2.compose_windowed_height_v2(win.build_skeleton_window(ox, oz, 1, spec), 1, spec, spiky)
    final_core_a = keeper_core_a + ra["carve_delta"]
    assert tc.needs_route_core(final_core_a, spec, p)["needs_route"] is False   # barrier resolved
    # seam-exact carve: adjacent windows agree on the shared border bit-exactly
    border = float(np.max(np.abs(ra["carve_delta"][:, -1] - rb["carve_delta"][:, 0])))
    assert border == 0.0, f"carve broke seams: {border}"
    assert np.max(np.abs(ra["carve_delta"])) <= p.carve_max_m + 1e-9
```

> **Refactor `needs_route` to expose a core-grid form (do this in Task 7).** Task 4's `needs_route(height_full, ...)` already crops to core first (`core_h = _core(height_full, spec)`) then works on `core_h`. Extract that core logic into `needs_route_core(core_height, spec, p)` and make `needs_route(height_full, spec, p)` call `needs_route_core(_core(height_full, spec), spec, p)`. This is a pure extract (no behavior change — guard with the existing Task 4 test). Then the guarantee check is trivially honest: `needs_route_core(keeper_core + carve_delta, spec, p)["needs_route"] is False`. The carve's loop target is exactly this: lower the path until `needs_route_core` of the post-carve core is False (bounded by `carve_max_m`; if it can't be met within the cap, report it — the per-game-impassable case, spec §4.3 / §8).

- [ ] **Step 2: Run it to verify it fails**

Run: `python -m pytest test_traverse_corridor.py::test_carve_is_zero_when_path_under_budget_and_seam_exact_when_carving -v`
Expected: FAIL — `AttributeError: ... 'build_traverse_corridor'`.

- [ ] **Step 3: Write minimal implementation**

Append to `traverse_corridor.py`:

```python
from scipy.ndimage import distance_transform_edt


def _carve_along_path(height_full, path, slopes, spec, p: TraverseParams) -> np.ndarray:
    """Subtractive delta (<=0) on the padded grid: lower the path's high points until each along-path
    step is <= slope_budget, then feather over corridor_width_m. Returns padded delta (metres)."""
    H, W = height_full.shape
    cell_m = float(spec.spacing_m)
    h_m = np.asarray(height_full, dtype=np.float64) * float(p.height_scale_m)   # work in metres
    # target metre-height profile along the path: a monotone-feasible corridor floor.
    idx = np.array(path, dtype=np.int64)
    prof = h_m[idx[:, 0], idx[:, 1]].astype(np.float64)
    max_drop_per_step = float(p.slope_budget) * cell_m
    # forward+backward clamp so consecutive samples never rise/fall faster than the budget
    for i in range(1, len(prof)):
        prof[i] = min(prof[i], prof[i - 1] + max_drop_per_step)
    for i in range(len(prof) - 2, -1, -1):
        prof[i] = min(prof[i], prof[i + 1] + max_drop_per_step)
    drop = np.clip(h_m[idx[:, 0], idx[:, 1]] - prof, 0.0, float(p.carve_max_m))   # >=0 metres to remove
    # scatter the path drop onto a grid, feather by distance-to-path
    on_path = np.zeros((H, W), dtype=bool)
    path_drop = np.zeros((H, W), dtype=np.float64)
    on_path[idx[:, 0], idx[:, 1]] = True
    path_drop[idx[:, 0], idx[:, 1]] = drop
    dist_px, (iy, ix) = distance_transform_edt(~on_path, return_indices=True)
    nearest_drop = path_drop[iy, ix]
    feather = np.clip(1.0 - (dist_px * cell_m) / max(float(p.corridor_width_m), 1.0), 0.0, 1.0)
    delta_m = -(nearest_drop * feather)                       # <= 0
    return delta_m / float(p.height_scale_m)                  # back to height units


def build_traverse_corridor(window, seed, spec, p: TraverseParams, keeper_params) -> dict:
    """Verify-then-carve for one window. Returns core-cropped carve_delta + route_dist + diagnostics."""
    full = v2.compose_windowed_height_v2_full(window, seed, spec, keeper_params)
    decision = needs_route(full, spec, p)
    cs = win._core_slice(spec)
    if not decision["needs_route"]:
        zero = np.zeros((cs.stop - cs.start, cs.stop - cs.start), dtype=np.float64)
        far = np.full_like(zero, float(spec.apron_m) * 0.68)
        return {"carve_delta": zero, "route_dist": far, "carved": False, **decision}

    slopes = padded_slope(full, spec, p)
    channel = np.asarray(window["channel_axis"], dtype=np.float64)
    keeper_core = v2.compose_windowed_height_v2(window, seed, spec, keeper_params)
    # Try both crossing axes; keep the one whose carve actually resolves the barrier (needs_route_core False)
    # with the least disturbance. The barrier may block WE or NS — do not assume x.
    best = None
    for axis in ("x", "z"):
        route = least_cost_crossing(slopes, full, channel, spec, p, axis=axis)
        delta_full = _carve_along_path(full, route["path"], slopes, spec, p)
        final_core = keeper_core + delta_full[cs, cs]
        resolved = not needs_route_core(final_core, spec, p)["needs_route"]
        disturbance = float(np.max(np.abs(delta_full)))
        cand = {"axis": axis, "route": route, "delta_full": delta_full, "resolved": resolved, "disturbance": disturbance}
        # prefer a resolving carve; among resolving (or among non-resolving) prefer less disturbance
        if best is None or (cand["resolved"], -cand["disturbance"]) > (best["resolved"], -best["disturbance"]):
            best = cand
    delta_full = best["delta_full"]
    carved = bool(np.any(delta_full != 0.0))
    route = best["route"]
    # honest reporting: if no axis resolved the barrier within carve_max_m, this window is impassable at this
    # relief (the per-game opt-in case, spec §4.3/§8). Flag it; do not silently claim success.
    resolved = bool(best["resolved"])
    # route distance fact (padded), saturating to "far" outside the apron-valid band
    on_path = np.zeros(full.shape, dtype=bool)
    pidx = np.array(route["path"], dtype=np.int64)
    on_path[pidx[:, 0], pidx[:, 1]] = True
    dist_full = np.minimum(distance_transform_edt(~on_path) * float(spec.spacing_m), float(spec.apron_m) * 0.68)
    return {
        "carve_delta": np.ascontiguousarray(delta_full[cs, cs]),
        "route_dist": np.ascontiguousarray(dist_full[cs, cs]),
        "carved": carved,
        "resolved": resolved,
        "max_step_slope": route["max_step_slope"],
        **decision,
    }
```

> **Carve must drive `needs_route_core` to False, not just slope-under-budget.** `_carve_along_path` as written lowers the path until each along-path *step* meets the slope budget — that resolves a slope-wall barrier, but a **low-corridor** barrier needs the path's height brought **at/under `low_corridor_cutoff`** so the valley route reconnects. Extend `_carve_along_path` (or its profile target) so the corridor floor along the path is `min(slope-feasible-profile, low_corridor_cutoff)` where the route crosses the core — i.e. the carved route is both walkable AND counts as "low". Keep the `carve_max_m` cap; if the cap can't get the path under the cutoff, `resolved` stays False and that is reported (not hidden). Verify with the Step-4 test (`needs_route_core(final_core)` False on the seed-1 barrier).

- [ ] **Step 4: Run it**

Run: `python -m pytest test_traverse_corridor.py::test_carve_zero_on_crossable_and_resolves_barrier_seam_exact -v`
Expected: PASS.

> **Seam-break debugging (likely needed here — this is the hard part).** If the border delta != 0.0: the carve
> reads the padded composed height + facts (world-coordinate-identical between neighbors), so the delta SHOULD
> match — but the **least-cost path is global**: the cheapest west→east route through window A's core can differ
> from the route B computes, because each sees a different apron slice, so the carve lands on different core
> cells. The fix that preserves seams: restrict the carve to the segment of the path **inside the core +
> `corridor_width_m` feather band**, AND make the path within that band a deterministic function of world
> coordinates only (e.g. anchor the crossing to the lowest-cost core column/row and carve a locally-computed
> feathered channel around it, not the globally-cheapest apron-to-apron path). If a globally-routed carve cannot
> be made seam-exact, that is the spec §8 finding — report it; fall back to a per-core-anchored carve or a wider
> apron. **Do not relax the `== 0.0` gate.** This is the one task where a BLOCKED escalation is legitimate if the
> seam-exact carve proves irreducibly global.

- [ ] **Step 5: Commit**

```bash
git add traverse_corridor.py test_traverse_corridor.py
git commit -m "feat(tier3): minimal feathered seam-exact carve + traverse_corridor fact"
```

---

## Task 8: compose_with_corridor + visible==collision parity

**Files:**
- Modify: `tools/dem_pack/traverse_corridor.py`
- Test: `tools/dem_pack/test_traverse_corridor.py`

- [ ] **Step 1: Write the failing test**

```python
def test_compose_with_corridor_adds_delta_and_is_deterministic():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    p = tc.TraverseParams()
    kp = v2.KeeperV2Params()
    w = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 133, spec)
    final1, res1 = tc.compose_with_corridor(w, 133, spec, p, kp)
    final2, res2 = tc.compose_with_corridor(w, 133, spec, p, kp)
    keeper = v2.compose_windowed_height_v2(w, 133, spec, kp)
    assert np.allclose(final1, keeper + res1["carve_delta"])     # final == keeper + carve
    assert np.array_equal(final1, final2)                        # deterministic (== "collision" sample)
```

- [ ] **Step 2: Run it to verify it fails**

Run: `python -m pytest test_traverse_corridor.py::test_compose_with_corridor_adds_delta_and_is_deterministic -v`
Expected: FAIL — `AttributeError: ... 'compose_with_corridor'`.

- [ ] **Step 3: Write minimal implementation**

Append to `traverse_corridor.py`:

```python
def compose_with_corridor(window, seed, spec, p: TraverseParams, keeper_params):
    """Final composed height = keeper core height + Tier-3 carve delta. Same value for render and
    collision (both call this) -> visible==collision parity holds by construction."""
    keeper = v2.compose_windowed_height_v2(window, seed, spec, keeper_params)
    res = build_traverse_corridor(window, seed, spec, p, keeper_params)
    return np.ascontiguousarray(keeper + res["carve_delta"]), res
```

- [ ] **Step 4: Run it**

Run: `python -m pytest test_traverse_corridor.py::test_compose_with_corridor_adds_delta_and_is_deterministic -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add traverse_corridor.py test_traverse_corridor.py
git commit -m "feat(tier3): compose_with_corridor (final = keeper + carve), parity-by-construction"
```

---

## Task 9: The guarantee gate + still-rugged + minimal-disturbance (headline)

**Files:**
- Modify: `tools/dem_pack/traverse_corridor.py` (add `crossing_holds` helper)
- Test: `tools/dem_pack/test_traverse_corridor.py`

The guarantee is `needs_route_core(final_core) is False` — the post-carve core no longer needs a route (the broken crossing, whichever type, is reconnected). `crossing_holds` is the thin boolean wrapper. Tested on BOTH measured barrier types (memory `worldgen10-tier3-barrier-measurements`): the **low-corridor** fixture (seed 1, spiky, 25.6 km) and the **slope-wall-sever** fixture (seed 42, gain 3.5, **4 km span**, `scene_width_m=4000`).

- [ ] **Step 1: Write the failing test**

```python
def test_guarantee_holds_on_both_barrier_types_minimal_and_still_rugged():
    spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    wall_kp = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=3.5, relief_amplitude=3.2)
    ox, oz = ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M

    # --- low-corridor barrier: seed 1, spiky, 25.6 km ---
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    p = tc.TraverseParams()
    w1 = win.build_skeleton_window(ox, oz, 1, spec)
    pre = tc.needs_route(v2.compose_windowed_height_v2_full(w1, 1, spec, spiky), spec, p)
    assert pre["needs_route"] is True                            # did-real-work: low-corridor barrier exists
    final1, res1 = tc.compose_with_corridor(w1, 1, spec, p, spiky)
    assert tc.crossing_holds(final1, spec, p) is True            # guarantee: barrier resolved
    assert res1["resolved"] is True
    assert np.max(np.abs(res1["carve_delta"])) <= p.carve_max_m + 1e-9   # minimal/bounded
    s1 = trav.slope_grid(final1, scene_width_m=p.scene_width_m, height_scale_m=p.height_scale_m)
    assert float(np.percentile(s1, 90.0)) >= trav.MIN_STRUCTURAL_SLOPE_P90   # still rugged

    # --- slope-wall-sever barrier: seed 42, gain 3.5, 4 km span ---
    spec_w = ex._window_spec(129, 4000.0)
    pw = dataclasses.replace(tc.TraverseParams(), scene_width_m=4000.0)
    w42 = win.build_skeleton_window(ox, oz, 42, spec_w)
    prew = tc.needs_route(v2.compose_windowed_height_v2_full(w42, 42, spec_w, wall_kp), spec_w, pw)
    assert prew["slope_wall_severs"] is True                     # did-real-work: a wall severs the crossing
    finalw, resw = tc.compose_with_corridor(w42, 42, spec_w, pw, wall_kp)
    assert tc.crossing_holds(finalw, spec_w, pw) is True         # guarantee: wall barrier resolved
    assert resw["resolved"] is True
```

- [ ] **Step 2: Run it to verify it fails**

Run: `python -m pytest test_traverse_corridor.py::test_guarantee_holds_on_both_barrier_types_minimal_and_still_rugged -v`
Expected: FAIL — `AttributeError: ... 'crossing_holds'`.

- [ ] **Step 3: Write minimal implementation**

Append to `traverse_corridor.py`:

```python
def crossing_holds(core_height, spec, p: TraverseParams) -> bool:
    """The guarantee: True iff the post-carve CORE no longer needs a route (the broken crossing — slope-wall
    OR low-corridor, whichever tripped needs_route — is reconnected). Thin wrapper over needs_route_core."""
    return not needs_route_core(np.asarray(core_height, dtype=np.float64), spec, p)["needs_route"]
```

- [ ] **Step 4: Run it**

Run: `python -m pytest test_traverse_corridor.py::test_guarantee_holds_on_both_barrier_types_minimal_and_still_rugged -v`
Expected: PASS.

> If the guarantee fails (`crossing_holds` False after carve), the carve did not bring the crossing under
> budget. Likely the chosen crossing axis was wrong (the barrier blocks NS, not WE) or `carve_max_m` is too
> small for this relief. Fix in `build_traverse_corridor`: try both axes, carve the one whose post-carve
> `crossing_holds`; if neither fits within `carve_max_m`, that window is genuinely impassable at this relief and
> must be reported (the per-game opt-in case, spec §4.3) — not silently passed.

- [ ] **Step 5: Commit**

```bash
git add traverse_corridor.py test_traverse_corridor.py
git commit -m "feat(tier3): connectivity-guarantee gate + still-rugged + minimal-disturbance"
```

---

## Task 10: Tier-3 report (multi-seed, multi-relief did-real-work gate)

**Files:**
- Create: `tools/dem_pack/report_tier3_traversability.py`
- Test: `tools/dem_pack/test_traverse_corridor.py`

Run the gates across multiple seeds and both a gentle and a high-relief config, so the guarantee is proven where barriers actually exist (spec §6 did-real-work).

The report runs over **verified barrier scenarios** (measured this session — memory `worldgen10-tier3-barrier-measurements`): a low-corridor barrier (seed 1, spiky, 25.6 km), a slope-wall-sever barrier (seed 42, gain 3.5, 4 km), AND a gentle no-op (seed 133, default, 25.6 km) to prove verify-first leaves crossable terrain untouched. Each scenario carries its OWN `(spec, params)` because span sets both the window spec and `scene_width_m`. Seeds (133, 1000) at the default span produce NO barrier, so do not use them as the barrier fixture.

- [ ] **Step 1: Write the failing test**

```python
def test_report_runs_scenarios_and_asserts_real_barrier_and_noop():
    import report_tier3_traversability as rep
    summary = rep.run_summary(rep.default_scenarios())
    assert summary["barriers_exercised"] >= 2            # both barrier types routed (low-corridor + wall-sever)
    assert summary["guarantee_failures"] == 0            # guarantee held everywhere it was needed
    assert summary["seam_max_delta"] == 0.0              # seams stayed exact
    assert summary["noop_carved"] == 0                   # gentle no-op carved nothing
```

- [ ] **Step 2: Run it to verify it fails**

Run: `python -m pytest test_traverse_corridor.py::test_report_runs_scenarios_and_asserts_real_barrier_and_noop -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'report_tier3_traversability'`.

- [ ] **Step 3: Write minimal implementation**

Create `report_tier3_traversability.py`:

```python
"""Tier-3 guarantee/seam/minimality gate. Runs verify-then-carve over VERIFIED barrier scenarios (both
barrier types) plus a gentle no-op, so the connectivity guarantee is exercised on terrain that genuinely
blocks (spec §6 did-real-work). Barrier scenarios measured in memory worldgen10-tier3-barrier-measurements.

Run: python report_tier3_traversability.py
"""
from __future__ import annotations
import dataclasses
import numpy as np

import export_godot_rough_world_chunks as ex
import geography_skeleton_windows as win
import keeper_v2 as v2
import traverse_corridor as tc


def _scn(label, seed, span_m, kp, expect_barrier):
    spec = ex._window_spec(129, span_m)
    p = dataclasses.replace(tc.TraverseParams(), scene_width_m=span_m)
    return {"label": label, "seed": seed, "spec": spec, "p": p, "kp": kp, "expect_barrier": expect_barrier}


def default_scenarios():
    spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    wall = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=3.5, relief_amplitude=3.2)
    return [
        _scn("low_corridor_25k", 1, ex.CHUNK_SPAN_M, spiky, True),    # broken valley route
        _scn("wall_sever_4k", 42, 4000.0, wall, True),               # slope-wall severs crossing
        _scn("gentle_noop_25k", 133, ex.CHUNK_SPAN_M, v2.KeeperV2Params(), False),
    ]


def run_summary(scenarios=None) -> dict:
    scenarios = scenarios if scenarios is not None else default_scenarios()
    barriers = failures = noop_carved = 0
    seam_max = max_carve = 0.0
    rows = []
    for s in scenarios:
        spec, p, kp = s["spec"], s["p"], s["kp"]
        ox, oz = ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M
        span = float(spec.core_span_m)
        wa = win.build_skeleton_window(ox, oz, s["seed"], spec)
        wb = win.build_skeleton_window(ox + span, oz, s["seed"], spec)
        final_a, ra = tc.compose_with_corridor(wa, s["seed"], spec, p, kp)
        _, rb = tc.compose_with_corridor(wb, s["seed"], spec, p, kp)
        if ra["needs_route"]:
            barriers += 1
            if not tc.crossing_holds(final_a, spec, p):
                failures += 1
        else:
            if np.count_nonzero(ra["carve_delta"]) != 0:
                noop_carved += 1
        seam = float(np.max(np.abs(ra["carve_delta"][:, -1] - rb["carve_delta"][:, 0])))
        seam_max = max(seam_max, seam)
        max_carve = max(max_carve, float(np.max(np.abs(ra["carve_delta"]))))
        rows.append((s["label"], ra["needs_route"], ra.get("resolved", None), seam))
    return {
        "barriers_exercised": barriers,
        "guarantee_failures": failures,
        "noop_carved": noop_carved,
        "seam_max_delta": seam_max,
        "max_carve": max_carve,
        "rows": rows,
    }


def main() -> None:
    s = run_summary()
    for label, needs, resolved, seam in s["rows"]:
        print(f"  {label:18s} needs_route={needs} resolved={resolved} seam={seam:.6g}")
    print(f"barriers_exercised={s['barriers_exercised']} guarantee_failures={s['guarantee_failures']} "
          f"noop_carved={s['noop_carved']} seam_max_delta={s['seam_max_delta']:.6g} max_carve={s['max_carve']:.3f}")
    ok = (s["barriers_exercised"] >= 2 and s["guarantee_failures"] == 0
          and s["seam_max_delta"] == 0.0 and s["noop_carved"] == 0)
    print(f"[wg10-tier3] status={'pass' if ok else 'FAIL'}")


if __name__ == "__main__":
    main()
```

- [ ] **Step 4: Run it**

Run: `python -m pytest test_traverse_corridor.py::test_report_runs_scenarios_and_asserts_real_barrier_and_noop -v`
Then: `python report_tier3_traversability.py`
Expected: test PASS; CLI prints `[wg10-tier3] status=pass` with `barriers_exercised >= 2`, `noop_carved=0`.

> If a barrier scenario reports `needs_route=False` (the measured fixture drifted) or `resolved=False` (the
> carve couldn't clear the barrier within `carve_max_m`), that is a real finding — do NOT relax the gate.
> Re-measure to refresh the fixture, or (if `resolved=False`) report it as a genuinely-impassable case for the
> per-game opt-in (spec §4.3). A passing gate that never routed a barrier is worthless (spec §6).

- [ ] **Step 5: Commit**

```bash
git add report_tier3_traversability.py test_traverse_corridor.py
git commit -m "feat(tier3): multi-seed multi-relief guarantee/seam gate (did-real-work)"
```

---

## Task 11: Corridors-on owner review sheet

**Files:**
- Create: `tools/dem_pack/render_tier3_corridor.py`

This is the owner-eye artifact (spec §6.1) — it is **not** a gate. It renders, for the high-relief barrier window: the pre-carve terrain, the post-carve terrain, the route line, and the carved cells highlighted, plus the Tier-1-style verdict before/after. Owner judges whether routes read as natural passes and untouched terrain still reads right.

- [ ] **Step 1: Write the renderer**

Create `render_tier3_corridor.py` (mirror the hillshade/oblique idiom already in `render_keeper_v2_compare.py`; reuse its colormap/hillshade helpers by import if present, else a minimal matplotlib hillshade):

```python
"""Render a corridors-on review sheet for owner acceptance (spec §6.1). NOT a gate.

Run: python render_tier3_corridor.py
Writes: D:/tmp/wg10_geography_engine/tier3_corridor_sheet.png
"""
from __future__ import annotations
import dataclasses
from pathlib import Path
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

import export_godot_rough_world_chunks as ex
import geography_skeleton_windows as win
import keeper_v2 as v2
import traverse_corridor as tc

OUT = Path("D:/tmp/wg10_geography_engine/tier3_corridor_sheet.png")


def main() -> None:
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    p = tc.TraverseParams()
    spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    w = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 133, spec)
    pre = v2.compose_windowed_height_v2(w, 133, spec, spiky)
    post, res = tc.compose_with_corridor(w, 133, spec, p, spiky)
    carved = np.abs(res["carve_delta"]) > 1e-6
    fig, ax = plt.subplots(1, 3, figsize=(15, 5))
    ax[0].imshow(pre, cmap="terrain"); ax[0].set_title("pre-carve (high relief)")
    ax[1].imshow(post, cmap="terrain"); ax[1].set_title("post-carve (route guaranteed)")
    ax[2].imshow(post, cmap="terrain")
    ys, xs = np.where(carved)
    ax[2].scatter(xs, ys, s=2, c="red", alpha=0.5); ax[2].set_title("carved cells (red)")
    for a in ax: a.set_xticks([]); a.set_yticks([])
    holds = tc.crossing_holds(post, spec, p)
    fig.suptitle(f"Tier-3 corridor: needs_route={res['needs_route']} carved={res['carved']} "
                 f"crossing_holds={holds} max|carve|={np.max(np.abs(res['carve_delta'])):.4f}")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    fig.tight_layout(); fig.savefig(OUT, dpi=110); print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Run it**

Run: `python render_tier3_corridor.py`
Expected: prints `wrote D:/tmp/wg10_geography_engine/tier3_corridor_sheet.png`; the file exists.

- [ ] **Step 3: Commit**

```bash
git add render_tier3_corridor.py
git commit -m "feat(tier3): corridors-on owner review sheet (pre/post/carved)"
```

- [ ] **Step 4: STOP — owner review checkpoint.** Open the sheet for the owner. Per the pillars, look-quality is owner-judged: do **not** mark Tier-3 accepted, re-freeze fixtures, or flip the Slice-3 blocker from a passed gate. Present the sheet + the `report_tier3_traversability.py` verdict and ask for eye acceptance. If the owner wants a different config/scale, render more panels at their settings.

---

## Task 12: Full suite + docs alignment (after owner accepts)

**Files:**
- Modify: `docs/plans/STATUS.md`, `docs/plans/ROADMAP.md`, `docs/plans/LOOSE_ENDS_LEDGER.md` (B8) — pointer updates only, no fourth source of truth.

- [ ] **Step 1: Run the whole Tier-3 + keeper suite**

Run: `python -m pytest test_traverse_corridor.py test_keeper_v2.py -v`
Expected: all PASS.

- [ ] **Step 2: Run the gate CLI**

Run: `python report_tier3_traversability.py`
Expected: `[wg10-tier3] status=pass`.

- [ ] **Step 3: Update living docs (only after owner eye-accepts the sheet)**

In `STATUS.md` fork-resolution update and `ROADMAP.md` Slice 3 and `LEDGER` B8: flip Tier-3 from "spec written" to "built + owner-accepted; keeper+corridor stack is the Slice-3 port candidate." Do **not** flip if the owner has not accepted — record "built, gates green, owner review pending" instead. Quote the verdict numbers.

- [ ] **Step 4: Commit**

```bash
git add docs/plans/STATUS.md docs/plans/ROADMAP.md docs/plans/LOOSE_ENDS_LEDGER.md
git commit -m "docs(tier3): align living docs to Tier-3 build + owner verdict"
```

---

## Self-Review notes (for the executor)

- **Spec coverage:** §4.1 barrier detection → Task 4; §4.2 verify path → Task 5; §4.3 carve → Task 7; §4.4 `traverse_corridor` fact → Task 7; seam-exactness §6 → Tasks 6,7,10; cross-seam join §6 → (covered by border-delta 0.0 in Task 10; a dedicated `adjacent_corridor_continuity` join test on `route_dist` can be added if the owner wants explicit route-continuity beyond seam-exactness); did-real-work §6 → Task 10; still-rugged §6 → Task 9; owner gate §6.1 → Task 11.
- **Known-open decision:** the gentle-default `needs_route` value depends on `low_corridor_cutoff` (Task 4 note). Resolve by measurement, keep the test honest.
- **Cross-seam join nuance:** Task 10 proves `carve_delta` border == 0.0 (the strong seam guarantee). If the owner wants the *route itself* proven to continue across the seam (not just the delta), add a test applying `win.adjacent_corridor_continuity`'s `_edge_match_count` idiom to a thresholded `route_dist` mask — listed here so it is not forgotten.
- **Performance:** Dijkstra over a 385² padded grid per window is the offline cost (spec §8). Verify-first skips it when `needs_route` is False. If too slow in the multi-seed report, coarsen the path grid (stride the padded grid) and feather onto full — `log()` the coarsening, never hide it.
