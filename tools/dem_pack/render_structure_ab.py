"""Render Slice 2A structure-basis A/B sheets for owner review.

This is deliberately offline and image-first. It keeps the old baseline intact, then renders candidate
structure bases with the same seeds/views so the owner can judge whether any variant reads as connected
terrain rather than same-noise roughness.

Run:
    python tools/dem_pack/render_structure_ab.py
    python tools/dem_pack/render_structure_ab.py --size 768 --families mountain badlands
"""
from __future__ import annotations

import argparse
import os
import sys

import numpy as np
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import worldgen_proto as wg  # noqa: E402
from render_worldgen import BADLANDS, MOUNTAIN, PLAINS, hillshade  # noqa: E402

OUT_DEFAULT = r"D:\tmp\wg10_structure_ab"
PARAMS = {
    "mountain": MOUNTAIN,
    "plains": PLAINS,
    "badlands": BADLANDS,
}
VIEWS = (
    ("overview_200km", 200000.0, 0.0, 0.0, 1.0),
    ("close_20km", 20000.0, 120000.0, 80000.0, 1.8),
)
LABELS = {
    "baseline": "baseline",
    "recursive_warp": "recursive warp",
    "multifractal_ridges": "multifractal ridges",
    "ridge_valley_coupled": "ridge+valley coupled",
    "cellular_valleys": "cellular valleys",
}


def grid(n: int, span: float, ox: float = 0.0, oz: float = 0.0):
    ii = np.linspace(0.0, span, n)
    return np.meshgrid(ii + ox, ii + oz)


def to_img(sh):
    return Image.fromarray((np.asarray(sh) * 255).astype(np.uint8), mode="L").convert("RGB")


def draw_label(img: Image.Image, text: str) -> Image.Image:
    pad = 22
    out = Image.new("RGB", (img.width, img.height + pad), (18, 18, 18))
    out.paste(img, (0, pad))
    ImageDraw.Draw(out).text((6, 4), text, fill=(225, 225, 225))
    return out


def save_sheet(path: str, panels: list[Image.Image]) -> None:
    gap = 8
    width = sum(p.width for p in panels) + gap * (len(panels) - 1)
    height = max(p.height for p in panels)
    sheet = Image.new("RGB", (width, height), (10, 10, 10))
    x = 0
    for panel in panels:
        sheet.paste(panel, (x, 0))
        x += panel.width + gap
    sheet.save(path)
    print(f"wrote {path}")


def render_family(out_dir: str, family: str, params: dict, variants: list[str], size: int, seed: int) -> None:
    for view_name, span, ox, oz, exaggeration in VIEWS:
        wx, wz = grid(size, span, ox=ox, oz=oz)
        panels = []
        for variant in variants:
            z = wg.generate_variant(wx, wz, params, seed=seed, variant=variant)
            sh = hillshade(z, exaggeration=exaggeration)
            panels.append(draw_label(to_img(sh), f"{family} {view_name} | {LABELS[variant]}"))
        save_sheet(os.path.join(out_dir, f"structure_ab_{family}_{view_name}.png"), panels)


def render_transition(out_dir: str, variants: list[str], size: int, seed: int) -> None:
    span = 200000.0
    wx, wz = grid(size, span)
    t = np.linspace(0.0, 1.0, size).reshape(1, -1)
    panels = []
    for variant in variants:
        hm = wg.generate_variant(wx, wz, MOUNTAIN, seed=seed, variant=variant)
        hp = wg.generate_variant(wx, wz, PLAINS, seed=seed, variant=variant)
        strip = hm * (1.0 - t) + hp * t
        panels.append(draw_label(to_img(hillshade(strip, exaggeration=1.5)), f"mountain->plains | {LABELS[variant]}"))
    save_sheet(os.path.join(out_dir, "structure_ab_transition_mountain_plains.png"), panels)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=OUT_DEFAULT)
    ap.add_argument("--size", type=int, default=512)
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--families", nargs="*", default=["mountain", "plains", "badlands"])
    ap.add_argument("--variants", nargs="*", default=list(wg.STRUCTURE_VARIANTS))
    args = ap.parse_args()

    unknown_families = [f for f in args.families if f not in PARAMS]
    if unknown_families:
        raise SystemExit(f"unknown families: {unknown_families}; expected {sorted(PARAMS)}")
    unknown_variants = [v for v in args.variants if v not in wg.STRUCTURE_VARIANTS]
    if unknown_variants:
        raise SystemExit(f"unknown variants: {unknown_variants}; expected {list(wg.STRUCTURE_VARIANTS)}")

    os.makedirs(args.out, exist_ok=True)
    for family in args.families:
        render_family(args.out, family, PARAMS[family], args.variants, args.size, args.seed)
    if "mountain" in args.families and "plains" in args.families:
        render_transition(args.out, args.variants, args.size, args.seed)


if __name__ == "__main__":
    main()
