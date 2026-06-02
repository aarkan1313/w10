// WIRE: add mod recipes_glacial; + test mod to lib.rs
//
//! GLACIAL biome recipe — seam-safe path, ported bit-close (f64) from
//! `tools/dem_pack/glacial_synthesis.py::generate` (`apron_px > 0` branch).
//!
//! Follows `recipes.rs::mountain` as the template and REUSES `crate::recipes::helpers`
//! for the genuinely shared pieces (affine_remap, smoothstep, clip, rotated,
//! apron_meshgrid, the recursive_domain_warp / fbm / ridged_multifractal call-arity
//! wrappers). Whole-array operators come from [`crate::array_ops`] (gaussian_filter_nearest,
//! flow_accumulation_mfd) — both fixture-proven.
//!
//! GLACIAL-SPECIFIC DIVERGENCE from the mountain template: glacial's seam-safe flow
//! channels (`_trough_channels_seam_safe`) pre-blur the surface with gaussian sigma=1.85,
//! whereas `helpers::flow_channels_seam_safe` hardcodes sigma=1.15 (the mountain value).
//! So this module implements its own `trough_channels_seam_safe` (sigma=1.85) rather than
//! calling the shared helper. Everything else mirrors the helper exactly (MFD power,
//! log1p fixed-max normalize, spread blur sigma=max(width_px,0.1), clip [0,1]).
//!
//! Flow tie-ordering note: glacial icefields/troughs CAN be flat plateaus, so exact-height
//! ties between cells are theoretically possible (unlike mountain's continuous-noise base).
//! The MFD downhill test is STRICT (`drop > 0`), so tied cells never flow to each other;
//! the only effect of tie order is one-ULP accumulation noise on a genuine plateau. The
//! continuous warped-noise base used here produces no exact ties in practice (parity ~1e-15).

// Consumed by the GPU/CPU producer seam (wired by the controller); until then exercised
// only by the parity test.
#![allow(dead_code)]

use crate::array_ops;
use crate::recipes::helpers as h;

// ---------------------------------------------------------------------------
// Apron constant (mirror of GLACIAL_APRON_PX).
// ---------------------------------------------------------------------------
pub const APRON_PX: usize = 160;

// ---------------------------------------------------------------------------
// Affine-remap constants (replace per-window zscore / norm01). Mirror of the
// GLACIAL_* module constants in glacial_synthesis.py.
// ---------------------------------------------------------------------------
pub const REGIONAL_CENTER: f64 = -0.446;
pub const REGIONAL_SCALE: f64 = 1.181;

pub const RELIEF_CENTER: f64 = -0.008;
pub const RELIEF_SCALE: f64 = 1.465;

pub const MASSIF_CENTER: f64 = 0.154;
pub const MASSIF_SCALE: f64 = 0.787;

pub const BASE_CENTER: f64 = 0.758;
pub const BASE_SCALE: f64 = 2.487;

pub const PRIMARY_CENTER: f64 = 0.003;
pub const PRIMARY_SCALE: f64 = 0.690;

pub const AXIAL_GATE_CENTER: f64 = -0.430;
pub const AXIAL_GATE_SCALE: f64 = 1.010;

pub const RELIEF_ZSCORE_CENTER: f64 = 0.503;
pub const RELIEF_ZSCORE_SCALE: f64 = 5.102;

pub const RIDGE_DETAIL_CENTER: f64 = 0.331;
pub const RIDGE_DETAIL_SCALE: f64 = 4.616;

pub const CLOSE_DETAIL_CENTER: f64 = 0.003;
pub const CLOSE_DETAIL_SCALE: f64 = 3.478;

pub const STRIATIONS_CENTER: f64 = 0.001;
pub const STRIATIONS_SCALE: f64 = 4.516;

pub const FINAL_CENTER: f64 = -0.096;
pub const FINAL_SCALE: f64 = 0.820;

/// Mirror of `GlacialStyle` (the fields the seam-safe pipeline reads). All fields used.
#[derive(Clone, Copy, Debug)]
pub struct GlacialStyle {
    pub key: &'static str,
    pub angle_rad: f64,
    pub uplift_gain: f64,
    pub trough_gain: f64,
    pub ridge_gain: f64,
    pub branch_gain: f64,
    pub trough_width_px: f64,
    pub ice_smooth_px: f64,
    pub detail_gain: f64,
    pub striation_gain: f64,
    pub anisotropy: f64,
}

