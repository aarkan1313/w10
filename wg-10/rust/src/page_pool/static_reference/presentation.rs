//! Presentation material-code texture for the accepted static reference.
//!
//! The accepted payload keeps height, corridor, and material hints separate.
//! This module owns the temporary renderer-facing R32F material-code projection.

use godot::classes::RenderingDevice;
use godot::prelude::*;

use crate::biome_page_compute::f32s_to_bytes;

use super::StaticHeightRuntime;

impl StaticHeightRuntime {
    pub(in crate::page_pool) fn has_presentation_materials(&self) -> bool {
        self.has_corridor || self.has_material_hints
    }

    pub(in crate::page_pool) fn write_material_page_texture(
        &self,
        rd: &mut Gd<RenderingDevice>,
        target_rid: Rid,
        page_origin_x: f64,
        page_origin_z: f64,
        world_span: f64,
        page_px: i64,
    ) -> Result<(), String> {
        if page_px < 2 {
            return Err(format!(
                "static reference: material page_px {page_px} must be >= 2"
            ));
        }
        let page_px = page_px as usize;
        let mut samples = vec![0.0_f32; page_px * page_px];
        let denom = (page_px - 1) as f64;
        for z in 0..page_px {
            let wz = page_origin_z + world_span * z as f64 / denom;
            for x in 0..page_px {
                let wx = page_origin_x + world_span * x as f64 / denom;
                samples[z * page_px + x] = self.sample_material_code(wx, wz);
            }
        }
        let bytes = f32s_to_bytes(&samples);
        let pba = PackedByteArray::from(bytes.as_slice());
        let err = rd.texture_update(target_rid, 0, &pba);
        if err != godot::global::Error::OK {
            return Err(format!(
                "static reference: material texture_update failed: {err:?}"
            ));
        }
        Ok(())
    }

    fn sample_material_code(&self, x_m: f64, z_m: f64) -> f32 {
        if !self.contains_reference_point(x_m, z_m) {
            return 0.0;
        }
        let (x0, z0, x1, z1, tx, tz) = self.sample_indices(x_m, z_m);
        if let Some(corridor) = self.corridor_grid.as_ref() {
            let ix = if tx >= 0.5 { x1 } else { x0 };
            let iz = if tz >= 0.5 { z1 } else { z0 };
            if corridor[iz * self.grid_n + ix] != 0 {
                return 1.0;
            }
        }

        let Some(hints) = self.material_hints.as_ref() else {
            return 0.0;
        };
        let sample = |grid: &Vec<f32>| -> f64 {
            let h00 = grid[z0 * self.grid_n + x0];
            let h10 = grid[z0 * self.grid_n + x1];
            let h01 = grid[z1 * self.grid_n + x0];
            let h11 = grid[z1 * self.grid_n + x1];
            let hx0 = h00 + (h10 - h00) * tx;
            let hx1 = h01 + (h11 - h01) * tx;
            (hx0 + (hx1 - hx0) * tz) as f64
        };
        let low_pass = sample(&hints.low_pass);
        let floor = sample(&hints.floor);
        let rock = sample(&hints.rock);
        let snow = sample(&hints.snow);

        let floorish = floor.max(low_pass);
        if snow >= rock && snow >= floorish && snow > 0.08 {
            3.0
        } else if rock >= floorish && rock > 0.08 {
            2.0
        } else if floorish > 0.08 {
            return 1.0;
        } else {
            0.0
        }
    }
}
