# WG10 Region-Fact Producer Integration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put the carved "baked look" on screen in the live runtime by baking regions
off-frame (GPU macro → readback → CPU carve+condition) and having pages sample the result.

**Architecture:** A background bake worker (own `RenderingDevice`) runs the proven
`bake_region` pipeline with its CPU macro step replaced by a GPU region-macro readback,
producing a `RegionFactRuntime` cached on a region LRU. The pool gets a `ProducerKind::RegionFact`
arm: baked regions sample the carved height; unbaked regions show the existing coarse fallback
(never-black) while the worker bakes them ahead. Cross-region condition seam is settled by an
early measurement gate (G-seam).

**Tech Stack:** Rust (`wg10_terrain` crate, godot-rust), Godot 4.6.2 RenderingDevice compute
(GLSL), windowed gates on RTX 5090.

**Conventions used throughout this plan:**
- Isolated Rust build/test (no editor): `$env:CARGO_TARGET_DIR='D:/tmp/wg10_check_target'; Push-Location 'D:\workflows\worldgen10\wg-10\rust'; cargo test -p wg10_terrain --lib <filter>; Pop-Location`
- Windowed gates run with the **editor closed** (`GODOT_BIN` set, `python tools\gate.py --suite <suite>`).
- Commit ONLY files this plan creates/modifies via scoped `git add` (the worktree has ~245
  preexisting dirty files — never `git add -A` / reset / broad-checkout).
- Baseline: `cargo test -p wg10_terrain --lib` is **251 passing** before Task 1.

---

## File Structure

| File | Responsibility | Created/Modified |
|---|---|---|
| `wg-10/rust/src/condition_world.rs` | add `condition_world_with_percentiles` (injected p05/p50/p95); existing fn delegates | Modify |
| `wg-10/rust/src/region_bake/mod.rs` | `RegionPercentiles`, `bake_region_from_raw` (carve→condition tail on injected RAW) | Create |
| `wg-10/rust/src/bake_region.rs` | existing all-CPU entry delegates to `bake_region_from_raw` (keeps parity test green) | Modify |
| `wg-10/rust/src/page_pool/region_fact.rs` | `RegionFactRuntime`: conditioned grid + bounds; `sample` bilinear; `write_page_texture` | Create |
| `wg-10/rust/src/region_bake/gpu_macro.rs` | GPU region-macro readback (reuse biome page-compute at region scale) → apron-cropped RAW | Create |
| `wg-10/rust/src/region_bake/worker.rs` | async bake thread (own RD); region-key in / `RegionFactRuntime` out via channels | Create |
| `wg-10/rust/src/page_pool/producer.rs` | `ProducerKind::RegionFact` arm + region cache routing | Modify |
| `wg-10/rust/src/lib.rs` (or `mod.rs`) | register `region_bake` module + `region_fact` submodule | Modify |
| `tools/dem_pack/export_region_seam_fixture.py` | emit two-adjacent-region fixture for G-seam | Create |

---

## Task 1: `condition_world_with_percentiles` (injected percentiles, refactor-safe)

**Files:**
- Modify: `wg-10/rust/src/condition_world.rs`
- Test: `wg-10/rust/src/condition_world_tests.rs` (existing test module — add a case)

- [ ] **Step 1: Write the failing test** — add to `condition_world_tests.rs`:

```rust
#[test]
fn with_percentiles_self_computed_equals_original() {
    // A small deterministic field; the injected-percentile path with self-computed
    // percentiles must be BIT-IDENTICAL to the original condition_world.
    let n = 8usize;
    let mut z = vec![0.0f64; n * n];
    for i in 0..n * n {
        z[i] = ((i * 131 % 97) as f64) * 0.37 - 12.0;
    }
    let (want, stats) = super::condition_world(&z, n);
    let got = super::condition_world_with_percentiles(&z, n, stats.p05, stats.p50, stats.p95);
    assert_eq!(got.len(), want.len());
    for i in 0..want.len() {
        assert_eq!(got[i].to_bits(), want[i].to_bits(), "cell {i} differs");
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p wg10_terrain --lib with_percentiles_self_computed_equals_original`
Expected: FAIL — `condition_world_with_percentiles` not found.

- [ ] **Step 3: Refactor `condition_world.rs`** — extract the post-percentile body into a new
public fn and have `condition_world` call it. Replace the body of `condition_world` (lines ~59-114)
so that after computing `p05/p50/p95` and the source stats it delegates:

```rust
/// Condition a region field using EXTERNALLY SUPPLIED percentiles (for cross-region seam
/// reconciliation). Identical math to `condition_world` from the `robust` step onward; only the
/// p05/p50/p95 source differs. The returned ConditionStats reports the SUPPLIED percentiles.
pub fn condition_world_with_percentiles(z: &[f64], n: usize, p05: f64, p50: f64, p95: f64) -> Vec<f64> {
    assert_eq!(z.len(), n * n, "condition_world_with_percentiles: z.len() != n*n");
    let denom = p95 - p05 + 1.0e-9;
    let robust: Vec<f64> = z.iter().map(|v| (v - p50) / denom * 2.10).collect();
    let smoothed = gaussian_filter_nearest(&robust, n, n, 0.55, 4.0);
    smoothed.iter().map(|v| v.tanh()).collect()
}
```

Then in `condition_world`, after computing `p05/p50/p95` and source/conditioned stats, replace the
inline `robust`/`smoothed`/`shaped` computation with:

```rust
    let shaped = condition_world_with_percentiles(z, n, p05, p50, p95);
```

(Keep the existing conditioned-stats computation over `shaped` exactly as-is.)

- [ ] **Step 4: Run tests to verify they pass** (new + existing condition_world parity)

Run: `cargo test -p wg10_terrain --lib condition_world`
Expected: PASS — new test + `condition_world` Python-oracle parity test both green.

- [ ] **Step 5: Commit**

