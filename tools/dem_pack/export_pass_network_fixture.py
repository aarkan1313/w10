"""Emit the Python pass-network ROUTING parity fixture.

The connected pass-network carve (least-cost-path valley routing -- the routing that gives the
terrain its good look) lives ONLY as offline pure-Python Dijkstra in `mountain_pass_network._routes`.
It was never ported to the live Rust/GPU engine. We are porting that routing to Rust bit-faithfully,
and this script produces the parity ORACLE the Rust port must reproduce EXACTLY: a committed JSON
fixture holding a deterministic input height grid + the exact cost-model params + the exact routes
Python produces.

Run from repo root:
    python tools/dem_pack/export_pass_network_fixture.py

Writes:
    tools/dem_pack/fixtures/pass_network_routes_fixture.json

Determinism: the height field is a fixed analytic sum-of-sines (NO numpy global RNG), so re-runs are
bit-identical. n is chosen == PassNetworkParams.coarse_n so the internal `zoom` downsample is identity
(scale factor 1.0), isolating routing from interpolation in the fixture.

Cost model captured: `_routes` routes via `traverse_corridor._dijkstra_cost_field` whose per-step cost
(`_step_cost`, traverse_corridor.py ~88-92) reads `slope_budget`, `slope_penalty`, `drainage_bias` off
the TraverseParams, plus a per-cell `cell_m = span_m/(n-1)` and the slope field from
`analyze_rough_world_traversability.slope_grid` (which itself reads only scene_width_m + height_scale_m,
both already in the fixture). In `_routes` the channel field is all-zeros, so the only cost inputs that
are NOT derivable from (height, span_m, height_scale_m, n) are those three TraverseParams scalars -- so
those are the values the Rust port needs and the ones we record under `params`.
"""

from __future__ import annotations

import dataclasses
import json
from pathlib import Path

import numpy as np

# mountain_pass_network / traverse_corridor / analyze_rough_world_traversability are flat modules under
# tools/dem_pack/ that import each other by bare name -- put that dir on sys.path before importing them.
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))

import mountain_pass_network as mpn          # noqa: E402
import traverse_corridor as tc               # noqa: E402
import analyze_rough_world_traversability as trav  # noqa: E402


# Field tuning. The cost model's only spatial signal that can BEND a route is the slope-penalty
# over-budget branch in _step_cost: base = cell_m * (1 + slope_penalty * max(0, slope - slope_budget)).
# If no cell exceeds slope_budget, `over` is 0 everywhere, base cost is constant, and straight lines are
# trivially optimal -> the routes are degenerate and the oracle is worthless (a broken straight-line
# Rust Dijkstra would pass). So the field MUST produce slope > slope_budget in BANDS, forcing routes to
# weave around over-budget walls toward passable gaps. AMPLITUDE_MULT * FREQ_MULT below were tuned so the
# slope crosses 0.28 over a large fraction of cells AND every route weaves substantially (free axis
# varies by tens of cells); see the verification + acceptance assertions in main(). Both are deterministic
# integer/float multipliers -- no RNG -- so re-runs stay bit-identical.
AMPLITUDE_MULT = 2.0   # scales the field so slope crosses slope_budget in ridges (over-budget walls)
FREQ_MULT = 2.0        # raises spatial frequencies so the over-budget ridges form a weave-forcing maze


def build_height(n: int) -> np.ndarray:
    """Deterministic synthetic height: a ridged sum-of-sines with over-budget walls and passable gaps.

    No RNG -- pure analytic f(u, v) so re-runs are bit-identical. Centered/normalized to ~unit std (like
    the tanh-conditioned field the real pipeline routes over: height ~0 == low ground, which the
    drainage_bias reward in _step_cost pulls routes toward), then scaled by AMPLITUDE_MULT at FREQ_MULT
    spatial frequency so slope exceeds slope_budget in bands -- the over-budget walls the routes must
    thread around (NOT a flat field, which yields trivial straight-line routes)."""
    ys, xs = np.mgrid[0:n, 0:n].astype(float)
    u = xs / (n - 1)
    v = ys / (n - 1)
    f = FREQ_MULT
    h = (
        np.sin(u * 9.0 * f) * np.cos(v * 7.0 * f)
        + 0.5 * np.sin(u * 17.0 * f + 1.3) * np.cos(v * 13.0 * f - 0.7)
        + 0.25 * np.sin(u * 31.0 * f) * np.cos(v * 29.0 * f)
    )
    h = (h - h.mean()) / (h.std() + 1e-9)   # ~tanh-centered like the conditioned field
    return h * AMPLITUDE_MULT


