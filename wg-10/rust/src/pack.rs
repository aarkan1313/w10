//! Terrain-pack format v1 loader + validation. The ONLY JSON-parsing file in
//! the crate (DESIGN §3 / design §2 constraint #3). Pure: no `godot` imports.
//! The grammar reads the in-memory `Pack`; it never sees JSON.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Every palette has exactly this many families. Fixed arity keeps the grammar
/// blend bounded and GPU-shaped (design §2 constraint #1).
pub const FAMILIES_PER_PALETTE: usize = 3;

const PACK_SCHEMA: &str = "worldgen10.terrain_pack.v1";

#[derive(Debug, Deserialize)]
pub struct GrammarConstants {
    pub region_size_m: f64,
    pub province_size_regions: i64,
    pub palette_primary_pct: u32,
    pub palette_compatible_pct: u32,
}

#[derive(Debug, Deserialize)]
pub struct Palette {
    pub id: String,
    pub families: Vec<String>,
}

/// Raw shape as it appears on disk (serde mirror). Validated into `Pack`.
#[derive(Debug, Deserialize)]
struct RawPack {
    schema: String,
    version: u32,
    grammar_constants: GrammarConstants,
    palettes: Vec<Palette>,
    compatibility: BTreeMap<String, Vec<String>>,
    families: BTreeMap<String, serde_json::Value>,
}

/// Validated, in-memory pack. This is the stable interface the grammar reads.
#[derive(Debug)]
pub struct Pack {
    pub grammar_constants: GrammarConstants,
    pub palettes: Vec<Palette>,
    pub compatibility: BTreeMap<String, Vec<String>>,
    pub family_ids: Vec<String>,
}

impl Pack {
    /// Index of a palette by id, or None.
    pub fn palette_index(&self, id: &str) -> Option<usize> {
        self.palettes.iter().position(|p| p.id == id)
    }
}

/// Load + validate a pack from a JSON string. Returns a descriptive error
/// string on any malformed/invalid input (DESIGN §7 no-shortcuts: reject, never
/// silently default).
pub fn load_pack_str(json: &str) -> Result<Pack, String> {
    let raw: RawPack = serde_json::from_str(json).map_err(|e| format!("pack parse error: {e}"))?;

    if raw.schema != PACK_SCHEMA {
        return Err(format!("bad schema: got {:?}, expected {:?}", raw.schema, PACK_SCHEMA));
    }
    if raw.version != 1 {
        return Err(format!("unsupported pack version: {}", raw.version));
    }

    let c = &raw.grammar_constants;
    if c.region_size_m <= 0.0 {
        return Err(format!("region_size_m must be > 0, got {}", c.region_size_m));
    }
    if c.province_size_regions <= 0 {
        return Err(format!("province_size_regions must be > 0, got {}", c.province_size_regions));
    }
    if c.palette_primary_pct as u64 + c.palette_compatible_pct as u64 > 100 {
        return Err(format!(
            "palette pct out of range: primary {} + compatible {} > 100",
            c.palette_primary_pct, c.palette_compatible_pct
        ));
    }

    // Each palette must have exactly FAMILIES_PER_PALETTE families, and every
    // referenced family must exist in families{}.
    for pal in &raw.palettes {
        if pal.families.len() != FAMILIES_PER_PALETTE {
            return Err(format!(
                "palette {:?} must have exactly {} families, got {}",
                pal.id, FAMILIES_PER_PALETTE, pal.families.len()
            ));
        }
        for fam in &pal.families {
            if !raw.families.contains_key(fam) {
                return Err(format!("palette {:?} references unknown family {:?}", pal.id, fam));
            }
        }
    }

    let mut family_ids: Vec<String> = raw.families.keys().cloned().collect();
    family_ids.sort(); // deterministic order (BTreeMap is already sorted; explicit for clarity)

    Ok(Pack {
        grammar_constants: raw.grammar_constants,
        palettes: raw.palettes,
        compatibility: raw.compatibility,
        family_ids,
    })
}
