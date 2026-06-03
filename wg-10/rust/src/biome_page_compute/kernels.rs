//! CPU-side kernel and apron geometry helpers for biome page compute.
//!
//! These helpers are pure: they have no Godot/RD ownership and no scheduling side effects. They
//! exist to keep the shader/runtime facade focused on dispatch and resource lifetime.

// ---------------------------------------------------------------------------
// CPU gaussian kernel: port of array_ops::gaussian_kernel1d. The GLSL gaussian passes
// use this uploaded kernel; it MUST match the Rust oracle bit-for-bit (radius / truncate
// / phi / normalization), or Tier-2 height parity drifts.
// ---------------------------------------------------------------------------

/// scipy `_gaussian_kernel1d(sigma, order=0, radius=lw)`: normalized half-width-`lw`
/// Gaussian taps indexed `0..=2*lw` (offsets `-lw..=lw`). Port of array_ops::gaussian_kernel1d.
/// `lw = int(truncate*sigma + 0.5)` (truncation toward zero); `phi[x]=exp(-0.5/sigma^2 * x^2)`;
/// normalized so sum == 1. Computed in f64 then narrowed to f32 for upload.
pub(crate) fn gaussian_kernel1d(sigma: f64, truncate: f64) -> Vec<f32> {
    let lw_i: i64 = (truncate * sigma + 0.5) as i64; // int(...) truncates toward zero
    let lw = lw_i.max(0) as usize;
    let sigma2 = sigma * sigma;
    let size = 2 * lw + 1;
    let mut phi = Vec::with_capacity(size);
    let mut sum = 0.0_f64;
    for k in 0..size {
        let x = (k as i64 - lw as i64) as f64;
        let v = (-0.5 / sigma2 * x * x).exp();
        phi.push(v);
        sum += v;
    }
    phi.iter().map(|&v| (v / sum) as f32).collect()
}

/// Kernel half-width `lw` for a given sigma/truncate (kernel length = 2*lw+1). Mirror of
/// array_ops radius `int(truncate*sigma + 0.5)` (clamped >= 0).
pub(crate) fn gaussian_radius(sigma: f64, truncate: f64) -> usize {
    let lw_i: i64 = (truncate * sigma + 0.5) as i64;
    lw_i.max(0) as usize
}

/// Working-grid (padded) dim helper: core + an apron on each side.
pub(crate) fn apron_dim(core_px: usize, apron_px: usize) -> usize {
    core_px + 2 * apron_px
}

/// Apron working-grid dim for one page: core + an apron on EACH side (mountain: 256 + 2*160 = 576).
/// The runtime producer (`build_biome_page_context` / `compute_biome_page_cached`) sizes every
/// field/pool buffer to `biome_apron_dim^2`; the GPU rebuilds the padded meshgrid from this dim.
/// (Same value as `apron_dim`; named for the runtime-producer call sites + its unit tests.)
pub(crate) fn biome_apron_dim(core_px: usize, apron_px: usize) -> usize {
    core_px + 2 * apron_px
}

/// Map a CORE (row, col) to its position on the apron working grid (offset by `apron_px` on each
/// axis). PASS_CROP / PASS_CROP_IMG read `height[(r+apron)*cols + (c+apron)]` for core cell (r,c);
/// this is the pure index-geometry the crop relies on, pinned by a unit test so the apron offset
/// can never silently drift.
pub(crate) fn core_to_apron_index(r: usize, c: usize, apron_px: usize) -> (usize, usize) {
    (r + apron_px, c + apron_px)
}

/// Fixed packed-kernel slot width. Each distinct gaussian sigma occupies
/// `slot * KERNEL_STRIDE..slot * KERNEL_STRIDE + kernel_len`.
pub(crate) const KERNEL_STRIDE: usize = 64;
