//! Super-region bake-then-slice: carving ONE super-field then slicing into region grids is
//! seam-exact at internal borders (the per-region global-Dijkstra carve seams; this does not).
use crate::pass_network::{PassNetworkParams, RampParams, TraverseParams};
use crate::region_bake::{bake_super_region, ScalarRegionPercentiles};

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
