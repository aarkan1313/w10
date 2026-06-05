//! Connected pass-network routing, ported from the offline Python
//! (mountain_pass_network.py / traverse_corridor.py) to fast Rust. Reproduces the
//! SAME routes (same chunk-network look) at a fraction of the cost. Pure Rust, no Godot.

pub mod cost;
pub mod dijkstra;
pub mod routes;

#[cfg(test)]
mod tests;

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
