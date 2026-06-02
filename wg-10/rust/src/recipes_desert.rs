// WIRE: add mod recipes_desert; + test mod
//! DESERT biome recipe — seam-safe (apron_px > 0) path, ported bit-close (f64) from
//! `tools/dem_pack/desert_synthesis.py::generate`. Follows the MOUNTAIN template
//! (`recipes.rs`) exactly: shared math comes from [`crate::recipes::helpers`]; only the
//! desert-specific constants, sub-fields (dunes / yardangs / mesas / basins / playas /
//! washes) and the assembly pipeline live here.
//!
//! Parity contract: `desert_seamsafe(...)` reproduces the Python core-cropped height
//! within a tight epsilon, verified against the committed fixture
//! `tools/dem_pack/fixtures/recipe_desert_fixture.json` (`recipes_desert_tests.rs`).
//!
//! Desert-specific notes vs the mountain/grassland template:
//!   * `recursive_domain_warp` uses freq_mul = 1.78 (mountain 1.75, grassland 1.70). The
//!     warp call also uses `steps=3, decay=0.52` and seed offset `+10`.
//!   * The wash carve uses desert's flow power 0.43 and a FIXED spread sigma 1.8
//!     (passed as `width_px` to `helpers::flow_channels_seam_safe`; 1.8 > 0.1 so the
//!     helper's `.max(0.1)` is a no-op — bit-identical to the Python `sigma=1.8`). The
//!     pre-blur sigma=1.15 inside the helper matches desert's `_wash_channels_seam_safe`.
//!   * `_dune_field` uses a fixed rotation about origin (cx=cz=0), an fbm warp, TWO crest
//!     terms, a power-shaping by `1.0 + 1.8*dune_width`, a gaussian sigma=0.70 blur, then
//!     affine_remap(DUNE)+clip. The blur runs on the WHOLE field (whole-array gaussian).
//!   * `_yardang_field` uses `ridged_multifractal` with the recipe default weight_gain=1.35
//!     (helper default), anisotropy on rz, and a smoothstep(0.42,0.86) gate.
//!   * `block_edges` uses `cellular_edges(..., sharpness=1.25)` (NOT the Python default 2.0)
//!     about a rotation angle `style.angle_rad + 0.78`.
//!   * STYLES[0] is `dune_sea`. dune_gain etc. are read straight from it.
//!   * Flow tie-ordering: desert basins/playas CAN be flat, so the surface fed to the wash
//!     MFD may contain EXACTLY-equal cells. The MFD downhill test is strict (`drop > 0`),
//!     so tied cells never flow to each other; the Rust stable-sort vs numpy quicksort tie
//!     order changes the result by at most ~1e-16 (same caveat as
//!     `array_ops::flow_accumulation_mfd`).

#![allow(dead_code)]

use crate::recipe_noise;
use crate::recipes::helpers as h;
use crate::array_ops;

// ---------------------------------------------------------------------------
// Apron constant — DESERT_APRON_PX. See Python docstring for the derivation.
// ---------------------------------------------------------------------------
pub const APRON_PX: usize = 160;

// ---------------------------------------------------------------------------
// Affine-remap constants (replace per-window zscore / norm01). Mirror of the
// DESERT_*_CENTER / DESERT_*_SCALE module constants in desert_synthesis.py.
// ---------------------------------------------------------------------------
pub const REGIONAL_CENTER: f64 = -0.668;
pub const REGIONAL_SCALE: f64 = 0.716;

pub const DUNE_CENTER: f64 = 0.018;
pub const DUNE_SCALE: f64 = 1.596;

pub const YARDANG_CENTER: f64 = 0.001;
pub const YARDANG_SCALE: f64 = 1.093;

pub const BASE_CENTER: f64 = 0.113;
pub const BASE_SCALE: f64 = 2.312;

pub const FINE_CENTER: f64 = 0.000;
pub const FINE_SCALE: f64 = 3.543;

pub const SALT_CENTER: f64 = 0.365;
pub const SALT_SCALE: f64 = 4.185;

pub const FINAL_CENTER: f64 = 0.000;
pub const FINAL_SCALE: f64 = 0.85;