/// `STYLES[0]` — fjorded_troughs (the reference style, matching the mountain template's
/// "use STYLES[0]" choice).
pub const FJORDED_TROUGHS: GlacialStyle = GlacialStyle {
    key: "fjorded_troughs",
    angle_rad: 0.56,
    uplift_gain: 1.16,
    trough_gain: 1.34,
    ridge_gain: 1.02,
    branch_gain: 0.82,
    trough_width_px: 6.8,
    ice_smooth_px: 6.2,
    detail_gain: 0.40,
    striation_gain: 0.82,
    anisotropy: 0.72,
};

/// Seam-safe `_oriented_relief` for the WHOLE field (rotation centre fixed at world
/// origin cx=cz=0). Returns the field AFTER the trailing `gaussian_filter(sigma=1.25)`.
fn oriented_relief(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    span_m: f64,
    style: &GlacialStyle,
    seed: i64,
) -> Vec<f64> {
    let n = rows * cols;
    let mut normed = vec![0.0_f64; n];
    for i in 0..n {
        let (rx, rz) = h::rotated(wx[i], wz[i], style.angle_rad, 0.0, 0.0);
        // recursive_domain_warp(rx, rz*anisotropy, span*0.054, 1/(span*0.68), seed+100, 3, 0.56, 1.78)
        let (w_rx, w_rz) = h::recursive_domain_warp(
            rx,
            rz * style.anisotropy,
            span_m * 0.054,
            1.0 / (span_m * 0.68),
            seed + 100,
            3,
            0.56,
            1.78,
        );
        let long = h::ridged_multifractal(w_rx, w_rz, 1.0 / (span_m * 0.44), 5, seed + 120, 0.56);
        let mid = h::ridged_multifractal(w_rx, w_rz, 1.0 / (span_m * 0.22), 4, seed + 130, 0.52);
        // cross = fbm(w_rx + 0.18*w_rz, w_rz - 0.10*w_rx, 1/(span*0.30), 5, seed+140, 0.54)
        let cross = h::fbm(
            w_rx + 0.18 * w_rz,
            w_rz - 0.10 * w_rx,
            1.0 / (span_m * 0.30),
            5,
            seed + 140,
            0.54,
        );
        let raw = 0.60 * long + 0.22 * mid + 0.14 * cross;
        // seam-safe: clip(affine_remap(raw, RELIEF_CENTER, RELIEF_SCALE), 0, 1)
        normed[i] = h::clip(h::affine_remap(raw, RELIEF_CENTER, RELIEF_SCALE), 0.0, 1.0);
    }
    // gaussian_filter(normed, sigma=1.25, mode='nearest')
    array_ops::gaussian_filter_nearest(&normed, rows, cols, 1.25, h::TRUNCATE)
}

/// Seam-safe `_axial_troughs` for the WHOLE field. Returns the field AFTER its trailing
/// `gaussian_filter(sigma=max(trough_width_px*0.18, 0.8))`.
fn axial_troughs(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    span_m: f64,
    style: &GlacialStyle,
    seed: i64,
) -> Vec<f64> {
    let n = rows * cols;
    // width = span * (0.030 + 0.010 * clip(trough_width_px/7.0, 0.0, 1.4))
    let width =
        span_m * (0.030 + 0.010 * h::clip(style.trough_width_px / 7.0, 0.0, 1.4));
    let width_div = width.max(1.0); // max(width, 1.0)
    let offsets: [f64; 3] = [-0.24, 0.0, 0.25];

    let mut pre = vec![0.0_f64; n];
    for i in 0..n {
        let (rx, rz) = h::rotated(wx[i], wz[i], style.angle_rad, 0.0, 0.0);
        // long_noise = fbm(rx, rz*0.10, 1/(span*0.70), 5, seed+170, 0.55)
        let long_noise = h::fbm(rx, rz * 0.10, 1.0 / (span_m * 0.70), 5, seed + 170, 0.55);
        // mid_noise = fbm(rx + rz*0.05, rz*0.16, 1/(span*0.34), 4, seed+180, 0.50)
        let mid_noise = h::fbm(rx + rz * 0.05, rz * 0.16, 1.0 / (span_m * 0.34), 4, seed + 180, 0.50);
        // meander = (0.72*long_noise + 0.28*mid_noise) * span * 0.13
        let meander = (0.72 * long_noise + 0.28 * mid_noise) * span_m * 0.13;
        // trough = max over offsets of exp(-(dist*dist)) where dist=|rz-center|/max(width,1)
        let mut trough = 0.0_f64;
        for &offset in offsets.iter() {
            let center = meander + span_m * offset;
            let dist = (rz - center).abs() / width_div;
            let g = (-(dist * dist)).exp();
            if g > trough {
                trough = g;
            }
        }
        // gate_raw = fbm(rx, rz, 1/(span*0.52), 4, seed+190, 0.52)
        let gate_raw = h::fbm(rx, rz, 1.0 / (span_m * 0.52), 4, seed + 190, 0.52);
        // gate = smoothstep(0.28, 0.88, clip(affine_remap(gate_raw, AXIAL_GATE), 0, 1))
        let gate = h::smoothstep(
            0.28,
            0.88,
            h::clip(h::affine_remap(gate_raw, AXIAL_GATE_CENTER, AXIAL_GATE_SCALE), 0.0, 1.0),
        );
        // clip(trough * (0.55 + 0.45*gate), 0, 1)
        pre[i] = h::clip(trough * (0.55 + 0.45 * gate), 0.0, 1.0);
    }
    // gaussian_filter(..., sigma=max(trough_width_px*0.18, 0.8), mode='nearest')
    let sigma = (style.trough_width_px * 0.18).max(0.8);
    array_ops::gaussian_filter_nearest(&pre, rows, cols, sigma, h::TRUNCATE)
}

