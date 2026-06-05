# WG10 carve_ramp → Rust Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `corridor_router.carve_ramp` (turn pass-network routes into a walkable-valley height delta) from offline Python to Rust, so the connected-carve LOOK can be produced live. Gated by a TOLERANCE parity check (Rust delta within a small metres epsilon of the Python delta — the output is Gaussian-smoothed + clamped, so bit-exact is unnecessary).

**Architecture:** Extend the existing `wg-10/rust/src/pass_network/` module (which already has parity-exact routing). Add `carve.rs` with `carve_ramp(...)`. The only genuinely new algorithm is a Euclidean distance transform with nearest-source index (scipy `distance_transform_edt(return_indices=True)`); use the separable exact 1D EDT (Felzenszwalb-Huttenlocher squared-distance), which gives exact distances. The Gaussian floor-smoothing reuses the EXISTING `array_ops::gaussian_filter_nearest` (already parity-proven against scipy mode='nearest' in the biome recipes). All other steps are per-texel math.

**Tech Stack:** Rust (`wg10_terrain` crate, pure, no Godot for the core), Python reference (`tools/dem_pack/corridor_router.py`) as the parity oracle, `cargo test` for the tolerance gate, `pytest`/numpy to emit the fixture.

**Hard constraints:**
- TOLERANCE gate, not bit-exact (owner-decided). The EDT nearest-index tie-break can differ from scipy; that washes out through the Gaussian + clamp. Gate: per-texel `|rust_delta - python_delta|` in metres, p99 below a small epsilon (start by MEASURING it on the first run, then set the budget ~1.5× the measured p99, like the un-intercept ladder's self-baseline; record in the fixture note). Do NOT chase bit-equality.
- Reuse `array_ops::gaussian_filter_nearest` for the floor smooth — do NOT write a new Gaussian.
- All Rust/Python only: NO Godot build, NO editor. Validate with `cd wg-10\rust ; $env:CARGO_TARGET_DIR='D:/tmp/wg10_check_target' ; cargo test -p wg10_terrain <filter>` (PowerShell, from repo root).
- Scoped `git add` of only the task's files (repo has ~250 preexisting dirty files; never `git add -A`, never clean/reset). Commit footer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`. Branch `slice4-gpu-page-integration`.

**Reference code being ported (read first):**
- `tools/dem_pack/corridor_router.py:213-266` — `carve_ramp(full, corridor, spec, p, height_scale_m=1700.0)`. The full algorithm (READ IT). Per route in `corridor["routes"]` (each `route["path"]` = list of (row,col) on the CORE grid):
  1. `along = core_m[path]` (core_m = core*height_scale_m); `prof = along.copy()`; `step = budget*ramp_floor_grade_frac*cell_m`; forward pass `prof[i]=min(prof[i], prof[i-1]+step)` for i in 1..; backward pass `prof[i]=min(prof[i], prof[i+1]+step)` for i in n-2..=0. (A descending-grade floor profile along the route.)
  2. `on_path` bool grid true at path cells; `prof_field` = inf except `prof` at path cells; `distpx,(iy,ix)=distance_transform_edt(~on_path, return_indices=True)`; `floor = gaussian_filter(prof_field[iy,ix], sigma=ramp_floor_smooth_px)`. (Nearest-path-cell profile value, smoothed.)
  3. `d_m = distpx*cell_m`; `wall_rise = clip(d_m - ramp_flat_half_m, 0, None)*(budget*ramp_wall_grade_frac)`; `target = floor + wall_rise`; `band = d_m <= ramp_half_width_m`; `this = where(band, min(target-core_m, 0), 0)`; `delta_m = min(delta_m, this)` (deepest carve wins).
  Finally: `delta_m = clip(delta_m, -ramp_carve_max_m, 0)`; return `delta_m / height_scale_m` (back to height units, CORE grid).
- `CorridorParams` (corridor_router.py:11-33) ramp fields + defaults (VERIFIED): `slope_budget=0.28`, `ramp_floor_grade_frac=0.35`, `ramp_wall_grade_frac=0.80`, `ramp_flat_half_m=200.0`, `ramp_half_width_m=1200.0`, `ramp_floor_smooth_px=5.0`, `ramp_carve_max_m=3500.0`.
- `_core(full, spec)` — for our fixture/use, the carve operates on a single field with no apron (the routing fixture is one continuous field), so `_core` is identity. The Rust `carve_ramp` takes the core `height` grid directly (n×n), NOT an apron-padded `full` — the caller passes the core.
- `array_ops::gaussian_filter_nearest(field, rows, cols, sigma) -> Vec<f64>` (wg-10/rust/src/array_ops.rs:41) — REUSE for step 2's smoothing. Confirm its exact signature when implementing.
- `scipy.ndimage.distance_transform_edt` with `return_indices=True` returns `(distances, (iy, ix))` where for each cell, `(iy[r,c], ix[r,c])` is the index of the NEAREST true cell in the input mask (the input here is `~on_path`, so EDT measures distance to the nearest ON-path cell... NOTE: `distance_transform_edt` measures distance to the nearest ZERO (False) cell. The Python passes `~on_path` — so True (1) = off-path, the transform measures distance from each off-path cell to the nearest path cell (where ~on_path is False). Confirm this semantics when porting: distance to nearest ON-path cell, and the returned index is that nearest path cell. READ scipy docs / test against the Python output to be sure.)

---

## File Structure

**New files:**
- `wg-10/rust/src/pass_network/edt.rs` — `edt_with_indices(mask, rows, cols) -> (Vec<f64> distances, Vec<usize> nearest_idx)`: exact separable EDT giving, for each cell, the Euclidean distance to the nearest "feature" cell and that cell's flat index. (Feature = the ON-path cells; match the Python's `~on_path` convention precisely — see Task 2.)
- `wg-10/rust/src/pass_network/carve.rs` — `carve_ramp(height, n, cell_m, height_scale_m, routes, params) -> Vec<f64>` (the n×n height delta, core grid). Plus a `RampParams` struct (the ramp fields of CorridorParams).
- `tools/dem_pack/export_carve_ramp_fixture.py` — emits a committed fixture: the same routing field + the routes (reuse the routing fixture's field/routes) + the Python `carve_ramp` delta.
- `tools/dem_pack/fixtures/carve_ramp_fixture.json` — committed fixture.

**Modified files:**
- `wg-10/rust/src/pass_network/mod.rs` — add `pub mod edt; pub mod carve;`, a `RampParams` struct (or put it in carve.rs and re-export), and extend the public API: `carve_ramp_delta(height, n, span_m, height_scale_m, routes, ramp_params) -> Vec<f64>`.
- `wg-10/rust/src/pass_network/tests.rs` — add the EDT unit test + the carve_ramp tolerance parity test.

**NOT in this plan (next step after):** wiring the bake (macro+condition+carve+ramp) into the live producer; the GPU port of the ramp; condition_world port. Those follow once carve_ramp is parity-proven on CPU.

---

## Task 1: Emit the carve_ramp Python fixture

**Files:**
- Create: `tools/dem_pack/export_carve_ramp_fixture.py`
- Create (generated): `tools/dem_pack/fixtures/carve_ramp_fixture.json`

- [ ] **Step 1: Write the exporter**

It reuses the SAME field + routes as the routing fixture (so we carve the proven routes), then runs the real `carve_ramp`:
```python
"""Emit a committed parity fixture for the Rust carve_ramp port. Reuses the routing fixture's
field + routes (the proven oracle routes) and records the Python carve_ramp height delta.
Run from repo root: python tools/dem_pack/export_carve_ramp_fixture.py
"""
import json, sys, types
from pathlib import Path
import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import corridor_router as cr  # noqa: E402

ROUTING = Path(__file__).resolve().parent / "fixtures" / "pass_network_routes_fixture.json"
OUT = Path(__file__).resolve().parent / "fixtures" / "carve_ramp_fixture.json"

def main() -> int:
    rf = json.loads(ROUTING.read_text())
    n = int(rf["n"])
    span_m = float(rf["span_m"])
    height_scale_m = float(rf["height_scale_m"])
    height = np.asarray(rf["height"], dtype=np.float64).reshape((n, n))
    routes = [[(int(r), int(c)) for (r, c) in rt] for rt in rf["routes"]]

    cell_m = span_m / (n - 1)
    # carve_ramp takes `full` + spec + a corridor dict {routes:[{path:[...]}]}; _core is identity here
    # (one continuous field, no apron) -- shim _core to identity exactly as mountain_pass_network does.
    import geography_skeleton_windows as win  # noqa: E402
    saved_core, saved_slice = cr._core, win._core_slice
    cr._core = lambda full, spec: np.asarray(full)
    win._core_slice = lambda spec: slice(0, n)
    try:
        spec = types.SimpleNamespace(spacing_m=cell_m, apron_m=0.0, core_span_m=span_m)
        p = cr.CorridorParams()  # defaults (ramp_* as documented)
        corridor = {"routes": [{"path": rt} for rt in routes]}
        delta = cr.carve_ramp(height, corridor, spec, p, height_scale_m=height_scale_m)
    finally:
        cr._core, win._core_slice = saved_core, saved_slice

    delta = np.asarray(delta, dtype=np.float64)
    payload = {
        "n": n, "span_m": span_m, "height_scale_m": height_scale_m,
        "params": {
            "slope_budget": p.slope_budget,
            "ramp_floor_grade_frac": p.ramp_floor_grade_frac,
            "ramp_wall_grade_frac": p.ramp_wall_grade_frac,
            "ramp_flat_half_m": p.ramp_flat_half_m,
            "ramp_half_width_m": p.ramp_half_width_m,
            "ramp_floor_smooth_px": p.ramp_floor_smooth_px,
            "ramp_carve_max_m": p.ramp_carve_max_m,
        },
        "height": height.ravel().tolist(),
        "routes": [[[int(r), int(c)] for (r, c) in rt] for rt in routes],
        "delta": delta.ravel().tolist(),
    }
    OUT.write_text(json.dumps(payload))
    carved = int((delta < -1e-9).sum())
    print(f"[ramp-fixture] wrote {OUT} carved_cells={carved} min_delta={float(delta.min()):.3f} (height units) "
          f"min_delta_m={float(delta.min())*height_scale_m:.1f}")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
```
> Verify when implementing: the `_core`/`win._core_slice` shim is exactly what
> `mountain_pass_network.carve_pass_network` does (corridor_router.py usage ~74-78
> in mountain_pass_network) — copy that shim so `_core` is identity on the single field.

- [ ] **Step 2: Run + confirm non-trivial carve**

From repo root:
```powershell
python tools\dem_pack\export_carve_ramp_fixture.py
```
Expected: `[ramp-fixture] wrote ...carve_ramp_fixture.json carved_cells=<thousands> min_delta=<negative> min_delta_m=<deep, up to -3500>`. carved_cells MUST be > 0 (the routes carve real valleys). If 0, the routes/field don't produce a carve — investigate (shouldn't happen; the routing fixture weaves through over-budget walls).

