//! CPU side of the GPU parity contract: the family-selection signature. The GLSL
//! shader (a later task) computes the identical value; the parity gate compares
//! them exactly (Tier 1). Pure, no godot.

use crate::grammar;
use crate::hash;
use crate::pack::Pack;

/// Salt for the family-signature fold. Must match the GLSL `SALT_SIG`.
pub const SALT_SIG: u32 = 0x5349_4753;

/// A deterministic signature of the SET of families present in the blend at
/// (x,z): sorted ascending family ids, folded via `stable_hash_ints`. Two coords
/// select the same families <=> same signature (ignores the float weights —
/// Tier 2 covers magnitude). CPU and GPU must agree on this exactly.
pub fn family_signature(x: f64, z: f64, seed: i64, pack: &Pack) -> u32 {
    let w = grammar::family_weights(x, z, seed, pack);
    let mut ids: Vec<i64> = w.entries().iter().map(|(fam, _)| *fam as i64).collect();
    ids.sort_unstable();
    hash::stable_hash_ints(SALT_SIG, &ids)
}
