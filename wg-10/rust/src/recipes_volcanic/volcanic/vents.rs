//! Volcanic vent metadata, packing, and field synthesis.

use crate::array_ops;
use crate::recipes::helpers as h;

use super::{
    CONES_CENTER, CONES_SCALE, CRATERS_CENTER, CRATERS_SCALE, FLOWS_CENTER, FLOWS_SCALE,
    SHIELDS_CENTER, SHIELDS_SCALE, VENTS_CENTER, VENTS_SCALE,
};
use super::super::npy_random::Generator;

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
pub(super) fn vent_fields(
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