- [ ] **Step 3: Commit**

```powershell
git add tools/dem_pack/export_carve_ramp_fixture.py tools/dem_pack/fixtures/carve_ramp_fixture.json
git commit -m @'
test(carve): emit Python carve_ramp parity fixture

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

## Task 2: Rust EDT (exact distances + nearest index)

**Files:**
- Create: `wg-10/rust/src/pass_network/edt.rs`
- Modify: `wg-10/rust/src/pass_network/mod.rs` (add `pub mod edt;`)
- Modify: `wg-10/rust/src/pass_network/tests.rs` (EDT unit test)

- [ ] **Step 1: Write the EDT unit test (TDD)**

The semantics to match scipy `distance_transform_edt(input)`: it computes, for each cell, the distance to the nearest ZERO (background) cell, treating nonzero as foreground. The Python calls it on `~on_path` (so on-path cells are False=0=background). Result: each cell's distance to the nearest ON-PATH cell, and the index of that nearest on-path cell. So our Rust `edt_with_indices` should take a `feature` mask (true = the target cells = on-path) and return distance-to-nearest-feature + that feature's index.

```rust
use super::edt::edt_with_indices;

#[test]
fn edt_distance_and_index_on_tiny_grid() {
    // 1x5 row, single feature at col 2. distances = |c-2|*cell? No -- EDT is in PIXELS (cell=1).
    // features at (0,2). distance at (0,0)=2, (0,1)=1, (0,2)=0, (0,3)=1, (0,4)=2. nearest idx all = 2.
    let rows = 1; let cols = 5;
    let mut feat = vec![false; rows*cols];
    feat[2] = true;
    let (dist, idx) = edt_with_indices(&feat, rows, cols);
    let exp = [2.0, 1.0, 0.0, 1.0, 2.0];
    for c in 0..cols { assert!((dist[c]-exp[c]).abs() < 1e-9, "dist[{c}]={} exp {}", dist[c], exp[c]); }
    for c in 0..cols { assert_eq!(idx[c], 2, "nearest idx[{c}]"); }
}

