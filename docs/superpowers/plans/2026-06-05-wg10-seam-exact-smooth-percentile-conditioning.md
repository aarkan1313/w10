# WG10 Seam-Exact Smooth-Percentile-Field Conditioning — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use `- [ ]` checkboxes.

**Goal:** Replace `condition_world`'s per-region scalar percentiles (the measured ~1090 m
cross-region seam source) with a seam-exact smooth percentile FIELD, behind a swappable
`PercentileProvider` interface (engine modularity).

**Architecture:** `condition_world` becomes a pure transform over per-cell percentile fields
(scalar = length-1 broadcast, bit-exact to today). A `PercentileProvider` trait yields the
fields; `ScalarRegionPercentiles` = today's behavior (tests/single-region), `SmoothFieldPercentiles`
= coarse seam-safe macro + windowed percentiles + bilinear upsample → seam-exact by construction,
locally adaptive. This is the revised Task 6 of the region-fact integration; it slots between
the (superseded) Task 6 and Task 7.

**Tech Stack:** Rust (`wg10_terrain` crate). Pure CPU + cargo gates (the off-frame bake path);
GPU live-path percentile sampling is a later optimization, out of scope here.

**Spec:** `docs/superpowers/specs/2026-06-05-wg10-seam-exact-smooth-percentile-conditioning-design.md`

**Conventions:** Isolated Rust test: `cd /d/workflows/worldgen10/wg-10/rust && CARGO_TARGET_DIR=/d/tmp/wg10_check_target cargo test -p wg10_terrain --lib <filter>`. Scoped `git add` only (worktree has ~245 preexisting dirty files — never `git add -A`). Baseline before Task 6a: **256** lib tests passing.

---

## File Structure

| File | Responsibility | C/M |
|---|---|---|
| `wg-10/rust/src/condition_world.rs` | add `condition_world_with_percentile_fields` (per-cell fields, length-1 broadcast); scalar fn delegates | Modify |
| `wg-10/rust/src/region_bake/percentile_provider.rs` | `PercentileProvider` trait + `PercentileFields` + `ScalarRegionPercentiles` + `SmoothFieldPercentiles` | Create |
| `wg-10/rust/src/region_bake/mod.rs` | `mod percentile_provider;`; `bake_region_from_raw` takes a `&dyn PercentileProvider` | Modify |
| `wg-10/rust/src/region_bake/percentile_seam_tests.rs` | seam-exactness + interior look-parity gates | Create |
| `tools/dem_pack/export_region_seam_fixture.py` | extend to also emit each region's world origin (for the provider) | Modify |

---

## Task 6a: Field-valued conditioning (`condition_world_with_percentile_fields`)

**Files:** Modify `condition_world.rs`; Test: `condition_world_tests.rs`.

- [ ] **Step 1: Write the failing test** — add to `condition_world_tests.rs`:

```rust
#[test]
fn field_percentiles_length1_broadcast_equals_scalar() {
    // A length-1 percentile "field" must broadcast to exactly the scalar path (bit-identical).
    let n = 8usize;
    let mut z = vec![0.0f64; n * n];
    for i in 0..n * n { z[i] = ((i * 131 % 97) as f64) * 0.37 - 12.0; }
    let (_w, stats) = super::condition_world(&z, n);
    let want = super::condition_world_with_percentiles(&z, n, stats.p05, stats.p50, stats.p95);
    let got = super::condition_world_with_percentile_fields(
        &z, n, &[stats.p05], &[stats.p50], &[stats.p95]);
    assert_eq!(got.len(), want.len());
    for i in 0..want.len() { assert_eq!(got[i].to_bits(), want[i].to_bits(), "cell {i}"); }
}

#[test]
fn field_percentiles_per_cell_varies() {
    // A genuine per-cell field: each cell normalized by its OWN p50 -> robust 0 everywhere pre-smooth.
    let n = 4usize;
    let z: Vec<f64> = (0..n*n).map(|i| i as f64).collect();
    let p05: Vec<f64> = z.iter().map(|v| v - 1.0).collect();
    let p50: Vec<f64> = z.clone();
    let p95: Vec<f64> = z.iter().map(|v| v + 1.0).collect();
    let got = super::condition_world_with_percentile_fields(&z, n, &p05, &p50, &p95);
    // robust[i] = (z-p50)/(p95-p05+eps)*2.10 = 0 -> tanh(gaussian(0)) = 0 everywhere.
    for (i, &v) in got.iter().enumerate() { assert!(v.abs() < 1e-9, "cell {i} = {v}"); }
}
```

