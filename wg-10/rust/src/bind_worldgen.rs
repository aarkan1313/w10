use godot::prelude::*;
use crate::hash;
use crate::grammar;
use crate::pack;
use crate::height;
use std::path::Path;

/// Thin Godot-facing wrapper over the engine-agnostic `hash` module. The only
/// file in the crate that imports `godot`. No math lives here.
#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10Hash {
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10Hash {
    fn init(base: Base<RefCounted>) -> Self {
        Self { base }
    }
}

#[godot_api]
impl Wg10Hash {
    #[func]
    fn stable_hash_ints(&self, prefix: GString, values: PackedInt64Array) -> i64 {
        let mut vals: Vec<hash::HashVal> = Vec::with_capacity(values.len() + 1);
        let p = prefix.to_string();
        vals.push(hash::HashVal::Str(&p));
        for v in values.as_slice() {
            vals.push(hash::HashVal::Int(*v));
        }
        hash::stable_hash(&vals) as i64
    }

    #[func]
    fn hash_grid(&self, ix: i64, iz: i64, seed: i64, salt: i64) -> f64 {
        hash::hash_grid(ix, iz, seed, salt)
    }

    #[func]
    fn value_noise(&self, x: f64, z: f64, scale_m: f64, seed: i64, salt: i64) -> f64 {
        hash::value_noise(x, z, scale_m, seed, salt)
    }

    #[func]
    fn fbm(&self, x: f64, z: f64, scale_m: f64, seed: i64, octaves: i64) -> f64 {
        hash::fbm(x, z, scale_m, seed, octaves.max(1) as u32)
    }
}

/// Thin Godot-facing wrapper over the engine-agnostic grammar. Loads a pack and
/// answers family-weight queries for headless checks. No math lives here.
#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10Grammar {
    pack: Option<pack::Pack>,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10Grammar {
    fn init(base: Base<RefCounted>) -> Self {
        Self { pack: None, base }
    }
}

#[godot_api]
impl Wg10Grammar {
    /// Load + validate a pack from a JSON string. Returns "" on success or the
    /// error message on failure (so GDScript can assert on it).
    #[func]
    fn load_pack_json(&mut self, json: GString) -> GString {
        match pack::load_pack_str(&json.to_string()) {
            Ok(p) => {
                self.pack = Some(p);
                GString::new()
            }
            Err(e) => GString::from(&e),
        }
    }

    /// Family ids present in the blend at (x,z). Parallel to `weight_values`.
    /// Empty if no pack loaded.
    #[func]
    fn family_ids(&self, x: f64, z: f64, seed: i64) -> PackedInt64Array {
        let mut ids = PackedInt64Array::new();
        if let Some(p) = &self.pack {
            for (fam, _weight) in grammar::family_weights(x, z, seed, p).entries() {
                ids.push(*fam as i64);
            }
        }
        ids
    }

    /// Blend weights at (x,z), parallel to `family_ids` (same order/length).
    /// Empty if no pack loaded.
    #[func]
    fn weight_values(&self, x: f64, z: f64, seed: i64) -> PackedFloat64Array {
        let mut weights = PackedFloat64Array::new();
        if let Some(p) = &self.pack {
            for (_fam, weight) in grammar::family_weights(x, z, seed, p).entries() {
                weights.push(*weight);
            }
        }
        weights
    }
}

/// Thin Godot-facing wrapper over the engine-agnostic height layer. Loads a pack
/// (resolving kernel .npy files relative to a base dir) and answers height
/// queries for headless checks. No math lives here.
#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10Height {
    pack: Option<pack::Pack>,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10Height {
    fn init(base: Base<RefCounted>) -> Self {
        Self { pack: None, base }
    }
}

#[godot_api]
impl Wg10Height {
    /// Load + validate a pack from a filesystem directory + file name, resolving
    /// kernel .npy files relative to that directory. Returns "" on success or the
    /// error message. `dir` is an absolute or cwd-relative OS path (GDScript
    /// resolves `res://` to an OS path via ProjectSettings.globalize_path).
    #[func]
    fn load_pack_dir(&mut self, dir: GString, file: GString) -> GString {
        match pack::load_pack_dir(Path::new(&dir.to_string()), &file.to_string()) {
            Ok(p) => {
                self.pack = Some(p);
                GString::new()
            }
            Err(e) => GString::from(&e),
        }
    }

    /// Elevation at (x,z). Returns 0.0 if no pack is loaded.
    #[func]
    fn height(&self, x: f64, z: f64, seed: i64) -> f64 {
        match &self.pack {
            Some(p) => height::height(x, z, seed, p),
            None => 0.0,
        }
    }

    /// CPU family-selection signature at (x,z) — matches the GPU's family_sig.
    #[func]
    fn family_signature(&self, x: f64, z: f64, seed: i64) -> i64 {
        match &self.pack {
            Some(p) => crate::parity::family_signature(x, z, seed, p) as i64,
            None => 0,
        }
    }
}
