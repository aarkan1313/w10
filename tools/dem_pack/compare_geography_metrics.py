r"""Compare cheap shape metrics for Slice 2A synth candidates and real DEM references.

Metrics are secondary diagnostics. They are not terrain-quality acceptance.

Run:
    python tools/dem_pack/compare_geography_metrics.py

Writes CSV/Markdown reports to:
    D:\tmp\wg10_geography_engine
"""

from __future__ import annotations

import csv
import json
from pathlib import Path

import numpy as np
from scipy.ndimage import distance_transform_edt, gaussian_filter, laplace, maximum_filter, minimum_filter, uniform_filter

import geography_engine as geo
import geography_skeleton as skel
from render_geography_engine import REFERENCES
from render_geography_focus import FOCUS as V5_FOCUS
from render_geography_skeleton_focus import FOCUS as ROUGH_FOCUS


OUT = Path(r"D:\tmp\wg10_geography_engine")
WG9_KERNELS = Path(r"D:\workflows\worldgen9\factory\kernels")
V5_SYNTH_KEYS = ("best_v5", "range_edge", "incised_rough", "scene_smooth")


def _odd_window(samples: float) -> int:
    n = max(3, int(round(samples)))
    return n + 1 if n % 2 == 0 else n


def _zscore(a: np.ndarray) -> np.ndarray:
    a = np.asarray(a, dtype=np.float64)
    return (a - float(a.mean())) / (float(a.std()) + 1e-9)


def _crop_center(z: np.ndarray, crop_px: int) -> np.ndarray:
    crop_px = max(32, min(int(crop_px), min(z.shape)))
    y0 = (z.shape[0] - crop_px) // 2
    x0 = (z.shape[1] - crop_px) // 2
    return z[y0 : y0 + crop_px, x0 : x0 + crop_px]


def load_reference(kernel_id: str, requested_span_m: float) -> tuple[np.ndarray, float, float]:
    root = WG9_KERNELS / kernel_id
    meta = json.loads((root / "kernel.json").read_text(encoding="utf-8"))
    height_m = root / "height_m.npy"
    z = np.load(height_m if height_m.exists() else root / "normalized_height.npy")
    spacing = float(meta.get("approx_sample_spacing_m", 369.0))
    crop_px = int(round(float(requested_span_m) / spacing))
    if crop_px < 220:
        crop_px = 220
    crop = _crop_center(np.asarray(z, dtype=np.float64), crop_px)
    return crop, spacing, crop.shape[0] * spacing


def local_relief_mean(z: np.ndarray, spacing_m: float, window_m: float) -> float:
    size = _odd_window(window_m / max(float(spacing_m), 1.0))
    hi = maximum_filter(z, size=size, mode="nearest")
    lo = minimum_filter(z, size=size, mode="nearest")
    return float(np.mean(hi - lo))


def hypsometric_integral(z: np.ndarray) -> float:
    z = np.asarray(z, dtype=np.float64)
    return float((float(np.mean(z)) - float(np.min(z))) / (float(np.ptp(z)) + 1e-9))


def _skew(a: np.ndarray) -> float:
    a = np.asarray(a, dtype=np.float64)
    centered = a - float(np.mean(a))
    std = float(np.std(centered)) + 1e-9
    return float(np.mean((centered / std) ** 3))


def _mean_vrm(gx: np.ndarray, gy: np.ndarray, window_px: int = 7) -> float:
    nx = -gx
    ny = -gy
    nz = np.ones_like(gx, dtype=np.float64)
    length = np.sqrt(nx * nx + ny * ny + nz * nz) + 1e-9
    nx = nx / length
    ny = ny / length
    nz = nz / length
    mx = uniform_filter(nx, size=window_px, mode="nearest")
    my = uniform_filter(ny, size=window_px, mode="nearest")
    mz = uniform_filter(nz, size=window_px, mode="nearest")
    return float(np.mean(1.0 - np.sqrt(mx * mx + my * my + mz * mz)))


def _feature_spacing_m(zn: np.ndarray, spacing_m: float, feature: str) -> float:
    smooth = gaussian_filter(zn, sigma=2.0)
    neighborhood = _odd_window(9)
    if feature == "ridge":
        mask = (smooth >= maximum_filter(smooth, size=neighborhood, mode="nearest") - 1e-9) & (
            smooth > np.quantile(smooth, 0.68)
        )
    else:
        mask = (smooth <= minimum_filter(smooth, size=neighborhood, mode="nearest") + 1e-9) & (
            smooth < np.quantile(smooth, 0.32)
        )
    if int(np.sum(mask)) < 4:
        return 0.0
    # Median distance-to-feature is roughly half spacing between nearby ridges/valleys.
    return float(np.median(distance_transform_edt(~mask) * float(spacing_m)) * 2.0)


