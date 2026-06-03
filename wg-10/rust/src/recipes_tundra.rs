// WIRE: add mod recipes_tundra; + test mod to lib.rs
//
//! TUNDRA biome recipe — seam-safe path, ported bit-close (f64) from
//! `tools/dem_pack/tundra_synthesis.py::generate` (`apron_px > 0` branch).
//!
//! Follows `recipes.rs::mountain` as the template and REUSES `crate::recipes::helpers`
//! for the genuinely shared pieces (affine_remap, smoothstep, clip, rotated,
//! apron_meshgrid, the recursive_domain_warp / fbm / ridged_multifractal call-arity
//! wrappers, and `flow_channels_seam_safe`). Whole-array operators come from
//! [`crate::array_ops`] (gaussian_filter_nearest, flow_accumulation_mfd) — both
//! fixture-proven. The per-point `cellular_edges` primitive (not wrapped in helpers,
//! since only tundra uses it) is called directly from [`crate::recipe_noise`].
//!
//! TUNDRA-SPECIFIC notes vs the mountain/glacial templates:
//!   * The seam-safe drainage (`_drainage_channels_seam_safe`) pre-blurs the surface with
//!     gaussian sigma=1.15 (the SAME value the shared `helpers::flow_channels_seam_safe`
//!     hardcodes) and spreads with sigma=2.0 (passed as `width_px`) at MFD power 0.48. So
//!     unlike glacial (which needs sigma=1.85 and rolls its own), tundra REUSES the shared
//!     helper verbatim: `flow_channels_seam_safe(flow_source, width_px=2.0, power=0.48)`.
//!   * `_rotated` is used for the POLYGON / STRIPE / FOOTHILL patterns and must rotate
//!     about a FIXED world centre (cx=cz=0) in seam-safe mode — exactly like mountain's
//!     oriented ridges. It rotates the WARPED coords `w_x`/`w_z`, not the raw `wx`/`wz`.
//!   * `pattern` is MULTIPLIED by `plain` (a broad-plain mask) before assembly.
//!   * `fringe` uses the UNROTATED warped coords with z anisotropy 0.48; `foothills` uses
//!     the ROTATED coords with z anisotropy 0.48 and has NO trailing blur (just smoothstep
//!     over the raw ridged_multifractal).
//!
//! Flow tie-ordering note (same as glacial): broad flat Arctic plains CAN produce exact-
//! height ties, but the MFD downhill test is STRICT (`drop > 0`), so tied cells never flow
//! to each other; the only effect of tie order is one-ULP accumulation noise on a genuine
//! plateau. The continuous warped-noise flow_source used here produces no exact ties in
//! practice (parity ~1e-15).

// Consumed by the GPU/CPU producer seam (wired by the controller); until then exercised
// only by the parity test.
#![allow(dead_code)]

use crate::array_ops;
use crate::recipe_noise;
use crate::recipes::helpers as h;

// ---------------------------------------------------------------------------
// Apron constant (mirror of TUNDRA_APRON_PX).
// ---------------------------------------------------------------------------
pub const APRON_PX: usize = 160;

// ---------------------------------------------------------------------------
// Affine-remap constants (replace per-window zscore / norm01). Mirror of the
// TUNDRA_* module constants in tundra_synthesis.py.
// ---------------------------------------------------------------------------

// macro fbm (norm01): mean_min=-0.668, ptp=1.398 -> center=-0.668, scale=0.715
pub const MACRO_CENTER: f64 = -0.668;
pub const MACRO_SCALE: f64 = 0.715;

// macro normed (second-stage zscore for height term): center=0.497, scale=1/0.236
pub const MACRO_ZSCORE_CENTER: f64 = 0.497;
pub const MACRO_ZSCORE_SCALE: f64 = 4.24;

