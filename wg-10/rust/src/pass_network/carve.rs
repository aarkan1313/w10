//! `carve_ramp` — cut a walkable VALLEY through a steep barrier from the pass-network routes.
//!
//! Bit-faithful port of `carve_ramp` (corridor_router.py:213-266), with the Python `_core(full, spec)`
//! step taken as identity (the routes are already in core index space). For each route it builds a
//! slope-feasible floor along the path (graded at the REDUCED grade `slope_budget * floor_grade_frac`
//! so the combined 2D gradient stays within budget), scatters it to a field, smooths it, and grades
//! walls up away from the route; the deepest carve wins where routes overlap. Subtractive (<=0),
//! bounded by `carve_max_m`, returned in HEIGHT units.
//!
//! Mapping of the two numpy idioms that aren't per-point loops:
//! * `distance_transform_edt(~on_path, return_indices=True)` -> [`edt_with_indices`] with
//!   `feature = on_path` (true = on-path): returns the pixel distance to the nearest on-path cell and
//!   that cell's flat index.
//! * `prof_field[iy, ix]` (numpy fancy-index by the EDT's nearest-index pair) -> the per-cell gather
//!   `gathered[i] = prof_field[nearest[i]]`.
//! * `gaussian_filter(..., sigma)` -> [`gaussian_filter_nearest`] (scipy default `truncate = 4.0`).

use super::edt::edt_with_indices;
use super::RampParams;
use crate::array_ops::gaussian_filter_reflect;

/// Carve a walkable valley delta (HEIGHT units, n*n row-major) for `routes` over the core grid
/// `height`. `cell_m` is the per-cell spacing in metres (`spec.spacing_m`). Mirror of
/// corridor_router.py:226-266.
pub fn carve_ramp(
    height: &[f64],
    n: usize,
    cell_m: f64,
    height_scale_m: f64,
    routes: &[Vec<(usize, usize)>],
    p: &RampParams,
) -> Vec<f64> {
    // core_m = core * height_scale_m (height in metres).
    let core_m: Vec<f64> = height.iter().map(|h| h * height_scale_m).collect();
    let mut delta_m = vec![0.0_f64; n * n];
    let budget = p.slope_budget;

    for route in routes {
        if route.is_empty() {
            continue;
        }
        let m = route.len();

        // 1) slope-feasible floor ALONG the route at the REDUCED grade (margin for cross-slope).
        //    prof[i] = min over forward then backward sweeps of the budgeted step.
        let mut prof: Vec<f64> = route.iter().map(|&(r, c)| core_m[r * n + c]).collect();
        let step = budget * p.floor_grade_frac * cell_m;
        for i in 1..m {
            prof[i] = prof[i].min(prof[i - 1] + step);
        }
        for i in (0..m - 1).rev() {
            prof[i] = prof[i].min(prof[i + 1] + step);
        }

        // 2) scatter to a floor field, EDT-gather the nearest-on-path profile, smooth, grade walls up.
        let mut on_path = vec![false; n * n];
        let mut prof_field = vec![f64::INFINITY; n * n];
        for (k, &(r, c)) in route.iter().enumerate() {
            on_path[r * n + c] = true;
            prof_field[r * n + c] = prof[k];
        }
        let (distpx, nearest) = edt_with_indices(&on_path, n, n);
        // prof_field[iy, ix]: for each cell, the profile value at its nearest on-path cell.
        let gathered: Vec<f64> = (0..n * n).map(|i| prof_field[nearest[i]]).collect();
        // scipy gaussian_filter DEFAULT mode='reflect' (corridor_router.carve_ramp passes no mode);
        // the gathered floor has thousands-of-metres border gradients, so 'nearest' would diverge by
        // 100m+ at the edges (latent: the narrow-band carve_ramp fixture excluded those border cells).
        let floor = gaussian_filter_reflect(&gathered, n, n, p.floor_smooth_px, 4.0);

        for i in 0..n * n {
            let d_m = distpx[i] * cell_m;
            let wall_rise = (d_m - p.flat_half_m).max(0.0) * (budget * p.wall_grade_frac);
            let target = floor[i] + wall_rise;
            // band = d_m <= half_width_m; outside the band this route contributes 0 (no carve).
            if d_m <= p.half_width_m {
                let this = (target - core_m[i]).min(0.0);
                // delta_m = min(delta_m, this): deepest carve wins where routes overlap.
                if this < delta_m[i] {
                    delta_m[i] = this;
                }
            }
        }
    }

    // clip(delta_m, -carve_max_m, 0) then back to height units.
    for v in delta_m.iter_mut() {
        *v = v.clamp(-p.carve_max_m, 0.0) / height_scale_m;
    }
    delta_m
}
