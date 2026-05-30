#!/usr/bin/env python3
"""Slice 2 orchestrator: distill the real WG9 DEMs (by family) into a per-family biome_params table.
WG9 is READ-ONLY. Run from repo root (or tools/dem_pack). Writes tools/dem_pack/biome_params.json.

  python tools/dem_pack/distill_biomes.py                 # all 12 families
  python tools/dem_pack/distill_biomes.py --families mountain grassland badlands   # subset (prove-on-3)
"""
from __future__ import annotations
import argparse
import collections
import json
import os
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import biome_distill as bd  # noqa: E402

WG9_KERNELS = "D:/workflows/worldgen9/factory/kernels"
MAP_PATH = os.path.join(HERE, "kernel_family_map.approved.json")
OUT_PATH = os.path.join(HERE, "biome_params.json")
MAX_ABS_ZSCORE = 12.0   # reuse build_pack.py's spike guard


def load_family_map():
    return dict(json.load(open(MAP_PATH))["map"])


def load_kernel(kid):
    z = np.load(f"{WG9_KERNELS}/{kid}/normalized_height.npy")
    meta = json.load(open(f"{WG9_KERNELS}/{kid}/kernel.json"))
    return z, meta


def distill(families=None):
    """Return {family: biome_params} for the requested families (default all)."""
    fam_of = load_family_map()
    by_fam = collections.defaultdict(list)
    for kid, fam in fam_of.items():
        by_fam[fam].append(kid)
    if families:
        by_fam = {f: by_fam[f] for f in families if f in by_fam}
        for f in families:
            if f not in by_fam:
                raise SystemExit(f"[distill] unknown family {f!r}")
    out = {}
    for fam in sorted(by_fam):
        metrics_list = []
        used = 0
        for kid in sorted(by_fam[fam]):
            z, meta = load_kernel(kid)
            if max(abs(float(z.min())), abs(float(z.max()))) > MAX_ABS_ZSCORE:
                print(f"[distill] {fam}: dropped {kid} (z-score spike)")
                continue
            metrics_list.append(bd.metrics_for_dem(z, meta))
            used += 1
        if not metrics_list:
            raise SystemExit(f"[distill] family {fam!r}: all kernels dropped — nothing to distill")
        agg = bd.aggregate_median(metrics_list)
        out[fam] = bd.params_from_metrics(agg)
        print(f"[distill] {fam}: {used} kernels -> "
              f"relief={out[fam]['relief_m']:.0f} ridge={out[fam]['ridge_strength']:.2f} "
              f"valley={out[fam]['valley_depth']:.2f} warp={out[fam]['warp_amount']:.0f} "
              f"base_wl={1.0/out[fam]['base_freq']:.0f}m")
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--families", nargs="*", default=None)
    args = ap.parse_args()
    params = distill(args.families)
    with open(OUT_PATH, "w") as f:
        json.dump(params, f, indent=1)
        f.write("\n")
    print(f"[distill] wrote {len(params)} families -> {OUT_PATH}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
