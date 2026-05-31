r"""Render an oblique scene-read probe for the current best Slice 2A candidates.

This is not a Godot/runtime render. It is a quick offline question-answering tool:
does the best hillshade candidate still read as terrain when viewed obliquely instead
of as a forensic top-down DEM?

Run:
    python tools/dem_pack/render_geography_scene_probe.py

Writes:
    D:\tmp\wg10_geography_engine\geography_scene_probe_v0.png
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

import geography_engine as geo
from render_geography_engine import contact_sheet, labeled
from render_geography_focus import FOCUS
from render_worldgen import hillshade


OUT = Path(r"D:\tmp\wg10_geography_engine")
SCENE_KEYS = ("best_v5", "range_edge", "incised_rough", "scene_smooth")


def _terrain_color(height01: float, shade: float) -> tuple[int, int, int]:
    low = np.array([76, 92, 76], dtype=np.float64)
    mid = np.array([132, 119, 88], dtype=np.float64)
    high = np.array([176, 172, 155], dtype=np.float64)
    if height01 < 0.55:
        t = height01 / 0.55
        base = low * (1.0 - t) + mid * t
    else:
        t = (height01 - 0.55) / 0.45
        base = mid * (1.0 - t) + high * t
    lit = base * (0.38 + 0.90 * shade)
    return tuple(np.clip(lit, 0, 255).astype(np.uint8))


def _project(x01: np.ndarray, z01: np.ndarray, h01: np.ndarray, width: int, height: int) -> tuple[np.ndarray, np.ndarray]:
    perspective = 0.48 + 0.70 * z01
    sx = width * 0.50 + (x01 - 0.5) * width * 1.16 * perspective + (z01 - 0.5) * width * 0.10
    sy = height * 0.10 + z01 * height * 0.78 - h01 * height * 0.30 * perspective
    return sx, sy


def oblique_panel(z: np.ndarray, label: str, width: int = 680, height: int = 430) -> Image.Image:
    z = np.asarray(z, dtype=np.float64)
    z01 = (z - float(z.min())) / (float(np.ptp(z)) + 1e-9)
    shade = hillshade(z, exaggeration=1.75)

    scale = 2
    img = Image.new("RGB", (width * scale, height * scale), (144, 170, 202))
    draw = ImageDraw.Draw(img)
    for y in range(height * scale):
        t = y / max(height * scale - 1, 1)
        sky = np.array([150, 176, 210]) * (1.0 - t) + np.array([210, 215, 205]) * t
        draw.line([(0, y), (width * scale, y)], fill=tuple(sky.astype(np.uint8)))

    rows, cols = z.shape
    xs = np.linspace(0.0, 1.0, cols)
    zs = np.linspace(0.0, 1.0, rows)
    xx, zz = np.meshgrid(xs, zs)
    sx, sy = _project(xx, zz, z01, width * scale, height * scale)

    # Painter's algorithm: far rows first, near rows last.
    for j in range(rows - 2, -1, -1):
        for i in range(cols - 1):
            poly = [
                (float(sx[j, i]), float(sy[j, i])),
                (float(sx[j, i + 1]), float(sy[j, i + 1])),
                (float(sx[j + 1, i + 1]), float(sy[j + 1, i + 1])),
                (float(sx[j + 1, i]), float(sy[j + 1, i])),
            ]
            h = float((z01[j, i] + z01[j, i + 1] + z01[j + 1, i] + z01[j + 1, i + 1]) * 0.25)
            sh = float((shade[j, i] + shade[j, i + 1] + shade[j + 1, i] + shade[j + 1, i + 1]) * 0.25)
            draw.polygon(poly, fill=_terrain_color(h, sh))

    img = img.resize((width, height), Image.Resampling.LANCZOS)
    return labeled(img, label, sub="offline oblique probe, 45 km crop")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    scenarios = {scenario.key: scenario for scenario in FOCUS}
    wx, wz = geo.grid(176, 45000.0, ox=84000.0, oz=62000.0)
    panels = []
    for key in SCENE_KEYS:
        scenario = scenarios[key]
        result = geo.compose_height(wx, wz, seed=91, scenario=scenario)
        panels.append(oblique_panel(result["height"], f"SYN {scenario.label}"))
    path = OUT / "geography_scene_probe_v0.png"
    contact_sheet(path, panels, cols=2)


if __name__ == "__main__":
    main()

