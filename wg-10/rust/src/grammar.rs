//! WorldGen10 region/province grammar. Pure deterministic math, no `godot`
//! imports (DESIGN §6.3). Turns a world coordinate into a bounded blend of
//! terrain families. WG9 is reference/sanity-oracle only — this is W10's design.

use crate::hash::{self, HashVal};
use crate::pack::{Pack, FAMILIES_PER_PALETTE};

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

/// A family identified by its index into `Pack::family_ids`.
pub type FamilyId = u32;

/// The narrow family-source seam (design §2 constraint #2). Given a region,
/// return its FAMILIES_PER_PALETTE family ids and a normalized bias split.
/// Palette-based implementation; M6's climate-field source replaces ONLY this
/// function — the blend math (Task 4) must not change.
pub fn families_for_region(
    rx: i64,
    rz: i64,
    seed: i64,
    pack: &Pack,
) -> ([FamilyId; FAMILIES_PER_PALETTE], [f64; FAMILIES_PER_PALETTE]) {
    let palette = &pack.palettes[palette_for_region(rx, rz, seed, pack)];

    // Map the palette's family names to global family-table indices.
    let mut fams = [0u32; FAMILIES_PER_PALETTE];
    for (i, name) in palette.families.iter().enumerate() {
        let idx = pack
            .family_ids
            .iter()
            .position(|f| f == name)
            .expect("validated pack: palette family exists in family_ids");
        fams[i] = idx as u32;
    }

    // Base bias split, rotated deterministically per region so adjacent regions
    // with the same palette don't all weight the same family first.
    let base = [0.55, 0.30, 0.15];
    let roll = (hash::stable_hash(&[
        HashVal::Str("family_roll"),
        HashVal::Int(rx),
        HashVal::Int(rz),
        HashVal::Int(seed),
    ]) % FAMILIES_PER_PALETTE as u32) as usize;
    let mut bias = [0.0f64; FAMILIES_PER_PALETTE];
    for i in 0..FAMILIES_PER_PALETTE {
        bias[i] = base[(i + roll) % FAMILIES_PER_PALETTE];
    }
    (fams, bias)
}

/// Max distinct families a blend can contain: 4 region corners x 3 families.
pub const MAX_FAMILY_WEIGHTS: usize = 4 * FAMILIES_PER_PALETTE;

/// The grammar's hot-path output: a bounded list of (family, weight) pairs that
/// sum to 1. Fixed-capacity buffer + length — no heap allocation, GPU-shaped
/// (design §2 constraint #1). `entries()` yields the live slice.
#[derive(Debug, Clone, PartialEq)]
pub struct FamilyWeights {
    buf: [(FamilyId, f64); MAX_FAMILY_WEIGHTS],
    len: usize,
}

impl FamilyWeights {
    fn new() -> Self {
        Self { buf: [(0, 0.0); MAX_FAMILY_WEIGHTS], len: 0 }
    }
    fn add(&mut self, fam: FamilyId, weight: f64) {
        for i in 0..self.len {
            if self.buf[i].0 == fam {
                self.buf[i].1 += weight;
                return;
            }
        }
        debug_assert!(self.len < MAX_FAMILY_WEIGHTS, "FamilyWeights buffer overflow");
        self.buf[self.len] = (fam, weight);
        self.len += 1;
    }
    pub fn entries(&self) -> &[(FamilyId, f64)] {
        &self.buf[..self.len]
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn iter(&self) -> std::slice::Iter<'_, (FamilyId, f64)> {
        self.entries().iter()
    }
}

/// Blend family weights at a world coordinate: 4 region corners, smoothstep
/// interpolation, fixed-capacity accumulation, normalized to sum 1.
pub fn family_weights(x: f64, z: f64, seed: i64, pack: &Pack) -> FamilyWeights {
    let s = pack.grammar_constants.region_size_m;
    let gx = x / s;
    let gz = z / s;
    let rx = gx.floor() as i64;
    let rz = gz.floor() as i64;
    let tx = hash::smoothstep_unit(gx - rx as f64);
    let tz = hash::smoothstep_unit(gz - rz as f64);

    let corners = [
        (rx, rz, (1.0 - tx) * (1.0 - tz)),
        (rx + 1, rz, tx * (1.0 - tz)),
        (rx, rz + 1, (1.0 - tx) * tz),
        (rx + 1, rz + 1, tx * tz),
    ];

    let mut out = FamilyWeights::new();
    for (crx, crz, corner_w) in corners {
        if corner_w == 0.0 {
            continue;
        }
        let (fams, bias) = families_for_region(crx, crz, seed, pack);
        for i in 0..FAMILIES_PER_PALETTE {
            out.add(fams[i], corner_w * bias[i]);
        }
    }

    // Normalize to sum 1 (corner weights already sum to 1, bias sums to 1, so
    // the total is ~1; normalize to kill float drift and guarantee the contract).
    let total: f64 = out.entries().iter().map(|(_, w)| *w).sum::<f64>().max(1e-12);
    for i in 0..out.len {
        out.buf[i].1 /= total;
    }
    out
}