/// Seam-safe `_trough_channels_seam_safe`. GLACIAL pre-blur sigma is 1.85 (NOT the
/// mountain helper's 1.15), so this is implemented locally rather than via
/// `helpers::flow_channels_seam_safe`. Otherwise identical: real MFD accumulation, fixed-max
/// log1p normalization, spread blur sigma=max(width_px,0.1), clip [0,1]. All blurs 'nearest'.
fn trough_channels_seam_safe(
    surface: &[f64],
    rows: usize,
    cols: usize,
    width_px: f64,
    power: f64,
) -> Vec<f64> {
    // pre = gaussian_filter(surface, sigma=1.85, mode='nearest')
    let pre = array_ops::gaussian_filter_nearest(surface, rows, cols, 1.85, h::TRUNCATE);
    // acc = _flow_accumulation_mfd(pre, power)
    let acc = array_ops::flow_accumulation_mfd(&pre, rows, cols, power);
    // discharge = clip(log1p(acc) / log1p(acc.size), 0, 1)   (acc.size == rows*cols)
    let log_size = ((rows * cols) as f64).ln_1p();
    let mut discharge: Vec<f64> = acc
        .iter()
        .map(|&a| h::clip(a.ln_1p() / log_size, 0.0, 1.0))
        .collect();
    // gaussian_filter(discharge, sigma=max(width_px, 0.1), mode='nearest'); clip [0,1]
    let sigma = width_px.max(0.1);
    discharge = array_ops::gaussian_filter_nearest(&discharge, rows, cols, sigma, h::TRUNCATE);
    for v in discharge.iter_mut() {
        *v = h::clip(*v, 0.0, 1.0);
    }
    discharge
}

/// Seam-safe `_striations` for the WHOLE field. Returns `affine_remap(raw, STRIATIONS)`
/// (NO clip, NO trailing blur — matches the Python seam-safe branch exactly).
fn striations(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    span_m: f64,
    style: &GlacialStyle,
    seed: i64,
) -> Vec<f64> {
    let n = rows * cols;
    let mut out = vec![0.0_f64; n];
    for i in 0..n {
        let (rx, rz) = h::rotated(wx[i], wz[i], style.angle_rad, 0.0, 0.0);
        // long_scrape = fbm(rx, rz*0.18, 1/(span*0.030), 4, seed+210, 0.48)
        let long_scrape = h::fbm(rx, rz * 0.18, 1.0 / (span_m * 0.030), 4, seed + 210, 0.48);
        // fine_scrape = fbm(rx + 0.18*rz, rz*0.12, 1/(span*0.014), 3, seed+220, 0.44)
        let fine_scrape = h::fbm(rx + 0.18 * rz, rz * 0.12, 1.0 / (span_m * 0.014), 3, seed + 220, 0.44);
        let raw = 0.72 * long_scrape + 0.28 * fine_scrape;
        out[i] = h::affine_remap(raw, STRIATIONS_CENTER, STRIATIONS_SCALE);
    }
    out
}

