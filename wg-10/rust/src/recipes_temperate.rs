// WIRE: add mod recipes_temperate; + test mod
//! TEMPERATE biome recipe — seam-safe (apron_px > 0) path, ported bit-close (f64) from
//! `tools/dem_pack/temperate_synthesis.py::generate`. Follows the MOUNTAIN template
//! (`recipes.rs`) exactly: shared math comes from [`crate::recipes::helpers`]; only the
//! temperate-specific constants, sub-fields (folded ridges / hills / MFD valleys) and the
//! assembly pipeline live here.
//!
//! Parity contract: `temperate_seamsafe(...)` reproduces the Python core-cropped height
//! within a tight epsilon, verified against the committed fixture
//! `tools/dem_pack/fixtures/recipe_temperate_fixture.json` (`recipes_temperate_tests.rs`).
//!
//! Temperate-specific notes vs the mountain/grassland templates:
//!   * `recursive_domain_warp` uses freq_mul = 1.72 (mountain used 1.75, grassland 1.70) —
//!     inlined at the call site below.
//!   * The valley carve uses temperate's flow power 0.43 and CANNOT reuse
//!     `helpers::flow_channels_seam_safe` directly: Python's `_valley_channels_seam_safe`
//!     returns the RAW discharge field (pre-blur 1.15 -> MFD -> log1p/log1p(size) clip),
//!     WITHOUT any spread blur, then the caller applies TWO different spreads
//!     (sigma=1.8 for `valleys`, sigma=4.2 for `broad_valleys`). The helper bakes in a
//!     single spread, so we use a local `valley_discharge_seam_safe` variant that stops
//!     before the spread (steps 1-3 of the helper) and blur separately downstream.
//!   * `ridged_multifractal` uses the recipe defaults (offset=1.0, weight_gain=1.35), same
//!     as `helpers::ridged_multifractal` — no inline needed.
//!   * `folded` is computed on ROTATED coords with the anisotropic `rz * 0.22` scaling
//!     (NOTE the 0.22, like grassland's low_ripple `rz*0.34`).
//!   * The hills term re-enters height as `affine_remap(hills, 0.5, 2.0)` — a SECOND
//!     fixed remap of the already-remapped `hills` field (the seam-safe stand-in for the
//!     legacy `zscore(hills)`), distinct from the HILLS_CENTER/SCALE remap.
//!   * STYLES[0] = appalachian_ridges (seed_offset=0).
//!   * Flow tie-ordering: temperate valleys can be flat, so `flow_source` may contain
//!     exactly-equal cells; the MFD downhill test is strict (`drop > 0`), so tied cells
//!     never flow to each other and the Rust stable-sort vs numpy quicksort tie order
//!     changes the result by at most ~1e-16 (same caveat as `array_ops::flow_accumulation_mfd`).

#![allow(dead_code)]

use crate::recipes::helpers as h;
use crate::array_ops;

// ---------------------------------------------------------------------------
// Apron constant — TEMPERATE_APRON_PX. See Python docstring for the derivation.
// ---------------------------------------------------------------------------
pub const APRON_PX: usize = 160;

// ---------------------------------------------------------------------------
// Affine-remap constants (replace per-window zscore / norm01). Mirror of the
// TEMPERATE_*_CENTER / TEMPERATE_*_SCALE module constants in temperate_synthesis.py.
// ---------------------------------------------------------------------------
pub const MACRO_CENTER: f64 = -0.428;
pub const MACRO_SCALE: f64 = 1.061;

pub const FOLDED_CENTER: f64 = 0.004;
pub const FOLDED_SCALE: f64 = 1.085;

pub const HILLS_CENTER: f64 = 0.008;
pub const HILLS_SCALE: f64 = 1.339;

pub const FLOW_SRC_CENTER: f64 = 0.583;
pub const FLOW_SRC_SCALE: f64 = 3.895;

pub const FINE_CENTER: f64 = 0.000;
pub const FINE_SCALE: f64 = 3.436;

pub const ROUNDED_CENTER: f64 = 0.458;
pub const ROUNDED_SCALE: f64 = 6.390;

pub const FINAL_CENTER: f64 = 0.079;
pub const FINAL_SCALE: f64 = 1.995;

