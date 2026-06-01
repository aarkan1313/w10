"""Corridors-on review sheet for owner acceptance (connected-corridor spec section 8.1). NOT a gate.

Two legible views per barrier fixture, using the same oblique idiom the owner already reviews keepers with:
  - OBLIQUE pre vs post: see the landscape and the pass actually open up.
  - TOP-DOWN passable+route: green = walkable (slope <= budget), dark = too steep, cyan line = the guaranteed
    route. This directly answers "can a player get across?" (the thing the false-color sheet hid).

Run: python render_corridor_sheet.py  ->  D:/tmp/wg10_geography_engine/corridor_sheet.png
"""
from __future__ import annotations
import dataclasses
from pathlib import Path
import numpy as np
from PIL import Image, ImageDraw

import export_godot_rough_world_chunks as ex
import geography_skeleton_windows as win
import keeper_v2 as v2
import traverse_corridor as tc
import analyze_rough_world_traversability as trav
from render_geography_skeleton_focus import oblique_panel, labeled

OUT = Path("D:/tmp/wg10_geography_engine/corridor_sheet.png")


def _passable_route_panel(height, route_dist, p, label, px=430):
    """Top-down: green=walkable, dark=too steep (the WALL), cyan=guaranteed route. Reads at a glance."""
    n = height.shape[0]
    slopes = trav.slope_grid(height, scene_width_m=float(p.scene_width_m), height_scale_m=float(p.height_scale_m))
    passable = slopes <= float(p.slope_budget)
    img = np.zeros((n, n, 3), dtype=np.uint8)
    img[passable] = (70, 150, 70)        # walkable = green
    img[~passable] = (60, 45, 40)        # too steep / barrier = dark brown
    on_route = np.asarray(route_dist) == 0.0
    img[on_route] = (40, 200, 230)       # guaranteed route = cyan
    pil = Image.fromarray(img).resize((px, px), Image.Resampling.NEAREST)
    return labeled(pil, label, sub="green=walkable  dark=too steep  cyan=route")


def _oblique_same_scale(pre, post, label_pre, label_post):
    """oblique_panel normalizes each z internally; to make the carve visible we render post on the SAME
    range as pre by appending pre's min/max as invisible corner anchors is overkill -- instead just render
    both and rely on the route panel for the quantitative read; oblique shows landscape character."""
    return oblique_panel(pre, label_pre), oblique_panel(post, label_post)


def _row(label, seed, span, kp):
    spec = ex._window_spec(129, span)
    p = dataclasses.replace(tc.TraverseParams(), scene_width_m=span)
    w = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, seed, spec)
    pre = v2.compose_windowed_height_v2(w, seed, spec, kp)
    res = tc.build_traverse_corridor(w, seed, spec, p, kp)
    post = pre + res["carve_delta"]
    ob_pre, ob_post = _oblique_same_scale(pre, post, f"{label}\npre-carve", f"post  resolved={res.get('resolved')}")
    route_panel = _passable_route_panel(post, res["route_dist"], p, "walkable + route (post)")
    return [ob_pre, ob_post, route_panel]


def _contact(path, rows, pad=12, bg=(250, 250, 250)):
    # rows: list of lists of PIL images (same count per row). Tile into a grid.
    rh = [max(im.height for im in row) for row in rows]
    cw = [max(rows[r][c].width for r in range(len(rows))) for c in range(len(rows[0]))]
    W = sum(cw) + pad * (len(cw) + 1)
    H = sum(rh) + pad * (len(rows) + 1)
    sheet = Image.new("RGB", (W, H), bg)
    y = pad
    for r, row in enumerate(rows):
        x = pad
        for c, im in enumerate(row):
            sheet.paste(im, (x, y))
            x += cw[c] + pad
        y += rh[r] + pad
    path.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(path)


def main():
    spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    wall = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=3.5, relief_amplitude=3.2)
    rows = [
        _row("low-corridor seed1 25.6km (play scale)", 1, ex.CHUNK_SPAN_M, spiky),
        _row("wall-sever seed42 4km (extreme)", 42, 4000.0, wall),
    ]
    _contact(OUT, rows)
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
