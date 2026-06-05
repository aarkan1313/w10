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


def build_height(n: int) -> np.ndarray:
    """Deterministic synthetic height: a ridged sum-of-sines with connected low ground to route along.

    No RNG -- pure analytic f(u, v) so re-runs are bit-identical. Centered/normalized to ~unit std like
    the tanh-conditioned field the real pipeline routes over (height ~0 == low ground, which the
    drainage_bias reward in _step_cost pulls routes toward)."""
    ys, xs = np.mgrid[0:n, 0:n].astype(float)
    u = xs / (n - 1)
    v = ys / (n - 1)
    h = (
        np.sin(u * 9.0) * np.cos(v * 7.0)
        + 0.5 * np.sin(u * 17.0 + 1.3) * np.cos(v * 13.0 - 0.7)
        + 0.25 * np.sin(u * 31.0) * np.cos(v * 29.0)
    )
    h = (h - h.mean()) / (h.std() + 1e-9)   # ~tanh-centered like the conditioned field
    return h


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

    routes = mpn._routes(h, span_m, height_scale_m, p_trav, pp)

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


if __name__ == "__main__":
    main()
