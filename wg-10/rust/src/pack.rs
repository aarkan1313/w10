//! Terrain-pack format v1 loader + validation. The ONLY JSON-parsing file in
//! the crate (DESIGN §3 / design §2 constraint #3). Pure: no `godot` imports.
//! The grammar reads the in-memory `Pack`; it never sees JSON.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::npy::{self, Kernel};

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
    #[serde(default = "default_moderation_min")]
    pub moderation_min: f64,
    #[serde(default = "default_moderation_strength")]
    pub moderation_strength: f64,
}

fn default_moderation_min() -> f64 { 0.4 }
fn default_moderation_strength() -> f64 { 0.5 }

#[derive(Debug, Deserialize)]
pub struct Palette {
    pub id: String,
    pub families: Vec<String>,
}

/// Raw per-family kernel reference as it appears on disk. All fields optional so
/// a `{}` family (grammar-only) deserializes fine.
#[derive(Debug, Deserialize, Default)]
struct RawFamily {
    #[serde(default)]
    kernel: Option<String>,
    #[serde(default)]
    relief_m: Option<f64>,
    #[serde(default)]
    footprint_m: Option<f64>,
}

/// Resolved kernel data for one family: the loaded array + amplitude/footprint.
#[derive(Debug, Clone)]
pub struct FamilyKernel {
    pub kernel: Kernel,
    pub relief_m: f64,
    pub footprint_m: f64,
}

/// Raw shape as it appears on disk (serde mirror). Validated into `Pack`.
#[derive(Debug, Deserialize)]
struct RawPack {
    schema: String,
    version: u32,
    grammar_constants: GrammarConstants,
    palettes: Vec<Palette>,
    compatibility: BTreeMap<String, Vec<String>>,
    families: BTreeMap<String, RawFamily>,
}

/// Validated, in-memory pack. This is the stable interface the grammar reads.
#[derive(Debug)]
pub struct Pack {
    pub grammar_constants: GrammarConstants,
    pub palettes: Vec<Palette>,
    pub compatibility: BTreeMap<String, Vec<String>>,
    pub family_ids: Vec<String>,
    pub family_kernels: BTreeMap<String, FamilyKernel>,
}

impl Pack {
    /// Index of a palette by id, or None.
    pub fn palette_index(&self, id: &str) -> Option<usize> {
        self.palettes.iter().position(|p| p.id == id)
    }
    /// Resolved kernel for a family id, if it declared one.
    pub fn family_kernel(&self, family_id: &str) -> Option<&FamilyKernel> {
        self.family_kernels.get(family_id)
    }
}

/// Load + validate a pack from a JSON string WITHOUT resolving kernel files
/// (grammar-only path; a referenced kernel is recorded but not read). Used by
/// the grammar layer and its golden pack.
pub fn load_pack_str(json: &str) -> Result<Pack, String> {
    load_pack_impl(json, None)
}

/// Load + validate a pack and resolve kernel `.npy` files relative to `base`.
pub fn load_pack_with_base(json: &str, base: &Path) -> Result<Pack, String> {
    load_pack_impl(json, Some(base))
}

/// Load + validate a pack from `<dir>/<file>`, resolving kernels relative to `dir`.
pub fn load_pack_dir(dir: &Path, file: &str) -> Result<Pack, String> {
    let path = dir.join(file);
    let json = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read pack {path:?}: {e}"))?;
    load_pack_impl(&json, Some(dir))
}

fn load_pack_impl(json: &str, base: Option<&Path>) -> Result<Pack, String> {
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
    if !(0.0..=1.0).contains(&c.moderation_min) {
        return Err(format!("moderation_min must be in [0,1], got {}", c.moderation_min));
    }
    if c.moderation_strength < 0.0 {
        return Err(format!("moderation_strength must be >= 0, got {}", c.moderation_strength));
    }

    if raw.palettes.is_empty() {
        return Err("pack has no palettes".to_string());
    }

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

    // Resolve kernels for families that declare one (opt-in). Validation: a
    // family that declares ANY kernel field must declare all three and load.
    let mut family_kernels: BTreeMap<String, FamilyKernel> = BTreeMap::new();
    for (id, rf) in &raw.families {
        let declares = rf.kernel.is_some() || rf.relief_m.is_some() || rf.footprint_m.is_some();
        if !declares {
            continue; // {} family — grammar-only, no kernel.
        }
        let kernel_path = rf.kernel.as_ref()
            .ok_or_else(|| format!("family {id:?} has relief/footprint but no kernel path"))?;
        let relief_m = rf.relief_m
            .ok_or_else(|| format!("family {id:?} kernel missing relief_m"))?;
        let footprint_m = rf.footprint_m
            .ok_or_else(|| format!("family {id:?} kernel missing footprint_m"))?;
        if relief_m <= 0.0 {
            return Err(format!("family {id:?} relief_m must be > 0, got {relief_m}"));
        }
        if footprint_m <= 0.0 {
            return Err(format!("family {id:?} footprint_m must be > 0, got {footprint_m}"));
        }
        let base = base.ok_or_else(|| format!(
            "family {id:?} declares a kernel but pack loaded without a base dir (use load_pack_dir)"
        ))?;
        let full = base.join(kernel_path);
        let bytes = std::fs::read(&full)
            .map_err(|e| format!("family {id:?} kernel {kernel_path:?} unreadable: {e}"))?;
        let kernel = npy::read_npy_f32(&bytes)
            .map_err(|e| format!("family {id:?} kernel {kernel_path:?}: {e}"))?;
        family_kernels.insert(id.clone(), FamilyKernel { kernel, relief_m, footprint_m });
    }

    let mut family_ids: Vec<String> = raw.families.keys().cloned().collect();
    family_ids.sort();

    Ok(Pack {
        grammar_constants: raw.grammar_constants,
        palettes: raw.palettes,
        compatibility: raw.compatibility,
        family_ids,
        family_kernels,
    })
}
