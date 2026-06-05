//! Exact Euclidean distance transform with nearest-feature index, in the spirit of
//! `scipy.ndimage.distance_transform_edt(return_indices=True)`. Separable parabola-envelope method
//! (Felzenszwalb & Huttenlocher, "Distance Transforms of Sampled Functions", 2012): EXACT Euclidean
//! distances. The nearest-INDEX tie-break may differ from scipy on exact ties; acceptable here
//! because the result feeds carve_ramp's Gaussian smooth + clamp (corridor_router.py:256-265), and a
//! downstream TOLERANCE gate absorbs the difference. Distances are exact regardless of tie-break.
//!
//! Backs `distance_transform_edt(~on_path, return_indices=True)`: callers pass `feature[i] = true`
//! for the target (on-path) cells; we return, per cell, the Euclidean distance to the nearest
//! feature cell plus that cell's flat (row-major) index.

const INF: f64 = 1e20;

/// Exact Euclidean distance transform.
///
/// `feature[i] == true` marks the target cells. Returns `(dist, nearest_idx)`:
/// - `dist[i]`: exact Euclidean distance (in pixels, cell size 1) from cell `i` to the nearest
///   feature cell. Row-major, length `rows*cols`. `0.0` at feature cells.
/// - `nearest_idx[i]`: flat index (`r*cols + c`) of that nearest feature cell. At a feature cell,
///   the index is the cell itself. If there are NO feature cells, distances are `INF` and indices
///   are `usize::MAX`.
///
/// Method: seed squared-distance `0` at feature cells / `+INF` elsewhere, run the 1D lower-envelope
/// transform down each column, then along each row. The source index is carried through both 1D
/// passes (each column's pass yields the nearest feature *within that column*; the row pass then
/// selects, per cell, the column whose carried squared-distance is minimal and adopts ITS carried
/// index), so `nearest_idx` ends up holding the true 2D nearest feature.
pub fn edt_with_indices(feature: &[bool], rows: usize, cols: usize) -> (Vec<f64>, Vec<usize>) {
    assert_eq!(
        feature.len(),
        rows * cols,
        "edt_with_indices: feature.len()={} must equal rows*cols={}",
        feature.len(),
        rows * cols
    );
    let n = rows * cols;
    let mut f = vec![INF; n];
    let mut src = vec![usize::MAX; n];
    for i in 0..n {
        if feature[i] {
            f[i] = 0.0;
            src[i] = i;
        }
    }
    if rows == 0 || cols == 0 {
        return (Vec::new(), Vec::new());
    }

    // Pass 1: 1D transform down each column (axis 0). After this, f[r*cols+c] is the squared
    // distance to the nearest feature in column c, and src carries that feature's flat index.
    let mut g = vec![INF; rows];
    let mut gs = vec![usize::MAX; rows];
    for c in 0..cols {
        for r in 0..rows {
            g[r] = f[r * cols + c];
            gs[r] = src[r * cols + c];
        }
        let (d, s) = edt_1d(&g, &gs);
        for r in 0..rows {
            f[r * cols + c] = d[r];
            src[r * cols + c] = s[r];
        }
    }

    // Pass 2: 1D transform along each row (axis 1). Combines the per-column results into the true
    // 2D squared distance, and propagates the matching nearest-feature index.
    let mut g = vec![INF; cols];
    let mut gs = vec![usize::MAX; cols];
    for r in 0..rows {
        let base = r * cols;
        for c in 0..cols {
            g[c] = f[base + c];
            gs[c] = src[base + c];
        }
        let (d, s) = edt_1d(&g, &gs);
        for c in 0..cols {
            f[base + c] = d[c];
            src[base + c] = s[c];
        }
    }

    let dist: Vec<f64> = f.iter().map(|v| v.max(0.0).sqrt()).collect();
    (dist, src)
}

/// 1D squared-distance transform carrying a source index.
///
/// Given seeds `g` (squared distances; `INF` where there is no seed) and their source indices `gs`,
/// returns `(d, s)` where `d[q] = min_p ( (q - p)^2 + g[p] )` and `s[q] = gs[p*]` for the minimizing
/// `p*`. This is the lower-envelope-of-parabolas algorithm (FH 2012): each seed `p` contributes the
/// parabola `(x - p)^2 + g[p]`; we compute the lower envelope, then sample it at each integer `q`.
///
/// Envelope edge cases handled:
/// - all-`INF` `g` (a row/column with no feature): every `d[q]` stays `INF`, every `s[q]` stays
///   `usize::MAX`; the perpendicular pass fixes such cells.
/// - an `INF` prefix followed by a finite seed: the dominating finite parabola correctly evicts the
///   `INF` parabola at the start (`k == 0` reset below).
fn edt_1d(g: &[f64], gs: &[usize]) -> (Vec<f64>, Vec<usize>) {
    let n = g.len();
    let mut d = vec![0.0_f64; n];
    let mut s = vec![usize::MAX; n];
    if n == 0 {
        return (d, s);
    }
    // v[k]: location of the k-th parabola in the lower envelope.
    // z[k]..z[k+1]: the x-range over which v[k] is the lowest parabola.
    let mut v = vec![0usize; n];
    let mut z = vec![0.0_f64; n + 1];
    let mut k = 0usize;
    v[0] = 0;
    z[0] = -INF;
    z[1] = INF;
    for q in 1..n {
        loop {
            let p = v[k];
            // Intersection (in x) of the parabolas rooted at p and q.
            let denom = 2.0 * q as f64 - 2.0 * p as f64;
            let sint = ((g[q] + (q * q) as f64) - (g[p] + (p * p) as f64)) / denom;
            if sint <= z[k] {
                // q's parabola hides v[k] entirely from this side; pop v[k].
                if k == 0 {
                    // v[k] was the first parabola; q dominates from the start. Replace it.
                    v[0] = q;
                    break;
                }
                k -= 1;
            } else {
                k += 1;
                v[k] = q;
                z[k] = sint;
                z[k + 1] = INF;
                break;
            }
        }
    }
    k = 0;
    for q in 0..n {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f64 - p as f64;
        d[q] = dq * dq + g[p];
        s[q] = gs[p];
    }
    (d, s)
}