def metrics(label: str, kind: str, z: np.ndarray, spacing_m: float, span_m: float) -> dict[str, float | str]:
    zn = _zscore(z)
    gy, gx = np.gradient(zn, float(spacing_m), float(spacing_m))
    slope = np.sqrt(gx * gx + gy * gy)
    curv = laplace(zn)
    highpass = zn - gaussian_filter(zn, sigma=max(1.0, 4000.0 / max(float(spacing_m), 1.0)))
    rel_2k = local_relief_mean(zn, spacing_m, 2000.0)
    rel_10k = local_relief_mean(zn, spacing_m, 10000.0)
    return {
        "label": label,
        "kind": kind,
        "source": "",
        "span_km": round(float(span_m) / 1000.0, 2),
        "spacing_m": round(float(spacing_m), 2),
        "std": round(float(np.std(zn)), 5),
        "ptp_z": round(float(np.ptp(zn)), 5),
        "hypsometric_integral": round(hypsometric_integral(z), 5),
        "relief_2km": round(rel_2k, 5),
        "relief_10km": round(rel_10k, 5),
        "relief_ratio_2_10": round(rel_2k / (rel_10k + 1e-9), 5),
        "slope_mean": round(float(np.mean(slope)), 8),
        "slope_p95": round(float(np.percentile(slope, 95)), 8),
        "slope_std": round(float(np.std(slope)), 8),
        "slope_skew": round(_skew(slope), 5),
        "vrm_7px": round(_mean_vrm(gx, gy), 8),
        "curv_pos_frac": round(float(np.mean(curv > 0.0)), 5),
        "curv_neg_frac": round(float(np.mean(curv < 0.0)), 5),
        "curv_balance": round(float(np.mean(curv > 0.0) - np.mean(curv < 0.0)), 5),
        "curv_abs_mean": round(float(np.mean(np.abs(curv))), 5),
        "ridge_spacing_m": round(_feature_spacing_m(zn, spacing_m, "ridge"), 2),
        "valley_spacing_m": round(_feature_spacing_m(zn, spacing_m, "valley"), 2),
        "highpass_std": round(float(np.std(highpass)), 5),
        "straight_score": round(float(geo.straight_artifact_score(zn)), 5),
    }


def add_regime_metrics(row: dict[str, float | str], weights: list[np.ndarray]) -> dict[str, float | str]:
    names = ("basin", "fan", "foothill", "plateau", "range", "badlands")
    entropy = np.zeros_like(weights[0], dtype=np.float64)
    for name, weight in zip(names, weights):
        w = np.asarray(weight, dtype=np.float64)
        row[f"{name}_prop"] = round(float(np.mean(w > 0.33)), 5)
        entropy -= w * np.log2(np.clip(w, 1e-9, 1.0))
    row["regime_entropy"] = round(float(np.mean(entropy)), 5)
    return row


def synth_rows_v5(span_m: float, n: int, spacing_m: float) -> list[dict[str, float | str]]:
    scenarios = {scenario.key: scenario for scenario in V5_FOCUS}
    wx, wz = geo.grid(n, span_m, ox=84000.0 if span_m < 70000 else 0.0, oz=62000.0 if span_m < 70000 else 0.0)
    rows = []
    for key in V5_SYNTH_KEYS:
        scenario = scenarios[key]
        z = geo.compose_height(wx, wz, seed=91, scenario=scenario)["height"]
        rows.append(metrics(f"SYN {scenario.label}", "synth", z, spacing_m, span_m))
    return rows


def synth_rows_skeleton_rough(
    span_m: float,
    n: int,
    spacing_m: float,
    coarse_n: int = 176,
) -> list[dict[str, float | str]]:
    wx, wz = geo.grid(n, span_m, ox=84000.0 if span_m < 70000 else 60000.0, oz=62000.0 if span_m < 70000 else 36000.0)
    rows = []
    for scenario in ROUGH_FOCUS:
        result = skel.compose_height(wx, wz, seed=133, scenario=scenario, coarse_n=coarse_n)
        row = metrics(f"SYN {scenario.label}", "skeleton_rough", result["height"], spacing_m, span_m)
        row["source"] = scenario.key
        add_regime_metrics(row, result["weights"])
        rows.append(row)
    return rows


def reference_rows(span_m: float) -> list[dict[str, float | str]]:
    rows = []
    for kernel_id, label in REFERENCES:
        z, spacing, actual_span = load_reference(kernel_id, span_m)
        rows.append(metrics(label, "reference", z, spacing, actual_span))
    return rows


def write_reports(rows: list[dict[str, float | str]], suffix: str) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    csv_path = _writable_report_path(OUT / f"geography_metrics_{suffix}.csv")
    md_path = csv_path.with_suffix(".md")
    keys: list[str] = []
    for row in rows:
        for key in row:
            if key not in keys:
                keys.append(key)
    with csv_path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=keys)
        writer.writeheader()
        writer.writerows(rows)
    lines = ["| " + " | ".join(keys) + " |", "| " + " | ".join(["---"] * len(keys)) + " |"]
    for row in rows:
        lines.append("| " + " | ".join(str(row.get(k, "")) for k in keys) + " |")
    md_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {csv_path}")
    print(f"wrote {md_path}")


def _writable_report_path(path: Path) -> Path:
    for i in range(8):
        candidate = path if i == 0 else path.with_name(f"{path.stem}_{i}{path.suffix}")
        try:
            with candidate.open("a", encoding="utf-8"):
                pass
            return candidate
        except PermissionError:
            continue
    raise PermissionError(f"could not find writable report path near {path}")


def render_metrics_v5(span_m: float, n: int, suffix: str) -> None:
    spacing = float(span_m) / float(n - 1)
    rows = reference_rows(span_m) + synth_rows_v5(span_m, n, spacing)
    write_reports(rows, suffix)


def render_metrics_skeleton_rough(span_m: float, n: int, suffix: str) -> None:
    spacing = float(span_m) / float(n - 1)
    rows = reference_rows(span_m) + synth_rows_skeleton_rough(span_m, n, spacing)
    write_reports(rows, f"skeleton_rough_{suffix}")


def main() -> None:
    render_metrics_v5(200000.0, 384, "200km")
    render_metrics_v5(45000.0, 512, "45km_close")
    render_metrics_skeleton_rough(200000.0, 384, "200km")
    render_metrics_skeleton_rough(45000.0, 512, "45km_close")


if __name__ == "__main__":
    main()
