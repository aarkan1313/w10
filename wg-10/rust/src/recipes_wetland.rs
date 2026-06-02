// WIRE: add mod recipes_wetland; + test mod
//! WETLAND biome recipe -- seam-safe (apron_px > 0) path, ported bit-close (f64) from
//! `tools/dem_pack/wetland_synthesis.py::generate`. Follows the MOUNTAIN template
//! (`recipes.rs`) exactly: shared math comes from [`crate::recipes::helpers`]; only the
//! wetland-specific constants, sub-fields (basin / floodplain / meander-channels / levees /
//! flat-base) and the assembly pipeline live here.
//!
//! NOTE: wetland is a TERRAIN/MASK setup biome -- water rendering, flooding, and materials
//! are later runtime work. Only its HEIGHT generation is ported here (the diagnostic masks
//! channels/floodplain/levees/basin/backwater are NOT emitted -- only `height` feeds the
//! parity oracle, exactly like the other biome ports). `backwater` is therefore NOT computed
//! at all: the Python builds it but it never enters the height assembly.
//!
//! Parity contract: `wetland_seamsafe(...)` reproduces the Python core-cropped height
//! within a tight epsilon, verified against the committed fixture
//! `tools/dem_pack/fixtures/recipe_wetland_fixture.json` (`recipes_wetland_tests.rs`).
//!
//! Wetland-specific notes vs the mountain/grassland/rainforest templates:
//!   * `recursive_domain_warp` uses warp_amount = feature_span*0.018,
//!     warp_freq = 1/(feature_span*0.88), seed = sseed+10, steps = 3, decay = 0.54,
//!     freq_mul = 1.68 (mountain 1.75 / grassland 1.70 / rainforest 1.74 -- all differ).
//!   * The fine-flow carve (`_fine_flow_seam_safe`, power = 0.44) is EXACTLY
//!     `helpers::flow_channels_seam_safe` with width_px = 1.8: pre-blur sigma = 1.15,
//!     MFD power = 0.44, FIXED-max log1p/log1p(size) normalize, spread sigma = 1.8
//!     (1.8 > 0.1 so the helper's `.max(0.1)` is a no-op -- bit-identical to the Python
//!     `sigma=1.8`). So the helper IS reused (unlike rainforest's two-mask drainage).
//!   * The meander field (`_meander_field`, seam_safe_mode=True) rotates the ALREADY
//!     domain-warped coords (`w_x`/`w_z`) about the FIXED world origin (cx=cz=0), then
//!     builds a Gaussian trunk + ridged_multifractal distributaries:
//!       meander = fbm(rx, rz, 1/(span*0.24), 5, sseed+120, gain=0.55) * span*0.050
//!       trunk_phase = (rz + meander) / max(span*0.090, 1.0) * 2*pi
//!       trunk = exp(-((sin(trunk_phase)/0.18)^2))
//!       distributary = ridged_multifractal(rx+meander, rz*0.38, 1/(span*0.13), 4, sseed+140, 0.50)
//!       -> clip(0.62*trunk + 0.58*smoothstep(0.50, 0.88, distributary), 0, 1)
//!     NOTE the `rz*0.38` z-scaling in the distributary call and the `rx+meander` x-shift.
//!     `_meander_field` is called with seed = `sseed` (NOT an extra offset), so its internal
//!     offsets are sseed+120 / sseed+140.
//!   * Walrus reassignment: `channels` is computed (meander*floodplain), THEN reassigned
//!     `channels = clip(0.68*channels + 0.50*smoothstep(0.56, 0.94, fine_flow), 0, 1)`.
//!     The reassigned `channels` is what feeds levees and the height carve.
//!   * `flow_input = affine_remap(macro - 0.34*basin, FLOW_INPUT)` is NOT clipped (it is the
//!     surface fed to MFD flow accumulation).
//!   * `micro = affine_remap(fbm(w_x,w_z, 1/(span*0.026),3,sseed+220,gain=0.44), MICRO)` is
//!     NOT clipped; it re-enters the texture term as `micro*(0.30 + 0.70*floodplain)`.
//!   * `flat_base` blurs (sigma=smoothing_px=4.4) the affine_remap'd combo
//!     `0.42*macro - 0.58*basin + 0.20*floodplain` (FLAT_BASE), no clip before/after.
//!   * Final assembly mixes `height = 0.66*height + 0.34*flat_base`, then
//!     `final_blend = 0.88*height + 0.12*gaussian(height, sigma=1.2)`, then
//!     `height = affine_remap(final_blend, FINAL)` -- the trailing zscore replacement.
//!   * Flow ties on flat areas: WETLAND IS LOW-RELIEF / FLAT, so the affine-remapped
//!     `flow_input` surface fed to MFD can contain EXACTLY-equal cells far more readily than
//!     the steeper biomes. The MFD downhill test is strict (`drop > 0`), so tied cells never
//!     flow to each other; the Rust stable-sort (ascending-index tie break) vs numpy
//!     quicksort tie order changes the result by at most ~1e-16 (same caveat as
//!     `array_ops::flow_accumulation_mfd`). The committed fixture's measured max |delta|
//!     (printed by the parity test) confirms the drift stays at the f64 noise floor here.

