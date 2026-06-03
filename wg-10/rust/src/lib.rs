use godot::prelude::*;

mod hash;
mod npy;
mod pack;
mod grammar;
mod height;
mod parity;
mod bind_worldgen;
mod gpu_compute;
mod page_compute;
mod page_policy;
mod page_pool;
mod schedule_policy;
mod streamer;
mod ring_geometry;
mod clipmap_rings;
mod terrain_view;
mod edit_layer;
mod facts;
mod facts_api;
mod recipe_noise;
mod array_ops;
mod recipes;
mod recipes_volcanic;
mod recipes_glacial;
mod recipes_karst;
mod recipes_grassland;
mod recipes_desert;
mod recipes_temperate;
mod recipes_tundra;
mod recipes_rainforest;
mod recipes_coast;
mod recipes_wetland;
mod biome_compose;
pub(crate) mod flow_spike;
mod page_measure;
mod primitive_probe;
mod biome_page_compute;

#[cfg(test)]
mod hash_tests;
#[cfg(test)]
mod npy_tests;
#[cfg(test)]
mod pack_tests;
#[cfg(test)]
mod grammar_tests;
#[cfg(test)]
mod height_tests;
#[cfg(test)]
mod parity_tests;
#[cfg(test)]
mod page_policy_tests;
#[cfg(test)]
mod schedule_policy_tests;
#[cfg(test)]
mod ring_geometry_tests;
#[cfg(test)]
mod edit_layer_tests;
#[cfg(test)]
mod facts_tests;
#[cfg(test)]
mod recipe_noise_tests;
#[cfg(test)]
mod array_ops_tests;
#[cfg(test)]
mod recipes_tests;
#[cfg(test)]
mod recipes_volcanic_tests;
#[cfg(test)]
mod recipes_glacial_tests;
#[cfg(test)]
mod recipes_karst_tests;
#[cfg(test)]
mod recipes_grassland_tests;
#[cfg(test)]
mod recipes_desert_tests;
#[cfg(test)]
mod recipes_temperate_tests;
#[cfg(test)]
mod recipes_tundra_tests;
#[cfg(test)]
mod recipes_rainforest_tests;
#[cfg(test)]
mod recipes_coast_tests;
#[cfg(test)]
mod recipes_wetland_tests;
#[cfg(test)]
mod biome_compose_tests;
#[cfg(test)]
mod page_measure_tests;
#[cfg(test)]
mod biome_page_runtime_tests;

struct Wg10Terrain;

#[gdextension]
unsafe impl ExtensionLibrary for Wg10Terrain {}