/// Mirror of `DesertStyle` (only the fields the seam-safe pipeline reads).
#[derive(Clone, Copy, Debug)]
pub struct DesertStyle {
    pub key: &'static str,
    pub angle_rad: f64,
    pub dune_gain: f64,
    pub yardang_gain: f64,
    pub wash_gain: f64,
    pub mesa_gain: f64,
    pub playa_gain: f64,
    pub basin_gain: f64,
    pub dune_spacing_m: f64,
    pub dune_width: f64,
    pub yardang_anisotropy: f64,
    pub floor_smooth_px: f64,
    pub detail_gain: f64,
    pub seed_offset: i64,
}

/// `STYLES[0]` — dune_sea (the desert reference style).
pub const DUNE_SEA: DesertStyle = DesertStyle {
    key: "dune_sea",
    angle_rad: 0.48,
    dune_gain: 1.42,
    yardang_gain: 0.28,
    wash_gain: 0.34,
    mesa_gain: 0.20,
    playa_gain: 0.52,
    basin_gain: 0.92,
    dune_spacing_m: 2400.0,
    dune_width: 0.36,
    yardang_anisotropy: 0.30,
    floor_smooth_px: 5.2,
    detail_gain: 0.24,
    seed_offset: 0,
};

/// Mirror of `_dune_field(..., seam_safe_mode=True, blur_mode='nearest')` returning the
/// WHOLE field. Rotation centre is fixed at the world origin (cx=cz=0) — seam-safe.
///
/// `wx`/`wz` here are the ALREADY domain-warped grids (`generate` passes `w_x`/`w_z`).
/// pointwise dune ridges -> nearest gaussian blur sigma=0.70 -> affine_remap(DUNE)+clip.
fn dune_field(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    feature_span_m: f64,
    style: &DesertStyle,
    seed: i64,
) -> Vec<f64> {
    let n = rows * cols;
    let pi = std::f64::consts::PI;
    let spacing = style.dune_spacing_m.max(1.0);
    let secondary_spacing = (style.dune_spacing_m * 1.75).max(1.0);
    let mut raw = vec![0.0_f64; n];
    for i in 0..n {
        let (rx, rz) = h::rotated(wx[i], wz[i], style.angle_rad, 0.0, 0.0);
        let warp = h::fbm(wx[i], wz[i], 1.0 / (feature_span_m * 0.20), 4, seed + 120, 0.52)
            * style.dune_spacing_m
            * 0.72;
        let phase = (rx + warp) / spacing * pi * 2.0;
        let crest = 1.0 - phase.sin().abs();
        let secondary = 1.0
            - ((rx * 0.62 + rz * 0.16 + warp * 0.35) / secondary_spacing * pi * 2.0)
                .sin()
                .abs();
        let base = h::clip(0.78 * crest + 0.22 * secondary, 0.0, 1.0);
        raw[i] = base.powf(1.0 + 1.8 * style.dune_width);
    }
    let blurred = array_ops::gaussian_filter_nearest(&raw, rows, cols, 0.70, h::TRUNCATE);
    blurred
        .iter()
        .map(|&v| h::clip(h::affine_remap(v, DUNE_CENTER, DUNE_SCALE), 0.0, 1.0))
        .collect()
}

/// Mirror of `_yardang_field(..., seam_safe_mode=True)` returning the WHOLE field.
/// Rotation centre is fixed at the world origin (cx=cz=0) — seam-safe. No blur.
fn yardang_field(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    feature_span_m: f64,
    style: &DesertStyle,
    seed: i64,
) -> Vec<f64> {
    let n = rows * cols;
    let mut out = vec![0.0_f64; n];
    for i in 0..n {
        let (rx, rz) = h::rotated(wx[i], wz[i], style.angle_rad, 0.0, 0.0);
        let ridges = h::ridged_multifractal(
            rx,
            rz * style.yardang_anisotropy,
            1.0 / (feature_span_m * 0.075),
            5,
            seed + 210,
            0.50,
        );
        let fine = h::ridged_multifractal(
            rx + 0.22 * rz,
            rz * 0.18,
            1.0 / (feature_span_m * 0.038),
            3,
            seed + 230,
            0.46,
        );
        let combo = 0.72 * ridges + 0.28 * fine;
        out[i] = h::smoothstep(
            0.42,
            0.86,
            h::clip(h::affine_remap(combo, YARDANG_CENTER, YARDANG_SCALE), 0.0, 1.0),
        );
    }
    out
}

