use godot::prelude::*;

mod hash;
mod bind_worldgen;

#[cfg(test)]
mod hash_tests;

struct Wg10Terrain;

#[gdextension]
unsafe impl ExtensionLibrary for Wg10Terrain {}
