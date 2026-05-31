r"""Render a narrow rough-highlands focus pass for Slice 2A.

Run:
    python tools/dem_pack/render_geography_skeleton_focus.py

Writes review images to D:\tmp\wg10_geography_engine.
"""

from __future__ import annotations

from dataclasses import replace
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

import geography_skeleton as skel
from render_geography_engine import REFERENCES, contact_sheet, labeled, panel_from_height, reference_panel
from render_worldgen import hillshade


OUT = Path(r"D:\tmp\wg10_geography_engine")

ROUGH = next(scenario for scenario in skel.SCENARIOS if scenario.key == "rough_highlands")
FOCUS = (
    replace(ROUGH, key="rough_anchor", label="rough anchor"),
    replace(
        ROUGH,
        key="rough_dry_fans",
        label="rough dry fans",
        fan_gain=1.32,
        channel_width=1.12,
        basin_smoothing=1.18,
        tributary_gain=0.78,
    ),
    replace(
        ROUGH,
        key="rough_dense_cuts",
        label="rough dense cuts",
        incision_gain=1.12,
        badlands_gain=1.18,
        close_detail=1.34,
        tributary_gain=1.16,
        channel_width=0.88,
    ),
    replace(
        ROUGH,
        key="rough_broad_crests",
        label="rough broad crests",
        range_texture=1.42,
        close_detail=1.02,
        range_spread=1.02,
        incision_gain=0.92,
    ),
    replace(
        ROUGH,
        key="rough_sharp_front",
        label="rough sharp front",
        range_texture=1.72,
        close_detail=1.08,
        range_spread=0.64,
        incision_gain=1.02,
        channel_width=0.82,
    ),
    replace(
        ROUGH,
        key="rough_soft_basin",
        label="rough soft basin",
        fill_gain=1.02,
        basin_smoothing=1.46,
        incision_gain=0.78,
        channel_width=1.18,
        close_detail=0.96,
    ),
)


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


def _panels(span_m: float, n: int, panel_px: int, seed: int, ox: float, oz: float, exaggeration: float):
    wx, wz = skel.geo.grid(n, span_m, ox=ox, oz=oz)
    panels: list[Image.Image] = []
    debug: list[Image.Image] = []
    notes: list[str] = []
    for scenario in FOCUS:
        result = skel.compose_height(wx, wz, seed=seed, scenario=scenario)
        z = result["height"]
        weights = result["weights"]
        skeleton = result["skeleton"]
        score = skel.straight_artifact_score(z)
        panels.append(panel_from_height(z, f"SYN {scenario.label}", panel_px, exaggeration, sub=f"line-score {score:.3f}"))
        debug.append(labeled(Image.fromarray(skel.regime_rgb(weights), mode="RGB").resize((panel_px, panel_px), Image.Resampling.NEAREST), f"{scenario.label} regimes"))
        debug.append(panel_from_height(skeleton["uplift"], f"{scenario.label} uplift", panel_px, 1.0))
        debug.append(panel_from_height(skeleton["discharge"], f"{scenario.label} discharge", panel_px, 1.0))
        debug.append(panel_from_height(skeleton["tributary"], f"{scenario.label} tributaries", panel_px, 1.0))
        notes.append(f"{scenario.key}: line_score={score:.4f} relief_ptp={float(np.ptp(z)):.4f}")
    return panels, debug, notes


def render_sheet(span_m: float, n: int, panel_px: int, seed: int, ox: float, oz: float, suffix: str, exaggeration: float) -> None:
    refs = [reference_panel(kernel_id, label, span_m, panel_px, exaggeration) for kernel_id, label in REFERENCES[:4]]
    synth, debug, notes = _panels(span_m, n, panel_px, seed, ox, oz, exaggeration)
    contact_sheet(OUT / f"geography_skeleton_rough_focus_{suffix}.png", refs + synth, cols=4)
    contact_sheet(OUT / f"geography_skeleton_rough_focus_{suffix}_debug.png", debug, cols=4)
    (OUT / f"geography_skeleton_rough_focus_{suffix}_notes.txt").write_text("\n".join(notes) + "\n", encoding="utf-8")


def render_scene(seed: int, ox: float, oz: float) -> None:
    wx, wz = skel.geo.grid(176, 45000.0, ox=ox, oz=oz)
    panels = []
    notes = []
    for scenario in FOCUS:
        result = skel.compose_height(wx, wz, seed=seed, scenario=scenario)
        z = result["height"]
        panels.append(oblique_panel(z, f"SYN {scenario.label}"))
        notes.append(f"{scenario.key}: line_score={skel.straight_artifact_score(z):.4f} relief_ptp={float(np.ptp(z)):.4f}")
    contact_sheet(OUT / "geography_skeleton_rough_focus_scene.png", panels, cols=2)
    (OUT / "geography_skeleton_rough_focus_scene_notes.txt").write_text("\n".join(notes) + "\n", encoding="utf-8")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    render_sheet(span_m=200000.0, n=384, panel_px=300, seed=133, ox=0.0, oz=0.0, suffix="200km", exaggeration=0.92)
    render_sheet(span_m=45000.0, n=512, panel_px=300, seed=133, ox=84000.0, oz=62000.0, suffix="45km_close", exaggeration=1.55)
    render_scene(seed=133, ox=84000.0, oz=62000.0)


if __name__ == "__main__":
    main()
