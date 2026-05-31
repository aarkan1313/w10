"""Render Slice 2A landform-regime composition contact sheets.

This probe is a step beyond averaged biome params: build a coarse landform map first, give each regime its
own generator, route drainage over the composed scaffold, then add restrained detail. It is offline only.

Run:
    python tools/dem_pack/render_landform_regimes.py
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

OUT = r"D:\tmp\wg10_landform_regimes"


@dataclass(frozen=True)
class Scenario:
    key: str
    label: str
    range_gain: float
    basin_gain: float
    plateau_gain: float
    channel_gain: float
    detail_gain: float
    old_surface: float


SCENARIOS = (
    Scenario("balanced", "balanced mix", 1.00, 0.85, 0.70, 0.34, 0.060, 0.25),
    Scenario("range_front", "mountain front", 1.35, 0.65, 0.55, 0.36, 0.070, 0.15),
    Scenario("dissected_plateau", "dissected plateau", 0.55, 0.75, 1.35, 0.48, 0.085, 0.35),
    Scenario("basin_fans", "basin + fans", 0.75, 1.30, 0.45, 0.28, 0.035, 0.40),
    Scenario("rugged_ranges", "rugged ranges", 1.60, 0.45, 0.65, 0.42, 0.095, 0.10),
    Scenario("badlands_mix", "badlands mix", 0.70, 0.85, 1.20, 0.58, 0.120, 0.20),
    Scenario("soft_lowlands", "soft lowlands", 0.35, 1.45, 0.35, 0.22, 0.030, 0.55),
    Scenario("karst_like", "karst-like blocks", 0.65, 0.75, 1.05, 0.36, 0.070, 0.45),
)

REGIME_COLORS = np.array([
    [42, 78, 145],    # basin floor
    [83, 145, 68],    # alluvial plain
    [174, 140, 72],   # foothills
    [118, 95, 64],    # plateau
    [170, 170, 170],  # range core
], dtype=np.float64)


def grid(n: int, span: float, ox: float = 0.0, oz: float = 0.0):
    ii = np.linspace(0.0, span, n)
    return np.meshgrid(ii + ox, ii + oz)


def norm01(a: np.ndarray) -> np.ndarray:
    a = np.asarray(a, dtype=np.float64)
    return (a - float(a.min())) / (float(np.ptp(a)) + 1e-9)


def znorm(a: np.ndarray) -> np.ndarray:
    a = np.asarray(a, dtype=np.float64)
    return (a - float(a.mean())) / (float(a.std()) + 1e-9)


def softmax(scores: list[np.ndarray], temp: float = 0.34) -> list[np.ndarray]:
    stack = np.stack(scores, axis=0) / float(temp)
    stack = stack - np.max(stack, axis=0, keepdims=True)
    e = np.exp(stack)
    e = e / (np.sum(e, axis=0, keepdims=True) + 1e-9)
    return [e[i] for i in range(e.shape[0])]


def ridge_segments(wx: np.ndarray, wz: np.ndarray, span: float, seed: int, count: int, width_mul: float) -> np.ndarray:
    rng = np.random.default_rng(seed)
    out = np.zeros_like(wx, dtype=np.float64)
    for _ in range(count):
        cx, cz = rng.uniform(-0.18 * span, 1.18 * span, size=2)
        angle = rng.uniform(0.0, np.pi)
        length = rng.uniform(0.32 * span, 0.78 * span)
        width = rng.uniform(0.030 * span, 0.080 * span) * width_mul
        amp = rng.uniform(0.75, 1.2)
        vx = np.cos(angle) * length
        vz = np.sin(angle) * length
        x0 = cx - vx * 0.5
        z0 = cz - vz * 0.5
        denom = vx * vx + vz * vz + 1e-9
        t = np.clip(((wx - x0) * vx + (wz - z0) * vz) / denom, 0.0, 1.0)
        px = x0 + t * vx
        pz = z0 + t * vz
        d = np.sqrt((wx - px) * (wx - px) + (wz - pz) * (wz - pz))
        broad = np.exp(-((d / width) ** 2))
        crest = np.exp(-((d / (width * 0.24)) ** 2))
        out = np.maximum(out, amp * (0.64 * broad + 0.36 * crest))
    return norm01(out)


def fault_field(wx: np.ndarray, wz: np.ndarray, span: float, seed: int) -> np.ndarray:
    rng = np.random.default_rng(seed)
    out = np.zeros_like(wx, dtype=np.float64)
    for _ in range(7):
        cx, cz = rng.uniform(-0.1 * span, 1.1 * span, size=2)
        angle = rng.uniform(0.0, np.pi)
        nx = -np.sin(angle)
        nz = np.cos(angle)
        width = rng.uniform(0.025 * span, 0.075 * span)
        amp = rng.uniform(-0.8, 0.8)
        signed = (wx - cx) * nx + (wz - cz) * nz
        out += amp * np.tanh(signed / width) * np.exp(-((signed / (span * 0.55)) ** 2))
    return znorm(out)


def sinkhole_field(wx: np.ndarray, wz: np.ndarray, span: float, seed: int, count: int = 45) -> np.ndarray:
    rng = np.random.default_rng(seed)
    out = np.zeros_like(wx, dtype=np.float64)
    for _ in range(count):
        cx, cz = rng.uniform(0.0, span, size=2)
        r = rng.uniform(0.009 * span, 0.026 * span)
        d2 = (wx - cx) * (wx - cx) + (wz - cz) * (wz - cz)
        out += np.exp(-d2 / (2.0 * r * r))
    return norm01(out)


def alluvial_fans(wx: np.ndarray, wz: np.ndarray, span: float, ranges: np.ndarray, channels: np.ndarray) -> np.ndarray:
    foot = gaussian_filter(ranges, sigma=5.0) - gaussian_filter(ranges, sigma=15.0)
    foot = norm01(np.clip(foot, 0.0, None))
    fans = gaussian_filter(channels * foot, sigma=5.0)
    return norm01(fans)


def regime_weights(wx: np.ndarray, wz: np.ndarray, span: float, seed: int, scenario: Scenario):
    basins = norm01(wg.fbm(wx, wz, 1.0 / (span * 0.90), 4, seed + 1, gain=0.58))
    broad_ranges = ridge_segments(wx, wz, span, seed + 2, count=7, width_mul=1.65)
    sharp_ranges = ridge_segments(wx, wz, span, seed + 2, count=7, width_mul=0.55)
    faults = fault_field(wx, wz, span, seed + 3)
    plateau_noise = norm01(wg.fbm(wx, wz, 1.0 / (span * 0.34), 4, seed + 4, gain=0.52))

    range_score = scenario.range_gain * (1.55 * broad_ranges + 0.60 * sharp_ranges) + 0.18 * faults
    foothill_score = scenario.range_gain * (1.0 - np.abs(broad_ranges - 0.42) * 2.1) + 0.18 * plateau_noise
    plateau_score = scenario.plateau_gain * (0.85 * plateau_noise + 0.30 * np.abs(faults))
    basin_score = scenario.basin_gain * (1.15 * (1.0 - basins) + 0.28 * (1.0 - broad_ranges))
    alluvial_score = scenario.basin_gain * (0.55 * (1.0 - basins) + 0.45 * broad_ranges)

    weights = softmax([basin_score, alluvial_score, foothill_score, plateau_score, range_score], temp=0.32)
    fields = {
        "basins": basins,
        "broad_ranges": broad_ranges,
        "sharp_ranges": sharp_ranges,
        "faults": faults,
        "plateau_noise": plateau_noise,
    }
    return weights, fields


def compose_height(span: float, n: int, seed: int, scenario: Scenario, ox: float = 0.0, oz: float = 0.0):
    wx, wz = grid(n, span, ox=ox, oz=oz)
    weights, fields = regime_weights(wx - ox, wz - oz, span, seed, scenario)
    basin_w, alluvial_w, foothill_w, plateau_w, range_w = weights

    warped_x, warped_z = wg.recursive_domain_warp(wx, wz, span * 0.035, 1.0 / (span * 0.68), seed + 20, steps=2)
    low = znorm(wg.fbm(warped_x, warped_z, 1.0 / (span * 0.80), 4, seed + 30, gain=0.56))
    fine = znorm(wg.fbm(warped_x, warped_z, 1.0 / (span * 0.045), 3, seed + 31, gain=0.48))
    ridges = znorm(wg.ridged_multifractal(warped_x, warped_z, 1.0 / (span * 0.070), 5, seed + 32, gain=0.56))
    sharp = fields["sharp_ranges"]
    broad = fields["broad_ranges"]
    faults = fields["faults"]
    plateau = fields["plateau_noise"]

    range_h = 1.60 * broad + 1.05 * sharp + 0.22 * ridges + 0.18 * low
    foothill_h = 0.65 * broad + 0.42 * low + 0.17 * ridges
    plateau_h = 0.58 * plateau + 0.30 * faults + 0.20 * low
    basin_h = -0.38 + 0.34 * low - 0.24 * broad
    alluvial_h = -0.55 + 0.18 * low + 0.10 * fine

    scaffold = (
        basin_w * basin_h
        + alluvial_w * alluvial_h
        + foothill_w * foothill_h
        + plateau_w * plateau_h
        + range_w * range_h
    )
    scaffold = gaussian_filter(scaffold, sigma=1.1 + scenario.old_surface)
    scaffold = znorm(scaffold)

    channels = wg.flow_accumulation_channels(gaussian_filter(scaffold, sigma=1.7), power=0.62)
    channels = gaussian_filter(channels, sigma=1.15)
    fans = alluvial_fans(wx - ox, wz - oz, span, broad, channels)
    sinks = sinkhole_field(wx - ox, wz - oz, span, seed + 80, count=34)

    channel_mask = (0.45 + 0.65 * np.clip(scaffold, 0.0, None)) * channels
    height = scaffold - scenario.channel_gain * channel_mask
    height = height + 0.24 * fans * alluvial_w
    if scenario.key == "karst_like":
        height = height - 0.22 * sinks * (plateau_w + basin_w * 0.45)
    if scenario.detail_gain > 0.0:
        detail_mask = 0.25 + 0.55 * range_w + 0.35 * plateau_w + 0.20 * foothill_w
        height = height + scenario.detail_gain * detail_mask * (0.65 * ridges + 0.35 * fine)
    height = znorm(height)

    weights_rgb = np.zeros((*height.shape, 3), dtype=np.float64)
    for i, w in enumerate(weights):
        weights_rgb += w[..., None] * REGIME_COLORS[i]
    return height, channels, np.clip(weights_rgb, 0, 255).astype(np.uint8)


def to_hillshade_panel(z: np.ndarray, label: str, size: int, exaggeration: float) -> Image.Image:
    sh = hillshade(z, exaggeration=exaggeration)
    img = Image.fromarray((sh * 255).astype(np.uint8), mode="L").convert("RGB").resize((size, size))
    return labeled(img, label)


def labeled(img: Image.Image, label: str) -> Image.Image:
    label_h = 24
    out = Image.new("RGB", (img.width, img.height + label_h), (14, 14, 14))
    out.paste(img, (0, label_h))
    ImageDraw.Draw(out).text((6, 5), label, fill=(230, 230, 230))
    return out


def contact_sheet(path: str, panels: list[Image.Image], cols: int) -> None:
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


def render(span: float, n: int, panel: int, seed: int, ox: float, oz: float, suffix: str, exaggeration: float) -> None:
    panels = []
    debug_panels = []
    for scenario in SCENARIOS:
        z, channels, regimes = compose_height(span, n, seed, scenario, ox=ox, oz=oz)
        panels.append(to_hillshade_panel(z, scenario.label, panel, exaggeration))
        ch = Image.fromarray((channels * 255).astype(np.uint8), mode="L").convert("RGB").resize((panel, panel))
        reg = Image.fromarray(regimes, mode="RGB").resize((panel, panel))
        debug_panels.append(labeled(reg, f"{scenario.label} regimes"))
        debug_panels.append(labeled(ch, f"{scenario.label} drainage"))
    contact_sheet(os.path.join(OUT, f"landform_regimes_{suffix}.png"), panels, cols=4)
    contact_sheet(os.path.join(OUT, f"landform_regimes_{suffix}_debug.png"), debug_panels, cols=4)


def main() -> None:
    os.makedirs(OUT, exist_ok=True)
    render(span=220000.0, n=384, panel=320, seed=42, ox=0.0, oz=0.0, suffix="220km", exaggeration=1.05)
    render(span=45000.0, n=384, panel=320, seed=42, ox=82000.0, oz=56000.0, suffix="45km_close", exaggeration=1.65)


if __name__ == "__main__":
    main()
