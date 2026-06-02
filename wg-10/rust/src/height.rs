//! WorldGen10 height layer. Pure deterministic math, no `godot` imports
//! (DESIGN §6.3). Turns the grammar's family-weight blend into elevation by
//! tiled-sampling per-family kernels, slope-moderating amplitude, and composing.
//! Consumes `grammar::family_weights`; the grammar never reads kernel data.
//!
//! ⚠ LEGACY / SCAFFOLDING — being REPLACED at Slice 4. `sample_kernel`/`height` are the OLD
//! kernel-tiling formula (the "blobby/tiling" look the worldgen-core rebuild supersedes). They are
//! still the LIVE per-point formula behind `facts_api::base_height` until the Slice-4 page-path swap
//! routes the runtime through the accepted 11-biome composition stack (recipes_*.rs + biome_compose.rs,
//! all parity-ported to Rust). KNOWN BUG carried by this legacy path (do NOT build new work on it):
//! `sample * relief_m` uses z-score DEM kernels with the FULL height_range_m as `relief_m`, which
//! over-amplifies relief (DESIGN §"z-score"; LOOSE_ENDS_LEDGER). The fix is the Slice-4 replacement,
//! not patching this scaffolding. Kept only so M0-M4 parity gates + facts keep working pre-swap.

use crate::grammar;
use crate::pack::FamilyKernel;
use crate::pack::Pack;

/// Wrap an integer grid index into `[0, n)` (tile). `n` is always >= 1.
fn wrap(i: i64, n: usize) -> usize {
    (i.rem_euclid(n as i64)) as usize
}

/// Tiled bilinear sample of one kernel at a world coordinate, scaled to
/// `relief_m`. The kernel repeats every `footprint_m`; neighbours past the last
/// texel wrap to texel 0, so the stamp tiles seamlessly (C0 across footprints;
/// not C1 — visible creases at footprint seams are expected for naive tiling,
/// anti-repetition is deferred per design §1).
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

/// Amplitude moderation from local kernel slope. Steeper -> smaller factor,
/// clamped to `[moderation_min, 1.0]`. AMPLITUDE ONLY — never changes which
/// families appear (design §2 constraint #3).
pub fn moderation(slope: f64, moderation_min: f64, strength: f64) -> f64 {
    (1.0 - strength * slope).clamp(moderation_min, 1.0)
}

/// Local slope magnitude of a kernel at a world coord: central difference of the
/// (relief-scaled) sample over one kernel texel in each axis, normalised by the
/// texel's world size so slope is a normalized rise (sample delta / relief_m)
/// over one texel's world width — a tunable heuristic, not a physical gradient.
fn local_slope(fk: &FamilyKernel, x: f64, z: f64) -> f64 {
    let dx = fk.footprint_m / fk.kernel.cols as f64;
    let dz = fk.footprint_m / fk.kernel.rows as f64;
    let sx = (sample_kernel(fk, x + dx, z) - sample_kernel(fk, x - dx, z)) / (2.0 * fk.relief_m);
    let sz = (sample_kernel(fk, x, z + dz) - sample_kernel(fk, x, z - dz)) / (2.0 * fk.relief_m);
    (sx * sx + sz * sz).sqrt()
}

/// Elevation at a world coordinate: blend each present family's moderated kernel
/// contribution by its grammar weight. The grammar (which never sees kernel
/// data) decides the families; this layer only sets amplitude.
pub fn height(x: f64, z: f64, seed: i64, pack: &Pack) -> f64 {
    let c = &pack.grammar_constants;
    let weights = grammar::family_weights(x, z, seed, pack);
    let mut h = 0.0;
    for (fam, weight) in weights.entries() {
        let name = &pack.family_ids[*fam as usize];
        let fk = pack
            .family_kernel(name)
            .unwrap_or_else(|| panic!("height layer: family {name:?} has no kernel data"));
        let slope = local_slope(fk, x, z);
        let m = moderation(slope, c.moderation_min, c.moderation_strength);
        h += weight * m * sample_kernel(fk, x, z);
    }
    h
}
