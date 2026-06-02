// WIRE: add mod recipes_grassland; + test mod
//! GRASSLAND biome recipe — seam-safe (apron_px > 0) path, ported bit-close (f64) from
//! `tools/dem_pack/grassland_synthesis.py::generate`. Follows the MOUNTAIN template
//! (`recipes.rs`) exactly: shared math comes from [`crate::recipes::helpers`]; only the
//! grassland-specific constants, sub-fields (sandhills / escarpments / draws) and the
//! assembly pipeline live here.
//!
//! Parity contract: `grassland_seamsafe(...)` reproduces the Python core-cropped height
//! within a tight epsilon, verified against the committed fixture
//! `tools/dem_pack/fixtures/recipe_grassland_fixture.json` (`recipes_grassland_tests.rs`).
//!
//! Grassland-specific notes vs the mountain template:
//!   * Many MORE affine-remap constants (MACRO/SECONDARY/SWELLS/SWELLS_ZSCORE/BASE_FLOW/
//!     FINE_GRAIN/LOW_RIPPLE/SH_ENVELOPE/SH_BROKEN/SH_FINAL/ESC_PLATEAU/ESC_FINAL/FINAL).
//!   * `recursive_domain_warp` uses freq_mul = 1.70 (mountain used 1.75).
//!   * The draw carve uses grassland's flow power 0.50 and a FIXED spread sigma 2.1
//!     (passed as `width_px` to `helpers::flow_channels_seam_safe`; 2.1 > 0.1 so the
//!     helper's `.max(0.1)` is a no-op — bit-identical to the Python `sigma=2.1`).
//!   * The escarpment field calls `fault_block_field` with the Python default
//!     `neighborhood=2`.
//!   * Sandhills are computed bit-for-bit even when `sandhill_gain == 0` (STYLES[0]):
//!     they re-enter the texture term as `low_ripple * (0.35 + 0.65*sandhills)`, which is
//!     NOT gated by `sandhill_gain`.
//!   * Flow tie-ordering: grassland pans CAN be flat, so `base_for_flow` may contain
//!     EXACTLY-equal cells (constant swells/escarpments/pans). The MFD downhill test is
//!     strict (`drop > 0`), so tied cells never flow to each other; the Rust stable-sort
//!     vs numpy quicksort tie order changes the result by at most ~1e-16 (same caveat as
//!     `array_ops::flow_accumulation_mfd`).

#![allow(dead_code)]

use crate::recipe_noise;
use crate::recipes::helpers as h;
use crate::array_ops;

// ---------------------------------------------------------------------------
// Apron constant — GRASSLAND_APRON_PX. See Python docstring for the derivation.
// ---------------------------------------------------------------------------
pub const APRON_PX: usize = 160;

// ---------------------------------------------------------------------------
// Affine-remap constants (replace per-window zscore / norm01). Mirror of the
// GRASSLAND_*_CENTER / GRASSLAND_*_SCALE module constants in grassland_synthesis.py.
// ---------------------------------------------------------------------------
pub const MACRO_CENTER: f64 = -0.50;
pub const MACRO_SCALE: f64 = 1.14;

pub const SECONDARY_CENTER: f64 = -0.69;
pub const SECONDARY_SCALE: f64 = 0.72;

pub const SWELLS_CENTER: f64 = 0.13;
pub const SWELLS_SCALE: f64 = 1.37;

pub const SWELLS_ZSCORE_CENTER: f64 = 0.507;
pub const SWELLS_ZSCORE_SCALE: f64 = 4.49;

pub const BASE_FLOW_CENTER: f64 = 0.503;
pub const BASE_FLOW_SCALE: f64 = 5.11;

pub const FINE_GRAIN_CENTER: f64 = 0.00;
pub const FINE_GRAIN_SCALE: f64 = 3.47;

pub const LOW_RIPPLE_CENTER: f64 = 0.353;
pub const LOW_RIPPLE_SCALE: f64 = 4.27;

pub const SH_ENVELOPE_CENTER: f64 = -0.38;
pub const SH_ENVELOPE_SCALE: f64 = 1.01;

pub const SH_BROKEN_CENTER: f64 = -0.87;
pub const SH_BROKEN_SCALE: f64 = 0.58;

pub const SH_FINAL_CENTER: f64 = 0.00;
pub const SH_FINAL_SCALE: f64 = 1.00;

pub const ESC_PLATEAU_CENTER: f64 = -0.51;
pub const ESC_PLATEAU_SCALE: f64 = 0.90;

pub const ESC_FINAL_CENTER: f64 = 0.00;
pub const ESC_FINAL_SCALE: f64 = 1.00;

