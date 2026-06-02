// WIRE: add mod recipes_karst; + test mod
//
//! KARST biome — seam-safe path, ported bit-close (f64) from
//! `tools/dem_pack/karst_synthesis.py::generate` (`apron_px > 0` branch).
//!
//! Follows the MOUNTAIN template (`crate::recipes::mountain`) exactly: this module
//! REUSES the shared `crate::recipes::helpers` (affine_remap, smoothstep, clip,
//! rotated, flow_channels_seam_safe, apron_meshgrid, the fbm / ridged_multifractal /
//! recursive_domain_warp wrappers) and the fixture-proven whole-array operators in
//! [`crate::array_ops`] + per-point noise in [`crate::recipe_noise`].
//!
//! Karst has the most sub-fields of any biome: a warped regional plateau, residual
//! TOWERS (cone + local, blurred sparse), DOLINE bowls, LINEAMENT control lines, a
//! cellular network, a COCKPIT depression field, real MFD dry-valley drainage, plus
//! fine + karren detail. Each sub-field's data-dependent zscore / norm01 is replaced
//! by `affine_remap` with the KARST_* constants below (the seam-safe contract), and
//! every `gaussian_filter` is `mode='nearest'`.
//!
//! Parity contract: `karst_seamsafe(...)` reproduces the Python core-cropped height
//! within a tight epsilon, verified against the committed fixture
//! `tools/dem_pack/fixtures/recipe_karst_fixture.json` (`recipes_karst_tests.rs`).

// Consumed by the (not-yet-wired) producer seam; until then exercised only by the test.
#![allow(dead_code)]

use crate::array_ops;
use crate::recipe_noise;
use crate::recipes::helpers as h;

// ---- apron constant ---------------------------------------------------------
/// `KARST_APRON_PX` — apron-padding the caller must supply (see Python module docstring).
pub const APRON_PX: usize = 160;

// ---- affine-remap constants (replace per-window zscore / norm01) ------------
// regional fbm raw (norm01)
const REGIONAL_CENTER: f64 = -0.673;
const REGIONAL_SCALE: f64 = 0.679;
// tower cone+local combo (norm01 inside _tower_field)
const TOWER_CONE_CENTER: f64 = 0.0005;
const TOWER_CONE_SCALE: f64 = 1.104;
// tower blurred-sparse output (norm01 at end of _tower_field)
const TOWER_FINAL_CENTER: f64 = 0.00;
const TOWER_FINAL_SCALE: f64 = 1.437;
// doline pits combo (norm01 inside _doline_field)
const DOLINE_PITS_CENTER: f64 = 0.0003;
const DOLINE_PITS_SCALE: f64 = 1.082;
// doline blurred-bowls output (norm01 at end of _doline_field)
const DOLINE_BOWLS_CENTER: f64 = 0.00;
const DOLINE_BOWLS_SCALE: f64 = 4.274;
// lineament combo (norm01 on 0.68*lineA + 0.32*lineB)
const LINEAMENT_CENTER: f64 = 0.001;
const LINEAMENT_SCALE: f64 = 1.092;
// cockpit_noise fbm raw (norm01)
const COCKPIT_NOISE_CENTER: f64 = -0.880;
const COCKPIT_NOISE_SCALE: f64 = 0.565;
// cockpit combo (norm01 on 0.50*dolines + 0.26*(1-cellular) + 0.24*cockpit_noise)
const COCKPIT_CENTER: f64 = 0.072;
const COCKPIT_SCALE: f64 = 1.360;
// base raw (zscore on plateau_gain*(1.06*plateau + 0.18*regional))
const BASE_CENTER: f64 = 0.560;
const BASE_SCALE: f64 = 2.090;
// fine fbm raw (zscore)
const FINE_CENTER: f64 = 0.00;
const FINE_SCALE: f64 = 3.539;
// karren ridged raw (zscore)
const KARREN_CENTER: f64 = 0.356;
const KARREN_SCALE: f64 = 4.257;
// final height before trailing zscore (affine replaces trailing zscore)
const FINAL_CENTER: f64 = 0.08;
const FINAL_SCALE: f64 = 0.964;

