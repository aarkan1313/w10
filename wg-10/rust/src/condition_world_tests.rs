//! Parity tests: `condition_world.rs` (Rust) vs the Python oracle
//! (`mountain_world_layer.condition_world`).
//!
//! Two checks:
//! 1. `percentile_linear_matches_numpy` — the np.percentile default 'linear' interpolation is
//!    deterministic + bit-portable, so the Rust port must match baked numpy values to ~1e-12.
//! 2. `condition_world_matches_python_within_tolerance` — loads the committed fixture
//!    (`tools/dem_pack/fixtures/condition_world_fixture.json`, from
//!    `export_condition_world_fixture.py`) and compares `shaped` within a TOLERANCE. The only
//!    tolerance source is scipy's gaussian default mode='reflect' vs the Rust gaussian's
//!    mode='nearest', which only perturbs the 1-2 border rows/cols (the sigma=0.55 kernel is ~3
//!    taps); the interior is near-exact and the percentile-derived stats match exactly. A large p99
//!    would be a REAL bug (wrong percentile, wrong robust formula, wrong gaussian sigma), not the
//!    border-mode noise — debug, don't widen.

use crate::condition_world as cw;

const FIXTURE: &str = include_str!("../../../tools/dem_pack/fixtures/condition_world_fixture.json");

#[test]
fn percentile_linear_matches_numpy() {
    // Reference values captured from numpy 2.4.4 (np.percentile, default interpolation='linear').

    // [0,1,2,3,4] (N=5): rank = q/100*(N-1).
    //   p5  -> rank 0.2  -> 0 + 0.2*(1-0) = 0.2
    //   p50 -> rank 2.0  -> 2.0
    //   p95 -> rank 3.8  -> 3 + 0.8*(4-3) = 3.8
    //   p25 -> rank 1.0  -> 1.0 ; p0 -> 0.0 ; p100 -> 4.0 (hi index clamps, frac 0)
    let a = [0.0_f64, 1.0, 2.0, 3.0, 4.0];
    let cases_a = [
        (5.0, 0.2),
        (50.0, 2.0),
        (95.0, 3.8),
        (25.0, 1.0),
        (0.0, 0.0),
        (100.0, 4.0),
    ];
    for (q, want) in cases_a {
        let got = cw::percentile_linear(&a, q);
        assert!(
            (got - want).abs() < 1e-12,
            "percentile_linear([0..4], {q}) = {got}, want {want}"
        );
    }

    // [1,2,3,4,5,8,9] (N=7, ascending): rank = q/100*6.
    //   p5  -> rank 0.30 -> 1 + 0.30*(2-1) = 1.3
    //   p50 -> rank 3.00 -> 4.0
    //   p95 -> rank 5.70 -> 8 + 0.70*(9-8) = 8.7
    //   p33 -> rank 1.98 -> 2 + 0.98*(3-2) = 2.98
    let b = [1.0_f64, 2.0, 3.0, 4.0, 5.0, 8.0, 9.0];
    let cases_b = [(5.0, 1.3), (50.0, 4.0), (95.0, 8.7), (33.0, 2.98)];
    for (q, want) in cases_b {
        let got = cw::percentile_linear(&b, q);
        assert!(
            (got - want).abs() < 1e-12,
            "percentile_linear([1,2,3,4,5,8,9], {q}) = {got}, want {want}"
        );
    }

    // Degenerate guards: empty -> 0.0, single element -> that element (any q).
    assert_eq!(cw::percentile_linear(&[], 50.0), 0.0);
    assert_eq!(cw::percentile_linear(&[7.5], 5.0), 7.5);
    assert_eq!(cw::percentile_linear(&[7.5], 95.0), 7.5);
}

