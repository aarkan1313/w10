r"""Render Slice 2A geography-engine contact sheets with real DEM references.

Run:
    python tools/dem_pack/render_geography_engine.py

Writes review images to D:\tmp\wg10_geography_engine.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import geography_engine as geo  # noqa: E402
from render_worldgen import hillshade  # noqa: E402


OUT = Path(r"D:\tmp\wg10_geography_engine")
WG9_KERNELS = Path(r"D:\workflows\worldgen9\factory\kernels")

REFERENCES = (
    ("mountain__cop30_bulk20260524_mountain_alps_mont_blanc_6_8_45_8", "REF Alps Mont Blanc"),
    ("mountain__cop30_bulk20260524_mountain_rockies_banff_115_6_51_2", "REF Rockies Banff"),
    ("badlands__cop30_badlands_grand_canyon_112_1_36_1", "REF Grand Canyon"),
    ("badlands__cop30_badlands_utah_canyonlands_109_7_38_25", "REF Canyonlands"),
    ("grassland__cop30_grassland_flint_hills_96_6_38_6", "REF Flint Hills"),
    ("karst__cop30_bulk20260524_karst_appalachian_valley_80_3_37_3", "REF Appalachian karst"),
)


def labeled(img: Image.Image, label: str, sub: str | None = None) -> Image.Image:
    label_h = 34 if sub else 24
    out = Image.new("RGB", (img.width, img.height + label_h), (14, 14, 14))
    out.paste(img, (0, label_h))
    draw = ImageDraw.Draw(out)
    draw.text((6, 4), label, fill=(236, 236, 236))
    if sub:
        draw.text((6, 18), sub, fill=(176, 176, 176))
    return out


def contact_sheet(path: Path, panels: list[Image.Image], cols: int = 4) -> None:
    gap = 8
    rows = int(np.ceil(len(panels) / cols))
    w = cols * panels[0].width + (cols - 1) * gap
    h = rows * panels[0].height + (rows - 1) * gap
    sheet = Image.new("RGB", (w, h), (8, 8, 8))
    for i, panel in enumerate(panels):
        x = (i % cols) * (panel.width + gap)
        y = (i // cols) * (panel.height + gap)
        sheet.paste(panel, (x, y))
    sheet.save(path)
    print(f"wrote {path}")


def panel_from_height(z: np.ndarray, label: str, panel_px: int, exaggeration: float, sub: str | None = None) -> Image.Image:
    sh = hillshade(np.asarray(z, dtype=np.float64), exaggeration=exaggeration)
    img = Image.fromarray((sh * 255).astype(np.uint8), mode="L").convert("RGB")
    img = img.resize((panel_px, panel_px), Image.Resampling.BICUBIC)
    return labeled(img, label, sub=sub)


def _load_reference_height(kernel_id: str) -> tuple[np.ndarray, float]:
    root = WG9_KERNELS / kernel_id
    meta = json.loads((root / "kernel.json").read_text(encoding="utf-8"))
    height_m = root / "height_m.npy"
    if height_m.exists():
        z = np.load(height_m)
    else:
        z = np.load(root / "normalized_height.npy")
    spacing = float(meta.get("approx_sample_spacing_m", 369.0))
    return np.asarray(z, dtype=np.float64), spacing


def reference_panel(kernel_id: str, label: str, span_m: float, panel_px: int, exaggeration: float) -> Image.Image:
    z, spacing = _load_reference_height(kernel_id)
    crop_px = int(round(float(span_m) / spacing))
    # The current WG9 reference kernels are ~512 px over continental crops. A literal 45 km crop can be only
    # ~120 source pixels and becomes blocky when enlarged. Keep enough native pixels for visual comparison and
    # label the true span honestly.
    if crop_px < 220:
        crop_px = 220
    crop_px = max(32, min(crop_px, min(z.shape)))
    actual_span_m = crop_px * spacing
    y0 = (z.shape[0] - crop_px) // 2
    x0 = (z.shape[1] - crop_px) // 2
    crop = z[y0 : y0 + crop_px, x0 : x0 + crop_px]
    return panel_from_height(crop, label, panel_px, exaggeration, sub=f"real DEM crop ~{actual_span_m/1000:.0f} km")


def synth_panels(span_m: float, n: int, panel_px: int, seed: int, ox: float, oz: float, exaggeration: float):
    panels: list[Image.Image] = []
    debug_panels: list[Image.Image] = []
    notes: list[str] = []
    for i, scenario in enumerate(geo.SCENARIOS):
        sx = ox
        sz = oz
        wx, wz = geo.grid(n, span_m, ox=sx, oz=sz)
        result = geo.compose_height(wx, wz, seed=seed, scenario=scenario)
        z = result["height"]
        weights = result["weights"]
        fields = result["fields"]
        branches = result["branches"]
        score = geo.straight_artifact_score(z)
        panels.append(panel_from_height(z, f"SYN {scenario.label}", panel_px, exaggeration, sub=f"line-score {score:.3f}"))
        debug_panels.append(labeled(Image.fromarray(geo.regime_rgb(weights), mode="RGB").resize((panel_px, panel_px), Image.Resampling.NEAREST), f"{scenario.label} regimes"))
        debug_panels.append(panel_from_height(fields["channels"], f"{scenario.label} drainage", panel_px, 1.0))
        debug_panels.append(panel_from_height(branches, f"{scenario.label} branches", panel_px, 1.0))
        debug_panels.append(panel_from_height(fields["range"], f"{scenario.label} range mask", panel_px, 1.0))
        notes.append(f"{scenario.key}: line_score={score:.4f} relief_ptp={float(np.ptp(z)):.4f}")
    return panels, debug_panels, notes


def render_sheet(span_m: float, n: int, panel_px: int, seed: int, ox: float, oz: float, suffix: str, exaggeration: float) -> None:
    refs = [reference_panel(kernel_id, label, span_m, panel_px, exaggeration) for kernel_id, label in REFERENCES]
    synth, debug, notes = synth_panels(span_m, n, panel_px, seed, ox, oz, exaggeration)
    contact_sheet(OUT / f"geography_engine_v5_{suffix}.png", refs + synth, cols=4)
    contact_sheet(OUT / f"geography_engine_v5_{suffix}_debug.png", debug, cols=4)
    (OUT / f"geography_engine_v5_{suffix}_notes.txt").write_text("\n".join(notes) + "\n", encoding="utf-8")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    render_sheet(span_m=200000.0, n=384, panel_px=300, seed=91, ox=0.0, oz=0.0, suffix="200km", exaggeration=1.05)
    render_sheet(span_m=45000.0, n=512, panel_px=300, seed=91, ox=84000.0, oz=62000.0, suffix="45km_close", exaggeration=2.10)


if __name__ == "__main__":
    main()
