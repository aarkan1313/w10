//! G-seam: measure the cross-region condition seam. Two adjacent regions condition their shared
//! border with per-region percentiles; quantify (a) percentile drift, (b) conditioned-height delta
//! along the shared border column. The verdict drives the reconcile rule (Task 6).
#[test]
fn measure_cross_region_condition_seam() {
    use std::path::Path;
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/dem_pack/fixtures/region_seam_fixture.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let n = v["n"].as_u64().unwrap() as usize;
    let carved_a: Vec<f64> = v["carved_a"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let carved_b: Vec<f64> = v["carved_b"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();

    let (ha, sa) = crate::condition_world::condition_world(&carved_a, n);
    let (hb, sb) = crate::condition_world::condition_world(&carved_b, n);

    let p05_drift = (sa.p05 - sb.p05).abs();
    let p50_drift = (sa.p50 - sb.p50).abs();
    let p95_drift = (sa.p95 - sb.p95).abs();

    // Shared border: A's rightmost column (x = n-1) vs B's leftmost column (x = 0), same z rows.
    let mut max_border_delta = 0.0f64;
    for r in 0..n {
        let a_edge = ha[r * n + (n - 1)];
        let b_edge = hb[r * n];
        max_border_delta = max_border_delta.max((a_edge - b_edge).abs());
    }
    let hs = v["height_scale_m"].as_f64().unwrap();
    println!("[g-seam] p05_drift={p05_drift:.4} p50_drift={p50_drift:.4} p95_drift={p95_drift:.4} | max_border_delta(tanh)={max_border_delta:.4} ~= {:.3}m", max_border_delta * hs);

    // This test ALWAYS passes; it is a measurement. The PRINTED numbers decide Task 6's rule.
    // Guardrail only: the fields must be non-trivial (not all-zero) so the measurement is real.
    assert!(ha.iter().any(|&x| x.abs() > 1e-6) && hb.iter().any(|&x| x.abs() > 1e-6), "vacuous seam fixture");
}
