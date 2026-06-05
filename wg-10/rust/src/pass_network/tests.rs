use super::cost::slope_grid;
use super::cost::step_cost;
use super::dijkstra::{dijkstra_cost_field, reconstruct_path};
use super::edt::edt_with_indices;
use super::TraverseParams;

/// THE PARITY GATE: Rust `carve_routes` vs the committed Python `_routes` fixture.
/// If this passes, the whole port (cost model + Dijkstra + routing) is validated end-to-end:
/// the fixture is n=193==coarse_n (zoom identity) so this isolates routing, and the fixture
/// was adversarially proven a meaningful oracle (wrong cost formula / heap tie-break => different
/// routes). Asserts every route matches point-for-point.
#[test]
fn routes_match_python_fixture() {
    use std::path::Path;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/dem_pack/fixtures/pass_network_routes_fixture.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()));
    let v: serde_json::Value = serde_json::from_str(&raw).expect("parse fixture");
    let n = v["n"].as_u64().unwrap() as usize;
    let span_m = v["span_m"].as_f64().unwrap();
    let height_scale_m = v["height_scale_m"].as_f64().unwrap();
    let height: Vec<f64> = v["height"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    let pp = super::PassNetworkParams {
        n_we: v["params"]["n_we"].as_u64().unwrap() as usize,
        n_ns: v["params"]["n_ns"].as_u64().unwrap() as usize,
        coarse_n: v["params"]["coarse_n"].as_u64().unwrap() as usize,
    };
    let tp = super::TraverseParams {
        slope_budget: v["params"]["slope_budget"].as_f64().unwrap(),
        slope_penalty: v["params"]["slope_penalty"].as_f64().unwrap(),
        drainage_bias: v["params"]["drainage_bias"].as_f64().unwrap(),
        scene_width_m: span_m,
        height_scale_m,
    };
    // Echo the loaded params so a parity failure can be debugged against the fixture.
    eprintln!(
        "[carve-parity] loaded n={n} span_m={span_m} height_scale_m={height_scale_m} \
         n_we={} n_ns={} coarse_n={} slope_budget={} slope_penalty={} drainage_bias={}",
        pp.n_we, pp.n_ns, pp.coarse_n, tp.slope_budget, tp.slope_penalty, tp.drainage_bias
    );
    assert_eq!(height.len(), n * n, "height len {} != n*n", height.len());

    let got = super::carve_routes(&height, n, span_m, height_scale_m, &pp, &tp);
    let want: Vec<Vec<(usize, usize)>> = v["routes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|rt| {
            rt.as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    let a = p.as_array().unwrap();
                    (a[0].as_u64().unwrap() as usize, a[1].as_u64().unwrap() as usize)
                })
                .collect()
        })
        .collect();
    assert_eq!(
        got.len(),
        want.len(),
        "route count: got {} want {}",
        got.len(),
        want.len()
    );
    let mut total = 0usize;
    for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
        if g != w {
            // Pinpoint the first differing cell to make debugging concrete.
            let first_diff = g
                .iter()
                .zip(w.iter())
                .position(|(a, b)| a != b)
                .map(|j| format!("at cell {j}: got {:?} want {:?}", g.get(j), w.get(j)))
                .unwrap_or_else(|| "lengths differ only".to_string());
            panic!(
                "route {i} differs (len got {} want {}) {first_diff}",
                g.len(),
                w.len()
            );
        }
        total += g.len();
    }
    println!("[carve-parity] routes={} total_points={} MATCH", got.len(), total);
}

