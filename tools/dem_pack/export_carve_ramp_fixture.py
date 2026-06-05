"""Emit a parity ORACLE fixture for the carve_ramp Rust port.

Reuses the proven routing fixture (pass_network_routes_fixture.json: a single
continuous field + the connected pass-network routes that weave through over-budget
walls) and records the EXACT Python `corridor_router.carve_ramp` height delta. The
Rust port (later tasks) must reproduce `delta` from `height`+`routes` within a metres
tolerance.

Run from repo root:
    python tools/dem_pack/export_carve_ramp_fixture.py
"""
from __future__ import annotations

import json
import sys
import types
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

import corridor_router as cr
import geography_skeleton_windows as win

HERE = Path(__file__).resolve().parent
ROUTES_FIXTURE = HERE / "fixtures" / "pass_network_routes_fixture.json"
OUT_FIXTURE = HERE / "fixtures" / "carve_ramp_fixture.json"


def main() -> None:
    src = json.loads(ROUTES_FIXTURE.read_text())
    n = int(src["n"])
    span_m = float(src["span_m"])
    height_scale_m = float(src["height_scale_m"])
    height = np.asarray(src["height"], dtype=np.float64).reshape(n, n)
    # routes: list of routes, each a list of [row, col] pairs -> list of (row, col) tuples
    routes = [[(int(r), int(c)) for r, c in rt] for rt in src["routes"]]

    cell_m = span_m / (n - 1)

    spec = types.SimpleNamespace(spacing_m=cell_m, apron_m=0.0, core_span_m=span_m)
    p = cr.CorridorParams()
    corridor = {"routes": [{"path": rt} for rt in routes]}

    # carve_ramp operates on cr._core(full, spec). For a single continuous field (no
    # apron) _core must be IDENTITY. Replicate the mountain_pass_network shim exactly
    # (save/restore in try/finally).
    saved_core, saved_slice = cr._core, win._core_slice
    cr._core = lambda full, spec: np.asarray(full)
    win._core_slice = lambda spec: slice(0, n)
    try:
        delta = cr.carve_ramp(height, corridor, spec, p, height_scale_m=height_scale_m)
    finally:
        cr._core, win._core_slice = saved_core, saved_slice

    delta = np.asarray(delta, dtype=np.float64)

    params = {
        "slope_budget": float(p.slope_budget),
        "ramp_floor_grade_frac": float(p.ramp_floor_grade_frac),
        "ramp_wall_grade_frac": float(p.ramp_wall_grade_frac),
        "ramp_flat_half_m": float(p.ramp_flat_half_m),
        "ramp_half_width_m": float(p.ramp_half_width_m),
        "ramp_floor_smooth_px": float(p.ramp_floor_smooth_px),
        "ramp_carve_max_m": float(p.ramp_carve_max_m),
    }

    out = {
        "n": n,
        "span_m": span_m,
        "height_scale_m": height_scale_m,
        "params": params,
        "height": height.ravel().tolist(),
        "routes": [[[int(r), int(c)] for r, c in rt] for rt in routes],
        "delta": delta.ravel().tolist(),
    }
    OUT_FIXTURE.write_text(json.dumps(out))

    carved_cells = int(np.count_nonzero(delta < -1e-9))
    min_delta = float(delta.min())
    min_delta_m = min_delta * height_scale_m
    print(
        f"[ramp-fixture] wrote {OUT_FIXTURE} carved_cells={carved_cells} "
        f"min_delta={min_delta} min_delta_m={min_delta_m}"
    )


if __name__ == "__main__":
    main()
