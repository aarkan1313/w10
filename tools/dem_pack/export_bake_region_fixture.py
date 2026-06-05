"""Emit the end-to-end Python ORACLE fixture for the seam-safe ``bake_region`` pipeline.

``bake_region`` assembles the three already-ported pieces into one seam-safe
"baked look": SEAM-SAFE mountain macro -> connected pass-network carve ->
``condition_world``. The Rust ``bake_region`` assembly (next task) must reproduce
``height`` (and the intermediate ``raw`` / ``carve_delta``) from these same
constants + params.

CRITICAL — this uses the SEAM-SAFE branch of ``mountain.generate`` (apron_px > 0),
mirroring ``test_mountain_world_layer_contract._live_seamsafe_page`` exactly, NOT
``build_network_world`` (which uses the full-field branch — different by design;
the live runtime uses the seam-safe branch).

CRITICAL — the composition ORDER is load-bearing (mountain_world_layer.build_network_world
lines 485-488): carve runs on the RAW macro field, THEN ``condition_world`` normalizes the
CARVED result:

    carved     = carve_pass_network(raw, ...)
    raw_carved = raw + carved["delta"]
    height, stats = condition_world(raw_carved)

If ``height`` is not ~[-1, 1] the order is wrong (condition must run last).

Run from repo root:
    python tools/dem_pack/export_bake_region_fixture.py
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

import mountain_synthesis as mountain
import mountain_pass_network as mpn
import mountain_world_layer as L
import corridor_router as cr
import geography_skeleton_windows as win

HERE = Path(__file__).resolve().parent
OUT_FIXTURE = HERE / "fixtures" / "bake_region_fixture.json"

# --- constants (per task spec) ---
SAMPLE_N = 193
FEATURE_SPAN_M = 90_000.0
HEIGHT_SCALE_M = 1700.0
SEED = 177
SOURCE_SPAN_M = 270_000.0
SOURCE_ORIGIN_X = 207_000.0
SOURCE_ORIGIN_Z = 176_000.0


def main() -> None:
    # ------------------------------------------------------------------
    # 1) SEAM-SAFE mountain macro (mirror _live_seamsafe_page exactly).
    #    apron-padded grid in -> generate computes on padded -> crops to the
    #    inner SAMPLE_N x SAMPLE_N core before returning.
    # ------------------------------------------------------------------
    apron_px = int(mountain.MOUNTAIN_APRON_PX)
    spacing_m = SOURCE_SPAN_M / float(SAMPLE_N - 1)
    padded_n = SAMPLE_N + 2 * apron_px
    padded_span_m = SOURCE_SPAN_M + 2.0 * float(apron_px) * spacing_m
    wx, wz = mountain.grid(
        padded_n,
        padded_span_m,
        ox=SOURCE_ORIGIN_X - float(apron_px) * spacing_m,
        oz=SOURCE_ORIGIN_Z - float(apron_px) * spacing_m,
    )
    result = mountain.generate(
        wx,
        wz,
        seed=SEED,
        style=mountain.STYLES[0],
        feature_span_m=FEATURE_SPAN_M,
        apron_px=apron_px,
        spacing_m=spacing_m,
        flow_on=True,
    )
    raw = np.asarray(result["height"], dtype=np.float64)  # CORE (apron already cropped)
    n = raw.shape[0]
    assert n == SAMPLE_N, f"expected core {SAMPLE_N}, got {n} (apron crop mismatch)"
    assert raw.shape == (SAMPLE_N, SAMPLE_N), f"raw not square SAMPLE_N: {raw.shape}"

    # ------------------------------------------------------------------
    # 2) Carve a connected pass network on the RAW core (BEFORE conditioning).
    #    carve_pass_network's internal _routes calls cr._core(full, spec); for a
    #    standalone CORE field (no apron) _core must be IDENTITY. Replicate the
    #    export_carve_ramp_fixture.py shim VERBATIM (save/restore in try/finally).
    # ------------------------------------------------------------------
    pp = mpn.PassNetworkParams()  # defaults: n_we=4, n_ns=4, coarse_n=193
    saved_core, saved_slice = cr._core, win._core_slice
    cr._core = lambda full, spec: np.asarray(full)
    win._core_slice = lambda spec: slice(0, n)
    try:
        carved = mpn.carve_pass_network(raw, span_m=SOURCE_SPAN_M, height_scale_m=HEIGHT_SCALE_M, pp=pp)
    finally:
        cr._core, win._core_slice = saved_core, saved_slice

    delta = np.asarray(carved["delta"], dtype=np.float64)
    raw_carved = raw + delta

    # ------------------------------------------------------------------
    # 3) condition_world on the CARVED field (LAST — tanh-bounds to ~[-1, 1]).
    # ------------------------------------------------------------------
    height, stats = L.condition_world(raw_carved)
    height = np.asarray(height, dtype=np.float64)

    # ------------------------------------------------------------------
    # params — capture EXACTLY what carve_pass_network constructs internally so
    # the Rust bake_region can rebuild identical PassNetworkParams + TraverseParams
    # + RampParams and reproduce the same routes + carve.
    #
    # carve_pass_network builds (see mountain_pass_network.carve_pass_network):
    #   p_trav = dataclasses.replace(tc.TraverseParams(),
    #                                scene_width_m=span_m, height_scale_m=height_scale_m)
    #     -> slope_budget / slope_penalty / drainage_bias stay at TraverseParams defaults
    #        (0.28 / 24.0 / 0.55); scene_width_m=SOURCE_SPAN_M; height_scale_m=HEIGHT_SCALE_M.
    #   p_cor = cr.CorridorParams(corridor_density=1,
    #                             slope_budget=p_trav.slope_budget,
    #                             ramp_half_width_m=span_m * pp.ramp_half_frac,
    #                             ramp_flat_half_m=span_m * pp.ramp_flat_frac,
    #                             ramp_carve_max_m=pp.carve_max_m)
    #     -> ramp_half_width_m / ramp_flat_half_m are SPAN-RELATIVE (NOT the 1200/200
    #        CorridorParams defaults); ramp_floor_grade_frac / ramp_wall_grade_frac /
    #        ramp_floor_smooth_px stay at CorridorParams defaults (0.35 / 0.80 / 5.0).
    # ------------------------------------------------------------------
    import dataclasses
    import traverse_corridor as tc
    p_trav = dataclasses.replace(tc.TraverseParams(), scene_width_m=SOURCE_SPAN_M, height_scale_m=HEIGHT_SCALE_M)
    p_cor = cr.CorridorParams(
        corridor_density=1,
        slope_budget=float(p_trav.slope_budget),
        ramp_half_width_m=SOURCE_SPAN_M * float(pp.ramp_half_frac),
        ramp_flat_half_m=SOURCE_SPAN_M * float(pp.ramp_flat_frac),
        ramp_carve_max_m=float(pp.carve_max_m),
    )

    params = {
        # PassNetworkParams (routing fan-out + coarse grid)
        "n_we": int(pp.n_we),
        "n_ns": int(pp.n_ns),
        "coarse_n": int(pp.coarse_n),
        # TraverseParams the carve routes with (Dijkstra cost model)
        "slope_budget": float(p_trav.slope_budget),
        "slope_penalty": float(p_trav.slope_penalty),
        "drainage_bias": float(p_trav.drainage_bias),
        "scene_width_m": float(p_trav.scene_width_m),
        "traverse_height_scale_m": float(p_trav.height_scale_m),
        # CorridorParams (carve_ramp) — the REAL constructed values, NOT defaults
        "corridor_density": int(p_cor.corridor_density),
        "ramp_floor_grade_frac": float(p_cor.ramp_floor_grade_frac),
        "ramp_wall_grade_frac": float(p_cor.ramp_wall_grade_frac),
        "ramp_flat_half_m": float(p_cor.ramp_flat_half_m),
        "ramp_half_width_m": float(p_cor.ramp_half_width_m),
        "ramp_floor_smooth_px": float(p_cor.ramp_floor_smooth_px),
        "ramp_carve_max_m": float(p_cor.ramp_carve_max_m),
    }

    out = {
        "n": int(n),
        "span_m": float(SOURCE_SPAN_M),
        "height_scale_m": float(HEIGHT_SCALE_M),
        "seed": int(SEED),
        "feature_span_m": float(FEATURE_SPAN_M),
        "apron_px": int(apron_px),
        "spacing_m": float(spacing_m),
        "source_origin_x_m": float(SOURCE_ORIGIN_X),
        "source_origin_z_m": float(SOURCE_ORIGIN_Z),
        "params": params,
        "raw": raw.ravel().tolist(),
        "carve_delta": delta.ravel().tolist(),
        "height": height.ravel().tolist(),
        "stats": {k: float(v) for k, v in stats.items()},
    }
    OUT_FIXTURE.write_text(json.dumps(out))

    carved_cells = int(np.count_nonzero(delta < -1e-9))
    hmin = float(np.min(height))
    hmax = float(np.max(height))
    p50 = float(stats["p50"])
    print(
        f"[bake-fixture] wrote {OUT_FIXTURE} n={n} "
        f"carved_cells={carved_cells} "
        f"height_range=[{hmin:.6f},{hmax:.6f}] "
        f"p50={p50:.6f}"
    )


if __name__ == "__main__":
    main()
