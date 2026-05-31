r"""Render focused Slice 2A variants around the current best badlands_mix candidate.

Run:
    python tools/dem_pack/render_geography_focus.py

Writes review images to D:\tmp\wg10_geography_engine.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
from PIL import Image

import geography_engine as geo
from render_geography_engine import REFERENCES, contact_sheet, panel_from_height, reference_panel


OUT = Path(r"D:\tmp\wg10_geography_engine")

FOCUS = (
    geo.GeographyScenario(
        "best_v5",
        "best v5",
        range_gain=0.75,
        plateau_gain=1.25,
        badlands_gain=1.55,
        channel_gain=0.78,
        detail_gain=0.10,
    ),
    geo.GeographyScenario(
        "less_channel",
        "less channel",
        range_gain=0.75,
        plateau_gain=1.22,
        badlands_gain=1.45,
        channel_gain=0.62,
        detail_gain=0.10,
    ),
    geo.GeographyScenario(
        "more_texture",
        "more texture",
        range_gain=0.76,
        plateau_gain=1.20,
        badlands_gain=1.58,
        channel_gain=0.70,
        detail_gain=0.135,
    ),
    geo.GeographyScenario(
        "plateau_cut",
        "plateau cut",
        range_gain=0.62,
        plateau_gain=1.52,
        badlands_gain=1.45,
        channel_gain=0.70,
        detail_gain=0.095,
    ),
    geo.GeographyScenario(
        "range_edge",
        "range edge",
        range_gain=1.00,
        plateau_gain=1.08,
        foothill_gain=1.15,
        badlands_gain=1.35,
        channel_gain=0.66,
        detail_gain=0.105,
    ),
    geo.GeographyScenario(
        "scene_smooth",
        "scene smooth",
        range_gain=0.72,
        plateau_gain=1.18,
        badlands_gain=1.32,
        channel_gain=0.54,
        detail_gain=0.070,
    ),
    geo.GeographyScenario(
        "incised_rough",
        "incised rough",
        range_gain=0.82,
        plateau_gain=1.18,
        badlands_gain=1.80,
        channel_gain=0.86,
        detail_gain=0.130,
    ),
    geo.GeographyScenario(
        "basin_edge",
        "basin edge",
        range_gain=0.72,
        basin_gain=1.15,
        fan_gain=1.20,
        plateau_gain=1.10,
        badlands_gain=1.42,
        channel_gain=0.62,
        detail_gain=0.090,
    ),
)


def _variant_panels(span_m: float, n: int, panel_px: int, seed: int, ox: float, oz: float, exaggeration: float):
    wx, wz = geo.grid(n, span_m, ox=ox, oz=oz)
    panels: list[Image.Image] = []
    notes: list[str] = []
    for scenario in FOCUS:
        result = geo.compose_height(wx, wz, seed=seed, scenario=scenario)
        z = result["height"]
        score = geo.straight_artifact_score(z)
        panels.append(panel_from_height(z, f"SYN {scenario.label}", panel_px, exaggeration, sub=f"line-score {score:.3f}"))
        notes.append(f"{scenario.key}: line_score={score:.4f} relief_ptp={float(np.ptp(z)):.4f}")
    return panels, notes


def render_focus(span_m: float, n: int, panel_px: int, seed: int, ox: float, oz: float, suffix: str, exaggeration: float) -> None:
    # Four references keeps the focus sheet compact while still showing the target visual bar.
    refs = [reference_panel(kernel_id, label, span_m, panel_px, exaggeration) for kernel_id, label in REFERENCES[:4]]
    synth, notes = _variant_panels(span_m, n, panel_px, seed, ox, oz, exaggeration)
    path = OUT / f"geography_focus_badlands_v0_{suffix}.png"
    contact_sheet(path, refs + synth, cols=4)
    (OUT / f"geography_focus_badlands_v0_{suffix}_notes.txt").write_text("\n".join(notes) + "\n", encoding="utf-8")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    render_focus(span_m=200000.0, n=384, panel_px=300, seed=91, ox=0.0, oz=0.0, suffix="200km", exaggeration=1.05)
    render_focus(span_m=45000.0, n=512, panel_px=300, seed=91, ox=84000.0, oz=62000.0, suffix="45km_close", exaggeration=2.10)


if __name__ == "__main__":
    main()

