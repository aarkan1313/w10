use serde::Deserialize;

use super::{
    height_percentile, material_hint_fractions, validate_conditioning_stats,
    validate_source_display_mapping, StaticPassNetwork,
};
use crate::page_pool::static_reference::{
    StaticConditioningStats, StaticHeightRuntime, StaticMaterialHintGrids,
};

#[derive(Deserialize)]
pub(in crate::page_pool::static_reference) struct StaticRuntimeTilePayload {
    pub(super) generator_version: String,
    pub(super) source_scope: String,
    pub(super) chunk_count: usize,
    pub(super) chunk_n: usize,
    pub(super) field_n: usize,
    pub(super) world_span_m: f64,
    pub(super) height_scale_m: f64,
    pub(super) feature_span_m: f64,
    pub(super) tiles: Vec<StaticRuntimeTile>,
}

#[derive(Deserialize)]
pub(super) struct StaticRuntimeTile {
    pub(super) generator_version: String,
    pub(super) source_scope: String,
    pub(super) chunk_count: usize,
    pub(super) chunk_n: usize,
    pub(super) field_n: usize,
    pub(super) field_origin_x_m: f64,
    pub(super) field_origin_z_m: f64,
    pub(super) field_span_m: f64,
    pub(super) source_origin_x_m: f64,
    pub(super) source_origin_z_m: f64,
    pub(super) source_span_m: f64,
    pub(super) source_scene_ratio: f64,
    pub(super) height_scale_m: f64,
    pub(super) pass_network: Option<StaticPassNetwork>,
    pub(super) stats: Option<StaticConditioningStats>,
    pub(super) fields: StaticRuntimeTileFields,
}

#[derive(Deserialize)]
pub(super) struct StaticRuntimeTileFields {
    pub(super) height: Vec<f32>,
    pub(super) corridor: Option<Vec<i64>>,
    pub(super) low_pass_hint: Option<Vec<f32>>,
    pub(super) floor_hint: Option<Vec<f32>>,
    pub(super) rock_hint: Option<Vec<f32>>,
    pub(super) snow_hint: Option<Vec<f32>>,
}

