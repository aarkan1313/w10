//! Bit-exact (within f64) Rust port of the offline noise primitives in
//! `tools/dem_pack/worldgen_proto.py` — the basis the 11 seam-safe biome recipes are
//! built on. This is the PARITY ORACLE for the whole biome port: every function here
//! reproduces worldgen_proto's exact f64 math, verified against committed Python
//! fixtures (`recipe_noise_tests.rs`).
//!
//! IMPORTANT: this is a DIFFERENT hash/noise than `hash.rs` (the WG9 port). Do not mix
//! them. The two divergences that matter vs hash.rs are baked in below:
//!   * the 31-bit mask `0x7fffffff` (not a full u32), and
//!   * the seed mix `seed * 362437` added per call (not a salt).
//!
//! The Python primitives run on numpy int64 arrays with WRAPPING integer arithmetic;
//! that is reproduced here with `wrapping_*` on `i64` and arithmetic `>>` on signed i64.
//! Per-point pure f(x,z): every function is local and deterministic (seam-safe).
//!
//! NOT ported: `flow_accumulation_channels` — worldgen_proto's own docstring calls it
//! "intentionally not a cheap local per-page operator" (whole-grid argsort + steepest-
//! descent flow). It is not part of the seam-safe local f(x,z) path, so it is out of
//! scope for this oracle.

const MASK_31: i64 = 0x7fff_ffff;

/// Integer lattice hash -> [0,1). Mirror of worldgen_proto `_hash2`.
///
/// `h = ix*374761393 + iz*668265263 + seed*362437` (wrapping i64); then
/// `h = (h ^ (h >> 13)) * 1274126177` (wrapping i64); then `h & 0x7fffffff`;
/// finally `h / 0x7fffffff`. The `>> 13` is an arithmetic (sign-preserving) shift,
/// matching numpy int64 `>>`.
#[inline]
fn hash2(ix: i64, iz: i64, seed: i64) -> f64 {
    let mut h = ix
        .wrapping_mul(374_761_393)
        .wrapping_add(iz.wrapping_mul(668_265_263))
        .wrapping_add(seed.wrapping_mul(362_437));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h &= MASK_31;
    (h as f64) / (MASK_31 as f64)
}

/// Quintic smootherstep (C2): `t*t*t*(t*(t*6-15)+10)`.
#[inline]
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Value noise on world-coord grids (cell size = 1 world unit at given coords) -> [-1,1].
/// Mirror of `value_noise(wx, wz, seed)`. NOTE: takes RAW coords (callers pre-multiply
/// by frequency), so there is no scale parameter — exactly matching worldgen_proto.
pub fn value_noise(wx: f64, wz: f64, seed: i64) -> f64 {
    let x0 = wx.floor() as i64;
    let z0 = wz.floor() as i64;
    let tx = fade(wx - x0 as f64);
    let tz = fade(wz - z0 as f64);
    let c00 = hash2(x0, z0, seed);
    let c10 = hash2(x0 + 1, z0, seed);
    let c01 = hash2(x0, z0 + 1, seed);
    let c11 = hash2(x0 + 1, z0 + 1, seed);
    let top = c00 + (c10 - c00) * tx;
    let bot = c01 + (c11 - c01) * tx;
    (top + (bot - top) * tz) * 2.0 - 1.0
}

/// Multi-octave value-noise fBm, normalized to ~[-1,1]. Mirror of
/// `fbm(wx, wz, base_freq, octaves, seed, gain, lacunarity)`.
/// Per-octave seed is `seed + i` (NOT a salt).
pub fn fbm(
    wx: f64,
    wz: f64,
    base_freq: f64,
    octaves: u32,
    seed: i64,
    gain: f64,
    lacunarity: f64,
) -> f64 {
    let mut h = 0.0_f64;
    let mut amp = 1.0_f64;
    let mut norm = 0.0_f64;
    let mut freq = base_freq;
    for i in 0..octaves {
        h += amp * value_noise(wx * freq, wz * freq, seed + i as i64);
        norm += amp;
        amp *= gain;
        freq *= lacunarity;
    }
    h / norm.max(1e-9)
}

/// Ridged fBm: each octave = `1 - |value_noise|`, summed+normalized -> [0,1].
/// Mirror of `ridged_fbm(...)`.
pub fn ridged_fbm(
    wx: f64,
    wz: f64,
    base_freq: f64,
    octaves: u32,
    seed: i64,
    gain: f64,
    lacunarity: f64,
) -> f64 {
    let mut h = 0.0_f64;
    let mut amp = 1.0_f64;
    let mut norm = 0.0_f64;
    let mut freq = base_freq;
    for i in 0..octaves {
        let n = 1.0 - value_noise(wx * freq, wz * freq, seed + i as i64).abs();
        h += amp * n;
        norm += amp;
        amp *= gain;
        freq *= lacunarity;
    }
    h / norm.max(1e-9)
}

