//! PercentileProvider: the swappable source of conditioning percentiles (engine modularity).
//! condition_world is a pure transform; THIS decides how p05/p50/p95 are derived, which IS the
//! cross-region SEAM strategy. Two impls: ScalarRegionPercentiles (per-region — today's look /
//! single-region / tests; NOT seam-exact across regions) and SmoothFieldPercentiles (seam-exact
//! engine default, added in the next task).
#![allow(dead_code)]
use crate::condition_world::percentile_linear;

/// Per-cell percentile fields for a region grid. Each is length 1 (scalar broadcast) or n*n.
pub struct PercentileFields {
    pub p05: Vec<f64>,
    pub p50: Vec<f64>,
    pub p95: Vec<f64>,
}

/// The cross-region seam strategy, swappable (engine consumers can supply their own). `z` is the
/// region's carved RAW field (length n*n); the world coords + span let a smooth provider sample a
/// position-continuous percentile field that agrees across region borders.
pub trait PercentileProvider {
    fn percentiles(&self, z: &[f64], region_x0_m: f64, region_z0_m: f64, span_m: f64, n: usize)
        -> PercentileFields;
}

/// Today's behavior: percentiles over the region's OWN field (one triple, length-1 broadcast).
/// Preserves the accepted single-region look + the existing bit-exact gates. NOT seam-exact across
/// regions (this is exactly the measured ~1090 m seam) — use for single-region bakes / tests only.
pub struct ScalarRegionPercentiles;

impl PercentileProvider for ScalarRegionPercentiles {
    fn percentiles(&self, z: &[f64], _x0: f64, _z0: f64, _span: f64, _n: usize) -> PercentileFields {
        let mut sorted = z.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).expect("ScalarRegionPercentiles: NaN in field"));
        PercentileFields {
            p05: vec![percentile_linear(&sorted, 5.0)],
            p50: vec![percentile_linear(&sorted, 50.0)],
            p95: vec![percentile_linear(&sorted, 95.0)],
        }
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    #[test]
    fn scalar_provider_matches_self_percentiles() {
        // ScalarRegionPercentiles over a field yields that field's own p05/p50/p95 as length-1 fields,
        // and conditioning through them equals today's condition_world bit-for-bit.
        let n = 8usize;
        let z: Vec<f64> = (0..n*n).map(|i| ((i*131%97) as f64)*0.37 - 12.0).collect();
        let prov = ScalarRegionPercentiles;
        let f = prov.percentiles(&z, 0.0, 0.0, 1000.0, n);
        assert_eq!(f.p05.len(), 1);
        assert_eq!(f.p50.len(), 1);
        assert_eq!(f.p95.len(), 1);
        let (want, _s) = crate::condition_world::condition_world(&z, n);
        let got = crate::condition_world::condition_world_with_percentile_fields(&z, n, &f.p05, &f.p50, &f.p95);
        assert_eq!(got.len(), want.len());
        for i in 0..want.len() { assert_eq!(got[i].to_bits(), want[i].to_bits(), "cell {i}"); }
    }
}