```bash
git add wg-10/rust/src/condition_world.rs wg-10/rust/src/condition_world_tests.rs
git commit -m "feat(condition): condition_world_with_percentiles (injected p05/p50/p95), refactor-safe"
```

---

## Task 2: `region_bake` module — `RegionPercentiles` + `bake_region_from_raw`

**Files:**
- Create: `wg-10/rust/src/region_bake/mod.rs`
- Modify: `wg-10/rust/src/lib.rs` (register `mod region_bake;`)
- Modify: `wg-10/rust/src/bake_region.rs` (delegate to `bake_region_from_raw`)
- Test: `wg-10/rust/src/region_bake/region_bake_tests.rs`

- [ ] **Step 1: Register the module** — in `wg-10/rust/src/lib.rs`, add near the other `mod`
declarations:

```rust
mod region_bake;
```

- [ ] **Step 2: Write the failing test** — create `wg-10/rust/src/region_bake/region_bake_tests.rs`:

```rust
//! bake_region_from_raw must reproduce the existing all-CPU bake_region exactly when fed the
//! SAME RAW field the CPU macro produces (the tail = carve -> condition, unchanged).
use crate::pass_network::{PassNetworkParams, RampParams, TraverseParams};

#[test]
fn from_raw_matches_full_cpu_bake() {
    let n = 64usize;
    let span_m = 25600.0;
    let hs = 260.0;
    let seed = 7;
    // A deterministic RAW field standing in for the macro output (z-score-ish range).
    let mut raw = vec![0.0f64; n * n];
    for i in 0..n * n {
        let x = (i % n) as f64 / n as f64;
        let z = (i / n) as f64 / n as f64;
        raw[i] = (x * 6.0).sin() * (z * 6.0).cos() * 1.5 + (i % 13) as f64 * 0.05;
    }
    let pass = PassNetworkParams::default();
    let traverse = TraverseParams { scene_width_m: span_m, height_scale_m: hs, ..Default::default() };
    let ramp = RampParams::default();

    // Oracle: carve + condition done inline (the exact tail of bake_region).
    let routes = crate::pass_network::carve_routes(&raw, n, span_m, hs, &pass, &traverse);
    let delta = crate::pass_network::carve_ramp_delta(&raw, n, span_m, hs, &routes, &ramp);
    let raw_carved: Vec<f64> = raw.iter().zip(delta.iter()).map(|(r, d)| r + d).collect();
    let (want_h, want_stats) = crate::condition_world::condition_world(&raw_carved, n);

    let got = super::bake_region_from_raw(&raw, n, span_m, hs, &pass, &traverse, &ramp, None);
    assert_eq!(got.height.len(), want_h.len());
    for i in 0..want_h.len() {
        assert_eq!(got.height[i].to_bits(), want_h[i].to_bits(), "height cell {i}");
    }
    assert_eq!(got.stats.p50.to_bits(), want_stats.p50.to_bits());
}
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo test -p wg10_terrain --lib from_raw_matches_full_cpu_bake`
Expected: FAIL — `region_bake::bake_region_from_raw` not found.

- [ ] **Step 4: Implement `region_bake/mod.rs`**:

```rust
//! region_bake: the carve -> condition TAIL of the baked-look pipeline, fed a RAW field that
//! comes from the GPU region-macro readback (the live path) instead of the CPU macro. The CPU
//! macro entry (`bake_region::bake_region`) delegates here so the existing end-to-end parity
//! gate still covers the tail.
#![allow(dead_code)]
use crate::condition_world::{condition_world, condition_world_with_percentiles, ConditionStats};
use crate::pass_network::{carve_ramp_delta, carve_routes, PassNetworkParams, RampParams, TraverseParams};

#[cfg(test)]
mod region_bake_tests;

/// Externally supplied conditioning percentiles (cross-region seam reconcile). When `None`,
/// `bake_region_from_raw` self-computes them per-region (the single-region / interior case).
#[derive(Clone, Copy, Debug)]
pub struct RegionPercentiles {
    pub p05: f64,
    pub p50: f64,
    pub p95: f64,
}

pub struct BakeResult {
    pub height: Vec<f64>,
    pub carve_delta: Vec<f64>,
    pub stats: ConditionStats,
}

/// Carve (on RAW) -> raw+delta -> condition. ORDER load-bearing (carve on raw, THEN condition).
/// `percentiles=None` => self-compute per-region; `Some(..)` => use the reconciled set.
#[allow(clippy::too_many_arguments)]
pub fn bake_region_from_raw(
    raw: &[f64],
    n: usize,
    span_m: f64,
    height_scale_m: f64,
    pass: &PassNetworkParams,
    traverse: &TraverseParams,
    ramp: &RampParams,
    percentiles: Option<RegionPercentiles>,
) -> BakeResult {
    let routes = carve_routes(raw, n, span_m, height_scale_m, pass, traverse);
    let carve_delta = carve_ramp_delta(raw, n, span_m, height_scale_m, &routes, ramp);
    let raw_carved: Vec<f64> = raw.iter().zip(carve_delta.iter()).map(|(r, d)| r + d).collect();
    let (height, stats) = match percentiles {
        None => condition_world(&raw_carved, n),
        Some(p) => {
            let h = condition_world_with_percentiles(&raw_carved, n, p.p05, p.p50, p.p95);
            // stats report the SUPPLIED percentiles; conditioned min/max from the shaped field.
            let (mut cmin, mut cmax) = (h[0], h[0]);
            for &v in &h {
                if v < cmin { cmin = v; }
                if v > cmax { cmax = v; }
            }
            let mut sorted = raw_carved.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let stats = ConditionStats {
                source_min: sorted[0],
                source_max: sorted[n * n - 1],
                source_ptp: sorted[n * n - 1] - sorted[0],
                p05: p.p05, p50: p.p50, p95: p.p95,
                conditioned_min: cmin, conditioned_max: cmax, conditioned_ptp: cmax - cmin,
            };
            (h, stats)
        }
    };
    BakeResult { height, carve_delta, stats }
}
```

