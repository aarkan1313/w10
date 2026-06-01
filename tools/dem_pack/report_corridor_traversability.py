"""Connected-corridor guarantee/seam gate. Runs the router over the measured barrier fixtures + a gentle
no-op, asserting the guarantee resolves real barriers seam-exactly (spec section 8 did-real-work). The
low-corridor barrier is the realistic play-scale case (must resolve); the wall-sever 4km is an extreme stress
fixture (detected + carved + seam-exact at density 1; full resolution at default width is a known limit).

Run: python report_corridor_traversability.py
"""
from __future__ import annotations
import dataclasses
import numpy as np

import export_godot_rough_world_chunks as ex
import geography_skeleton_windows as win
import keeper_v2 as v2
import traverse_corridor as tc


def _scn(label, seed, span, kp, expect_resolve):
    spec = ex._window_spec(129, span)
    p = dataclasses.replace(tc.TraverseParams(), scene_width_m=span)
    return {"label": label, "seed": seed, "spec": spec, "p": p, "kp": kp, "expect_resolve": expect_resolve}


def scenarios():
    spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    wall = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=3.5, relief_amplitude=3.2)
    return [
        _scn("low_corridor_25k", 1, ex.CHUNK_SPAN_M, spiky, True),    # realistic play-scale -> must resolve
        _scn("wall_sever_4k", 42, 4000.0, wall, False),              # extreme stress -> carved + seam-exact
        _scn("gentle_noop_25k", 133, ex.CHUNK_SPAN_M, v2.KeeperV2Params(), True),  # crossable -> no-op
    ]


def run_summary():
    barriers = resolved_ok = seam_fail = noop_carved = 0
    seam_max = 0.0
    rows = []
    for s in scenarios():
        spec, p, kp = s["spec"], s["p"], s["kp"]
        span = float(spec.core_span_m)
        ox, oz = ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M
        wa = win.build_skeleton_window(ox, oz, s["seed"], spec)
        wb = win.build_skeleton_window(ox + span, oz, s["seed"], spec)
        ra = tc.build_traverse_corridor(wa, s["seed"], spec, p, kp)
        rb = tc.build_traverse_corridor(wb, s["seed"], spec, p, kp)
        keeper_core = v2.compose_windowed_height_v2(wa, s["seed"], spec, kp)
        resolved = not tc.needs_route_core(keeper_core + ra["carve_delta"], spec, p)["needs_route"]
        seam = float(np.max(np.abs(ra["carve_delta"][:, -1] - rb["carve_delta"][:, 0])))
        seam_max = max(seam_max, seam)
        if seam != 0.0:
            seam_fail += 1
        if ra["needs_route"]:
            barriers += 1
            if s["expect_resolve"] and resolved:
                resolved_ok += 1
        else:
            if np.count_nonzero(ra["carve_delta"]) != 0:
                noop_carved += 1
        rows.append((s["label"], ra["needs_route"], ra.get("carved"), resolved, seam))
    return {
        "barriers_exercised": barriers,
        "play_scale_resolved": resolved_ok,
        "seam_failures": seam_fail,
        "seam_max_delta": seam_max,
        "noop_carved": noop_carved,
        "rows": rows,
    }


def main():
    s = run_summary()
    for label, needs, carved, resolved, seam in s["rows"]:
        print(f"  {label:18s} needs_route={needs} carved={carved} resolved={resolved} seam={seam:.6g}")
    # pass = at least 1 real barrier; the play-scale barrier resolved; no seam breaks; no-op didn't carve.
    ok = (s["barriers_exercised"] >= 1 and s["play_scale_resolved"] >= 1
          and s["seam_failures"] == 0 and s["noop_carved"] == 0)
    print(f"barriers={s['barriers_exercised']} play_scale_resolved={s['play_scale_resolved']} "
          f"seam_failures={s['seam_failures']} seam_max={s['seam_max_delta']:.6g} noop_carved={s['noop_carved']}")
    print(f"[wg10-corridor] status={'pass' if ok else 'FAIL'}")


if __name__ == "__main__":
    main()