/// Musgrave-style ridged multifractal -> [0,1]. Mirror of
/// `ridged_multifractal(..., offset=1.0, weight_gain=1.35)`.
///
/// Each octave: `signal = offset - |value_noise|`; clamp to >=0; square; multiply by the
/// running `weight`; accumulate `amp*signal`; then `weight = clamp(signal*weight_gain, 0, 1)`.
/// Final result is `clamp(h / max(norm, 1e-9), 0, 1)`.
#[allow(clippy::too_many_arguments)]
pub fn ridged_multifractal(
    wx: f64,
    wz: f64,
    base_freq: f64,
    octaves: u32,
    seed: i64,
    gain: f64,
    lacunarity: f64,
    offset: f64,
    weight_gain: f64,
) -> f64 {
    let mut h = 0.0_f64;
    let mut weight = 1.0_f64;
    let mut amp = 1.0_f64;
    let mut norm = 0.0_f64;
    let mut freq = base_freq;
    for i in 0..octaves {
        let mut signal = offset - value_noise(wx * freq, wz * freq, seed + i as i64).abs();
        // np.clip(signal, 0.0, None): lower bound only.
        if signal < 0.0 {
            signal = 0.0;
        }
        signal = signal * signal;
        signal *= weight;
        h += amp * signal;
        norm += amp;
        // weight = np.clip(signal * weight_gain, 0.0, 1.0)
        weight = (signal * weight_gain).clamp(0.0, 1.0);
        amp *= gain;
        freq *= lacunarity;
    }
    (h / norm.max(1e-9)).clamp(0.0, 1.0)
}

/// Single-pass domain warp. Mirror of `domain_warp(wx, wz, warp_amount, warp_freq, seed)`.
/// `warp_amount == 0` is a no-op. Warp vector is two 3-octave fbm fields at `seed+17`/`seed+43`,
/// with fbm's default gain=0.5, lacunarity=2.0.
pub fn domain_warp(wx: f64, wz: f64, warp_amount: f64, warp_freq: f64, seed: i64) -> (f64, f64) {
    if warp_amount == 0.0 {
        return (wx, wz);
    }
    let dx = fbm(wx, wz, warp_freq, 3, seed + 17, 0.5, 2.0);
    let dz = fbm(wx, wz, warp_freq, 3, seed + 43, 0.5, 2.0);
    (wx + warp_amount * dx, wz + warp_amount * dz)
}

/// Recursive domain warp. Mirror of
/// `recursive_domain_warp(wx, wz, warp_amount, warp_freq, seed, steps=3, decay=0.55, freq_mul=1.9)`.
///
/// `warp_amount == 0` or `steps <= 0` is a no-op. Each step:
///   `dx = fbm(out_x, out_z, freq, 3, seed+101+i*37)`,
///   `dz = fbm(out_x, out_z, freq, 3, seed+151+i*37)`,
///   `out_x += amount*dx; out_z += amount*dz`,
/// THEN update `amount *= decay; freq *= freq_mul` (order matters: the update is applied
/// AFTER the displacement, so the first step uses the unscaled amount/freq).
#[allow(clippy::too_many_arguments)]
pub fn recursive_domain_warp(
    wx: f64,
    wz: f64,
    warp_amount: f64,
    warp_freq: f64,
    seed: i64,
    steps: u32,
    decay: f64,
    freq_mul: f64,
) -> (f64, f64) {
    if warp_amount == 0.0 || steps == 0 {
        return (wx, wz);
    }
    let mut out_x = wx;
    let mut out_z = wz;
    let mut amount = warp_amount;
    let mut freq = warp_freq;
    for i in 0..steps as i64 {
        let dx = fbm(out_x, out_z, freq, 3, seed + 101 + i * 37, 0.5, 2.0);
        let dz = fbm(out_x, out_z, freq, 3, seed + 151 + i * 37, 0.5, 2.0);
        out_x += amount * dx;
        out_z += amount * dz;
        amount *= decay;
        freq *= freq_mul;
    }
    (out_x, out_z)
}