#[test]
fn measure_carve_cost_production_scale() {
    use std::time::Instant;
    // Representative routing-grid scale. coarse_n=193 is what the Dijkstra actually routes on
    // (the carve downsamples to coarse_n), so cost is ~independent of the full n; use n==coarse_n
    // so zoom is identity and we time pure routing (the part that was 4s in Python).
    let n = 193usize;
    let span_m = 270000.0;
    let height_scale_m = 1700.0;
    // Deterministic wall-dense field (same shape as the fixture generator: forces real weaving,
    // not trivial straight lines, so the timing reflects genuine least-cost routing work).
    let amp = 2.0_f64;
    let f = 2.0_f64;
    let mut height = vec![0.0_f64; n * n];
    for r in 0..n {
        for c in 0..n {
            let u = c as f64 / (n - 1) as f64;
            let v = r as f64 / (n - 1) as f64;
            let h = (u * 9.0 * f).sin() * (v * 7.0 * f).cos()
                + 0.5 * (u * 17.0 * f + 1.3).sin() * (v * 13.0 * f - 0.7).cos()
                + 0.25 * (u * 31.0 * f).sin() * (v * 29.0 * f).cos();
            height[r * n + c] = h;
        }
    }
    // center/normalize to ~unit std then scale (mirror the fixture field so routing is non-trivial)
    let mean: f64 = height.iter().sum::<f64>() / (n * n) as f64;
    let var: f64 = height.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (n * n) as f64;
    let std = var.sqrt() + 1e-9;
    for x in height.iter_mut() {
        *x = (*x - mean) / std * amp;
    }

    let pp = super::PassNetworkParams::default();
    let tp = super::TraverseParams { scene_width_m: span_m, height_scale_m, ..Default::default() };

    // Warm + measure a few runs; report the median-ish (min is fine for a floor; also report mean).
    let _ = super::carve_routes(&height, n, span_m, height_scale_m, &pp, &tp); // warm
    let runs = 5;
    let mut times_ms = Vec::new();
    let mut routes_len = 0;
    let mut total_pts = 0;
    for _ in 0..runs {
        let t0 = Instant::now();
        let routes = super::carve_routes(&height, n, span_m, height_scale_m, &pp, &tp);
        times_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        routes_len = routes.len();
        total_pts = routes.iter().map(|r| r.len()).sum();
    }
    times_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let best = times_ms[0];
    let median = times_ms[times_ms.len() / 2];
    println!(
        "[carve-cost] n={n} coarse_n={} routes={} total_points={} best={:.3}ms median={:.3}ms (python was ~4000ms)",
        pp.coarse_n, routes_len, total_pts, best, median
    );
    // Non-vacuous: it actually routed something.
    assert!(routes_len > 0 && total_pts > 0, "carve produced no routes (vacuous measurement)");
}

#[test]
fn dijkstra_finds_min_cost_crossing_on_tiny_grid() {
    // 3x3 uniform-cost grid (slope below budget, no channel, h=0 -> step_cost = cell_m everywhere).
    // Source = all of column 0; target = column 2. Shortest path is a straight horizontal line, 3 cells.
    let n = 3;
    let slope = vec![0.0_f64; n * n];
    let h = vec![0.0_f64; n * n];
    let chan = vec![0.0_f64; n * n];
    let p = TraverseParams {
        slope_budget: 0.28,
        slope_penalty: 0.0,
        drainage_bias: 0.0,
        ..Default::default()
    };
    let cell = 100.0;
    let sources: Vec<(usize, usize)> = (0..n).map(|r| (r, 0)).collect();
    let (prev, _dist, target) =
        dijkstra_cost_field(&slope, &h, &chan, n, n, cell, &p, &sources, |_r, c| c == n - 1);
    assert!(target >= 0, "no target reached");
    let path = reconstruct_path(&prev, target as usize, n);
    assert_eq!(path.first().unwrap().1, 0, "path must start at column 0");
    assert_eq!(path.last().unwrap().1, n - 1, "path must end at column n-1");
    assert_eq!(path.len(), n, "straight crossing is n cells");
}

