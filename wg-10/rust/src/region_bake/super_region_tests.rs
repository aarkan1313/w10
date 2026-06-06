//! Super-region bake-then-slice: carving ONE super-field then slicing into region grids is
//! seam-exact at internal borders (the per-region global-Dijkstra carve seams; this does not).
use crate::pass_network::{PassNetworkParams, RampParams, TraverseParams};
use crate::region_bake::{
    bake_region_from_raw_with_provider, bake_super_region, RegionSlice, ScalarRegionPercentiles,
};

// Replicate RegionFactRuntime::sample EXACTLY (page_pool/region_fact.rs::sample): texel-corner
// bilinear, u=clamp((x-origin)/span,0,1); gx=u*(grid_n-1); floor; bilinear. RegionFactRuntime is
// pub(in crate::page_pool) so it is NOT reachable here; this inline copy lets GATE 2 test the
// sampling MATH a RegionFactRuntime would do over a slice without constructing one (and without
// changing its visibility for a test). Kept in f64 (the slice grid is f64; the runtime is f32 — we
// test the index/origin math, which is identical, not the f32 precision).
fn sample_slice(s: &RegionSlice, x_m: f64, z_m: f64) -> f64 {
    let n = s.grid_n;
    let u = ((x_m - s.origin_x_m) / s.span_m).clamp(0.0, 1.0);
    let v = ((z_m - s.origin_z_m) / s.span_m).clamp(0.0, 1.0);
    let gx = u * (n - 1) as f64;
    let gz = v * (n - 1) as f64;
    let x0 = gx.floor() as usize;
    let z0 = gz.floor() as usize;
    let x1 = (x0 + 1).min(n - 1);
    let z1 = (z0 + 1).min(n - 1);
    let tx = gx - x0 as f64;
    let tz = gz - z0 as f64;
    let g = &s.grid;
    let h00 = g[z0 * n + x0];
    let h10 = g[z0 * n + x1];
    let h01 = g[z1 * n + x0];
    let h11 = g[z1 * n + x1];
    let hx0 = h00 + (h10 - h00) * tx;
    let hx1 = h01 + (h11 - h01) * tx;
    hx0 + (hx1 - hx0) * tz
}

fn synth_raw(super_n: usize) -> Vec<f64> {
    // A deterministic non-trivial macro stand-in over the whole super-field.
    let mut v = vec![0.0f64; super_n*super_n];
    for r in 0..super_n { for c in 0..super_n {
        let x = c as f64 / super_n as f64; let z = r as f64 / super_n as f64;
        v[r*super_n + c] = (x*7.0).sin()*(z*5.0).cos()*1.5 + ((r*super_n+c)%17) as f64*0.04;
    }}
    v
}

#[test]
fn super_region_slices_are_seam_exact_internally() {
    let n = 33usize;            // region grid side
    let k = 2usize;            // 2x2 super-region
    let super_n = k*(n-1) + 1; // 65
    let span_m = 25600.0; let hs = 260.0;
    let super_x0 = 100000.0; let super_z0 = 50000.0;
    let raw = synth_raw(super_n);
    let pass = PassNetworkParams::default();
    let traverse = TraverseParams { scene_width_m: span_m*k as f64, height_scale_m: hs, ..Default::default() };
    let ramp = RampParams::default();

    // Bake the super-region: carve+condition over the whole super-field, slice into k*k regions.
    let slices = bake_super_region(&raw, super_n, n, k, span_m, hs, super_x0, super_z0,
        &pass, &traverse, &ramp, &ScalarRegionPercentiles);
    assert_eq!(slices.len(), k*k);

    // Region (0,0) right edge must equal region (1,0) left edge (shared internal X border), bit-exact.
    let idx = |gi: usize, gj: usize| gj*k + gi;
    let r00 = &slices[idx(0,0)];
    let r10 = &slices[idx(1,0)];
    assert_eq!(r00.grid_n, n);
    // r00 col n-1 vs r10 col 0, all rows:
    for row in 0..n {
        let a = r00.grid[row*n + (n-1)];
        let b = r10.grid[row*n + 0];
        assert_eq!(a.to_bits(), b.to_bits(), "internal X seam at row {row}: {a} vs {b}");
    }
    // Region (0,0) bottom edge vs region (0,1) top edge (shared internal Z border):
    let r01 = &slices[idx(0,1)];
    for col in 0..n {
        let a = r00.grid[(n-1)*n + col];
        let b = r01.grid[0*n + col];
        assert_eq!(a.to_bits(), b.to_bits(), "internal Z seam at col {col}: {a} vs {b}");
    }
    // Each slice carries its correct world origin (texel-corner, no overlap in world coords beyond the shared edge).
    assert!((r10.origin_x_m - (super_x0 + span_m)).abs() < 1e-6, "r10 x origin {}", r10.origin_x_m);
    assert!((r01.origin_z_m - (super_z0 + span_m)).abs() < 1e-6, "r01 z origin {}", r01.origin_z_m);
}

