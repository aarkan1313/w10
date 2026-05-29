//! WorldGen10 region/province grammar. Pure deterministic math, no `godot`
//! imports (DESIGN §6.3). Turns a world coordinate into a bounded blend of
//! terrain families. WG9 is reference/sanity-oracle only — this is W10's design.

use crate::hash::{self, HashVal};
use crate::pack::Pack;

/// Region cell index for a world coordinate. Floor (not truncate) so the cell
/// index is continuous across the x=0 / z=0 axes (DESIGN §4 seam rule).
pub fn region_of(x: f64, z: f64, pack: &Pack) -> (i64, i64) {
    let s = pack.grammar_constants.region_size_m;
    ((x / s).floor() as i64, (z / s).floor() as i64)
}

/// Province index of a region axis value (floor-divide by province size).
pub fn province_of(region_axis: i64, pack: &Pack) -> i64 {
    region_axis.div_euclid(pack.grammar_constants.province_size_regions)
}

/// Primary palette index for a province (province sets the regional bias).
fn province_primary_palette(prx: i64, prz: i64, seed: i64, pack: &Pack) -> usize {
    let h = hash::stable_hash(&[
        HashVal::Str("province_palette"),
        HashVal::Int(prx),
        HashVal::Int(prz),
        HashVal::Int(seed),
    ]);
    (h as usize) % pack.palettes.len()
}

/// Palette index for a region: primary (province bias) / compatible / rare,
/// chosen by a deterministic roll using the pack's pct thresholds.
pub fn palette_for_region(rx: i64, rz: i64, seed: i64, pack: &Pack) -> usize {
    let prx = province_of(rx, pack);
    let prz = province_of(rz, pack);
    let primary = province_primary_palette(prx, prz, seed, pack);

    let roll = hash::stable_hash(&[
        HashVal::Str("palette_local"),
        HashVal::Int(rx),
        HashVal::Int(rz),
        HashVal::Int(prx),
        HashVal::Int(prz),
        HashVal::Int(seed),
    ]) % 100;

    let c = &pack.grammar_constants;
    if roll < c.palette_primary_pct {
        return primary;
    }
    if roll < c.palette_primary_pct + c.palette_compatible_pct {
        // compatible neighbor of the primary palette, if any
        let primary_id = &pack.palettes[primary].id;
        if let Some(compat) = pack.compatibility.get(primary_id) {
            if !compat.is_empty() {
                let pick = hash::stable_hash(&[
                    HashVal::Str("palette_compatible"),
                    HashVal::Int(rx),
                    HashVal::Int(rz),
                    HashVal::Int(seed),
                ]) as usize
                    % compat.len();
                if let Some(idx) = pack.palette_index(&compat[pick]) {
                    return idx;
                }
            }
        }
        return primary; // no compatible defined -> fall back to primary
    }
    // rare: any palette
    hash::stable_hash(&[
        HashVal::Str("palette_rare"),
        HashVal::Int(rx),
        HashVal::Int(rz),
        HashVal::Int(seed),
    ]) as usize
        % pack.palettes.len()
}