/// Cheap Worley/cellular edge network -> [0,1], high near cell borders.
/// Mirror of `cellular_edges(wx, wz, freq, seed, sharpness=2.0)`.
///
/// f1/f2 nearest two feature distances over the 3x3 neighborhood; feature offset uses
/// `_hash2(cx, cz, seed+11)`/`seed+29`. `gap = sqrt(f2) - sqrt(f1)`;
/// return `1 - clamp(gap*sharpness, 0, 1)`.
pub fn cellular_edges(wx: f64, wz: f64, freq: f64, seed: i64, sharpness: f64) -> f64 {
    let x = wx * freq;
    let z = wz * freq;
    let ix = x.floor() as i64;
    let iz = z.floor() as i64;
    let fx = x - ix as f64;
    let fz = z - iz as f64;
    let mut f1 = f64::INFINITY;
    let mut f2 = f64::INFINITY;
    for dz in [-1_i64, 0, 1] {
        for dx in [-1_i64, 0, 1] {
            let cx = ix + dx;
            let cz = iz + dz;
            let px = dx as f64 + hash2(cx, cz, seed + 11);
            let pz = dz as f64 + hash2(cx, cz, seed + 29);
            let d2 = (px - fx) * (px - fx) + (pz - fz) * (pz - fz);
            // old_f1 = f1; f1 = min(f1, d2); f2 = min(max(old_f1, d2), f2)
            let old_f1 = f1;
            f1 = f1.min(d2);
            f2 = old_f1.max(d2).min(f2);
        }
    }
    let gap = f2.sqrt() - f1.sqrt();
    1.0 - (gap * sharpness).clamp(0.0, 1.0)
}

/// World-anchored procedural range spines -> [0,1], high near long deterministic segments.
/// Mirror of `range_spine_field(wx, wz, cell_size, width, seed, neighborhood)`.
pub fn range_spine_field(
    wx: f64,
    wz: f64,
    cell_size: f64,
    width: f64,
    seed: i64,
    neighborhood: i32,
) -> f64 {
    let gx = (wx / cell_size).floor() as i64;
    let gz = (wz / cell_size).floor() as i64;
    let mut out = 0.0_f64;
    let pi = std::f64::consts::PI;
    for dz in -neighborhood..=neighborhood {
        for dx in -neighborhood..=neighborhood {
            let cx = gx + dx as i64;
            let cz = gz + dz as i64;
            let jitter_x = (hash2(cx, cz, seed + 1) - 0.5) * 0.65;
            let jitter_z = (hash2(cx, cz, seed + 2) - 0.5) * 0.65;
            let center_x = (cx as f64 + 0.5 + jitter_x) * cell_size;
            let center_z = (cz as f64 + 0.5 + jitter_z) * cell_size;
            let angle = hash2(cx, cz, seed + 3) * pi * 2.0;
            let length = cell_size * (1.15 + 0.75 * hash2(cx, cz, seed + 4));
            let vx = angle.cos() * length;
            let vz = angle.sin() * length;
            let x0 = center_x - vx * 0.5;
            let z0 = center_z - vz * 0.5;
            let denom = vx * vx + vz * vz + 1e-9;
            let t = (((wx - x0) * vx + (wz - z0) * vz) / denom).clamp(0.0, 1.0);
            let px = x0 + t * vx;
            let pz = z0 + t * vz;
            let d = ((wx - px) * (wx - px) + (wz - pz) * (wz - pz)).sqrt();
            out = out.max((-((d / width).powi(2))).exp());
        }
    }
    out.clamp(0.0, 1.0)
}

/// Broad signed fault bands -> [-1,1]. Mirror of
/// `fault_block_field(wx, wz, cell_size, width, seed, neighborhood)`.
pub fn fault_block_field(
    wx: f64,
    wz: f64,
    cell_size: f64,
    width: f64,
    seed: i64,
    neighborhood: i32,
) -> f64 {
    let gx = (wx / cell_size).floor() as i64;
    let gz = (wz / cell_size).floor() as i64;
    let mut out = 0.0_f64;
    let mut norm = 0.0_f64;
    for dz in -neighborhood..=neighborhood {
        for dx in -neighborhood..=neighborhood {
            let cx = gx + dx as i64;
            let cz = gz + dz as i64;
            let center_x =
                (cx as f64 + 0.5 + (hash2(cx, cz, seed + 10) - 0.5) * 0.45) * cell_size;
            let center_z =
                (cz as f64 + 0.5 + (hash2(cx, cz, seed + 11) - 0.5) * 0.45) * cell_size;
            let angle = hash2(cx, cz, seed + 12) * std::f64::consts::PI * 2.0;
            let nx = -angle.sin();
            let nz = angle.cos();
            let signed = (wx - center_x) * nx + (wz - center_z) * nz;
            let amp = hash2(cx, cz, seed + 13) * 2.0 - 1.0;
            let influence = (-((signed / (cell_size * 0.55)).powi(2))).exp();
            out += amp * (signed / width).tanh() * influence;
            norm += 1.0;
        }
    }
    (out / (norm * 0.22).max(1e-9)).clamp(-1.0, 1.0)
}