#[test]
fn dijkstra_tiebreak_prefers_lowest_flattened_index() {
    // Uniform-cost grid: every cell costs the same, so EVERY source/path is cost-tied.
    // The (cost, idx) min-heap must resolve those ~ties by lowest flattened index, exactly
    // like Python heapq on (cost, idx) tuples. With all of column 0 as sources, row 0 (idx 0)
    // is the smallest flattened index, so the popped target on row 0 must be reached via the
    // row-0 source, giving a straight path along row 0: [(0,0),(0,1),(0,2)].
    let n = 3;
    let slope = vec![0.0_f64; n * n];
    let h = vec![0.0_f64; n * n];
    let chan = vec![0.0_f64; n * n];
    let p = TraverseParams {
        slope_budget: 0.28,
        slope_penalty: 0.0,
        drainage_bias: 0.0,
        ..Default::default()
    };
    let cell = 100.0;
    let sources: Vec<(usize, usize)> = (0..n).map(|r| (r, 0)).collect();
    let (prev, _dist, target) =
        dijkstra_cost_field(&slope, &h, &chan, n, n, cell, &p, &sources, |_r, c| c == n - 1);
    assert!(target >= 0, "no target reached");
    // Lowest-index target column-2 cell is (0,2) = idx 2; lower-index wins the tie.
    assert_eq!(target, 2, "tie-break must pop the lowest flattened-index target first");
    let path = reconstruct_path(&prev, target as usize, n);
    assert_eq!(
        path,
        vec![(0, 0), (0, 1), (0, 2)],
        "lower-index tie-break yields the straight row-0 path"
    );
}

#[test]
fn step_cost_matches_python_formula() {
    let p = TraverseParams {
        slope_budget: 0.28,
        slope_penalty: 2.0,
        drainage_bias: 0.5,
        ..Default::default()
    };
    let cell = 100.0;
    // slope below budget, chan 0, h>0 -> over=0, reward=0 -> base=cell=100
    assert!((step_cost(0.1, 1.0, 0.0, cell, &p) - 100.0).abs() < 1e-9);
    // slope over budget by 0.22 -> base=100*(1+2*0.22)=144; reward 0 -> 144
    assert!((step_cost(0.5, 1.0, 0.0, cell, &p) - 144.0).abs() < 1e-9);
    // chan=1,h=0 -> reward=0.5*(0.6*1+0.4*0)=0.3; base=100 -> 70
    assert!((step_cost(0.1, 0.0, 1.0, cell, &p) - 70.0).abs() < 1e-9);
    // floor: huge reward can't go below cell*0.05=5
    let p2 = TraverseParams { drainage_bias: 100.0, ..p };
    assert!((step_cost(0.1, -1.0, 1.0, cell, &p2) - 5.0).abs() < 1e-9);
}

#[test]
fn slope_grid_matches_known_ramp() {
    // A linear ramp in x: height[r][c] = c (so y = c*height_scale). With cell_m and height_scale,
    // dx everywhere = height_scale/cell_m (constant), dz = 0 -> slope = height_scale/cell_m everywhere.
    let n = 5usize;
    let scene_width_m = 400.0; // cell_m = 400/4 = 100
    let height_scale_m = 260.0;
    let mut h = vec![0.0_f64; n * n];
    for r in 0..n {
        for c in 0..n {
            h[r * n + c] = c as f64;
        }
    }
    let s = slope_grid(&h, n, scene_width_m, height_scale_m);
    let expected = height_scale_m / 100.0; // 2.6
    for v in &s {
        assert!((v - expected).abs() < 1e-9, "got {v} expected {expected}");
    }
}