pub const FINAL_CENTER: f64 = 0.00;
pub const FINAL_SCALE: f64 = 0.82;

/// Mirror of `GrasslandStyle` (the fields the seam-safe pipeline reads).
#[derive(Clone, Copy, Debug)]
pub struct GrasslandStyle {
    pub key: &'static str,
    pub angle_rad: f64,
    pub swell_gain: f64,
    pub draw_gain: f64,
    pub sandhill_gain: f64,
    pub pan_gain: f64,
    pub escarpment_gain: f64,
    pub texture_gain: f64,
    pub smoothing_px: f64,
    pub seed_offset: i64,
}

/// `STYLES[0]` — rolling_prairie (the grassland reference style).
pub const ROLLING_PRAIRIE: GrasslandStyle = GrasslandStyle {
    key: "rolling_prairie",
    angle_rad: 0.34,
    swell_gain: 1.18,
    draw_gain: 0.72,
    sandhill_gain: 0.00,
    pan_gain: 0.18,
    escarpment_gain: 0.18,
    texture_gain: 0.42,
    smoothing_px: 3.7,
    seed_offset: 0,
};

/// Mirror of `_sandhill_field(..., seam_safe_mode=True, blur_mode='nearest')` returning
/// the WHOLE field. Rotation centre is fixed at the world origin (cx=cz=0) — seam-safe.
///
/// `wx`/`wz` here are the ALREADY domain-warped grids (`generate` passes `w_x`/`w_z`).
fn sandhill_field(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    feature_span_m: f64,
    style: &GrasslandStyle,
    seed: i64,
) -> Vec<f64> {
    let n = rows * cols;
    let spacing = feature_span_m * 0.030;
    let pi = std::f64::consts::PI;
    // pointwise softened*envelope*broken, then a nearest gaussian blur, then affine_remap+clip.
    let mut pre = vec![0.0_f64; n];
    for i in 0..n {
        let (rx, rz) = h::rotated(wx[i], wz[i], style.angle_rad, 0.0, 0.0);
        let warp = h::fbm(wx[i], wz[i], 1.0 / (feature_span_m * 0.30), 4, seed + 120, 0.52)
            * spacing
            * 1.20;
        let cross = h::fbm(
            wx[i] + rz * 0.18,
            wz[i] + rx * 0.08,
            1.0 / (feature_span_m * 0.12),
            3,
            seed + 126,
            0.50,
        );
        let phase = (rx + warp + cross * spacing * 0.42) / spacing.max(1.0) * pi * 2.0;
        let secondary =
            (rx * 0.74 + rz * 0.18 + warp * 0.30) / (spacing * 1.65).max(1.0) * pi * 2.0;
        let ridges =
            0.74 * (1.0 - phase.sin().abs()) + 0.26 * (1.0 - secondary.sin().abs());
        let softened = h::clip(ridges, 0.0, 1.0).powf(1.55);

        let envelope_raw = h::fbm(wx[i], wz[i], 1.0 / (feature_span_m * 0.76), 4, seed + 130, 0.5);
        let envelope = h::smoothstep(
            0.48,
            0.80,
            h::clip(h::affine_remap(envelope_raw, SH_ENVELOPE_CENTER, SH_ENVELOPE_SCALE), 0.0, 1.0),
        );
        let broken_raw = h::fbm(wx[i], wz[i], 1.0 / (feature_span_m * 0.055), 3, seed + 136, 0.46);
        let broken = 0.55
            + 0.45 * h::clip(h::affine_remap(broken_raw, SH_BROKEN_CENTER, SH_BROKEN_SCALE), 0.0, 1.0);
        pre[i] = softened * envelope * broken;
    }
    let blurred = array_ops::gaussian_filter_nearest(&pre, rows, cols, 1.55, h::TRUNCATE);
    blurred
        .iter()
        .map(|&v| h::clip(h::affine_remap(v, SH_FINAL_CENTER, SH_FINAL_SCALE), 0.0, 1.0))
        .collect()
}