- [ ] **Step 5: Make `condition_world::ConditionStats` reachable** — ensure `region_bake` can name it.
It is already `pub`. Confirm the `use crate::condition_world::{... ConditionStats}` resolves.

- [ ] **Step 6: Delegate the existing all-CPU entry** — in `wg-10/rust/src/bake_region.rs`, replace
the carve→condition tail of `bake_region` (lines 39-43) with a call into the new tail, so the parity
test now exercises `bake_region_from_raw`:

```rust
    let raw = mountain_seamsafe(wx, wz, pn, pn, seed, feature_span_m, apron_px, spacing_m, flow_on);
    let r = crate::region_bake::bake_region_from_raw(&raw, n, span_m, height_scale_m, pass, traverse, ramp, None);
    BakeResult { height: r.height, carve_delta: r.carve_delta, stats: r.stats }
```

(Keep `bake_region.rs`'s own `BakeResult` type and its `use` lines for `mountain_seamsafe`; drop the
now-unused `carve_*`/`condition_world` imports there to avoid warnings.)

- [ ] **Step 7: Run tests to verify they pass** (new + existing bake_region parity)

Run: `cargo test -p wg10_terrain --lib from_raw_matches_full_cpu_bake bake_region_matches_python`
Expected: PASS — both green (the Python-oracle parity test still passes through the delegated tail).

- [ ] **Step 8: Commit**

```bash
git add wg-10/rust/src/region_bake/mod.rs wg-10/rust/src/region_bake/region_bake_tests.rs wg-10/rust/src/bake_region.rs wg-10/rust/src/lib.rs
git commit -m "feat(region_bake): bake_region_from_raw (carve->condition tail on injected RAW) + RegionPercentiles"
```

---

## Task 3: `RegionFactRuntime` (sampling + page write)

**Files:**
- Create: `wg-10/rust/src/page_pool/region_fact.rs`
- Modify: `wg-10/rust/src/page_pool.rs` (or `page_pool/mod.rs`) — add `mod region_fact;`
- Test: inline `#[cfg(test)]` in `region_fact.rs`

- [ ] **Step 1: Register the submodule** — in the page_pool module root, add:

```rust
mod region_fact;
```

- [ ] **Step 2: Write the failing test** — at the bottom of `region_fact.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::RegionFactRuntime;

    fn ramp_grid(n: usize) -> Vec<f32> {
        // height = x index (so a known bilinear answer mid-cell).
        (0..n * n).map(|i| (i % n) as f32).collect()
    }

    #[test]
    fn samples_bilinear_at_known_point() {
        let n = 4;
        let rt = RegionFactRuntime::new(ramp_grid(n), n, 0.0, 0.0, 300.0, 300.0);
        // grid_n-1 = 3 spans 300m -> 100m/cell. At x=150m we are at gx=1.5 -> value 1.5.
        let h = rt.sample(150.0, 0.0);
        assert!((h - 1.5).abs() < 1e-5, "got {h}");
    }

    #[test]
    fn abutting_regions_share_boundary_sample() {
        // Region A [0,300], region B [300,600], same column values at the shared x=300 edge.
        let n = 4;
        let a = RegionFactRuntime::new(ramp_grid(n), n, 0.0, 0.0, 300.0, 300.0);
        // B's grid: leftmost column equals A's rightmost column value (n-1).
        let mut bgrid = vec![0.0f32; n * n];
        for r in 0..n { for c in 0..n { bgrid[r * n + c] = (n - 1) as f32 + c as f32; } }
        let b = RegionFactRuntime::new(bgrid, n, 300.0, 0.0, 300.0, 300.0);
        let edge_a = a.sample(300.0, 60.0);
        let edge_b = b.sample(300.0, 60.0);
        assert!((edge_a - edge_b).abs() < 1e-5, "seam: a={edge_a} b={edge_b}");
    }
}
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo test -p wg10_terrain --lib region_fact`
Expected: FAIL — `RegionFactRuntime` not found.

- [ ] **Step 4: Implement `region_fact.rs`** (mirror `static_reference` sampling, texel-corner):

```rust
//! RegionFactRuntime: a baked, carved+conditioned region tile that pages sample. Near-copy of
//! StaticHeightRuntime's grid+bilinear, but the grid comes from a region bake (not JSON) and the
//! region tiles the plane (no outside-height / edge-fade — every page in the region is inside it).

use godot::classes::RenderingDevice;
use godot::prelude::*;
use crate::biome_page_compute::f32s_to_bytes;

#[derive(Clone)]
pub(super) struct RegionFactRuntime {
    grid: Vec<f32>,
    grid_n: usize,
    origin_x_m: f64,
    origin_z_m: f64,
    span_x_m: f64,
    span_z_m: f64,
}

impl RegionFactRuntime {
    pub(super) fn new(
        grid: Vec<f32>, grid_n: usize,
        origin_x_m: f64, origin_z_m: f64, span_x_m: f64, span_z_m: f64,
    ) -> Self {
        assert_eq!(grid.len(), grid_n * grid_n, "RegionFactRuntime: grid not grid_n^2");
        Self { grid, grid_n, origin_x_m, origin_z_m, span_x_m, span_z_m }
    }

    pub(super) fn sample(&self, x_m: f64, z_m: f64) -> f32 {
        let u = ((x_m - self.origin_x_m) / self.span_x_m).clamp(0.0, 1.0);
        let v = ((z_m - self.origin_z_m) / self.span_z_m).clamp(0.0, 1.0);
        let gx = u * (self.grid_n - 1) as f64;
        let gz = v * (self.grid_n - 1) as f64;
        let x0 = gx.floor() as usize;
        let z0 = gz.floor() as usize;
        let x1 = (x0 + 1).min(self.grid_n - 1);
        let z1 = (z0 + 1).min(self.grid_n - 1);
        let tx = (gx - x0 as f64) as f32;
        let tz = (gz - z0 as f64) as f32;
        let g = &self.grid;
        let n = self.grid_n;
        let h00 = g[z0 * n + x0];
        let h10 = g[z0 * n + x1];
        let h01 = g[z1 * n + x0];
        let h11 = g[z1 * n + x1];
        let hx0 = h00 + (h10 - h00) * tx;
        let hx1 = h01 + (h11 - h01) * tx;
        hx0 + (hx1 - hx0) * tz
    }

    pub(super) fn write_page_texture(
        &self, rd: &mut Gd<RenderingDevice>, target_rid: Rid,
        page_origin_x: f64, page_origin_z: f64, world_span: f64, page_px: i64,
    ) -> Result<(), String> {
        if page_px < 2 {
            return Err(format!("region fact: page_px {page_px} must be >= 2"));
        }
        let page_px = page_px as usize;
        let mut samples = vec![0.0_f32; page_px * page_px];
        let denom = (page_px - 1) as f64;
        for z in 0..page_px {
            let wz = page_origin_z + world_span * z as f64 / denom;
            for x in 0..page_px {
                let wx = page_origin_x + world_span * x as f64 / denom;
                samples[z * page_px + x] = self.sample(wx, wz);
            }
        }
        let bytes = f32s_to_bytes(&samples);
        let pba = PackedByteArray::from(bytes.as_slice());
        let err = rd.texture_update(target_rid, 0, &pba);
        if err != godot::global::Error::OK {
            return Err(format!("region fact: texture_update failed: {err:?}"));
        }
        Ok(())
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p wg10_terrain --lib region_fact`
Expected: PASS — both sampling tests green.

