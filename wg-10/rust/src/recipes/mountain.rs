//! Mountain biome recipe composition.

use super::helpers as h;
use crate::array_ops;

// ---- apron constant -----------------------------------------------------
/// `MOUNTAIN_APRON_PX` — apron-padding the caller must supply (see Python docstring).
pub const APRON_PX: usize = 160;

// ---- affine-remap constants (replace per-window zscore / norm01) --------
pub const REGIONAL_CENTER: f64 = -0.50;
pub const REGIONAL_SCALE: f64 = 1.00;
pub const RIDGES_CENTER: f64 = 0.10;
pub const RIDGES_SCALE: f64 = 1.15;
pub const MASSIF_CENTER: f64 = 0.12;
pub const MASSIF_SCALE: f64 = 0.72;
pub const BASE_CENTER: f64 = 0.83;
pub const BASE_SCALE: f64 = 2.28;
pub const RANGES_ZSCORE_CENTER: f64 = 0.42;
pub const RANGES_ZSCORE_SCALE: f64 = 7.00;
pub const RIDGE_DETAIL_CENTER: f64 = 0.31;
pub const RIDGE_DETAIL_SCALE: f64 = 4.85;
pub const NEAR_DETAIL_CENTER: f64 = 0.00;
pub const NEAR_DETAIL_SCALE: f64 = 3.60;
pub const FINAL_CENTER: f64 = 0.00;
pub const FINAL_SCALE: f64 = 0.80;

// ---- LOOK levers (seam-safe path only) ----------------------------------
pub const PRIMARY_THRESH_LO: f64 = 0.26;
pub const PRIMARY_THRESH_HI: f64 = 0.40;
pub const TRIBUTARY_THRESH_LO: f64 = 0.24;
pub const TRIBUTARY_THRESH_HI: f64 = 0.40;
pub const SEAMSAFE_CARVE_GAIN: f64 = 2.00;
pub const SEAMSAFE_BRANCH_GAIN: f64 = 1.70;
pub const SEAMSAFE_RIDGE_GAIN: f64 = 1.12;
pub const SEAMSAFE_DETAIL_GAIN: f64 = 1.05;

/// Mirror of `MountainStyle` (only the fields the seam-safe pipeline reads).
#[derive(Clone, Copy, Debug)]
pub struct MountainStyle {
    pub key: &'static str,
    pub angle_rad: f64,
    pub uplift_gain: f64,
    pub ridge_gain: f64,
    pub carve_gain: f64,
    pub branch_gain: f64,
    pub valley_width_px: f64,
    pub floor_smooth_px: f64,
    pub detail_gain: f64,
    pub anisotropy: f64,
}

/// `STYLES[0]` — alpine_branching (the template's reference style).
pub const ALPINE_BRANCHING: MountainStyle = MountainStyle {
    key: "alpine_branching",
    angle_rad: 0.42,
    uplift_gain: 1.12,
    ridge_gain: 1.18,
    carve_gain: 1.08,
    branch_gain: 1.18,
    valley_width_px: 2.4,
    floor_smooth_px: 4.0,
    detail_gain: 0.72,
    anisotropy: 0.72,
};

/// Mirror of `_oriented_ridges(..., seam_safe_mode=True)` for a single point.
/// Rotation centre is fixed at the world origin (cx=cz=0) — seam-safe.
fn oriented_ridges_point(wx: f64, wz: f64, span_m: f64, style: &MountainStyle, seed: i64) -> f64 {
    let (rx, rz) = h::rotated(wx, wz, style.angle_rad, 0.0, 0.0);
    // recursive_domain_warp(rx, rz*anisotropy, ...). NOTE the seed offset +100.
    let (w_rx, w_rz) = h::recursive_domain_warp(
        rx,
        rz * style.anisotropy,
        span_m * 0.065,
        1.0 / (span_m * 0.58),
        seed + 100,
        3,
        0.54,
        1.85,
    );
    let long = h::ridged_multifractal(w_rx, w_rz, 1.0 / (span_m * 0.34), 5, seed + 120, 0.58);
    let mid = h::ridged_multifractal(w_rx, w_rz, 1.0 / (span_m * 0.15), 4, seed + 130, 0.54);
    // organic uses w_x := w_rx + 0.28*w_rz, w_z := w_rz - 0.18*w_rx (Python walrus).
    let w_x = w_rx + 0.28 * w_rz;
    let w_z = w_rz - 0.18 * w_rx;
    let organic = h::ridged_multifractal(w_x, w_z, 1.0 / (span_m * 0.22), 5, seed + 140, 0.56);
    let cross = h::ridged_multifractal(w_x, w_z, 1.0 / (span_m * 0.095), 3, seed + 150, 0.50);
    let raw = 0.42 * long + 0.24 * mid + 0.48 * organic + 0.18 * cross;
    // seam-safe: affine_remap then clip [0,1].
    h::clip(h::affine_remap(raw, RIDGES_CENTER, RIDGES_SCALE), 0.0, 1.0)
}