#[test]
fn edt_euclidean_diagonal() {
    // 3x3, single feature at (0,0). distance at (2,2) = sqrt(8) ~ 2.828.
    let rows = 3; let cols = 3;
    let mut feat = vec![false; rows*cols];
    feat[0] = true; // (0,0)
    let (dist, idx) = edt_with_indices(&feat, rows, cols);
    assert!((dist[2*cols+2] - (8.0_f64).sqrt()).abs() < 1e-9, "got {}", dist[2*cols+2]);
    assert_eq!(idx[2*cols+2], 0);
}
```

- [ ] **Step 2: Run, expect FAIL**

```powershell
cd wg-10\rust ; $env:CARGO_TARGET_DIR='D:/tmp/wg10_check_target' ; cargo test -p wg10_terrain pass_network::tests::edt
```

- [ ] **Step 3: Implement `edt.rs`** (separable exact squared-EDT, Felzenszwalb-Huttenlocher, with index tracking)

```rust
//! Exact Euclidean distance transform with nearest-feature index, ported in spirit from
//! scipy.ndimage.distance_transform_edt(return_indices=True). Separable 1D parabola-envelope
//! method (Felzenszwalb & Huttenlocher 2012): exact Euclidean DISTANCES. The nearest-INDEX
//! tie-break may differ from scipy on exact ties, which is acceptable here (the result feeds a
//! Gaussian smooth + clamp downstream, so sub-texel index differences wash out).

