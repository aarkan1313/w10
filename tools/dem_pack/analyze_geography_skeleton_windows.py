r"""Write non-visual seam reports for the Phase 7B-lite skeleton window spike.

Run:
    python tools/dem_pack/analyze_geography_skeleton_windows.py

Writes:
    D:\tmp\wg10_geography_engine\geography_skeleton_window_seams.{csv,md}
"""

from __future__ import annotations

import csv
from pathlib import Path

import geography_skeleton_windows as win


OUT = Path(r"D:\tmp\wg10_geography_engine")


def seam_rows(
    spec: win.SkeletonWindowSpec = win.SkeletonWindowSpec(),
    seeds: tuple[int, ...] = (211, 213, 217),
    origins: tuple[tuple[float, float], ...] = ((0.0, 0.0), (90000.0, -90000.0), (-180000.0, 90000.0)),
) -> list[dict[str, float | int | str]]:
    rows: list[dict[str, float | int | str]] = []
    for seed in seeds:
        for origin_x, origin_z in origins:
            for axis in ("x", "z"):
                deltas = win.adjacent_seam_deltas(seed=seed, spec=spec, origin_x=origin_x, origin_z=origin_z, axis=axis)
                row: dict[str, float | int | str] = {
                    "seed": seed,
                    "origin_x": origin_x,
                    "origin_z": origin_z,
                    "axis": axis,
                }
                for field, value in deltas.items():
                    row[field] = round(float(value), 6)
                row["crest_dist_core_frac"] = round(float(deltas["crest_dist"]) / float(spec.core_span_m), 6)
                row["channel_dist_core_frac"] = round(float(deltas["channel_dist"]) / float(spec.core_span_m), 6)
                rows.append(row)
    return rows


def write_reports(rows: list[dict[str, float | int | str]]) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    csv_path = OUT / "geography_skeleton_window_seams.csv"
    md_path = OUT / "geography_skeleton_window_seams.md"
    keys = list(rows[0].keys())
    with csv_path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=keys)
        writer.writeheader()
        writer.writerows(rows)
    lines = ["| " + " | ".join(keys) + " |", "| " + " | ".join(["---"] * len(keys)) + " |"]
    for row in rows:
        lines.append("| " + " | ".join(str(row[key]) for key in keys) + " |")
    md_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {csv_path}")
    print(f"wrote {md_path}")


def main() -> None:
    write_reports(seam_rows())


if __name__ == "__main__":
    main()
