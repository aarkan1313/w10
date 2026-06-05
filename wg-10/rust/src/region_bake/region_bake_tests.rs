//! bake_region_from_raw must reproduce the existing all-CPU bake_region exactly when fed the
//! SAME RAW field the CPU macro produces (the tail = carve -> condition, unchanged).
use crate::pass_network::{PassNetworkParams, RampParams, TraverseParams};

#[test]
fn from_raw_matches_full_cpu_bake() {
    let n = 64usize;
    let span_m = 25600.0;
    let hs = 260.0;
    let _seed = 7;
    // A deterministic RAW field standing in for the macro output (z-score-ish range).
    let mut raw = vec![0.0f64; n * n];
    for i in 0..n * n {
        let x = (i % n) as f64 / n as f64;
        let z = (i / n) as f64 / n as f64;
        raw[i] = (x * 6.0).sin() * (z * 6.0).cos() * 1.5 + (i % 13) as f64 * 0.05;
    }
    let pass = PassNetworkParams::default();
    let traverse = TraverseParams { scene_width_m: span_m, height_scale_m: hs, ..Default::default() };
    let ramp = RampParams::default();

    // Oracle: carve + condition done inline (the exact tail of bake_region).
    let routes = crate::pass_network::carve_routes(&raw, n, span_m, hs, &pass, &traverse);
    let delta = crate::pass_network::carve_ramp_delta(&raw, n, span_m, hs, &routes, &ramp);
    let raw_carved: Vec<f64> = raw.iter().zip(delta.iter()).map(|(r, d)| r + d).collect();
    let (want_h, want_stats) = crate::condition_world::condition_world(&raw_carved, n);

    let got = super::bake_region_from_raw(&raw, n, span_m, hs, &pass, &traverse, &ramp, None);
    assert_eq!(got.height.len(), want_h.len());
    for i in 0..want_h.len() {
        assert_eq!(got.height[i].to_bits(), want_h[i].to_bits(), "height cell {i}");
    }
    assert_eq!(got.stats.p50.to_bits(), want_stats.p50.to_bits());
}

#[test]
fn from_raw_scalar_provider_equals_none_path() {
    use crate::region_bake::ScalarRegionPercentiles;
    let n = 48usize; let span_m = 25600.0; let hs = 260.0;
    let mut raw = vec![0.0f64; n*n];
    for i in 0..n*n { let x=(i%n) as f64/n as f64; let z=(i/n) as f64/n as f64;
        raw[i] = (x*6.0).sin()*(z*6.0).cos()*1.5 + (i%13) as f64*0.05; }
    let pass = PassNetworkParams::default();
    let traverse = TraverseParams { scene_width_m: span_m, height_scale_m: hs, ..Default::default() };
    let ramp = RampParams::default();
    // The existing None path (self-percentile scalar) ...
    let want = super::bake_region_from_raw(&raw, n, span_m, hs, &pass, &traverse, &ramp, None);
    // ... must equal routing through a ScalarRegionPercentiles provider.
    let got = super::bake_region_from_raw_with_provider(
        &raw, n, span_m, hs, 0.0, 0.0, &pass, &traverse, &ramp, &ScalarRegionPercentiles);
    assert_eq!(got.height.len(), want.height.len());
    for i in 0..want.height.len() { assert_eq!(got.height[i].to_bits(), want.height[i].to_bits(), "cell {i}"); }
    // carve_delta identical too (carve is provider-independent).
    for i in 0..want.carve_delta.len() { assert_eq!(got.carve_delta[i].to_bits(), want.carve_delta[i].to_bits(), "delta {i}"); }
}
