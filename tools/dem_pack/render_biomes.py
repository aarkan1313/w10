"""Render real-vs-synth side-by-side hillshades for the owner's eye (render-first, Slice 2).
Left = the family's real DEM (a representative kernel), right = synth from its distilled params,
at MATCHED metres/pixel. Captioned with the distilled metrics. Writes to D:\\tmp\\.
  python render_biomes.py --families mountain grassland badlands
  python render_biomes.py                 # all families in biome_params.json
NOT a test — a runnable inspection tool. Character match (same KIND of terrain), not pixel copy."""
from __future__ import annotations
import argparse
import json
import os
import sys

import numpy as np
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import worldgen_proto as wg  # noqa: E402

WG9_KERNELS = "D:/workflows/worldgen9/factory/kernels"
MAP_PATH = os.path.join(HERE, "kernel_family_map.approved.json")
PARAMS_PATH = os.path.join(HERE, "biome_params.json")
OUT = r"D:\tmp"
TILE = 512  # render size per panel


def hillshade(z, az=315.0, alt=45.0):
    zn = (z - z.min()) / (np.ptp(z) + 1e-9)
    gy, gx = np.gradient(zn * 80.0)
    slope = np.pi / 2.0 - np.arctan(np.sqrt(gx * gx + gy * gy))
    aspect = np.arctan2(-gx, gy)
    azr = np.radians(360 - az + 90); altr = np.radians(alt)
    sh = np.sin(altr) * np.sin(slope) + np.cos(altr) * np.cos(slope) * np.cos(azr - aspect)
    return np.clip(sh, 0, 1)


def representative_kernel(fam):
    fam_of = dict(json.load(open(MAP_PATH))["map"])
    ids = sorted([k for k, f in fam_of.items() if f == fam])
    if not ids:
        raise SystemExit(f"[render] no kernels for family {fam!r}")
    return ids[0]  # deterministic; the family's first id by sort


def real_panel(fam):
    kid = representative_kernel(fam)
    z = np.load(f"{WG9_KERNELS}/{kid}/normalized_height.npy")
    meta = json.load(open(f"{WG9_KERNELS}/{kid}/kernel.json"))
    return hillshade(z.astype(np.float64)), meta["approx_sample_spacing_m"], kid


def synth_panel(params, spacing_m):
    # match the real tile's metres/pixel so the comparison is at the same scale
    span = TILE * float(spacing_m)
    ii = np.linspace(0, span, TILE)
    wx, wz = np.meshgrid(ii + 123456.0, ii + 654321.0)  # arbitrary world offset (not origin)
    return hillshade(wg.generate(wx, wz, params, seed=7))


def to_img(sh):
    return Image.fromarray((np.asarray(sh) * 255).astype(np.uint8), mode="L").resize((TILE, TILE)).convert("RGB")


def render_family(fam, params):
    real_sh, spacing, kid = real_panel(fam)
    synth_sh = synth_panel(params, spacing)
    pad = 24
    canvas = Image.new("RGB", (TILE * 2 + pad * 3, TILE + pad * 3), (20, 20, 20))
    canvas.paste(to_img(real_sh), (pad, pad))
    canvas.paste(to_img(synth_sh), (pad * 2 + TILE, pad))
    d = ImageDraw.Draw(canvas)
    d.text((pad, 4), f"{fam}  REAL: {kid[:48]} ({spacing:.0f} m/px)", fill=(220, 220, 220))
    d.text((pad * 2 + TILE, 4), f"{fam}  SYNTH (distilled params)", fill=(220, 220, 220))
    cap = (f"relief={params['relief_m']:.0f}m ridge={params['ridge_strength']:.2f} "
           f"valley={params['valley_depth']:.2f} warp={params['warp_amount']:.0f}m "
           f"base_wl={1.0/params['base_freq']:.0f}m slope={params['slope_bias']:.1f}deg")
    d.text((pad, TILE + pad + 6), cap, fill=(180, 200, 180))
    path = rf"{OUT}\biome_{fam}.png"
    canvas.save(path)
    print(f"wrote {path}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--families", nargs="*", default=None)
    args = ap.parse_args()
    params_all = json.load(open(PARAMS_PATH))
    fams = args.families or sorted(params_all)
    for fam in fams:
        if fam not in params_all:
            raise SystemExit(f"[render] {fam!r} not in biome_params.json — run distill_biomes.py first")
        render_family(fam, params_all[fam])


if __name__ == "__main__":
    main()
