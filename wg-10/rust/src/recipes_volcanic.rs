//! VOLCANIC biome RECIPE, ported bit-close (f64) from the Python original
//! `tools/dem_pack/volcanic_synthesis.py::generate` (`apron_px > 0` seam-safe path).
//!
//! WIRE: add `mod recipes_volcanic;` + `#[cfg(test)] mod recipes_volcanic_tests;` to lib.rs
//!
//! Follows the MOUNTAIN template (`recipes.rs`) exactly: it reuses
//! `crate::recipes::helpers` for the shared seam-safe primitives (affine_remap,
//! smoothstep, clip, rotated, flow_channels_seam_safe, apron_meshgrid, the
//! recursive_domain_warp / fbm / ridged_multifractal wrappers) and only adds the
//! volcanic-specific math here: the per-style constants, the explicit vent/cone/
//! crater/shield/flow construction, and the numpy PCG64 random stream needed to place
//! vents (mountain has no RNG; volcanic does).
//!
//! Parity contract: `volcanic_seamsafe(...)` reproduces the Python core-cropped height
//! within a tight epsilon (verified against `fixtures/recipe_volcanic_fixture.json` in
//! `recipes_volcanic_tests.rs`).
//!
//! The vent positions and the per-vent flow directions are PURE functions of
//! `(seed, style, feature_span_m)` — NOT of the window's world coordinates (in seam-safe
//! mode the vent centre is the FIXED world origin 0,0 and the span is the caller's fixed
//! `feature_span_m`). That is exactly why they are seam-safe: every adjacent window draws
//! the identical RNG stream and so computes identical vent fields. To stay bit-close we
//! reproduce numpy's PCG64 + SeedSequence stream exactly (see `npy_random` below).

// Consumed by the (not-yet-wired) producer seam; exercised by the parity test for now.
#![allow(dead_code)]

/// Bit-exact reproduction of the slice of numpy's random machinery the volcanic recipe
/// touches: `np.random.default_rng(int)` (SeedSequence -> PCG64), and the three draw
/// methods the recipe calls — `random()`, `uniform(lo, hi)`, `normal(loc, scale)`.
///
/// Only the integer-seed, scalar-draw path is implemented (that is all volcanic uses).
mod npy_random;

/// VOLCANIC biome — mirrors `tools/dem_pack/volcanic_synthesis.py`.
pub mod volcanic {
    use crate::recipes::helpers as h;
    use crate::array_ops;

    // ---- apron constant -----------------------------------------------------
    /// `VOLCANIC_APRON_PX` — matches mountain's calibrated floor (160).
    pub const APRON_PX: usize = 160;

    // ---- affine-remap constants (replace per-window zscore / norm01) --------
    pub const REGIONAL_CENTER: f64 = -0.492;
    pub const REGIONAL_SCALE: f64 = 1.004;
    pub const CONES_CENTER: f64 = 0.003;
    pub const CONES_SCALE: f64 = 0.712;
    pub const CRATERS_CENTER: f64 = 0.000;
    pub const CRATERS_SCALE: f64 = 0.898;
    pub const SHIELDS_CENTER: f64 = 0.010;
    pub const SHIELDS_SCALE: f64 = 0.434;
    pub const FLOWS_CENTER: f64 = 0.003;
    pub const FLOWS_SCALE: f64 = 1.459;
    pub const VENTS_CENTER: f64 = 0.000;
    pub const VENTS_SCALE: f64 = 1.008;
    pub const BASE_CENTER: f64 = 0.459;
    pub const BASE_SCALE: f64 = 5.30;
    pub const LAVA_TEXTURE_CENTER: f64 = -0.002;
    pub const LAVA_TEXTURE_SCALE: f64 = 3.63;
    pub const ROUGH_AA_CENTER: f64 = 0.335;
    pub const ROUGH_AA_SCALE: f64 = 4.47;
    pub const FINAL_CENTER: f64 = 0.376;
    pub const FINAL_SCALE: f64 = 0.82;

    mod vents;

    pub use vents::{STRATOVOLCANO_CLUSTER, VolcanicStyle};
    pub(crate) use vents::{packed_vents, MAX_VENTS, VENT_STRIDE};

    /// Mirror of `_gully_channels_seam_safe(surface, power=0.40)`:
    /// pre-blur sigma=1.15 -> MFD acc (power) -> FIXED-max log1p normalise -> spread
    /// blur sigma=1.2 -> clip [0,1]. Note the SPREAD sigma is 1.2 (mountain uses
    /// `max(width_px, 0.1)`); volcanic's spread is a fixed 1.2, so this is a small
    /// dedicated copy rather than `helpers::flow_channels_seam_safe`.
    fn gully_channels_seam_safe(surface: &[f64], rows: usize, cols: usize, power: f64) -> Vec<f64> {
        let pre = array_ops::gaussian_filter_nearest(surface, rows, cols, 1.15, h::TRUNCATE);
        let acc = array_ops::flow_accumulation_mfd(&pre, rows, cols, power);
        let log_size = ((rows * cols) as f64).ln_1p();
        let mut discharge: Vec<f64> = acc
            .iter()
            .map(|&a| h::clip(a.ln_1p() / log_size, 0.0, 1.0))
            .collect();
        discharge = array_ops::gaussian_filter_nearest(&discharge, rows, cols, 1.2, h::TRUNCATE);
        for v in discharge.iter_mut() {
            *v = h::clip(*v, 0.0, 1.0);
        }
        discharge
    }

