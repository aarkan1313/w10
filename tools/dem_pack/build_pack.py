#!/usr/bin/env python3
"""Phase B — build the real DEM terrain pack from the approved family map.
Emits wg-10/worldgen_terrain/packs/dem_v1/{terrain_pack.json, kernels/*.npy}.
WG9 is read-only. Run from repo root AFTER approving the family map.

  python tools/dem_pack/build_pack.py --validate
  python tools/dem_pack/build_pack.py --gate-subset 24 --validate
"""
from __future__ import annotations
import argparse
import json
import os
import shutil
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import dem_pack_lib as lib  # noqa: E402

REPO = os.path.dirname(os.path.dirname(HERE))  # tools/dem_pack -> repo root
WG9_KERNELS = "D:/workflows/worldgen9/factory/kernels"
MAP_PATH = os.path.join(HERE, "kernel_family_map.approved.json")
OUT_DIR = os.path.join(REPO, "wg-10", "worldgen_terrain", "packs", "dem_v1")


def load_meta(kernel_id):
    with open(f"{WG9_KERNELS}/{kernel_id}/kernel.json") as f:
        return json.load(f)


def validate_npy(path):
    """Confirm a .npy parses as a 512x512 (or NxN) C-order float32 array."""
    a = np.load(path, mmap_mode="r")
    if a.dtype != np.dtype("<f4") and a.dtype != np.float32:
        raise ValueError(f"{path}: dtype {a.dtype} not float32")
    if a.ndim != 2:
        raise ValueError(f"{path}: ndim {a.ndim} not 2")
    return a.shape


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--validate", action="store_true")
    ap.add_argument("--gate-subset", type=int, default=0,
                    help="if >0, build a small subset pack (terrain_pack.gate.json) "
                         "with N kernels spread across families, for the gates")
    ap.add_argument("--footprint-scale", type=float, default=1.0)
    args = ap.parse_args()

    approved = json.load(open(MAP_PATH))
    fam_of_full = dict(approved["map"])
    if not fam_of_full:
        raise SystemExit("[build] approved map is empty — review/approve it first (Phase A)")

    # subset: take up to ceil(N/num_families) per family, deterministic by sorted id
    if args.gate_subset > 0:
        by_fam = {}
        for kid, fam in sorted(fam_of_full.items()):
            by_fam.setdefault(fam, []).append(kid)
        per = max(1, -(-args.gate_subset // len(by_fam)))  # ceil
        fam_of = {}
        for fam in sorted(by_fam):
            for kid in by_fam[fam][:per]:
                fam_of[kid] = fam
        out_json = "terrain_pack.gate.json"
    else:
        fam_of = fam_of_full
        out_json = "terrain_pack.json"

    meta = {kid: load_meta(kid) for kid in fam_of}
    pack = lib.build_pack_dict(fam_of, meta, footprint_scale=args.footprint_scale)

    os.makedirs(os.path.join(OUT_DIR, "kernels"), exist_ok=True)
    # copy kernels
    copied = 0
    for kid in fam_of:
        src = f"{WG9_KERNELS}/{kid}/normalized_height.npy"
        dst = os.path.join(OUT_DIR, "kernels", f"{kid}.npy")
        shutil.copyfile(src, dst)
        copied += 1
    with open(os.path.join(OUT_DIR, out_json), "w") as f:
        json.dump(pack, f, indent=1)
        f.write("\n")

    if args.validate:
        # every family's .npy exists + parses; every palette family resolves
        fam_ids = set(pack["families"])
        for kid, fam in pack["families"].items():
            shape = validate_npy(os.path.join(OUT_DIR, fam["kernel"].replace("/", os.sep)))
            if shape[0] != shape[1]:
                raise ValueError(f"{kid}: non-square kernel {shape}")
        for p in pack["palettes"]:
            if len(p["families"]) != lib.FAMILIES_PER_PALETTE:
                raise ValueError(f"palette {p['id']}: not {lib.FAMILIES_PER_PALETTE} families")
            for fid in p["families"]:
                if fid not in fam_ids:
                    raise ValueError(f"palette {p['id']}: family {fid} not in families")
        print(f"[build] validate OK: {len(fam_ids)} families, {len(pack['palettes'])} palettes")

    print(f"[build] wrote {out_json} ({len(fam_of)} kernels copied) -> {OUT_DIR}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