/// Mirror of `KarstStyle` (only the fields the seam-safe pipeline reads).
#[derive(Clone, Copy, Debug)]
pub struct KarstStyle {
    pub key: &'static str,
    pub angle_rad: f64,
    pub plateau_gain: f64,
    pub tower_gain: f64,
    pub cockpit_gain: f64,
    pub doline_gain: f64,
    pub valley_gain: f64,
    pub lineament_gain: f64,
    pub tower_width_px: f64,
    pub doline_width_px: f64,
    pub floor_smooth_px: f64,
    pub detail_gain: f64,
    pub anisotropy: f64,
    pub seed_offset: i64,
}

/// `STYLES[0]` — tower_karst (the karst reference style).
pub const TOWER_KARST: KarstStyle = KarstStyle {
    key: "tower_karst",
    angle_rad: 0.42,
    plateau_gain: 0.86,
    tower_gain: 1.45,
    cockpit_gain: 1.02,
    doline_gain: 0.82,
    valley_gain: 0.62,
    lineament_gain: 0.74,
    tower_width_px: 2.0,
    doline_width_px: 2.6,
    floor_smooth_px: 2.8,
    detail_gain: 0.54,
    anisotropy: 0.48,
    seed_offset: 0,
};

/// `ridged_multifractal` with the recipe default offset=1.0 but an EXPLICIT weight_gain
/// (the tower cone uses 1.62, not the helper's fixed 1.35). All other calls use 1.35,
/// available via `h::ridged_multifractal`.
#[inline]
fn ridged_mf_wg(wx: f64, wz: f64, base_freq: f64, octaves: u32, seed: i64, gain: f64, weight_gain: f64) -> f64 {
    recipe_noise::ridged_multifractal(wx, wz, base_freq, octaves, seed, gain, 2.0, 1.0, weight_gain)
}

/// Mirror of `_lineaments(..., seam_safe_mode=True)` for a single point.
/// Rotation centre fixed at world origin (cx=cz=0) — seam-safe.
fn lineaments_point(wx: f64, wz: f64, span: f64, style: &KarstStyle, seed: i64) -> f64 {
    let (rx, rz) = h::rotated(wx, wz, style.angle_rad, 0.0, 0.0);
    let line_a = h::ridged_multifractal(rx, rz * style.anisotropy, 1.0 / (span * 0.18), 4, seed + 100, 0.54);
    let line_b = h::ridged_multifractal(
        rx * 0.58 - rz * 0.32,
        rz * 0.58 + rx * 0.32,
        1.0 / (span * 0.11),
        3,
        seed + 130,
        0.48,
    );
    let combo = 0.68 * line_a + 0.32 * line_b;
    // seam-safe: smoothstep(0.46, 0.82, clip(affine_remap(combo, LINEAMENT), 0, 1))
    h::smoothstep(
        0.46,
        0.82,
        h::clip(h::affine_remap(combo, LINEAMENT_CENTER, LINEAMENT_SCALE), 0.0, 1.0),
    )
}

