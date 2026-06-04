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
        if self.sample_corridor(x_m, z_m) {
            return 1.0;
        }
        let Some(hints) = self.sample_material_hints(x_m, z_m) else {
            return 0.0;
        };
        let floorish = hints.floor.max(hints.low_pass);
        if hints.snow >= hints.rock && hints.snow >= floorish && hints.snow > 0.08 {
            3.0
        } else if hints.rock >= floorish && hints.rock > 0.08 {
            2.0
        } else if floorish > 0.08 {
            1.0
        } else {
            0.0
        }
    }
}