const INF: f64 = 1e20;

/// `feature[i]` true = a target cell (distance measured TO the nearest such cell).
/// Returns (distance, nearest_feature_flat_index) per cell, row-major rows*cols.
pub fn edt_with_indices(feature: &[bool], rows: usize, cols: usize) -> (Vec<f64>, Vec<usize>) {
    // f[i] = squared distance seed: 0 at feature cells, INF elsewhere.
    let mut f = vec![INF; rows * cols];
    for i in 0..rows * cols {
        if feature[i] { f[i] = 0.0; }
    }
    // We track the source index alongside the squared distance through both passes.
    // src[i] = flat index of the nearest feature found so far.
    let mut src = vec![usize::MAX; rows * cols];
    for i in 0..rows * cols {
        if feature[i] { src[i] = i; }
    }

    // Pass 1: transform along columns (each column independently, 1D).
    for c in 0..cols {
        let mut col_f = vec![0.0_f64; rows];
        let mut col_src = vec![usize::MAX; rows];
        for r in 0..rows { col_f[r] = f[r * cols + c]; col_src[r] = src[r * cols + c]; }
        let (d, s) = edt_1d(&col_f, &col_src, /*along_stride_is_row=*/true, c, cols);
        for r in 0..rows { f[r * cols + c] = d[r]; src[r * cols + c] = s[r]; }
    }
    // Pass 2: transform along rows.
    for r in 0..rows {
        let mut row_f = vec![0.0_f64; cols];
        let mut row_src = vec![usize::MAX; cols];
        for c in 0..cols { row_f[c] = f[r * cols + c]; row_src[c] = src[r * cols + c]; }
        let (d, s) = edt_1d(&row_f, &row_src, /*along_stride_is_row=*/false, r, cols);
        for c in 0..cols { f[r * cols + c] = d[c]; src[r * cols + c] = s[c]; }
    }

    let dist: Vec<f64> = f.iter().map(|v| v.max(0.0).sqrt()).collect();
    (dist, src)
}

