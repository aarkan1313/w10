// WIRE: add mod recipes_coast; + test mod
//! COAST biome recipe -- seam-safe (apron_px > 0) path, ported bit-close (f64) from
//! `tools/dem_pack/coast_synthesis.py::generate`. Follows the MOUNTAIN template
//! (`recipes.rs`) exactly: shared math comes from [`crate::recipes::helpers`]; only the
//! coast-specific constants, sub-fields (inland / headland / scarp / channels / fjords /
//! islands / sea-floor / shelf) and the assembly pipeline live here.
//!
//! COAST is a TERRAIN/MASK setup biome (runtime water / sea-level integration is later
//! work). This port reproduces ONLY its HEIGHT generation -- the same scope as the other
//! biome ports. The `sea`/`land`/`shelf` etc. fields are used purely as height-shaping
//! masks here; no water behaviour is ported.
//!
//! Parity contract: `coast_seamsafe(...)` reproduces the Python core-cropped height
//! within a tight epsilon, verified against the committed fixture
//! `tools/dem_pack/fixtures/recipe_coast_fixture.json` (`recipes_coast_tests.rs`).
//!
//! Coast-specific notes vs the mountain/desert/rainforest templates:
//!   * `recursive_domain_warp` uses warp_amount = feature_span*0.026,
//!     warp_freq = 1/(feature_span*0.82), seed = sseed+10, steps = 3, decay = 0.55,
//!     freq_mul = 1.72 (mountain 1.75 / desert 1.78 / rainforest 1.74; decay 0.55 here).
//!   * Rotation `rx, rz = rotated(wx, wz, angle_rad, cx=0, cz=0)` is applied to the RAW
//!     world coords (NOT the domain-warped `w_x`/`w_z`), about the fixed world origin.
//!     `coast_warp` (fbm) is then evaluated on the WARPED `w_x`/`w_z`. `signed` mixes the
//!     two: `signed = rx + coast_warp * feature_span * 0.15 * coastline_warp`.
//!   * The channel carve uses coast's flow power 0.47 and a FIXED spread sigma 1.9 (passed
//!     as `width_px` to `helpers::flow_channels_seam_safe`; 1.9 > 0.1 so the helper's
//!     `.max(0.1)` is a no-op -- bit-identical to the Python `sigma=1.9`). The pre-blur
//!     sigma=1.15 inside the helper matches coast's `_flow_channels_seam_safe`.
//!   * `ridge_source` is affine_remap'd WITHOUT a trailing clip (unlike `inland` /
//!     `texture` `sea_floor` which clip differently -- see below).
//!   * `texture` is affine_remap'd with NO clip; `sea_floor` clips the remap to [0,1]
//!     BEFORE the `-0.74 - 0.22*...` shaping; `inland` clips the remap to [0,1].
//!   * `fjord_grooves` evaluates `ridged_multifractal(rz, rx*0.24, ...)` on the ROTATED
//!     RAW coords (note the argument order: rz first, rx*0.24 second).
//!   * `islands_seed = cellular_edges(w_x, w_z, ..., sharpness=1.30)` (NOT the Python
//!     default 2.0); the seed is then whole-array blurred (sigma=2.0) before the smoothstep.
//!   * `smoothed_sea = gaussian(height, sigma=3.0)` then `height = height*(1 - 0.34*sea)
//!     + smoothed_sea*(0.34*sea)` -- a whole-array sea-smoothing blend.
//!   * STYLES[0] is `cliffed_headlands`. scarp_gain / fjord_gain / ... read straight from it.
//!   * Flow ties on flat sea-floor / shelf: the surface fed to the channel MFD
//!     (`ridge_source`) can contain EXACTLY-equal cells over flat regions. The MFD downhill
//!     test is strict (`drop > 0`), so tied cells never flow to each other; the Rust
//!     stable-sort vs numpy quicksort tie order changes the result by at most ~1e-16 (same
//!     caveat as `array_ops::flow_accumulation_mfd`).

#![allow(dead_code)]

use crate::recipe_noise;
use crate::recipes::helpers as h;
use crate::array_ops;

// ---------------------------------------------------------------------------
// Apron constant -- COAST_APRON_PX. See Python docstring for the derivation.
// ---------------------------------------------------------------------------
pub const APRON_PX: usize = 160;

// ---------------------------------------------------------------------------
// Affine-remap constants (replace per-window zscore / norm01). Mirror of the
// COAST_*_CENTER / COAST_*_SCALE module constants in coast_synthesis.py.
// ---------------------------------------------------------------------------
// inland fbm (norm01)
pub const INLAND_CENTER: f64 = -0.551;
pub const INLAND_SCALE: f64 = 0.923;

// ridge_source (zscore)
pub const RIDGE_SOURCE_CENTER: f64 = 0.500;
pub const RIDGE_SOURCE_SCALE: f64 = 4.474;