/// Mirror of `_tower_field(..., seam_safe_mode=True)`. Builds the per-point sparse field,
/// blurs `pow(sparse, 1.20)` (nearest, sigma=max(tower_width_px, 0.2)), then affine_remap.
/// Returns the whole field.
fn tower_field(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    span: f64,
    style: &KarstStyle,
    seed: i64,
) -> Vec<f64> {
    let n = rows * cols;
    let mut sparse_pow = vec![0.0_f64; n];
    for i in 0..n {
        let cone = ridged_mf_wg(wx[i], wz[i], 1.0 / (span * 0.055), 5, seed + 210, 0.52, 1.62);
        let local = h::ridged_multifractal(wx[i], wz[i], 1.0 / (span * 0.026), 3, seed + 240, 0.45);
        let combo = 0.78 * cone + 0.22 * local;
        let sparse = h::smoothstep(
            0.46,
            0.84,
            h::clip(h::affine_remap(combo, TOWER_CONE_CENTER, TOWER_CONE_SCALE), 0.0, 1.0),
        );
        sparse_pow[i] = sparse.powf(1.20);
    }
    let towers = array_ops::gaussian_filter_nearest(&sparse_pow, rows, cols, style.tower_width_px.max(0.2), h::TRUNCATE);
    let mut out = vec![0.0_f64; n];
    for i in 0..n {
        out[i] = h::clip(h::affine_remap(towers[i], TOWER_FINAL_CENTER, TOWER_FINAL_SCALE), 0.0, 1.0);
    }
    out
}

/// Mirror of `_doline_field(..., seam_safe_mode=True)`. pits_b uses skewed coords
/// `wx + 0.31*wz, wz - 0.17*wx`. Blurs `pow(pits, 1.45)` (nearest, sigma=max(doline_width_px, 0.2)),
/// then affine_remap. Returns the whole field.
fn doline_field(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    span: f64,
    style: &KarstStyle,
    seed: i64,
) -> Vec<f64> {
    let n = rows * cols;
    let mut pits_pow = vec![0.0_f64; n];
    for i in 0..n {
        let pits_a = h::ridged_multifractal(wx[i], wz[i], 1.0 / (span * 0.040), 4, seed + 310, 0.50);
        let pits_b = h::ridged_multifractal(
            wx[i] + 0.31 * wz[i],
            wz[i] - 0.17 * wx[i],
            1.0 / (span * 0.022),
            3,
            seed + 330,
            0.46,
        );
        let combo = 0.66 * pits_a + 0.34 * pits_b;
        let pits = h::smoothstep(
            0.55,
            0.90,
            h::clip(h::affine_remap(combo, DOLINE_PITS_CENTER, DOLINE_PITS_SCALE), 0.0, 1.0),
        );
        pits_pow[i] = pits.powf(1.45);
    }
    let bowls = array_ops::gaussian_filter_nearest(&pits_pow, rows, cols, style.doline_width_px.max(0.2), h::TRUNCATE);
    let mut out = vec![0.0_f64; n];
    for i in 0..n {
        out[i] = h::clip(h::affine_remap(bowls[i], DOLINE_BOWLS_CENTER, DOLINE_BOWLS_SCALE), 0.0, 1.0);
    }
    out
}

/// Seam-safe CONNECTED dry-valley drainage. Mirror of `_dry_valleys_seam_safe(surface,
/// mode='nearest', power=0.54)`: pre-blur sigma=1.15, MFD accumulation (power), FIXED-max
/// log1p normalize, spread blur sigma=2.6 (nearest), clip [0,1].
///
/// This is identical to `helpers::flow_channels_seam_safe` EXCEPT the spread sigma is the
/// fixed karst value 2.6 (the mountain helper uses a per-style width). So it is built
/// inline here from the same fixture-proven building blocks.
fn dry_valleys_seamsafe(surface: &[f64], rows: usize, cols: usize, power: f64) -> Vec<f64> {
    let pre = array_ops::gaussian_filter_nearest(surface, rows, cols, 1.15, h::TRUNCATE);
    let acc = array_ops::flow_accumulation_mfd(&pre, rows, cols, power);
    let log_size = ((rows * cols) as f64).ln_1p();
    let mut discharge: Vec<f64> = acc.iter().map(|&a| h::clip(a.ln_1p() / log_size, 0.0, 1.0)).collect();
    discharge = array_ops::gaussian_filter_nearest(&discharge, rows, cols, 2.6, h::TRUNCATE);
    for v in discharge.iter_mut() {
        *v = h::clip(*v, 0.0, 1.0);
    }
    discharge
}

