r"""Export a bounded adjacent-chunk rough-highlands world for Godot review.

This is a render-first proof artifact, not a Rust/GLSL runtime port. It builds
one authoritative world-coordinate 3x3 super-window, conditions it once, then
splits it into adjacent 25.6 km chunks with one-sample aprons for seam-stable
normals. The bounded proof demonstrates that adjacent chunks can be different
parts of the same seeded world while sharing exact border samples.

Run:
    python tools/dem_pack/export_godot_rough_world_chunks.py

Writes:
    wg-10/worldgen_terrain/generated/review/rough_world_chunks_3x3.json
    D:/tmp/wg10_geography_engine/rough_world_chunks_3x3_seams.{csv,md}
"""

from __future__ import annotations

import csv
import json
from pathlib import Path
from typing import Iterable

import numpy as np

import analyze_rough_world_traversability as trav
import geography_skeleton as skel
from export_godot_rough_world_review import _condition
from render_geography_skeleton_focus import FOCUS


OUT = Path("wg-10/worldgen_terrain/generated/review/rough_world_chunks_3x3.json")
REPORT_DIR = Path("D:/tmp/wg10_geography_engine")
REPORT_CSV = REPORT_DIR / "rough_world_chunks_3x3_seams.csv"
REPORT_MD = REPORT_DIR / "rough_world_chunks_3x3_seams.md"

GENERATOR_VERSION = "rough_world_chunks_v1_superwindow"
CHUNK_COUNT = 3
CHUNK_N = 129
CHUNK_SPAN_M = 25_600.0
WORLD_ORIGIN_X_M = 60_000.0
WORLD_ORIGIN_Z_M = 36_000.0
SEEDS = (133, 211)
SCENARIO = next(scenario for scenario in FOCUS if scenario.key == "rough_anchor")


def _world_grid(origin_x_m: float, origin_z_m: float, chunk_count: int, chunk_n: int, chunk_span_m: float) -> tuple[np.ndarray, np.ndarray]:
    super_n = int(chunk_count) * (int(chunk_n) - 1) + 1
    spacing = float(chunk_span_m) / float(int(chunk_n) - 1)
    xs = float(origin_x_m) + np.arange(super_n, dtype=np.float64) * spacing
    zs = float(origin_z_m) + np.arange(super_n, dtype=np.float64) * spacing
    return np.meshgrid(xs, zs)


def _build_conditioned_world(
    seed: int,
    *,
    chunk_count: int = CHUNK_COUNT,
    chunk_n: int = CHUNK_N,
    chunk_span_m: float = CHUNK_SPAN_M,
    origin_x_m: float = WORLD_ORIGIN_X_M,
    origin_z_m: float = WORLD_ORIGIN_Z_M,
    coarse_n: int = 176,
) -> tuple[np.ndarray, dict[str, float]]:
    wx, wz = _world_grid(origin_x_m, origin_z_m, chunk_count, chunk_n, chunk_span_m)
    result = skel.compose_height(wx, wz, seed=int(seed), scenario=SCENARIO, coarse_n=coarse_n)
    return _condition(np.asarray(result["height"], dtype=np.float64))


def _chunk_from_world(
    conditioned: np.ndarray,
    *,
    chunk_x: int,
    chunk_z: int,
    chunk_count: int,
    chunk_n: int,
    chunk_span_m: float,
    world_origin_x_m: float,
    world_origin_z_m: float,
) -> dict[str, object]:
    step = int(chunk_n) - 1
    x0 = int(chunk_x) * step
    z0 = int(chunk_z) * step
    core = conditioned[z0 : z0 + chunk_n, x0 : x0 + chunk_n]
    padded = np.pad(conditioned, 1, mode="edge")
    apron = padded[z0 : z0 + chunk_n + 2, x0 : x0 + chunk_n + 2]
    display_origin_x = (float(chunk_x) - float(chunk_count) * 0.5) * float(chunk_span_m)
    display_origin_z = (float(chunk_z) - float(chunk_count) * 0.5) * float(chunk_span_m)
    return {
        "chunk_x": int(chunk_x),
        "chunk_z": int(chunk_z),
        "key": f"{chunk_x}_{chunk_z}",
        "label": f"chunk {chunk_x},{chunk_z}",
        "n": int(chunk_n),
        "apron_n": int(chunk_n) + 2,
        "span_m": float(chunk_span_m),
        "world_origin_x_m": float(world_origin_x_m) + float(chunk_x) * float(chunk_span_m),
        "world_origin_z_m": float(world_origin_z_m) + float(chunk_z) * float(chunk_span_m),
        "display_origin_x_m": display_origin_x,
        "display_origin_z_m": display_origin_z,
        "height": np.round(core, 4).astype(float).ravel().tolist(),
        "apron_height": np.round(apron, 4).astype(float).ravel().tolist(),
    }