/// Mirror of `_escarpment_field(..., seam_safe_mode=True, blur_mode='nearest')` returning
/// the WHOLE field. Rotation uses `style.angle_rad + 0.58` about fixed origin (cx=cz=0).
fn escarpment_field(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    feature_span_m: f64,
    style: &GrasslandStyle,
    seed: i64,
) -> Vec<f64> {
    let n = rows * cols;
    let mut edge = vec![0.0_f64; n];
    for i in 0..n {
        let (rx, rz) = h::rotated(wx[i], wz[i], style.angle_rad + 0.58, 0.0, 0.0);
        // fault_block_field with the Python default neighborhood=2.
        let bands = recipe_noise::fault_block_field(
            rx,
            rz,
            feature_span_m * 0.54,
            feature_span_m * 0.040,
            seed + 210,
            2,
        );
        let plateau_raw = h::fbm(wx[i], wz[i], 1.0 / (feature_span_m * 0.64), 4, seed + 230, 0.5);
        let plateau = h::smoothstep(
            0.44,
            0.78,
            h::clip(h::affine_remap(plateau_raw, ESC_PLATEAU_CENTER, ESC_PLATEAU_SCALE), 0.0, 1.0),
        );
        edge[i] = h::smoothstep(0.18, 0.62, bands.abs()) * plateau;
    }
    let blurred = array_ops::gaussian_filter_nearest(&edge, rows, cols, 1.4, h::TRUNCATE);
    blurred
        .iter()
        .map(|&v| h::clip(h::affine_remap(v, ESC_FINAL_CENTER, ESC_FINAL_SCALE), 0.0, 1.0))
        .collect()
}