def main() -> None:
    # n == PassNetworkParams.coarse_n so the internal zoom() downsample is identity (isolates routing).
    n = 193
    span_m = 270000.0
    height_scale_m = 1700.0

    pp = mpn.PassNetworkParams()
    assert n == pp.coarse_n, f"n ({n}) must equal PassNetworkParams.coarse_n ({pp.coarse_n}) for identity zoom"

    # Mirror how mountain_pass_network sets up p_trav (carve_pass_network does the same replace()).
    p_trav = tc.TraverseParams()
    p_trav = dataclasses.replace(p_trav, scene_width_m=span_m, height_scale_m=height_scale_m)

    h = build_height(n)

    # VERIFY the field is non-trivial for the cost model: slope must exceed slope_budget so the
    # slope-penalty branch in _step_cost is active and routing actually has to avoid walls. A field with
    # slope.max() <= slope_budget makes every route a degenerate straight line (the oracle would be
    # worthless). Require a meaningful over-budget fraction (walls with passable gaps), not a single spike.
    slopes = trav.slope_grid(h, scene_width_m=span_m, height_scale_m=height_scale_m)
    slope_max = float(slopes.max())
    over_frac = float(np.mean(slopes > p_trav.slope_budget))
    if not (slope_max > p_trav.slope_budget):
        raise SystemExit(
            f"[fixture] DEGENERATE field: slope max {slope_max:.4f} <= slope_budget "
            f"{p_trav.slope_budget:.4f} -> slope-penalty inert, routes would be trivial straight lines. "
            f"Increase AMPLITUDE_MULT/FREQ_MULT."
        )
    if not (over_frac > 0.10):
        raise SystemExit(
            f"[fixture] WEAK field: only {over_frac*100:.1f}% of cells over slope_budget "
            f"{p_trav.slope_budget:.4f} (want >10%) -> too few walls to force weaving. "
            f"Increase AMPLITUDE_MULT/FREQ_MULT."
        )

    routes = mpn._routes(h, span_m, height_scale_m, p_trav, pp)

    # ACCEPTANCE: routes must be non-empty AND actually WEAVE (the free axis must vary), so a trivially-
    # broken straight-line Rust Dijkstra cannot pass this parity gate. WE routes (the first n_we) cross
    # west->east, so their FREE axis is the row (index 0); NS routes (the rest) cross north->south, so
    # their free axis is the col (index 1). A free-axis range of 0 == a perfectly straight line == a
    # degenerate fixture; refuse to write it.
    if not routes:
        raise SystemExit("[fixture] no routes found -- the field has no crossable ground.")
    n_we = int(pp.n_we)

    def _free_axis_range(route, axis: int) -> int:
        vals = [pt[axis] for pt in route]
        return int(max(vals) - min(vals))

    for k, route in enumerate(routes):
        is_we = k < n_we
        axis = 0 if is_we else 1            # WE free axis = row; NS free axis = col
        rng = _free_axis_range(route, axis)
        if rng <= 0:
            kind = "WE" if is_we else "NS"
            axis_name = "row" if is_we else "col"
            raise SystemExit(
                f"[fixture] DEGENERATE route #{k} ({kind}): free axis ({axis_name}) range {rng} <= 0 -> "
                f"a perfectly straight line. Routing is trivial; refusing to write the fixture. "
                f"Make the field weave (raise AMPLITUDE_MULT/FREQ_MULT so slope crosses slope_budget in "
                f"bands)."
            )

    # Capture the EXACT cost-model scalars the Rust port must match. slope_budget/slope_penalty/
    # drainage_bias feed _step_cost; n_we/n_ns/coarse_n control how many routes and the routing grid.
    params = {
        "n_we": int(pp.n_we),
        "n_ns": int(pp.n_ns),
        "coarse_n": int(pp.coarse_n),
        "slope_budget": float(p_trav.slope_budget),
        "slope_penalty": float(p_trav.slope_penalty),
        "drainage_bias": float(p_trav.drainage_bias),
    }

    payload = {
        "n": int(n),
        "span_m": float(span_m),
        "height_scale_m": float(height_scale_m),
        "params": params,
        "height": h.ravel().tolist(),
        "routes": [[[int(r), int(c)] for (r, c) in route] for route in routes],
    }

    out_path = Path(__file__).resolve().parent / "fixtures" / "pass_network_routes_fixture.json"
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(payload), encoding="utf-8")

    total_points = sum(len(route) for route in routes)
    print(f"[fixture] wrote {out_path} routes={len(routes)} total_points={total_points}")
    we_ranges = [_free_axis_range(r, 0) for r in routes[:n_we]]
    ns_ranges = [_free_axis_range(r, 1) for r in routes[n_we:]]
    print(
        f"[fixture] slope_max={slope_max:.3f} over_budget={over_frac*100:.1f}% "
        f"(budget={p_trav.slope_budget:.2f}) | weave WE row-ranges={we_ranges} NS col-ranges={ns_ranges}"
    )


if __name__ == "__main__":
    main()
