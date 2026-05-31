"""Export a wider 5x5 rough-highlands travel review payload for Godot.

This reuses the accepted rough_highlands_keeper_v1 chunk/window contract but
renders a wider 128 km review area for terrain/travel pacing. It is still a
static offline review artifact, not runtime streaming.

Run:
    python tools/dem_pack/export_godot_rough_world_travel_review.py
"""

from __future__ import annotations

import csv
import json
from pathlib import Path

import export_godot_rough_world_chunks as chunks
import render_rough_world_chunks_review as chunk_render


OUT = Path("wg-10/worldgen_terrain/generated/review/rough_world_chunks_travel_5x5.json")
REPORT_DIR = Path("D:/tmp/wg10_geography_engine")
REPORT_CSV = REPORT_DIR / "rough_world_chunks_travel_5x5.csv"
REPORT_MD = REPORT_DIR / "rough_world_chunks_travel_5x5.md"
CONTACT_SHEET = REPORT_DIR / "rough_world_chunks_travel_5x5_contact.png"

TRAVEL_CHUNK_COUNT = 5
TRAVEL_CHUNK_N = 65
TRAVEL_ORIGIN_X_M = chunks.WORLD_ORIGIN_X_M - chunks.CHUNK_SPAN_M
TRAVEL_ORIGIN_Z_M = chunks.WORLD_ORIGIN_Z_M - chunks.CHUNK_SPAN_M


def build_payload() -> dict[str, object]:
    payload = chunks.build_payload(
        seeds=chunks.SEEDS,
        chunk_count=TRAVEL_CHUNK_COUNT,
        chunk_n=TRAVEL_CHUNK_N,
        chunk_span_m=chunks.CHUNK_SPAN_M,
        origin_x_m=TRAVEL_ORIGIN_X_M,
        origin_z_m=TRAVEL_ORIGIN_Z_M,
    )
    payload["title"] = "WorldGen10 rough-highlands 5x5 travel review"
    payload["review_intent"] = "terrain_travel_pacing_not_runtime_streaming"
    payload["review_resolution_note"] = "5x5 travel scene uses 65x65 vertices per 25.6 km chunk for a wider 128 km flyable read."
    return payload


def summary_rows(payload: dict[str, object]) -> list[dict[str, object]]:
    seam_rows = chunks.seam_rows(payload)
    visual_rows = chunks.visual_seam_rows(payload)
    variation_rows = chunks.adjacent_pair_variation_rows(payload)
    return [
        {
            "kind": "summary",
            "chunk_count": int(payload["chunk_count"]),
            "chunk_n": int(payload["chunk_n"]),
            "world_span_km": float(payload["world_span_m"]) / 1000.0,
            "seam_rows": len(seam_rows),
            "height_max_abs_delta": max(float(row["height_max_abs_delta"]) for row in seam_rows),
            "corridor_min_match_frac": min(float(row["corridor_match_frac"]) for row in seam_rows),
            "normal_max_angle_deg": max(float(row["normal_max_angle_deg"]) for row in visual_rows),
            "corridor_edge_mismatch_count": max(int(row["corridor_edge_mismatch_count"]) for row in visual_rows),
            "adjacent_pair_count": len(variation_rows),
            "adjacent_mean_abs_delta_median": sorted(float(row["mean_abs_delta"]) for row in variation_rows)[len(variation_rows) // 2],
            "adjacent_corrcoef_max": max(float(row["corrcoef"]) for row in variation_rows),
        }
    ]


def write_report(payload: dict[str, object], rows: list[dict[str, object]]) -> None:
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    with REPORT_CSV.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(rows)
    row = rows[0]
    lines = [
        "# Rough-World 5x5 Travel Review",
        "",
        f"Generator: `{payload['generator_version']}`; scenario: `{payload['scenario_key']}`.",
        f"Review area: {row['chunk_count']}x{row['chunk_count']} chunks, {float(row['world_span_km']):.1f} km wide.",
        "This is an offline static Godot review payload. It supports terrain/travel judgement, not runtime streaming/cache acceptance.",
        "",
        "| chunks | chunk_n | seams | height max | corridor min | normal max deg | corridor mismatches | adjacent pairs | adjacent median delta | adjacent max corr |",
        "|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
        (
            f"| {row['chunk_count']}x{row['chunk_count']} | {row['chunk_n']} | {row['seam_rows']} | "
            f"{float(row['height_max_abs_delta']):.6f} | {float(row['corridor_min_match_frac']):.3f} | "
            f"{float(row['normal_max_angle_deg']):.4f} | {row['corridor_edge_mismatch_count']} | "
            f"{row['adjacent_pair_count']} | {float(row['adjacent_mean_abs_delta_median']):.4f} | "
            f"{float(row['adjacent_corrcoef_max']):.4f} |"
        ),
        "",
        f"Contact sheet: `{CONTACT_SHEET}`",
        "Godot scene: `wg-10/worldgen_terrain/harness/rough_world_travel_review.tscn`",
    ]
    REPORT_MD.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    payload = build_payload()
    OUT.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    rows = summary_rows(payload)
    write_report(payload, rows)
    chunk_render.render(payload_path=OUT, out_path=CONTACT_SHEET, panel_px=300)
    print(f"wrote {OUT}")
    print(f"wrote {REPORT_CSV}")
    print(f"wrote {REPORT_MD}")
    print(f"wrote {CONTACT_SHEET}")
    print(f"height max={float(rows[0]['height_max_abs_delta']):.6f} corridor min={float(rows[0]['corridor_min_match_frac']):.3f}")


if __name__ == "__main__":
    main()
