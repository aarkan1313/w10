"""Render a broad Slice 2A coarse-structure matrix for owner review.

This intentionally tests combinations, not one "new noise." Rows are coarse scaffolds; columns add
drainage/detail treatments. The goal is to see whether a world-anchored coarse uplift + routed drainage
stack reads more like geography before any runtime/Rust port.

Run:
    python tools/dem_pack/render_structure_matrix.py
"""
from __future__ import annotations

import os
import sys
from dataclasses import dataclass

import numpy as np
from PIL import Image, ImageDraw
from scipy.ndimage import gaussian_filter

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import worldgen_proto as wg  # noqa: E402
from render_worldgen import hillshade  # noqa: E402

OUT = r"D:\tmp\wg10_structure_matrix"


@dataclass(frozen=True)
class Scaffold:
    key: str
    label: str


@dataclass(frozen=True)
class Treatment:
    key: str
    label: str
    carve: float
    detail: float
    relax: float


SCAFFOLDS = (
    Scaffold("basins", "broad basins"),
    Scaffold("ranges", "range spines"),
    Scaffold("faulted", "faulted ranges"),
    Scaffold("mixed", "basins + ranges"),
)

TREATMENTS = (
    Treatment("raw", "raw scaffold", 0.00, 0.00, 1.2),
    Treatment("soft_flow", "soft flow carve", 0.22, 0.03, 1.0),
    Treatment("strong_flow", "strong flow carve", 0.42, 0.04, 0.8),
    Treatment("flow_detail", "flow + fine detail", 0.34, 0.13, 0.6),
)


def grid(n: int, span: float, ox: float = 0.0, oz: float = 0.0):
    ii = np.linspace(0.0, span, n)
    return np.meshgrid(ii + ox, ii + oz)


def normalize(z: np.ndarray) -> np.ndarray:
    z = np.asarray(z, dtype=np.float64)
    return (z - float(z.mean())) / (float(z.std()) + 1e-9)


def rng_segments(seed: int, span: float, count: int):
    rng = np.random.default_rng(seed)
    centers = rng.uniform(-0.15 * span, 1.15 * span, size=(count, 2))
    angles = rng.uniform(0.0, np.pi, size=count)
    lengths = rng.uniform(0.35 * span, 0.75 * span, size=count)
    widths = rng.uniform(0.035 * span, 0.085 * span, size=count)
    amps = rng.uniform(0.7, 1.15, size=count)
    return centers, angles, lengths, widths, amps


def segment_ridges(wx: np.ndarray, wz: np.ndarray, span: float, seed: int, count: int) -> np.ndarray:
    centers, angles, lengths, widths, amps = rng_segments(seed, span, count)
    out = np.zeros_like(wx, dtype=np.float64)
    for i in range(count):
        vx = np.cos(angles[i]) * lengths[i]
        vz = np.sin(angles[i]) * lengths[i]
        x0 = centers[i, 0] - vx * 0.5
        z0 = centers[i, 1] - vz * 0.5
        denom = vx * vx + vz * vz + 1e-9
        t = np.clip(((wx - x0) * vx + (wz - z0) * vz) / denom, 0.0, 1.0)
        px = x0 + t * vx
        pz = z0 + t * vz
        d = np.sqrt((wx - px) * (wx - px) + (wz - pz) * (wz - pz))
        broad = np.exp(-((d / widths[i]) ** 2))
        crest = np.exp(-((d / (widths[i] * 0.28)) ** 2))
        out = np.maximum(out, amps[i] * (0.68 * broad + 0.32 * crest))
    return np.clip(out, 0.0, None)


def fault_planes(wx: np.ndarray, wz: np.ndarray, span: float, seed: int, count: int) -> np.ndarray:
    rng = np.random.default_rng(seed)
    out = np.zeros_like(wx, dtype=np.float64)
    for _ in range(count):
        cx, cz = rng.uniform(-0.1 * span, 1.1 * span, size=2)
        angle = rng.uniform(0.0, np.pi)
        nx = -np.sin(angle)
        nz = np.cos(angle)
        width = rng.uniform(0.04 * span, 0.11 * span)
        amp = rng.uniform(-0.9, 0.9)
        signed = (wx - cx) * nx + (wz - cz) * nz
        influence = np.exp(-((signed / (span * 0.45)) ** 2))
        out += amp * np.tanh(signed / width) * influence
    return out / max(count * 0.35, 1e-9)


