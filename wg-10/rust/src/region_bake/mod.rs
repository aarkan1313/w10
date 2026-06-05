//! region_bake: the carve -> condition TAIL of the baked-look pipeline, fed a RAW field that
//! comes from the GPU region-macro readback (the live path) instead of the CPU macro. The CPU
//! macro entry (`bake_region::bake_region`) delegates here so the existing end-to-end parity
//! gate still covers the tail.
#![allow(dead_code)]
use crate::condition_world::{condition_world, condition_world_with_percentiles, ConditionStats};
use crate::pass_network::{carve_ramp_delta, carve_routes, PassNetworkParams, RampParams, TraverseParams};

mod gpu_macro;
pub use gpu_macro::gpu_macro_region;

mod percentile_provider;
pub use percentile_provider::{PercentileFields, PercentileProvider, ScalarRegionPercentiles};

#[cfg(test)]
mod region_bake_tests;
#[cfg(test)]
mod seam_tests;

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
