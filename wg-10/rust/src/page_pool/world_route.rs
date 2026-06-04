//! Grammar-to-runtime-biome routing helpers for `Wg10PagePool`.
//!
//! This module owns WORLD-mode grammar-to-runtime-biome mapping.
//!
//! Page-center selection remains available for diagnostics and HUD route labels.
//! Runtime WORLD production uses the texel-corner weight field so active biome
//! weights feed the compose producer instead of choosing one biome for the page.

use std::collections::BTreeMap;

use crate::grammar;
use crate::pack::Pack;

pub(super) struct BiomeWeightField {
    pub names: Vec<String>,
    pub weights: Vec<Vec<f32>>,
    pub rows: usize,
    pub cols: usize,
}

pub(super) fn biome_weights_for_page<F>(
    seed: i64,
    pack: &Pack,
    origin_x: f64,
    origin_z: f64,
    world_span: f64,
    is_supported: &F,
) -> BTreeMap<String, f64>
where
    F: Fn(&str) -> bool,
{
    let cx = origin_x + world_span * 0.5;
    let cz = origin_z + world_span * 0.5;
    biome_weights_at_point(seed, pack, cx, cz, is_supported)
}

pub(super) fn biome_weights_at_point<F>(
    seed: i64,
    pack: &Pack,
    x: f64,
    z: f64,
    is_supported: &F,
) -> BTreeMap<String, f64>
where
    F: Fn(&str) -> bool,
{
    let weights = grammar::family_weights(x, z, seed, pack);
    let mut by_biome: BTreeMap<String, f64> = BTreeMap::new();
    for &(family_idx, weight) in weights.entries() {
        let Some(family_id) = pack.family_ids.get(family_idx as usize) else {
            continue;
        };
        let biome = runtime_biome_from_family_id(family_id);
        if is_supported(biome) {
            *by_biome.entry(biome.to_string()).or_insert(0.0) += weight;
        }
    }
    by_biome
}

pub(super) fn selected_biome_name(weights: &BTreeMap<String, f64>) -> String {
    weights
        .iter()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(biome, _)| biome.as_str())
        .unwrap_or("mountain")
        .to_string()
}

pub(super) fn select_biome_name_for_page<F>(
    seed: i64,
    pack: &Pack,
    origin_x: f64,
    origin_z: f64,
    world_span: f64,
    is_supported: &F,
) -> String
where
    F: Fn(&str) -> bool,
{
    selected_biome_name(&biome_weights_for_page(
        seed,
        pack,
        origin_x,
        origin_z,
        world_span,
        is_supported,
    ))
}

pub(super) fn biome_weight_field_for_page<F>(
    seed: i64,
    pack: &Pack,
    origin_x: f64,
    origin_z: f64,
    world_span: f64,
    page_px: usize,
    is_supported: &F,
) -> BiomeWeightField
where
    F: Fn(&str) -> bool,
{
    let n = page_px * page_px;
    let denom = page_px.saturating_sub(1).max(1) as f64;
    let mut by_biome: BTreeMap<String, Vec<f32>> = BTreeMap::new();

    for row in 0..page_px {
        let z = origin_z + world_span * (row as f64 / denom);
        for col in 0..page_px {
            let x = origin_x + world_span * (col as f64 / denom);
            let idx = row * page_px + col;
            for (biome, weight) in biome_weights_at_point(seed, pack, x, z, is_supported) {
                by_biome.entry(biome).or_insert_with(|| vec![0.0; n])[idx] = weight as f32;
            }
        }
    }

    let mut names = Vec::with_capacity(by_biome.len());
    let mut weights = Vec::with_capacity(by_biome.len());
    for (name, field) in by_biome {
        names.push(name);
        weights.push(field);
    }
    BiomeWeightField {
        names,
        weights,
        rows: page_px,
        cols: page_px,
    }
}

fn runtime_biome_from_family_id(family_id: &str) -> &str {
    let stem = family_id
        .split_once("__")
        .map(|(stem, _)| stem)
        .unwrap_or(family_id);
    match stem {
        // Badlands has accepted setup artifacts, but no GPU fragment/schedule in the 11-biome
        // runtime set yet. Desert is the closest available routed fallback until badlands is ported.
        "badlands" => "desert",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grammar_pack() -> Pack {
        crate::pack::load_pack_str(
            r#"{
                "schema":"worldgen10.terrain_pack.v1",
                "version":1,
                "grammar_constants":{
                    "region_size_m":1024.0,
                    "province_size_regions":2,
                    "palette_primary_pct":100,
                    "palette_compatible_pct":0
                },
                "palettes":[
                    {"id":"p0","families":["mountain__a","desert__a","wetland__a"]}
                ],
                "compatibility":{},
                "families":{
                    "mountain__a":{},
                    "desert__a":{},
                    "wetland__a":{}
                }
            }"#,
        )
        .expect("test pack")
    }

    #[test]
    fn weight_field_is_partition_of_unity_per_texel() {
        let pack = grammar_pack();
        let supported = |biome: &str| matches!(biome, "mountain" | "desert" | "wetland");
        let field = biome_weight_field_for_page(1337, &pack, -2048.0, 512.0, 4096.0, 9, &supported);

        assert_eq!(field.rows, 9);
        assert_eq!(field.cols, 9);
        assert!(!field.names.is_empty());
        assert_eq!(field.names.len(), field.weights.len());
        for weights in &field.weights {
            assert_eq!(weights.len(), 81);
        }

        for idx in 0..81 {
            let sum: f32 = field.weights.iter().map(|w| w[idx]).sum();
            assert!(
                (sum - 1.0).abs() < 1.0e-5,
                "weight sum at {idx} = {sum}, names={:?}",
                field.names
            );
        }
    }

    #[test]
    fn unsupported_families_are_dropped_from_weight_field() {
        let pack = grammar_pack();
        let supported = |biome: &str| biome == "mountain";
        let field = biome_weight_field_for_page(1337, &pack, 0.0, 0.0, 1024.0, 3, &supported);

        assert_eq!(field.names, vec!["mountain".to_string()]);
        assert_eq!(field.weights.len(), 1);
        assert!(field.weights[0].iter().all(|w| *w >= 0.0 && *w <= 1.0));
    }
}