// 1D squared-distance transform of a function `g` (already squared-distance values), carrying the
// source index. Felzenszwalb-Huttenlocher lower-envelope of parabolas. `g[q]` is the seed squared
// distance at position q; the result `d[q]` = min over p of (q-p)^2 + g[p], with src[q] = argmin's source.
fn edt_1d(g: &[f64], gsrc: &[usize], _stride_row: bool, _line: usize, _cols: usize) -> (Vec<f64>, Vec<usize>) {
    let n = g.len();
    let mut d = vec![0.0_f64; n];
    let mut dsrc = vec![usize::MAX; n];
    let mut v = vec![0usize; n];      // locations of parabolas in lower envelope
    let mut z = vec![0.0_f64; n + 1]; // boundaries between parabolas
    let mut k = 0usize;
    v[0] = 0;
    z[0] = -INF;
    z[1] = INF;
    for q in 1..n {
        // intersection of parabola from q with the one at v[k]
        loop {
            let p = v[k];
            // s = ((g[q]+q^2) - (g[p]+p^2)) / (2q - 2p)
            let s = ((g[q] + (q * q) as f64) - (g[p] + (p * p) as f64)) / (2.0 * q as f64 - 2.0 * p as f64);
            if s <= z[k] {
                if k == 0 { break; }
                k -= 1;
            } else {
                k += 1;
                v[k] = q;
                z[k] = s;
                z[k + 1] = INF;
                break;
            }
        }
    }
    k = 0;
    for q in 0..n {
        while z[k + 1] < q as f64 { k += 1; }
        let p = v[k];
        let dq = (q as f64 - p as f64);
        d[q] = dq * dq + g[p];
        dsrc[q] = gsrc[p];
    }
    (d, dsrc)
}
```
> NOTE the index handling: `edt_1d` carries `gsrc[p]` (the source from the prior pass's winning
> position). After pass 1 (columns), src holds the nearest feature within each column; pass 2 (rows)
> then picks the column whose carried source is globally nearest — so `src` ends as the 2D nearest
> feature index. This is the standard separable-EDT index propagation. The unit tests in Step 1
> verify both distance AND index on known grids; if the diagonal index test fails, the index
> propagation through the two passes needs review (the distance is the easy part; the index is the
> subtle part). VERIFY against the tests before moving on.

- [ ] **Step 4: Run EDT tests, expect PASS**

```powershell
cd wg-10\rust ; $env:CARGO_TARGET_DIR='D:/tmp/wg10_check_target' ; cargo test -p wg10_terrain pass_network::tests::edt
```
If the index test fails but distances pass: the separable index propagation is the issue — debug `edt_1d`'s `dsrc` carry. (Distances are well-tested FH; the index carry is the part to get right.)

- [ ] **Step 5: Commit**

```powershell
git add wg-10/rust/src/pass_network/edt.rs wg-10/rust/src/pass_network/mod.rs wg-10/rust/src/pass_network/tests.rs
git commit -m @'
feat(carve): rust exact EDT with nearest-feature index

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

## Task 3: Rust carve_ramp + tolerance parity gate