#[test]
fn slope_grid_matches_numpy_nonlinear_reference() {
    // Hard parity check against real numpy np.gradient(edge_order=1) on a NON-linear field
    // (so interior central differences vs one-sided edges actually diverge — the linear ramp
    // tests above cannot distinguish them). `expected` was generated by feeding this exact `h`
    // through numpy: y = h*height_scale; dz,dx = np.gradient(y, cell_m, cell_m, edge_order=1);
    // slope = sqrt(dx*dx + dz*dz), with cell_m = scene_width_m/(n-1).
    let n = 6usize;
    let scene_width_m = 1234.5_f64;
    let height_scale_m = 260.0_f64;
    let h: [f64; 36] = [
        -1.4238250364546312, 1.2637284581291104, -0.8706617379590857, -0.2591732349343976,
        -0.07534330701052097, -0.740884652085609, -1.3677927017829434, 0.6488928021930399,
        0.361058113054895, -1.95286306301219, 2.347409654378852, 0.9684969057519236,
        -0.7593871804245066, 0.9021982742122517, -0.46695317332055025, -0.06068951873702798,
        0.7888443445192008, -1.2566681331396765, 0.5758575143959287, 1.3989789947237192,
        1.3222980607327857, -0.29969851529910546, 0.9029193414250598, -1.6215827341822058,
        -0.15818926067687128, 0.44948393210667503, -1.343601072486395, -0.08168759069683368,
        1.7247399323163304, 2.61815942636784, 0.777361343810768, 0.8286331955673406,
        -0.9589883130180109, -1.2093882869743162, -1.4122920134741184, 0.5415468299050529,
    ];
    let expected: [f64; 36] = [
        2.830764516736808, 0.7099518525743221, 1.524914108779285, 1.832053888329938,
        2.563875635575847, 1.9317028177601288, 2.152309690315736, 0.9299803060274752,
        1.3862932745835663, 1.0510800279793375, 1.604070656571484, 1.477252422481672,
        2.027050403542578, 0.42389569058804816, 0.7163764900271036, 1.0931000429180115,
        0.9874232039002444, 2.5494550536561817, 0.9227868522083533, 0.4596582025318273,
        1.0064855254901726, 0.22109165366811037, 0.8527946306295267, 3.3510882359034344,
        0.6486509033766904, 0.692639888871268, 1.2332934358211387, 1.6850779314901092,
        1.8726509693836726, 1.477279540965105, 0.9866673559179334, 0.9976197956016356,
        1.1469680730902345, 1.211282094650801, 3.429706801530175, 3.0025645438282016,
    ];
    let s = slope_grid(&h, n, scene_width_m, height_scale_m);
    for (i, (&got, &exp)) in s.iter().zip(expected.iter()).enumerate() {
        assert!((got - exp).abs() < 1e-12, "idx {i}: got {got} expected {exp}");
    }
}

#[test]
fn slope_grid_diagonal_ramp_uses_hypot() {
    // 2D ramp: height[r][c] = r + c. Both dz and dx == height_scale/cell_m everywhere
    // (central interior AND one-sided edges, since the field is linear). slope == that * sqrt(2).
    let n = 5usize;
    let scene_width_m = 400.0; // cell_m = 100
    let height_scale_m = 260.0;
    let mut h = vec![0.0_f64; n * n];
    for r in 0..n {
        for c in 0..n {
            h[r * n + c] = (r + c) as f64;
        }
    }
    let s = slope_grid(&h, n, scene_width_m, height_scale_m);
    let g = height_scale_m / 100.0; // per-axis gradient 2.6
    let expected = (2.0_f64).sqrt() * g;
    for v in &s {
        assert!((v - expected).abs() < 1e-9, "got {v} expected {expected}");
    }
}

// --- EDT (exact Euclidean distance transform with nearest-feature index) ---
// Backs carve_ramp's `distance_transform_edt(~on_path, return_indices=True)` (corridor_router.py:256):
// distance to nearest on-path cell + the flat index of that cell (to gather the floor profile).

#[test]
fn edt_distance_and_index_1d_row() {
    // 1x5, feature at col 2. distances 2,1,0,1,2; nearest idx all = 2.
    let (dist, idx) = edt_with_indices(&[false, false, true, false, false], 1, 5);
    let exp = [2.0, 1.0, 0.0, 1.0, 2.0];
    for c in 0..5 {
        assert!((dist[c] - exp[c]).abs() < 1e-9, "dist[{c}]={}", dist[c]);
    }
    for c in 0..5 {
        assert_eq!(idx[c], 2);
    }
}

#[test]
fn edt_euclidean_diagonal() {
    // 3x3, feature at (0,0)=idx0. distance at (2,2) = sqrt(8); nearest idx 0.
    let mut feat = vec![false; 9];
    feat[0] = true;
    let (dist, idx) = edt_with_indices(&feat, 3, 3);
    assert!((dist[8] - (8.0_f64).sqrt()).abs() < 1e-9, "got {}", dist[8]);
    assert_eq!(idx[8], 0);
}

#[test]
fn edt_two_features_picks_nearest_index() {
    // 1x7, features at col 1 (idx1) and col 5 (idx5). col 2 -> nearest idx1 (dist1);
    // col 4 -> nearest idx5 (dist1); col 3 -> tie dist2, idx is implementation tie-break
    // (assert it's 1 or 5).
    let mut feat = vec![false; 7];
    feat[1] = true;
    feat[5] = true;
    let (dist, idx) = edt_with_indices(&feat, 1, 7);
    assert!((dist[2] - 1.0).abs() < 1e-9);
    assert_eq!(idx[2], 1);
    assert!((dist[4] - 1.0).abs() < 1e-9);
    assert_eq!(idx[4], 5);
    assert!((dist[3] - 2.0).abs() < 1e-9);
    assert!(idx[3] == 1 || idx[3] == 5, "tie idx {}", idx[3]);
}