/// Mirror of `_draw_channels_seam_safe(surface, mode='nearest', power=0.50)`.
///
/// pre-blur sigma=1.15 -> MFD flow accumulation (power) -> FIXED-max log1p/log1p(size)
/// normalize -> spread blur sigma=2.1. This is exactly `helpers::flow_channels_seam_safe`
/// with width_px=2.1 (the helper's `.max(0.1)` is a no-op since 2.1 > 0.1).
#[inline]
fn draw_channels_seam_safe(surface: &[f64], rows: usize, cols: usize) -> Vec<f64> {
    h::flow_channels_seam_safe(surface, rows, cols, 2.1, 0.50)
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
    style: &GrasslandStyle,
    feature_span_m: f64,
    apron_px: usize,
) -> Vec<f64> {
    assert_eq!(wx.len(), rows * cols, "wx len != rows*cols");
    assert_eq!(wz.len(), rows * cols, "wz len != rows*cols");
    let n = rows * cols;
    let feature_span = feature_span_m.max(1.0);
    let sseed = seed + style.seed_offset;

    // --- pointwise: recursive domain warp (freq_mul=1.70), then macro / secondary ---
    // and capture the warped coords w_x/w_z for downstream sub-fields.
    let mut w_x = vec![0.0_f64; n];
    let mut w_z = vec![0.0_f64; n];
    let mut macro_f = vec![0.0_f64; n];
    let mut secondary = vec![0.0_f64; n];
    for i in 0..n {
        let (wx_w, wz_w) = h::recursive_domain_warp(
            wx[i],
            wz[i],
            feature_span * 0.020,
            1.0 / (feature_span * 0.78),
            sseed + 10,
            3,
            0.55,
            1.70,
        );
        w_x[i] = wx_w;
        w_z[i] = wz_w;
        let m = h::fbm(wx_w, wz_w, 1.0 / (feature_span * 0.92), 5, sseed + 30, 0.58);
        macro_f[i] = h::clip(h::affine_remap(m, MACRO_CENTER, MACRO_SCALE), 0.0, 1.0);
        let s = h::fbm(wx_w, wz_w, 1.0 / (feature_span * 0.34), 4, sseed + 50, 0.55);
        secondary[i] = h::clip(h::affine_remap(s, SECONDARY_CENTER, SECONDARY_SCALE), 0.0, 1.0);
    }

    // --- swells: blur the 0.74*macro + 0.26*secondary combo, then affine_remap+clip ---
    let mut combo = vec![0.0_f64; n];
    for i in 0..n {
        combo[i] = 0.74 * macro_f[i] + 0.26 * secondary[i];
    }
    let pre_swells = array_ops::gaussian_filter_nearest(&combo, rows, cols, style.smoothing_px, h::TRUNCATE);
    let mut swells = vec![0.0_f64; n];
    for i in 0..n {
        swells[i] = h::clip(h::affine_remap(pre_swells[i], SWELLS_CENTER, SWELLS_SCALE), 0.0, 1.0);
    }

    // --- pans = smoothstep(0.54, 0.88, gaussian(1 - swells, sigma=5.2)) ---
    let mut one_minus_swells = vec![0.0_f64; n];
    for i in 0..n {
        one_minus_swells[i] = 1.0 - swells[i];
    }
    let pans_blur = array_ops::gaussian_filter_nearest(&one_minus_swells, rows, cols, 5.2, h::TRUNCATE);
    let mut pans = vec![0.0_f64; n];
    for i in 0..n {
        pans[i] = h::smoothstep(0.54, 0.88, pans_blur[i]);
    }

    // --- sandhills / escarpments (whole-field sub-pipelines on warped coords) ---
    let sandhills = sandhill_field(&w_x, &w_z, rows, cols, feature_span, style, sseed);
    let escarpments = escarpment_field(&w_x, &w_z, rows, cols, feature_span, style, sseed);

    // --- base_for_flow = affine_remap(0.82*swells + 0.28*escarpments - 0.34*pans) (NO clip) ---
    let mut base_for_flow = vec![0.0_f64; n];
    for i in 0..n {
        let inner = 0.82 * swells[i] + 0.28 * escarpments[i] - 0.34 * pans[i];
        base_for_flow[i] = h::affine_remap(inner, BASE_FLOW_CENTER, BASE_FLOW_SCALE);
    }

    // --- draws (walrus reassignments): flow channels -> smoothstep -> *= pan factor ---
    let mut draws = draw_channels_seam_safe(&base_for_flow, rows, cols);
    for i in 0..n {
        let d = h::smoothstep(0.60, 0.94, draws[i]);
        draws[i] = d * (0.42 + 0.58 * (1.0 - pans[i]));
    }

    // --- fine_grain + low_ripple: seam-safe rotation (angle + 1.10), fixed origin ---
    let mut fine_grain = vec![0.0_f64; n];
    let mut low_ripple = vec![0.0_f64; n];
    for i in 0..n {
        let (rx, rz) = h::rotated(w_x[i], w_z[i], style.angle_rad + 1.10, 0.0, 0.0);
        let fg = h::fbm(rx, rz, 1.0 / (feature_span * 0.032), 4, sseed + 310, 0.46);
        fine_grain[i] = h::affine_remap(fg, FINE_GRAIN_CENTER, FINE_GRAIN_SCALE);
        // ridged_multifractal(rx, rz*0.34, ...) — NOTE the rz*0.34 scaling.
        let lr = h::ridged_multifractal(rx, rz * 0.34, 1.0 / (feature_span * 0.075), 3, sseed + 330, 0.44);
        low_ripple[i] = h::affine_remap(lr, LOW_RIPPLE_CENTER, LOW_RIPPLE_SCALE);
    }

    // --- assemble height ---
    // height  = affine_remap(swells, SWELLS_ZSCORE) * (0.52 * swell_gain)
    // height += 0.16 * sandhill_gain * sandhills
    // height += 0.34 * escarpment_gain * escarpments
    // height -= 0.28 * pan_gain * pans
    // height -= 0.24 * draw_gain * draws
    // height += texture_gain * (0.050*fine_grain + 0.050*low_ripple*(0.35 + 0.65*sandhills))
    let mut height = vec![0.0_f64; n];
    for i in 0..n {
        let mut hv = h::affine_remap(swells[i], SWELLS_ZSCORE_CENTER, SWELLS_ZSCORE_SCALE)
            * (0.52 * style.swell_gain);
        hv += 0.16 * style.sandhill_gain * sandhills[i];
        hv += 0.34 * style.escarpment_gain * escarpments[i];
        hv -= 0.28 * style.pan_gain * pans[i];
        hv -= 0.24 * style.draw_gain * draws[i];
        hv += style.texture_gain
            * (0.050 * fine_grain[i] + 0.050 * low_ripple[i] * (0.35 + 0.65 * sandhills[i]));
        height[i] = hv;
    }

    // --- floor blend ---
    // smooth = gaussian(height, sigma=max(smoothing_px, 0.5))
    let smooth = array_ops::gaussian_filter_nearest(
        &height,
        rows,
        cols,
        style.smoothing_px.max(0.5),
        h::TRUNCATE,
    );
    // open_floor = clip(0.62*pans + 0.26*(1 - escarpments), 0, 1)
    // height = height*(1 - 0.28*open_floor) + smooth*(0.28*open_floor)
    for i in 0..n {
        let open_floor = h::clip(0.62 * pans[i] + 0.26 * (1.0 - escarpments[i]), 0.0, 1.0);
        height[i] = height[i] * (1.0 - 0.28 * open_floor) + smooth[i] * (0.28 * open_floor);
    }

    // --- final blend ---
    // final_blend = 0.86*height + 0.14*gaussian(height, sigma=1.1)
    // height = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE)
    let height_blur = array_ops::gaussian_filter_nearest(&height, rows, cols, 1.1, h::TRUNCATE);
    for i in 0..n {
        let final_blend = 0.86 * height[i] + 0.14 * height_blur[i];
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

/// Public entry point: GRASSLAND seam-safe height, core-cropped. Uses `STYLES[0]`
/// (rolling_prairie). Signature matches the task contract.
#[allow(clippy::too_many_arguments)]
pub fn grassland_seamsafe(
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
        &ROLLING_PRAIRIE,
        feature_span_m,
        apron_px,
    )
}
