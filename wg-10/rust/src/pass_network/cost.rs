//! Cost model for the pass-network Dijkstra: per-step edge cost + the slope field.
//! Ported bit-faithfully from the offline Python so the routes (and thus the terrain
//! look) reproduce exactly.

use super::TraverseParams;

/// Per-step edge cost. Mirror of `traverse_corridor._step_cost` (py lines 88-92):
/// ```text
/// over = max(0.0, slope_b - p.slope_budget)
/// base = cell_m * (1.0 + p.slope_penalty * over)
/// reward = p.drainage_bias * (0.6 * chan_b + 0.4 * clip(-h_b, 0.0, 1.0))
/// return max(base * (1.0 - reward), cell_m * 0.05)
/// ```
/// f64 throughout to match Python's float math.
pub fn step_cost(slope_b: f64, h_b: f64, chan_b: f64, cell_m: f64, p: &TraverseParams) -> f64 {
    let over = (slope_b - p.slope_budget).max(0.0);
    let base = cell_m * (1.0 + p.slope_penalty * over);
    let reward = p.drainage_bias * (0.6 * chan_b + 0.4 * (-h_b).clamp(0.0, 1.0));
    (base * (1.0 - reward)).max(cell_m * 0.05)
}

/// Slope magnitude over a height grid. Mirror of
/// `analyze_rough_world_traversability.slope_grid` (py lines 61-68):
/// ```text
/// cell_m = scene_width_m / (n - 1)
/// y = height * height_scale_m
/// dz, dx = np.gradient(y, cell_m, cell_m, edge_order=1)   # dz=axis0 (rows), dx=axis1 (cols)
/// slope = sqrt(dx*dx + dz*dz)
/// ```
/// `h` is `n*n` row-major; returns `n*n` row-major. The vertical scale is applied to the
/// height BEFORE differencing (scale then gradient), exactly as numpy does.
///
/// `np.gradient(..., edge_order=1)` semantics with a scalar spacing `cell_m`:
/// - interior point i: central difference `(y[i+1] - y[i-1]) / (2*cell_m)`
/// - first edge (i=0): one-sided first difference `(y[1] - y[0]) / cell_m`
/// - last edge (i=n-1): one-sided first difference `(y[n-1] - y[n-2]) / cell_m`
pub fn slope_grid(h: &[f64], n: usize, scene_width_m: f64, height_scale_m: f64) -> Vec<f64> {
    assert!(n >= 2, "slope_grid requires n >= 2");
    assert_eq!(h.len(), n * n, "slope_grid: h.len() must equal n*n");

    let cell_m = scene_width_m / (n as f64 - 1.0);

    // y = height * height_scale_m (scale BEFORE differencing).
    let y: Vec<f64> = h.iter().map(|&v| v * height_scale_m).collect();

    let idx = |r: usize, c: usize| r * n + c;

    // dz = gradient along axis 0 (down the rows; varying r, fixed c).
    // dx = gradient along axis 1 (across the cols; varying c, fixed r).
    let mut slope = vec![0.0_f64; n * n];
    for r in 0..n {
        for c in 0..n {
            // dz: row-direction (axis 0)
            let dz = if r == 0 {
                (y[idx(1, c)] - y[idx(0, c)]) / cell_m
            } else if r == n - 1 {
                (y[idx(n - 1, c)] - y[idx(n - 2, c)]) / cell_m
            } else {
                (y[idx(r + 1, c)] - y[idx(r - 1, c)]) / (2.0 * cell_m)
            };
            // dx: col-direction (axis 1)
            let dx = if c == 0 {
                (y[idx(r, 1)] - y[idx(r, 0)]) / cell_m
            } else if c == n - 1 {
                (y[idx(r, n - 1)] - y[idx(r, n - 2)]) / cell_m
            } else {
                (y[idx(r, c + 1)] - y[idx(r, c - 1)]) / (2.0 * cell_m)
            };
            slope[idx(r, c)] = (dx * dx + dz * dz).sqrt();
        }
    }
    slope
}