impl StaticHeightRuntime {
    pub(super) fn from_runtime_tile_payload(
        payload: StaticRuntimeTilePayload,
    ) -> Result<Self, String> {
        if payload.chunk_count == 0 {
            return Err("static reference runtime tile: chunk_count must be > 0".into());
        }
        if payload.chunk_n < 2 {
            return Err("static reference runtime tile: chunk_n must be >= 2".into());
        }
        if payload.field_n < 2 {
            return Err("static reference runtime tile: field_n must be >= 2".into());
        }
        let expected_field_n = payload
            .chunk_count
            .checked_mul(payload.chunk_n - 1)
            .and_then(|v| v.checked_add(1))
            .ok_or("static reference runtime tile: field_n overflow")?;
        if payload.field_n != expected_field_n {
            return Err(format!(
                "static reference runtime tile: field_n={} expected {} from chunk_count/chunk_n",
                payload.field_n, expected_field_n
            ));
        }
        if payload.source_scope.trim().is_empty() {
            return Err("static reference runtime tile: source_scope must be non-empty".into());
        }
        if payload.generator_version.trim().is_empty() {
            return Err(
                "static reference runtime tile: generator_version must be non-empty".into(),
            );
        }
        if payload.world_span_m <= 0.0 || !payload.world_span_m.is_finite() {
            return Err(
                "static reference runtime tile: world_span_m must be finite and > 0".into(),
            );
        }
        if payload.height_scale_m <= 0.0 || !payload.height_scale_m.is_finite() {
            return Err(
                "static reference runtime tile: height_scale_m must be finite and > 0".into(),
            );
        }
        let tile = payload
            .tiles
            .first()
            .ok_or("static reference runtime tile: payload has no tiles")?;
        // StaticHeightRuntime is one reference field. Multi-style runtime payloads
        // mirror review payload seeds, so this bridge selects the first style.
        if tile.source_scope.trim().is_empty() {
            return Err(
                "static reference runtime tile: tile source_scope must be non-empty".into(),
            );
        }
        if tile.generator_version.trim().is_empty() {
            return Err(
                "static reference runtime tile: tile generator_version must be non-empty".into(),
            );
        }
        if tile.chunk_count != payload.chunk_count {
            return Err(format!(
                "static reference runtime tile: tile chunk_count={} expected {}",
                tile.chunk_count, payload.chunk_count
            ));
        }
        if tile.chunk_n != payload.chunk_n {
            return Err(format!(
                "static reference runtime tile: tile chunk_n={} expected {}",
                tile.chunk_n, payload.chunk_n
            ));
        }
        if tile.field_n != payload.field_n {
            return Err(format!(
                "static reference runtime tile: tile field_n={} expected {}",
                tile.field_n, payload.field_n
            ));
        }
        if (tile.field_span_m - payload.world_span_m).abs() > 0.01 {
            return Err(format!(
                "static reference runtime tile: tile span {:.3} expected {:.3}",
                tile.field_span_m, payload.world_span_m
            ));
        }
        if (tile.height_scale_m - payload.height_scale_m).abs() > 1.0e-6 {
            return Err(format!(
                "static reference runtime tile: tile height_scale_m={} expected {}",
                tile.height_scale_m, payload.height_scale_m
            ));
        }
        validate_source_display_mapping(
            "static reference runtime tile",
            tile.source_origin_x_m,
            tile.source_origin_z_m,
            tile.source_span_m,
            tile.source_span_m,
            tile.source_scene_ratio,
        )?;

        let grid_n = payload.field_n;
        let grid_len = grid_n
            .checked_mul(grid_n)
            .ok_or("static reference runtime tile: grid size overflow")?;
        if tile.fields.height.len() != grid_len {
            return Err(format!(
                "static reference runtime tile: height={} expected {}",
                tile.fields.height.len(),
                grid_len
            ));
        }
        if tile.fields.height.iter().any(|v| !v.is_finite()) {
            return Err("static reference runtime tile: height values must be finite".into());
        }
        let grid = tile
            .fields
            .height
            .iter()
            .map(|h| *h * payload.height_scale_m as f32)
            .collect::<Vec<_>>();

        let corridor_grid = match tile.fields.corridor.as_ref() {
            Some(values) => {
                if values.len() != grid_len {
                    return Err(format!(
                        "static reference runtime tile: corridor={} expected {}",
                        values.len(),
                        grid_len
                    ));
                }
                Some(
                    values
                        .iter()
                        .map(|v| if *v != 0 { 1_u8 } else { 0_u8 })
                        .collect::<Vec<_>>(),
                )
            }
            None => None,
        };

        let all_hints = tile.fields.has_all_material_hints();
        let any_hints = tile.fields.has_any_material_hint();
        if any_hints && !all_hints {
            return Err("static reference runtime tile: material hints must be present as a complete set or none".into());
        }
        let material_hints = if all_hints {
            tile.fields.validate_material_hint_lengths(grid_len)?;
            Some(StaticMaterialHintGrids {
                low_pass: tile.fields.low_pass_hint.as_ref().unwrap().clone(),
                floor: tile.fields.floor_hint.as_ref().unwrap().clone(),
                rock: tile.fields.rock_hint.as_ref().unwrap().clone(),
                snow: tile.fields.snow_hint.as_ref().unwrap().clone(),
            })
        } else {
            None
        };

        let corridor_frac = corridor_grid
            .as_ref()
            .map(|corridor| {
                corridor.iter().filter(|v| **v != 0).count() as f64 / corridor.len() as f64
            })
            .unwrap_or(0.0);
        let pass_network = tile.pass_network.as_ref();
        let pass_network_routes = pass_network.map(|p| p.routes).unwrap_or(0);
        let pass_network_walkable_frac = pass_network.map(|p| p.band_walkable_frac).unwrap_or(0.0);
        let pass_network_carved_frac = pass_network.map(|p| p.carved_frac).unwrap_or(0.0);
        let (has_conditioning_stats, conditioning_stats) = match tile.stats {
            Some(stats) => {
                validate_conditioning_stats(stats)?;
                (true, stats)
            }
            None => (false, StaticConditioningStats::default()),
        };
        let material_hint_fracs = material_hints
            .as_ref()
            .map(material_hint_fractions)
            .unwrap_or_default();
        let has_corridor = corridor_grid.is_some();
        let has_material_hints = material_hints.is_some();
        let outside_height_m = height_percentile(&grid, 0.05);
        let edge_fade_m = tile.field_span_m * 0.08;

        Ok(Self {
            grid,
            corridor_grid,
            material_hints,
            grid_n,
            origin_x_m: tile.field_origin_x_m,
            origin_z_m: tile.field_origin_z_m,
            span_x_m: tile.field_span_m,
            span_z_m: tile.field_span_m,
            outside_height_m,
            edge_fade_m,
            generator_version: tile.generator_version.clone(),
            source_scope: tile.source_scope.clone(),
            height_scale_m: payload.height_scale_m,
            feature_span_m: payload.feature_span_m,
            source_origin_x_m: tile.source_origin_x_m,
            source_origin_z_m: tile.source_origin_z_m,
            source_span_x_m: tile.source_span_m,
            source_span_z_m: tile.source_span_m,
            source_scene_ratio: tile.source_scene_ratio,
            has_corridor,
            corridor_frac,
            has_material_hints,
            material_hint_fracs,
            pass_network_routes,
            pass_network_walkable_frac,
            pass_network_carved_frac,
            has_conditioning_stats,
            conditioning_stats,
        })
    }
}

impl StaticRuntimeTileFields {
    fn has_all_material_hints(&self) -> bool {
        self.low_pass_hint.is_some()
            && self.floor_hint.is_some()
            && self.rock_hint.is_some()
            && self.snow_hint.is_some()
    }

    fn has_any_material_hint(&self) -> bool {
        self.low_pass_hint.is_some()
            || self.floor_hint.is_some()
            || self.rock_hint.is_some()
            || self.snow_hint.is_some()
    }

    fn validate_material_hint_lengths(&self, expected: usize) -> Result<(), String> {
        for (name, values) in [
            ("low_pass_hint", self.low_pass_hint.as_ref()),
            ("floor_hint", self.floor_hint.as_ref()),
            ("rock_hint", self.rock_hint.as_ref()),
            ("snow_hint", self.snow_hint.as_ref()),
        ] {
            let Some(values) = values else {
                return Err(format!("static reference runtime tile: missing {name}"));
            };
            if values.len() != expected {
                return Err(format!(
                    "static reference runtime tile: {name}={} expected {}",
                    values.len(),
                    expected
                ));
            }
            if values
                .iter()
                .any(|v| !v.is_finite() || *v < 0.0 || *v > 1.0)
            {
                return Err(format!(
                    "static reference runtime tile: {name} values must be finite in [0,1]"
                ));
            }
        }
        Ok(())
    }
}
