r"""Audit Slice 2B candidate metrics across the approved WG9 kernel set.

Metrics are diagnostics, not visual acceptance. This script answers the non-visual
roadmap question: which metrics actually vary across families enough to deserve
schema/knob responsibility?

Run:
    python tools/dem_pack/analyze_geography_metric_schema.py

Writes:
    D:\tmp\wg10_geography_engine\geography_metric_schema_audit_kernels.csv
    D:\tmp\wg10_geography_engine\geography_metric_schema_audit_families.{csv,md}
"""

from __future__ import annotations

import csv
from pathlib import Path

import numpy as np

import biome_distill as bd
import compare_geography_metrics as gm
import distill_biomes as distill


OUT = Path(r"D:\tmp\wg10_geography_engine")
AUDIT_METRICS = (
    "anisotropy",
    "ridge_linearity",
    "dominant_wavelength_m",
    "incision_ratio",
    "hypsometric_integral",
    "relief_2km",
    "relief_10km",
    "relief_ratio_2_10",
    "slope_mean",
    "slope_p95",
    "slope_skew",
    "vrm_7px",
    "curv_balance",
    "curv_abs_mean",
    "ridge_spacing_m",
    "valley_spacing_m",
    "highpass_std",
    "straight_score",
)


def _percentile(values: list[float], p: float) -> float:
    if not values:
        return 0.0
    return float(np.percentile(np.asarray(values, dtype=np.float64), p))


def summarize_metric_rows(rows: list[dict[str, float | str]], metrics: tuple[str, ...] = AUDIT_METRICS) -> list[dict[str, float | str]]:
    families = sorted({str(row["family"]) for row in rows})
    out: list[dict[str, float | str]] = []
    for family in families:
        fam_rows = [row for row in rows if row["family"] == family]
        summary: dict[str, float | str] = {"family": family, "count": len(fam_rows)}
        for metric in metrics:
            values = [float(row[metric]) for row in fam_rows if metric in row and np.isfinite(float(row[metric]))]
            if not values:
                continue
            p25 = _percentile(values, 25.0)
            p75 = _percentile(values, 75.0)
            summary[f"{metric}_median"] = round(_percentile(values, 50.0), 6)
            summary[f"{metric}_iqr"] = round(p75 - p25, 6)
            summary[f"{metric}_min"] = round(min(values), 6)
            summary[f"{metric}_max"] = round(max(values), 6)
        out.append(summary)
    return out


def audit_kernel_rows() -> list[dict[str, float | str]]:
    rows: list[dict[str, float | str]] = []
    fam_of = distill.load_family_map()
    for kernel_id, family in sorted(fam_of.items(), key=lambda item: (item[1], item[0])):
        z, meta = distill.load_kernel(kernel_id)
        if max(abs(float(z.min())), abs(float(z.max()))) > distill.MAX_ABS_ZSCORE:
            continue
        spacing = float(meta["approx_sample_spacing_m"])
        shape = gm.metrics(kernel_id, "reference", z, spacing, z.shape[0] * spacing)
        distilled = bd.metrics_for_dem(z, meta)
        relief = max(float(distilled["relief_real_m"]), 1.0)
        row: dict[str, float | str] = {
            "family": family,
            "kernel_id": kernel_id,
            "relief_real_m": round(float(distilled["relief_real_m"]), 6),
            "slope_bias_deg": round(float(distilled["slope_bias_deg"]), 6),
            "anisotropy": round(float(distilled["anisotropy"]), 6),
            "ridge_linearity": round(float(distilled["ridge_linearity"]), 6),
            "dominant_wavelength_m": round(float(distilled["dominant_wavelength_m"]), 6),
            "incision_depth_m": round(float(distilled["incision_depth_m"]), 6),
            "incision_ratio": round(float(distilled["incision_depth_m"]) / relief, 6),
        }
        for key in AUDIT_METRICS:
            if key in shape and key not in row:
                row[key] = shape[key]
        rows.append(row)
    return rows


def _keys(rows: list[dict[str, float | str]]) -> list[str]:
    keys: list[str] = []
    for row in rows:
        for key in row:
            if key not in keys:
                keys.append(key)
    return keys


def _write_csv(path: Path, rows: list[dict[str, float | str]]) -> None:
    keys = _keys(rows)
    with path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=keys)
        writer.writeheader()
        writer.writerows(rows)


def _write_md(path: Path, rows: list[dict[str, float | str]]) -> None:
    keys = _keys(rows)
    lines = ["| " + " | ".join(keys) + " |", "| " + " | ".join(["---"] * len(keys)) + " |"]
    for row in rows:
        lines.append("| " + " | ".join(str(row.get(key, "")) for key in keys) + " |")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _family_medians(family_rows: list[dict[str, float | str]], metric: str) -> list[float]:
    key = f"{metric}_median"
    return [float(row[key]) for row in family_rows if key in row and np.isfinite(float(row[key]))]


def audit_summary_lines(family_rows: list[dict[str, float | str]]) -> list[str]:
    lines = [
        "# Geography Metric Schema Audit",
        "",
        "Generated from approved WG9 kernels. Metrics are diagnostics only; owner image review still decides look.",
        "",
    ]
    for metric in (
        "anisotropy",
        "ridge_linearity",
        "dominant_wavelength_m",
        "incision_ratio",
        "hypsometric_integral",
        "relief_ratio_2_10",
        "vrm_7px",
        "ridge_spacing_m",
        "valley_spacing_m",
        "straight_score",
    ):
        medians = _family_medians(family_rows, metric)
        if not medians:
            continue
        spread = max(medians) - min(medians)
        lines.append(
            f"- `{metric}` family medians: min={min(medians):.6g}, max={max(medians):.6g}, range={spread:.6g}."
        )

    anisotropy = _family_medians(family_rows, "anisotropy")
    if anisotropy:
        spread = max(anisotropy) - min(anisotropy)
        if spread < 0.20:
            lines.append("- Decision: `anisotropy` is too clustered to drive a primary schema knob by itself.")
        else:
            lines.append(
                "- Decision: `anisotropy` is not dead, but overlap remains; keep it as a secondary/context "
                "metric rather than the sole `warp_amount` driver."
            )
    vrm = _family_medians(family_rows, "vrm_7px")
    if vrm and max(vrm) < 1e-5:
        lines.append("- Decision: current `vrm_7px` implementation is effectively dead at this normalization/scale.")
    lines.append("")
    return lines


def write_reports(kernel_rows: list[dict[str, float | str]], family_rows: list[dict[str, float | str]]) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    _write_csv(OUT / "geography_metric_schema_audit_kernels.csv", kernel_rows)
    family_csv = OUT / "geography_metric_schema_audit_families.csv"
    family_md = OUT / "geography_metric_schema_audit_families.md"
    summary_md = OUT / "geography_metric_schema_audit_summary.md"
    _write_csv(family_csv, family_rows)
    _write_md(family_md, family_rows)
    summary_md.write_text("\n".join(audit_summary_lines(family_rows)) + "\n", encoding="utf-8")
    print(f"wrote {OUT / 'geography_metric_schema_audit_kernels.csv'}")
    print(f"wrote {family_csv}")
    print(f"wrote {family_md}")
    print(f"wrote {summary_md}")


def main() -> None:
    kernel_rows = audit_kernel_rows()
    family_rows = summarize_metric_rows(kernel_rows)
    write_reports(kernel_rows, family_rows)


if __name__ == "__main__":
    main()