def build_seed_world(
    seed: int,
    *,
    chunk_count: int = CHUNK_COUNT,
    chunk_n: int = CHUNK_N,
    chunk_span_m: float = CHUNK_SPAN_M,
    origin_x_m: float = WORLD_ORIGIN_X_M,
    origin_z_m: float = WORLD_ORIGIN_Z_M,
    coarse_n: int = 176,
) -> dict[str, object]:
    conditioned, stats = _build_conditioned_world(
        seed,
        chunk_count=chunk_count,
        chunk_n=chunk_n,
        chunk_span_m=chunk_span_m,
        origin_x_m=origin_x_m,
        origin_z_m=origin_z_m,
        coarse_n=coarse_n,
    )
    chunks = [
        _chunk_from_world(
            conditioned,
            chunk_x=x,
            chunk_z=z,
            chunk_count=chunk_count,
            chunk_n=chunk_n,
            chunk_span_m=chunk_span_m,
            world_origin_x_m=origin_x_m,
            world_origin_z_m=origin_z_m,
        )
        for z in range(chunk_count)
        for x in range(chunk_count)
    ]
    return {
        "seed": int(seed),
        "label": f"seed {seed}",
        "height": np.round(conditioned, 4).astype(float).ravel().tolist(),
        "world_n": int(conditioned.shape[0]),
        "stats": stats,
        "chunks": chunks,
    }


def build_payload(
    *,
    seeds: Iterable[int] = SEEDS,
    chunk_count: int = CHUNK_COUNT,
    chunk_n: int = CHUNK_N,
    chunk_span_m: float = CHUNK_SPAN_M,
    origin_x_m: float = WORLD_ORIGIN_X_M,
    origin_z_m: float = WORLD_ORIGIN_Z_M,
    coarse_n: int = 176,
) -> dict[str, object]:
    seed_worlds = [
        build_seed_world(
            int(seed),
            chunk_count=chunk_count,
            chunk_n=chunk_n,
            chunk_span_m=chunk_span_m,
            origin_x_m=origin_x_m,
            origin_z_m=origin_z_m,
            coarse_n=coarse_n,
        )
        for seed in seeds
    ]
    return {
        "title": "WorldGen10 rough-highlands adjacent-chunk review",
        "generator_version": GENERATOR_VERSION,
        "scenario_key": SCENARIO.key,
        "scenario_label": SCENARIO.label,
        "chunk_count": int(chunk_count),
        "chunk_n": int(chunk_n),
        "chunk_span_m": float(chunk_span_m),
        "world_span_m": float(chunk_count) * float(chunk_span_m),
        "world_origin_x_m": float(origin_x_m),
        "world_origin_z_m": float(origin_z_m),
        "seeds": seed_worlds,
    }


def _chunk_grid(seed_world: dict[str, object], chunk_count: int) -> list[list[dict[str, object]]]:
    grid: list[list[dict[str, object]]] = [[{} for _ in range(chunk_count)] for _ in range(chunk_count)]
    for chunk in seed_world["chunks"]:
        c = dict(chunk)
        grid[int(c["chunk_z"])][int(c["chunk_x"])] = c
    return grid


def _height_array(chunk: dict[str, object]) -> np.ndarray:
    n = int(chunk["n"])
    return np.asarray(chunk["height"], dtype=np.float64).reshape((n, n))


def _corridor_mask(world_height: np.ndarray, world_span_m: float) -> np.ndarray:
    slopes = trav.slope_grid(world_height, scene_width_m=float(world_span_m), height_scale_m=trav.BASE_HEIGHT_SCALE_M)
    return (slopes <= trav.PASSABLE_SLOPE) & (world_height <= np.percentile(world_height, 55.0))


def _edge_match_count(source_edge: np.ndarray, target_band: np.ndarray, row_tolerance: int = 2) -> int:
    matches = 0
    for row, enters in enumerate(np.asarray(source_edge, dtype=bool)):
        if not enters:
            continue
        lo = max(0, row - row_tolerance)
        hi = min(target_band.shape[0], row + row_tolerance + 1)
        if bool(np.any(target_band[lo:hi, :])):
            matches += 1
    return matches


