//! Connected pass-network routing, ported from the offline Python
//! (mountain_pass_network.py / traverse_corridor.py) to fast Rust. Reproduces the
//! SAME routes (same chunk-network look) at a fraction of the cost. Pure Rust, no Godot.

pub mod carve;
pub mod cost;
pub mod dijkstra;
pub mod edt;
pub mod routes;

#[cfg(test)]
mod tests;

/// Mirror of the `carve_ramp` knobs on Python `CorridorParams` (corridor_router.py). Defaults
/// verified against `CorridorParams` (the `ramp_*` family + `slope_budget`).
#[derive(Clone, Copy, Debug)]
pub struct RampParams {
    pub slope_budget: f64,
    pub floor_grade_frac: f64,
    pub wall_grade_frac: f64,
    pub flat_half_m: f64,
    pub half_width_m: f64,
    pub floor_smooth_px: f64,
    pub carve_max_m: f64,
}
impl Default for RampParams {
    fn default() -> Self {
        Self {
            slope_budget: 0.28,
            floor_grade_frac: 0.35,
            wall_grade_frac: 0.80,
            flat_half_m: 200.0,
            half_width_m: 1200.0,
            floor_smooth_px: 5.0,
            carve_max_m: 3500.0,
        }
    }
}

/// carve_ramp delta for a core height grid + routes. Mirror of `carve_ramp`
/// (corridor_router.py:213-266) with the `_core` step as identity (the routes are already in core
/// index space). Returns the subtractive delta in HEIGHT units (n*n, row-major); add it to `height`.
pub fn carve_ramp_delta(
    height: &[f64],
    n: usize,
    span_m: f64,
    height_scale_m: f64,
    routes: &[Vec<(usize, usize)>],
    ramp: &RampParams,
) -> Vec<f64> {
    let cell_m = span_m / (n - 1) as f64;
    carve::carve_ramp(height, n, cell_m, height_scale_m, routes, ramp)
}

/// Public entry: connected pass-network routes for a single height field, full-res index space.
/// Mirror of `mountain_pass_network._routes` (the routing half of `carve_pass_network`).
pub fn carve_routes(
    height: &[f64],
    n: usize,
    span_m: f64,
    height_scale_m: f64,
    params: &PassNetworkParams,
    traverse: &TraverseParams,
) -> Vec<Vec<(usize, usize)>> {
    routes::routes(height, n, span_m, height_scale_m, traverse, params)
}

/// Mirror of Python `PassNetworkParams` (mountain_pass_network.py:30-37). Same defaults.
#[derive(Clone, Copy, Debug)]
pub struct PassNetworkParams {
    pub n_we: usize,
    pub n_ns: usize,
    pub coarse_n: usize,
}
impl Default for PassNetworkParams {
    fn default() -> Self {
        Self { n_we: 4, n_ns: 4, coarse_n: 193 }
    }
}

/// Mirror of the Python `TraverseParams` fields that feed `_step_cost`/`slope_grid`.
/// Defaults verified against traverse_corridor.py and analyze_rough_world_traversability.py.
#[derive(Clone, Copy, Debug)]
pub struct TraverseParams {
    pub slope_budget: f64,    // traverse_corridor.py: 0.28 (== PASSABLE_SLOPE)
    pub slope_penalty: f64,   // traverse_corridor.py: 24.0
    pub drainage_bias: f64,   // traverse_corridor.py: 0.55
    pub scene_width_m: f64,
    pub height_scale_m: f64,  // analyze_rough_world_traversability.BASE_HEIGHT_SCALE_M = 260.0
}
impl Default for TraverseParams {
    fn default() -> Self {
        Self {
            slope_budget: 0.28,
            slope_penalty: 24.0,
            drainage_bias: 0.55,
            scene_width_m: 25600.0,
            height_scale_m: 260.0,
        }
    }
}
