//! bake_region: the seam-safe "baked look" pipeline assembled in Rust end-to-end.
//! macro (seam-safe) -> carve routes -> carve_ramp delta (on RAW) -> raw+delta -> condition_world.
//! ORDER load-bearing: carve on RAW, THEN condition (mountain_world_layer.build_network_world:485-488).
//! The path the LIVE runtime uses (seam-safe branch), NOT the offline full-field artifact.
//!
//! Like the per-biome recipes, this assembly is consumed by the GPU/CPU producer seam that is not
//! wired to the runtime yet; until then it is exercised only by the end-to-end parity test.
#![allow(dead_code)]
use crate::condition_world::{condition_world, ConditionStats};
use crate::pass_network::{carve_ramp_delta, carve_routes, PassNetworkParams, RampParams, TraverseParams};
use crate::recipes::mountain_seamsafe;

pub struct BakeResult {
    pub height: Vec<f64>,
    pub carve_delta: Vec<f64>,
    pub stats: ConditionStats,
}

#[allow(clippy::too_many_arguments)]
pub fn bake_region(
    wx: &[f64],
    wz: &[f64],
    n: usize,
    seed: i64,
    feature_span_m: f64,
    apron_px: usize,
    spacing_m: f64,
    span_m: f64,
    height_scale_m: f64,
    flow_on: bool,
    pass: &PassNetworkParams,
    traverse: &TraverseParams,
    ramp: &RampParams,
) -> BakeResult {
    // mountain_seamsafe takes the PADDED grid dims (wx/wz are apron-padded) and crops the
    // apron internally, returning the inner core (n*n). Derive the padded side from apron_px.
    let pn = n + 2 * apron_px;
    let raw = mountain_seamsafe(wx, wz, pn, pn, seed, feature_span_m, apron_px, spacing_m, flow_on);
    let routes = carve_routes(&raw, n, span_m, height_scale_m, pass, traverse);
    let carve_delta = carve_ramp_delta(&raw, n, span_m, height_scale_m, &routes, ramp);
    let raw_carved: Vec<f64> = raw.iter().zip(carve_delta.iter()).map(|(r, d)| r + d).collect();
    let (height, stats) = condition_world(&raw_carved, n);
    BakeResult { height, carve_delta, stats }
}