- [ ] **Step 6: Commit**

```bash
git add wg-10/rust/src/page_pool/region_fact.rs wg-10/rust/src/page_pool.rs
git commit -m "feat(page_pool): RegionFactRuntime (bilinear sample + per-page texture write)"
```

---

## Task 4: GPU region-macro readback (windowed)

**Files:**
- Create: `wg-10/rust/src/region_bake/gpu_macro.rs`
- Modify: `wg-10/rust/src/region_bake/mod.rs` (`mod gpu_macro;` + re-export)
- Gate: a `*_check.gd` scene exercising it (windowed, editor closed)

> This step touches the GPU, so it is gated WINDOWED, not by a cargo unit test. The function is a
> thin reuse of the existing biome page-compute seam (`build_biome_page_context` +
> `compute_biome_page_cached` + `texture_get_data`), already proven to match `mountain_seamsafe`
> to 1e-6 per-page; here we run it at region scale.

- [ ] **Step 1: Implement `gpu_macro.rs`** (mirror `generate_runtime_page_flow`, region-sized):

```rust
//! GPU region-macro readback: run the proven seam-safe macro (compute_biome_page_cached) over a
//! whole region+apron grid, read it back, crop the apron, return the region-core RAW as Vec<f64>.
//! OFF-FRAME / worker only (deliberate GPU->CPU stall). Bare local RD, no scene/viewport.
use godot::classes::rendering_device::{DataFormat, TextureUsageBits};
use godot::classes::{RdTextureFormat, RdTextureView, RenderingDevice, RenderingServer};
use godot::prelude::*;

// These are re-exported at the biome_page_compute module root (biome_page_compute.rs:53-60),
// NOT at their private submodule paths — import them from the root.
use crate::biome_page_compute::{
    build_biome_page_context, compute_biome_page_cached, free_biome_page_context, bytes_to_f32s,
};

/// Returns the region-core RAW macro field (core_px*core_px, row-major f64), apron cropped.
/// `core_px` is the region grid side (e.g. region_size_m / spacing + 1); `apron_px` the seam apron.
#[allow(clippy::too_many_arguments)]
pub fn gpu_macro_region(
    primitives_src: &str,
    machine_src: &str,
    mountain_fragment_src: &str,
    region_origin_x: f64,
    region_origin_z: f64,
    spacing_m: f64,
    core_px: usize,
    apron_px: usize,
    flow_iters: usize,
    feature_span_m: f64,
    seed: i64,
    flow_on: bool,
) -> Result<Vec<f64>, String> {
    let mut rd: Gd<RenderingDevice> = RenderingServer::singleton()
        .create_local_rendering_device()
        .ok_or("gpu_macro_region: create_local_rendering_device returned null")?;

    let ctx = build_biome_page_context(
        &mut rd, primitives_src, machine_src, mountain_fragment_src,
        core_px, apron_px, flow_iters.max(1), 1.0,
    ).inspect_err(|_| rd.clone().free())?;

    let mut fmt = RdTextureFormat::new_gd();
    fmt.set_width(core_px as u32);
    fmt.set_height(core_px as u32);
    fmt.set_format(DataFormat::R32_SFLOAT);
    fmt.set_usage_bits(
        TextureUsageBits::STORAGE_BIT | TextureUsageBits::SAMPLING_BIT | TextureUsageBits::CAN_COPY_FROM_BIT,
    );
    let view = RdTextureView::new_gd();
    let tex = rd.texture_create(&fmt, &view);
    if tex.is_invalid() {
        free_biome_page_context(&mut rd, &ctx);
        rd.free();
        return Err("gpu_macro_region: texture_create invalid".into());
    }

    let page_px = core_px as i64;
    let world_span = spacing_m * (page_px as f64 - 1.0);
    let origin_x = region_origin_x + apron_px as f64 * spacing_m;
    let origin_z = region_origin_z + apron_px as f64 * spacing_m;

    if let Err(e) = compute_biome_page_cached(
        &mut rd, &ctx, tex, origin_x, origin_z, world_span, page_px, feature_span_m, seed, flow_on,
    ) {
        rd.free_rid(tex);
        free_biome_page_context(&mut rd, &ctx);
        rd.free();
        return Err(format!("gpu_macro_region: {e}"));
    }

    let raw = rd.texture_get_data(tex, 0);
    let core = bytes_to_f32s(&raw.to_vec());
    rd.free_rid(tex);
    free_biome_page_context(&mut rd, &ctx);
    rd.free();

    if core.len() != core_px * core_px {
        return Err(format!("gpu_macro_region: readback {} != {}", core.len(), core_px * core_px));
    }
    Ok(core.iter().map(|&v| v as f64).collect())
}
```

