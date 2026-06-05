//! RegionFactRuntime: a baked, carved+conditioned region tile that pages sample. Near-copy of
//! StaticHeightRuntime's grid+bilinear, but the grid comes from a region bake (not JSON) and the
//! region tiles the plane (no outside-height / edge-fade — every page in the region is inside it).
#![allow(dead_code)]

use godot::classes::RenderingDevice;
use godot::prelude::*;
use crate::biome_page_compute::f32s_to_bytes;

#[derive(Clone)]
pub(in crate::page_pool) struct RegionFactRuntime {
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
