//! End-to-end parity gate for the Rust `bake_region` assembly vs the Python oracle
//! (`tools/dem_pack/fixtures/bake_region_fixture.json`, emitted by Task 1).
//!
//! Rebuilds the PADDED wx/wz world grid in Rust (mountain.grid formula), builds the
//! pass-network / traverse / ramp params FROM THE FIXTURE (the SPAN-RELATIVE ramp widths,
//! NOT RampParams::default()), runs the whole macro->carve->condition pipeline, and compares
//! height / carve_delta / condition stats to the oracle.

#[test]
fn bake_region_matches_python_seamsafe_pipeline() {
    use std::path::Path;
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/dem_pack/fixtures/bake_region_fixture.json");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let n = v["n"].as_u64().unwrap() as usize;
    let span_m = v["span_m"].as_f64().unwrap();
    let hs = v["height_scale_m"].as_f64().unwrap();
    let seed = v["seed"].as_i64().unwrap();
    let feature_span_m = v["feature_span_m"].as_f64().unwrap();
    let apron_px = v["apron_px"].as_u64().unwrap() as usize;
    let spacing_m = v["spacing_m"].as_f64().unwrap();
    let ox = v["source_origin_x_m"].as_f64().unwrap();
    let oz = v["source_origin_z_m"].as_f64().unwrap();

    // Rebuild the PADDED world grid: padded side = n + 2*apron_px, padded span grows by the apron.
    let pn = n + 2 * apron_px;
    let psp = span_m + 2.0 * apron_px as f64 * spacing_m;
    let gox = ox - apron_px as f64 * spacing_m;
    let goz = oz - apron_px as f64 * spacing_m;
    let mut wx = vec![0.0_f64; pn * pn];
    let mut wz = vec![0.0_f64; pn * pn];
    for r in 0..pn {
        for c in 0..pn {
            wx[r * pn + c] = gox + (c as f64 / (pn as f64 - 1.0)) * psp;
            wz[r * pn + c] = goz + (r as f64 / (pn as f64 - 1.0)) * psp;
        }
    }

    // Params FROM THE FIXTURE (span-relative ramp widths live in `params`: half=5400, flat=1620;
    // do NOT use RampParams::default() = 1200/200, which would diverge the carve).
    let pr = &v["params"];
    let pass = super::pass_network::PassNetworkParams {
        n_we: pr["n_we"].as_u64().unwrap() as usize,
        n_ns: pr["n_ns"].as_u64().unwrap() as usize,
        coarse_n: pr["coarse_n"].as_u64().unwrap() as usize,
    };
    let traverse = super::pass_network::TraverseParams {
        slope_budget: pr["slope_budget"].as_f64().unwrap(),
        slope_penalty: pr["slope_penalty"].as_f64().unwrap(),
        drainage_bias: pr["drainage_bias"].as_f64().unwrap(),
        scene_width_m: span_m,
        height_scale_m: hs,
    };
    let ramp = super::pass_network::RampParams {
        slope_budget: pr["slope_budget"].as_f64().unwrap(),
        floor_grade_frac: pr["ramp_floor_grade_frac"].as_f64().unwrap(),
        wall_grade_frac: pr["ramp_wall_grade_frac"].as_f64().unwrap(),
        flat_half_m: pr["ramp_flat_half_m"].as_f64().unwrap(),
        half_width_m: pr["ramp_half_width_m"].as_f64().unwrap(),
        floor_smooth_px: pr["ramp_floor_smooth_px"].as_f64().unwrap(),
        carve_max_m: pr["ramp_carve_max_m"].as_f64().unwrap(),
    };

    let got = super::bake_region::bake_region(
        &wx, &wz, n, seed, feature_span_m, apron_px, spacing_m, span_m, hs, true, &pass, &traverse,
        &ramp,
    );

    let want_h: Vec<f64> = v["height"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let want_d: Vec<f64> =
        v["carve_delta"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let want_raw: Vec<f64> = v["raw"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert_eq!(got.height.len(), want_h.len());

    // raw-macro sanity FIRST (sharpest signal: if raw differs, grid/macro is wrong).
    // (bake_region doesn't return raw; recompute it here for the diagnostic. mountain_seamsafe
    // takes the PADDED dims -- it crops the apron internally and returns the n*n core.)
    let raw = super::recipes::mountain_seamsafe(
        &wx, &wz, pn, pn, seed, feature_span_m, apron_px, spacing_m, true,
    );
    let mut rawd: Vec<f64> = (0..raw.len()).map(|i| (raw[i] - want_raw[i]).abs()).collect();
    rawd.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let raw_p99 = rawd[((rawd.len() as f64) * 0.99) as usize];
    let raw_peak = *rawd.last().unwrap();

    let mut hd: Vec<f64> = (0..want_h.len()).map(|i| ((got.height[i] - want_h[i]) * hs).abs()).collect();
    hd.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let hmean = hd.iter().sum::<f64>() / hd.len() as f64;
    let hp99 = hd[((hd.len() as f64) * 0.99) as usize];
    let hpeak = *hd.last().unwrap();

    let mut dd: Vec<f64> = (0..want_d.len()).map(|i| ((got.carve_delta[i] - want_d[i]) * hs).abs()).collect();
    dd.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let dp99 = dd[((dd.len() as f64) * 0.99) as usize];

    let carved = want_d.iter().filter(|d| **d < -1e-9).count();
    let p05_d = (got.stats.p05 - v["stats"]["p05"].as_f64().unwrap()).abs();
    let p50_d = (got.stats.p50 - v["stats"]["p50"].as_f64().unwrap()).abs();
    let p95_d = (got.stats.p95 - v["stats"]["p95"].as_f64().unwrap()).abs();

    println!("[bake-parity] carved={carved} RAW p99={raw_p99:.2e} peak={raw_peak:.2e} | height mean_m={hmean:.4} p99_m={hp99:.4} peak_m={hpeak:.4} | carve_delta p99_m={dp99:.4} | p05d={p05_d:.2e} p50d={p50_d:.2e} p95d={p95_d:.2e}");

    assert!(carved > 0, "vacuous");
    assert!(
        raw_p99 < 1e-6,
        "RAW macro diverges (p99={raw_p99:.2e}) -> grid or mountain_seamsafe mismatch, fix FIRST"
    );
    assert!(
        p05_d < 1e-6 && p50_d < 1e-6 && p95_d < 1e-6,
        "condition stats diverge -> raw+carve composition wrong"
    );
    // Carve is now bit-exact (p99 measured 0.0000m) after the carve_ramp gaussian was switched to
    // scipy's default mode='reflect' (the gathered floor has thousands-of-metres border gradients,
    // so the prior mode='nearest' diverged 12m p99 / 196m peak on the WIDE bake ramp band -- a latent
    // carve_ramp bug the narrow-band carve_ramp fixture missed). A non-zero dp99 now would be a real
    // regression, not tie noise.
    assert!(dp99 < 1e-6, "carve_delta p99 {dp99:.4}m diverges -> carve_ramp regression");

    // Height p99 measured 0.0918m (peak 1.5337m). The ONLY residual is the documented condition_world
    // reflect-vs-nearest border RING (condition_world deliberately uses mode='nearest', tolerance-gated):
    // bit-exact interior, sub-2m border. Budget ~1.6x the measured p99; a real assembly bug would push
    // p99 far past this (carve divergences were 12m). Do NOT widen to mask a regression.
    let hp99_budget = 0.15;
    assert!(hp99 < hp99_budget, "bake height p99 {hp99:.4}m > {hp99_budget}m -- assembly bug");
}