/// Port of `glacial_synthesis.generate(..., apron_px=APRON_PX)` SEAM-SAFE path, returning
/// the CORE-cropped height (length `(rows-2*apron)*(cols-2*apron)`).
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
    style: &GlacialStyle,
    feature_span_m: f64,
    apron_px: usize,
) -> Vec<f64> {
    assert_eq!(wx.len(), rows * cols, "wx len != rows*cols");
    assert_eq!(wz.len(), rows * cols, "wz len != rows*cols");
    let n = rows * cols;
    let feature_span = feature_span_m.max(1.0);

    // --- top-level recursive domain warp, then pointwise regional / detail fields ---
    // Python: w_x, w_z = recursive_domain_warp(wx, wz, span*0.044, 1/(span*0.78),
    //         seed+10, 3, 0.58, 1.70)
    // We must materialise w_x/w_z because _oriented_relief / _axial_troughs / _striations
    // each ROTATE w_x/w_z (about cx=cz=0) before their own warp.
    let mut w_x = vec![0.0_f64; n];
    let mut w_z = vec![0.0_f64; n];
    let mut regional = vec![0.0_f64; n];
    let mut ridge_detail = vec![0.0_f64; n];
    let mut close_detail = vec![0.0_f64; n];
    for i in 0..n {
        let (wxi, wzi) = h::recursive_domain_warp(
            wx[i],
            wz[i],
            feature_span * 0.044,
            1.0 / (feature_span * 0.78),
            seed + 10,
            3,
            0.58,
            1.70,
        );
        w_x[i] = wxi;
        w_z[i] = wzi;
        // regional = clip(affine_remap(fbm(w_x,w_z, 1/(span*0.96),5,seed+20,0.56), REGIONAL), 0,1)
        let reg = h::fbm(wxi, wzi, 1.0 / (feature_span * 0.96), 5, seed + 20, 0.56);
        regional[i] = h::clip(h::affine_remap(reg, REGIONAL_CENTER, REGIONAL_SCALE), 0.0, 1.0);
        // ridge_detail = affine_remap(ridged_multifractal(w_x,w_z, 1/(span*0.060),4,seed+40,0.50), RIDGE_DETAIL)
        let rd = h::ridged_multifractal(wxi, wzi, 1.0 / (feature_span * 0.060), 4, seed + 40, 0.50);
        ridge_detail[i] = h::affine_remap(rd, RIDGE_DETAIL_CENTER, RIDGE_DETAIL_SCALE);
        // close_detail = affine_remap(fbm(w_x,w_z, 1/(span*0.026),4,seed+50,0.46), CLOSE_DETAIL)
        let cd = h::fbm(wxi, wzi, 1.0 / (feature_span * 0.026), 4, seed + 50, 0.46);
        close_detail[i] = h::affine_remap(cd, CLOSE_DETAIL_CENTER, CLOSE_DETAIL_SCALE);
    }

    // relief = _oriented_relief(w_x, w_z, span, style, seed, seam_safe=True)  [incl. sigma=1.25 blur]
    let relief = oriented_relief(&w_x, &w_z, rows, cols, feature_span, style, seed);

    // relief_envelope = smoothstep(0.22, 0.62, gaussian(relief, sigma=5.8, nearest))
    let relief_blur58 = array_ops::gaussian_filter_nearest(&relief, rows, cols, 5.8, h::TRUNCATE);
    let mut relief_envelope = vec![0.0_f64; n];
    for i in 0..n {
        relief_envelope[i] = h::smoothstep(0.22, 0.62, relief_blur58[i]);
    }

    // icefield = smoothstep(0.48, 0.78, gaussian(0.56*regional + 0.44*relief_envelope, sigma=7.0, nearest))
    let mut ice_inner = vec![0.0_f64; n];
    for i in 0..n {
        ice_inner[i] = 0.56 * regional[i] + 0.44 * relief_envelope[i];
    }
    let ice_blur = array_ops::gaussian_filter_nearest(&ice_inner, rows, cols, 7.0, h::TRUNCATE);
    let mut icefield = vec![0.0_f64; n];
    for i in 0..n {
        icefield[i] = h::smoothstep(0.48, 0.78, ice_blur[i]);
    }

    // massif = gaussian(clip(affine_remap(0.72*regional + 0.72*relief_envelope + 0.20*relief, MASSIF), 0,1), sigma=2.8, nearest)
    let mut massif_inner = vec![0.0_f64; n];
    for i in 0..n {
        let inner = 0.72 * regional[i] + 0.72 * relief_envelope[i] + 0.20 * relief[i];
        massif_inner[i] = h::clip(h::affine_remap(inner, MASSIF_CENTER, MASSIF_SCALE), 0.0, 1.0);
    }
    let massif = array_ops::gaussian_filter_nearest(&massif_inner, rows, cols, 2.8, h::TRUNCATE);

    // base = affine_remap(uplift_gain*(1.34*massif + 0.22*relief - 0.16*(1-icefield)), BASE)
    let mut base = vec![0.0_f64; n];
    for i in 0..n {
        let inner = style.uplift_gain * (1.34 * massif[i] + 0.22 * relief[i] - 0.16 * (1.0 - icefield[i]));
        base[i] = h::affine_remap(inner, BASE_CENTER, BASE_SCALE);
    }

    // flow_primary = _trough_channels_seam_safe(base, width=trough_width_px, power=0.58)
    let flow_primary = trough_channels_seam_safe(&base, rows, cols, style.trough_width_px, 0.58);
    // axial = _axial_troughs(w_x, w_z, span, style, seed, seam_safe=True)
    let axial = axial_troughs(&w_x, &w_z, rows, cols, feature_span, style, seed);
    // primary = clip(affine_remap(0.58*flow_primary + 1.18*axial, PRIMARY), 0, 1)
    // primary_mask = smoothstep(0.34, 0.84, primary)
    let mut primary_mask = vec![0.0_f64; n];
    for i in 0..n {
        let primary =
            h::clip(h::affine_remap(0.58 * flow_primary[i] + 1.18 * axial[i], PRIMARY_CENTER, PRIMARY_SCALE), 0.0, 1.0);
        primary_mask[i] = h::smoothstep(0.34, 0.84, primary);
    }

    // relief_z = affine_remap(relief, RELIEF_ZSCORE)
    // branch_surface = base + 0.10*relief_z - 0.18*gaussian(primary_mask, sigma=1.6, nearest)
    let pm_blur16 = array_ops::gaussian_filter_nearest(&primary_mask, rows, cols, 1.6, h::TRUNCATE);
    let mut branch_surface = vec![0.0_f64; n];
    for i in 0..n {
        let relief_z = h::affine_remap(relief[i], RELIEF_ZSCORE_CENTER, RELIEF_ZSCORE_SCALE);
        branch_surface[i] = base[i] + 0.10 * relief_z - 0.18 * pm_blur16[i];
    }
    // tributary = _trough_channels_seam_safe(branch_surface, width=max(trough_width_px*0.48, 0.8), power=0.36)
    let trib_width = (style.trough_width_px * 0.48).max(0.8);
    let tributary = trough_channels_seam_safe(&branch_surface, rows, cols, trib_width, 0.36);
    // tributary_mask = smoothstep(0.54, 0.96, tributary) * (0.45 + 0.55*relief_envelope)
    let mut tributary_mask = vec![0.0_f64; n];
    for i in 0..n {
        tributary_mask[i] = h::smoothstep(0.54, 0.96, tributary[i]) * (0.45 + 0.55 * relief_envelope[i]);
    }

    // scrapes = _striations(w_x, w_z, span, style, seed, seam_safe=True)
    let scrapes = striations(&w_x, &w_z, rows, cols, feature_span, style, seed);

    // ---- shared assembly (identical in both paths) ----
    // ridge_wall = smoothstep(0.48, 0.84, relief_envelope) * (1 - 0.52*primary_mask)
    // trough_floor = clip(0.90*primary_mask + 0.44*tributary_mask, 0, 1)
    // high_ice = clip(icefield * (1 - 0.30*primary_mask), 0, 1)
    let mut ridge_wall = vec![0.0_f64; n];
    let mut trough_floor = vec![0.0_f64; n];
    let mut high_ice = vec![0.0_f64; n];
    for i in 0..n {
        ridge_wall[i] = h::smoothstep(0.48, 0.84, relief_envelope[i]) * (1.0 - 0.52 * primary_mask[i]);
        trough_floor[i] = h::clip(0.90 * primary_mask[i] + 0.44 * tributary_mask[i], 0.0, 1.0);
        high_ice[i] = h::clip(icefield[i] * (1.0 - 0.30 * primary_mask[i]), 0.0, 1.0);
    }

    // height = base.copy()
    // height += ridge_gain * (0.10 + 0.52*ridge_wall) * (0.24*ridge_detail)
    // height += detail_gain * (0.04 + 0.18*ridge_wall) * (0.18*close_detail)
    // height += striation_gain * (0.04 + 0.22*(high_ice + trough_floor)) * (0.18*scrapes)
    // height -= trough_gain * (0.44 + 0.44*high_ice + 0.16*ridge_wall) * primary_mask
    // height -= branch_gain * (0.12 + 0.34*ridge_wall) * tributary_mask
    let mut height = vec![0.0_f64; n];
    for i in 0..n {
        let rw = ridge_wall[i];
        let hi = high_ice[i];
        let mut hv = base[i];
        hv += style.ridge_gain * (0.10 + 0.52 * rw) * (0.24 * ridge_detail[i]);
        hv += style.detail_gain * (0.04 + 0.18 * rw) * (0.18 * close_detail[i]);
        hv += style.striation_gain * (0.04 + 0.22 * (hi + trough_floor[i])) * (0.18 * scrapes[i]);
        hv -= style.trough_gain * (0.44 + 0.44 * hi + 0.16 * rw) * primary_mask[i];
        hv -= style.branch_gain * (0.12 + 0.34 * rw) * tributary_mask[i];
        height[i] = hv;
    }

    // floor_mask = clip(smoothstep(0.36, 0.80, gaussian(trough_floor, sigma=1.6, nearest)), 0, 1)
    let tf_blur16 = array_ops::gaussian_filter_nearest(&trough_floor, rows, cols, 1.6, h::TRUNCATE);
    let mut floor_mask = vec![0.0_f64; n];
    for i in 0..n {
        floor_mask[i] = h::clip(h::smoothstep(0.36, 0.80, tf_blur16[i]), 0.0, 1.0);
    }
    // ice_mask = clip(smoothstep(0.50, 0.90, high_ice), 0, 1)
    let mut ice_mask = vec![0.0_f64; n];
    for i in 0..n {
        ice_mask[i] = h::clip(h::smoothstep(0.50, 0.90, high_ice[i]), 0.0, 1.0);
    }
    // floor = gaussian(height, sigma=max(ice_smooth_px, 0.2), nearest)
    let floor = array_ops::gaussian_filter_nearest(&height, rows, cols, style.ice_smooth_px.max(0.2), h::TRUNCATE);
    // ice_smooth = gaussian(height, sigma=max(ice_smooth_px*0.65, 0.2), nearest)
    let ice_smooth = array_ops::gaussian_filter_nearest(&height, rows, cols, (style.ice_smooth_px * 0.65).max(0.2), h::TRUNCATE);
    // height = height*(1 - 0.52*floor_mask) + floor*(0.52*floor_mask)
    // height = height*(1 - 0.28*ice_mask) + ice_smooth*(0.28*ice_mask)
    // height -= 0.16*floor_mask
    for i in 0..n {
        height[i] = height[i] * (1.0 - 0.52 * floor_mask[i]) + floor[i] * (0.52 * floor_mask[i]);
        height[i] = height[i] * (1.0 - 0.28 * ice_mask[i]) + ice_smooth[i] * (0.28 * ice_mask[i]);
        height[i] -= 0.16 * floor_mask[i];
    }

    // --- final blend (seam-safe) ---
    // final_blend = 0.66*height + 0.34*gaussian(height, sigma=1.35, nearest)
    // height = affine_remap(final_blend, FINAL)
    let height_blur135 = array_ops::gaussian_filter_nearest(&height, rows, cols, 1.35, h::TRUNCATE);
    for i in 0..n {
        let final_blend = 0.66 * height[i] + 0.34 * height_blur135[i];
        height[i] = h::affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
    }

    // --- crop to core: height[a:-a, a:-a] ---
    crop_core(&height, rows, cols, apron_px)
}

/// Crop the inner core `field[a:-a, a:-a]` -> flat row-major `(rows-2a)x(cols-2a)`.
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

/// Public entry point: GLACIAL seam-safe height, core-cropped. Uses `STYLES[0]`
/// (fjorded_troughs). Mirrors `glacial_synthesis.generate(...)["height"]`.
#[allow(clippy::too_many_arguments)]
pub fn glacial_seamsafe(
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
        &FJORDED_TROUGHS,
        feature_span_m,
        apron_px,
    )
}