def seam_rows(payload: dict[str, object]) -> list[dict[str, object]]:
    chunk_count = int(payload["chunk_count"])
    chunk_n = int(payload["chunk_n"])
    step = chunk_n - 1
    rows: list[dict[str, object]] = []
    for seed_world in payload["seeds"]:
        seed = int(seed_world["seed"])
        grid = _chunk_grid(seed_world, chunk_count)
        world_n = int(seed_world["world_n"])
        world_height = np.asarray(seed_world["height"], dtype=np.float64).reshape((world_n, world_n))
        corridors = _corridor_mask(world_height, float(payload["world_span_m"]))
        for z in range(chunk_count):
            for x in range(chunk_count - 1):
                left = _height_array(grid[z][x])
                right = _height_array(grid[z][x + 1])
                boundary = (x + 1) * step
                edge = corridors[z * step : z * step + chunk_n, boundary]
                target = corridors[z * step : z * step + chunk_n, boundary + 1 : boundary + 4]
                matched = _edge_match_count(edge, target)
                entering = int(np.count_nonzero(edge))
                rows.append({
                    "seed": seed,
                    "axis": "x",
                    "a": f"{x},{z}",
                    "b": f"{x + 1},{z}",
                    "height_max_abs_delta": float(np.max(np.abs(left[:, -1] - right[:, 0]))),
                    "corridor_entering_count": entering,
                    "corridor_matched_count": matched,
                    "corridor_match_frac": float(matched / entering) if entering else 1.0,
                })
        for z in range(chunk_count - 1):
            for x in range(chunk_count):
                top = _height_array(grid[z][x])
                bottom = _height_array(grid[z + 1][x])
                boundary = (z + 1) * step
                edge = corridors[boundary, x * step : x * step + chunk_n]
                target = corridors[boundary + 1 : boundary + 4, x * step : x * step + chunk_n].T
                matched = _edge_match_count(edge, target)
                entering = int(np.count_nonzero(edge))
                rows.append({
                    "seed": seed,
                    "axis": "z",
                    "a": f"{x},{z}",
                    "b": f"{x},{z + 1}",
                    "height_max_abs_delta": float(np.max(np.abs(top[-1, :] - bottom[0, :]))),
                    "corridor_entering_count": entering,
                    "corridor_matched_count": matched,
                    "corridor_match_frac": float(matched / entering) if entering else 1.0,
                })
    return rows


def variation_rows(payload: dict[str, object]) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    for seed_world in payload["seeds"]:
        seed = int(seed_world["seed"])
        grid = _chunk_grid(seed_world, int(payload["chunk_count"]))
        center = _height_array(grid[1][1])
        east = _height_array(grid[1][2])
        rows.append({
            "kind": "adjacent_chunk",
            "seed": seed,
            "a": "1,1",
            "b": "2,1",
            "mean_abs_delta": float(np.mean(np.abs(center - east))),
            "corrcoef": float(np.corrcoef(center.ravel(), east.ravel())[0, 1]),
        })
    if len(payload["seeds"]) >= 2:
        first = _chunk_grid(payload["seeds"][0], int(payload["chunk_count"]))[1][1]
        second = _chunk_grid(payload["seeds"][1], int(payload["chunk_count"]))[1][1]
        a = _height_array(first)
        b = _height_array(second)
        rows.append({
            "kind": "seed_pair",
            "seed": f"{payload['seeds'][0]['seed']}->{payload['seeds'][1]['seed']}",
            "a": "center",
            "b": "center",
            "mean_abs_delta": float(np.mean(np.abs(a - b))),
            "corrcoef": float(np.corrcoef(a.ravel(), b.ravel())[0, 1]),
        })
    return rows


def write_reports(payload: dict[str, object], rows: list[dict[str, object]], variation: list[dict[str, object]]) -> None:
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    csv_rows = rows + variation
    keys = sorted({key for row in csv_rows for key in row.keys()})
    with REPORT_CSV.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=keys)
        writer.writeheader()
        writer.writerows(csv_rows)
    lines = [
        "# Rough-World 3x3 Chunk Seam Report",
        "",
        f"Generator: `{payload['generator_version']}`; scenario: `{payload['scenario_key']}`; chunk span: {float(payload['chunk_span_m'])/1000.0:.1f} km.",
        "This is a bounded review export: one authoritative world-coordinate super-window split into chunks, not a runtime streaming port.",
        "",
        "## Seams",
        "",
        "| seed | axis | a | b | height max abs delta | corridor entering | corridor matched | match frac |",
        "|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for row in rows:
        lines.append(
            f"| {row['seed']} | {row['axis']} | {row['a']} | {row['b']} | "
            f"{float(row['height_max_abs_delta']):.6f} | {row['corridor_entering_count']} | "
            f"{row['corridor_matched_count']} | {float(row['corridor_match_frac']):.3f} |"
        )
    lines += [
        "",
        "## Variation",
        "",
        "| kind | seed | a | b | mean abs delta | corrcoef |",
        "|---|---:|---:|---:|---:|---:|",
    ]
    for row in variation:
        lines.append(
            f"| {row['kind']} | {row['seed']} | {row['a']} | {row['b']} | "
            f"{float(row['mean_abs_delta']):.4f} | {float(row['corrcoef']):.4f} |"
        )
    REPORT_MD.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    payload = build_payload()
    OUT.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    rows = seam_rows(payload)
    variation = variation_rows(payload)
    write_reports(payload, rows, variation)
    max_seam = max(float(row["height_max_abs_delta"]) for row in rows)
    min_corridor = min(float(row["corridor_match_frac"]) for row in rows)
    print(f"wrote {OUT}")
    print(f"wrote {REPORT_CSV}")
    print(f"wrote {REPORT_MD}")
    print(f"max seam delta={max_seam:.6f} min corridor match={min_corridor:.3f}")


if __name__ == "__main__":
    main()
