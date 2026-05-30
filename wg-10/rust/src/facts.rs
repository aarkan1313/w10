//! Facts composition (DESIGN M4). Pure, no `godot` imports. Combines the base height, an edit
//! delta, and a config clamp into the authoritative surface height the simulation reads:
//!   height = clamp(base + delta, bedrock_floor, ceiling)
//! and samples that over a grid for collision. The base height fn + provider live elsewhere; this
//! module is just the composition + grid sampler so it is trivially unit-testable.

/// Authoritative composed height at a point: base + edit delta, clamped to [floor, ceil].
/// `floor`/`ceil` are config (e.g. bedrock at -2 m, or NEG_INFINITY/INFINITY for unlimited).
pub fn composed_height(base: f64, delta: f64, floor: f64, ceil: f64) -> f64 {
    (base + delta).clamp(floor, ceil)
}
