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

struct Wg10Terrain;

#[gdextension]
unsafe impl ExtensionLibrary for Wg10Terrain {}
