//! WorldGen10 height layer. Pure deterministic math, no `godot` imports
//! (DESIGN §6.3). Turns the grammar's family-weight blend into elevation by
//! tiled-sampling per-family kernels, slope-moderating amplitude, and composing.
//! Consumes `grammar::family_weights`; the grammar never reads kernel data.

use crate::pack::FamilyKernel;

/// Wrap an integer grid index into `[0, n)` (tile). `n` is always >= 1.
fn wrap(i: i64, n: usize) -> usize {
    (i.rem_euclid(n as i64)) as usize
}

/// Tiled bilinear sample of one kernel at a world coordinate, scaled to
/// `relief_m`. The kernel repeats every `footprint_m`; neighbours past the last
/// texel wrap to texel 0, so the stamp tiles seamlessly (C0 across footprints).
pub fn sample_kernel(fk: &FamilyKernel, x: f64, z: f64) -> f64 {
    let cols = fk.kernel.cols;
    let rows = fk.kernel.rows;

    // World -> kernel grid space. fract into [0,1), scaled to grid cells.
    let u = (x / fk.footprint_m).rem_euclid(1.0) * cols as f64;
    let v = (z / fk.footprint_m).rem_euclid(1.0) * rows as f64;
    let u0 = u.floor() as i64;
    let v0 = v.floor() as i64;
    let tu = u - u0 as f64;
    let tv = v - v0 as f64;

    let c00 = texel(fk, wrap(v0, rows), wrap(u0, cols));
    let c10 = texel(fk, wrap(v0, rows), wrap(u0 + 1, cols));
    let c01 = texel(fk, wrap(v0 + 1, rows), wrap(u0, cols));
    let c11 = texel(fk, wrap(v0 + 1, rows), wrap(u0 + 1, cols));

    let top = c00 + (c10 - c00) * tu;
    let bot = c01 + (c11 - c01) * tu;
    let sample = top + (bot - top) * tv; // normalized kernel value
    sample * fk.relief_m
}

/// Row-major texel access (already wrapped indices).
fn texel(fk: &FamilyKernel, row: usize, col: usize) -> f64 {
    fk.kernel.data[row * fk.kernel.cols + col] as f64
}
