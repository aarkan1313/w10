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

/// Seam-exact smooth percentile field. p05/p50/p95 vary smoothly with WORLD POSITION (not stepped
/// per-region), so abutting regions agree at their shared border BY CONSTRUCTION while staying
/// locally adaptive. The percentile field is computed from a coarse macro SAMPLER over world
/// position: coarse lattice (absolute-world-keyed) -> world-window percentiles -> bilinear upsample.
pub struct SmoothFieldPercentiles<F: Fn(f64, f64) -> f64> {
    pub macro_sampler: F,     // f(world_x_m, world_z_m) -> macro value (the SAME for all regions)
    pub coarse_stride_m: f64, // world spacing between coarse lattice nodes
    pub window_radius_m: f64, // half-size (metres) of the world window the percentiles reduce over
    pub window_samples: usize, // sub-samples per axis inside each window (e.g. 9)
}

impl<F: Fn(f64, f64) -> f64> PercentileProvider for SmoothFieldPercentiles<F> {
    fn percentiles(&self, _z: &[f64], region_x0_m: f64, region_z0_m: f64, span_m: f64, n: usize)
        -> PercentileFields
    {
        let stride = self.coarse_stride_m;
        let rad = self.window_radius_m;
        let ws = self.window_samples.max(1);

        // --- 1) Coarse lattice keyed to ABSOLUTE world coords via GLOBAL INTEGER node indices. ---
        // Every node lives at world position `gi * stride` for an integer global index `gi`. Two
        // abutting regions that reference the same `gi` get a BIT-IDENTICAL node position (and hence
        // an identical window and percentile) — the seam guarantee, immune to per-region float
        // accumulation. Columns cover [region_x0 - rad, region_x0 + span + rad]; rows likewise in Z.
        let gi_lo = |lo: f64| -> i64 { (lo / stride).floor() as i64 };
        let gi_hi = |hi: f64| -> i64 { (hi / stride).ceil() as i64 };
        let gx0 = gi_lo(region_x0_m - rad);
        let gx1 = gi_hi(region_x0_m + span_m + rad);
        let gz0 = gi_lo(region_z0_m - rad);
        let gz1 = gi_hi(region_z0_m + span_m + rad);
        // Global-index -> world position (pure function of the integer index => seam-exact).
        let xs: Vec<f64> = (gx0..=gx1).map(|g| g as f64 * stride).collect();
        let zs: Vec<f64> = (gz0..=gz1).map(|g| g as f64 * stride).collect();
        let (ncx, ncz) = (xs.len(), zs.len());

        // --- 2) Per-node world-window percentiles. ---
        // Window of ws*ws macro sub-samples spanning [node - rad, node + rad] on each axis.
        // Everything keyed to the node's ABSOLUTE world position -> identical across regions.
        let mut node_p05 = vec![0.0f64; ncx * ncz];
        let mut node_p50 = vec![0.0f64; ncx * ncz];
        let mut node_p95 = vec![0.0f64; ncx * ncz];
        let sub_step = if ws > 1 { (2.0 * rad) / (ws as f64 - 1.0) } else { 0.0 };
        let mut win: Vec<f64> = Vec::with_capacity(ws * ws);
        for (jz, &nz) in zs.iter().enumerate() {
            for (jx, &nx) in xs.iter().enumerate() {
                win.clear();
                for sz in 0..ws {
                    let wz = nz - rad + sub_step * sz as f64;
                    for sx in 0..ws {
                        let wx = nx - rad + sub_step * sx as f64;
                        win.push((self.macro_sampler)(wx, wz));
                    }
                }
                win.sort_by(|a, b| a.partial_cmp(b).expect("SmoothFieldPercentiles: NaN in window"));
                let idx = jz * ncx + jx;
                node_p05[idx] = percentile_linear(&win, 5.0);
                node_p50[idx] = percentile_linear(&win, 50.0);
                node_p95[idx] = percentile_linear(&win, 95.0);
            }
        }

        // --- 3) Bilinear upsample the coarse node grids to the region's n*n cells. ---
        let cell_m = if n > 1 { span_m / (n as f64 - 1.0) } else { 0.0 };
        // Bilinear over the global-index node grid at world position (wx, wz). The interpolation
        // fraction is computed from the ABSOLUTE coordinate `wx/stride` and the GLOBAL index, NOT a
        // region-local x0 subtraction — so two regions sampling the SAME world point get the SAME
        // (gi, frac) bit-for-bit. `gx0`/`gz0` map a global index to a local array slot.
        let bilerp = |grid: &[f64], wx: f64, wz: f64| -> f64 {
            let gfx = (wx / stride).clamp(gx0 as f64, gx1 as f64);
            let gfz = (wz / stride).clamp(gz0 as f64, gz1 as f64);
            let gix = gfx.floor();
            let giz = gfz.floor();
            let tx = gfx - gix; // fraction is a pure function of wx (no region x0) => seam-exact
            let tz = gfz - giz;
            let ix = ((gix as i64 - gx0) as usize).min(ncx - 1);
            let iz = ((giz as i64 - gz0) as usize).min(ncz - 1);
            let ix1 = (ix + 1).min(ncx - 1);
            let iz1 = (iz + 1).min(ncz - 1);
            let v00 = grid[iz * ncx + ix];
            let v10 = grid[iz * ncx + ix1];
            let v01 = grid[iz1 * ncx + ix];
            let v11 = grid[iz1 * ncx + ix1];
            let a = v00 + (v10 - v00) * tx;
            let b = v01 + (v11 - v01) * tx;
            a + (b - a) * tz
        };

        let nn = n * n;
        let mut p05 = vec![0.0f64; nn];
        let mut p50 = vec![0.0f64; nn];
        let mut p95 = vec![0.0f64; nn];
        for r in 0..n {
            let wz = region_z0_m + r as f64 * cell_m;
            for c in 0..n {
                let wx = region_x0_m + c as f64 * cell_m;
                let i = r * n + c;
                p05[i] = bilerp(&node_p05, wx, wz);
                p50[i] = bilerp(&node_p50, wx, wz);
                p95[i] = bilerp(&node_p95, wx, wz);
            }
        }
        PercentileFields { p05, p50, p95 }
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
