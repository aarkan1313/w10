r"""Export A | B | v2 as three switchable items in ONE Godot review payload.

Lets the owner flip between the three rough-highlands height formulas in place at the
same camera/scale, instead of choosing one off a contact sheet:
  A  = geography_skeleton.compose_height (the owner-approved 90 km look)
  B  = export_godot_rough_world_chunks._compose_windowed_height (frozen keeper_v1)
  v2 = keeper_v2.compose_windowed_height_v2 (A's regimes on B's seam-safe substrate)

All three are sampled on the SAME 25.6 km window core (matched world coords) and conditioned
identically, so switching is a true apples-to-apples in-place swap. This is an offline static
review payload, NOT runtime streaming (A is not seam-safe as independent windows anyway).

Run:
    python tools/dem_pack/export_godot_rough_world_abv.py
Writes:
    wg-10/worldgen_terrain/generated/review/rough_world_abv.json
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np

import geography_skeleton as skel
import geography_skeleton_windows as win
import export_godot_rough_world_chunks as ex
import keeper_v2 as v2
from export_godot_rough_world_review import _condition, N, _resample
from render_geography_skeleton_focus import FOCUS


OUT = Path("wg-10/worldgen_terrain/generated/review/rough_world_abv.json")
SEED = 133


def _matched_core() -> tuple[np.ndarray, np.ndarray, np.ndarray, float]:
    """A, B, v2 on the SAME 25.6 km window core, matched world coords."""
    sc = next(s for s in FOCUS if s.key == "rough_anchor")
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    w = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, SEED, spec)
    cs = win._core_slice(spec)
    s0, s1 = cs.start, cs.stop
    b_full, _ = ex._compose_windowed_height(w, SEED, spec)
    B = np.asarray(b_full[s0:s1, s0:s1], dtype=np.float64)
    wx = np.asarray(w["wx"])[s0:s1, s0:s1]
    wz = np.asarray(w["wz"])[s0:s1, s0:s1]
    A = np.asarray(skel.compose_height(wx, wz, seed=SEED, scenario=sc)["height"], dtype=np.float64)
    V = v2.compose_windowed_height_v2(w, SEED, spec, v2.KeeperV2Params())
    return A, B, V, float(ex.CHUNK_SPAN_M)


def _item(key: str, label: str, height: np.ndarray, span_m: float) -> dict:
    conditioned, stats = _condition(_resample(height))
    return {
        "key": key,
        "label": label,
        "kind": "synth",
        "span_km": round(float(span_m) / 1000.0, 1),
        "n": int(N),
        "height": np.round(conditioned, 4).astype(float).ravel().tolist(),
        "stats": stats,
    }


def build_payload() -> dict:
    A, B, V, span_m = _matched_core()
    return {
        "title": "WorldGen10 rough-highlands A | B | v2 switcher",
        "review_intent": "owner_eye_pick_between_three_height_formulas_matched_coords",
        "span_km": round(span_m / 1000.0, 1),
        "items": [
            _item("A_approved", "A approved (compose_height)", A, span_m),
            _item("B_keeper_v1", "B keeper_v1 (windowed, seam-safe)", B, span_m),
            _item("v2_best_of_both", "v2 best-of-both (A regimes on B substrate)", V, span_m),
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
