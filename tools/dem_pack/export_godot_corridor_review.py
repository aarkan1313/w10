r"""Export a corridor on/off review payload for the Godot fly-through (Tier-3 connected-corridor).

Two switchable items at MATCHED world coords + the SAME conditioning transform, so flipping shows the
guaranteed-corridor carve as a real depression (not normalized away):
  1 = "v2 (no corridor)"   -> keeper_v2 height, the barrier present
  2 = "v2 + corridor"      -> keeper_v2 + Tier-3 carve_delta, the guaranteed route carved in

Uses the low-corridor BARRIER config (post_tanh_gain 2.4 / relief_amplitude 3.2) so there is a real barrier
to resolve and a visible carve. Conditioning: the carved item is conditioned with the NO-corridor item's
percentiles (shared transform) so the carve is visible, not re-normalized away.

Run:
    python tools/dem_pack/export_godot_corridor_review.py
Writes:
    wg-10/worldgen_terrain/generated/review/rough_world_corridor.json
"""

from __future__ import annotations

import dataclasses
import json
from pathlib import Path

import numpy as np

import export_godot_rough_world_chunks as ex
import geography_skeleton_windows as win
import keeper_v2 as v2
import traverse_corridor as tc
from export_godot_rough_world_review import _condition, _resample, N


OUT = Path("wg-10/worldgen_terrain/generated/review/rough_world_corridor.json")
SEED = 1
SPAN_M = ex.CHUNK_SPAN_M


def _condition_with(z: np.ndarray, ref_stats: dict) -> np.ndarray:
    """Apply the SAME percentile transform as _condition, but using ref_stats (the no-corridor item's
    percentiles) instead of z's own -> the carve shows as a real depression, shared scale with the reference."""
    from scipy.ndimage import gaussian_filter
    p05, p50, p95 = ref_stats["p05"], ref_stats["p50"], ref_stats["p95"]
    robust = (np.asarray(z, dtype=np.float64) - p50) / (p95 - p05 + 1e-9) * 2.15
    return np.tanh(gaussian_filter(robust, sigma=0.65))


def _item(key, label, conditioned, stats):
    return {
        "key": key,
        "label": label,
        "kind": "synth",
        "span_km": round(float(SPAN_M) / 1000.0, 1),
        "n": int(N),
        "height": np.round(conditioned, 4).astype(float).ravel().tolist(),
        "stats": stats,
    }


def build_payload() -> dict:
    spec = ex._window_spec(129, SPAN_M)
    p = dataclasses.replace(tc.TraverseParams(), scene_width_m=SPAN_M)
    kp = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    w = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, SEED, spec)

    keeper = v2.compose_windowed_height_v2(w, SEED, spec, kp)
    res = tc.build_traverse_corridor(w, SEED, spec, p, kp)
    carved = keeper + res["carve_delta"]

    # condition the NO-corridor item normally; reuse its percentiles for the carved item (shared transform)
    keeper_cond, ref_stats = _condition(_resample(keeper))
    carved_cond = _condition_with(_resample(carved), ref_stats)

    return {
        "title": "WG10 Tier-3 corridor on/off (seed 1, 25.6 km, barrier config)",
        "review_intent": "owner_fly_corridor_carve_makes_a_crossable_route",
        "span_km": round(SPAN_M / 1000.0, 1),
        "items": [
            _item("v2_no_corridor", "1: v2 (no corridor) — barrier", keeper_cond, ref_stats),
            _item("v2_with_corridor",
                  f"2: v2 + corridor — resolved={res.get('resolved')} carved={int(np.count_nonzero(res['carve_delta']))}cells",
                  carved_cond, ref_stats),
        ],
    }


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    payload = build_payload()
    OUT.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    print(f"wrote {OUT}")
    for it in payload["items"]:
        h = np.asarray(it["height"], dtype=np.float64)
        print(f"  {it['key']}: n={it['n']} std={h.std():.3f} ptp={np.ptp(h):.3f}")


if __name__ == "__main__":
    main()
