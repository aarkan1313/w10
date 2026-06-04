//! Presentation material-fact texture for the accepted static reference.
//!
//! The accepted payload keeps height, corridor, and material hints separate.
//! This module owns the renderer-facing RGBA32F projection of those facts:
//! R=low-pass/corridor, G=floor, B=rock, A=snow.

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
        let samples =
            self.material_page_samples(page_origin_x, page_origin_z, world_span, page_px)?;
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

    fn material_page_samples(
        &self,
        page_origin_x: f64,
        page_origin_z: f64,
        world_span: f64,
        page_px: usize,
    ) -> Result<Vec<f32>, String> {
        if page_px < 2 {
            return Err(format!(
                "static reference: material page_px {page_px} must be >= 2"
            ));
        }
        let mut samples = vec![0.0_f32; page_px * page_px * 4];
        let denom = (page_px - 1) as f64;
        for z in 0..page_px {
            let wz = page_origin_z + world_span * z as f64 / denom;
            for x in 0..page_px {
                let wx = page_origin_x + world_span * x as f64 / denom;
                let channels = self.sample_material_channels(wx, wz);
                let idx = (z * page_px + x) * 4;
                samples[idx] = channels[0];
                samples[idx + 1] = channels[1];
                samples[idx + 2] = channels[2];
                samples[idx + 3] = channels[3];
            }
        }
        Ok(samples)
    }

    fn sample_material_channels(&self, x_m: f64, z_m: f64) -> [f32; 4] {
        if !self.contains_reference_point(x_m, z_m) {
            return [0.0; 4];
        }
        let (x0, z0, x1, z1, tx, tz) = self.sample_indices(x_m, z_m);
        let mut low_pass_or_corridor = 0.0_f32;
        if let Some(corridor) = self.corridor_grid.as_ref() {
            let ix = if tx >= 0.5 { x1 } else { x0 };
            let iz = if tz >= 0.5 { z1 } else { z0 };
            if corridor[iz * self.grid_n + ix] != 0 {
                low_pass_or_corridor = 1.0;
            }
        }

        let Some(hints) = self.material_hints.as_ref() else {
            return [low_pass_or_corridor, 0.0, 0.0, 0.0];
        };
        let sample = |grid: &Vec<f32>| -> f32 {
            let h00 = grid[z0 * self.grid_n + x0];
            let h10 = grid[z0 * self.grid_n + x1];
            let h01 = grid[z1 * self.grid_n + x0];
            let h11 = grid[z1 * self.grid_n + x1];
            let hx0 = h00 + (h10 - h00) * tx;
            let hx1 = h01 + (h11 - h01) * tx;
            hx0 + (hx1 - hx0) * tz
        };
        [
            low_pass_or_corridor.max(sample(&hints.low_pass)),
            sample(&hints.floor),
            sample(&hints.rock),
            sample(&hints.snow),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        StaticConditioningStats, StaticMaterialHintFractions, StaticMaterialHintGrids,
    };
    use super::*;

    fn runtime_with_material_facts() -> StaticHeightRuntime {
        StaticHeightRuntime {
            grid: vec![0.0, 1.0, 2.0, 3.0],
            corridor_grid: Some(vec![1, 0, 1, 0]),
            material_hints: Some(StaticMaterialHintGrids {
                low_pass: vec![1.0, 0.0, 1.0, 0.0],
                floor: vec![1.0, 0.25, 0.75, 0.0],
                rock: vec![0.0, 0.5, 0.75, 1.0],
                snow: vec![0.0, 0.0, 1.0, 1.0],
            }),
            grid_n: 2,
            origin_x_m: -5.0,
            origin_z_m: -5.0,
            span_x_m: 10.0,
            span_z_m: 10.0,
            outside_height_m: 0.0,
            edge_fade_m: 8192.0,
            generator_version: "test".into(),
            source_scope: "test".into(),
            height_scale_m: 100.0,
            feature_span_m: 90_000.0,
            source_origin_x_m: -5.0,
            source_origin_z_m: -5.0,
            source_span_x_m: 10.0,
            source_span_z_m: 10.0,
            source_scene_ratio: 1.0,
            has_corridor: true,
            corridor_frac: 0.5,
            has_material_hints: true,
            material_hint_fracs: StaticMaterialHintFractions::default(),
            pass_network_routes: 1,
            pass_network_walkable_frac: 0.5,
            pass_network_carved_frac: 0.25,
            has_conditioning_stats: true,
            conditioning_stats: StaticConditioningStats::default(),
        }
    }

    #[test]
    fn material_page_preserves_four_fact_channels() {
        let rt = runtime_with_material_facts();
        let samples = rt
            .material_page_samples(-5.0, -5.0, 10.0, 2)
            .expect("material page should sample");

        assert_eq!(samples.len(), 16);
        assert_eq!(&samples[0..4], &[1.0, 1.0, 0.0, 0.0]);
        assert_eq!(&samples[12..16], &[0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn material_page_does_not_smear_facts_outside_reference_domain() {
        let rt = runtime_with_material_facts();
        let samples = rt
            .material_page_samples(45.0, 45.0, 10.0, 2)
            .expect("outside material page should sample");

        assert!(samples.iter().all(|v| *v == 0.0));
    }
}
