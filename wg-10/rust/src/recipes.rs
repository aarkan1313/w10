//! Seam-safe biome RECIPES, ported bit-close (f64) from the Python originals in
//! `tools/dem_pack/*_synthesis.py`.
//!
//! This is the FIRST of 11 biome ports; the MOUNTAIN recipe
//! (`tools/dem_pack/mountain_synthesis.py::generate`, `apron_px > 0` seam-safe path)
//! is the TEMPLATE. The shared `helpers` submodule below holds everything the other
//! 10 biomes reuse (affine_remap, smoothstep, the pointwise-noise grid driver, the
//! seam-safe flow-channels wrapper, the rotation helper). Biome-specific math
//! (constants, style fields, the assembly pipeline) lives in the per-biome submodule
//! (`mountain` here). Add the next biome as a sibling submodule that leans on `helpers`.
//!
//! Parity contract: the public `mountain_seamsafe(...)` reproduces the Python core-
//! cropped height within a tight epsilon, verified against the committed fixture
//! `tools/dem_pack/fixtures/recipe_mountain_fixture.json` (`recipes_tests.rs`).
//!
//! Whole-array building blocks come from [`crate::array_ops`] (gaussian_filter_nearest,
//! flow_accumulation_mfd) and the per-point noise from [`crate::recipe_noise`]; both are
//! already fixture-proven, so this module only has to compose them faithfully.

// The recipes are consumed by the GPU/CPU producer seam that is not wired yet; until
// then several entry points are exercised only by the parity test.
#![allow(dead_code)]

/// Shared recipe helpers reused by every biome port. Keep additions here SMALL and
/// genuinely shared; biome-specific math belongs in the per-biome submodule.
pub mod helpers {
    use crate::array_ops;
    use crate::recipe_noise;

    /// scipy's default gaussian truncate. All seam-safe blurs use it.
    pub const TRUNCATE: f64 = 4.0;

    /// Data-independent affine remap: `(field - center) * scale`.
    /// Mirror of `seam_safe.affine_remap`. The seam-safe replacement for per-window
    /// zscore / norm01: identical transform for every window keeps borders bit-exact.
    #[inline]
    pub fn affine_remap(v: f64, center: f64, scale: f64) -> f64 {
        (v - center) * scale
    }

    /// In-place affine remap over a whole field.
    pub fn affine_remap_field(field: &mut [f64], center: f64, scale: f64) {
        for v in field.iter_mut() {
            *v = affine_remap(*v, center, scale);
        }
    }

