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
mod presentation;
mod sampling;

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

}