**Files:**
- Create: `wg-10/rust/src/pass_network/carve.rs`
- Modify: `wg-10/rust/src/pass_network/mod.rs` (add `pub mod carve;` + `RampParams` + public `carve_ramp_delta`)
- Modify: `wg-10/rust/src/pass_network/tests.rs` (the tolerance parity test)

- [ ] **Step 1: Add `RampParams` to mod.rs (defaults verified from CorridorParams)**

```rust
/// Ramp-carve params: the ramp_* fields of Python CorridorParams (corridor_router.py:11-33). Same defaults.
#[derive(Clone, Copy, Debug)]
pub struct RampParams {
    pub slope_budget: f64,
    pub floor_grade_frac: f64,
    pub wall_grade_frac: f64,
    pub flat_half_m: f64,
    pub half_width_m: f64,
    pub floor_smooth_px: f64,
    pub carve_max_m: f64,
}
impl Default for RampParams {
    fn default() -> Self {
        Self {
            slope_budget: 0.28,
            floor_grade_frac: 0.35,
            wall_grade_frac: 0.80,
            flat_half_m: 200.0,
            half_width_m: 1200.0,
            floor_smooth_px: 5.0,
            carve_max_m: 3500.0,
        }
    }
}
```

- [ ] **Step 2: Implement `carve.rs`** (mirror carve_ramp lines 226-266; reuse gaussian + edt)

```rust
//! carve_ramp port (corridor_router.py:213-266): turn pass-network routes into a walkable-valley
//! height DELTA (<=0). Per route: a reduced-grade floor profile along the route, scattered + smoothed
//! to a floor field via nearest-on-path EDT, graded walls rising away, banded + clamped. Deepest carve
//! wins where routes overlap. Reuses array_ops::gaussian_filter_nearest (scipy mode='nearest' parity)
//! and pass_network::edt. Operates on the CORE n*n grid (no apron). Returns the delta in HEIGHT units.

use crate::array_ops::gaussian_filter_nearest;
use super::edt::edt_with_indices;
use super::RampParams;

/// routes: each a slice of (row,col) on the core grid. height: core n*n row-major (height units).
pub fn carve_ramp(height: &[f64], n: usize, cell_m: f64, height_scale_m: f64, routes: &[Vec<(usize, usize)>], p: &RampParams) -> Vec<f64> {
    let core_m: Vec<f64> = height.iter().map(|h| h * height_scale_m).collect();
    let mut delta_m = vec![0.0_f64; n * n];
    let budget = p.slope_budget;
    if routes.iter().all(|r| r.is_empty()) {
        return vec![0.0_f64; n * n];
    }
    for route in routes {
        if route.is_empty() { continue; }
        // 1) reduced-grade floor profile along the route (forward then backward min-pass).
        let m = route.len();
        let mut prof: Vec<f64> = route.iter().map(|&(r, c)| core_m[r * n + c]).collect();
        let step = budget * p.floor_grade_frac * cell_m;
        for i in 1..m { prof[i] = prof[i].min(prof[i - 1] + step); }
        for i in (0..m - 1).rev() { prof[i] = prof[i].min(prof[i + 1] + step); }
        // 2) scatter to a field, nearest-on-path EDT, smooth.
        let mut on_path = vec![false; n * n];
        let mut prof_field = vec![f64::INFINITY; n * n];
        for (k, &(r, c)) in route.iter().enumerate() {
            on_path[r * n + c] = true;
            prof_field[r * n + c] = prof[k];
        }
        let (distpx, nearest) = edt_with_indices(&on_path, n, n);
        // prof_field[nearest[i]] is the profile value of the nearest on-path cell (the EDT's iy/ix gather).
        let gathered: Vec<f64> = (0..n * n).map(|i| prof_field[nearest[i]]).collect();
        let floor = gaussian_filter_nearest(&gathered, n, n, p.floor_smooth_px);
        // 3) walls + band + deepest-wins.
        for i in 0..n * n {
            let d_m = distpx[i] * cell_m;
            let wall_rise = (d_m - p.flat_half_m).max(0.0) * (budget * p.wall_grade_frac);
            let target = floor[i] + wall_rise;
            if d_m <= p.half_width_m {
                let this = (target - core_m[i]).min(0.0);
                if this < delta_m[i] { delta_m[i] = this; }
            }
        }
    }
    // clamp to [-carve_max_m, 0], back to height units.
    for v in delta_m.iter_mut() { *v = v.clamp(-p.carve_max_m, 0.0) / height_scale_m; }
    delta_m
}
```
> Confirm `gaussian_filter_nearest`'s exact signature (array_ops.rs:41) — arg order
> `(field, rows, cols, sigma)` assumed; match the real one. The `gathered` step is the
> Rust equivalent of numpy's `prof_field[iy, ix]` fancy-index gather: for each cell i,
> take the profile value at its nearest on-path cell (`nearest[i]`).

