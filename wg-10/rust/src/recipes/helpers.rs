//! Shared recipe helpers reused by biome recipe ports.

use crate::array_ops;
use crate::recipe_noise;

/// scipy's default gaussian truncate. All seam-safe blurs use it.
pub const TRUNCATE: f64 = 4.0;

/// Reference spacing (metres/pixel) for world-anchored blur sigmas. MUST equal the Python S_REF
/// (mountain_synthesis.py). sigma_cells(sc, spacing) = sc*S_REF/max(spacing,1e-6) -> a blur covers
/// the same WORLD distance at any spacing -> macro structure identical across clipmap levels.
pub const S_REF: f64 = 32.0;

/// World-anchored blur sigma in CELLS. `sigma_cell_ref` is the reference cell sigma (the sigma at
/// `S_REF` metres/pixel); rescaling by `S_REF / spacing_m` keeps the blur covering the same world
/// distance at any spacing. At `spacing_m == S_REF` this is the identity, reproducing the
/// pre-scale-invariant recipe byte-for-byte.
#[inline]
pub fn sigma_cells(sigma_cell_ref: f64, spacing_m: f64) -> f64 {
    (sigma_cell_ref * S_REF) / spacing_m.max(1e-6)
}

/// Data-independent affine remap: `(field - center) * scale`.
/// Mirror of `seam_safe.affine_remap`. The seam-safe replacement for per-window
/// zscore / norm01: identical transform for every window keeps borders bit-exact.
#[inline]
pub fn affine_remap(v: f64, center: f64, scale: f64) -> f64 {
    (v - center) * scale
}

/// In-place affine remap over a whole field.
pub fn affine_remap_field(field: &mut [f64], center: f64, scale: f64) {
    for v in field.iter_mut() {
        *v = affine_remap(*v, center, scale);
    }
}

/// Hermite smoothstep with the Python's `+ 1e-9` denominator guard.
/// Mirror of `mountain_synthesis.smoothstep`:
/// `t = clip((x-e0)/(e1-e0+1e-9), 0, 1); t*t*(3-2t)`.
#[inline]
pub fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0 + 1e-9)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `clip(v, lo, hi)` matching numpy's `np.clip`.
#[inline]
pub fn clip(v: f64, lo: f64, hi: f64) -> f64 {
    v.clamp(lo, hi)
}

/// Rotate a single `(wx, wz)` about a fixed world centre `(cx, cz)` by `angle_rad`.
/// Mirror of `mountain_synthesis._rotated` with EXPLICIT centre (seam-safe: in the
/// apron path the Python passes cx=cz=0.0, never the data-dependent window midpoint).
///
/// `x = wx - cx; z = wz - cz; (c*x + s*z, -s*x + c*z)`.
#[inline]
pub fn rotated(wx: f64, wz: f64, angle_rad: f64, cx: f64, cz: f64) -> (f64, f64) {
    let x = wx - cx;
    let z = wz - cz;
    let c = angle_rad.cos();
    let s = angle_rad.sin();
    (c * x + s * z, -s * x + c * z)
}

/// Seam-safe CONNECTED-drainage discharge field. Mirror of
/// `mountain_synthesis._flow_channels_seam_safe(surface, width_px, mode='nearest', power)`:
///
/// 1. pre-blur `surface` with gaussian sigma=sigma_cells(1.15, spacing_m) (nearest),
/// 2. real MFD flow accumulation (`array_ops::flow_accumulation_mfd`, given `power`),
/// 3. FIXED-max normalize: `clip(log1p(acc) / log1p(acc.size), 0, 1)` (data-independent),
/// 4. spread with gaussian sigma=sigma_cells(max(width_px, 0.1), spacing_m) (nearest), clip [0, 1].
///
/// `spacing_m` world-anchors both blur sigmas (pass `S_REF` for the reference-level identity).
/// Reused verbatim by every biome that carves channels.
pub fn flow_channels_seam_safe(
    surface: &[f64],
    rows: usize,
    cols: usize,
    width_px: f64,
    power: f64,
    spacing_m: f64,
) -> Vec<f64> {
    let pre =
        array_ops::gaussian_filter_nearest(surface, rows, cols, sigma_cells(1.15, spacing_m), TRUNCATE);
    let acc = array_ops::flow_accumulation_mfd(&pre, rows, cols, power);
    // log1p(acc.size): acc.size is the element count (rows*cols), matching numpy.
    let log_size = ((rows * cols) as f64).ln_1p();
    let mut discharge: Vec<f64> = acc
        .iter()
        .map(|&a| clip(a.ln_1p() / log_size, 0.0, 1.0))
        .collect();
    let sigma = sigma_cells(width_px.max(0.1), spacing_m);
    discharge = array_ops::gaussian_filter_nearest(&discharge, rows, cols, sigma, TRUNCATE);
    for v in discharge.iter_mut() {
        *v = clip(*v, 0.0, 1.0);
    }
    discharge
}

/// Build an apron-padded world-coordinate meshgrid, identical to the fixture's
/// Python construction: `xs[c] = (c - apron_px)*spacing + ox`,
/// `zs[r] = (r - apron_px)*spacing + oz`, then `wx[r][c]=xs[c]`, `wz[r][c]=zs[r]`.
/// Returns `(wx, wz)` as flat row-major vectors of length `rows*cols`.
pub fn apron_meshgrid(
    rows: usize,
    cols: usize,
    apron_px: usize,
    spacing: f64,
    ox: f64,
    oz: f64,
) -> (Vec<f64>, Vec<f64>) {
    let a = apron_px as f64;
    let xs: Vec<f64> = (0..cols).map(|c| (c as f64 - a) * spacing + ox).collect();
    let zs: Vec<f64> = (0..rows).map(|r| (r as f64 - a) * spacing + oz).collect();
    let mut wx = vec![0.0_f64; rows * cols];
    let mut wz = vec![0.0_f64; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            wx[r * cols + c] = xs[c];
            wz[r * cols + c] = zs[r];
        }
    }
    (wx, wz)
}

/// Re-export the per-point recursive domain warp at the recipe call's exact arity.
/// (Thin pass-through so per-biome code reads close to the Python.)
#[allow(clippy::too_many_arguments)]
#[inline]
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
    recipe_noise::recursive_domain_warp(wx, wz, warp_amount, warp_freq, seed, steps, decay, freq_mul)
}

/// `fbm` with the Python recipe default gain/lacunarity made explicit at call sites.
#[inline]
pub fn fbm(wx: f64, wz: f64, base_freq: f64, octaves: u32, seed: i64, gain: f64) -> f64 {
    recipe_noise::fbm(wx, wz, base_freq, octaves, seed, gain, 2.0)
}

/// `ridged_multifractal` with the recipe defaults (offset=1.0, weight_gain=1.35).
#[inline]
pub fn ridged_multifractal(
    wx: f64,
    wz: f64,
    base_freq: f64,
    octaves: u32,
    seed: i64,
    gain: f64,
) -> f64 {
    recipe_noise::ridged_multifractal(wx, wz, base_freq, octaves, seed, gain, 2.0, 1.0, 1.35)
}