#[test]
fn condition_world_matches_python_within_tolerance() {
    let v: serde_json::Value = serde_json::from_str(FIXTURE).expect("parse condition_world_fixture.json");
    let n = v["n"].as_u64().unwrap() as usize;
    let height: Vec<f64> = v["height"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    let want_shaped: Vec<f64> = v["shaped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    assert_eq!(height.len(), n * n, "height len {} != n*n", height.len());
    assert_eq!(want_shaped.len(), n * n, "shaped len {} != n*n", want_shaped.len());

    let (got_shaped, stats) = cw::condition_world(&height, n);
    assert_eq!(got_shaped.len(), want_shaped.len(), "shaped len mismatch");

    // --- percentile stats: bit-portable, must match the fixture exactly (~1e-9). ---
    let want_p05 = v["stats"]["p05"].as_f64().unwrap();
    let want_p50 = v["stats"]["p50"].as_f64().unwrap();
    let want_p95 = v["stats"]["p95"].as_f64().unwrap();
    let p05_diff = (stats.p05 - want_p05).abs();
    let p50_diff = (stats.p50 - want_p50).abs();
    let p95_diff = (stats.p95 - want_p95).abs();

    // Source min/max/ptp are exact reductions over z -> must also match the fixture exactly.
    let src_min_diff = (stats.source_min - v["stats"]["source_min"].as_f64().unwrap()).abs();
    let src_max_diff = (stats.source_max - v["stats"]["source_max"].as_f64().unwrap()).abs();
    let src_ptp_diff = (stats.source_ptp - v["stats"]["source_ptp"].as_f64().unwrap()).abs();

    // --- shaped field: tolerance over all cells, plus an interior-vs-border split to confirm the
    //     diagnosis (interior near-exact; only border cells carry the reflect-vs-nearest mode diff). ---
    let mut diffs: Vec<f64> = (0..got_shaped.len())
        .map(|i| (got_shaped[i] - want_shaped[i]).abs())
        .collect();
    let nd = diffs.len();

    // Interior = cells not on the outer 2-cell ring (the nearest gaussian's ~3-tap kernel only reaches
    // 2 cells in from each edge before it sees a clamped/reflected sample, so 2 rings isolates the
    // mode-affected border from the truly-interior cells).
    let border = 2usize;
    let mut interior_max = 0.0_f64;
    let mut border_max = 0.0_f64;
    let mut border_cells = 0usize;
    for r in 0..n {
        for c in 0..n {
            let d = diffs[r * n + c];
            let is_border = r < border || c < border || r >= n - border || c >= n - border;
            if is_border {
                border_cells += 1;
                if d > border_max {
                    border_max = d;
                }
            } else if d > interior_max {
                interior_max = d;
            }
        }
    }

    diffs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = diffs.iter().sum::<f64>() / nd as f64;
    let p99 = diffs[((nd as f64) * 0.99) as usize];
    let peak = *diffs.last().unwrap();

    println!(
        "[cond-parity] mean={mean:.3e} p99={p99:.3e} peak={peak:.3e} \
         interior_max={interior_max:.3e} border_max={border_max:.3e} (border_cells={border_cells}/{nd}) \
         p05_diff={p05_diff:.3e} p50_diff={p50_diff:.3e} p95_diff={p95_diff:.3e} \
         src_min_diff={src_min_diff:.3e} src_max_diff={src_max_diff:.3e} src_ptp_diff={src_ptp_diff:.3e} \
         p05={} p50={} p95={}",
        stats.p05, stats.p50, stats.p95
    );

    // Stats are bit-portable -> tight epsilon (catches a wrong percentile / wrong reduction).
    let stats_eps = 1e-9;
    assert!(p05_diff < stats_eps, "p05 diff {p05_diff:.3e} > {stats_eps:.0e} (percentile mismatch)");
    assert!(p50_diff < stats_eps, "p50 diff {p50_diff:.3e} > {stats_eps:.0e} (percentile mismatch)");
    assert!(p95_diff < stats_eps, "p95 diff {p95_diff:.3e} > {stats_eps:.0e} (percentile mismatch)");
    assert!(src_min_diff < stats_eps, "source_min diff {src_min_diff:.3e} > {stats_eps:.0e}");
    assert!(src_max_diff < stats_eps, "source_max diff {src_max_diff:.3e} > {stats_eps:.0e}");
    assert!(src_ptp_diff < stats_eps, "source_ptp diff {src_ptp_diff:.3e} > {stats_eps:.0e}");

    // The interior is computed identically to scipy (same kernel, mode is irrelevant away from edges)
    // -> it must be near-exact. A failure here is a REAL bug (formula/sigma), not the border mode diff.
    let interior_eps = 1e-9;
    assert!(
        interior_max < interior_eps,
        "interior_max {interior_max:.3e} > {interior_eps:.0e} -- interior diverges (not a border-mode \
         effect): a real bug (wrong robust formula / percentile / gaussian sigma), debug don't widen"
    );

    // Whole-field tolerance gate. shaped is tanh output in ~[-1,1]; the reflect-vs-nearest border diff
    // is small. p99 < 0.01 is generous over the measured residual; if p99 blows past this it is a real
    // divergence (not isolated border cells), so debug rather than widen.
    let p99_budget = 0.01;
    assert!(
        p99 < p99_budget,
        "condition_world p99 {p99:.3e} > {p99_budget} -- diverges beyond the gaussian border-mode \
         residual: a real bug, debug don't widen"
    );
}
