r"""Render Slice 2A / 7B-lite skeleton-first contact sheets.

Run:
    python tools/dem_pack/render_geography_skeleton.py

Writes review images to D:\tmp\wg10_geography_engine.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
from PIL import Image

import geography_skeleton as skel
from render_geography_engine import REFERENCES, contact_sheet, labeled, panel_from_height, reference_panel


OUT = Path(r"D:\tmp\wg10_geography_engine")


def _panels(span_m: float, n: int, panel_px: int, seed: int, ox: float, oz: float, exaggeration: float):
    wx, wz = skel.geo.grid(n, span_m, ox=ox, oz=oz)
    panels: list[Image.Image] = []
    debug: list[Image.Image] = []
    notes: list[str] = []
    for scenario in skel.SCENARIOS:
        result = skel.compose_height(wx, wz, seed=seed, scenario=scenario)
        z = result["height"]
        weights = result["weights"]
        skeleton = result["skeleton"]
        score = skel.straight_artifact_score(z)
        panels.append(panel_from_height(z, f"SYN {scenario.label}", panel_px, exaggeration, sub=f"line-score {score:.3f}"))
        debug.append(labeled(Image.fromarray(skel.regime_rgb(weights), mode="RGB").resize((panel_px, panel_px), Image.Resampling.NEAREST), f"{scenario.label} regimes"))
        debug.append(panel_from_height(skeleton["uplift"], f"{scenario.label} uplift", panel_px, 1.0))
        debug.append(panel_from_height(skeleton["discharge"], f"{scenario.label} discharge", panel_px, 1.0))
        debug.append(panel_from_height(skeleton["channel_dist"], f"{scenario.label} channel dist", panel_px, 1.0))
        notes.append(f"{scenario.key}: line_score={score:.4f} relief_ptp={float(np.ptp(z)):.4f}")
    return panels, debug, notes


def render_sheet(span_m: float, n: int, panel_px: int, seed: int, ox: float, oz: float, suffix: str, exaggeration: float) -> None:
    refs = [reference_panel(kernel_id, label, span_m, panel_px, exaggeration) for kernel_id, label in REFERENCES[:4]]
    synth, debug, notes = _panels(span_m, n, panel_px, seed, ox, oz, exaggeration)
    contact_sheet(OUT / f"geography_skeleton_v1_{suffix}.png", refs + synth, cols=4)
    contact_sheet(OUT / f"geography_skeleton_v1_{suffix}_debug.png", debug, cols=4)
    (OUT / f"geography_skeleton_v1_{suffix}_notes.txt").write_text("\n".join(notes) + "\n", encoding="utf-8")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    render_sheet(span_m=200000.0, n=384, panel_px=300, seed=133, ox=0.0, oz=0.0, suffix="200km", exaggeration=0.92)
    render_sheet(span_m=45000.0, n=512, panel_px=300, seed=133, ox=84000.0, oz=62000.0, suffix="45km_close", exaggeration=1.55)


if __name__ == "__main__":
    main()