> NOTE: `compute_biome_page_cached` already returns the apron-cropped CORE (the context's
> `core_px` excludes the apron — see `generate_runtime_page_flow`, which passes `core_px = padded - 2*apron`).
> So the returned field is already the region core; no extra crop here. Confirm against
> `runtime_context.rs` when implementing — if the readback includes the apron, crop it here.

- [ ] **Step 2: Wire `mod gpu_macro;`** in `region_bake/mod.rs` and `pub use gpu_macro::gpu_macro_region;`.
The imported items (`build_biome_page_context`, `compute_biome_page_cached`, `free_biome_page_context`,
`bytes_to_f32s`) are already `pub(crate)` re-exports at `crate::biome_page_compute::` — no visibility
changes needed.

- [ ] **Step 3: Isolated compile check** (no GPU run, just that it builds)

Run: `cargo test -p wg10_terrain --lib --no-run`
Expected: compiles clean (no run needed yet).

- [ ] **Step 4: Add the GPU readback in a Godot-exposed func for the gate** — add a `#[func]` to the
existing `Wg10BiomePageCompute` (in `biome_page_compute/page_api.rs`) named
`bake_region_macro_readback(spacing, ox, oz, core_px, apron_px, flow_iters, feature_span_m, seed, flow_on, mountain_fragment_path) -> PackedFloat64Array`
that loads the shader sources (as `generate_runtime_page_flow` does), calls `gpu_macro_region`, and
returns the field. This is the windowed-gate entry (mirrors the existing readback funcs' arg shape).

- [ ] **Step 5: Write the windowed gate** `wg-10/godot/tests/region_macro_readback_check.gd`: a BARE
`Wg10BiomePageCompute`, call `bake_region_macro_readback` at a small region scale (e.g. core_px=129,
apron=16), and assert the field matches a CPU `mountain_seamsafe` reference (exported fixture or a
second CPU-path call) within `1e-5`. Print `[wg10-region-macro] status=pass maxd=...`.

- [ ] **Step 6: Run the windowed gate** (editor closed)

```powershell
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'
python tools\gate.py --suite region_macro   # (add the suite entry pointing at the new .gd)
```
Expected: `[wg10-region-macro] status=pass`, maxd ≤ 1e-5.

- [ ] **Step 7: Commit**

```bash
git add wg-10/rust/src/region_bake/gpu_macro.rs wg-10/rust/src/region_bake/mod.rs wg-10/rust/src/biome_page_compute/page_api.rs wg-10/godot/tests/region_macro_readback_check.gd tools/gate.py
git commit -m "feat(region_bake): GPU region-macro readback (reuse biome page-compute at region scale) + windowed gate"
```

---

## Task 5: G-seam measurement gate (cross-region condition seam)

**Files:**
- Create: `tools/dem_pack/export_region_seam_fixture.py` (two adjacent regions' RAW+carve fields)
- Test: `wg-10/rust/src/region_bake/seam_tests.rs`

> This gate MEASURES the seam before any reconcile is built. It decides whether a simple
> deterministic percentile reconcile suffices or a coarse-global field is needed. Run it, read the
> numbers, then implement Task 6 accordingly.

- [ ] **Step 1: Export the fixture** — `export_region_seam_fixture.py` emits, for two adjacent
regions A=(0,0) and B=(1,0) at `region_size_m=32768`, each region's RAW (seam-safe macro) +
carved field (raw+delta) at a modest grid (e.g. n=129), as `region_seam_fixture.json`. Reuse the
existing seam-safe macro + carve Python (the same path `bake_region_fixture.json` used).

- [ ] **Step 2: Write the measurement test** — `region_bake/seam_tests.rs`:

```rust
//! G-seam: measure the cross-region condition seam. Two adjacent regions condition their shared
//! border with per-region percentiles; quantify (a) percentile drift, (b) conditioned-height delta
//! along the shared border column. The verdict drives the reconcile rule (Task 6).
#[test]
fn measure_cross_region_condition_seam() {
    use std::path::Path;
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/dem_pack/fixtures/region_seam_fixture.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let n = v["n"].as_u64().unwrap() as usize;
    let carved_a: Vec<f64> = v["carved_a"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let carved_b: Vec<f64> = v["carved_b"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();

    let (ha, sa) = crate::condition_world::condition_world(&carved_a, n);
    let (hb, sb) = crate::condition_world::condition_world(&carved_b, n);

    let p05_drift = (sa.p05 - sb.p05).abs();
    let p50_drift = (sa.p50 - sb.p50).abs();
    let p95_drift = (sa.p95 - sb.p95).abs();

    // Shared border: A's rightmost column (x = n-1) vs B's leftmost column (x = 0), same z rows.
    let mut max_border_delta = 0.0f64;
    for r in 0..n {
        let a_edge = ha[r * n + (n - 1)];
        let b_edge = hb[r * n];
        max_border_delta = max_border_delta.max((a_edge - b_edge).abs());
    }
    // Conditioned units are tanh (~[-1,1]); convert to metres with the fixture height scale.
    let hs = v["height_scale_m"].as_f64().unwrap();
    println!("[g-seam] p05_drift={p05_drift:.4} p50_drift={p50_drift:.4} p95_drift={p95_drift:.4} | max_border_delta(tanh)={max_border_delta:.4} ~= {:.3}m", max_border_delta * hs);

    // This test ALWAYS passes; it is a measurement. The PRINTED numbers decide Task 6's rule.
    // Guardrail only: the fields must be non-trivial (not all-zero) so the measurement is real.
    assert!(ha.iter().any(|&x| x.abs() > 1e-6) && hb.iter().any(|&x| x.abs() > 1e-6), "vacuous seam fixture");
}
```

- [ ] **Step 2b: Wire the module** — `mod seam_tests;` (under `#[cfg(test)]`) in `region_bake/mod.rs`.

- [ ] **Step 3: Run the gate and RECORD the numbers**

Run: `cargo test -p wg10_terrain --lib measure_cross_region_condition_seam -- --nocapture`
Expected: PASS; capture the `[g-seam]` line. **Decision rule:**
- If `max_border_delta * hs` ≲ 0.15 m (the accepted condition-residual budget): a deterministic
  percentile reconcile (Task 6) will easily close it — proceed with the key-deterministic blend.
- If it is large (metres): the reconcile must be stronger; record the number and choose
  reconcile-vs-coarse with the data per the spec's G-seam branch (and add a look-parity guard).

- [ ] **Step 4: Commit** (the measurement + fixture exporter)

```bash
git add tools/dem_pack/export_region_seam_fixture.py tools/dem_pack/fixtures/region_seam_fixture.json wg-10/rust/src/region_bake/seam_tests.rs wg-10/rust/src/region_bake/mod.rs
git commit -m "test(region_bake): G-seam measurement gate (cross-region condition drift + border delta)"
```

---

## Task 6: Deterministic percentile reconcile (verdict-driven)

**Files:**
- Modify: `wg-10/rust/src/region_bake/mod.rs` (`reconciled_percentiles` helper)
- Test: `wg-10/rust/src/region_bake/seam_tests.rs`

> Implement the rule G-seam selected. The default (small-drift case) below: each region quantizes
> its own percentiles to a shared grid keyed by region position, so two regions that share a border
> agree at that border by construction — a DETERMINISTIC function of region keys + each region's own
> field (no region bake blocks on a neighbor, per the spec's ordering risk).

- [ ] **Step 1: Write the failing test** — add to `seam_tests.rs`:

```rust
#[test]
fn reconciled_percentiles_are_seam_deterministic() {
    // Two regions with slightly different true percentiles must produce a reconcile that, applied
    // to BOTH, yields a border-conditioned delta below the budget. Uses the same fixture.
    use std::path::Path;
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/dem_pack/fixtures/region_seam_fixture.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let n = v["n"].as_u64().unwrap() as usize;
    let hs = v["height_scale_m"].as_f64().unwrap();
    let carved_a: Vec<f64> = v["carved_a"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let carved_b: Vec<f64> = v["carved_b"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();

    let rp = super::reconciled_percentiles(&carved_a, &carved_b, n);
    let ha = crate::condition_world::condition_world_with_percentiles(&carved_a, n, rp.p05, rp.p50, rp.p95);
    let hb = crate::condition_world::condition_world_with_percentiles(&carved_b, n, rp.p05, rp.p50, rp.p95);
    let mut maxd = 0.0f64;
    for r in 0..n { maxd = maxd.max((ha[r * n + (n - 1)] - hb[r * n]).abs()); }
    assert!(maxd * hs < 0.15, "reconciled border still seams: {:.3}m", maxd * hs);
}
```

> If G-seam found LARGE drift, replace this assertion's approach per the recorded verdict (e.g.
> assert the coarse-global path's look-parity instead). Do not weaken the budget to force green.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p wg10_terrain --lib reconciled_percentiles_are_seam_deterministic`
Expected: FAIL — `reconciled_percentiles` not found.

- [ ] **Step 3: Implement `reconciled_percentiles`** in `region_bake/mod.rs`. The small-drift default
(average the two regions' percentiles — deterministic and symmetric, so both sides compute the same):

```rust
use crate::condition_world::percentile_linear;

/// Border-reconciled conditioning percentiles for two adjacent regions: the symmetric mean of each
/// region's own percentiles. Deterministic (same result regardless of which side computes it), so a
/// region never blocks on a neighbor's bake — it only needs the neighbor's percentiles, which are a
/// cheap reduction of the neighbor's already-available RAW+carve field (or recomputed identically).
pub fn reconciled_percentiles(carved_a: &[f64], carved_b: &[f64], n: usize) -> RegionPercentiles {
    let _ = n; // n unused; percentiles are over the full field
    let pct = |f: &[f64], q: f64| { let mut s = f.to_vec(); s.sort_by(|a,b| a.partial_cmp(b).unwrap()); percentile_linear(&s, q) };
    let p = |q: f64| (pct(carved_a, q) + pct(carved_b, q)) * 0.5;
    RegionPercentiles { p05: p(5.0), p50: p(50.0), p95: p(95.0) }
}
```

(`RegionPercentiles` is defined in this same `mod.rs` (Task 2), so it resolves unqualified.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wg10_terrain --lib reconciled_percentiles_are_seam_deterministic`
Expected: PASS — reconciled border delta under 0.15 m.

- [ ] **Step 5: Commit**

```bash
git add wg-10/rust/src/region_bake/mod.rs wg-10/rust/src/region_bake/seam_tests.rs
git commit -m "feat(region_bake): deterministic cross-region percentile reconcile (seam-closing)"
```

---

## Task 7: Async bake worker (own RD, channel round-trip — windowed)

**Files:**
- Create: `wg-10/rust/src/region_bake/worker.rs`
- Modify: `wg-10/rust/src/region_bake/mod.rs` (`mod worker;`)
- Gate: windowed (the worker dispatches GPU)

> The worker bakes ≥ 2 regions back-to-back in the gate to catch RD-context-reuse corruption (the
> spec's "context reuse vs RD state" risk).

- [ ] **Step 1: Implement `worker.rs`**:

```rust
//! Async region-bake worker: a dedicated thread owning its own local RenderingDevice. Region keys
//! (+ params) in via one channel; finished baked grids out via another. RD is per-thread — never
//! shared with the pool's RD.
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;
use crate::pass_network::{PassNetworkParams, RampParams, TraverseParams};

pub struct BakeRequest {
    pub region_key: (i64, i64),
    pub region_origin_x: f64,
    pub region_origin_z: f64,
    pub spacing_m: f64,
    pub core_px: usize,
    pub apron_px: usize,
    pub flow_iters: usize,
    pub flow_on: bool,
    pub feature_span_m: f64,
    pub span_m: f64,
    pub height_scale_m: f64,
    pub seed: i64,
    pub pass: PassNetworkParams,
    pub traverse: TraverseParams,
    pub ramp: RampParams,
}

pub struct BakedRegion {
    pub region_key: (i64, i64),
    pub result: Result<(Vec<f32>, usize), String>, // (conditioned grid, grid_n)
}

pub struct BakeWorker {
    pub tx: Sender<BakeRequest>,
    pub rx: Receiver<BakedRegion>,
    handle: Option<JoinHandle<()>>,
}

impl BakeWorker {
    /// `shaders` carries the three GLSL sources the GPU macro needs (primitives, machine, fragment).
    pub fn spawn(primitives: String, machine: String, fragment: String) -> Self {
        let (req_tx, req_rx) = std::sync::mpsc::channel::<BakeRequest>();
        let (out_tx, out_rx) = std::sync::mpsc::channel::<BakedRegion>();
        let handle = std::thread::Builder::new()
            .name("wg10-region-bake".into())
            .spawn(move || worker_loop(req_rx, out_tx, primitives, machine, fragment))
            .expect("spawn region-bake worker");
        Self { tx: req_tx, rx: out_rx, handle: Some(handle) }
    }
}

impl Drop for BakeWorker {
    fn drop(&mut self) {
        // Dropping tx closes the channel; the loop exits; join.
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn worker_loop(
    rx: Receiver<BakeRequest>, tx: Sender<BakedRegion>,
    primitives: String, machine: String, fragment: String,
) {
    while let Ok(req) = rx.recv() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bake_one(&req, &primitives, &machine, &fragment)
        })).unwrap_or_else(|_| Err("region bake panicked".into()));
        if tx.send(BakedRegion { region_key: req.region_key, result }).is_err() {
            break; // pool gone
        }
    }
}

fn bake_one(req: &BakeRequest, prim: &str, machine: &str, frag: &str) -> Result<(Vec<f32>, usize), String> {
    let raw = super::gpu_macro::gpu_macro_region(
        prim, machine, frag, req.region_origin_x, req.region_origin_z, req.spacing_m,
        req.core_px, req.apron_px, req.flow_iters, req.feature_span_m, req.seed, req.flow_on,
    )?;
    let n = req.core_px;
    let baked = super::bake_region_from_raw(
        &raw, n, req.span_m, req.height_scale_m, &req.pass, &req.traverse, &req.ramp, None,
    );
    let grid: Vec<f32> = baked.height.iter().map(|&h| (h * req.height_scale_m) as f32).collect();
    Ok((grid, n))
}
```

> NOTE on the conditioned→metres scale: pages render metres. The conditioned field is tanh-unit;
> multiply by `height_scale_m` here (matching what the static reference grid stores). Confirm the
> exact scale against `static_reference` payload loading when wiring Task 8; adjust the one line in
> `bake_one` if the reference stores a different convention.

- [ ] **Step 2: Wire `mod worker;`** in `region_bake/mod.rs`.

- [ ] **Step 3: Isolated compile check**

Run: `cargo test -p wg10_terrain --lib --no-run`
Expected: compiles clean.

- [ ] **Step 4: Windowed round-trip gate** — add a `#[func]` on a test helper (or extend the Task-4
gate func) `bake_region_via_worker(...) -> PackedFloat64Array` that spawns a `BakeWorker`, sends two
adjacent region requests, receives both, and returns one region's grid. The `.gd` gate asserts:
(a) both regions return `Ok`, (b) the worker's grid sampled at a few points equals a synchronous
`gpu_macro_region`+`bake_region_from_raw` of the same region within `1e-4`. Print
`[wg10-bake-worker] status=pass regions=2 maxd=...`.

- [ ] **Step 5: Run the windowed gate** (editor closed)

```powershell
python tools\gate.py --suite bake_worker
```
Expected: `[wg10-bake-worker] status=pass regions=2`, maxd ≤ 1e-4.

- [ ] **Step 6: Commit**

```bash
git add wg-10/rust/src/region_bake/worker.rs wg-10/rust/src/region_bake/mod.rs wg-10/godot/tests/bake_worker_check.gd tools/gate.py
git commit -m "feat(region_bake): async bake worker (own RD, channel round-trip), 2-region reuse gate"
```

---

## Task 8: Producer arm + region LRU + on-screen Rung-1 gate

**Files:**
- Modify: `wg-10/rust/src/page_pool/producer.rs` (`ProducerKind::RegionFact` + dispatch)
- Modify: `wg-10/rust/src/page_pool.rs` (region cache field, worker handle, tick drain, configure func)
- Gate: windowed on-screen Rung-1

- [ ] **Step 1: Add the producer kind** — in `producer.rs`, extend `ProducerKind`:

```rust
    RegionFact,
```
add its `runtime_mode` arm (`Self::RegionFact => "region_fact"`), include it in `uses_biome_path`
(it's a real generated path → `true`), and in `active_producer_kind` return
`Some(ProducerKind::RegionFact)` when the region-fact producer is configured (a new
`self.region_fact_cfg.is_some()` check, placed ABOVE `StaticReference` so it wins when configured).

- [ ] **Step 2: Add pool state** — in `page_pool.rs`, add fields: the `BakeWorker`, an LRU/map
`region_cache: HashMap<(i64,i64), RegionFactRuntime>` bounded by a region capacity (reuse the
generic `PagePolicy` keyed by region key), a `baking: HashSet<(i64,i64)>` set, and the bake params
(seed, sizes, GLSL paths). Add a `#[func] configure_region_fact(...)` that loads the shader sources,
spawns the `BakeWorker`, and records params.

- [ ] **Step 3: Drain finished bakes each tick** — in the pool's per-tick entry (where producers are
serviced), add: `while let Ok(done) = self.worker.rx.try_recv() { ... }` — on `Ok`, insert the
`RegionFactRuntime` into `region_cache` (evicting via the policy) and remove from `baking`; on `Err`,
log and remove from `baking` (no tight retry).

- [ ] **Step 4: Dispatch arm** — in `dispatch_page_compute`, add the `RegionFact` branch:

```rust
            Some(ProducerKind::RegionFact) => {
                let (rx, rz) = crate::grammar::region_of(origin_x, origin_z, self.pack_ref());
                if let Some(region) = self.region_cache.get(&(rx, rz)) {
                    region.write_page_texture(rd, tex_rid, origin_x, origin_z, world_span, page_px)
                } else {
                    // Not baked: enqueue once, render the coarse fallback for this page so the
                    // screen is never black.
                    if self.baking.insert((rx, rz)) {
                        let _ = self.worker.tx.send(self.make_bake_request(rx, rz));
                    }
                    self.dispatch_coarse_fallback(rd, tex_rid, origin_x, origin_z, world_span, page_px)
                }
            }
```

`make_bake_request` builds a `BakeRequest` from pool params + `region_of` origin.
`dispatch_coarse_fallback` reuses the pool's existing pre-region producer (e.g. the world/biome
GPU page or a coarse closed-form) for not-yet-baked pages — wire it to whatever the pool already
shows as the streaming fallback.

- [ ] **Step 5: Isolated compile + routing unit test** — add a pure routing test (mock the cache):
baked region routes to `RegionFact`; an unbaked region enqueues exactly once and falls through.
(If pool routing can't be unit-tested without GPU, assert the smaller pieces: `region_of` keying +
`baking` insert-once semantics in a focused test.)

Run: `cargo test -p wg10_terrain --lib region_fact producer`
Expected: PASS.

- [ ] **Step 6: On-screen Rung-1 gate** (windowed, editor closed) — extend the un-intercept ladder
Rung-1 scene (or a new `region_fact_runtime_check.gd`): configure the pool with the region-fact
producer, drive the camera into a region, pump frames until the bake completes (drain shows the
region cached), read back a page via the bare-pool readback recipe, and assert the on-screen height
matches `bake_region`'s CPU oracle for that region within the established bar (height p99 ≤ ~0.15 m,
matching the bake parity budget). This is the gate that proves **the carved look reaches the screen**.
Print `[wg10-region-rung1] status=pass p99_m=...`.

