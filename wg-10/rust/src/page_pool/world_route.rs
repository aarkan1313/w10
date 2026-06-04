//! Grammar-to-runtime-biome routing helpers for `Wg10PagePool`.
//!
//! This module owns the current WORLD-mode route decision: sample grammar at
//! the page center, aggregate family weights by available runtime biome, and
//! select the strongest biome. Full Slice 4 Part B should replace this hard
//! page route with per-pixel active-biome weights feeding the compose producer.

use std::collections::BTreeMap;

use crate::grammar;

use super::BiomeWorldRuntime;

pub(super) fn biome_weights_for_page(
    seed: i64,
    world: &BiomeWorldRuntime,
    origin_x: f64,
    origin_z: f64,
    world_span: f64,
) -> BTreeMap<String, f64> {
    let cx = origin_x + world_span * 0.5;
    let cz = origin_z + world_span * 0.5;
    biome_weights_at_point(seed, world, cx, cz)
}

pub(super) fn biome_weights_at_point(
    seed: i64,
    world: &BiomeWorldRuntime,
    x: f64,
    z: f64,
) -> BTreeMap<String, f64> {
    let weights = grammar::family_weights(x, z, seed, &world.pack);
    let mut by_biome: BTreeMap<String, f64> = BTreeMap::new();
    for &(family_idx, weight) in weights.entries() {
        let Some(family_id) = world.pack.family_ids.get(family_idx as usize) else {
            continue;
        };
        let biome = runtime_biome_from_family_id(family_id);
        if world.contexts.contains_key(biome) {
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

pub(super) fn select_biome_name_for_page(
    seed: i64,
    world: &BiomeWorldRuntime,
    origin_x: f64,
    origin_z: f64,
    world_span: f64,
) -> String {
    selected_biome_name(&biome_weights_for_page(
        seed, world, origin_x, origin_z, world_span,
    ))
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
