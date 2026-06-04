use serde::Deserialize;

use super::{
    StaticConditioningStats, StaticHeightRuntime, StaticMaterialHintFractions,
    StaticMaterialHintGrids,
};

#[derive(Deserialize)]
pub(super) struct StaticPayload {
    generator_version: String,
    source_scope: String,
    chunk_count: usize,
    chunk_n: usize,
    chunk_span_m: f64,
    world_span_m: f64,
    height_scale_m: f64,
    feature_span_m: f64,
    seeds: Vec<StaticSeed>,
}

#[derive(Deserialize)]
struct StaticSeed {
    chunks: Vec<StaticChunk>,
    pass_network: Option<StaticPassNetwork>,
    stats: Option<StaticConditioningStats>,
}

#[derive(Deserialize)]
struct StaticPassNetwork {
    routes: i64,
    band_walkable_frac: f64,
    carved_frac: f64,
}

#[derive(Deserialize)]
struct StaticChunk {
    chunk_x: usize,
    chunk_z: usize,
    n: usize,
    span_m: f64,
    display_origin_x_m: f64,
    display_origin_z_m: f64,
    height: Vec<f32>,
    corridor: Option<Vec<i64>>,
    low_pass_hint: Option<Vec<f32>>,
    floor_hint: Option<Vec<f32>>,
    rock_hint: Option<Vec<f32>>,
    snow_hint: Option<Vec<f32>>,
}

