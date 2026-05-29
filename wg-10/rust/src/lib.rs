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

struct Wg10Terrain;

#[gdextension]
unsafe impl ExtensionLibrary for Wg10Terrain {}
