use godot::prelude::*;
use crate::hash;

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