// texture ridged multifractal (zscore)
pub const TEXTURE_CENTER: f64 = 0.350;
pub const TEXTURE_SCALE: f64 = 4.437;

// sea_floor fbm (norm01)
pub const SEA_FLOOR_CENTER: f64 = -0.708;
pub const SEA_FLOOR_SCALE: f64 = 0.713;

// inland used in land_height (zscore)
pub const INLAND_ZSCORE_CENTER: f64 = -0.045;
pub const INLAND_ZSCORE_SCALE: f64 = 4.499;

// final blend (replaces trailing zscore)
pub const FINAL_CENTER: f64 = -0.518;
pub const FINAL_SCALE: f64 = 1.662;

/// Mirror of `CoastStyle` (only the fields the seam-safe pipeline reads).
#[derive(Clone, Copy, Debug)]
pub struct CoastStyle {
    pub key: &'static str,
    pub angle_rad: f64,
    pub scarp_gain: f64,
    pub fjord_gain: f64,
    pub island_gain: f64,
    pub shelf_gain: f64,
    pub headland_gain: f64,
    pub texture_gain: f64,
    pub coastline_warp: f64,
    pub seed_offset: i64,
}

/// `STYLES[0]` -- cliffed_headlands (the coast reference style).
pub const CLIFFED_HEADLANDS: CoastStyle = CoastStyle {
    key: "cliffed_headlands",
    angle_rad: 0.12,
    scarp_gain: 1.28,
    fjord_gain: 0.28,
    island_gain: 0.34,
    shelf_gain: 0.82,
    headland_gain: 1.14,
    texture_gain: 0.72,
    coastline_warp: 0.92,
    seed_offset: 0,
};

