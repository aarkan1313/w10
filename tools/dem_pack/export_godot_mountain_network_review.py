r"""Export the whole 9x9 mountain RANGE with the connected pass NETWORK on/off, for the Godot fly-through.

Two items at matched coords + shared conditioning:
  1 = "range (no passes)"   -> the 96%-impassable mountain range
  2 = "range + pass network" -> connected valley passes carved through the whole range

The whole 270 km field is one item (fly the entire range, flip the network on/off). Carved on the single big
field then conditioned -> the passes are seam-exact across the implicit chunks by construction. ~70% of the
carved valleys are walkable (honest: occasional steep scrambles, like real passes); the network reads as
natural connected valleys (mountain_pass_network).

Run:
    python tools/dem_pack/export_godot_mountain_network_review.py
Writes:
    wg-10/worldgen_terrain/generated/review/mountain_network.json
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
from scipy.ndimage import gaussian_filter, zoom

import mountain_synthesis as ms
import mountain_pass_network as mpn


OUT = Path("wg-10/worldgen_terrain/generated/review/mountain_network.json")
CHUNK = 9
SRC_SPAN_M = 30000.0
FEATURE_SPAN_M = 90000.0
HEIGHT_SCALE_M = 1700.0
SEED = 3
STEP = 128
N_DISPLAY = 513      # display resolution for the whole 270 km range


def _condition(z, pct=None):
    z = np.asarray(z, dtype=np.float64)
    if pct is None:
        pct = tuple(float(np.percentile(z, q)) for q in (5.0, 50.0, 95.0))
    p05, p50, p95 = pct
    robust = (z - p50) / (p95 - p05 + 1e-9) * 2.15
    return np.tanh(gaussian_filter(robust, sigma=0.65)), pct


def _resample(z):
    s = N_DISPLAY / z.shape[0]
    return zoom(z, (s, s), order=1)[:N_DISPLAY, :N_DISPLAY]


def _item(key, label, conditioned):
    return {
        "key": key, "label": label, "kind": "synth",
        "span_km": round((25600.0 / 3.0) * CHUNK / 1000.0, 1),
        "n": int(N_DISPLAY),
        "height": np.round(conditioned, 4).astype(float).ravel().tolist(),
        "stats": {},
    }


def build_payload() -> dict:
    src_world = CHUNK * SRC_SPAN_M
    src_cell = SRC_SPAN_M / STEP
    padded_span = src_world + 2.0 * src_cell
    padded_n = CHUNK * STEP + 1 + 2
    wx, wz = ms.grid(padded_n, padded_span, ox=60000.0 - src_cell, oz=36000.0 - src_cell)
    h = np.asarray(ms.generate(wx, wz, seed=SEED, style=ms.STYLES[0], feature_span_m=FEATURE_SPAN_M)["height"],
                   dtype=np.float64)
    display_total = (25600.0 / 3.0) * CHUNK

    res = mpn.carve_pass_network(h, span_m=display_total, height_scale_m=HEIGHT_SCALE_M,
                                 pp=mpn.PassNetworkParams(n_we=6, n_ns=6, coarse_n=257))
    base_cond, pct = _condition(_resample(h))
    net_cond, _ = _condition(_resample(res["final"]), pct)   # shared transform -> passes show

    return {
        "title": "WG10 MOUNTAIN range + pass network (1 = range, 2 = + passes)",
        "review_intent": "owner_fly_does_the_pass_network_make_the_range_traversable_and_read_natural",
        "span_km": round(display_total / 1000.0, 1),
        "items": [
            _item("range_no_passes", "1: mountain range (no passes)", base_cond),
            _item("range_with_network",
                  f"2: + pass network - {len(res['routes'])} routes, {res['band_passable_frac']*100:.0f}% walkable, "
                  f"{res['carved_frac']*100:.0f}% area", net_cond),
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