- [ ] **Step 2: Run, verify FAIL** (`condition_world_with_percentile_fields` not found):
`cargo test -p wg10_terrain --lib field_percentiles`

- [ ] **Step 3: Implement** in `condition_world.rs`. Add the field-valued fn and make the scalar
`condition_world_with_percentiles` delegate to it with length-1 slices:

```rust
/// Field-valued conditioning: `p05f/p50f/p95f` are EITHER length 1 (scalar, broadcast to every
/// cell) OR length `n*n` (a per-cell percentile field, for seam-exact cross-region normalization).
/// Identical math to `condition_world_with_percentiles` when the fields are length 1.
///
/// # Panics
/// Panics if `z.len() != n*n`, or if any percentile field is neither length 1 nor length `n*n`.
pub fn condition_world_with_percentile_fields(
    z: &[f64], n: usize, p05f: &[f64], p50f: &[f64], p95f: &[f64],
) -> Vec<f64> {
    let nn = n * n;
    assert_eq!(z.len(), nn, "condition_world_with_percentile_fields: z.len() != n*n");
    let at = |f: &[f64], i: usize| -> f64 {
        if f.len() == 1 { f[0] } else { f[i] }
    };
    for (name, f) in [("p05f", p05f), ("p50f", p50f), ("p95f", p95f)] {
        assert!(f.len() == 1 || f.len() == nn,
            "condition_world_with_percentile_fields: {name} len {} not 1 or {nn}", f.len());
    }
    let robust: Vec<f64> = (0..nn).map(|i| {
        let denom = at(p95f, i) - at(p05f, i) + 1.0e-9;
        (z[i] - at(p50f, i)) / denom * 2.10
    }).collect();
    let smoothed = gaussian_filter_nearest(&robust, n, n, 0.55, 4.0);
    smoothed.iter().map(|v| v.tanh()).collect()
}
```

Then rewrite `condition_world_with_percentiles` (the scalar one) to delegate:

```rust
pub fn condition_world_with_percentiles(z: &[f64], n: usize, p05: f64, p50: f64, p95: f64) -> Vec<f64> {
    condition_world_with_percentile_fields(z, n, &[p05], &[p50], &[p95])
}
```

(Keep its doc-comment. `condition_world` still calls `condition_world_with_percentiles` — unchanged,
so it stays bit-exact and the Python-oracle + bake_region gates remain green.)

- [ ] **Step 4: Run** `cargo test -p wg10_terrain --lib condition_world field_percentiles` — new tests
pass AND `condition_world_matches_python_within_tolerance` + `with_percentiles_self_computed_equals_original`
+ `bake_region_matches_python_seamsafe_pipeline` all still green. Then full suite (expect 258).

- [ ] **Step 5: Commit:**
```
git add wg-10/rust/src/condition_world.rs wg-10/rust/src/condition_world_tests.rs
git commit -m "feat(condition): field-valued condition_world_with_percentile_fields (length-1 broadcast = scalar, bit-exact)"
```

---

## Task 6b: `PercentileProvider` interface + `ScalarRegionPercentiles`

**Files:** Create `region_bake/percentile_provider.rs`; Modify `region_bake/mod.rs`.

