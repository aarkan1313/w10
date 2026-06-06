//! region_bake: the carve -> condition TAIL of the baked-look pipeline, fed a RAW field that
//! comes from the GPU region-macro readback (the live path) instead of the CPU macro. The CPU
//! macro entry (`bake_region::bake_region`) delegates here so the existing end-to-end parity
//! gate still covers the tail.
#![allow(dead_code)]
use crate::condition_world::{condition_world, condition_world_with_percentile_fields, condition_world_with_percentiles, ConditionStats};
use crate::pass_network::{carve_ramp_delta, carve_routes, PassNetworkParams, RampParams, TraverseParams};

mod gpu_macro;
pub use gpu_macro::gpu_macro_region;

mod percentile_provider;
// Public engine-API surface: the swappable percentile-provider types. Some are not yet referenced
// inside the crate (consumed by tests / downstream games / pending wiring) -> allow unused re-export.
#[allow(unused_imports)]
pub use percentile_provider::{
    PercentileFields, PercentileProvider, ScalarRegionPercentiles, SmoothFieldPercentiles,
};

mod worker;
// Public worker API surface (result types are consumed by the pool's drain + tests). Allow the
// re-exports the crate doesn't reference internally yet.
#[allow(unused_imports)]
pub use worker::{BakeWorker, BakedRegionFact, SuperBakeRequest, SuperBakeResult};

#[cfg(test)]
mod region_bake_tests;
#[cfg(test)]
mod percentile_seam_tests;
#[cfg(test)]
mod super_region_tests;
#[cfg(test)]
mod outer_seam_tests;

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

/// Carve (on RAW) -> raw+delta -> condition VIA a PercentileProvider (the swappable cross-region
/// seam strategy). `region_x0_m/region_z0_m` are the region's world origin (the smooth provider
/// needs them; the scalar provider ignores them). Routing the `None`/scalar path through
/// ScalarRegionPercentiles is BIT-EXACT to `condition_world` (gated).
#[allow(clippy::too_many_arguments)]
pub fn bake_region_from_raw_with_provider(
    raw: &[f64],
    n: usize,
    span_m: f64,
    height_scale_m: f64,
    region_x0_m: f64,
    region_z0_m: f64,
    pass: &PassNetworkParams,
    traverse: &TraverseParams,
    ramp: &RampParams,
    provider: &dyn PercentileProvider,
) -> BakeResult {
    let routes = carve_routes(raw, n, span_m, height_scale_m, pass, traverse);
    let carve_delta = carve_ramp_delta(raw, n, span_m, height_scale_m, &routes, ramp);
    let raw_carved: Vec<f64> = raw.iter().zip(carve_delta.iter()).map(|(r, d)| r + d).collect();
    let pf = provider.percentiles(&raw_carved, region_x0_m, region_z0_m, span_m, n);
    let height = condition_world_with_percentile_fields(&raw_carved, n, &pf.p05, &pf.p50, &pf.p95);
    // stats: source from sorted raw_carved ends; percentiles = the field MEAN (well-defined for
    // length-1 broadcast AND per-cell fields); conditioned from the shaped field.
    let mut sorted = raw_carved.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = |f: &[f64]| f.iter().sum::<f64>() / f.len() as f64;
    let (mut cmin, mut cmax) = (height[0], height[0]);
    for &v in &height { if v < cmin { cmin = v; } if v > cmax { cmax = v; } }
    let stats = ConditionStats {
        source_min: sorted[0], source_max: sorted[n * n - 1], source_ptp: sorted[n * n - 1] - sorted[0],
        p05: mean(&pf.p05), p50: mean(&pf.p50), p95: mean(&pf.p95),
        conditioned_min: cmin, conditioned_max: cmax, conditioned_ptp: cmax - cmin,
    };
    BakeResult { height, carve_delta, stats }
}

/// A baked region sliced from a super-region: the conditioned height grid (n*n, row-major) + its
/// world origin/span. The grid is the conditioned height (tanh units); callers scale by height_scale
/// when writing page textures (matching the RegionFactRuntime convention).
pub struct RegionSlice {
    pub grid: Vec<f64>,      // n*n conditioned height (tanh units)
    pub grid_n: usize,       // = n
    pub origin_x_m: f64,
    pub origin_z_m: f64,
    pub span_m: f64,
}

/// Bake a k*k super-region as ONE field (carve + condition over the whole super-field), then SLICE
/// into k*k region grids. Internal borders are seam-exact BY CONSTRUCTION (the carve's global path
/// is computed once over the super-field, not per-region). `super_n` MUST equal k*(n-1)+1.
/// `span_m` is ONE region's span; the super-field spans k*span_m. `super_x0/z0` is the super-region
/// world origin. Returns k*k RegionSlice in row-major (gj*k + gi) order, gi/gj the region's column/row.
#[allow(clippy::too_many_arguments)]
pub fn bake_super_region(
    super_raw: &[f64],
    super_n: usize,
    n: usize,
    k: usize,
    span_m: f64,
    height_scale_m: f64,
    super_x0_m: f64,
    super_z0_m: f64,
    pass: &PassNetworkParams,
    traverse: &TraverseParams,
    ramp: &RampParams,
    provider: &dyn PercentileProvider,
) -> Vec<RegionSlice> {
    assert_eq!(super_n, k*(n-1) + 1, "bake_super_region: super_n must be k*(n-1)+1");
    assert_eq!(super_raw.len(), super_n*super_n, "bake_super_region: super_raw size");
    let super_span_m = span_m * k as f64;
    // Carve + condition over the WHOLE super-field (seam-exact internally).
    let baked = bake_region_from_raw_with_provider(
        super_raw, super_n, super_span_m, height_scale_m, super_x0_m, super_z0_m,
        pass, traverse, ramp, provider);
    // Slice into k*k region grids (texel-corner: region (gi,gj) takes cells
    // [gi*(n-1) .. gi*(n-1)+n) x [gj*(n-1) .. gj*(n-1)+n), overlapping by 1 at shared edges).
    let mut out = Vec::with_capacity(k*k);
    for gj in 0..k {
        for gi in 0..k {
            let c0 = gi*(n-1);
            let r0 = gj*(n-1);
            let mut grid = vec![0.0f64; n*n];
            for r in 0..n {
                for c in 0..n {
                    grid[r*n + c] = baked.height[(r0 + r)*super_n + (c0 + c)];
                }
            }
            out.push(RegionSlice {
                grid, grid_n: n,
                origin_x_m: super_x0_m + gi as f64 * span_m,
                origin_z_m: super_z0_m + gj as f64 * span_m,
                span_m,
            });
        }
    }
    out
}
