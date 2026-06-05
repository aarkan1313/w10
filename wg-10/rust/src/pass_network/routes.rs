//! Coarse-grid WE+NS least-cost crossings, ported from mountain_pass_network._routes (py:40-61).
use super::cost::slope_grid;
use super::dijkstra::{dijkstra_cost_field, reconstruct_path};
use super::{PassNetworkParams, TraverseParams};

/// Bilinear downsample to m x m. IDENTITY (exact) when n==m (the fixture case; isolates routing).
pub fn zoom_bilinear(h: &[f64], n: usize, m: usize) -> Vec<f64> {
    if n == m {
        return h.to_vec();
    }
    // general bilinear (NOT exercised by the n==coarse_n fixture; correctness here is best-effort
    // until a separate n!=coarse_n fixture exists). scipy zoom(order=1) maps out coord o -> in o/sc, sc=m/n.
    let mut out = vec![0.0_f64; m * m];
    let inv = n as f64 / m as f64;
    for orr in 0..m {
        for occ in 0..m {
            let fr = orr as f64 * inv;
            let fc = occ as f64 * inv;
            let r0 = (fr.floor() as usize).min(n - 1);
            let c0 = (fc.floor() as usize).min(n - 1);
            let r1 = (r0 + 1).min(n - 1);
            let c1 = (c0 + 1).min(n - 1);
            let tr = fr - r0 as f64;
            let tc = fc - c0 as f64;
            let a = h[r0 * n + c0];
            let b = h[r0 * n + c1];
            let c = h[r1 * n + c0];
            let d = h[r1 * n + c1];
            let ab = a + (b - a) * tc;
            let cd = c + (d - c) * tc;
            out[orr * m + occ] = ab + (cd - ab) * tr;
        }
    }
    out
}

/// Explicit grid transpose: `t[c*nc+r] = g[r*nc+c]`. Matches numpy `.T` on a square nc*nc array.
fn transpose(g: &[f64], nc: usize) -> Vec<f64> {
    let mut t = vec![0.0_f64; nc * nc];
    for r in 0..nc {
        for c in 0..nc {
            t[c * nc + r] = g[r * nc + c];
        }
    }
    t
}

/// Mirror of mountain_pass_network._routes (py:40-61). Returns routes in FULL-RES index space.
///
/// WE crossings route on `(slc, hc)`; NS crossings route on the TRANSPOSE `(slc.T, hc.T)` and
/// map back with the coord SWAP `(cc/sc, rr/sc)` (Python py:60). `ch` is all-zeros and symmetric,
/// so the same buffer serves the transposed solve.
pub fn routes(
    height: &[f64],
    n: usize,
    span_m: f64,
    height_scale_m: f64,
    p: &TraverseParams,
    pp: &PassNetworkParams,
) -> Vec<Vec<(usize, usize)>> {
    let nc = pp.coarse_n;
    let sc = nc as f64 / n as f64;
    let hc = zoom_bilinear(height, n, nc);
    let slc = slope_grid(&hc, nc, span_m, height_scale_m);
    let cm = span_m / (nc - 1) as f64;
    let ch = vec![0.0_f64; nc * nc];
    let mut out: Vec<Vec<(usize, usize)>> = Vec::new();

    // WE crossings: seed (r0, 0), target c == nc-1, map (rr,cc) -> (rr/sc, cc/sc).
    for k in 0..pp.n_we {
        let r0 = ((k as f64 + 0.5) / pp.n_we as f64 * nc as f64) as usize;
        let (prev, _d, tgt) =
            dijkstra_cost_field(&slc, &hc, &ch, nc, nc, cm, p, &[(r0, 0)], |_r, c| c == nc - 1);
        if tgt >= 0 {
            let path = reconstruct_path(&prev, tgt as usize, nc);
            out.push(
                path.into_iter()
                    .map(|(rr, cc)| ((rr as f64 / sc) as usize, (cc as f64 / sc) as usize))
                    .collect(),
            );
        }
    }

    // NS crossings: route on transposed fields, seed (c0, 0), target c == nc-1,
    // map (rr,cc) -> (cc/sc, rr/sc)  <-- coord SWAP vs WE (Python py:60).
    let slc_t = transpose(&slc, nc);
    let hc_t = transpose(&hc, nc);
    for k in 0..pp.n_ns {
        let c0 = ((k as f64 + 0.5) / pp.n_ns as f64 * nc as f64) as usize;
        let (prev, _d, tgt) = dijkstra_cost_field(
            &slc_t,
            &hc_t,
            &ch,
            nc,
            nc,
            cm,
            p,
            &[(c0, 0)],
            |_r, c| c == nc - 1,
        );
        if tgt >= 0 {
            let path = reconstruct_path(&prev, tgt as usize, nc);
            out.push(
                path.into_iter()
                    .map(|(rr, cc)| ((cc as f64 / sc) as usize, (rr as f64 / sc) as usize))
                    .collect(),
            );
        }
    }

    out
}
