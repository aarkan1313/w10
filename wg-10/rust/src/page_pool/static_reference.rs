//! Static accepted-reference page producer for `Wg10PagePool`.
//!
//! This is deliberately a reference bridge, not a new runtime synthesis path:
//! it streams the owner-accepted generated mountain-network payload through the
//! same page pool and clipmap renderer so renderer behavior can be reviewed
//! independently from the live biome recipe.

use godot::classes::RenderingDevice;
use godot::prelude::*;
use serde::Deserialize;
use std::path::Path;

use crate::biome_page_compute::f32s_to_bytes;

mod payload;

use self::payload::StaticPayload;

#[derive(Clone)]
pub(super) struct StaticHeightRuntime {
    grid: Vec<f32>,
    corridor_grid: Option<Vec<u8>>,
    material_hints: Option<StaticMaterialHintGrids>,
    grid_n: usize,
    origin_x_m: f64,
    origin_z_m: f64,
    span_x_m: f64,
    span_z_m: f64,
    pub(super) generator_version: String,
    pub(super) source_scope: String,
    pub(super) height_scale_m: f64,
    pub(super) feature_span_m: f64,
    pub(super) has_corridor: bool,
    pub(super) corridor_frac: f64,
    pub(super) has_material_hints: bool,
    pub(super) material_hint_fracs: StaticMaterialHintFractions,
    pub(super) pass_network_routes: i64,
    pub(super) pass_network_walkable_frac: f64,
    pub(super) pass_network_carved_frac: f64,
    pub(super) has_conditioning_stats: bool,
    pub(super) conditioning_stats: StaticConditioningStats,
}

#[derive(Clone, Copy, Default)]
pub(super) struct StaticMaterialHintFractions {
    pub(super) low_pass: f64,
    pub(super) floor: f64,
    pub(super) rock: f64,
    pub(super) snow: f64,
}

#[derive(Clone, Copy, Default, Deserialize)]
pub(super) struct StaticConditioningStats {
    pub(super) source_min: f64,
    pub(super) source_max: f64,
    pub(super) source_ptp: f64,
    pub(super) p05: f64,
    pub(super) p50: f64,
    pub(super) p95: f64,
    pub(super) conditioned_min: f64,
    pub(super) conditioned_max: f64,
    pub(super) conditioned_ptp: f64,
}

#[derive(Clone)]
struct StaticMaterialHintGrids {
    low_pass: Vec<f32>,
    floor: Vec<f32>,
    rock: Vec<f32>,
    snow: Vec<f32>,
}

impl StaticHeightRuntime {
    pub(super) fn from_json_path(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("static reference: cannot read {path:?}: {e}"))?;
        let payload: StaticPayload = serde_json::from_str(&text)
            .map_err(|e| format!("static reference: invalid json {path:?}: {e}"))?;
        Self::from_payload(payload)
    }

    pub(super) fn write_page_texture(
        &self,
        rd: &mut Gd<RenderingDevice>,
        target_rid: Rid,
        page_origin_x: f64,
        page_origin_z: f64,
        world_span: f64,
        page_px: i64,
    ) -> Result<(), String> {
        if page_px < 2 {
            return Err(format!("static reference: page_px {page_px} must be >= 2"));
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
            return Err(format!("static reference: texture_update failed: {err:?}"));
        }
        Ok(())
    }

    pub(super) fn has_presentation_materials(&self) -> bool {
        self.has_corridor || self.has_material_hints
    }

    pub(super) fn write_material_page_texture(
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

    fn sample(&self, x_m: f64, z_m: f64) -> f32 {
        let (x0, z0, x1, z1, tx, tz) = self.sample_indices(x_m, z_m);
        let h00 = self.grid[z0 * self.grid_n + x0];
        let h10 = self.grid[z0 * self.grid_n + x1];
        let h01 = self.grid[z1 * self.grid_n + x0];
        let h11 = self.grid[z1 * self.grid_n + x1];
        let hx0 = h00 + (h10 - h00) * tx;
        let hx1 = h01 + (h11 - h01) * tx;
        hx0 + (hx1 - hx0) * tz
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

    fn sample_corridor(&self, x_m: f64, z_m: f64) -> bool {
        let Some(corridor) = self.corridor_grid.as_ref() else {
            return false;
        };
        let (x0, z0, x1, z1, tx, tz) = self.sample_indices(x_m, z_m);
        let ix = if tx >= 0.5 { x1 } else { x0 };
        let iz = if tz >= 0.5 { z1 } else { z0 };
        corridor[iz * self.grid_n + ix] != 0
    }

    fn sample_material_hints(&self, x_m: f64, z_m: f64) -> Option<StaticMaterialHintFractions> {
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

    pub(super) fn corridor_fraction_for_page(
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

    pub(super) fn material_hint_fractions_for_page(
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
