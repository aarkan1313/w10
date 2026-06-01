r"""Export a MOUNTAIN corridor on/off review for the Godot fly-through (Tier-3 carve_ramp on real mountains).

Two switchable items at matched coords + the SAME conditioning transform, so flipping shows the guaranteed
valley carved through the mountain wall:
  1 = "mountain (no corridor)"  -> mountain_synthesis height, the wall present
  2 = "mountain + corridor"     -> mountain + carve_ramp delta, a walkable valley cut through the wall

The mountain is a genuine slope-wall barrier (~70% impassably steep). carve_ramp opens a passable crossing.
NOTE: single-window review (seam-exactness of the wide ramp is a separate WIP); this judges the LOOK of the
carved pass on real mountain terrain.

Run:
    python tools/dem_pack/export_godot_mountain_corridor_review.py
Writes:
    wg-10/worldgen_terrain/generated/review/mountain_corridor.json
"""

from __future__ import annotations

import dataclasses
import json
import types
from pathlib import Path

import numpy as np
from scipy.ndimage import gaussian_filter, zoom

import mountain_synthesis as ms
import traverse_corridor as tc
import corridor_router as cr


OUT = Path("wg-10/worldgen_terrain/generated/review/mountain_corridor.json")
N_GEN = 129          # generation grid
N_DISPLAY = 225      # display resolution (matches the other review payloads)
DISPLAY_SPAN_M = 25600.0
FEATURE_SPAN_M = 90000.0
HEIGHT_SCALE_M = 1700.0
SEED = 3


def _condition(z, p05=None, p50=None, p95=None):
    """Compress relief for review; reuse passed percentiles (shared transform) so the carve stays visible."""
    z = np.asarray(z, dtype=np.float64)
    if p05 is None:
        p05, p50, p95 = (float(np.percentile(z, q)) for q in (5.0, 50.0, 95.0))
    robust = (z - p50) / (p95 - p05 + 1e-9) * 2.15
    shaped = np.tanh(gaussian_filter(robust, sigma=0.65))
    return shaped, (p05, p50, p95)


def _resample(z):
    s = N_DISPLAY / z.shape[0]
    return zoom(z, (s, s), order=1)[:N_DISPLAY, :N_DISPLAY]


def _item(key, label, conditioned):
    return {
        "key": key,
        "label": label,
        "kind": "synth",
        "span_km": round(DISPLAY_SPAN_M / 1000.0, 1),
        "n": int(N_DISPLAY),
        "height": np.round(conditioned, 4).astype(float).ravel().tolist(),
        "stats": {},
    }


def build_payload() -> dict:
    cell_m = DISPLAY_SPAN_M / (N_GEN - 1)
    spec = types.SimpleNamespace(spacing_m=cell_m, apron_m=0.0, core_span_m=DISPLAY_SPAN_M)
    p = dataclasses.replace(tc.TraverseParams(), scene_width_m=DISPLAY_SPAN_M, height_scale_m=HEIGHT_SCALE_M)
    p_cor = cr.CorridorParams(corridor_density=1, slope_budget=float(p.slope_budget))

    wx, wz = ms.grid(N_GEN, DISPLAY_SPAN_M, ox=60000.0, oz=36000.0)
    h = np.asarray(ms.generate(wx, wz, seed=SEED, style=ms.STYLES[0], feature_span_m=FEATURE_SPAN_M)["height"],
                   dtype=np.float64)

    # no-apron single-window shim so the corridor pieces run on the mountain grid
    import geography_skeleton_windows as win
    saved_core, saved_slice = cr._core, win._core_slice
    cr._core = lambda full, spec: np.asarray(full)
    win._core_slice = lambda spec: slice(0, N_GEN)
    try:
        corridor = cr.build_corridor(h, spec, p, p_cor)
        carve = cr.carve_ramp(h, corridor, spec, p_cor, height_scale_m=HEIGHT_SCALE_M)
        resolved = not tc.needs_route_core(h + carve, spec, p)["needs_route"]
    finally:
        cr._core, win._core_slice = saved_core, saved_slice

    carved = h + carve
    base_cond, pct = _condition(_resample(h))
    carved_cond, _ = _condition(_resample(carved), *pct)   # shared transform -> the valley shows
    frac = float(np.mean(carve != 0.0))

    return {
        "title": "WG10 Tier-3 MOUNTAIN corridor on/off (seed 3, 90km feature / 25.6km scene)",
        "review_intent": "owner_fly_does_the_carved_mountain_pass_read_right",
        "span_km": round(DISPLAY_SPAN_M / 1000.0, 1),
        "items": [
            _item("mtn_no_corridor", "1: mountain (no corridor) - the wall", base_cond),
            _item("mtn_with_corridor", f"2: mountain + corridor - resolved={resolved} carved={frac*100:.0f}%area",
                  carved_cond),
        ],
    }


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    payload = build_payload()
    OUT.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    print(f"wrote {OUT}")
    for it in payload["items"]:
        hh = np.asarray(it["height"], dtype=np.float64)
        print(f"  {it['key']}: n={it['n']} std={hh.std():.3f} ptp={np.ptp(hh):.3f}")


if __name__ == "__main__":
    main()