// flow_source_inner (zscore): mean=0.1525, std=0.1761 -> center=0.153, scale=5.68
pub const FLOW_SOURCE_CENTER: f64 = 0.153;
pub const FLOW_SOURCE_SCALE: f64 = 5.68;

// fine fbm (zscore): mean=-0.001, std=0.309 -> center=0.000, scale=3.24
pub const FINE_CENTER: f64 = 0.000;
pub const FINE_SCALE: f64 = 3.24;

// base_inner (0.74*macro+0.26*foothills) zscore: mean=0.405, std=0.185 -> center=0.405, scale=5.41
pub const BASE_CENTER: f64 = 0.405;
pub const BASE_SCALE: f64 = 5.41;

// final blend (replaces trailing zscore): center=0.000, scale=0.82
pub const FINAL_CENTER: f64 = 0.000;
pub const FINAL_SCALE: f64 = 0.82;

/// Mirror of `TundraStyle` (the fields the seam-safe pipeline reads). All fields used.
#[derive(Clone, Copy, Debug)]
pub struct TundraStyle {
    pub key: &'static str,
    pub angle_rad: f64,
    pub plain_gain: f64,
    pub pattern_gain: f64,
    pub fringe_gain: f64,
    pub foothill_gain: f64,
    pub drainage_gain: f64,
    pub texture_gain: f64,
    pub smoothing_px: f64,
    pub seed_offset: i64,
}

/// `STYLES[0]` — arctic_plain (the reference style, matching the mountain template's
/// "use STYLES[0]" choice).
pub const ARCTIC_PLAIN: TundraStyle = TundraStyle {
    key: "arctic_plain",
    angle_rad: 0.10,
    plain_gain: 1.30,
    pattern_gain: 0.32,
    fringe_gain: 0.18,
    foothill_gain: 0.22,
    drainage_gain: 0.48,
    texture_gain: 0.22,
    smoothing_px: 5.0,
    seed_offset: 0,
};