- [ ] **Step 3: Add the public entry to mod.rs**

```rust
pub mod carve;
// ...
/// Public entry: carve_ramp delta for a core height grid + routes. Delta is height units, n*n.
pub fn carve_ramp_delta(height: &[f64], n: usize, span_m: f64, height_scale_m: f64, routes: &[Vec<(usize, usize)>], ramp: &RampParams) -> Vec<f64> {
    let cell_m = span_m / (n - 1) as f64;
    carve::carve_ramp(height, n, cell_m, height_scale_m, routes, ramp)
}
```

- [ ] **Step 4: Write the tolerance parity test (loads the Task-1 fixture)**

```rust
#[test]
fn carve_ramp_matches_python_within_tolerance() {
    use std::path::Path;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/dem_pack/fixtures/carve_ramp_fixture.json");
    let raw = std::fs::read_to_string(&path).expect("read ramp fixture");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("parse");
    let n = v["n"].as_u64().unwrap() as usize;
    let span_m = v["span_m"].as_f64().unwrap();
    let height_scale_m = v["height_scale_m"].as_f64().unwrap();
    let height: Vec<f64> = v["height"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let want: Vec<f64> = v["delta"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let routes: Vec<Vec<(usize,usize)>> = v["routes"].as_array().unwrap().iter().map(|rt|
        rt.as_array().unwrap().iter().map(|p| { let a = p.as_array().unwrap(); (a[0].as_u64().unwrap() as usize, a[1].as_u64().unwrap() as usize) }).collect()
    ).collect();
    let rp = super::RampParams {
        slope_budget: v["params"]["slope_budget"].as_f64().unwrap(),
        floor_grade_frac: v["params"]["ramp_floor_grade_frac"].as_f64().unwrap(),
        wall_grade_frac: v["params"]["ramp_wall_grade_frac"].as_f64().unwrap(),
        flat_half_m: v["params"]["ramp_flat_half_m"].as_f64().unwrap(),
        half_width_m: v["params"]["ramp_half_width_m"].as_f64().unwrap(),
        floor_smooth_px: v["params"]["ramp_floor_smooth_px"].as_f64().unwrap(),
        carve_max_m: v["params"]["ramp_carve_max_m"].as_f64().unwrap(),
    };
    let got = super::carve_ramp_delta(&height, n, span_m, height_scale_m, &routes, &rp);
    assert_eq!(got.len(), want.len());
    // Tolerance gate (metres). The delta is height units; compare in METRES = *height_scale_m.
    let mut diffs: Vec<f64> = (0..got.len()).map(|i| ((got[i]-want[i]) * height_scale_m).abs()).collect();
    diffs.sort_by(|a,b| a.partial_cmp(b).unwrap());
    let n_d = diffs.len();
    let mean = diffs.iter().sum::<f64>() / n_d as f64;
    let p99 = diffs[((n_d as f64)*0.99) as usize];
    let peak = *diffs.last().unwrap();
    // non-vacuous: the Python actually carved (some want < 0).
    let carved = want.iter().filter(|d| **d < -1e-9).count();
    println!("[ramp-parity] carved_cells={carved} mean_m={mean:.4} p99_m={p99:.4} peak_m={peak:.4}");
    assert!(carved > 0, "fixture vacuous: nothing carved");
    // TOLERANCE: set on first run from the measured p99 (see plan). Start generous, then tighten.
    // The output is Gaussian-smoothed + clamped, so EDT index tie-break diffs should keep p99 small
    // (single-digit metres on a 1700m-relief field). If p99 is HUGE (>~50m), the port has a real bug
    // (wrong EDT semantics / wrong gather / wrong profile pass) -- debug, don't just widen the budget.
    let p99_budget_m = 50.0; // GENEROUS first-run ceiling; record actual p99 + tighten to ~1.5x measured.
    assert!(p99 < p99_budget_m, "carve_ramp p99 {p99:.4} m exceeds {p99_budget_m} m -- likely a real bug, not tie-break noise");
}
```