/// Port of `generate(..., apron_px=APRON_PX)` SEAM-SAFE path, returning the CORE-cropped
/// height (length `core_rows * core_cols`).
///
/// `wx`/`wz` are the apron-padded world-coord grids (flat row-major, length `rows*cols`);
/// `rows`/`cols` are the PADDED dimensions. `feature_span_m` MUST be the fixed CORE span
/// shared by adjacent windows (NOT derived from the padded extent). `apron_px` cells are
/// cropped off every side at the end.
#[allow(clippy::too_many_arguments)]
pub fn generate_seamsafe(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    seed: i64,
    style: &KarstStyle,
    feature_span_m: f64,
    apron_px: usize,
) -> Vec<f64> {
    assert_eq!(wx.len(), rows * cols, "wx len != rows*cols");
    assert_eq!(wz.len(), rows * cols, "wz len != rows*cols");
    let n = rows * cols;
    let feature_span = feature_span_m.max(1.0);
    // sseed = seed + style.seed_offset (tower_karst -> +0)
    let sseed = seed + style.seed_offset;

    // --- recursive domain warp (per point) -> warped coords w_x, w_z ---
    // recursive_domain_warp(wx, wz, span*0.035, 1/(span*0.62), sseed+10, 3, 0.55, 1.82)
    let mut w_x = vec![0.0_f64; n];
    let mut w_z = vec![0.0_f64; n];
    for i in 0..n {
        let (wxw, wzw) = h::recursive_domain_warp(
            wx[i],
            wz[i],
            feature_span * 0.035,
            1.0 / (feature_span * 0.62),
            sseed + 10,
            3,
            0.55,
            1.82,
        );
        w_x[i] = wxw;
        w_z[i] = wzw;
    }

    // --- regional = clip(affine_remap(fbm(w_x,w_z, 1/(span*0.74),5,sseed+30,0.56), REGIONAL), 0,1) ---
    let mut regional = vec![0.0_f64; n];
    for i in 0..n {
        let reg = h::fbm(w_x[i], w_z[i], 1.0 / (feature_span * 0.74), 5, sseed + 30, 0.56);
        regional[i] = h::clip(h::affine_remap(reg, REGIONAL_CENTER, REGIONAL_SCALE), 0.0, 1.0);
    }
    // plateau = smoothstep(0.30, 0.72, gaussian(regional, sigma=5.8))
    let regional_blur = array_ops::gaussian_filter_nearest(&regional, rows, cols, 5.8, h::TRUNCATE);
    let mut plateau = vec![0.0_f64; n];
    for i in 0..n {
        plateau[i] = h::smoothstep(0.30, 0.72, regional_blur[i]);
    }

    // --- towers / dolines / lineaments (whole-field sub-fields, warped coords) ---
    let towers = tower_field(&w_x, &w_z, rows, cols, feature_span, style, sseed);
    let dolines = doline_field(&w_x, &w_z, rows, cols, feature_span, style, sseed);
    let mut lineaments = vec![0.0_f64; n];
    for i in 0..n {
        lineaments[i] = lineaments_point(w_x[i], w_z[i], feature_span, style, sseed);
    }

    // --- cellular = gaussian(cellular_edges(w_x,w_z, 1/(span*0.145), sseed+160, 1.45), sigma=3.8) ---
    let mut cellular_raw = vec![0.0_f64; n];
    for i in 0..n {
        cellular_raw[i] =
            recipe_noise::cellular_edges(w_x[i], w_z[i], 1.0 / (feature_span * 0.145), sseed + 160, 1.45);
    }
    let cellular = array_ops::gaussian_filter_nearest(&cellular_raw, rows, cols, 3.8, h::TRUNCATE);

    // --- cockpit_noise = clip(affine_remap(fbm(w_x,w_z,1/(span*0.052),4,sseed+180,0.54), COCKPIT_NOISE),0,1) ---
    let mut cockpit_noise = vec![0.0_f64; n];
    for i in 0..n {
        let cn = h::fbm(w_x[i], w_z[i], 1.0 / (feature_span * 0.052), 4, sseed + 180, 0.54);
        cockpit_noise[i] = h::clip(h::affine_remap(cn, COCKPIT_NOISE_CENTER, COCKPIT_NOISE_SCALE), 0.0, 1.0);
    }
    // cockpit = smoothstep(0.52, 0.90, clip(affine_remap(0.50*dolines + 0.26*(1-cellular) + 0.24*cockpit_noise, COCKPIT),0,1))
    let mut cockpit = vec![0.0_f64; n];
    for i in 0..n {
        let inner = 0.50 * dolines[i] + 0.26 * (1.0 - cellular[i]) + 0.24 * cockpit_noise[i];
        cockpit[i] = h::smoothstep(
            0.52,
            0.90,
            h::clip(h::affine_remap(inner, COCKPIT_CENTER, COCKPIT_SCALE), 0.0, 1.0),
        );
    }

    // --- base = affine_remap(plateau_gain*(1.06*plateau + 0.18*regional), BASE) ---
    let mut base = vec![0.0_f64; n];
    for i in 0..n {
        let inner = style.plateau_gain * (1.06 * plateau[i] + 0.18 * regional[i]);
        base[i] = h::affine_remap(inner, BASE_CENTER, BASE_SCALE);
    }

    // --- dry_valleys = _dry_valleys_seam_safe(base - 0.30*lineaments - 0.10*dolines, power=0.54) ---
    let mut dv_surface = vec![0.0_f64; n];
    for i in 0..n {
        dv_surface[i] = base[i] - 0.30 * lineaments[i] - 0.10 * dolines[i];
    }
    let mut dry_valleys = dry_valleys_seamsafe(&dv_surface, rows, cols, 0.54);
    // dry_valleys = smoothstep(0.58, 0.92, dry_valleys)
    // dry_valleys = clip(dry_valleys * (0.72 + 0.28*valley_gain), 0, 1)
    let dv_scale = 0.72 + 0.28 * style.valley_gain;
    for i in 0..n {
        let s = h::smoothstep(0.58, 0.92, dry_valleys[i]);
        dry_valleys[i] = h::clip(s * dv_scale, 0.0, 1.0);
    }

    // --- masks ---
    // tower_mask = smoothstep(0.22,0.74,towers) * (0.50 + 0.50*plateau)
    // cockpit_mask = smoothstep(0.46,0.86,cockpit) * (0.35 + 0.65*plateau)
    // doline_mask = smoothstep(0.46,0.88,dolines) * (0.30 + 0.70*plateau)
    // lineament_mask = clip(lineament_gain * lineaments * (0.35 + 0.65*plateau), 0, 1)
    // tower_mask = tower_mask * (1 - 0.50*doline_mask) * (1 - 0.30*dry_valleys)
    let mut tower_mask = vec![0.0_f64; n];
    let mut cockpit_mask = vec![0.0_f64; n];
    let mut doline_mask = vec![0.0_f64; n];
    let mut lineament_mask = vec![0.0_f64; n];
    for i in 0..n {
        let pl = plateau[i];
        let tm = h::smoothstep(0.22, 0.74, towers[i]) * (0.50 + 0.50 * pl);
        cockpit_mask[i] = h::smoothstep(0.46, 0.86, cockpit[i]) * (0.35 + 0.65 * pl);
        doline_mask[i] = h::smoothstep(0.46, 0.88, dolines[i]) * (0.30 + 0.70 * pl);
        lineament_mask[i] = h::clip(style.lineament_gain * lineaments[i] * (0.35 + 0.65 * pl), 0.0, 1.0);
        tower_mask[i] = tm; // pre dependent-modulation; finalized below
    }
    // tower_mask depends on doline_mask + dry_valleys (computed above), apply in a pass.
    for i in 0..n {
        tower_mask[i] = tower_mask[i] * (1.0 - 0.50 * doline_mask[i]) * (1.0 - 0.30 * dry_valleys[i]);
    }

    // --- fine + karren detail (warped coords) ---
    let mut fine = vec![0.0_f64; n];
    let mut karren = vec![0.0_f64; n];
    for i in 0..n {
        let f = h::fbm(w_x[i], w_z[i], 1.0 / (feature_span * 0.018), 4, sseed + 410, 0.48);
        fine[i] = h::affine_remap(f, FINE_CENTER, FINE_SCALE);
        let k = h::ridged_multifractal(w_x[i], w_z[i], 1.0 / (feature_span * 0.016), 3, sseed + 430, 0.46);
        karren[i] = h::affine_remap(k, KARREN_CENTER, KARREN_SCALE);
    }

    // --- assemble height ---
    // height = base
    // height += tower_gain*(0.84*tower_mask + 0.20*tower_mask*karren)
    // height += lineament_gain*0.20*lineament_mask
    // height -= cockpit_gain*0.26*cockpit_mask
    // height -= doline_gain*0.72*doline_mask
    // height -= valley_gain*0.40*dry_valleys
    // height += detail_gain*(0.08 + 0.24*tower_mask + 0.10*lineament_mask)*fine
    let mut height = vec![0.0_f64; n];
    for i in 0..n {
        let tm = tower_mask[i];
        let mut hv = base[i];
        hv += style.tower_gain * (0.84 * tm + 0.20 * tm * karren[i]);
        hv += style.lineament_gain * 0.20 * lineament_mask[i];
        hv -= style.cockpit_gain * 0.26 * cockpit_mask[i];
        hv -= style.doline_gain * 0.72 * doline_mask[i];
        hv -= style.valley_gain * 0.40 * dry_valleys[i];
        hv += style.detail_gain * (0.08 + 0.24 * tm + 0.10 * lineament_mask[i]) * fine[i];
        height[i] = hv;
    }

    // --- floor blend ---
    // floor_mask = clip(0.72*doline_mask + 0.56*cockpit_mask + 0.48*dry_valleys, 0, 1)
    // smoothed_floor = gaussian(height, sigma=max(floor_smooth_px, 0.2))
    // height = height*(1 - 0.34*floor_mask) + smoothed_floor*(0.34*floor_mask)
    let mut floor_mask = vec![0.0_f64; n];
    for i in 0..n {
        floor_mask[i] = h::clip(0.72 * doline_mask[i] + 0.56 * cockpit_mask[i] + 0.48 * dry_valleys[i], 0.0, 1.0);
    }
    let smoothed_floor =
        array_ops::gaussian_filter_nearest(&height, rows, cols, style.floor_smooth_px.max(0.2), h::TRUNCATE);
    for i in 0..n {
        height[i] = height[i] * (1.0 - 0.34 * floor_mask[i]) + smoothed_floor[i] * (0.34 * floor_mask[i]);
    }

    // --- final blend (seam-safe) ---
    // final_blend = 0.80*height + 0.20*gaussian(height, sigma=0.95)
    // height = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE)
    let height_blur = array_ops::gaussian_filter_nearest(&height, rows, cols, 0.95, h::TRUNCATE);
    for i in 0..n {
        let final_blend = 0.80 * height[i] + 0.20 * height_blur[i];
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

/// Public entry point: KARST seam-safe height, core-cropped. Uses `STYLES[0]` (tower_karst).
///
/// `wx`/`wz` are apron-padded world-coord grids (flat row-major, PADDED `rows*cols`);
/// returns the inner core height (length `(rows-2*apron_px)*(cols-2*apron_px)`), exactly
/// like the Python `generate(...)["height"]`.
#[allow(clippy::too_many_arguments)]
pub fn karst_seamsafe(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    seed: i64,
    feature_span_m: f64,
    apron_px: usize,
) -> Vec<f64> {
    generate_seamsafe(wx, wz, rows, cols, seed, &TOWER_KARST, feature_span_m, apron_px)
}