#![allow(dead_code)]

use crate::recipes::helpers as h;
use crate::array_ops;

// ---------------------------------------------------------------------------
// Apron constant -- WETLAND_APRON_PX. See Python docstring for the derivation.
// ---------------------------------------------------------------------------
pub const APRON_PX: usize = 160;

// ---------------------------------------------------------------------------
// Affine-remap constants (replace per-window zscore / norm01). Mirror of the
// WETLAND_*_CENTER / WETLAND_*_SCALE module constants in wetland_synthesis.py.
// ---------------------------------------------------------------------------
pub const MACRO_CENTER: f64 = -0.38;
pub const MACRO_SCALE: f64 = 1.14;

pub const FLOW_INPUT_CENTER: f64 = 0.28;
pub const FLOW_INPUT_SCALE: f64 = 3.00;

pub const MICRO_CENTER: f64 = 0.00;
pub const MICRO_SCALE: f64 = 3.29;

pub const FLAT_BASE_CENTER: f64 = 0.13;
pub const FLAT_BASE_SCALE: f64 = 3.49;

pub const MACRO_ZSCORE_CENTER: f64 = 0.50;
pub const MACRO_ZSCORE_SCALE: f64 = 4.00;

pub const FINAL_CENTER: f64 = 0.00;
pub const FINAL_SCALE: f64 = 0.82;

/// Mirror of `WetlandStyle` (only the fields the seam-safe pipeline reads).
#[derive(Clone, Copy, Debug)]
pub struct WetlandStyle {
    pub key: &'static str,
    pub angle_rad: f64,
    pub channel_gain: f64,
    pub floodplain_gain: f64,
    pub levee_gain: f64,
    pub basin_gain: f64,
    pub texture_gain: f64,
    pub smoothing_px: f64,
    pub seed_offset: i64,
}

/// `STYLES[0]` -- delta_distributary (the wetland reference style).
pub const DELTA_DISTRIBUTARY: WetlandStyle = WetlandStyle {
    key: "delta_distributary",
    angle_rad: 0.08,
    channel_gain: 1.32,
    floodplain_gain: 1.08,
    levee_gain: 0.90,
    basin_gain: 0.74,
    texture_gain: 0.32,
    smoothing_px: 4.4,
    seed_offset: 0,
};

/// Mirror of `_meander_field(wx, wz, feature_span_m, style, seed, seam_safe_mode=True)`
/// for a single point. `wx`/`wz` are the ALREADY domain-warped coords (`w_x`/`w_z`).
/// Rotation centre is fixed at the world origin (cx=cz=0) -- seam-safe.
#[inline]
fn meander_field_point(
    wx: f64,
    wz: f64,
    feature_span_m: f64,
    style: &WetlandStyle,
    seed: i64,
) -> f64 {
    let (rx, rz) = h::rotated(wx, wz, style.angle_rad, 0.0, 0.0);
    let meander = h::fbm(rx, rz, 1.0 / (feature_span_m * 0.24), 5, seed + 120, 0.55)
        * feature_span_m
        * 0.050;
    let pi = std::f64::consts::PI;
    let trunk_phase = (rz + meander) / (feature_span_m * 0.090).max(1.0) * pi * 2.0;
    let s = trunk_phase.sin() / 0.18;
    let trunk = (-(s * s)).exp();
    let distributary = h::ridged_multifractal(
        rx + meander,
        rz * 0.38,
        1.0 / (feature_span_m * 0.13),
        4,
        seed + 140,
        0.50,
    );
    h::clip(0.62 * trunk + 0.58 * h::smoothstep(0.50, 0.88, distributary), 0.0, 1.0)
}