/// Mirror of `_lowland_mask(range_field, regional, blur_mode='nearest')`.
/// Returns the whole field. `broad_range` = gaussian(range_field, sigma=7.0).
fn lowland_mask(
    range_field: &[f64],
    regional: &[f64],
    rows: usize,
    cols: usize,
    spacing_m: f64,
) -> Vec<f64> {
    let broad_range = array_ops::gaussian_filter_nearest(
        range_field,
        rows,
        cols,
        h::sigma_cells(7.0, spacing_m),
        h::TRUNCATE,
    );
    let n = rows * cols;
    let mut out = vec![0.0_f64; n];
    for i in 0..n {
        let low = h::smoothstep(0.48, 0.84, 1.0 - broad_range[i]);
        let regional_low = h::smoothstep(0.44, 0.78, 1.0 - regional[i]);
        out[i] = h::clip(low * (0.35 + 0.65 * regional_low), 0.0, 1.0);
    }
    out
}

/// Port of `generate(..., apron_px=APRON_PX)` SEAM-SAFE path, returning the
/// CORE-cropped height (length `core_rows * core_cols`).
///
/// `wx`/`wz` are the apron-padded world-coord grids (flat row-major, length
/// `rows*cols`); `rows`/`cols` are the PADDED dimensions. `feature_span_m` MUST be
/// the fixed CORE span shared by adjacent windows (NOT derived from the padded
/// extent). `apron_px` cells are cropped off every side at the end.
#[allow(clippy::too_many_arguments)]
pub fn generate_seamsafe(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    seed: i64,
    style: &MountainStyle,
    feature_span_m: f64,
    apron_px: usize,
    spacing_m: f64,
    flow_on: bool,
) -> Vec<f64> {
    assert_eq!(wx.len(), rows * cols, "wx len != rows*cols");
    assert_eq!(wz.len(), rows * cols, "wz len != rows*cols");
    let n = rows * cols;
    let feature_span = feature_span_m.max(1.0);

    // --- pointwise: recursive domain warp, then regional / ranges / details ---
    // Python: w_x, w_z = recursive_domain_warp(wx, wz, span*0.050, 1/(span*0.72),
    //         seed+10, 3, 0.58, 1.75)
    let mut regional = vec![0.0_f64; n];
    let mut ranges = vec![0.0_f64; n];
    let mut ridge_detail = vec![0.0_f64; n];
    let mut near_detail = vec![0.0_f64; n];
    for i in 0..n {
        let (w_x, w_z) = h::recursive_domain_warp(
            wx[i],
            wz[i],
            feature_span * 0.050,
            1.0 / (feature_span * 0.72),
            seed + 10,
            3,
            0.58,
            1.75,
        );
        // regional = clip(affine_remap(fbm(w_x,w_z, 1/(span*0.88),5,seed+20,gain=0.56)), 0,1)
        let reg = h::fbm(w_x, w_z, 1.0 / (feature_span * 0.88), 5, seed + 20, 0.56);
        regional[i] = h::clip(h::affine_remap(reg, REGIONAL_CENTER, REGIONAL_SCALE), 0.0, 1.0);
        // ranges = _oriented_ridges(w_x, w_z, span, style, seed, seam_safe=True)
        ranges[i] = oriented_ridges_point(w_x, w_z, feature_span, style, seed);
        // ridge_detail = affine_remap(ridged_multifractal(w_x,w_z,1/(span*0.045),5,seed+40,0.52))
        let rd = h::ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.045), 5, seed + 40, 0.52);
        ridge_detail[i] = h::affine_remap(rd, RIDGE_DETAIL_CENTER, RIDGE_DETAIL_SCALE);
        // near_detail = affine_remap(fbm(w_x,w_z,1/(span*0.020),4,seed+50,0.48))
        let nd = h::fbm(w_x, w_z, 1.0 / (feature_span * 0.020), 4, seed + 50, 0.48);
        near_detail[i] = h::affine_remap(nd, NEAR_DETAIL_CENTER, NEAR_DETAIL_SCALE);
    }

    // --- range_envelope = smoothstep(0.24,0.58, gaussian(ranges, sigma=sigma_cells(5.0))) ---
    let ranges_blur5 = array_ops::gaussian_filter_nearest(
        &ranges,
        rows,
        cols,
        h::sigma_cells(5.0, spacing_m),
        h::TRUNCATE,
    );
    let mut range_envelope = vec![0.0_f64; n];
    for i in 0..n {
        range_envelope[i] = h::smoothstep(0.24, 0.58, ranges_blur5[i]);
    }

    // --- lowland ---
    let lowland = lowland_mask(&ranges, &regional, rows, cols, spacing_m);

    // --- massif ---
    // massif_inner = 0.58*regional + 0.86*range_envelope + 0.28*gaussian(ranges, sigma=sigma_cells(1.8))
    let ranges_blur18 = array_ops::gaussian_filter_nearest(
        &ranges,
        rows,
        cols,
        h::sigma_cells(1.8, spacing_m),
        h::TRUNCATE,
    );
    let mut massif = vec![0.0_f64; n];
    for i in 0..n {
        let massif_inner =
            0.58 * regional[i] + 0.86 * range_envelope[i] + 0.28 * ranges_blur18[i];
        // massif = clip(affine_remap(massif_inner, MASSIF_CENTER, MASSIF_SCALE), 0, 1)
        massif[i] = h::clip(h::affine_remap(massif_inner, MASSIF_CENTER, MASSIF_SCALE), 0.0, 1.0);
    }
    // massif = gaussian(massif, sigma=sigma_cells(2.0))
    let massif = array_ops::gaussian_filter_nearest(
        &massif,
        rows,
        cols,
        h::sigma_cells(2.0, spacing_m),
        h::TRUNCATE,
    );

    // --- base = affine_remap(uplift_gain*(1.50*massif + 0.18*ranges - 0.46*lowland), BASE) ---
    let mut base = vec![0.0_f64; n];
    for i in 0..n {
        let inner = style.uplift_gain * (1.50 * massif[i] + 0.18 * ranges[i] - 0.46 * lowland[i]);
        base[i] = h::affine_remap(inner, BASE_CENTER, BASE_SCALE);
    }

    // --- primary + tributary channel masks (flow_on gated) ---
    // When flow_on == false (coarse clipmap levels) the two expensive
    // flow_channels_seam_safe passes are SKIPPED and both masks are all-zeros,
    // exactly like the Python `else: primary_mask = tributary_mask = zeros_like(base)`.
    // The two `height -= ...` carve terms below then vanish -> MACRO surface.
    let mut primary_mask = vec![0.0_f64; n];
    let mut tributary_mask = vec![0.0_f64; n];
    if flow_on {
        // primary = _flow_channels_seam_safe(base, width=valley_width_px, power=0.48)
        let primary = h::flow_channels_seam_safe(
            &base,
            rows,
            cols,
            style.valley_width_px,
            0.48,
            spacing_m,
        );
        // primary_mask = smoothstep(PRIMARY_LO, PRIMARY_HI, primary)
        for i in 0..n {
            primary_mask[i] = h::smoothstep(PRIMARY_THRESH_LO, PRIMARY_THRESH_HI, primary[i]);
        }

        // rough_surface = base + 0.18 * affine_remap(ranges, RANGES_ZSCORE_CENTER, _SCALE)
        let mut rough_surface = vec![0.0_f64; n];
        for i in 0..n {
            rough_surface[i] = base[i]
                + 0.18 * h::affine_remap(ranges[i], RANGES_ZSCORE_CENTER, RANGES_ZSCORE_SCALE);
        }
        // tributary = _flow_channels_seam_safe(rough_surface, width=max(valley_width*0.42, 0.6), power=0.34)
        let trib_width = (style.valley_width_px * 0.42).max(0.6);
        let tributary =
            h::flow_channels_seam_safe(&rough_surface, rows, cols, trib_width, 0.34, spacing_m);
        for i in 0..n {
            tributary_mask[i] =
                h::smoothstep(TRIBUTARY_THRESH_LO, TRIBUTARY_THRESH_HI, tributary[i]);
        }
    }

    // --- high_mask / valley_mask (shared by both paths) ---
    let mut high_mask = vec![0.0_f64; n];
    let mut valley_mask = vec![0.0_f64; n];
    for i in 0..n {
        high_mask[i] = h::smoothstep(0.48, 0.86, massif[i]) * (1.0 - 0.38 * lowland[i]);
        valley_mask[i] = h::clip(0.72 * primary_mask[i] + 0.46 * tributary_mask[i], 0.0, 1.0);
    }

    // --- seam-safe LOOK gains ---
    let ridge_g = style.ridge_gain * SEAMSAFE_RIDGE_GAIN;
    let detail_g = style.detail_gain * SEAMSAFE_DETAIL_GAIN;
    let carve_g = style.carve_gain * SEAMSAFE_CARVE_GAIN;
    let branch_g = style.branch_gain * SEAMSAFE_BRANCH_GAIN;

    // --- assemble height ---
    // height = base
    // height += ridge_g*(0.08+0.58*high)*(0.24*ridge_detail)
    // height += detail_g*(0.04+0.34*high)*(0.34*near_detail)
    // height -= carve_g*(0.42+0.58*high)*primary_mask
    // height -= branch_g*(0.18+0.42*high)*tributary_mask
    let mut height = vec![0.0_f64; n];
    for i in 0..n {
        let hm = high_mask[i];
        let mut hv = base[i];
        hv += ridge_g * (0.08 + 0.58 * hm) * (0.24 * ridge_detail[i]);
        hv += detail_g * (0.04 + 0.34 * hm) * (0.34 * near_detail[i]);
        hv -= carve_g * (0.42 + 0.58 * hm) * primary_mask[i];
        hv -= branch_g * (0.18 + 0.42 * hm) * tributary_mask[i];
        height[i] = hv;
    }

    // --- floor blend ---
    // floor_mask = clip(smoothstep(0.48,0.86, gaussian(valley_mask, sigma=sigma_cells(1.2))) + 0.24*lowland, 0,1)
    let valley_blur = array_ops::gaussian_filter_nearest(
        &valley_mask,
        rows,
        cols,
        h::sigma_cells(1.2, spacing_m),
        h::TRUNCATE,
    );
    let mut floor_mask = vec![0.0_f64; n];
    for i in 0..n {
        floor_mask[i] =
            h::clip(h::smoothstep(0.48, 0.86, valley_blur[i]) + 0.24 * lowland[i], 0.0, 1.0);
    }
    // floor = gaussian(height, sigma=sigma_cells(max(floor_smooth_px, 0.2)))
    let floor = array_ops::gaussian_filter_nearest(
        &height,
        rows,
        cols,
        h::sigma_cells(style.floor_smooth_px.max(0.2), spacing_m),
        h::TRUNCATE,
    );
    // height = height*(1 - 0.38*floor_mask) + floor*(0.38*floor_mask); height -= 0.18*floor_mask
    for i in 0..n {
        height[i] = height[i] * (1.0 - 0.38 * floor_mask[i]) + floor[i] * (0.38 * floor_mask[i]);
        height[i] -= 0.18 * floor_mask[i];
    }

    // --- final blend (seam-safe) ---
    // final_blend = 0.74*height + 0.26*gaussian(height, sigma=sigma_cells(1.20))
    // height = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE)
    let height_blur = array_ops::gaussian_filter_nearest(
        &height,
        rows,
        cols,
        h::sigma_cells(1.20, spacing_m),
        h::TRUNCATE,
    );
    for i in 0..n {
        let final_blend = 0.74 * height[i] + 0.26 * height_blur[i];
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