/// Mirror of `_wash_channels_seam_safe(surface, mode='nearest', power=0.43)`.
///
/// pre-blur sigma=1.15 -> MFD flow accumulation (power=0.43) -> FIXED-max
/// log1p/log1p(size) normalize -> spread blur sigma=1.8. This is exactly
/// `helpers::flow_channels_seam_safe` with width_px=1.8 (the helper's `.max(0.1)` is a
/// no-op since 1.8 > 0.1).
#[inline]
fn wash_channels_seam_safe(surface: &[f64], rows: usize, cols: usize) -> Vec<f64> {
    h::flow_channels_seam_safe(surface, rows, cols, 1.8, 0.43)
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
    style: &DesertStyle,
    feature_span_m: f64,
    apron_px: usize,
) -> Vec<f64> {
    assert_eq!(wx.len(), rows * cols, "wx len != rows*cols");
    assert_eq!(wz.len(), rows * cols, "wz len != rows*cols");
    let n = rows * cols;
    let feature_span = feature_span_m.max(1.0);
    let sseed = seed + style.seed_offset;

    // --- pointwise: recursive domain warp (freq_mul=1.78), then regional fbm ---
    // and capture the warped coords w_x/w_z for downstream sub-fields.
    // Python: w_x, w_z = recursive_domain_warp(wx, wz, span*0.030, 1/(span*0.72),
    //         sseed+10, 3, 0.52, 1.78)
    let mut w_x = vec![0.0_f64; n];
    let mut w_z = vec![0.0_f64; n];
    let mut regional = vec![0.0_f64; n];
    for i in 0..n {
        let (wx_w, wz_w) = h::recursive_domain_warp(
            wx[i],
            wz[i],
            feature_span * 0.030,
            1.0 / (feature_span * 0.72),
            sseed + 10,
            3,
            0.52,
            1.78,
        );
        w_x[i] = wx_w;
        w_z[i] = wz_w;
        // regional = clip(affine_remap(fbm(w_x,w_z, 1/(span*0.86),5,sseed+30,gain=0.58)), 0,1)
        let reg = h::fbm(wx_w, wz_w, 1.0 / (feature_span * 0.86), 5, sseed + 30, 0.58);
        regional[i] = h::clip(h::affine_remap(reg, REGIONAL_CENTER, REGIONAL_SCALE), 0.0, 1.0);
    }

    // --- basin = smoothstep(0.34, 0.78, 1 - gaussian(regional, sigma=6.2)) ---
    let regional_blur = array_ops::gaussian_filter_nearest(&regional, rows, cols, 6.2, h::TRUNCATE);
    let mut basin = vec![0.0_f64; n];
    for i in 0..n {
        basin[i] = h::smoothstep(0.34, 0.78, 1.0 - regional_blur[i]);
    }

    // --- playa = smoothstep(0.56, 0.90, gaussian(basin, sigma=5.0)) ---
    let basin_blur = array_ops::gaussian_filter_nearest(&basin, rows, cols, 5.0, h::TRUNCATE);
    let mut playa = vec![0.0_f64; n];
    for i in 0..n {
        playa[i] = h::smoothstep(0.56, 0.90, basin_blur[i]);
    }

    // --- dunes / yardangs (whole-field sub-pipelines on warped coords) ---
    let dunes = dune_field(&w_x, &w_z, rows, cols, feature_span, style, sseed);
    let yardangs = yardang_field(&w_x, &w_z, rows, cols, feature_span, style, sseed);

    // --- block_cores / mesa_blocks / rocky_relief / mesas ---
    // rot uses angle_rad + 0.78 about fixed origin. block_edges = cellular_edges(rx,rz,
    //   1/(span*0.210), sseed+310, sharpness=1.25). block_cores = smoothstep(0.22,0.76,
    //   gaussian(1 - block_edges, sigma=3.2)).
    let mut one_minus_block_edges = vec![0.0_f64; n];
    // also need rocky_relief which uses the same rotated rx/rz -> compute in this loop.
    let mut rocky_relief = vec![0.0_f64; n];
    for i in 0..n {
        let (rx, rz) = h::rotated(w_x[i], w_z[i], style.angle_rad + 0.78, 0.0, 0.0);
        let block_edges =
            recipe_noise::cellular_edges(rx, rz, 1.0 / (feature_span * 0.210), sseed + 310, 1.25);
        one_minus_block_edges[i] = 1.0 - block_edges;
        // rocky_relief = smoothstep(0.36, 0.84, ridged_multifractal(rx, rz*0.42,
        //   1/(span*0.18), 4, sseed+330, gain=0.52))
        let rr = h::ridged_multifractal(
            rx,
            rz * 0.42,
            1.0 / (feature_span * 0.18),
            4,
            sseed + 330,
            0.52,
        );
        rocky_relief[i] = h::smoothstep(0.36, 0.84, rr);
    }
    let block_cores_blur =
        array_ops::gaussian_filter_nearest(&one_minus_block_edges, rows, cols, 3.2, h::TRUNCATE);
    let mut block_cores = vec![0.0_f64; n];
    for i in 0..n {
        block_cores[i] = h::smoothstep(0.22, 0.76, block_cores_blur[i]);
    }
    // mesa_blocks = smoothstep(0.52, 0.82, gaussian(regional, sigma=2.2)) * block_cores
    //               * (1 - 0.68*basin)
    let regional_blur22 = array_ops::gaussian_filter_nearest(&regional, rows, cols, 2.2, h::TRUNCATE);
    let mut mesas = vec![0.0_f64; n];
    for i in 0..n {
        let mesa_blocks =
            h::smoothstep(0.52, 0.82, regional_blur22[i]) * block_cores[i] * (1.0 - 0.68 * basin[i]);
        // mesas = clip(0.68*mesa_blocks + 0.32*rocky_relief*(1 - 0.42*basin), 0, 1)
        mesas[i] = h::clip(
            0.68 * mesa_blocks + 0.32 * rocky_relief[i] * (1.0 - 0.42 * basin[i]),
            0.0,
            1.0,
        );
    }

    // --- base_surface = affine_remap(0.72*regional + 0.24*mesas - 0.62*basin, BASE) ---
    let mut base_surface = vec![0.0_f64; n];
    for i in 0..n {
        let inner = 0.72 * regional[i] + 0.24 * mesas[i] - 0.62 * basin[i];
        base_surface[i] = h::affine_remap(inner, BASE_CENTER, BASE_SCALE);
    }

    // --- washes (walrus reassignments) ---
    // washes = _wash_channels_seam_safe(base_surface + 0.16*mesas, power=0.43)
    let mut wash_surface = vec![0.0_f64; n];
    for i in 0..n {
        wash_surface[i] = base_surface[i] + 0.16 * mesas[i];
    }
    let mut washes = wash_channels_seam_safe(&wash_surface, rows, cols);
    // washes = smoothstep(0.57, 0.94, washes) * (0.35 + 0.65*(1 - playa))
    for i in 0..n {
        let w = h::smoothstep(0.57, 0.94, washes[i]);
        washes[i] = w * (0.35 + 0.65 * (1.0 - playa[i]));
    }

    // --- fine + salt: pointwise on warped coords ---
    let mut fine = vec![0.0_f64; n];
    let mut salt = vec![0.0_f64; n];
    for i in 0..n {
        let fv = h::fbm(w_x[i], w_z[i], 1.0 / (feature_span * 0.018), 4, sseed + 410, 0.48);
        fine[i] = h::affine_remap(fv, FINE_CENTER, FINE_SCALE);
        let sv = h::ridged_multifractal(
            w_x[i],
            w_z[i],
            1.0 / (feature_span * 0.025),
            3,
            sseed + 430,
            0.42,
        );
        salt[i] = h::affine_remap(sv, SALT_CENTER, SALT_SCALE);
    }

    // --- masks (shared with both paths) ---
    // sand_mask = clip((0.42 + 0.58*basin) * (1 - 0.42*mesas), 0, 1)
    // dune_mask = dunes * sand_mask * (0.25 + 0.75*basin)
    // yardang_mask = yardangs * (0.45 + 0.55*basin) * (1 - 0.35*dune_mask)
    // wash_mask = washes * (0.45 + 0.55*(1 - basin + 0.35*mesas))
    // playa_mask = playa * (1 - 0.45*dune_mask)
    let mut dune_relief = vec![0.0_f64; n];
    let mut yardang_relief = vec![0.0_f64; n];
    let mut wash_relief = vec![0.0_f64; n];
    let mut playa_relief = vec![0.0_f64; n];
    let mut mesa_relief = vec![0.0_f64; n];
    let mut height = vec![0.0_f64; n];
    for i in 0..n {
        let sand_mask = h::clip((0.42 + 0.58 * basin[i]) * (1.0 - 0.42 * mesas[i]), 0.0, 1.0);
        let dune_mask = dunes[i] * sand_mask * (0.25 + 0.75 * basin[i]);
        let yardang_mask = yardangs[i] * (0.45 + 0.55 * basin[i]) * (1.0 - 0.35 * dune_mask);
        let wash_mask = washes[i] * (0.45 + 0.55 * (1.0 - basin[i] + 0.35 * mesas[i]));
        let playa_mask = playa[i] * (1.0 - 0.45 * dune_mask);

        let d_relief = dune_mask * style.dune_gain;
        let y_relief = yardang_mask * style.yardang_gain;
        let w_relief = wash_mask * style.wash_gain;
        let p_relief = playa_mask * style.playa_gain;
        let m_relief = mesas[i] * style.mesa_gain;

        // --- assemble height ---
        // height  = base_surface
        // height += basin_gain * 0.24 * (1 - basin)
        // height += 0.50*mesa_relief + 0.14*mesa_relief*fine
        // height += 0.44*dune_relief + 0.10*dune_relief*fine
        // height += 0.34*yardang_relief + 0.08*yardang_relief*salt
        // height -= 0.36*wash_relief
        // height -= 0.38*playa_relief
        // height += detail_gain * (0.08 + 0.12*mesas + 0.12*yardang_mask) * fine
        let mut hv = base_surface[i];
        hv += style.basin_gain * 0.24 * (1.0 - basin[i]);
        hv += 0.50 * m_relief + 0.14 * m_relief * fine[i];
        hv += 0.44 * d_relief + 0.10 * d_relief * fine[i];
        hv += 0.34 * y_relief + 0.08 * y_relief * salt[i];
        hv -= 0.36 * w_relief;
        hv -= 0.38 * p_relief;
        hv += style.detail_gain * (0.08 + 0.12 * mesas[i] + 0.12 * yardang_mask) * fine[i];

        dune_relief[i] = d_relief;
        yardang_relief[i] = y_relief;
        wash_relief[i] = w_relief;
        playa_relief[i] = p_relief;
        mesa_relief[i] = m_relief;
        height[i] = hv;
    }

    // --- floor blend ---
    // floor_mask = clip(0.68*playa_relief + 0.46*basin + 0.34*wash_relief, 0, 1)
    // smooth_floor = gaussian(height, sigma=max(floor_smooth_px, 0.2))
    // height = height*(1 - 0.34*floor_mask) + smooth_floor*(0.34*floor_mask)
    let smooth_floor = array_ops::gaussian_filter_nearest(
        &height,
        rows,
        cols,
        style.floor_smooth_px.max(0.2),
        h::TRUNCATE,
    );
    for i in 0..n {
        let floor_mask = h::clip(
            0.68 * playa_relief[i] + 0.46 * basin[i] + 0.34 * wash_relief[i],
            0.0,
            1.0,
        );
        height[i] = height[i] * (1.0 - 0.34 * floor_mask) + smooth_floor[i] * (0.34 * floor_mask);
    }

    // --- final blend (seam-safe) ---
    // final_blend = 0.82*height + 0.18*gaussian(height, sigma=0.95)
    // height = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE)
    let height_blur = array_ops::gaussian_filter_nearest(&height, rows, cols, 0.95, h::TRUNCATE);
    for i in 0..n {
        let final_blend = 0.82 * height[i] + 0.18 * height_blur[i];
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

/// Public entry point: DESERT seam-safe height, core-cropped. Uses `STYLES[0]`
/// (dune_sea). Signature matches the task contract.
#[allow(clippy::too_many_arguments)]
pub fn desert_seamsafe(
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
        &DUNE_SEA,
        feature_span_m,
        apron_px,
    )
}