def flow_channels(z: np.ndarray, sigma: float = 1.4, power: float = 0.58) -> np.ndarray:
    channels = wg.flow_accumulation_channels(gaussian_filter(z, sigma=sigma), power=power)
    channels = gaussian_filter(channels, sigma=1.0)
    channels = channels / (float(channels.max()) + 1e-9)
    return channels


def make_height(scaffold: Scaffold, treatment: Treatment, n: int, span: float, seed: int, ox: float, oz: float) -> np.ndarray:
    wx, wz = grid(n, span, ox=ox, oz=oz)
    low = wg.fbm(wx, wz, 1.0 / (span * 0.92), 4, seed + 10, gain=0.56)
    mid = wg.fbm(wx, wz, 1.0 / (span * 0.38), 3, seed + 20, gain=0.50)
    basins = normalize(0.75 * low + 0.35 * mid)
    ranges = segment_ridges(wx - ox, wz - oz, span, seed + 30, count=6)
    faults = fault_planes(wx - ox, wz - oz, span, seed + 40, count=5)
    fine_ridge = wg.ridged_multifractal(wx, wz, 1.0 / (span * 0.075), 4, seed + 50, gain=0.55)
    fine_noise = wg.fbm(wx, wz, 1.0 / (span * 0.045), 3, seed + 60, gain=0.48)

    if scaffold.key == "basins":
        h = 0.95 * basins + 0.20 * ranges
    elif scaffold.key == "ranges":
        h = 0.42 * basins + 1.05 * ranges
    elif scaffold.key == "faulted":
        h = 0.38 * basins + 0.62 * ranges + 0.72 * faults
    elif scaffold.key == "mixed":
        h = 0.72 * basins + 0.72 * ranges + 0.28 * faults
    else:
        raise ValueError(scaffold.key)

    h = normalize(h)
    channels = flow_channels(h)
    h = h - treatment.carve * (0.35 + 0.65 * np.clip(h, 0.0, None)) * channels
    if treatment.detail > 0.0:
        h = h + treatment.detail * (0.65 * normalize(fine_ridge) + 0.35 * fine_noise)
    if treatment.relax > 0.0:
        h = gaussian_filter(h, sigma=treatment.relax)
    return normalize(h)


def to_panel(z: np.ndarray, title: str, size: int, exaggeration: float) -> Image.Image:
    sh = hillshade(z, exaggeration=exaggeration)
    img = Image.fromarray((sh * 255).astype(np.uint8), mode="L").convert("RGB").resize((size, size))
    label_h = 26
    panel = Image.new("RGB", (size, size + label_h), (16, 16, 16))
    panel.paste(img, (0, label_h))
    ImageDraw.Draw(panel).text((6, 6), title, fill=(230, 230, 230))
    return panel


def save_matrix(path: str, span: float, n: int, panel_size: int, seed: int, ox: float, oz: float, exaggeration: float) -> None:
    panels = []
    for scaffold in SCAFFOLDS:
        row = []
        for treatment in TREATMENTS:
            z = make_height(scaffold, treatment, n=n, span=span, seed=seed, ox=ox, oz=oz)
            row.append(to_panel(z, f"{scaffold.label} | {treatment.label}", panel_size, exaggeration))
        panels.append(row)

    gap = 8
    header_h = 34
    w = len(TREATMENTS) * panel_size + (len(TREATMENTS) - 1) * gap
    h = header_h + len(SCAFFOLDS) * (panel_size + 26) + (len(SCAFFOLDS) - 1) * gap
    sheet = Image.new("RGB", (w, h), (8, 8, 8))
    d = ImageDraw.Draw(sheet)
    d.text((8, 8), f"WorldGen10 Slice 2A coarse structure matrix | span={span/1000:.0f}km | seed={seed}", fill=(235, 235, 235))
    y = header_h
    for row in panels:
        x = 0
        for panel in row:
            sheet.paste(panel, (x, y))
            x += panel_size + gap
        y += panel_size + 26 + gap
    sheet.save(path)
    print(f"wrote {path}")


def main() -> None:
    os.makedirs(OUT, exist_ok=True)
    save_matrix(
        os.path.join(OUT, "structure_matrix_200km.png"),
        span=200000.0,
        n=320,
        panel_size=300,
        seed=19,
        ox=0.0,
        oz=0.0,
        exaggeration=1.1,
    )
    save_matrix(
        os.path.join(OUT, "structure_matrix_40km_close.png"),
        span=40000.0,
        n=320,
        panel_size=300,
        seed=19,
        ox=90000.0,
        oz=60000.0,
        exaggeration=1.8,
    )


if __name__ == "__main__":
    main()
