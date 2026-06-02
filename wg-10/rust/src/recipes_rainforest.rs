// WIRE: add mod recipes_rainforest; + test mod
//! RAINFOREST biome recipe -- seam-safe (apron_px > 0) path, ported bit-close (f64) from
//! `tools/dem_pack/rainforest_synthesis.py::generate`. Follows the MOUNTAIN template
//! (`recipes.rs`) exactly: shared math comes from [`crate::recipes::helpers`]; only the
//! rainforest-specific constants, sub-fields (hills / plateau / drainage / wet-rounding)
//! and the assembly pipeline live here.
//!
//! Parity contract: `rainforest_seamsafe(...)` reproduces the Python core-cropped height
//! within a tight epsilon, verified against the committed fixture
//! `tools/dem_pack/fixtures/recipe_rainforest_fixture.json` (`recipes_rainforest_tests.rs`).
//!
//! Rainforest-specific notes vs the mountain/grassland templates:
//!   * `recursive_domain_warp` uses steps = 4 (mountain/grassland used 3) and
//!     freq_mul = 1.74, decay = 0.54, warp_amount = feature_span*0.034,
//!     warp_freq = 1/(feature_span*0.72), seed = sseed+10.
//!   * The drainage carve is a LOCAL variant (`drainage_seam_safe`): it shares the
//!     pre-blur sigma=1.15 + MFD power=0.38 + FIXED-max log1p/log1p(size) normalize with
//!     `helpers::flow_channels_seam_safe`, BUT emits TWO masks from the SAME discharge:
//!       tributaries = smoothstep(0.42, 0.88, gaussian(discharge, sigma=1.15))
//!       trunk       = smoothstep(0.68, 0.95, gaussian(discharge, sigma=2.2))
//!     `helpers::flow_channels_seam_safe` only returns one spread blur, so it is NOT
//!     reused here (the second sigma + the two smoothsteps differ).
//!   * `hills` blurs the ridged_multifractal field FIRST (sigma=1.7, nearest) THEN
//!     affine_remap+clip -- the blur is inside, the remap outside.
//!   * `ridges` rotates about the FIXED world origin (cx=cz=0) and scales the rotated z by
//!     0.42: `ridged_multifractal(rx, rz*0.42, ...)`, then smoothstep(0.42, 0.83, .).
//!   * `flow_source` is affine_remap'd WITHOUT a trailing clip (unlike macro/hills/plateau).
//!   * `close` is a very-low-freq fbm at feature_span*0.030 (seam-safe affine_remap, no clip).
//!   * Walrus reassignment: `height = 0.72*height + 0.28*wet_rounding`, then
//!     `final_blend = 0.84*height + 0.16*gaussian(height, sigma=1.0)`, then
//!     `height = affine_remap(final_blend, FINAL)` -- the trailing zscore replacement.
//!   * Flow ties on flat areas: the MFD downhill test is strict (`drop > 0`), so tied cells
//!     never flow to each other; Rust stable-sort vs numpy quicksort tie order changes the
//!     result by at most ~1e-16 (same caveat as `array_ops::flow_accumulation_mfd`).

#![allow(dead_code)]

use crate::recipes::helpers as h;
use crate::array_ops;

// ---------------------------------------------------------------------------
// Apron constant -- RAINFOREST_APRON_PX. See Python docstring for the derivation.
// ---------------------------------------------------------------------------
pub const APRON_PX: usize = 160;

// ---------------------------------------------------------------------------
// Affine-remap constants (replace per-window zscore / norm01). Mirror of the
// RAINFOREST_*_CENTER / RAINFOREST_*_SCALE module constants in rainforest_synthesis.py.
// ---------------------------------------------------------------------------
pub const MACRO_CENTER: f64 = -0.667;
pub const MACRO_SCALE: f64 = 0.717;

pub const HILLS_CENTER: f64 = 0.000;
pub const HILLS_SCALE: f64 = 1.199;

pub const PLATEAU_SEED_CENTER: f64 = -0.847;
pub const PLATEAU_SEED_SCALE: f64 = 0.626;

pub const FLOW_CENTER: f64 = 0.481;
pub const FLOW_SCALE: f64 = 3.059;

