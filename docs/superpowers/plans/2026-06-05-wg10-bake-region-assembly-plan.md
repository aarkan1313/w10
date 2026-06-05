# WG10 bake_region Assembly Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Assemble the three already-ported pieces (seam-safe mountain macro + carve + condition_world) into one pure-Rust `bake_region`, and prove the ASSEMBLED conditioned+carved region height matches a Python seam-safe oracle end-to-end. This is the last pure-Rust step before the live-producer integration — landing it means the whole offline look-pipeline is reproduced in Rust, verified.

**Architecture:** A new `wg-10/rust/src/bake_region.rs` that composes, IN ORDER: `recipes::mountain_seamsafe` (raw macro, seam-safe branch) → `pass_network::carve_routes` → `pass_network::carve_ramp_delta` (carve delta on the RAW field) → `raw + delta` → `condition_world::condition_world` (normalize the carved field). Gated against a Python fixture running the SAME seam-safe pipeline (NOT `build_network_world`'s full-field branch — different by design; live runtime uses seam-safe).

**Tech Stack:** Rust (`wg10_terrain`, pure, no Godot), Python reference (`tools/dem_pack/`, mountain_synthesis + mountain_pass_network + mountain_world_layer.condition_world) as the oracle, `cargo test` (isolated target) for the gate, pytest/numpy to emit the fixture.

**Hard constraints:**
- Compose the SEAM-SAFE branch (`mountain.generate(apron_px>0)` / Rust `mountain_seamsafe`), per the approved spec. Do NOT port or gate against the full-field branch.
- ORDER is load-bearing: carve on RAW macro, THEN condition_world (Python `build_network_world:485-488`: `raw_carved = raw + carve.delta; condition_world(raw_carved)`).
- TOLERANCE gate (per-piece residuals flow through: carve_ramp EDT-tie ~0, condition gaussian-border ~1e-4 tanh). Self-baseline the budget from the first measured p99 ×1.5, with a "huge p99 = real assembly bug" backstop. Stats percentiles match ~1e-9.
- All Rust/Python only: NO Godot build, NO editor. Validate: `$env:CARGO_TARGET_DIR='D:/tmp/wg10_check_target'; Push-Location 'D:\workflows\worldgen10\wg-10\rust'; cargo test -p wg10_terrain <filter>; Pop-Location` (ABSOLUTE path — shell cwd drifts).
- Scoped `git add` per file group (repo has ~250 preexisting dirty files; NEVER `git add -A`, never clean/reset). Footer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`. Branch `slice4-gpu-page-integration`.

**Verified API facts (read from source 2026-06-05):**
- Rust macro: `recipes::mountain_seamsafe(wx: &[f64], wz: &[f64], rows: usize, cols: usize, seed: i64, feature_span_m: f64, apron_px: usize, spacing_m: f64, flow_on: bool) -> Vec<f64>` (recipes.rs:41; returns the CORE n×n, apron already cropped). Uses `mountain::ALPINE_BRANCHING` style.
- Rust carve: `pass_network::carve_routes(height: &[f64], n, span_m, height_scale_m, &PassNetworkParams, &TraverseParams) -> Vec<Vec<(usize,usize)>>` and `pass_network::carve_ramp_delta(height: &[f64], n, span_m, height_scale_m, &routes, &RampParams) -> Vec<f64>` (delta, height units, n*n).
- Rust condition: `condition_world::condition_world(z: &[f64], n: usize) -> (Vec<f64>, ConditionStats)`.
- Python seam-safe macro invocation: `mountain_synthesis.py` `_live_seamsafe_page` pattern (test_mountain_world_layer_contract.py:35-63): `apron_px = mountain.MOUNTAIN_APRON_PX`; `padded_n = SAMPLE_N + 2*apron_px`; `wx,wz = mountain.grid(padded_n, padded_span_m, ox=origin - apron*spacing, oz=...)`; `result = mountain.generate(wx, wz, seed, style=mountain.STYLES[0], feature_span_m, apron_px, spacing_m, flow_on=True)`; `result["height"]` is the CORE (SAMPLE_N×SAMPLE_N, apron cropped).
- Python carve: `mountain_pass_network.carve_pass_network(raw_core, span_m, height_scale_m, pp) -> {delta, ...}` (operates on the core field; the `_core` identity shim is NOT needed when passing an already-core field of the right size — BUT confirm: carve_pass_network internally calls `_core(full, spec)` via `_routes`; for a standalone core field use the same identity shim as the carve fixture exporter `export_carve_ramp_fixture.py`). Returns `delta` (height units, core-sized).
- `condition_world(raw + delta)` → `(shaped, stats)`.
- Constants: `FEATURE_SPAN_M=90000.0`, `HEIGHT_SCALE_M=1700.0`, `mountain.STYLES[0]` == ALPINE_BRANCHING (matches Rust). `mountain.MOUNTAIN_APRON_PX` — READ its value.

---

## File Structure

**New:**
- `tools/dem_pack/export_bake_region_fixture.py` — runs the Python seam-safe pipeline over one known region; emits the fixture.
- `tools/dem_pack/fixtures/bake_region_fixture.json` — the oracle.
- `wg-10/rust/src/bake_region.rs` — `bake_region(...) -> BakeResult`.
- `wg-10/rust/src/bake_region_tests.rs` — end-to-end parity gate.

**Modified:**
- `wg-10/rust/src/lib.rs` — `mod bake_region;` + `#[cfg(test)] mod bake_region_tests;`.

**Out of scope (next session):** producer wiring, region-fact LRU, GPU/CPU coordination, cross-region condition seam, full-field branch port. (Spec §"Out of scope".)

---

## Task 1: Emit the Python seam-safe bake_region fixture

**Files:**
- Create: `tools/dem_pack/export_bake_region_fixture.py`
- Create (generated): `tools/dem_pack/fixtures/bake_region_fixture.json`

- [ ] **Step 1: Write the exporter** — mirror the seam-safe macro invocation, then carve + condition.

```python
"""Emit a committed parity fixture for the Rust bake_region assembly: the SEAM-SAFE pipeline
(mountain.generate apron_px>0 -> carve_pass_network -> condition_world), the path the live runtime
uses. NOT build_network_world (that's the full-field branch). Run from repo root:
  python tools/dem_pack/export_bake_region_fixture.py
"""
import json, sys, types
from pathlib import Path
import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import mountain_synthesis as mountain  # noqa: E402
import mountain_pass_network as mpn      # noqa: E402
import mountain_world_layer as L         # noqa: E402

OUT = Path(__file__).resolve().parent / "fixtures" / "bake_region_fixture.json"

# A known region: a single seam-safe page-sized core. Keep n moderate so the fixture isn't huge
# but big enough that carve routes weave + condition percentiles are meaningful.
SAMPLE_N = 193
FEATURE_SPAN_M = 90000.0
HEIGHT_SCALE_M = 1700.0
SEED = 177
SOURCE_SPAN_M = 270000.0           # the accepted source-window span for this region
SOURCE_ORIGIN_X = 207000.0
SOURCE_ORIGIN_Z = 176000.0

def main() -> int:
    apron_px = int(mountain.MOUNTAIN_APRON_PX)
    spacing_m = SOURCE_SPAN_M / float(SAMPLE_N - 1)
    padded_n = SAMPLE_N + 2 * apron_px
    padded_span_m = SOURCE_SPAN_M + 2.0 * float(apron_px) * spacing_m
    wx, wz = mountain.grid(
        padded_n, padded_span_m,
        ox=SOURCE_ORIGIN_X - float(apron_px) * spacing_m,
        oz=SOURCE_ORIGIN_Z - float(apron_px) * spacing_m,
    )
    result = mountain.generate(
        wx, wz, seed=SEED, style=mountain.STYLES[0],
        feature_span_m=FEATURE_SPAN_M, apron_px=apron_px, spacing_m=spacing_m, flow_on=True,
    )
    raw = np.asarray(result["height"], dtype=np.float64)   # CORE SAMPLE_N x SAMPLE_N
    n = raw.shape[0]
    assert n == SAMPLE_N, f"core n {n} != SAMPLE_N {SAMPLE_N}"

    # carve on the core. carve_pass_network's _routes calls _core(full, spec); for a standalone
    # core, shim _core to identity (same as export_carve_ramp_fixture.py).
    import corridor_router as cr  # noqa: E402
    import geography_skeleton_windows as win  # noqa: E402
    pp = mpn.PassNetworkParams()  # defaults n_we=4,n_ns=4,coarse_n=193 (match the carve fixture/Rust defaults)
    saved_core, saved_slice = cr._core, win._core_slice
    cr._core = lambda full, spec: np.asarray(full)
    win._core_slice = lambda spec: slice(0, n)
    try:
        carved = mpn.carve_pass_network(raw, span_m=SOURCE_SPAN_M, height_scale_m=HEIGHT_SCALE_M, pp=pp)
    finally:
        cr._core, win._core_slice = saved_core, saved_slice
    delta = np.asarray(carved["delta"], dtype=np.float64)
    raw_carved = raw + delta

    height, stats = L.condition_world(raw_carved)
    height = np.asarray(height, dtype=np.float64)

    payload = {
        "n": n, "span_m": SOURCE_SPAN_M, "height_scale_m": HEIGHT_SCALE_M, "seed": SEED,
        "feature_span_m": FEATURE_SPAN_M, "apron_px": apron_px, "spacing_m": spacing_m,
        "source_origin_x_m": SOURCE_ORIGIN_X, "source_origin_z_m": SOURCE_ORIGIN_Z,
        "params": {
            "n_we": pp.n_we, "n_ns": pp.n_ns, "coarse_n": pp.coarse_n,
            # carve_pass_network derives p_trav internally; capture the ramp/traverse scalars it uses.
            # READ carve_pass_network to see which TraverseParams/CorridorParams it builds and capture
            # the exact values (slope_budget, slope_penalty, drainage_bias, and the ramp_* fields).
        },
        "raw": raw.ravel().tolist(),
        "carve_delta": delta.ravel().tolist(),
        "height": height.ravel().tolist(),
        "stats": {k: float(v) for k, v in stats.items()},
    }
    OUT.write_text(json.dumps(payload))
    carved_cells = int((delta < -1e-9).sum())
    print(f"[bake-fixture] wrote {OUT} n={n} carved_cells={carved_cells} "
          f"height_range=[{height.min():.4f},{height.max():.4f}] p50={stats['p50']:.4f}")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
```
> CRITICAL when implementing: READ `mountain_pass_network.carve_pass_network` (corridor_router /
> mountain_pass_network) to find the EXACT `TraverseParams`/`CorridorParams` it constructs (slope_budget,
> slope_penalty, drainage_bias, ramp_floor_grade_frac, ramp_wall_grade_frac, ramp_flat_half_m,
> ramp_half_width_m, ramp_floor_smooth_px, ramp_carve_max_m). The Rust `bake_region` must build the SAME
> param values (the prior carve fixtures captured them: slope_budget=0.28, slope_penalty=24.0,
> drainage_bias=0.55; ramp defaults floor_grade_frac=0.35, wall_grade_frac=0.80, flat_half_m=200,
> half_width_m=1200, floor_smooth_px=5, carve_max_m=3500). Put ALL of them in `params` so the Rust gate
> constructs identical PassNetworkParams/TraverseParams/RampParams. If carve_pass_network uses different
> values than the standalone defaults, capture WHAT IT ACTUALLY USES.

- [ ] **Step 2: Run + confirm non-trivial** — `python tools\dem_pack\export_bake_region_fixture.py`. Expect `[bake-fixture] ... carved_cells=<hundreds-thousands> height_range=[~-1,~1]` (condition_world tanh-bounds to ~[-1,1]). carved_cells>0 AND height in ~[-1,1]. If height isn't ~[-1,1], condition_world didn't run / wrong order.

- [ ] **Step 3: Commit** — `git add tools/dem_pack/export_bake_region_fixture.py tools/dem_pack/fixtures/bake_region_fixture.json` then commit `test(bake): emit Python seam-safe bake_region parity fixture`.

---

## Task 2: Rust bake_region + end-to-end parity gate

**Files:**
- Create: `wg-10/rust/src/bake_region.rs`
- Modify: `wg-10/rust/src/lib.rs` (`mod bake_region;` + `#[cfg(test)] mod bake_region_tests;`)
- Create: `wg-10/rust/src/bake_region_tests.rs`

- [ ] **Step 1: Implement `bake_region.rs`**

```rust
//! bake_region: assemble the seam-safe "baked look" pipeline in Rust end-to-end.
//! macro (seam-safe) -> carve routes -> carve_ramp delta (on RAW) -> raw+delta -> condition_world.
//! ORDER is load-bearing: carve on RAW, THEN condition (mountain_world_layer.build_network_world:485-488).
//! Pure Rust. The path the live runtime uses (seam-safe branch), NOT the offline full-field artifact.

use crate::condition_world::{condition_world, ConditionStats};
use crate::pass_network::{carve_ramp_delta, carve_routes, PassNetworkParams, RampParams, TraverseParams};
use crate::recipes::mountain_seamsafe;

pub struct BakeResult {
    pub height: Vec<f64>,      // conditioned + carved, n*n core
    pub carve_delta: Vec<f64>, // the carve delta (height units), n*n core
    pub stats: ConditionStats,
}

/// wx/wz are the PADDED (apron-included) world-coord grids for mountain_seamsafe; n is the CORE side
/// (mountain_seamsafe crops apron and returns the n*n core). span_m/height_scale_m are the CORE region's.
#[allow(clippy::too_many_arguments)]
pub fn bake_region(
    wx: &[f64], wz: &[f64], n: usize,
    seed: i64, feature_span_m: f64, apron_px: usize, spacing_m: f64,
    span_m: f64, height_scale_m: f64, flow_on: bool,
    pass: &PassNetworkParams, traverse: &TraverseParams, ramp: &RampParams,
) -> BakeResult {
    // 1) seam-safe macro -> core n*n raw.
    let raw = mountain_seamsafe(wx, wz, n, n, seed, feature_span_m, apron_px, spacing_m, flow_on);
    // 2) carve on RAW: routes -> ramp delta.
    let routes = carve_routes(&raw, n, span_m, height_scale_m, pass, traverse);
    let carve_delta = carve_ramp_delta(&raw, n, span_m, height_scale_m, &routes, ramp);
    // 3) raw + delta.
    let raw_carved: Vec<f64> = raw.iter().zip(carve_delta.iter()).map(|(r, d)| r + d).collect();
    // 4) condition the carved field.
    let (height, stats) = condition_world(&raw_carved, n);
    BakeResult { height, carve_delta, stats }
}
```
> Confirm the real `pass_network` re-exports (`carve_routes`, `carve_ramp_delta`, `PassNetworkParams`,
> `RampParams`, `TraverseParams`) are `pub use`/`pub` from `pass_network::mod`. If `TraverseParams` is
> only in a submodule, use its real path. Adjust imports to what actually exists.

- [ ] **Step 2: Write the end-to-end parity gate** (`bake_region_tests.rs`)

Load `bake_region_fixture.json`; rebuild the PADDED wx/wz grid in Rust the SAME way the Python did
(`mountain.grid(padded_n, padded_span_m, ox, oz)` — port that grid construction: padded_n = n + 2*apron_px,
padded_span_m = span_m + 2*apron_px*spacing_m, ox = source_origin_x - apron_px*spacing_m, etc.; the grid is
linspace-style world coords — READ `mountain.grid` to match exactly). Build params from the fixture. Run
`bake_region`. Compare `height` to the fixture `height` (tolerance, metres = *height_scale_m), `carve_delta`
to fixture `carve_delta`, and `stats` p05/p50/p95 to ~1e-9.

```rust
#[test]
fn bake_region_matches_python_seamsafe_pipeline() {
    use std::path::Path;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/dem_pack/fixtures/bake_region_fixture.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");
    let n = v["n"].as_u64().unwrap() as usize;
    let span_m = v["span_m"].as_f64().unwrap();
    let hs = v["height_scale_m"].as_f64().unwrap();
    let seed = v["seed"].as_i64().unwrap();
    let feature_span_m = v["feature_span_m"].as_f64().unwrap();
    let apron_px = v["apron_px"].as_u64().unwrap() as usize;
    let spacing_m = v["spacing_m"].as_f64().unwrap();
    let ox = v["source_origin_x_m"].as_f64().unwrap();
    let oz = v["source_origin_z_m"].as_f64().unwrap();
    // Rebuild the padded grid exactly as mountain.grid did (PORT mountain.grid's coord formula).
    let padded_n = n + 2 * apron_px;
    let padded_span_m = span_m + 2.0 * apron_px as f64 * spacing_m;
    let gox = ox - apron_px as f64 * spacing_m;
    let goz = oz - apron_px as f64 * spacing_m;
    let (wx, wz) = build_grid(padded_n, padded_span_m, gox, goz); // helper mirroring mountain.grid
    let pass = super::pass_network::PassNetworkParams {
        n_we: v["params"]["n_we"].as_u64().unwrap() as usize,
        n_ns: v["params"]["n_ns"].as_u64().unwrap() as usize,
        coarse_n: v["params"]["coarse_n"].as_u64().unwrap() as usize,
    };
    let traverse = super::pass_network::TraverseParams { /* from fixture params: slope_budget/penalty/bias, scene_width_m=span_m, height_scale_m=hs */ ..Default::default() };
    let ramp = super::pass_network::RampParams { /* from fixture params */ ..Default::default() };
    let got = super::bake_region::bake_region(&wx, &wz, n, seed, feature_span_m, apron_px, spacing_m, span_m, hs, true, &pass, &traverse, &ramp);

    let want_h: Vec<f64> = v["height"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let want_d: Vec<f64> = v["carve_delta"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert_eq!(got.height.len(), want_h.len());
    let mut hd: Vec<f64> = (0..want_h.len()).map(|i| ((got.height[i]-want_h[i])*hs).abs()).collect();
    hd.sort_by(|a,b| a.partial_cmp(b).unwrap());
    let hmean = hd.iter().sum::<f64>()/hd.len() as f64;
    let hp99 = hd[((hd.len() as f64)*0.99) as usize];
    let hpeak = *hd.last().unwrap();
    let mut dd: Vec<f64> = (0..want_d.len()).map(|i| ((got.carve_delta[i]-want_d[i])*hs).abs()).collect();
    dd.sort_by(|a,b| a.partial_cmp(b).unwrap());
    let dp99 = dd[((dd.len() as f64)*0.99) as usize];
    let carved = want_d.iter().filter(|d| **d < -1e-9).count();
    let p05_d = (got.stats.p05 - v["stats"]["p05"].as_f64().unwrap()).abs();
    let p50_d = (got.stats.p50 - v["stats"]["p50"].as_f64().unwrap()).abs();
    let p95_d = (got.stats.p95 - v["stats"]["p95"].as_f64().unwrap()).abs();
    println!("[bake-parity] carved={carved} height mean_m={hmean:.4} p99_m={hp99:.4} peak_m={hpeak:.4} | carve_delta p99_m={dp99:.4} | p05d={p05_d:.2e} p50d={p50_d:.2e} p95d={p95_d:.2e}");
    assert!(carved > 0, "vacuous fixture");
    // stats percentiles are on the carved field; they match only if raw+carve_delta match -> ~1e-9 if so.
    assert!(p05_d < 1e-6 && p50_d < 1e-6 && p95_d < 1e-6, "condition stats diverge -> raw or carve differs");
    let hp99_budget = 2.0; // GENEROUS first ceiling (metres); record actual + tighten to ~1.5x.
    assert!(hp99 < hp99_budget, "bake height p99 {hp99:.4}m > {hp99_budget}m -- assembly bug (order/param/grid), not residual noise");
}
```
> The `build_grid` helper + the `TraverseParams`/`RampParams` field fills are the parts to get right.
> READ `mountain.grid` (mountain_synthesis or a shared util) to port the coord formula EXACTLY — if the
> raw macro grid is off, EVERYTHING downstream diverges. The stats-percentile assert is a sharp early
> signal: if p05/p50/p95 diverge, the raw+carve composition is wrong (condition saw a different field);
> if stats match but height p99 is large, it's the condition step. Debug accordingly.

- [ ] **Step 3: Run the gate** (`-- --nocapture`). Expect `[bake-parity] carved=<n> height ... p99_m=<small>`.
   - Stats match (p05/p50/p95 ~1e-9) + height p99 small (single-digit metres, since carve_ramp p99=0 and condition interior is bit-exact; the residual is condition's border ring) → SUCCESS. Tighten `hp99_budget` to ~1.5× measured, commit.
   - Stats DIVERGE → the raw macro or carve_delta differs (the assembly fed condition a wrong field). Debug: (a) is `build_grid` matching mountain.grid? compare a few wx/wz vs the fixture's implied coords; (b) does the raw macro match (add a temp assert on raw vs fixture `raw`); (c) carve params match? This is where an assembly bug shows.
   - Stats match but height p99 large → condition step issue (sigma/order). Debug condition call.
   - Do NOT widen the budget to pass a real divergence.

- [ ] **Step 4: Full lib no-regression** — `cargo test -p wg10_terrain --lib` (expect 250 + new).

- [ ] **Step 5: Commit** — `git add wg-10/rust/src/bake_region.rs wg-10/rust/src/bake_region_tests.rs wg-10/rust/src/lib.rs` then commit `feat(bake): rust bake_region assembly + end-to-end seam-safe parity gate`.

---

## Final: record + push
- [ ] Update STATUS.md (prepend): bake_region assembles macro+carve+condition in Rust, end-to-end parity GREEN (the whole offline look-pipeline now reproduced + verified in Rust); next = wire into the live producer (region-fact bake + LRU + GPU/CPU coordination). Note the cross-region condition-seam boundary.
- [ ] Update memory `worldgen10-carve-ported-to-rust.md` accordingly.
- [ ] `git add docs/plans/STATUS.md` + commit; `GIT_TERMINAL_PROMPT=0 git push origin slice4-gpu-page-integration`.

---

## Self-Review notes (author)

- **Spec coverage:** assembly in order (macro→carve→condition) → Task 2 bake_region; seam-safe-not-full-field → fixture uses `mountain.generate(apron_px>0)`; end-to-end parity → Task 2 gate; stats-percentile sharp signal → gate asserts p05/p50/p95; non-vacuous → carved>0 + height~[-1,1]; cross-region condition seam → recorded in spec + final STATUS note (NOT solved here, correctly deferred).
- **The two real risks, both gated:** (1) `build_grid` matching `mountain.grid` (if the macro grid is off, all downstream diverges) — caught by the stats-percentile assert + a debug step to compare raw. (2) carve param values matching what `carve_pass_network` actually uses — Task 1 Step 1 explicitly says READ carve_pass_network and capture its ACTUAL params, not assume defaults.
- **Deferred value:** `hp99_budget` self-baselined (measure→×1.5) with a "large = assembly bug" backstop — same proven pattern as carve_ramp/condition_world, not a placeholder.
- **No new algorithms** — pure composition of three parity-proven pieces + a grid-construction port. The gate proves the composition; the pieces are already individually proven.