    /// Hermite smoothstep with the Python's `+ 1e-9` denominator guard.
    /// Mirror of `mountain_synthesis.smoothstep`:
    /// `t = clip((x-e0)/(e1-e0+1e-9), 0, 1); t*t*(3-2t)`.
    #[inline]
    pub fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
        let t = ((x - edge0) / (edge1 - edge0 + 1e-9)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// `clip(v, lo, hi)` matching numpy's `np.clip`.
    #[inline]
    pub fn clip(v: f64, lo: f64, hi: f64) -> f64 {
        v.clamp(lo, hi)
    }

    /// Rotate a single `(wx, wz)` about a fixed world centre `(cx, cz)` by `angle_rad`.
    /// Mirror of `mountain_synthesis._rotated` with EXPLICIT centre (seam-safe: in the
    /// apron path the Python passes cx=cz=0.0, never the data-dependent window midpoint).
    ///
    /// `x = wx - cx; z = wz - cz; (c*x + s*z, -s*x + c*z)`.
    #[inline]
    pub fn rotated(wx: f64, wz: f64, angle_rad: f64, cx: f64, cz: f64) -> (f64, f64) {
        let x = wx - cx;
        let z = wz - cz;
        let c = angle_rad.cos();
        let s = angle_rad.sin();
        (c * x + s * z, -s * x + c * z)
    }

    /// Seam-safe CONNECTED-drainage discharge field. Mirror of
    /// `mountain_synthesis._flow_channels_seam_safe(surface, width_px, mode='nearest', power)`:
    ///
    /// 1. pre-blur `surface` with gaussian sigma=1.15 (nearest),
    /// 2. real MFD flow accumulation (`array_ops::flow_accumulation_mfd`, given `power`),
    /// 3. FIXED-max normalize: `clip(log1p(acc) / log1p(acc.size), 0, 1)` (data-independent),
    /// 4. spread with gaussian sigma=max(width_px, 0.1) (nearest), clip [0, 1].
    ///
    /// Reused verbatim by every biome that carves channels.
    pub fn flow_channels_seam_safe(
        surface: &[f64],
        rows: usize,
        cols: usize,
        width_px: f64,
        power: f64,
    ) -> Vec<f64> {
        let pre = array_ops::gaussian_filter_nearest(surface, rows, cols, 1.15, TRUNCATE);
        let acc = array_ops::flow_accumulation_mfd(&pre, rows, cols, power);
        // log1p(acc.size): acc.size is the element count (rows*cols), matching numpy.
        let log_size = ((rows * cols) as f64).ln_1p();
        let mut discharge: Vec<f64> = acc
            .iter()
            .map(|&a| clip(a.ln_1p() / log_size, 0.0, 1.0))
            .collect();
        let sigma = width_px.max(0.1);
        discharge = array_ops::gaussian_filter_nearest(&discharge, rows, cols, sigma, TRUNCATE);
        for v in discharge.iter_mut() {
            *v = clip(*v, 0.0, 1.0);
        }
        discharge
    }

    /// Build an apron-padded world-coordinate meshgrid, identical to the fixture's
    /// Python construction: `xs[c] = (c - apron_px)*spacing + ox`,
    /// `zs[r] = (r - apron_px)*spacing + oz`, then `wx[r][c]=xs[c]`, `wz[r][c]=zs[r]`.
    /// Returns `(wx, wz)` as flat row-major vectors of length `rows*cols`.
    pub fn apron_meshgrid(
        rows: usize,
        cols: usize,
        apron_px: usize,
        spacing: f64,
        ox: f64,
        oz: f64,
    ) -> (Vec<f64>, Vec<f64>) {
        let a = apron_px as f64;
        let xs: Vec<f64> = (0..cols).map(|c| (c as f64 - a) * spacing + ox).collect();
        let zs: Vec<f64> = (0..rows).map(|r| (r as f64 - a) * spacing + oz).collect();
        let mut wx = vec![0.0_f64; rows * cols];
        let mut wz = vec![0.0_f64; rows * cols];
        for r in 0..rows {
            for c in 0..cols {
                wx[r * cols + c] = xs[c];
                wz[r * cols + c] = zs[r];
            }
        }
        (wx, wz)
    }

    /// Re-export the per-point recursive domain warp at the recipe call's exact arity.
    /// (Thin pass-through so per-biome code reads close to the Python.)
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn recursive_domain_warp(
        wx: f64,
        wz: f64,
        warp_amount: f64,
        warp_freq: f64,
        seed: i64,
        steps: u32,
        decay: f64,
        freq_mul: f64,
    ) -> (f64, f64) {
        recipe_noise::recursive_domain_warp(wx, wz, warp_amount, warp_freq, seed, steps, decay, freq_mul)
    }

    /// `fbm` with the Python recipe default gain/lacunarity made explicit at call sites.
    #[inline]
    pub fn fbm(wx: f64, wz: f64, base_freq: f64, octaves: u32, seed: i64, gain: f64) -> f64 {
        recipe_noise::fbm(wx, wz, base_freq, octaves, seed, gain, 2.0)
    }

    /// `ridged_multifractal` with the recipe defaults (offset=1.0, weight_gain=1.35).
    #[inline]
    pub fn ridged_multifractal(
        wx: f64,
        wz: f64,
        base_freq: f64,
        octaves: u32,
        seed: i64,
        gain: f64,
    ) -> f64 {
        recipe_noise::ridged_multifractal(wx, wz, base_freq, octaves, seed, gain, 2.0, 1.0, 1.35)
    }
}

/// MOUNTAIN biome — the template port. Mirrors `tools/dem_pack/mountain_synthesis.py`.
pub mod mountain {
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
    fn lowland_mask(range_field: &[f64], regional: &[f64], rows: usize, cols: usize) -> Vec<f64> {
        let broad_range = array_ops::gaussian_filter_nearest(range_field, rows, cols, 7.0, h::TRUNCATE);
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

        // --- range_envelope = smoothstep(0.24,0.58, gaussian(ranges, sigma=5.0)) ---
        let ranges_blur5 = array_ops::gaussian_filter_nearest(&ranges, rows, cols, 5.0, h::TRUNCATE);
        let mut range_envelope = vec![0.0_f64; n];
        for i in 0..n {
            range_envelope[i] = h::smoothstep(0.24, 0.58, ranges_blur5[i]);
        }

        // --- lowland ---
        let lowland = lowland_mask(&ranges, &regional, rows, cols);

        // --- massif ---
        // massif_inner = 0.58*regional + 0.86*range_envelope + 0.28*gaussian(ranges, sigma=1.8)
        let ranges_blur18 = array_ops::gaussian_filter_nearest(&ranges, rows, cols, 1.8, h::TRUNCATE);
        let mut massif = vec![0.0_f64; n];
        for i in 0..n {
            let massif_inner =
                0.58 * regional[i] + 0.86 * range_envelope[i] + 0.28 * ranges_blur18[i];
            // massif = clip(affine_remap(massif_inner, MASSIF_CENTER, MASSIF_SCALE), 0, 1)
            massif[i] = h::clip(h::affine_remap(massif_inner, MASSIF_CENTER, MASSIF_SCALE), 0.0, 1.0);
        }
        // massif = gaussian(massif, sigma=2.0)
        let massif = array_ops::gaussian_filter_nearest(&massif, rows, cols, 2.0, h::TRUNCATE);

        // --- base = affine_remap(uplift_gain*(1.50*massif + 0.18*ranges - 0.46*lowland), BASE) ---
        let mut base = vec![0.0_f64; n];
        for i in 0..n {
            let inner = style.uplift_gain * (1.50 * massif[i] + 0.18 * ranges[i] - 0.46 * lowland[i]);
            base[i] = h::affine_remap(inner, BASE_CENTER, BASE_SCALE);
        }

        // --- primary channels ---
        // primary = _flow_channels_seam_safe(base, width=valley_width_px, power=0.48)
        let primary =
            h::flow_channels_seam_safe(&base, rows, cols, style.valley_width_px, 0.48);
        // primary_mask = smoothstep(PRIMARY_LO, PRIMARY_HI, primary)
        let mut primary_mask = vec![0.0_f64; n];
        for i in 0..n {
            primary_mask[i] = h::smoothstep(PRIMARY_THRESH_LO, PRIMARY_THRESH_HI, primary[i]);
        }

        // --- tributaries ---
        // rough_surface = base + 0.18 * affine_remap(ranges, RANGES_ZSCORE_CENTER, _SCALE)
        let mut rough_surface = vec![0.0_f64; n];
        for i in 0..n {
            rough_surface[i] =
                base[i] + 0.18 * h::affine_remap(ranges[i], RANGES_ZSCORE_CENTER, RANGES_ZSCORE_SCALE);
        }
        // tributary = _flow_channels_seam_safe(rough_surface, width=max(valley_width*0.42, 0.6), power=0.34)
        let trib_width = (style.valley_width_px * 0.42).max(0.6);
        let tributary = h::flow_channels_seam_safe(&rough_surface, rows, cols, trib_width, 0.34);
        let mut tributary_mask = vec![0.0_f64; n];
        for i in 0..n {
            tributary_mask[i] = h::smoothstep(TRIBUTARY_THRESH_LO, TRIBUTARY_THRESH_HI, tributary[i]);
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
        // floor_mask = clip(smoothstep(0.48,0.86, gaussian(valley_mask, sigma=1.2)) + 0.24*lowland, 0,1)
        let valley_blur = array_ops::gaussian_filter_nearest(&valley_mask, rows, cols, 1.2, h::TRUNCATE);
        let mut floor_mask = vec![0.0_f64; n];
        for i in 0..n {
            floor_mask[i] =
                h::clip(h::smoothstep(0.48, 0.86, valley_blur[i]) + 0.24 * lowland[i], 0.0, 1.0);
        }
        // floor = gaussian(height, sigma=max(floor_smooth_px, 0.2))
        let floor = array_ops::gaussian_filter_nearest(
            &height,
            rows,
            cols,
            style.floor_smooth_px.max(0.2),
            h::TRUNCATE,
        );
        // height = height*(1 - 0.38*floor_mask) + floor*(0.38*floor_mask); height -= 0.18*floor_mask
        for i in 0..n {
            height[i] = height[i] * (1.0 - 0.38 * floor_mask[i]) + floor[i] * (0.38 * floor_mask[i]);
            height[i] -= 0.18 * floor_mask[i];
        }

        // --- final blend (seam-safe) ---
        // final_blend = 0.74*height + 0.26*gaussian(height, sigma=1.20)
        // height = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE)
        let height_blur = array_ops::gaussian_filter_nearest(&height, rows, cols, 1.20, h::TRUNCATE);
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
}

/// Public template entry point: MOUNTAIN seam-safe height, core-cropped.
///
/// `wx`/`wz` are apron-padded world-coord grids (flat row-major, PADDED `rows*cols`);
/// returns the inner core height (length `(rows-2*apron_px)*(cols-2*apron_px)`), exactly
/// like the Python `generate(...)["height"]`. Uses `STYLES[0]` (alpine_branching).
#[allow(clippy::too_many_arguments)]
pub fn mountain_seamsafe(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    seed: i64,
    feature_span_m: f64,
    apron_px: usize,
) -> Vec<f64> {
    mountain::generate_seamsafe(
        wx,
        wz,
        rows,
        cols,
        seed,
        &mountain::ALPINE_BRANCHING,
        feature_span_m,
        apron_px,
    )
}