#[test]
fn edt_validates_against_scipy_on_small_grid() {
    // Spot-checked against scipy.ndimage.distance_transform_edt(~on_path) for a 5x5 with features
    // (true) at idx 0=(0,0), 12=(2,2), 24=(4,4). Distances are Euclidean (cell size 1).
    let mut feat = vec![false; 25];
    feat[0] = true;
    feat[12] = true;
    feat[24] = true;
    let (dist, _idx) = edt_with_indices(&feat, 5, 5);
    // (1,1)=idx6: nearest is (0,0) or (2,2), both sqrt(2) -> dist sqrt(2).
    assert!((dist[6] - (2.0_f64).sqrt()).abs() < 1e-9, "got {}", dist[6]);
    // (0,4)=idx4: nearest (2,2) = sqrt(4+4) = sqrt(8) (beats (0,0)=4 and (4,4)=4).
    assert!((dist[4] - (8.0_f64).sqrt()).abs() < 1e-9, "got {}", dist[4]);
    // (4,0)=idx20: symmetric -> (2,2) dist sqrt(8).
    assert!((dist[20] - (8.0_f64).sqrt()).abs() < 1e-9, "got {}", dist[20]);
}

#[test]
fn edt_feature_cells_are_self() {
    // Every feature cell: distance 0, index = itself.
    let mut feat = vec![false; 9];
    feat[0] = true;
    feat[4] = true;
    feat[8] = true;
    let (dist, idx) = edt_with_indices(&feat, 3, 3);
    for &i in &[0usize, 4, 8] {
        assert!((dist[i] - 0.0).abs() < 1e-12, "feature dist[{i}]={}", dist[i]);
        assert_eq!(idx[i], i, "feature idx[{i}] must be self");
    }
}

#[test]
fn edt_full_grid_vs_brute_force() {
    // Exhaustive cross-check on a non-trivial 6x7 grid: compare exact-EDT distances against a
    // brute-force nearest-feature scan, and verify each carried index actually sits at that
    // minimal distance (guards the separable INDEX carry through both 1D passes).
    let rows = 6usize;
    let cols = 7usize;
    let mut feat = vec![false; rows * cols];
    // A scattered, asymmetric set of features (no symmetry to hide carry bugs).
    for &i in &[3usize, 9, 16, 22, 30, 41] {
        feat[i] = true;
    }
    let (dist, idx) = edt_with_indices(&feat, rows, cols);
    let feats: Vec<(i64, i64)> = (0..rows * cols)
        .filter(|&i| feat[i])
        .map(|i| ((i / cols) as i64, (i % cols) as i64))
        .collect();
    for r in 0..rows {
        for c in 0..cols {
            let q = r * cols + c;
            // brute-force exact nearest distance
            let mut best = f64::INFINITY;
            for &(fr, fc) in &feats {
                let dr = r as i64 - fr;
                let dc = c as i64 - fc;
                let d2 = (dr * dr + dc * dc) as f64;
                if d2 < best {
                    best = d2;
                }
            }
            let best = best.sqrt();
            assert!((dist[q] - best).abs() < 1e-9, "dist[{q}]={} brute={best}", dist[q]);
            // the carried index must be a feature, and exactly at the minimal distance
            let s = idx[q];
            assert!(s != usize::MAX, "idx[{q}] unset");
            assert!(feat[s], "idx[{q}]={s} is not a feature");
            let (sr, sc) = ((s / cols) as i64, (s % cols) as i64);
            let dr = r as i64 - sr;
            let dc = c as i64 - sc;
            let carried = ((dr * dr + dc * dc) as f64).sqrt();
            assert!(
                (carried - best).abs() < 1e-9,
                "carried idx[{q}]={s} at dist {carried} but nearest is {best}"
            );
        }
    }
}
