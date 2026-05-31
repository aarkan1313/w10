r"""Render a static review contact sheet for the rough-world chunk proof.

This is an offline owner-review aid for the JSON used by the Godot chunk scene.
It does not replace flying the scene; it makes seams, corridors, slope bands,
and seed variation easy to inspect at a glance.

Run:
    python tools/dem_pack/render_rough_world_chunks_review.py

Writes:
    D:\tmp\wg10_geography_engine\rough_world_chunks_review_contact.png
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

import analyze_rough_world_traversability as trav
from render_worldgen import hillshade


DATA_PATH = Path("wg-10/worldgen_terrain/generated/review/rough_world_chunks_3x3.json")
OUT = Path(r"D:\tmp\wg10_geography_engine\rough_world_chunks_review_contact.png")


def _world_array(seed_world: dict[str, object], key: str) -> np.ndarray:
    n = int(seed_world["world_n"])
    dtype = bool if key == "corridor" else np.float64
    return np.asarray(seed_world[key], dtype=dtype).reshape((n, n))


def _terrain_base_rgb(height: np.ndarray) -> np.ndarray:
    h = np.asarray(height, dtype=np.float64)
    t = np.clip((h + 1.0) * 0.5, 0.0, 1.0)
    low = np.array([0.40, 0.48, 0.38], dtype=np.float64)
    mid = np.array([0.62, 0.56, 0.40], dtype=np.float64)
    high = np.array([0.74, 0.70, 0.58], dtype=np.float64)
    crest = np.array([0.82, 0.79, 0.68], dtype=np.float64)
    out = np.zeros((*h.shape, 3), dtype=np.float64)
    m0 = t < 0.58
    m1 = (t >= 0.58) & (t < 0.90)
    m2 = t >= 0.90
    out[m0] = low + (mid - low) * (t[m0] / 0.58)[:, None]
    out[m1] = mid + (high - mid) * ((t[m1] - 0.58) / 0.32)[:, None]
    out[m2] = high + (crest - high) * ((t[m2] - 0.90) / 0.10)[:, None]
    shade = hillshade(h, exaggeration=1.2)
    lit = out * (0.48 + 0.78 * shade[..., None])
    return np.clip(lit * 255.0, 0.0, 255.0).astype(np.uint8)


def _resize(img: Image.Image, size: int) -> Image.Image:
    return img.resize((size, size), Image.Resampling.BILINEAR)


def _draw_label(img: Image.Image, title: str, sub: str = "") -> Image.Image:
    out = img.convert("RGB")
    draw = ImageDraw.Draw(out)
    band_h = 34 if sub else 22
    draw.rectangle((0, 0, out.width, band_h), fill=(0, 0, 0))
    draw.text((6, 4), title, fill=(255, 255, 255))
    if sub:
        draw.text((6, 19), sub, fill=(210, 225, 235))
    return out


def _draw_seams(img: Image.Image, payload: dict[str, object], color: tuple[int, int, int] = (0, 245, 255)) -> Image.Image:
    out = img.convert("RGB")
    draw = ImageDraw.Draw(out)
    chunk_count = int(payload["chunk_count"])
    chunk_n = int(payload["chunk_n"])
    world_n = chunk_count * (chunk_n - 1) + 1
    for boundary in range(1, chunk_count):
        pos = round(boundary * (chunk_n - 1) * (out.width - 1) / max(world_n - 1, 1))
        draw.line((pos, 0, pos, out.height), fill=color, width=2)
        draw.line((0, pos, out.width, pos), fill=color, width=2)
    return out


def _terrain_panel(payload: dict[str, object], seed_world: dict[str, object], panel_px: int, seam_guides: bool) -> Image.Image:
    img = Image.fromarray(_terrain_base_rgb(_world_array(seed_world, "height")), mode="RGB")
    img = _resize(img, panel_px)
    if seam_guides:
        img = _draw_seams(img, payload)
    return _draw_label(img, "terrain + seam guides" if seam_guides else "terrain", "no guides" if not seam_guides else "cyan lines are chunk borders")


def _corridor_panel(payload: dict[str, object], seed_world: dict[str, object], panel_px: int) -> Image.Image:
    h = _world_array(seed_world, "height")
    shade = hillshade(h, exaggeration=1.2)
    base = np.repeat((shade * 145.0 + 45.0).astype(np.uint8)[..., None], 3, axis=2)
    corridor = _world_array(seed_world, "corridor")
    base[corridor] = np.array([20, 190, 225], dtype=np.uint8)
    img = _draw_seams(_resize(Image.fromarray(base, mode="RGB"), panel_px), payload, color=(255, 255, 255))
    return _draw_label(img, "corridor mask", "cyan = exported route/corridor")


def _slope_panel(payload: dict[str, object], seed_world: dict[str, object], panel_px: int) -> Image.Image:
    h = _world_array(seed_world, "height")
    slopes = trav.slope_grid(
        h,
        scene_width_m=float(payload["world_span_m"]),
        height_scale_m=trav.BASE_HEIGHT_SCALE_M,
    )
    rgb = np.zeros((*h.shape, 3), dtype=np.uint8)
    rgb[slopes < trav.EASY_SLOPE] = np.array([46, 158, 72], dtype=np.uint8)
    rgb[(slopes >= trav.EASY_SLOPE) & (slopes < trav.PASSABLE_SLOPE)] = np.array([219, 184, 56], dtype=np.uint8)
    rgb[(slopes >= trav.PASSABLE_SLOPE) & (slopes < trav.STEEP_SLOPE)] = np.array([235, 102, 46], dtype=np.uint8)
    rgb[slopes >= trav.STEEP_SLOPE] = np.array([178, 33, 31], dtype=np.uint8)
    img = _draw_seams(_resize(Image.fromarray(rgb, mode="RGB"), panel_px), payload, color=(255, 255, 255))
    return _draw_label(img, "slope bands", "green/yellow/orange/red")


def panels_for_payload(payload: dict[str, object], panel_px: int = 300) -> list[Image.Image]:
    panels: list[Image.Image] = []
    for seed_world in payload["seeds"]:
        seed = seed_world["seed"]
        row = [
            _terrain_panel(payload, seed_world, panel_px, seam_guides=False),
            _terrain_panel(payload, seed_world, panel_px, seam_guides=True),
            _corridor_panel(payload, seed_world, panel_px),
            _slope_panel(payload, seed_world, panel_px),
        ]
        row[0] = _draw_label(row[0], f"seed {seed} terrain", "natural read")
        panels.extend(row)
    return panels


def contact_sheet(panels: list[Image.Image], cols: int = 4, gutter: int = 10) -> Image.Image:
    if not panels:
        raise ValueError("no panels")
    w, h = panels[0].size
    rows = (len(panels) + cols - 1) // cols
    out = Image.new("RGB", (cols * w + (cols + 1) * gutter, rows * h + (rows + 1) * gutter), (24, 27, 28))
    for i, panel in enumerate(panels):
        x = gutter + (i % cols) * (w + gutter)
        y = gutter + (i // cols) * (h + gutter)
        out.paste(panel, (x, y))
    return out


def render(payload_path: Path = DATA_PATH, out_path: Path = OUT, panel_px: int = 330) -> Path:
    payload = json.loads(payload_path.read_text(encoding="utf-8"))
    out_path.parent.mkdir(parents=True, exist_ok=True)
    sheet = contact_sheet(panels_for_payload(payload, panel_px=panel_px), cols=4)
    sheet.save(out_path)
    return out_path


def main() -> None:
    path = render()
    print(f"wrote {path}")


if __name__ == "__main__":
    main()