/// Port of `tundra_synthesis.generate(..., apron_px=APRON_PX)` SEAM-SAFE path, returning
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
    style: &TundraStyle,
    feature_span_m: f64,
    apron_px: usize,
) -> Vec<f64> {
    assert_eq!(wx.len(), rows * cols, "wx len != rows*cols");
    assert_eq!(wz.len(), rows * cols, "wz len != rows*cols");
    let n = rows * cols;
    let feature_span = feature_span_m.max(1.0);
    // sseed = seed + style.seed_offset
    let sseed = seed + style.seed_offset;

    // --- top-level recursive domain warp, then pointwise macro / rotated-pattern fields ---
    // Python: w_x, w_z = recursive_domain_warp(wx, wz, span*0.020, 1/(span*0.86),
    //         sseed+10, 3, 0.54, 1.72)
    // We materialise w_x/w_z because the pattern/stripe/foothill fields ROTATE them
    // (about cx=cz=0) and the fringe field reuses them unrotated.
    let mut macro_field = vec![0.0_f64; n]; // `macro` (reserved word in Rust)
    let mut polygons = vec![0.0_f64; n];
    let mut stripes = vec![0.0_f64; n];
    let mut fringe_ridges = vec![0.0_f64; n];
    let mut foothills = vec![0.0_f64; n];
    let mut fine = vec![0.0_f64; n];
    for i in 0..n {
        let (w_x, w_z) = h::recursive_domain_warp(
            wx[i],
            wz[i],
            feature_span * 0.020,
            1.0 / (feature_span * 0.86),
            sseed + 10,
            3,
            0.54,
            1.72,
        );

        // macro = clip(affine_remap(fbm(w_x,w_z, 1/(span*0.94),5,sseed+30,gain=0.58), MACRO), 0, 1)
        let macro_raw = h::fbm(w_x, w_z, 1.0 / (feature_span * 0.94), 5, sseed + 30, 0.58);
        macro_field[i] = h::clip(h::affine_remap(macro_raw, MACRO_CENTER, MACRO_SCALE), 0.0, 1.0);

        // rx, rz = _rotated(w_x, w_z, angle, cx=0, cz=0)  (seam-safe fixed centre)
        let (rx, rz) = h::rotated(w_x, w_z, style.angle_rad, 0.0, 0.0);

        // polygons = cellular_edges(rx, rz, 1/(span*0.030), sseed+70, sharpness=1.70)
        polygons[i] = recipe_noise::cellular_edges(rx, rz, 1.0 / (feature_span * 0.030), sseed + 70, 1.70);
        // stripes = ridged_multifractal(rx, rz*0.18, 1/(span*0.055), 4, sseed+90, gain=0.48)
        stripes[i] = h::ridged_multifractal(rx, rz * 0.18, 1.0 / (feature_span * 0.055), 4, sseed + 90, 0.48);

        // fringe_ridges = ridged_multifractal(w_x, w_z*0.48, 1/(span*0.16), 5, sseed+130, gain=0.52)
        // NOTE: UNROTATED warped coords (w_x/w_z), z anisotropy 0.48.
        fringe_ridges[i] =
            h::ridged_multifractal(w_x, w_z * 0.48, 1.0 / (feature_span * 0.16), 5, sseed + 130, 0.52);

        // foothills = smoothstep(0.40, 0.80, ridged_multifractal(rx, rz*0.48, 1/(span*0.22), 5, sseed+160, 0.52))
        // NOTE: ROTATED coords (rx/rz), z anisotropy 0.48, NO trailing blur.
        let foothills_raw =
            h::ridged_multifractal(rx, rz * 0.48, 1.0 / (feature_span * 0.22), 5, sseed + 160, 0.52);
        foothills[i] = h::smoothstep(0.40, 0.80, foothills_raw);

        // fine = affine_remap(fbm(w_x,w_z, 1/(span*0.026),3,sseed+220,gain=0.44), FINE)
        let fine_raw = h::fbm(w_x, w_z, 1.0 / (feature_span * 0.026), 3, sseed + 220, 0.44);
        fine[i] = h::affine_remap(fine_raw, FINE_CENTER, FINE_SCALE);
    }

    // plain = smoothstep(0.36, 0.76, gaussian(1.0 - abs(macro - 0.46), sigma=5.8, nearest))
    let mut plain_inner = vec![0.0_f64; n];
    for i in 0..n {
        plain_inner[i] = 1.0 - (macro_field[i] - 0.46).abs();
    }
    let plain_blur = array_ops::gaussian_filter_nearest(&plain_inner, rows, cols, 5.8, h::TRUNCATE);
    let mut plain = vec![0.0_f64; n];
    for i in 0..n {
        plain[i] = h::smoothstep(0.36, 0.76, plain_blur[i]);
    }

    // pattern = smoothstep(0.46, 0.86, gaussian(0.56*polygons + 0.44*stripes, sigma=1.2, nearest)) * plain
    let mut pattern_inner = vec![0.0_f64; n];
    for i in 0..n {
        pattern_inner[i] = 0.56 * polygons[i] + 0.44 * stripes[i];
    }
    let pattern_blur = array_ops::gaussian_filter_nearest(&pattern_inner, rows, cols, 1.2, h::TRUNCATE);
    let mut pattern = vec![0.0_f64; n];
    for i in 0..n {
        pattern[i] = h::smoothstep(0.46, 0.86, pattern_blur[i]) * plain[i];
    }

    // fringe = smoothstep(0.42, 0.84, gaussian(fringe_ridges, sigma=1.8, nearest))
    let fringe_blur = array_ops::gaussian_filter_nearest(&fringe_ridges, rows, cols, 1.8, h::TRUNCATE);
    let mut fringe = vec![0.0_f64; n];
    for i in 0..n {
        fringe[i] = h::smoothstep(0.42, 0.84, fringe_blur[i]);
    }

    // flow_source_inner = 0.62*macro + 0.26*foothills + 0.22*fringe - 0.22*plain
    // flow_source = affine_remap(flow_source_inner, FLOW_SOURCE)
    let mut flow_source = vec![0.0_f64; n];
    for i in 0..n {
        let inner = 0.62 * macro_field[i] + 0.26 * foothills[i] + 0.22 * fringe[i] - 0.22 * plain[i];
        flow_source[i] = h::affine_remap(inner, FLOW_SOURCE_CENTER, FLOW_SOURCE_SCALE);
    }
    // channels = _drainage_channels_seam_safe(flow_source, power=0.48)
    //   == shared helper: pre-blur sigma=1.15, MFD power=0.48, fixed-max log1p norm,
    //      spread blur sigma=max(width_px,0.1)=2.0, clip [0,1].
    // trailing S_REF -> sigma_cells identity (tundra isn't level-anchored yet; byte-identical).
    let channels = h::flow_channels_seam_safe(&flow_source, rows, cols, 2.0, 0.48, h::S_REF);
    // drainage = smoothstep(0.58, 0.94, channels)
    let mut drainage = vec![0.0_f64; n];
    for i in 0..n {
        drainage[i] = h::smoothstep(0.58, 0.94, channels[i]);
    }

    // base_inner = 0.74*macro + 0.26*foothills
    // base = gaussian(affine_remap(base_inner, BASE), sigma=smoothing_px, nearest)
    let mut base_inner = vec![0.0_f64; n];
    for i in 0..n {
        let inner = 0.74 * macro_field[i] + 0.26 * foothills[i];
        base_inner[i] = h::affine_remap(inner, BASE_CENTER, BASE_SCALE);
    }
    let base = array_ops::gaussian_filter_nearest(&base_inner, rows, cols, style.smoothing_px, h::TRUNCATE);

    // --- assemble height ---
    // macro_zsc = affine_remap(macro, MACRO_ZSCORE)
    // height  = 0.24 * plain_gain * macro_zsc
    // height += 0.10 * pattern_gain * pattern
    // height += 0.34 * fringe_gain * fringe
    // height += 0.40 * foothill_gain * foothills
    // height -= 0.22 * drainage_gain * drainage
    // height += 0.045 * texture_gain * fine * (0.45 + 0.55*pattern)
    // height = 0.72*height + 0.28*base
    let mut height = vec![0.0_f64; n];
    for i in 0..n {
        let macro_zsc = h::affine_remap(macro_field[i], MACRO_ZSCORE_CENTER, MACRO_ZSCORE_SCALE);
        let mut hv = 0.24 * style.plain_gain * macro_zsc;
        hv += 0.10 * style.pattern_gain * pattern[i];
        hv += 0.34 * style.fringe_gain * fringe[i];
        hv += 0.40 * style.foothill_gain * foothills[i];
        hv -= 0.22 * style.drainage_gain * drainage[i];
        hv += 0.045 * style.texture_gain * fine[i] * (0.45 + 0.55 * pattern[i]);
        hv = 0.72 * hv + 0.28 * base[i];
        height[i] = hv;
    }

    // --- final blend (seam-safe) ---
    // final_blend = 0.86*height + 0.14*gaussian(height, sigma=1.1, nearest)
    // height = affine_remap(final_blend, FINAL)
    let height_blur11 = array_ops::gaussian_filter_nearest(&height, rows, cols, 1.1, h::TRUNCATE);
    for i in 0..n {
        let final_blend = 0.86 * height[i] + 0.14 * height_blur11[i];
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

/// Public entry point: TUNDRA seam-safe height, core-cropped. Uses `STYLES[0]`
/// (arctic_plain). Mirrors `tundra_synthesis.generate(...)["height"]`.
#[allow(clippy::too_many_arguments)]
pub fn tundra_seamsafe(
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
        &ARCTIC_PLAIN,
        feature_span_m,
        apron_px,
    )
}