    /// Port of `generate(..., apron_px=APRON_PX)` SEAM-SAFE path, returning the
    /// CORE-cropped height (length `core_rows * core_cols`).
    #[allow(clippy::too_many_arguments)]
    pub fn generate_seamsafe(
        wx: &[f64],
        wz: &[f64],
        rows: usize,
        cols: usize,
        seed: i64,
        style: &VolcanicStyle,
        feature_span_m: f64,
        apron_px: usize,
    ) -> Vec<f64> {
        assert_eq!(wx.len(), rows * cols, "wx len != rows*cols");
        assert_eq!(wz.len(), rows * cols, "wz len != rows*cols");
        let n = rows * cols;
        let feature_span = feature_span_m.max(1.0);
        let sseed = seed + style.seed_offset;

        // --- recursive domain warp (pointwise) ---
        // w_x, w_z = recursive_domain_warp(wx, wz, span*0.026, 1/(span*0.72), sseed+10, 3, 0.52, 1.82)
        let mut w_x = vec![0.0_f64; n];
        let mut w_z = vec![0.0_f64; n];
        for i in 0..n {
            let (a, b) = h::recursive_domain_warp(
                wx[i],
                wz[i],
                feature_span * 0.026,
                1.0 / (feature_span * 0.72),
                sseed + 10,
                3,
                0.52,
                1.82,
            );
            w_x[i] = a;
            w_z[i] = b;
        }

        // --- regional / rift (pointwise) ---
        // regional = clip(affine_remap(fbm(w_x,w_z, 1/(span*0.84),5,sseed+30,0.56)), 0,1)
        // rift_raw = ridged_multifractal(rotated(w_x,w_z,angle,0,0)->(rx, rz*0.22), 1/(span*0.16),4,sseed+80,0.52)
        // rift = clip(smoothstep(0.40,0.88,rift_raw) * rift_gain, 0, 1)
        let mut regional = vec![0.0_f64; n];
        let mut rift = vec![0.0_f64; n];
        for i in 0..n {
            let reg = h::fbm(w_x[i], w_z[i], 1.0 / (feature_span * 0.84), 5, sseed + 30, 0.56);
            regional[i] = h::clip(h::affine_remap(reg, REGIONAL_CENTER, REGIONAL_SCALE), 0.0, 1.0);

            let (rx, rz) = h::rotated(w_x[i], w_z[i], style.angle_rad, 0.0, 0.0);
            let rift_raw =
                h::ridged_multifractal(rx, rz * 0.22, 1.0 / (feature_span * 0.16), 4, sseed + 80, 0.52);
            rift[i] = h::clip(h::smoothstep(0.40, 0.88, rift_raw) * style.rift_gain, 0.0, 1.0);
        }

        // --- vent fields ---
        let (cones, craters, shields, flows, _vents) =
            vents::vent_fields(&w_x, &w_z, rows, cols, feature_span, style, sseed, true);

        // --- lava_texture / rough_aa / base (pointwise + affine) ---
        let mut lava_texture = vec![0.0_f64; n];
        let mut rough_aa = vec![0.0_f64; n];
        let mut base = vec![0.0_f64; n];
        for i in 0..n {
            let lt = h::fbm(w_x[i], w_z[i], 1.0 / (feature_span * 0.020), 5, sseed + 210, 0.48);
            lava_texture[i] = h::affine_remap(lt, LAVA_TEXTURE_CENTER, LAVA_TEXTURE_SCALE);
            let ra =
                h::ridged_multifractal(w_x[i], w_z[i], 1.0 / (feature_span * 0.027), 4, sseed + 240, 0.48);
            rough_aa[i] = h::affine_remap(ra, ROUGH_AA_CENTER, ROUGH_AA_SCALE);
            let base_inner = 0.58 * regional[i] + 0.52 * shields[i] * style.shield_gain + 0.22 * rift[i];
            base[i] = h::affine_remap(base_inner, BASE_CENTER, BASE_SCALE);
        }

        // --- radial_surface + gullies ---
        // radial_surface = base + 1.12*cones - 0.78*craters
        let mut radial_surface = vec![0.0_f64; n];
        for i in 0..n {
            radial_surface[i] = base[i] + 1.12 * cones[i] - 0.78 * craters[i];
        }
        let gullies_discharge = gully_channels_seam_safe(&radial_surface, rows, cols, 0.40);
        // gullies = smoothstep(0.52,0.92,gullies_discharge) * (0.30 + 0.70*cones)
        let mut gullies = vec![0.0_f64; n];
        for i in 0..n {
            gullies[i] = h::smoothstep(0.52, 0.92, gullies_discharge[i]) * (0.30 + 0.70 * cones[i]);
        }

        // --- caldera bowl/rim, cone lift (whole-field gaussian on shields+cones) ---
        // caldera_bowl = craters * smoothstep(0.52,0.88, gaussian(shields+cones, sigma=2.6))
        let mut shields_plus_cones = vec![0.0_f64; n];
        for i in 0..n {
            shields_plus_cones[i] = shields[i] + cones[i];
        }
        let spc_blur = array_ops::gaussian_filter_nearest(&shields_plus_cones, rows, cols, 2.6, h::TRUNCATE);
        let mut caldera_bowl = vec![0.0_f64; n];
        let mut caldera_rim = vec![0.0_f64; n];
        let mut cone_lift = vec![0.0_f64; n];
        for i in 0..n {
            caldera_bowl[i] = craters[i] * h::smoothstep(0.52, 0.88, spc_blur[i]);
            caldera_rim[i] =
                h::smoothstep(0.38, 0.78, cones[i]) * (1.0 - h::smoothstep(0.25, 0.72, craters[i]));
            cone_lift[i] = cones[i] * (1.0 - 0.88 * h::smoothstep(0.12, 0.78, craters[i]));
        }

        // --- assemble height ---
        let mut height = vec![0.0_f64; n];
        for i in 0..n {
            let mut hv = base[i];
            hv += style.cone_gain * (1.08 * cone_lift[i] + 0.20 * cone_lift[i] * rough_aa[i]);
            hv += style.shield_gain * 0.54 * shields[i];
            hv += 0.22 * rift[i];
            hv += style.flow_gain * (0.42 * flows[i] + 0.13 * flows[i] * lava_texture[i]);
            hv += style.caldera_gain * 0.22 * caldera_rim[i];
            hv -= style.caldera_gain * 1.48 * caldera_bowl[i];
            hv -= style.gully_gain * 0.30 * gullies[i];
            hv += style.detail_gain * (0.10 + 0.18 * flows[i] + 0.20 * cones[i]) * lava_texture[i];
            height[i] = hv;
        }

        // --- ash plain blend ---
        // ash_plain = smoothstep(0.52,0.86, 1 - gaussian(max(cones,flows), sigma=3.0))
        let mut max_cf = vec![0.0_f64; n];
        for i in 0..n {
            max_cf[i] = cones[i].max(flows[i]);
        }
        let max_cf_blur = array_ops::gaussian_filter_nearest(&max_cf, rows, cols, 3.0, h::TRUNCATE);
        let mut ash_plain = vec![0.0_f64; n];
        for i in 0..n {
            ash_plain[i] = h::smoothstep(0.52, 0.86, 1.0 - max_cf_blur[i]);
        }
        // smoothed_plain = gaussian(height, sigma=2.6)
        let smoothed_plain = array_ops::gaussian_filter_nearest(&height, rows, cols, 2.6, h::TRUNCATE);
        for i in 0..n {
            height[i] = height[i] * (1.0 - 0.30 * ash_plain[i]) + smoothed_plain[i] * (0.30 * ash_plain[i]);
        }

        // --- final blend (seam-safe) ---
        // final_blend = 0.82*height + 0.18*gaussian(height, sigma=0.85)
        // height = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE)
        let height_blur = array_ops::gaussian_filter_nearest(&height, rows, cols, 0.85, h::TRUNCATE);
        for i in 0..n {
            let final_blend = 0.82 * height[i] + 0.18 * height_blur[i];
            height[i] = h::affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
        }

        // --- crop to core: height[a:-a, a:-a] ---
        crop_core(&height, rows, cols, apron_px)
    }

    /// Crop the inner core: `field[a:-a, a:-a]`.
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

/// Public entry point: VOLCANIC seam-safe height, core-cropped. Uses `STYLES[0]`
/// (stratovolcano_cluster). `wx`/`wz` are apron-padded world-coord grids (flat row-major,
/// PADDED `rows*cols`); returns the inner core height.
#[allow(clippy::too_many_arguments)]
pub fn volcanic_seamsafe(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    seed: i64,
    feature_span_m: f64,
    apron_px: usize,
) -> Vec<f64> {
    volcanic::generate_seamsafe(
        wx,
        wz,
        rows,
        cols,
        seed,
        &volcanic::STRATOVOLCANO_CLUSTER,
        feature_span_m,
        apron_px,
    )
}