impl StaticHeightRuntime {
    pub(super) fn from_payload(payload: StaticPayload) -> Result<Self, String> {
        if payload.chunk_count == 0 {
            return Err("static reference: chunk_count must be > 0".into());
        }
        if payload.chunk_n < 2 {
            return Err("static reference: chunk_n must be >= 2".into());
        }
        if payload.source_scope.trim().is_empty() {
            return Err("static reference: source_scope must be non-empty".into());
        }
        if payload.generator_version.trim().is_empty() {
            return Err("static reference: generator_version must be non-empty".into());
        }
        let seed = payload
            .seeds
            .first()
            .ok_or("static reference: payload has no seeds")?;
        let expected_chunks = payload
            .chunk_count
            .checked_mul(payload.chunk_count)
            .ok_or("static reference: chunk_count overflow")?;
        if seed.chunks.len() != expected_chunks {
            return Err(format!(
                "static reference: chunks={} expected {}",
                seed.chunks.len(),
                expected_chunks
            ));
        }

        let step = payload.chunk_n - 1;
        let grid_n = payload
            .chunk_count
            .checked_mul(step)
            .and_then(|v| v.checked_add(1))
            .ok_or("static reference: grid_n overflow")?;
        let grid_len = grid_n
            .checked_mul(grid_n)
            .ok_or("static reference: grid size overflow")?;
        let mut grid = vec![0.0_f32; grid_len];
        let mut filled = vec![false; grid_len];
        let any_corridor = seed.chunks.iter().any(|chunk| chunk.corridor.is_some());
        let all_corridor = seed.chunks.iter().all(|chunk| chunk.corridor.is_some());
        if any_corridor && !all_corridor {
            return Err("static reference: corridor must be present on all chunks or none".into());
        }
        let mut corridor_grid = all_corridor.then(|| vec![0_u8; grid_len]);
        let mut corridor_filled = all_corridor.then(|| vec![false; grid_len]);
        let all_hints = seed
            .chunks
            .iter()
            .all(|chunk| chunk.has_all_material_hints());
        let any_hints = seed
            .chunks
            .iter()
            .any(|chunk| chunk.has_any_material_hint());
        if any_hints && !all_hints {
            return Err("static reference: material hints must be present as a complete set on all chunks or none".into());
        }
        let mut material_hints = all_hints.then(|| StaticMaterialHintGrids {
            low_pass: vec![0.0; grid_len],
            floor: vec![0.0; grid_len],
            rock: vec![0.0; grid_len],
            snow: vec![0.0; grid_len],
        });
        let mut material_hints_filled = all_hints.then(|| vec![false; grid_len]);

        let mut min_x = f64::INFINITY;
        let mut min_z = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_z = f64::NEG_INFINITY;

        for chunk in &seed.chunks {
            if chunk.n != payload.chunk_n {
                return Err(format!(
                    "static reference: chunk ({},{}) n={} expected {}",
                    chunk.chunk_x, chunk.chunk_z, chunk.n, payload.chunk_n
                ));
            }
            let expected_heights = payload
                .chunk_n
                .checked_mul(payload.chunk_n)
                .ok_or("static reference: height len overflow")?;
            if chunk.height.len() != expected_heights {
                return Err(format!(
                    "static reference: chunk ({},{}) heights={} expected {}",
                    chunk.chunk_x,
                    chunk.chunk_z,
                    chunk.height.len(),
                    expected_heights
                ));
            }
            if let Some(corridor) = chunk.corridor.as_ref() {
                if corridor.len() != expected_heights {
                    return Err(format!(
                        "static reference: chunk ({},{}) corridor={} expected {}",
                        chunk.chunk_x,
                        chunk.chunk_z,
                        corridor.len(),
                        expected_heights
                    ));
                }
            }
            if all_hints {
                chunk.validate_material_hint_lengths(expected_heights)?;
            }
            if (chunk.span_m - payload.chunk_span_m).abs() > 1.0e-6 {
                return Err(format!(
                    "static reference: chunk ({},{}) span={} expected {}",
                    chunk.chunk_x, chunk.chunk_z, chunk.span_m, payload.chunk_span_m
                ));
            }
            if chunk.chunk_x >= payload.chunk_count || chunk.chunk_z >= payload.chunk_count {
                return Err(format!(
                    "static reference: chunk index out of range ({},{})",
                    chunk.chunk_x, chunk.chunk_z
                ));
            }

            min_x = min_x.min(chunk.display_origin_x_m);
            min_z = min_z.min(chunk.display_origin_z_m);
            max_x = max_x.max(chunk.display_origin_x_m + chunk.span_m);
            max_z = max_z.max(chunk.display_origin_z_m + chunk.span_m);

            let gx0 = chunk.chunk_x * step;
            let gz0 = chunk.chunk_z * step;
            for z in 0..payload.chunk_n {
                for x in 0..payload.chunk_n {
                    let gx = gx0 + x;
                    let gz = gz0 + z;
                    let dst = gz * grid_n + gx;
                    let src = z * payload.chunk_n + x;
                    grid[dst] = chunk.height[src] * payload.height_scale_m as f32;
                    filled[dst] = true;
                    if let (Some(corridor), Some(cg), Some(cf)) = (
                        chunk.corridor.as_ref(),
                        corridor_grid.as_mut(),
                        corridor_filled.as_mut(),
                    ) {
                        let value = if corridor[src] != 0 { 1_u8 } else { 0_u8 };
                        if cf[dst] && cg[dst] != value {
                            return Err(format!(
                                "static reference: corridor seam mismatch at flat index {dst}"
                            ));
                        }
                        cg[dst] = value;
                        cf[dst] = true;
                    }
                    if let (Some(hints), Some(filled)) =
                        (material_hints.as_mut(), material_hints_filled.as_mut())
                    {
                        let values = chunk.material_hint_values(src).ok_or_else(|| {
                            format!(
                                "static reference: missing material hint at chunk ({},{})",
                                chunk.chunk_x, chunk.chunk_z
                            )
                        })?;
                        if filled[dst]
                            && (!same_hint(hints.low_pass[dst], values.low_pass)
                                || !same_hint(hints.floor[dst], values.floor)
                                || !same_hint(hints.rock[dst], values.rock)
                                || !same_hint(hints.snow[dst], values.snow))
                        {
                            return Err(format!(
                                "static reference: material hint seam mismatch at flat index {dst}"
                            ));
                        }
                        hints.low_pass[dst] = values.low_pass;
                        hints.floor[dst] = values.floor;
                        hints.rock[dst] = values.rock;
                        hints.snow[dst] = values.snow;
                        filled[dst] = true;
                    }
                }
            }
        }

        if let Some(idx) = filled.iter().position(|v| !*v) {
            return Err(format!(
                "static reference: missing grid sample at flat index {idx}"
            ));
        }
        if let Some(cf) = corridor_filled.as_ref() {
            if let Some(idx) = cf.iter().position(|v| !*v) {
                return Err(format!(
                    "static reference: missing corridor sample at flat index {idx}"
                ));
            }
        }
        if let Some(mf) = material_hints_filled.as_ref() {
            if let Some(idx) = mf.iter().position(|v| !*v) {
                return Err(format!(
                    "static reference: missing material hint sample at flat index {idx}"
                ));
            }
        }
        let span_x = max_x - min_x;
        let span_z = max_z - min_z;
        if (span_x - payload.world_span_m).abs() > 0.01
            || (span_z - payload.world_span_m).abs() > 0.01
        {
            return Err(format!(
                "static reference: stitched span {:.3}x{:.3} expected {:.3}",
                span_x, span_z, payload.world_span_m
            ));
        }
        let corridor_frac = corridor_grid
            .as_ref()
            .map(|corridor| {
                corridor.iter().filter(|v| **v != 0).count() as f64 / corridor.len() as f64
            })
            .unwrap_or(0.0);
        let pass_network = seed.pass_network.as_ref();
        let pass_network_routes = pass_network.map(|p| p.routes).unwrap_or(0);
        let pass_network_walkable_frac = pass_network.map(|p| p.band_walkable_frac).unwrap_or(0.0);
        let pass_network_carved_frac = pass_network.map(|p| p.carved_frac).unwrap_or(0.0);
        let (has_conditioning_stats, conditioning_stats) = match seed.stats {
            Some(stats) => {
                validate_conditioning_stats(stats)?;
                (true, stats)
            }
            None => (false, StaticConditioningStats::default()),
        };
        let has_corridor = corridor_grid.is_some();
        let material_hint_fracs = material_hints
            .as_ref()
            .map(material_hint_fractions)
            .unwrap_or_default();
        let has_material_hints = material_hints.is_some();
        let outside_height_m = height_percentile(&grid, 0.05);
        let edge_fade_m = span_x.min(span_z) * 0.08;

        Ok(Self {
            grid,
            corridor_grid,
            material_hints,
            grid_n,
            origin_x_m: min_x,
            origin_z_m: min_z,
            span_x_m: span_x,
            span_z_m: span_z,
            outside_height_m,
            edge_fade_m,
            generator_version: payload.generator_version,
            source_scope: payload.source_scope,
            height_scale_m: payload.height_scale_m,
            feature_span_m: payload.feature_span_m,
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

impl StaticChunk {
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
                return Err(format!(
                    "static reference: chunk ({},{}) missing {name}",
                    self.chunk_x, self.chunk_z
                ));
            };
            if values.len() != expected {
                return Err(format!(
                    "static reference: chunk ({},{}) {name}={} expected {}",
                    self.chunk_x,
                    self.chunk_z,
                    values.len(),
                    expected
                ));
            }
            if values
                .iter()
                .any(|v| !v.is_finite() || *v < 0.0 || *v > 1.0)
            {
                return Err(format!(
                    "static reference: chunk ({},{}) {name} values must be finite in [0,1]",
                    self.chunk_x, self.chunk_z
                ));
            }
        }
        Ok(())
    }

    fn material_hint_values(&self, idx: usize) -> Option<StaticMaterialHintValues> {
        Some(StaticMaterialHintValues {
            low_pass: *self.low_pass_hint.as_ref()?.get(idx)?,
            floor: *self.floor_hint.as_ref()?.get(idx)?,
            rock: *self.rock_hint.as_ref()?.get(idx)?,
            snow: *self.snow_hint.as_ref()?.get(idx)?,
        })
    }
}

#[derive(Clone, Copy)]
struct StaticMaterialHintValues {
    low_pass: f32,
    floor: f32,
    rock: f32,
    snow: f32,
}

fn same_hint(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1.0e-6
}

fn material_hint_fractions(hints: &StaticMaterialHintGrids) -> StaticMaterialHintFractions {
    fn coverage(values: &[f32]) -> f64 {
        values.iter().filter(|v| **v >= 0.5).count() as f64 / values.len() as f64
    }
    StaticMaterialHintFractions {
        low_pass: coverage(&hints.low_pass),
        floor: coverage(&hints.floor),
        rock: coverage(&hints.rock),
        snow: coverage(&hints.snow),
    }
}

fn height_percentile(grid: &[f32], fraction: f64) -> f32 {
    let mut values: Vec<f32> = grid.iter().copied().filter(|v| v.is_finite()).collect();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((values.len() as f64 * fraction).ceil() as usize)
        .saturating_sub(1)
        .min(values.len() - 1);
    values[idx]
}

fn validate_conditioning_stats(stats: StaticConditioningStats) -> Result<(), String> {
    for (name, value) in [
        ("source_min", stats.source_min),
        ("source_max", stats.source_max),
        ("source_ptp", stats.source_ptp),
        ("p05", stats.p05),
        ("p50", stats.p50),
        ("p95", stats.p95),
        ("conditioned_min", stats.conditioned_min),
        ("conditioned_max", stats.conditioned_max),
        ("conditioned_ptp", stats.conditioned_ptp),
    ] {
        if !value.is_finite() {
            return Err(format!(
                "static reference: conditioning stats {name} must be finite"
            ));
        }
    }
    if stats.source_min > stats.source_max {
        return Err("static reference: conditioning stats source_min > source_max".into());
    }
    if stats.conditioned_min > stats.conditioned_max {
        return Err(
            "static reference: conditioning stats conditioned_min > conditioned_max".into(),
        );
    }
    if stats.source_ptp <= 0.0 {
        return Err("static reference: conditioning stats source_ptp must be > 0".into());
    }
    if stats.conditioned_ptp <= 0.0 {
        return Err("static reference: conditioning stats conditioned_ptp must be > 0".into());
    }
    if stats.p05 > stats.p50 || stats.p50 > stats.p95 || stats.p95 <= stats.p05 {
        return Err("static reference: conditioning stats percentiles must satisfy p05 <= p50 <= p95 with p95 > p05".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_chunk_payload(corridor: Option<Vec<i64>>) -> StaticPayload {
        StaticPayload {
            generator_version: "mountain_synthesis_v0_9x9_original_scene_scale_review_pass_network"
                .into(),
            source_scope: "coherent_full_field_carved_with_pass_network_sliced_for_review".into(),
            chunk_count: 1,
            chunk_n: 2,
            chunk_span_m: 10.0,
            world_span_m: 10.0,
            height_scale_m: 100.0,
            feature_span_m: 90_000.0,
            seeds: vec![StaticSeed {
                pass_network: Some(StaticPassNetwork {
                    routes: 12,
                    band_walkable_frac: 0.75,
                    carved_frac: 0.25,
                }),
                stats: Some(StaticConditioningStats {
                    source_min: -1.0,
                    source_max: 2.0,
                    source_ptp: 3.0,
                    p05: -0.8,
                    p50: 0.1,
                    p95: 1.5,
                    conditioned_min: -0.9,
                    conditioned_max: 0.8,
                    conditioned_ptp: 1.7,
                }),
                chunks: vec![StaticChunk {
                    chunk_x: 0,
                    chunk_z: 0,
                    n: 2,
                    span_m: 10.0,
                    display_origin_x_m: -5.0,
                    display_origin_z_m: -5.0,
                    height: vec![0.0, 1.0, 2.0, 3.0],
                    corridor,
                    low_pass_hint: None,
                    floor_hint: None,
                    rock_hint: None,
                    snow_hint: None,
                }],
            }],
        }
    }

    fn one_chunk_payload_with_hints() -> StaticPayload {
        let mut payload = one_chunk_payload(Some(vec![1, 0, 1, 0]));
        let chunk = &mut payload.seeds[0].chunks[0];
        chunk.low_pass_hint = Some(vec![1.0, 0.0, 1.0, 0.0]);
        chunk.floor_hint = Some(vec![1.0, 0.25, 0.75, 0.0]);
        chunk.rock_hint = Some(vec![0.0, 0.5, 0.75, 1.0]);
        chunk.snow_hint = Some(vec![0.0, 0.0, 1.0, 1.0]);
        payload
    }

    #[test]
    fn payload_contract_metadata_and_corridor_are_preserved() {
        let rt = StaticHeightRuntime::from_payload(one_chunk_payload(Some(vec![1, 0, 1, 0])))
            .expect("payload should parse");

        assert_eq!(
            rt.source_scope,
            "coherent_full_field_carved_with_pass_network_sliced_for_review"
        );
        assert_eq!(rt.pass_network_routes, 12);
        assert!((rt.pass_network_walkable_frac - 0.75).abs() < 1.0e-12);
        assert!((rt.pass_network_carved_frac - 0.25).abs() < 1.0e-12);
        assert!(rt.has_conditioning_stats);
        assert!((rt.conditioning_stats.source_ptp - 3.0).abs() < 1.0e-12);
        assert!((rt.conditioning_stats.p05 + 0.8).abs() < 1.0e-12);
        assert!((rt.conditioning_stats.p95 - 1.5).abs() < 1.0e-12);
        assert!((rt.conditioning_stats.conditioned_ptp - 1.7).abs() < 1.0e-12);
        assert!(rt.has_corridor);
        assert!((rt.corridor_frac - 0.5).abs() < 1.0e-12);
        assert!((rt.corridor_fraction_for_page(-5.0, -5.0, 10.0, 2) - 0.5).abs() < 1.0e-12);
        assert_eq!(rt.sample(-5.0, -5.0), 0.0);
        assert_eq!(rt.sample(5.0, 5.0), 300.0);
    }

    #[test]
    fn payload_outside_domain_fades_to_low_floor_instead_of_smearing_edge() {
        let rt = StaticHeightRuntime::from_payload(one_chunk_payload(Some(vec![1, 0, 1, 0])))
            .expect("payload should parse");

        assert_eq!(rt.sample(5.0, 5.0), 300.0);
        assert_eq!(rt.sample(50.0, 50.0), 0.0);
        assert!(!rt.sample_corridor(50.0, 50.0));
        assert_eq!(
            rt.corridor_fraction_for_page(45.0, 45.0, 10.0, 2),
            0.0
        );
    }

    #[test]
    fn payload_without_source_scope_is_rejected() {
        let mut payload = one_chunk_payload(None);
        payload.source_scope.clear();

        let err = match StaticHeightRuntime::from_payload(payload) {
            Ok(_) => panic!("empty scope should be rejected"),
            Err(err) => err,
        };
        assert!(err.contains("source_scope"));
    }

    #[test]
    fn payload_invalid_conditioning_stats_are_rejected() {
        let mut payload = one_chunk_payload(None);
        payload.seeds[0].stats.as_mut().unwrap().p95 = -0.8;

        let err = match StaticHeightRuntime::from_payload(payload) {
            Ok(_) => panic!("invalid conditioning stats should be rejected"),
            Err(err) => err,
        };
        assert!(err.contains("conditioning stats"));
    }

    #[test]
    fn payload_material_hints_are_preserved_and_page_sampled() {
        let rt = StaticHeightRuntime::from_payload(one_chunk_payload_with_hints())
            .expect("payload should parse");

        assert!(rt.has_material_hints);
        assert!((rt.material_hint_fracs.low_pass - 0.5).abs() < 1.0e-12);
        assert!((rt.material_hint_fracs.floor - 0.5).abs() < 1.0e-12);
        assert!((rt.material_hint_fracs.rock - 0.75).abs() < 1.0e-12);
        assert!((rt.material_hint_fracs.snow - 0.5).abs() < 1.0e-12);

        let page = rt
            .material_hint_fractions_for_page(-5.0, -5.0, 10.0, 2)
            .expect("page hints should sample");
        assert!((page.low_pass - 0.5).abs() < 1.0e-12);
        assert!((page.floor - 0.5).abs() < 1.0e-12);
        assert!((page.rock - 0.5625).abs() < 1.0e-12);
        assert!((page.snow - 0.5).abs() < 1.0e-12);
    }

    #[test]
    fn payload_material_hints_outside_domain_do_not_smear_edge() {
        let rt = StaticHeightRuntime::from_payload(one_chunk_payload_with_hints())
            .expect("payload should parse");

        let page = rt
            .material_hint_fractions_for_page(45.0, 45.0, 10.0, 2)
            .expect("page hints should sample");
        assert_eq!(page.low_pass, 0.0);
        assert_eq!(page.floor, 0.0);
        assert_eq!(page.rock, 0.0);
        assert_eq!(page.snow, 0.0);
    }
}