> Use the `gpu-readback-bare-pool` recipe: bare `Wg10PagePool`, no camera/viewport for the numeric
> readback; `process_frame` to let the worker finish + the pool drain; then `get_resident_page` →
> `texture_get_data`. Flow off = `flow_max_level=0`, never `flow_iters=0`.

- [ ] **Step 7: Run the on-screen gate**

```powershell
python tools\gate.py --suite region_rung1
```
Expected: `[wg10-region-rung1] status=pass`, p99 ≤ ~0.15 m. The carved look is on screen.

- [ ] **Step 8: Full sweep** — confirm nothing regressed.

Run: `cargo test -p wg10_terrain --lib`
Expected: all prior 251 + the new tests green.

- [ ] **Step 9: Commit**

```bash
git add wg-10/rust/src/page_pool/producer.rs wg-10/rust/src/page_pool.rs wg-10/godot/tests/region_fact_runtime_check.gd tools/gate.py
git commit -m "feat(page_pool): RegionFact producer arm + region LRU + worker drain; on-screen Rung-1 gate (carved look on screen)"
```

---

## Task 9: Update STATUS + handoff, push

**Files:**
- Modify: `docs/plans/STATUS.md` (top), `docs/plans/WG10_HANDOFF_2026-06-05_CARVE_PORTED.md` (mark integration done / note follow-ons)