/// Mirror of `_flow_channels_seam_safe(surface, mode='nearest', power=0.47)`.
///
/// pre-blur sigma=1.15 -> MFD flow accumulation (power=0.47) -> FIXED-max
/// log1p/log1p(size) normalize -> spread blur sigma=1.9. This is exactly
/// `helpers::flow_channels_seam_safe` with width_px=1.9 (the helper's `.max(0.1)` is a
/// no-op since 1.9 > 0.1).
#[inline]
fn flow_channels_seam_safe(surface: &[f64], rows: usize, cols: usize) -> Vec<f64> {
    h::flow_channels_seam_safe(surface, rows, cols, 1.9, 0.47)
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
    style: &CoastStyle,
    feature_span_m: f64,
    apron_px: usize,
) -> Vec<f64> {
    assert_eq!(wx.len(), rows * cols, "wx len != rows*cols");
    assert_eq!(wz.len(), rows * cols, "wz len != rows*cols");
    let n = rows * cols;
    let feature_span = feature_span_m.max(1.0);
    let sseed = seed + style.seed_offset;

    // --- pointwise stage 1: rotation (raw coords), domain warp, coast_warp, signed,
    //     sea/land/nearshore/shelf, inland(_raw), headlands, scarp ---
    // Python:
    //   rx, rz = _rotated(wx, wz, angle_rad, cx=0, cz=0)           (RAW coords)
    //   w_x, w_z = recursive_domain_warp(wx, wz, span*0.026, 1/(span*0.82),
    //              sseed+10, 3, 0.55, 1.72)
    //   coast_warp = fbm(w_x, w_z, 1/(span*0.42), 5, sseed+30, gain=0.56)
    //   signed = rx + coast_warp * span * 0.15 * coastline_warp
    //   sea = smoothstep(span*0.030, -span*0.030, signed)
    //   land = 1 - sea
    //   nearshore = exp(-((signed / (span*0.045))^2))
    //   shelf = smoothstep(span*0.20, -span*0.060, signed)
    //   inland_raw = fbm(w_x, w_z, 1/(span*0.72), 5, sseed+60, gain=0.58)
    //   headlands_raw = ridged_multifractal(w_x, w_z, 1/(span*0.22), 4, sseed+80, gain=0.52)
    //   headlands = smoothstep(0.50, 0.84, headlands_raw)
    //   scarp = nearshore * land * (0.55 + 0.75 * headlands)
    //   inland = clip(affine_remap(inland_raw, INLAND), 0, 1)
    let mut rx_v = vec![0.0_f64; n];
    let mut rz_v = vec![0.0_f64; n];
    // Retain the domain-warped coords for all downstream warped-coord sub-fields
    // (coast_warp / inland / headlands / islands_seed / texture / sea_floor). Computed
    // once here (deterministic pure function of wx/wz), like the desert/rainforest ports.
    let mut w_x_v = vec![0.0_f64; n];
    let mut w_z_v = vec![0.0_f64; n];
    let mut signed_v = vec![0.0_f64; n];
    let mut sea = vec![0.0_f64; n];
    let mut land = vec![0.0_f64; n];
    let mut nearshore = vec![0.0_f64; n];
    let mut shelf = vec![0.0_f64; n];
    let mut inland_raw = vec![0.0_f64; n];
    let mut inland = vec![0.0_f64; n];
    let mut headlands = vec![0.0_f64; n];
    let mut scarp = vec![0.0_f64; n];
    // ridge_source feeds the channel flow; build it in this same pass (no whole-array op
    // sits between its inputs and it).
    let mut ridge_source = vec![0.0_f64; n];
    for i in 0..n {
        let (rx, rz) = h::rotated(wx[i], wz[i], style.angle_rad, 0.0, 0.0);
        rx_v[i] = rx;
        rz_v[i] = rz;
        let (w_x, w_z) = h::recursive_domain_warp(
            wx[i],
            wz[i],
            feature_span * 0.026,
            1.0 / (feature_span * 0.82),
            sseed + 10,
            3,
            0.55,
            1.72,
        );
        w_x_v[i] = w_x;
        w_z_v[i] = w_z;
        let coast_warp = h::fbm(w_x, w_z, 1.0 / (feature_span * 0.42), 5, sseed + 30, 0.56);
        let signed = rx + coast_warp * feature_span * 0.15 * style.coastline_warp;
        signed_v[i] = signed;
        let sea_i = h::smoothstep(feature_span * 0.030, -feature_span * 0.030, signed);
        sea[i] = sea_i;
        let land_i = 1.0 - sea_i;
        land[i] = land_i;
        let ns = (-((signed / (feature_span * 0.045)).powi(2))).exp();
        nearshore[i] = ns;
        shelf[i] = h::smoothstep(feature_span * 0.20, -feature_span * 0.060, signed);

        let ir = h::fbm(w_x, w_z, 1.0 / (feature_span * 0.72), 5, sseed + 60, 0.58);
        inland_raw[i] = ir;
        let headlands_raw =
            h::ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.22), 4, sseed + 80, 0.52);
        let hl = h::smoothstep(0.50, 0.84, headlands_raw);
        headlands[i] = hl;
        let sc = ns * land_i * (0.55 + 0.75 * hl);
        scarp[i] = sc;
        let inl = h::clip(h::affine_remap(ir, INLAND_CENTER, INLAND_SCALE), 0.0, 1.0);
        inland[i] = inl;
        // ridge_source = affine_remap(inland + 0.36*headlands + 0.18*scarp, RIDGE_SOURCE)  (NO clip)
        ridge_source[i] = h::affine_remap(
            inl + 0.36 * hl + 0.18 * sc,
            RIDGE_SOURCE_CENTER,
            RIDGE_SOURCE_SCALE,
        );
    }

    // --- channels (seam-safe flow) ---
    // channels_raw = _flow_channels_seam_safe(ridge_source, power=0.47)  (spread sigma=1.9)
    // channels = smoothstep(0.53, 0.92, channels_raw) * land
    let channels_raw = flow_channels_seam_safe(&ridge_source, rows, cols);
    let mut channels = vec![0.0_f64; n];
    for i in 0..n {
        channels[i] = h::smoothstep(0.53, 0.92, channels_raw[i]) * land[i];
    }

    // --- fjords / fjord_grooves / channel_relief ---
    // fjords = channels * nearshore * smoothstep(0.20, 0.80, land)
    // fjord_grooves = ridged_multifractal(rz, rx*0.24, 1/(span*0.11), 4, sseed+120, gain=0.50)
    // fjord_grooves = smoothstep(0.52, 0.88, fjord_grooves) * land
    //                 * smoothstep(span*0.25, -span*0.01, signed)
    // channel_relief = clip(
    //     channels * (0.34 + 0.34*fjord_gain)
    //     + fjords * fjord_gain
    //     + fjord_grooves * max(fjord_gain - 0.30, 0.0) * 0.44, 0, 1)
    let mut channel_relief = vec![0.0_f64; n];
    for i in 0..n {
        let fjords = channels[i] * nearshore[i] * h::smoothstep(0.20, 0.80, land[i]);
        let fg_raw = h::ridged_multifractal(
            rz_v[i],
            rx_v[i] * 0.24,
            1.0 / (feature_span * 0.11),
            4,
            sseed + 120,
            0.50,
        );
        let fjord_grooves = h::smoothstep(0.52, 0.88, fg_raw)
            * land[i]
            * h::smoothstep(feature_span * 0.25, -feature_span * 0.01, signed_v[i]);
        channel_relief[i] = h::clip(
            channels[i] * (0.34 + 0.34 * style.fjord_gain)
                + fjords * style.fjord_gain
                + fjord_grooves * (style.fjord_gain - 0.30).max(0.0) * 0.44,
            0.0,
            1.0,
        );
    }

    // --- islands ---
    // islands_seed = cellular_edges(w_x, w_z, 1/(span*0.18), sseed+160, sharpness=1.30)
    //   (w_x/w_z are the stage-1 domain-warped coords).
    // islands = smoothstep(0.50, 0.86, gaussian(islands_seed, sigma=2.0)) * sea
    // islands *= smoothstep(span*0.18, -span*0.02, signed)
    let mut islands_seed = vec![0.0_f64; n];
    for i in 0..n {
        islands_seed[i] = recipe_noise::cellular_edges(
            w_x_v[i],
            w_z_v[i],
            1.0 / (feature_span * 0.18),
            sseed + 160,
            1.30,
        );
    }
    let islands_blur = array_ops::gaussian_filter_nearest(&islands_seed, rows, cols, 2.0, h::TRUNCATE);
    let mut islands = vec![0.0_f64; n];
    for i in 0..n {
        let isl = h::smoothstep(0.50, 0.86, islands_blur[i]) * sea[i];
        islands[i] = isl * h::smoothstep(feature_span * 0.18, -feature_span * 0.02, signed_v[i]);
    }

    // --- texture / sea_floor / land_height + height assembly (pointwise) ---
    // texture = affine_remap(texture_raw, TEXTURE)  (NO clip)
    // sea_floor = -0.74 - 0.22 * clip(affine_remap(sea_floor_raw, SEA_FLOOR), 0, 1)
    // land_height = 0.68 * affine_remap(inland_raw, INLAND_ZSCORE) + 0.26 * headland_gain * headlands
    // land_height += 0.48 * scarp_gain * scarp
    // land_height -= 0.48 * channel_relief
    // land_height += texture_gain * 0.09 * texture * (0.35 + 0.65*land)
    // height = land*land_height + sea*sea_floor
    // height += island_gain * 0.62 * islands
    // height -= shelf_gain * 0.22 * shelf * sea
    let mut height = vec![0.0_f64; n];
    for i in 0..n {
        let w_x = w_x_v[i];
        let w_z = w_z_v[i];
        // texture_raw = ridged_multifractal(w_x, w_z, 1/(span*0.050), 4, sseed+220, gain=0.44)
        let texture_raw =
            h::ridged_multifractal(w_x, w_z, 1.0 / (feature_span * 0.050), 4, sseed + 220, 0.44);
        let texture = h::affine_remap(texture_raw, TEXTURE_CENTER, TEXTURE_SCALE);

        let sea_floor_raw = h::fbm(w_x, w_z, 1.0 / (feature_span * 0.34), 4, sseed + 260, 0.55);
        let sea_floor = -0.74
            - 0.22 * h::clip(h::affine_remap(sea_floor_raw, SEA_FLOOR_CENTER, SEA_FLOOR_SCALE), 0.0, 1.0);

        let mut land_height = 0.68 * h::affine_remap(inland_raw[i], INLAND_ZSCORE_CENTER, INLAND_ZSCORE_SCALE)
            + 0.26 * style.headland_gain * headlands[i];
        land_height += 0.48 * style.scarp_gain * scarp[i];
        land_height -= 0.48 * channel_relief[i];
        land_height += style.texture_gain * 0.09 * texture * (0.35 + 0.65 * land[i]);

        let mut hv = land[i] * land_height + sea[i] * sea_floor;
        hv += style.island_gain * 0.62 * islands[i];
        hv -= style.shelf_gain * 0.22 * shelf[i] * sea[i];
        height[i] = hv;
    }

    // --- whole-array sea smoothing blend ---
    // smoothed_sea = gaussian(height, sigma=3.0)
    // height = height*(1 - 0.34*sea) + smoothed_sea*(0.34*sea)
    let smoothed_sea = array_ops::gaussian_filter_nearest(&height, rows, cols, 3.0, h::TRUNCATE);
    for i in 0..n {
        height[i] = height[i] * (1.0 - 0.34 * sea[i]) + smoothed_sea[i] * (0.34 * sea[i]);
    }

    // --- final blend (seam-safe) ---
    // final_blend = 0.86*height + 0.14*gaussian(height, sigma=0.9)
    // height = affine_remap(final_blend, FINAL)
    let height_blur = array_ops::gaussian_filter_nearest(&height, rows, cols, 0.9, h::TRUNCATE);
    for i in 0..n {
        let final_blend = 0.86 * height[i] + 0.14 * height_blur[i];
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

/// Public entry point: COAST seam-safe height, core-cropped. Uses `STYLES[0]`
/// (cliffed_headlands). Signature matches the task contract.
#[allow(clippy::too_many_arguments)]
pub fn coast_seamsafe(
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
        &CLIFFED_HEADLANDS,
        feature_span_m,
        apron_px,
    )
}