/// Mirror of `_fine_flow_seam_safe(surface, mode='nearest', power=0.44)`.
///
/// pre-blur sigma=1.15 -> MFD flow accumulation (power=0.44) -> FIXED-max
/// log1p/log1p(size) normalize -> spread blur sigma=1.8. This is exactly
/// `helpers::flow_channels_seam_safe` with width_px=1.8 (the helper's `.max(0.1)`
/// is a no-op since 1.8 > 0.1).
#[inline]
fn fine_flow_seam_safe(surface: &[f64], rows: usize, cols: usize) -> Vec<f64> {
    h::flow_channels_seam_safe(surface, rows, cols, 1.8, 0.44)
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
    style: &WetlandStyle,
    feature_span_m: f64,
    apron_px: usize,
) -> Vec<f64> {
    assert_eq!(wx.len(), rows * cols, "wx len != rows*cols");
    assert_eq!(wz.len(), rows * cols, "wz len != rows*cols");
    let n = rows * cols;
    let feature_span = feature_span_m.max(1.0);
    let sseed = seed + style.seed_offset;

    // --- pointwise: recursive domain warp (freq_mul=1.68), then macro + micro + meander ---
    // and capture the warped coords w_x/w_z for downstream sub-fields.
    let mut w_x = vec![0.0_f64; n];
    let mut w_z = vec![0.0_f64; n];
    let mut macro_f = vec![0.0_f64; n];
    let mut micro = vec![0.0_f64; n];
    let mut meander = vec![0.0_f64; n];
    for i in 0..n {
        let (wx_w, wz_w) = h::recursive_domain_warp(
            wx[i],
            wz[i],
            feature_span * 0.018,
            1.0 / (feature_span * 0.88),
            sseed + 10,
            3,
            0.54,
            1.68,
        );
        w_x[i] = wx_w;
        w_z[i] = wz_w;
        // macro = clip(affine_remap(fbm(w_x,w_z, 1/(span*0.96),5,sseed+30,gain=0.58), MACRO), 0,1)
        let m = h::fbm(wx_w, wz_w, 1.0 / (feature_span * 0.96), 5, sseed + 30, 0.58);
        macro_f[i] = h::clip(h::affine_remap(m, MACRO_CENTER, MACRO_SCALE), 0.0, 1.0);
        // micro = affine_remap(fbm(w_x,w_z, 1/(span*0.026),3,sseed+220,gain=0.44), MICRO)  (NO clip)
        let mi = h::fbm(wx_w, wz_w, 1.0 / (feature_span * 0.026), 3, sseed + 220, 0.44);
        micro[i] = h::affine_remap(mi, MICRO_CENTER, MICRO_SCALE);
        // meander field (pointwise, rotates the warped coords about fixed origin)
        meander[i] = meander_field_point(wx_w, wz_w, feature_span, style, sseed);
    }

    // --- basin = smoothstep(0.48, 0.86, gaussian(1 - macro, sigma=5.8)) ---
    let mut one_minus_macro = vec![0.0_f64; n];
    for i in 0..n {
        one_minus_macro[i] = 1.0 - macro_f[i];
    }
    let basin_blur = array_ops::gaussian_filter_nearest(&one_minus_macro, rows, cols, 5.8, h::TRUNCATE);
    let mut basin = vec![0.0_f64; n];
    for i in 0..n {
        basin[i] = h::smoothstep(0.48, 0.86, basin_blur[i]);
    }

    // --- floodplain = smoothstep(0.36, 0.78, gaussian(1 - abs(macro - 0.42), sigma=5.2)) ---
    let mut floodplain_src = vec![0.0_f64; n];
    for i in 0..n {
        floodplain_src[i] = 1.0 - (macro_f[i] - 0.42).abs();
    }
    let floodplain_blur = array_ops::gaussian_filter_nearest(&floodplain_src, rows, cols, 5.2, h::TRUNCATE);
    let mut floodplain = vec![0.0_f64; n];
    for i in 0..n {
        floodplain[i] = h::smoothstep(0.36, 0.78, floodplain_blur[i]);
    }

    // --- channels = meander * floodplain  (first assignment; walrus reassigned below) ---
    let mut channels = vec![0.0_f64; n];
    for i in 0..n {
        channels[i] = meander[i] * floodplain[i];
    }

    // --- flow_input = affine_remap(macro - 0.34*basin, FLOW_INPUT)  (NO clip) ---
    let mut flow_input = vec![0.0_f64; n];
    for i in 0..n {
        flow_input[i] = h::affine_remap(macro_f[i] - 0.34 * basin[i], FLOW_INPUT_CENTER, FLOW_INPUT_SCALE);
    }
    // fine_flow = _fine_flow_seam_safe(flow_input, power=0.44)
    let fine_flow = fine_flow_seam_safe(&flow_input, rows, cols);

    // --- channels = clip(0.68*channels + 0.50*smoothstep(0.56, 0.94, fine_flow), 0, 1) ---
    for i in 0..n {
        channels[i] = h::clip(
            0.68 * channels[i] + 0.50 * h::smoothstep(0.56, 0.94, fine_flow[i]),
            0.0,
            1.0,
        );
    }

    // --- levees = smoothstep(0.02, 0.18, gaussian(channels,2.2) - gaussian(channels,5.2))
    //              *= 1 - smoothstep(0.42, 0.86, channels) ---
    let chan_blur22 = array_ops::gaussian_filter_nearest(&channels, rows, cols, 2.2, h::TRUNCATE);
    let chan_blur52 = array_ops::gaussian_filter_nearest(&channels, rows, cols, 5.2, h::TRUNCATE);
    let mut levees = vec![0.0_f64; n];
    for i in 0..n {
        let dog = chan_blur22[i] - chan_blur52[i];
        let lv = h::smoothstep(0.02, 0.18, dog);
        levees[i] = lv * (1.0 - h::smoothstep(0.42, 0.86, channels[i]));
    }

    // --- flat_base = gaussian(affine_remap(0.42*macro - 0.58*basin + 0.20*floodplain, FLAT_BASE),
    //                          sigma=smoothing_px) ---
    let mut flat_base_inner = vec![0.0_f64; n];
    for i in 0..n {
        let inner = 0.42 * macro_f[i] - 0.58 * basin[i] + 0.20 * floodplain[i];
        flat_base_inner[i] = h::affine_remap(inner, FLAT_BASE_CENTER, FLAT_BASE_SCALE);
    }
    let flat_base = array_ops::gaussian_filter_nearest(&flat_base_inner, rows, cols, style.smoothing_px, h::TRUNCATE);

    // --- assemble height ---
    // height  = affine_remap(macro, MACRO_ZSCORE) * 0.18
    // height -= 0.32 * basin_gain * basin
    // height -= 0.28 * floodplain_gain * floodplain
    // height -= 0.30 * channel_gain * channels
    // height += 0.54 * levee_gain * levees
    // height += 0.045 * texture_gain * micro * (0.30 + 0.70*floodplain)
    // height = 0.66*height + 0.34*flat_base
    let mut height = vec![0.0_f64; n];
    for i in 0..n {
        let mut hv = h::affine_remap(macro_f[i], MACRO_ZSCORE_CENTER, MACRO_ZSCORE_SCALE) * 0.18;
        hv -= 0.32 * style.basin_gain * basin[i];
        hv -= 0.28 * style.floodplain_gain * floodplain[i];
        hv -= 0.30 * style.channel_gain * channels[i];
        hv += 0.54 * style.levee_gain * levees[i];
        hv += 0.045 * style.texture_gain * micro[i] * (0.30 + 0.70 * floodplain[i]);
        hv = 0.66 * hv + 0.34 * flat_base[i];
        height[i] = hv;
    }

    // --- final blend ---
    // final_blend = 0.88*height + 0.12*gaussian(height, sigma=1.2)
    // height = affine_remap(final_blend, FINAL)
    let height_blur = array_ops::gaussian_filter_nearest(&height, rows, cols, 1.2, h::TRUNCATE);
    for i in 0..n {
        let final_blend = 0.88 * height[i] + 0.12 * height_blur[i];
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

/// Public entry point: WETLAND seam-safe height, core-cropped. Uses `STYLES[0]`
/// (delta_distributary). Signature matches the task contract.
#[allow(clippy::too_many_arguments)]
pub fn wetland_seamsafe(
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
        &DELTA_DISTRIBUTARY,
        feature_span_m,
        apron_px,
    )
}