- [ ] **Step 5: Run the parity test (--nocapture to see the numbers)**

```powershell
cd wg-10\rust ; $env:CARGO_TARGET_DIR='D:/tmp/wg10_check_target' ; cargo test -p wg10_terrain pass_network::tests::carve_ramp_matches_python -- --nocapture
```
Expected: `[ramp-parity] carved_cells=<thousands> mean_m=<small> p99_m=<small> peak_m=<maybe larger at a few edge cells>`. 
- If p99 is single-digit metres: GREAT — the port matches within smoothing/tie-break noise. Record the actual p99 in a comment + tighten `p99_budget_m` to ~1.5× the measured value, re-run, commit.
- If p99 is large (>~50m) or peak is enormous: a REAL bug. Debug in order: (a) EDT semantics — is distance measured to nearest ON-path cell (Python `~on_path`)? print a few EDT values vs a hand calc; (b) the `gathered` nearest-profile step — is `nearest[i]` indexing the right cell? (c) the profile forward/backward passes; (d) the gaussian sigma/mode. Do NOT widen the budget to pass — fix the bug.

- [ ] **Step 6: Tighten the budget + commit**

After observing the real p99, set `p99_budget_m` to ~1.5× it (e.g. measured 3m → budget 5m), re-run to confirm green, then:
```powershell
git add wg-10/rust/src/pass_network/carve.rs wg-10/rust/src/pass_network/mod.rs wg-10/rust/src/pass_network/tests.rs
git commit -m @'
feat(carve): rust carve_ramp + tolerance parity gate (delta within Nm of Python)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

- [ ] **Step 7: Full lib no-regression**

```powershell
cd wg-10\rust ; $env:CARGO_TARGET_DIR='D:/tmp/wg10_check_target' ; cargo test -p wg10_terrain --lib
```
Expect all prior (241) + the new EDT + carve_ramp tests green.

---

## Self-Review notes (author)

- **Spec coverage:** carve_ramp port → Task 3; the one new algorithm (EDT) → Task 2 (isolated + unit-tested for distance AND index); fixture/oracle → Task 1; tolerance gate (owner-decided) → Task 3 Step 5-6 with a self-baselined budget + "huge p99 = real bug, don't widen" guard. Reuse of gaussian_filter_nearest (don't rewrite) → carve.rs imports it. All Rust/Python-only, scoped commits.
- **Placeholder scan:** the one deferred value is `p99_budget_m` (measured on first run, then tightened) — that's deliberate self-baselining, not a placeholder, and it has a hard "huge = bug" backstop so it can't silently pass garbage. EDT/carve code is complete.
- **Type consistency:** `carve_ramp_delta`/`carve_ramp`/`edt_with_indices`/`RampParams` signatures consistent across mod.rs, carve.rs, edt.rs, tests. routes type `&[Vec<(usize,usize)>]` matches the routing port's output type.
- **The one real risk** is the EDT nearest-INDEX propagation through the separable passes (distances are textbook-exact; the index carry is the subtle part). Task 2's index unit tests gate it before carve_ramp depends on it, and the tolerance gate absorbs residual index tie-break differences vs scipy — which is exactly why the owner chose a tolerance gate.