// MFD valley channel thresholds (seam-safe path).
pub const VALLEY_THRESH_LO: f64 = 0.24;
pub const VALLEY_THRESH_HI: f64 = 0.40;
pub const BROAD_VALLEY_THRESH_LO: f64 = 0.20;
pub const BROAD_VALLEY_THRESH_HI: f64 = 0.36;

/// Mirror of `TemperateStyle` (the fields the seam-safe pipeline reads).
#[derive(Clone, Copy, Debug)]
pub struct TemperateStyle {
    pub key: &'static str,
    pub angle_rad: f64,
    pub ridge_gain: f64,
    pub hill_gain: f64,
    pub valley_gain: f64,
    pub upland_gain: f64,
    pub smoothing_px: f64,
    pub texture_gain: f64,
    pub seed_offset: i64,
}

/// `STYLES[0]` — appalachian_ridges (the temperate reference style).
pub const APPALACHIAN_RIDGES: TemperateStyle = TemperateStyle {
    key: "appalachian_ridges",
    angle_rad: 0.78,
    ridge_gain: 1.55,
    hill_gain: 0.72,
    valley_gain: 1.12,
    upland_gain: 0.62,
    smoothing_px: 1.8,
    texture_gain: 0.58,
    seed_offset: 0,
};

/// Mirror of `_valley_channels_seam_safe(surface, mode='nearest', power=0.43)`.
///
/// Returns the RAW discharge field BEFORE any spread blur (the caller applies the two
/// temperate spreads separately):
///   1. pre-blur `surface` with gaussian sigma=1.15 (nearest),
///   2. real MFD flow accumulation (`array_ops::flow_accumulation_mfd`, given `power`),
///   3. FIXED-max normalize: `clip(log1p(acc) / log1p(acc.size), 0, 1)` (data-independent).
///
/// This is exactly steps 1-3 of `helpers::flow_channels_seam_safe` (it stops before the
/// helper's single spread blur, because temperate needs two different spreads downstream).
fn valley_discharge_seam_safe(surface: &[f64], rows: usize, cols: usize, power: f64) -> Vec<f64> {
    let pre = array_ops::gaussian_filter_nearest(surface, rows, cols, 1.15, h::TRUNCATE);
    let acc = array_ops::flow_accumulation_mfd(&pre, rows, cols, power);
    let log_size = ((rows * cols) as f64).ln_1p();
    acc.iter()
        .map(|&a| h::clip(a.ln_1p() / log_size, 0.0, 1.0))
        .collect()
}

