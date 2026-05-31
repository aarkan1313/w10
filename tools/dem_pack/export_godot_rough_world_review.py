r"""Export rough-highlands generator worlds for a Godot switcher scene.

Run:
    python tools/dem_pack/export_godot_rough_world_review.py

Writes:
    wg-10/worldgen_terrain/generated/review/rough_world_3d.json
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
from scipy.ndimage import gaussian_filter, zoom

import geography_skeleton as skel
from render_geography_engine import REFERENCES, _load_reference_height
from render_geography_skeleton_focus import FOCUS


OUT = Path("wg-10/worldgen_terrain/generated/review/rough_world_3d.json")
N = 225
SPAN_M = 90000.0
SYN_OX = 60000.0
SYN_OZ = 36000.0
SEED = 133


def _center_crop(z: np.ndarray, crop_px: int) -> np.ndarray:
    crop_px = max(32, min(int(crop_px), min(z.shape)))
    y0 = (z.shape[0] - crop_px) // 2
    x0 = (z.shape[1] - crop_px) // 2
    return np.asarray(z[y0 : y0 + crop_px, x0 : x0 + crop_px], dtype=np.float64)


def _resample(z: np.ndarray) -> np.ndarray:
    scale_y = N / z.shape[0]
    scale_x = N / z.shape[1]
    out = zoom(z, (scale_y, scale_x), order=1)
    return out[:N, :N]


def _condition(z: np.ndarray) -> tuple[np.ndarray, dict[str, float]]:
    """Compress relief for review without clipping refs into white spikes."""
    z = np.asarray(z, dtype=np.float64)
    p05 = float(np.percentile(z, 5.0))
    p50 = float(np.percentile(z, 50.0))
    p95 = float(np.percentile(z, 95.0))
    robust = (z - p50) / (p95 - p05 + 1e-9) * 2.15
    broad = gaussian_filter(robust, sigma=0.65)
    shaped = np.tanh(broad)
    return shaped, {
        "source_min": float(np.min(z)),
        "source_max": float(np.max(z)),
        "source_ptp": float(np.ptp(z)),
        "p05": p05,
        "p50": p50,
        "p95": p95,
    }


def _item(key: str, label: str, kind: str, height: np.ndarray, span_m: float, source: str) -> dict:
    conditioned, stats = _condition(_resample(height))
    return {
        "key": key,
        "label": label,
        "kind": kind,
        "span_km": round(float(span_m) / 1000.0, 1),
        "source": source,
        "n": N,
        "height": np.round(conditioned, 4).astype(float).ravel().tolist(),
        "stats": stats,
    }


def _reference_items() -> list[dict]:
    items = []
    for kernel_id, label in REFERENCES[:4]:
        z, spacing = _load_reference_height(kernel_id)
        target_px = int(round(SPAN_M / spacing))
        crop_px = max(target_px, 240)
        crop = _center_crop(z, crop_px)
        actual_span_m = crop.shape[0] * float(spacing)
        key = "ref_" + label.lower().replace("ref ", "").replace(" ", "_")
        items.append(_item(key, label, "ref", crop, actual_span_m, kernel_id))
    return items


def _synth_items() -> list[dict]:
    wx, wz = skel.geo.grid(N, SPAN_M, ox=SYN_OX, oz=SYN_OZ)
    items = []
    for scenario in FOCUS:
        result = skel.compose_height(wx, wz, seed=SEED, scenario=scenario)
        items.append(_item(scenario.key, f"SYN {scenario.label}", "synth", result["height"], SPAN_M, scenario.key))
    return items


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "title": "WorldGen10 rough-highlands generated-world review",
        "span_km": SPAN_M / 1000.0,
        "items": _reference_items() + _synth_items(),
    }
    OUT.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
