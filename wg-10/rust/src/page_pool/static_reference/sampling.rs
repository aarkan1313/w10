//! Sampling helpers for the accepted static-reference runtime.

use super::{StaticHeightRuntime, StaticMaterialHintFractions};

impl StaticHeightRuntime {
    pub(super) fn sample(&self, x_m: f64, z_m: f64) -> f32 {
        let (x0, z0, x1, z1, tx, tz) = self.sample_indices(x_m, z_m);
        let h00 = self.grid[z0 * self.grid_n + x0];
        let h10 = self.grid[z0 * self.grid_n + x1];
        let h01 = self.grid[z1 * self.grid_n + x0];
        let h11 = self.grid[z1 * self.grid_n + x1];
        let hx0 = h00 + (h10 - h00) * tx;
        let hx1 = h01 + (h11 - h01) * tx;
        hx0 + (hx1 - hx0) * tz
    }

    pub(super) fn sample_corridor(&self, x_m: f64, z_m: f64) -> bool {
        let Some(corridor) = self.corridor_grid.as_ref() else {
            return false;
        };
        let (x0, z0, x1, z1, tx, tz) = self.sample_indices(x_m, z_m);
        let ix = if tx >= 0.5 { x1 } else { x0 };
        let iz = if tz >= 0.5 { z1 } else { z0 };
        corridor[iz * self.grid_n + ix] != 0
    }

    pub(super) fn sample_material_hints(
        &self,
        x_m: f64,
        z_m: f64,
    ) -> Option<StaticMaterialHintFractions> {
        let hints = self.material_hints.as_ref()?;
        let (x0, z0, x1, z1, tx, tz) = self.sample_indices(x_m, z_m);
        let sample = |grid: &Vec<f32>| -> f64 {
            let h00 = grid[z0 * self.grid_n + x0];
            let h10 = grid[z0 * self.grid_n + x1];
            let h01 = grid[z1 * self.grid_n + x0];
            let h11 = grid[z1 * self.grid_n + x1];
            let hx0 = h00 + (h10 - h00) * tx;
            let hx1 = h01 + (h11 - h01) * tx;
            (hx0 + (hx1 - hx0) * tz) as f64
        };
        Some(StaticMaterialHintFractions {
            low_pass: sample(&hints.low_pass),
            floor: sample(&hints.floor),
            rock: sample(&hints.rock),
            snow: sample(&hints.snow),
        })
    }

    pub(in crate::page_pool) fn corridor_fraction_for_page(
        &self,
        page_origin_x: f64,
        page_origin_z: f64,
        world_span: f64,
        samples_px: usize,
    ) -> f64 {
        let Some(corridor) = self.corridor_grid.as_ref() else {
            return 0.0;
        };
        let samples_px = samples_px.clamp(2, 65);
        let denom = (samples_px - 1) as f64;
        let mut count = 0usize;
        for z in 0..samples_px {
            let wz = page_origin_z + world_span * z as f64 / denom;
            for x in 0..samples_px {
                let wx = page_origin_x + world_span * x as f64 / denom;
                let (x0, z0, x1, z1, tx, tz) = self.sample_indices(wx, wz);
                let ix = if tx >= 0.5 { x1 } else { x0 };
                let iz = if tz >= 0.5 { z1 } else { z0 };
                if corridor[iz * self.grid_n + ix] != 0 {
                    count += 1;
                }
            }
        }
        count as f64 / (samples_px * samples_px) as f64
    }

    pub(in crate::page_pool) fn material_hint_fractions_for_page(
        &self,
        page_origin_x: f64,
        page_origin_z: f64,
        world_span: f64,
        samples_px: usize,
    ) -> Option<StaticMaterialHintFractions> {
        let hints = self.material_hints.as_ref()?;
        let samples_px = samples_px.clamp(2, 65);
        let denom = (samples_px - 1) as f64;
        let mut sums = StaticMaterialHintFractions::default();
        for z in 0..samples_px {
            let wz = page_origin_z + world_span * z as f64 / denom;
            for x in 0..samples_px {
                let wx = page_origin_x + world_span * x as f64 / denom;
                let (x0, z0, x1, z1, tx, tz) = self.sample_indices(wx, wz);
                let ix = if tx >= 0.5 { x1 } else { x0 };
                let iz = if tz >= 0.5 { z1 } else { z0 };
                let idx = iz * self.grid_n + ix;
                sums.low_pass += hints.low_pass[idx] as f64;
                sums.floor += hints.floor[idx] as f64;
                sums.rock += hints.rock[idx] as f64;
                sums.snow += hints.snow[idx] as f64;
            }
        }
        let total = (samples_px * samples_px) as f64;
        Some(StaticMaterialHintFractions {
            low_pass: sums.low_pass / total,
            floor: sums.floor / total,
            rock: sums.rock / total,
            snow: sums.snow / total,
        })
    }

    fn sample_indices(&self, x_m: f64, z_m: f64) -> (usize, usize, usize, usize, f32, f32) {
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
        (x0, z0, x1, z1, tx, tz)
    }
}