/// Port of `generate(..., apron_px=APRON_PX)` SEAM-SAFE path, returning the CORE-cropped
/// height (length `(rows-2*apron_px)*(cols-2*apron_px)`).
///
/// `wx`/`wz` are the apron-padded world-coord grids (flat row-major, length `rows*cols`);
/// `rows`/`cols` are the PADDED dimensions. `feature_span_m` MUST be the fixed CORE span
/// shared by adjacent windows. `apron_px` cells are cropped off every side at the end.
#[allow(clippy::too_many_arguments)]
pub fn generate_seamsafe(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    seed: i64,
    style: &TemperateStyle,
    feature_span_m: f64,
    apron_px: usize,
) -> Vec<f64> {
    assert_eq!(wx.len(), rows * cols, "wx len != rows*cols");
    assert_eq!(wz.len(), rows * cols, "wz len != rows*cols");
    let n = rows * cols;
    let feature_span = feature_span_m.max(1.0);
    let sseed = seed + style.seed_offset;

    // --- pointwise: recursive domain warp (freq_mul=1.72), then macro / folded / hills_raw / fine ---
    // and capture the warped coords w_x/w_z for downstream sub-fields.
    let mut w_x = vec![0.0_f64; n];
    let mut w_z = vec![0.0_f64; n];
    let mut macro_f = vec![0.0_f64; n];
    let mut folded_remap = vec![0.0_f64; n]; // clip(affine_remap(folded, FOLDED), 0, 1) -- pre-blur input to ridges
    let mut hills_raw = vec![0.0_f64; n];
    let mut fine = vec![0.0_f64; n];
    for i in 0..n {
        let (wx_w, wz_w) = h::recursive_domain_warp(
            wx[i],
            wz[i],
            feature_span * 0.030,
            1.0 / (feature_span * 0.76),
            sseed + 10,
            3,
            0.55,
            1.72,
        );
        w_x[i] = wx_w;
        w_z[i] = wz_w;

        // macro = clip(affine_remap(fbm(w_x, w_z, 1/(span*0.84), 5, sseed+30, gain=0.58), MACRO), 0, 1)
        let m = h::fbm(wx_w, wz_w, 1.0 / (feature_span * 0.84), 5, sseed + 30, 0.58);
        macro_f[i] = h::clip(h::affine_remap(m, MACRO_CENTER, MACRO_SCALE), 0.0, 1.0);

        // folded = ridged_multifractal(rx, rz*0.22, 1/(span*0.13), 5, sseed+60, gain=0.54)
        // on coords rotated about the fixed world origin (cx=cz=0).
        let (rx, rz) = h::rotated(wx_w, wz_w, style.angle_rad, 0.0, 0.0);
        let folded =
            h::ridged_multifractal(rx, rz * 0.22, 1.0 / (feature_span * 0.13), 5, sseed + 60, 0.54);
        folded_remap[i] = h::clip(h::affine_remap(folded, FOLDED_CENTER, FOLDED_SCALE), 0.0, 1.0);

        // hills_raw = ridged_multifractal(w_x, w_z, 1/(span*0.28), 5, sseed+90, gain=0.52)
        hills_raw[i] =
            h::ridged_multifractal(wx_w, wz_w, 1.0 / (feature_span * 0.28), 5, sseed + 90, 0.52);

        // fine = affine_remap(fbm(w_x, w_z, 1/(span*0.035), 4, sseed+150, gain=0.45), FINE)
        let fg = h::fbm(wx_w, wz_w, 1.0 / (feature_span * 0.035), 4, sseed + 150, 0.45);
        fine[i] = h::affine_remap(fg, FINE_CENTER, FINE_SCALE);
    }

    // --- ridges = smoothstep(0.40, 0.82, gaussian(folded_remap, sigma=1.1, nearest)) ---
    let folded_blur = array_ops::gaussian_filter_nearest(&folded_remap, rows, cols, 1.1, h::TRUNCATE);
    let mut ridges = vec![0.0_f64; n];
    for i in 0..n {
        ridges[i] = h::smoothstep(0.40, 0.82, folded_blur[i]);
    }

    // --- hills = clip(affine_remap(gaussian(hills_raw, sigma=2.4, nearest), HILLS), 0, 1) ---
    let hills_blur = array_ops::gaussian_filter_nearest(&hills_raw, rows, cols, 2.4, h::TRUNCATE);
    let mut hills = vec![0.0_f64; n];
    for i in 0..n {
        hills[i] = h::clip(h::affine_remap(hills_blur[i], HILLS_CENTER, HILLS_SCALE), 0.0, 1.0);
    }

    // --- upland = smoothstep(0.50, 0.82, gaussian(macro, sigma=4.2, nearest)) ---
    let macro_blur = array_ops::gaussian_filter_nearest(&macro_f, rows, cols, 4.2, h::TRUNCATE);
    let mut upland = vec![0.0_f64; n];
    for i in 0..n {
        upland[i] = h::smoothstep(0.50, 0.82, macro_blur[i]);
    }

    // --- flow_source = affine_remap(0.72*macro + 0.32*ridges + 0.28*hills + 0.26*upland, FLOW_SRC) (NO clip) ---
    let mut flow_source = vec![0.0_f64; n];
    for i in 0..n {
        let inner = 0.72 * macro_f[i] + 0.32 * ridges[i] + 0.28 * hills[i] + 0.26 * upland[i];
        flow_source[i] = h::affine_remap(inner, FLOW_SRC_CENTER, FLOW_SRC_SCALE);
    }

    // --- valley discharge (raw, no spread): pre-blur 1.15 -> MFD power=0.43 -> log1p/log1p(size) clip ---
    let discharge = valley_discharge_seam_safe(&flow_source, rows, cols, 0.43);

    // --- valleys = smoothstep(VALLEY, gaussian(discharge, sigma=1.8, nearest)) ---
    let discharge_blur_18 = array_ops::gaussian_filter_nearest(&discharge, rows, cols, 1.8, h::TRUNCATE);
    let mut valleys = vec![0.0_f64; n];
    for i in 0..n {
        valleys[i] = h::smoothstep(VALLEY_THRESH_LO, VALLEY_THRESH_HI, discharge_blur_18[i]);
    }

    // --- broad_valleys = smoothstep(BROAD_VALLEY, gaussian(discharge, sigma=4.2, nearest)) ---
    let discharge_blur_42 = array_ops::gaussian_filter_nearest(&discharge, rows, cols, 4.2, h::TRUNCATE);
    let mut broad_valleys = vec![0.0_f64; n];
    for i in 0..n {
        broad_valleys[i] =
            h::smoothstep(BROAD_VALLEY_THRESH_LO, BROAD_VALLEY_THRESH_HI, discharge_blur_42[i]);
    }

    // --- rounded = gaussian(affine_remap(0.52*macro + 0.48*hills, ROUNDED), sigma=max(smoothing_px,0.2), nearest) ---
    let mut rounded_inner = vec![0.0_f64; n];
    for i in 0..n {
        let inner = 0.52 * macro_f[i] + 0.48 * hills[i];
        rounded_inner[i] = h::affine_remap(inner, ROUNDED_CENTER, ROUNDED_SCALE);
    }
    let rounded = array_ops::gaussian_filter_nearest(
        &rounded_inner,
        rows,
        cols,
        style.smoothing_px.max(0.2),
        h::TRUNCATE,
    );

    // --- assemble height ---
    // height  = 0.42 * hill_gain * affine_remap(hills, 0.5, 2.0)
    // height += 0.42 * ridge_gain * ridges
    // height += 0.30 * upland_gain * upland
    // height -= 0.30 * valley_gain * valleys
    // height -= 0.16 * valley_gain * broad_valleys
    // height += 0.060 * texture_gain * fine * (0.45 + 0.55 * ridges)
    // height = 0.76 * height + 0.24 * rounded
    let mut height = vec![0.0_f64; n];
    for i in 0..n {
        let mut hv = 0.42 * style.hill_gain * h::affine_remap(hills[i], 0.5, 2.0);
        hv += 0.42 * style.ridge_gain * ridges[i];
        hv += 0.30 * style.upland_gain * upland[i];
        hv -= 0.30 * style.valley_gain * valleys[i];
        hv -= 0.16 * style.valley_gain * broad_valleys[i];
        hv += 0.060 * style.texture_gain * fine[i] * (0.45 + 0.55 * ridges[i]);
        height[i] = 0.76 * hv + 0.24 * rounded[i];
    }

    // --- final blend ---
    // final_blend = 0.85*height + 0.15*gaussian(height, sigma=1.0, nearest)
    // height = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE)
    let height_blur = array_ops::gaussian_filter_nearest(&height, rows, cols, 1.0, h::TRUNCATE);
    for i in 0..n {
        let final_blend = 0.85 * height[i] + 0.15 * height_blur[i];
        height[i] = h::affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
    }

    crop_core(&height, rows, cols, apron_px)
}

/// Crop the inner core: `field[a:-a, a:-a]`, returning a flat row-major
/// `(rows-2a) x (cols-2a)` vector. Matches numpy slicing exactly.
fn crop_core(field: &[f64], rows: usize, cols: usize, apron_px: usize) -> Vec<f64> {
    let a = apron_px;
    assert!(rows > 2 * a && cols > 2 * a, "apron too large for grid");
    let core_rows = rows - 2 * a;
    let core_cols = cols - 2 * a;
    let mut out = vec![0.0_f64; core_rows * core_cols];
    for r in 0..core_rows {
        for c in 0..core_cols {
            out[r * core_cols + c] = field[(r + a) * cols + (c + a)];
        }
    }
    out
}

/// Public entry point: TEMPERATE seam-safe height, core-cropped. Uses `STYLES[0]`
/// (appalachian_ridges). Signature matches the task contract.
#[allow(clippy::too_many_arguments)]
pub fn temperate_seamsafe(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    seed: i64,
    feature_span_m: f64,
    apron_px: usize,
) -> Vec<f64> {
    generate_seamsafe(
        wx,
        wz,
        rows,
        cols,
        seed,
        &APPALACHIAN_RIDGES,
        feature_span_m,
        apron_px,
    )
}
