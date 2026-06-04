//! WORLD producer route and weight-field helpers for `Wg10PagePool`.
//!
//! `world_route` owns pure grammar-to-runtime-biome math. This module adapts
//! that math to the configured WORLD runtime context stored in the page pool.

use std::collections::BTreeMap;

use super::{world_route, BiomeWorldRuntime, Wg10PagePool};

impl Wg10PagePool {
    pub(super) fn select_world_biome_name(
        &self,
        world: &BiomeWorldRuntime,
        origin_x: f64,
        origin_z: f64,
        world_span: f64,
    ) -> String {
        let supported = |biome: &str| world.contexts.contains_key(biome);
        world_route::select_biome_name_for_page(
            self.seed,
            &world.pack,
            origin_x,
            origin_z,
            world_span,
            &supported,
        )
    }

    pub(super) fn world_biome_weights(
        &self,
        world: &BiomeWorldRuntime,
        origin_x: f64,
        origin_z: f64,
        world_span: f64,
    ) -> BTreeMap<String, f64> {
        let supported = |biome: &str| world.contexts.contains_key(biome);
        world_route::biome_weights_for_page(
            self.seed,
            &world.pack,
            origin_x,
            origin_z,
            world_span,
            &supported,
        )
    }

    pub(super) fn world_biome_weights_at(
        &self,
        world: &BiomeWorldRuntime,
        x: f64,
        z: f64,
    ) -> BTreeMap<String, f64> {
        let supported = |biome: &str| world.contexts.contains_key(biome);
        world_route::biome_weights_at_point(self.seed, &world.pack, x, z, &supported)
    }

    pub(super) fn world_biome_weight_field(
        &self,
        world: &BiomeWorldRuntime,
        origin_x: f64,
        origin_z: f64,
        world_span: f64,
        page_px: usize,
    ) -> world_route::BiomeWeightField {
        let supported = |biome: &str| world.contexts.contains_key(biome);
        if world.active_limit == 1 {
            return world_route::single_biome_weight_field_for_page(
                self.seed,
                &world.pack,
                origin_x,
                origin_z,
                world_span,
                page_px,
                &supported,
            );
        }
        world_route::biome_weight_field_for_page(
            self.seed,
            &world.pack,
            origin_x,
            origin_z,
            world_span,
            page_px,
            world.active_limit,
            &supported,
        )
    }
}
