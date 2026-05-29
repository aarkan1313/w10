//! Engine-agnostic deterministic hash/noise. No Godot imports (DESIGN §6.3).

const FNV1A_INITIAL: u32 = 0x811c_9dc5;
const FNV1A_MULTIPLY: u32 = 0x0100_0193;

/// FNV-1a over the UTF-8 code units of `text`. WG9 hashes per `unicode_at`
/// (code point). For the ASCII join strings used here, bytes == code points.
pub fn fnv1a_32(text: &str) -> u32 {
    let mut h = FNV1A_INITIAL;
    for cp in text.chars() {
        h ^= cp as u32;
        h = h.wrapping_mul(FNV1A_MULTIPLY);
    }
    h
}

/// A value that can appear in a stable_hash key, formatted exactly as WG9's
/// `_format_value` does (base-10 ints; whole floats render as the int).
pub enum HashVal<'a> {
    Int(i64),
    Float(f64),
    Str(&'a str),
}

fn format_val(v: &HashVal) -> String {
    match v {
        HashVal::Int(i) => i.to_string(),
        HashVal::Float(f) => {
            if (*f - f.round()).abs() < f64::EPSILON {
                (f.round() as i64).to_string()
            } else {
                // GDScript str(float) formatting differs; floats are not used
                // as hash keys in the ported paths. Guard against silent drift.
                f.to_string()
            }
        }
        HashVal::Str(s) => (*s).to_string(),
    }
}

pub fn stable_hash(values: &[HashVal]) -> u32 {
    let joined = values.iter().map(format_val).collect::<Vec<_>>().join("|");
    fnv1a_32(&joined)
}

const U32_MASK: u64 = 0xffff_ffff;
const U32_DENOM: f64 = 4294967295.0;

pub fn hash_grid(ix: i64, iz: i64, seed: i64, salt: i64) -> f64 {
    // Bit-exact port of WG9 `terrain_hash.gd::hash_grid`. GDScript ints are
    // int64, and the original masks to u32 ONLY at two points: after the
    // weighted sum, and after the final xor. The middle multiply by
    // 1274126177 runs at FULL 64-bit width (it is NOT masked before the
    // `>> 16` on the next line). Masking to u32 before that multiply — as a
    // naive port does — silently changes every output while still producing a
    // plausible 0..1 value, so it is locked here against the fixture.
    let n0: u64 = ((ix.wrapping_mul(374761393)
        + iz.wrapping_mul(668265263)
        + seed.wrapping_mul(1442695041)
        + salt.wrapping_mul(69069)) as u64)
        & U32_MASK;
    let n1: u64 = (n0 ^ (n0 >> 13)).wrapping_mul(1274126177); // full-width, no mask
    let n2: u64 = (n1 ^ (n1 >> 16)) & U32_MASK;
    n2 as f64 / U32_DENOM
}

pub fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

pub fn smoothstep_unit(t: f64) -> f64 {
    let v = t.clamp(0.0, 1.0);
    v * v * (3.0 - 2.0 * v)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

pub fn value_noise(x: f64, z: f64, scale_m: f64, seed: i64, salt: i64) -> f64 {
    let fx = x / scale_m;
    let fz = z / scale_m;
    let ix = fx.floor() as i64;
    let iz = fz.floor() as i64;
    let tx = fade(fx - ix as f64);
    let tz = fade(fz - iz as f64);
    let a = hash_grid(ix, iz, seed, salt);
    let b = hash_grid(ix + 1, iz, seed, salt);
    let c = hash_grid(ix, iz + 1, seed, salt);
    let d = hash_grid(ix + 1, iz + 1, seed, salt);
    let ab = lerp(a, b, tx);
    let cd = lerp(c, d, tx);
    lerp(ab, cd, tz) * 2.0 - 1.0
}

pub fn fbm(x: f64, z: f64, scale_m: f64, seed: i64, octaves: u32) -> f64 {
    let mut total = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    for octave in 0..octaves {
        let s = scale_m / (1u64 << octave) as f64;
        total += value_noise(x, z, s, seed, octave as i64) * amp;
        norm += amp;
        amp *= 0.5;
    }
    total / norm.max(0.000001)
}