- [ ] **Step 1: Write the failing test** (inline in `percentile_provider.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scalar_provider_matches_self_percentiles() {
        // ScalarRegionPercentiles over a field yields that field's own p05/p50/p95 as length-1 fields.
        let n = 8usize;
        let z: Vec<f64> = (0..n*n).map(|i| ((i*131%97) as f64)*0.37 - 12.0).collect();
        let prov = ScalarRegionPercentiles;
        let f = prov.percentiles(&z, 0.0, 0.0, 1000.0, n);
        assert_eq!(f.p05.len(), 1);
        // Conditioning through the provider's fields must equal today's condition_world.
        let (want, _s) = crate::condition_world::condition_world(&z, n);
        let got = crate::condition_world::condition_world_with_percentile_fields(&z, n, &f.p05, &f.p50, &f.p95);
        for i in 0..want.len() { assert_eq!(got[i].to_bits(), want[i].to_bits(), "cell {i}"); }
    }
}
```

- [ ] **Step 2: Run, verify FAIL.** `cargo test -p wg10_terrain --lib scalar_provider`

- [ ] **Step 3: Implement `percentile_provider.rs`:**

```rust
//! PercentileProvider: the swappable source of conditioning percentiles (engine modularity).
//! condition_world is a pure transform; THIS decides how p05/p50/p95 are derived, which is the
//! cross-region SEAM strategy. Two impls: ScalarRegionPercentiles (per-region, today's look /
//! single-region / tests) and SmoothFieldPercentiles (seam-exact engine default, Task 6c).
#![allow(dead_code)]
use crate::condition_world::percentile_linear;

/// Per-cell percentile fields for a region grid. Each is length 1 (scalar broadcast) or n*n.
pub struct PercentileFields {
    pub p05: Vec<f64>,
    pub p50: Vec<f64>,
    pub p95: Vec<f64>,
}

/// The seam strategy, swappable. `z` is the region's carved RAW field (length n*n); the world
/// coords let a smooth provider sample a position-continuous percentile field.
pub trait PercentileProvider {
    fn percentiles(&self, z: &[f64], region_x0_m: f64, region_z0_m: f64, span_m: f64, n: usize)
        -> PercentileFields;
}

/// Today's behavior: percentiles over the region's OWN field (one triple, length-1 broadcast).
/// Keeps the accepted single-region look + the existing bit-exact gates. NOT seam-exact across
/// regions (that's exactly the measured ~1090 m seam) — use only for single-region/tests.
pub struct ScalarRegionPercentiles;

impl PercentileProvider for ScalarRegionPercentiles {
    fn percentiles(&self, z: &[f64], _x0: f64, _z0: f64, _span: f64, _n: usize) -> PercentileFields {
        let mut sorted = z.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).expect("ScalarRegionPercentiles: NaN"));
        PercentileFields {
            p05: vec![percentile_linear(&sorted, 5.0)],
            p50: vec![percentile_linear(&sorted, 50.0)],
            p95: vec![percentile_linear(&sorted, 95.0)],
        }
    }
}

#[cfg(test)]
mod tests;  // (or inline as written above)
```

(If `percentile_linear` is not `pub`, make it `pub` in `condition_world.rs` — it already is `pub`.)

- [ ] **Step 4: Wire** `mod percentile_provider;` + `pub use percentile_provider::*;` in `region_bake/mod.rs`.

- [ ] **Step 5: Run** `cargo test -p wg10_terrain --lib scalar_provider` (pass) + full suite (expect 259).

- [ ] **Step 6: Commit:**
```
git add wg-10/rust/src/region_bake/percentile_provider.rs wg-10/rust/src/region_bake/mod.rs
git commit -m "feat(region_bake): PercentileProvider interface + ScalarRegionPercentiles (engine-modular seam strategy)"
```

---

## Task 6c: `SmoothFieldPercentiles` (seam-exact) + seam gate

**Files:** Modify `percentile_provider.rs`; Create `region_bake/percentile_seam_tests.rs`; Modify `export_region_seam_fixture.py` (emit region origins).

- [ ] **Step 1: Extend the seam fixture** — in `export_region_seam_fixture.py`, also write each
region's world origin so the provider can be exercised at the true coords:
add `"origin_a_x", "origin_a_z", "origin_b_x", "origin_b_z"` (the SOURCE_ORIGIN values used for A
and B) to the JSON. Re-run `python tools/dem_pack/export_region_seam_fixture.py`.

- [ ] **Step 2: Write the seam-exactness + interior gates** — `region_bake/percentile_seam_tests.rs`:

```rust
//! SmoothFieldPercentiles must (1) be SEAM-EXACT: two abutting regions conditioned through it agree
//! at the shared border (the ~1090 m scalar seam -> ~0); (2) preserve the LOOK: in a region's
//! interior it matches today's per-region (ScalarRegionPercentiles) conditioning where the region
//! is internally uniform.
use crate::region_bake::{PercentileProvider, ScalarRegionPercentiles, SmoothFieldPercentiles};
use crate::condition_world::condition_world_with_percentile_fields as cond_f;

fn load() -> (usize, f64, Vec<f64>, Vec<f64>, [f64;4]) {
    use std::path::Path;
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/dem_pack/fixtures/region_seam_fixture.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let n = v["n"].as_u64().unwrap() as usize;
    let hs = v["height_scale_m"].as_f64().unwrap();
    let span = v["span_m"].as_f64().unwrap();
    let a: Vec<f64> = v["carved_a"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let b: Vec<f64> = v["carved_b"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let ax = v["origin_a_x"].as_f64().unwrap(); let az = v["origin_a_z"].as_f64().unwrap();
    let bx = v["origin_b_x"].as_f64().unwrap(); let bz = v["origin_b_z"].as_f64().unwrap();
    (n, span, a, b, [ax, az, bx, bz])  // hs folded via span; carry hs separately:
        ; // (note: adjust tuple to also return hs)
}

#[test]
fn smooth_field_is_seam_exact() {
    let (n, span, a, b, o) = load_with_hs_placeholder(); // see note: return hs too
    let prov = SmoothFieldPercentiles::for_seam_test(/* coarse params */);
    let fa = prov.percentiles(&a, o[0], o[1], span, n);
    let fb = prov.percentiles(&b, o[2], o[3], span, n);
    let ha = cond_f(&a, n, &fa.p05, &fa.p50, &fa.p95);
    let hb = cond_f(&b, n, &fb.p05, &fb.p50, &fb.p95);
    let hs = 1700.0;
    let mut maxd = 0.0f64;
    for r in 0..n { maxd = maxd.max((ha[r*n + (n-1)] - hb[r*n]).abs()); }
    assert!(maxd * hs < 0.15, "smooth-field seam still {:.3}m (was ~1090m scalar)", maxd*hs);
}

#[test]
fn smooth_field_preserves_interior_look() {
    let (n, span, a, _b, o) = load_with_hs_placeholder();
    let scalar = ScalarRegionPercentiles.percentiles(&a, o[0], o[1], span, n);
    let smooth = SmoothFieldPercentiles::for_seam_test().percentiles(&a, o[0], o[1], span, n);
    let hs = 1700.0;
    let hsa = cond_f(&a, n, &scalar.p05, &scalar.p50, &scalar.p95);
    let hsm = cond_f(&a, n, &smooth.p05, &smooth.p50, &smooth.p95);
    // Interior (skip a margin band); where the region's distribution is uniform the two agree.
    let m = n / 8;
    let mut maxd = 0.0f64;
    for r in m..n-m { for c in m..n-m { maxd = maxd.max((hsa[r*n+c] - hsm[r*n+c]).abs()); } }
    println!("[smooth-interior] maxd(tanh)={maxd:.4} ~= {:.2}m", maxd*hs);
    // Bar: interior look preserved within the accepted condition residual band. If this FAILS, the
    // coarse/window params under-resolve the real distribution -> tune (spec risk), do not widen blindly.
    assert!(maxd * hs < 30.0, "interior look drifted {:.2}m -> coarse/window under-resolves", maxd*hs);
}
```

> IMPLEMENTER NOTE: the `load()` sketch above is illustrative — write a single clean loader that
> returns `(n, hs, span, a, b, [ax,az,bx,bz])`. The interior bar (30 m here on a 1700 m relief, ~1.8%)
> is a STARTING point; record the measured `[smooth-interior]` number and, if it is far under, tighten
> the bar to the measured value + margin (a real gate, not a loose one). The seam bar (0.15 m) is firm.

- [ ] **Step 3: Implement `SmoothFieldPercentiles`** in `percentile_provider.rs`. The seam-exact
smooth field, derived from a coarse evaluation of the SAME carved field, windowed + bilinear:

```rust
/// Seam-exact smooth percentile field. Percentiles vary smoothly with world position (NOT stepped
/// per-region), so two abutting regions agree at their shared border BY CONSTRUCTION while staying
/// locally adaptive. Source: percentiles over a fixed-WORLD-SIZE window on a coarse lattice of the
/// region field, bilinearly upsampled to the full grid. (For the off-frame bake this is the SAME
/// carved field at coarse stride; the GPU live-path variant samples the coarse macro directly.)
pub struct SmoothFieldPercentiles {
    pub coarse_stride: usize,   // sample every k-th cell for the coarse lattice
    pub window_cells: usize,    // half-window (in coarse nodes) for the percentile reduction
}

impl SmoothFieldPercentiles {
    pub fn for_seam_test() -> Self { Self { coarse_stride: 8, window_cells: 4 } }
}

impl PercentileProvider for SmoothFieldPercentiles {
    fn percentiles(&self, z: &[f64], _x0: f64, _z0: f64, _span: f64, n: usize) -> PercentileFields {
        // 1. Coarse lattice node positions (in cell index space), inclusive of the far edge so the
        //    upsample covers [0, n-1]. Node value sets = a window of the field around each node.
        // 2. For each node, gather the field over a world-fixed window (here: +/- window_cells*stride
        //    cells, clamped) and compute p05/p50/p95 via percentile_linear over the sorted window.
        // 3. Bilinearly interpolate the three coarse node grids to the full n*n grid.
        //    Because adjacent regions share the coarse node lattice + window definition at the
        //    border (a continuous function of world position), the interpolated border values match.
        // Implement steps 1-3 producing p05/p50/p95 each length n*n.
        // (Concrete, deterministic; no RNG. See spec data-flow.)
        unimplemented!("implement coarse-lattice windowed-percentile bilinear-upsample per the spec")
    }
}
```

> IMPLEMENTER: write the full deterministic body (the doc enumerates the exact 3 steps). Keep it pure
> Rust + `percentile_linear`. The SEAM-EXACTNESS hinges on the coarse node lattice + window being a
> function of WORLD position shared by both regions at the border — for the fixture, regions A and B
> abut in X, so A's right-edge coarse column and B's left-edge coarse column must be the SAME world
> column with the SAME window. If your indexing keys the lattice to region-local indices only, the
> border won't match — key the window gather to world position (use region_x0 + cell*cell_m). The
> `smooth_field_is_seam_exact` test is the proof; iterate until it passes at < 0.15 m.

- [ ] **Step 4: Wire** `#[cfg(test)] mod percentile_seam_tests;` in `region_bake/mod.rs`.

- [ ] **Step 5: Run** the gates:
`cargo test -p wg10_terrain --lib smooth_field_is_seam_exact smooth_field_preserves_interior_look -- --nocapture`
Both must pass: seam < 0.15 m (vs the ~1090 m scalar baseline), interior within bar. CAPTURE the
`[smooth-interior]` number. Then full suite.

- [ ] **Step 6: Commit:**
```
git add wg-10/rust/src/region_bake/percentile_provider.rs wg-10/rust/src/region_bake/percentile_seam_tests.rs wg-10/rust/src/region_bake/mod.rs tools/dem_pack/export_region_seam_fixture.py tools/dem_pack/fixtures/region_seam_fixture.json
git commit -m "feat(region_bake): SmoothFieldPercentiles (seam-exact smooth percentile field) — closes the ~1090m condition seam"
```

---

## Task 6d: `bake_region_from_raw` takes a `&dyn PercentileProvider`

**Files:** Modify `region_bake/mod.rs`; Test: `region_bake_tests.rs`.

- [ ] **Step 1: Write the failing test** — `bake_region_from_raw` with `ScalarRegionPercentiles`
must equal the current `None`/self-percentile result (bit-exact regression):

```rust
#[test]
fn from_raw_scalar_provider_equals_self_percentiles() {
    let n = 48usize; let span_m = 25600.0; let hs = 260.0;
    let mut raw = vec![0.0f64; n*n];
    for i in 0..n*n { let x=(i%n) as f64/n as f64; let z=(i/n) as f64/n as f64;
        raw[i] = (x*6.0).sin()*(z*6.0).cos()*1.5 + (i%13) as f64*0.05; }
    let pass = crate::pass_network::PassNetworkParams::default();
    let traverse = crate::pass_network::TraverseParams { scene_width_m: span_m, height_scale_m: hs, ..Default::default() };
    let ramp = crate::pass_network::RampParams::default();
    let want = super::bake_region_from_raw(&raw, n, span_m, hs, &pass, &traverse, &ramp, None);
    let got = super::bake_region_from_raw_with_provider(&raw, n, span_m, hs, &pass, &traverse, &ramp,
        &super::ScalarRegionPercentiles);
    for i in 0..want.height.len() { assert_eq!(got.height[i].to_bits(), want.height[i].to_bits(), "cell {i}"); }
}
```

- [ ] **Step 2: Run, verify FAIL.**

- [ ] **Step 3: Implement** — add `bake_region_from_raw_with_provider` that runs the carve, then asks
the provider for the percentile fields over `raw_carved` (with the region world coords), then calls
`condition_world_with_percentile_fields`. Keep the existing `bake_region_from_raw(.., percentiles:
Option<RegionPercentiles>)` as a thin wrapper: `None` => `ScalarRegionPercentiles` provider (bit-exact
to today's `condition_world`); `Some(p)` => a fixed-scalar provider that returns those triples. This
preserves Task 2's signature + all green gates while routing through the provider. The region world
coords for the off-frame bake come from the bake request (Task 7/8); `bake_region_from_raw` without
coords passes `0.0,0.0` (the scalar provider ignores them).

> IMPLEMENTER: keep `bake_region.rs`'s end-to-end Python parity test green — it calls `bake_region`
> -> `bake_region_from_raw(.., None)` -> ScalarRegionPercentiles -> bit-exact `condition_world`. Verify.

- [ ] **Step 4: Run** the new test + `bake_region_matches_python_seamsafe_pipeline` + `from_raw_matches_full_cpu_bake` + full suite. All green.

- [ ] **Step 5: Commit:**
```
git add wg-10/rust/src/region_bake/mod.rs wg-10/rust/src/region_bake/region_bake_tests.rs
git commit -m "feat(region_bake): bake_region_from_raw_with_provider (&dyn PercentileProvider); None=scalar bit-exact"
```

---

## Wiring note for Task 7/8 (the worker + producer)

The worker (Task 7) constructs a `SmoothFieldPercentiles` provider and calls
`bake_region_from_raw_with_provider(.., &provider)` with the region's true world origin — so the live
producer's regions are seam-exact. (Task 7's `bake_one` switches from `bake_region_from_raw(.., None)`
to the provider form; update that one call.) The deferred OWNER VISUAL A/B (smooth vs per-region
conditioned look) is flagged in the spec and recorded in Task 9's STATUS update — it gates "look
shipped," not this integration.

---

## Self-review (coverage vs spec)
- Component 1 (PercentileProvider interface) → Task 6b. ✓
- Component 2 (SmoothFieldPercentiles source) → Task 6c. ✓
- Field-valued conditioning → Task 6a. ✓
- Gate 1 seam-exactness → Task 6c `smooth_field_is_seam_exact`. ✓
- Gate 2 interior look-parity → Task 6c `smooth_field_preserves_interior_look`. ✓
- Gate 3 scalar refactor bit-exact → Task 6a + 6b + 6d (multiple bit-exact regression tests + existing Python gate). ✓
- Gate 4 deferred owner visual A/B → flagged in Task 9 STATUS (not a code task). ✓
- Modularity (engine, swappable provider) → 6b interface, 6d injection. ✓
- GPU/Rust note: live-path GPU percentile sampling is explicitly out of scope (off-frame Rust bake here); flagged in spec + Task 9. ✓
