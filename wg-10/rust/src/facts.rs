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

/// Sample authoritative heights over an n×n square grid centred at (center_x, center_z) spanning
/// `world_size` metres, row-major. `height_at(x,z)` is the composed height fn (the caller passes a
/// closure that already does base + delta + clamp). Texel-corner convention: cell (i,j) is at world
/// corner + (i,j)/(n-1)*world_size, corner = center - world_size/2 — so the n samples span the full
/// square inclusively (what Jolt HeightMapShape3D's map_data expects, width=depth=n). Returns empty
/// for invalid args (n < 2 or world_size <= 0); the caller checks emptiness (Jolt needs >= 2).
pub fn collision_field<F: Fn(f64, f64) -> f64>(
    center_x: f64,
    center_z: f64,
    world_size: f64,
    n: usize,
    height_at: F,
) -> Vec<f32> {
    let mut out = Vec::new();
    if n < 2 || world_size <= 0.0 {
        return out;
    }
    out.reserve(n * n);
    let corner_x = center_x - world_size * 0.5;
    let corner_z = center_z - world_size * 0.5;
    let step = world_size / (n as f64 - 1.0);
    for j in 0..n {
        let wz = corner_z + j as f64 * step;
        for i in 0..n {
            let wx = corner_x + i as f64 * step;
            out.push(height_at(wx, wz) as f32);
        }
    }
    out
}