pub const CLOSE_CENTER: f64 = 0.000;
pub const CLOSE_SCALE: f64 = 3.436;

pub const WET_ROUNDING_CENTER: f64 = 0.503;
pub const WET_ROUNDING_SCALE: f64 = 5.066;

pub const HILLS_ZSCORE_CENTER: f64 = 0.386;
pub const HILLS_ZSCORE_SCALE: f64 = 3.960;

pub const FINAL_CENTER: f64 = 0.000;
pub const FINAL_SCALE: f64 = 1.70;

/// Mirror of `RainforestStyle` (only the fields the seam-safe pipeline reads).
#[derive(Clone, Copy, Debug)]
pub struct RainforestStyle {
    pub key: &'static str,
    pub angle_rad: f64,
    pub hill_gain: f64,
    pub ridge_gain: f64,
    pub drainage_gain: f64,
    pub plateau_gain: f64,
    pub lowland_gain: f64,
    pub texture_gain: f64,
    pub smoothing_px: f64,
    pub seed_offset: i64,
}

/// `STYLES[0]` -- humid_dissected_hills (the rainforest reference style).
pub const HUMID_DISSECTED_HILLS: RainforestStyle = RainforestStyle {
    key: "humid_dissected_hills",
    angle_rad: 0.42,
    hill_gain: 1.18,
    ridge_gain: 0.78,
    drainage_gain: 1.18,
    plateau_gain: 0.36,
    lowland_gain: 0.30,
    texture_gain: 0.58,
    smoothing_px: 2.6,
    seed_offset: 0,
};