#[test]
fn super_region_k1_equals_single_region() {
    // k=1 super-region == today's single bake_region_from_raw_with_provider (no slicing).
    let n = 40usize; let span_m = 25600.0; let hs = 260.0;
    let raw = synth_raw(n);
    let pass = PassNetworkParams::default();
    let traverse = TraverseParams { scene_width_m: span_m, height_scale_m: hs, ..Default::default() };
    let ramp = RampParams::default();
    let slices = bake_super_region(&raw, n, n, 1, span_m, hs, 0.0, 0.0, &pass, &traverse, &ramp, &ScalarRegionPercentiles);
    assert_eq!(slices.len(), 1);
    let single = crate::region_bake::bake_region_from_raw_with_provider(
        &raw, n, span_m, hs, 0.0, 0.0, &pass, &traverse, &ramp, &ScalarRegionPercentiles);
    for i in 0..n*n { assert_eq!(slices[0].grid[i].to_bits(), single.height[i].to_bits(), "k1 cell {i}"); }
}

// ---- GATE 2: end-to-end slice -> sample bit-faithfulness. ----
// Prove that a baked super-region's SLICED region grid, sampled (with RegionFactRuntime's EXACT
// bilinear math, replicated inline above) at its grid-corner world positions, returns the slice's
// grid values. This is the sample math a RegionFactRuntime does over a slice — proving it has no
// off-by-one / origin error. At a grid corner the bilinear fractions are 0 (so it IS the cell value),
// modulo float roundoff in u*(n-1) at the far edge.
#[test]
fn slice_sampled_at_grid_corners_returns_grid_values() {
    let n = 33usize;
    let k = 2usize;
    let super_n = k * (n - 1) + 1; // 65
    let span_m = 25600.0;
    let hs = 260.0;
    let super_x0 = 100000.0;
    let super_z0 = 50000.0;
    let raw = synth_raw(super_n);
    let pass = PassNetworkParams::default();
    let traverse = TraverseParams { scene_width_m: span_m * k as f64, height_scale_m: hs, ..Default::default() };
    let ramp = RampParams::default();

    let slices = bake_super_region(&raw, super_n, n, k, span_m, hs, super_x0, super_z0,
        &pass, &traverse, &ramp, &ScalarRegionPercentiles);

    let cell_m = span_m / (n as f64 - 1.0);
    let mut max_err = 0.0f64;
    for s in &slices {
        for r in 0..n {
            let wz = s.origin_z_m + r as f64 * cell_m;
            for c in 0..n {
                let wx = s.origin_x_m + c as f64 * cell_m;
                let got = sample_slice(s, wx, wz);
                let want = s.grid[r * n + c];
                max_err = max_err.max((got - want).abs());
            }
        }
    }
    println!("[slice-sample] max |sample(corner) - grid| = {max_err:.3e} (grid-corner bilinear, t=0)");
    // Grid-corner sampling has t=0 so it reads the cell directly; only u*(n-1) far-edge roundoff can
    // perturb it. Measured exact (0.0); a 1e-4 bar covers any roundoff without hiding an off-by-one.
    assert!(max_err < 1e-4, "slice sample not faithful to grid: max_err={max_err:.3e}");
}

// CROSS-CHECK: a k=1 super-region's single slice, sampled at the region center, matches
// bake_region_from_raw_with_provider's conditioned height at the corresponding cell (tanh units —
// RegionSlice.grid is tanh, NOT metres; the worker scales to metres later, so compare in tanh units).
#[test]
fn k1_slice_sample_matches_single_bake_at_center() {
    let n = 41usize; // odd -> exact integer center index (n-1)/2
    let span_m = 25600.0;
    let hs = 260.0;
    let raw = synth_raw(n);
    let pass = PassNetworkParams::default();
    let traverse = TraverseParams { scene_width_m: span_m, height_scale_m: hs, ..Default::default() };
    let ramp = RampParams::default();

    let slices = bake_super_region(&raw, n, n, 1, span_m, hs, 0.0, 0.0, &pass, &traverse, &ramp, &ScalarRegionPercentiles);
    assert_eq!(slices.len(), 1);
    let s = &slices[0];
    let single = bake_region_from_raw_with_provider(
        &raw, n, span_m, hs, 0.0, 0.0, &pass, &traverse, &ramp, &ScalarRegionPercentiles);

    // Region center cell (cc,cc) at world position (cc*cell_m, cc*cell_m).
    let cc = (n - 1) / 2;
    let cell_m = span_m / (n as f64 - 1.0);
    let wx = s.origin_x_m + cc as f64 * cell_m;
    let wz = s.origin_z_m + cc as f64 * cell_m;
    let sampled = sample_slice(s, wx, wz);
    let baked_center = single.height[cc * n + cc];
    println!("[slice-sample-xcheck] k1 center: sampled={sampled:.6} baked={baked_center:.6} (tanh units)");
    assert!((sampled - baked_center).abs() < 1e-4,
        "k1 slice center sample {sampled} != single bake center {baked_center} (tanh)");
}
