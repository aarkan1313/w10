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
    use super::npy_random::Generator;
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

    /// Mirror of `VolcanicStyle` (the fields the seam-safe pipeline reads).
    #[derive(Clone, Copy, Debug)]
    pub struct VolcanicStyle {
        pub key: &'static str,
        pub angle_rad: f64,
        pub vent_count: i32,
        pub cone_gain: f64,
        pub shield_gain: f64,
        pub caldera_gain: f64,
        pub flow_gain: f64,
        pub rift_gain: f64,
        pub gully_gain: f64,
        pub cone_width_m: f64,
        pub crater_width_m: f64,
        pub flow_length_m: f64,
        pub detail_gain: f64,
        pub seed_offset: i64,
    }

    /// `STYLES[0]` — stratovolcano_cluster (the reference style).
    pub const STRATOVOLCANO_CLUSTER: VolcanicStyle = VolcanicStyle {
        key: "stratovolcano_cluster",
        angle_rad: 0.35,
        vent_count: 4,
        cone_gain: 1.28,
        shield_gain: 0.62,
        caldera_gain: 0.72,
        flow_gain: 0.78,
        rift_gain: 0.34,
        gully_gain: 1.12,
        cone_width_m: 6700.0,
        crater_width_m: 1500.0,
        flow_length_m: 27000.0,
        detail_gain: 0.58,
        seed_offset: 0,
    };

    /// `_angle_delta(a, b)` = atan2(sin(a-b), cos(a-b)).
    #[inline]
    fn angle_delta(a: f64, b: f64) -> f64 {
        (a - b).sin().atan2((a - b).cos())
    }

    /// Max vents the GPU vent buffer is sized for (a FIXED upper bound so the storage buffer
    /// has a constant size regardless of style). STYLES[0] (stratovolcano_cluster) uses 4; the
    /// other styles use up to a handful. The buffer is packed `(vx, vz, amp, dir0..dir3)` = 7
    /// floats per vent, padded to `MAX_VENTS` entries; the actual count is passed via push
    /// constant. Keep this >= every style's `vent_count`.
    pub(crate) const MAX_VENTS: usize = 8;
    /// Floats per packed vent: vx, vz, amp, then the 4 flow directions (in vent-list order).
    pub(crate) const VENT_STRIDE: usize = 7;

    /// CPU-side build of the GPU vent buffer (THE key insight: the RNG stays in Rust, the GPU
    /// only consumes this small uploaded buffer). Reproduces `vent_fields`' EXACT two RNG streams:
    ///   * vent positions/amps via `vent_points(style, sseed, feature_span_m)` (RNG seed
    ///     `sseed + seed_offset + 500`),
    ///   * per-vent flow directions via a SECOND stream `default_rng(sseed + seed_offset + 900)`,
    ///     drawn 4-at-a-time IN VENT-LIST ORDER (exactly as `vent_fields` pre-draws them per vent).
    /// Returns the packed buffer (length `MAX_VENTS * VENT_STRIDE`, zero-padded past `vent_count`)
    /// and the actual `vent_count`. The GPU loops `[0, vent_count)` doing PURE f32 cone/crater/
    /// shield/flow math (NO RNG on the GPU).
    ///
    /// `sseed = seed + style.seed_offset` (the caller's resolved seed, matching `generate_seamsafe`).
    pub(crate) fn packed_vents(
        style: &VolcanicStyle,
        seed: i64,
        feature_span_m: f64,
    ) -> (Vec<f32>, usize) {
        let feature_span = feature_span_m.max(1.0);
        let sseed = seed + style.seed_offset;

        // Stream 1: vent positions/amps (vent_points opens default_rng(sseed + seed_offset + 500)).
        let vent_list = vent_points(style, sseed, feature_span);

        // Stream 2: flow directions, drawn 4 per vent in vent-list order (mirror of vent_fields).
        let mut flow_rng = Generator::from_seed_int((sseed + style.seed_offset + 900) as u128);

        let mut packed = vec![0.0_f32; MAX_VENTS * VENT_STRIDE];
        let count = vent_list.len();
        debug_assert!(
            count <= MAX_VENTS,
            "vent_count {count} > MAX_VENTS {MAX_VENTS} (bump MAX_VENTS)"
        );
        for (vi, &(vx, vz, amp)) in vent_list.iter().enumerate() {
            // The 4 flow directions for THIS vent, drawn BEFORE moving to the next vent
            // (byte-identical draw order to vent_fields' per-vent `dirs` array).
            let d0 = flow_rng.uniform(-std::f64::consts::PI, std::f64::consts::PI);
            let d1 = flow_rng.uniform(-std::f64::consts::PI, std::f64::consts::PI);
            let d2 = flow_rng.uniform(-std::f64::consts::PI, std::f64::consts::PI);
            let d3 = flow_rng.uniform(-std::f64::consts::PI, std::f64::consts::PI);
            let b = vi * VENT_STRIDE;
            packed[b] = vx as f32;
            packed[b + 1] = vz as f32;
            packed[b + 2] = amp as f32;
            packed[b + 3] = d0 as f32;
            packed[b + 4] = d1 as f32;
            packed[b + 5] = d2 as f32;
            packed[b + 6] = d3 as f32;
        }
        (packed, count)
    }

    /// Mirror of `_vent_points(..., seam_safe_mode=True)` for the seam-safe path.
    /// Returns `(vx, vz, amp)` tuples. RNG: `default_rng(seed + style.seed_offset + 500)`.
    /// In seam-safe mode the centre is the FIXED world origin (0,0) and `span` is the
    /// caller-supplied `feature_span_m` — so the vent set is window-independent.
    fn vent_points(style: &VolcanicStyle, sseed: i64, feature_span_m: f64) -> Vec<(f64, f64, f64)> {
        // Python: np.random.default_rng(int(seed) + int(style.seed_offset) + 500),
        // where the caller already passes sseed = seed + seed_offset. So the RNG seed is
        // sseed + 500 (style.seed_offset is added a SECOND time inside _vent_points).
        // Reproduce EXACTLY: the recipe calls _vent_fields(..., sseed, ...), which calls
        // _vent_points(wx,wz,style,sseed,...). Inside, rng seed = sseed + seed_offset + 500.
        let rng_seed = sseed + style.seed_offset + 500;
        let mut rng = Generator::from_seed_int(rng_seed as u128);

        let span = feature_span_m;
        let cx = 0.0_f64;
        let cz = 0.0_f64;
        let min_x = cx - span * 0.5;
        let min_z = cz - span * 0.5;

        let mut vents: Vec<(f64, f64, f64)> = Vec::new();
        match style.key {
            "rift_cone_chain" => {
                let c = style.angle_rad.cos();
                let s = style.angle_rad.sin();
                for i in 0..style.vent_count {
                    let t = (i as f64 / (style.vent_count as f64 - 1.0).max(1.0) - 0.5) * span * 0.74;
                    let lateral = rng.normal(0.0, span * 0.045);
                    let x = cx + c * t - s * lateral;
                    let z = cz + s * t + c * lateral;
                    let amp = 0.72 + 0.46 * rng.random();
                    vents.push((x, z, amp));
                }
            }
            "caldera_complex" => {
                let x0 = cx + rng.normal(0.0, span * 0.035);
                let z0 = cz + rng.normal(0.0, span * 0.035);
                vents.push((x0, z0, 1.20));
                for i in 0..(style.vent_count - 1) {
                    let a = 2.0 * std::f64::consts::PI * i as f64
                        / (style.vent_count as f64 - 1.0).max(1.0)
                        + rng.normal(0.0, 0.24);
                    let r = span * (0.17 + 0.06 * rng.random());
                    vents.push((cx + a.cos() * r, cz + a.sin() * r, 0.58 + 0.34 * rng.random()));
                }
            }
            _ => {
                let x0 = cx + rng.normal(0.0, span * 0.08);
                let z0 = cz + rng.normal(0.0, span * 0.08);
                vents.push((x0, z0, 1.08));
                for _ in 0..(style.vent_count - 1) {
                    let x = min_x + span * (0.18 + 0.64 * rng.random());
                    let z = min_z + span * (0.18 + 0.64 * rng.random());
                    let amp = 0.48 + 0.52 * rng.random();
                    vents.push((x, z, amp));
                }
            }
        }
        vents
    }

    /// Mirror of `_vent_fields(..., seam_safe_mode=True)`. Builds cones/craters/shields/
    /// flows/vents whole-field from the resolved vent list. Flow directions come from a
    /// SECOND RNG stream `default_rng(sseed + seed_offset + 900)`, drawn per vent (4 each)
    /// in vent-list order.
    #[allow(clippy::type_complexity)]
    fn vent_fields(
        wx: &[f64],
        wz: &[f64],
        rows: usize,
        cols: usize,
        feature_span_m: f64,
        style: &VolcanicStyle,
        sseed: i64,
        blur_mode_nearest: bool,
    ) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let n = rows * cols;
        let mut cones = vec![0.0_f64; n];
        let mut craters = vec![0.0_f64; n];
        let mut shields = vec![0.0_f64; n];
        let mut flows = vec![0.0_f64; n];
        let mut vents = vec![0.0_f64; n];

        // Flow-direction RNG (separate stream; same double-add of seed_offset as Python).
        let mut flow_rng = Generator::from_seed_int((sseed + style.seed_offset + 900) as u128);

        let vent_list = vent_points(style, sseed, feature_span_m);

        let cone_w = style.cone_width_m.max(1.0);
        let shield_w = (style.cone_width_m * 2.65).max(1.0);
        let crater_w = style.crater_width_m.max(1.0);
        let rim_w = (style.crater_width_m * 0.34).max(1.0);
        let rim_center = style.crater_width_m * 1.55;
        let flow_len = style.flow_length_m.max(1.0);
        let ds_e0 = style.crater_width_m * 1.8;
        let ds_e1 = style.cone_width_m * 1.4;

        for &(vx, vz, amp) in &vent_list {
            // Pre-draw the 4 flow directions for THIS vent (Python draws inside the
            // per-pixel loop, but the draws happen vent-by-vent in list order, 4 each).
            let dirs = [
                flow_rng.uniform(-std::f64::consts::PI, std::f64::consts::PI),
                flow_rng.uniform(-std::f64::consts::PI, std::f64::consts::PI),
                flow_rng.uniform(-std::f64::consts::PI, std::f64::consts::PI),
                flow_rng.uniform(-std::f64::consts::PI, std::f64::consts::PI),
            ];
            for i in 0..n {
                let dx = wx[i] - vx;
                let dz = wz[i] - vz;
                let r = (dx * dx + dz * dz).sqrt();
                let cone = (-r / cone_w).exp();
                let shield = (-((r / shield_w).powi(2))).exp();
                let crater = (-((r / crater_w).powi(2))).exp();
                let rim = (-(((r - rim_center) / rim_w).powi(2))).exp();
                cones[i] += amp * cone;
                shields[i] += amp * shield;
                craters[i] += amp * crater;
                cones[i] += 0.18 * amp * rim;
                if crater > vents[i] {
                    vents[i] = crater;
                }

                let angle = dz.atan2(dx);
                let downstream = h::smoothstep(ds_e0, ds_e1, r);
                let radial = (-r / flow_len).exp();
                let mut local_flow = 0.0_f64;
                for &direction in &dirs {
                    let angular = (-((angle_delta(angle, direction) / 0.25).powi(2))).exp();
                    let lobe = angular * radial * downstream;
                    if lobe > local_flow {
                        local_flow = lobe;
                    }
                }
                let scaled = amp * local_flow;
                if scaled > flows[i] {
                    flows[i] = scaled;
                }
            }
        }

        // seam-safe outputs.
        let mut cones_out = cones;
        for v in cones_out.iter_mut() {
            *v = h::clip(h::affine_remap(*v, CONES_CENTER, CONES_SCALE), 0.0, 1.0);
        }
        let mut craters_out = craters;
        for v in craters_out.iter_mut() {
            *v = h::clip(h::affine_remap(*v, CRATERS_CENTER, CRATERS_SCALE), 0.0, 1.0);
        }
        let mut shields_out = shields;
        for v in shields_out.iter_mut() {
            *v = h::clip(h::affine_remap(*v, SHIELDS_CENTER, SHIELDS_SCALE), 0.0, 1.0);
        }
        let _ = blur_mode_nearest; // seam-safe is always 'nearest'.
        let flows_blurred = array_ops::gaussian_filter_nearest(&flows, rows, cols, 1.1, h::TRUNCATE);
        let mut flows_out = flows_blurred;
        for v in flows_out.iter_mut() {
            *v = h::clip(h::affine_remap(*v, FLOWS_CENTER, FLOWS_SCALE), 0.0, 1.0);
        }
        let mut vents_out = vents;
        for v in vents_out.iter_mut() {
            *v = h::clip(h::affine_remap(*v, VENTS_CENTER, VENTS_SCALE), 0.0, 1.0);
        }

        (cones_out, craters_out, shields_out, flows_out, vents_out)
    }

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
            vent_fields(&w_x, &w_z, rows, cols, feature_span, style, sseed, true);

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