- [ ] **Step 1: Record the outcome** — STATUS top: region-fact producer wired; carved look on screen
(Rung-1 gate green, p99 number); G-seam verdict (the measured drift + reconcile rule chosen);
worker-throughput note (any follow-on for speed). Quote the actual gate numbers.

- [ ] **Step 2: Commit + push**

```bash
git add docs/plans/STATUS.md docs/plans/WG10_HANDOFF_2026-06-05_CARVE_PORTED.md
git commit -m "docs: region-fact producer wired (carved look on screen); G-seam verdict recorded"
git push origin slice4-gpu-page-integration
```

---

## Self-review notes (coverage vs spec)

- Spec component 1 (RegionFactRuntime) → Task 3. ✓
- Component 2 (GPU region-macro readback) → Task 4. ✓
- Component 3 (bake_region_from_raw) → Task 2. ✓
- Component 4 (async worker) → Task 7. ✓
- Component 5 (producer arm + region LRU) → Task 8. ✓
- Component 6 (seam-reconciled conditioning) → Tasks 1 (injected-percentile fn) + 6 (reconcile rule). ✓
- G-seam measurement gate → Task 5 (before the producer arm, as the spec requires). ✓
- On-screen Rung-1 gate → Task 8. ✓
- Error handling (worker failure no tight-retry, RD-unavailable, panic boundary, channel disconnect)
  → Task 7 (`catch_unwind`, channel drop) + Task 8 (drain `Err` → remove from `baking`). ✓
- Reconcile-vs-neighbor ordering risk → Task 6 uses a deterministic symmetric mean (no bake blocks
  on a neighbor's bake). ✓

**Known confirm-on-implement points (flagged inline, not placeholders):**
- Task 4 Step 1: whether `compute_biome_page_cached` returns apron-cropped core (it does for the
  existing 576 path) — crop only if the readback includes the apron.
- Task 7 Step 1: the conditioned→metres scale convention — match `static_reference` exactly when
  wiring Task 8.
- Task 8: `dispatch_coarse_fallback` binds to whatever the pool already streams as the fallback
  producer; identify it during implementation (the existing World/SingleBiome GPU page).
```
