use godot::prelude::*;

mod hash;
mod pack;
mod grammar;
mod bind_worldgen;

#[cfg(test)]
mod hash_tests;
#[cfg(test)]
mod pack_tests;
#[cfg(test)]
mod grammar_tests;

struct Wg10Terrain;

#[gdextension]
unsafe impl ExtensionLibrary for Wg10Terrain {}