/// Mirror of `_drainage_seam_safe(surface, mode='nearest', power=0.38)`.
///
/// pre-blur sigma=1.15 -> MFD flow accumulation (power) -> FIXED-max
/// `clip(log1p(acc)/log1p(acc.size), 0, 1)` normalize, THEN two spread paths:
///   tributaries = smoothstep(0.42, 0.88, gaussian(discharge, sigma=1.15))
///   trunk       = smoothstep(0.68, 0.95, gaussian(discharge, sigma=2.2))
///
/// Shares the first half with `helpers::flow_channels_seam_safe` but emits TWO masks,
/// so it is inlined here rather than reusing the helper.
fn drainage_seam_safe(
    surface: &[f64],
    rows: usize,
    cols: usize,
    power: f64,
) -> (Vec<f64>, Vec<f64>) {
    let pre = array_ops::gaussian_filter_nearest(surface, rows, cols, 1.15, h::TRUNCATE);
    let acc = array_ops::flow_accumulation_mfd(&pre, rows, cols, power);
    // FIXED-max normalization (SEAM-SAFE: no per-window statistics).
    // log1p(acc.size): acc.size is the element count (rows*cols), matching numpy.
    let log_size = ((rows * cols) as f64).ln_1p();
    let discharge: Vec<f64> = acc
        .iter()
        .map(|&a| h::clip(a.ln_1p() / log_size, 0.0, 1.0))
        .collect();
    let trib_blur = array_ops::gaussian_filter_nearest(&discharge, rows, cols, 1.15, h::TRUNCATE);
    let trunk_blur = array_ops::gaussian_filter_nearest(&discharge, rows, cols, 2.2, h::TRUNCATE);
    let tributaries: Vec<f64> = trib_blur.iter().map(|&v| h::smoothstep(0.42, 0.88, v)).collect();
    let trunk: Vec<f64> = trunk_blur.iter().map(|&v| h::smoothstep(0.68, 0.95, v)).collect();
    (tributaries, trunk)
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
    style: &RainforestStyle,
    feature_span_m: f64,
    apron_px: usize,
) -> Vec<f64> {
    assert_eq!(wx.len(), rows * cols, "wx len != rows*cols");
    assert_eq!(wz.len(), rows * cols, "wz len != rows*cols");
    let n = rows * cols;
    let feature_span = feature_span_m.max(1.0);
    let sseed = seed + style.seed_offset;

    // --- pointwise: recursive domain warp (steps=4, decay=0.54, freq_mul=1.74) ---
    // Python: w_x, w_z = recursive_domain_warp(wx, wz, span*0.034, 1/(span*0.72),
    //         sseed+10, 4, 0.54, 1.74). Capture w_x/w_z for downstream sub-fields.
    let mut w_x = vec![0.0_f64; n];
    let mut w_z = vec![0.0_f64; n];
    let mut macro_f = vec![0.0_f64; n];
    let mut plateau_seed = vec![0.0_f64; n];
    let mut ridges = vec![0.0_f64; n];
    // hills needs the blurred ridged_multifractal field -> compute the raw field first.
    let mut hills_raw = vec![0.0_f64; n];
    for i in 0..n {
        let (wx_w, wz_w) = h::recursive_domain_warp(
            wx[i],
            wz[i],
            feature_span * 0.034,
            1.0 / (feature_span * 0.72),
            sseed + 10,
            4,
            0.54,
            1.74,
        );
        w_x[i] = wx_w;
        w_z[i] = wz_w;
        // macro = clip(affine_remap(fbm(w_x,w_z, 1/(span*0.78),5,sseed+30,gain=0.58)), 0,1)
        let m = h::fbm(wx_w, wz_w, 1.0 / (feature_span * 0.78), 5, sseed + 30, 0.58);
        macro_f[i] = h::clip(h::affine_remap(m, MACRO_CENTER, MACRO_SCALE), 0.0, 1.0);
        // hills_raw = ridged_multifractal(w_x,w_z, 1/(span*0.24),5,sseed+60,gain=0.52)
        hills_raw[i] = h::ridged_multifractal(wx_w, wz_w, 1.0 / (feature_span * 0.24), 5, sseed + 60, 0.52);
        // ridges: rotate (w_x,w_z) about fixed origin (0,0), then ridged_multifractal(rx, rz*0.42, ...)
        let (rx, rz) = h::rotated(wx_w, wz_w, style.angle_rad, 0.0, 0.0);
        let rmf = h::ridged_multifractal(rx, rz * 0.42, 1.0 / (feature_span * 0.16), 5, sseed + 90, 0.50);
        ridges[i] = h::smoothstep(0.42, 0.83, rmf);
        // plateau_seed = clip(affine_remap(fbm(w_x,w_z, 1/(span*0.44),4,sseed+130,gain=0.55)), 0,1)
        let ps = h::fbm(wx_w, wz_w, 1.0 / (feature_span * 0.44), 4, sseed + 130, 0.55);
        plateau_seed[i] = h::clip(h::affine_remap(ps, PLATEAU_SEED_CENTER, PLATEAU_SEED_SCALE), 0.0, 1.0);
    }

    // --- hills = clip(affine_remap(gaussian(hills_raw, sigma=1.7), HILLS), 0, 1) ---
    let hills_blur = array_ops::gaussian_filter_nearest(&hills_raw, rows, cols, 1.7, h::TRUNCATE);
    let mut hills = vec![0.0_f64; n];
    for i in 0..n {
        hills[i] = h::clip(h::affine_remap(hills_blur[i], HILLS_CENTER, HILLS_SCALE), 0.0, 1.0);
    }

    // --- plateau = smoothstep(0.54,0.80, gaussian(plateau_seed, sigma=4.5)) * (1 - 0.38*ridges) ---
    let plateau_blur = array_ops::gaussian_filter_nearest(&plateau_seed, rows, cols, 4.5, h::TRUNCATE);
    let mut plateau = vec![0.0_f64; n];
    for i in 0..n {
        plateau[i] = h::smoothstep(0.54, 0.80, plateau_blur[i]) * (1.0 - 0.38 * ridges[i]);
    }

    // --- lowland = smoothstep(0.57 - 0.10*lg, 0.88 - 0.06*lg, gaussian(1 - macro, sigma=5.4)) ---
    let mut one_minus_macro = vec![0.0_f64; n];
    for i in 0..n {
        one_minus_macro[i] = 1.0 - macro_f[i];
    }
    let lowland_source = array_ops::gaussian_filter_nearest(&one_minus_macro, rows, cols, 5.4, h::TRUNCATE);
    let mut lowland = vec![0.0_f64; n];
    let lo_e0 = 0.57 - 0.10 * style.lowland_gain;
    let lo_e1 = 0.88 - 0.06 * style.lowland_gain;
    for i in 0..n {
        lowland[i] = h::smoothstep(lo_e0, lo_e1, lowland_source[i]);
    }

    // --- flow_source = affine_remap(0.66*macro + 0.46*hills + 0.28*ridges + 0.20*plateau
    //                                - 0.36*lowland, FLOW)  (NO clip) ---
    let mut flow_source = vec![0.0_f64; n];
    for i in 0..n {
        let inner =
            0.66 * macro_f[i] + 0.46 * hills[i] + 0.28 * ridges[i] + 0.20 * plateau[i] - 0.36 * lowland[i];
        flow_source[i] = h::affine_remap(inner, FLOW_CENTER, FLOW_SCALE);
    }

    // --- drainage = clip(0.68*tributaries + 0.58*trunk, 0, 1) ---
    let (tributaries, trunk) = drainage_seam_safe(&flow_source, rows, cols, 0.38);
    let mut drainage = vec![0.0_f64; n];
    for i in 0..n {
        drainage[i] = h::clip(0.68 * tributaries[i] + 0.58 * trunk[i], 0.0, 1.0);
    }

    // --- close = affine_remap(fbm(w_x,w_z, 1/(span*0.030),4,sseed+210,gain=0.45), CLOSE)  (NO clip) ---
    let mut close = vec![0.0_f64; n];
    for i in 0..n {
        let cf = h::fbm(w_x[i], w_z[i], 1.0 / (feature_span * 0.030), 4, sseed + 210, 0.45);
        close[i] = h::affine_remap(cf, CLOSE_CENTER, CLOSE_SCALE);
    }

    // --- wet_rounding = gaussian(affine_remap(0.62*macro + 0.36*hills + 0.26*plateau,
    //                                          WET_ROUNDING), sigma=smoothing_px) ---
    let mut wet_inner = vec![0.0_f64; n];
    for i in 0..n {
        let inner = 0.62 * macro_f[i] + 0.36 * hills[i] + 0.26 * plateau[i];
        wet_inner[i] = h::affine_remap(inner, WET_ROUNDING_CENTER, WET_ROUNDING_SCALE);
    }
    let wet_rounding = array_ops::gaussian_filter_nearest(&wet_inner, rows, cols, style.smoothing_px, h::TRUNCATE);

    // --- assemble height ---
    // height  = 0.46 * hill_gain * affine_remap(hills, HILLS_ZSCORE)
    // height += 0.34 * ridge_gain * ridges
    // height += 0.30 * plateau_gain * plateau
    // height -= 0.38 * lowland_gain * lowland
    // height -= 0.34 * drainage_gain * drainage
    // height += texture_gain * (0.055*close + 0.045*close*ridges)
    // height = 0.72*height + 0.28*wet_rounding
    let mut height = vec![0.0_f64; n];
    for i in 0..n {
        let mut hv = 0.46 * style.hill_gain * h::affine_remap(hills[i], HILLS_ZSCORE_CENTER, HILLS_ZSCORE_SCALE);
        hv += 0.34 * style.ridge_gain * ridges[i];
        hv += 0.30 * style.plateau_gain * plateau[i];
        hv -= 0.38 * style.lowland_gain * lowland[i];
        hv -= 0.34 * style.drainage_gain * drainage[i];
        hv += style.texture_gain * (0.055 * close[i] + 0.045 * close[i] * ridges[i]);
        hv = 0.72 * hv + 0.28 * wet_rounding[i];
        height[i] = hv;
    }

    // --- final blend ---
    // final_blend = 0.84*height + 0.16*gaussian(height, sigma=1.0)
    // height = affine_remap(final_blend, FINAL)
    let height_blur = array_ops::gaussian_filter_nearest(&height, rows, cols, 1.0, h::TRUNCATE);
    for i in 0..n {
        let final_blend = 0.84 * height[i] + 0.16 * height_blur[i];
        height[i] = h::affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
    }

    // --- crop to core: height[a:-a, a:-a] ---
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

/// Public entry point: RAINFOREST seam-safe height, core-cropped. Uses `STYLES[0]`
/// (humid_dissected_hills). Signature matches the task contract.
#[allow(clippy::too_many_arguments)]
pub fn rainforest_seamsafe(
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
        &HUMID_DISSECTED_HILLS,
        feature_span_m,
        apron_px,
    )
}
