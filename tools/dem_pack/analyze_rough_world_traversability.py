"""Audit rough-world review scale/traversability.

Reads the generated Godot review JSON and evaluates the same conditioned height
grid the review scene displays. This is a review heuristic, not gameplay navmesh
truth: thresholds are slope bands over the conditioned scene mesh, meant to
compare scale choices and variants before a runtime port.

Run:
    python tools/dem_pack/analyze_rough_world_traversability.py

Writes:
    D:/tmp/wg10_geography_engine/rough_world_traversability_scale.csv
    D:/tmp/wg10_geography_engine/rough_world_traversability_scale.md
"""

from __future__ import annotations

import csv
import json
from collections import deque
from pathlib import Path
from typing import Iterable

import numpy as np


DATA_PATH = Path("wg-10/worldgen_terrain/generated/review/rough_world_3d.json")
OUT_DIR = Path("D:/tmp/wg10_geography_engine")
OUT_CSV = OUT_DIR / "rough_world_traversability_scale.csv"
OUT_MD = OUT_DIR / "rough_world_traversability_scale.md"

BASE_WORLD_SIZE_M = 128.0
BASE_HEIGHT_SCALE_M = 260.0
SCALE_PRESETS = (10.0, 25.0, 50.0, 100.0, 150.0, 200.0)

# Slope = rise/run. These are review bands, not engine constants.
EASY_SLOPE = 0.12
PASSABLE_SLOPE = 0.28
STEEP_SLOPE = 0.45
CLIFF_SLOPE = 0.80


def height_grid(item: dict) -> np.ndarray:
    n = int(item["n"])
    h = np.asarray(item["height"], dtype=np.float64)
    if h.size != n * n:
        raise ValueError(f"{item.get('key', '?')}: height length {h.size} != n*n {n*n}")
    return h.reshape((n, n))


def slope_grid(height: np.ndarray, scene_width_m: float, height_scale_m: float = BASE_HEIGHT_SCALE_M) -> np.ndarray:
    """Return slope magnitude over the displayed scene mesh."""
    if height.ndim != 2 or height.shape[0] < 2 or height.shape[1] < 2:
        raise ValueError("height must be a 2D grid with at least 2 samples per axis")
    cell_m = scene_width_m / float(height.shape[0] - 1)
    y = np.asarray(height, dtype=np.float64) * float(height_scale_m)
    dz, dx = np.gradient(y, cell_m, cell_m, edge_order=1)
    return np.sqrt(dx * dx + dz * dz)


def component_stats(mask: np.ndarray) -> dict[str, float | int | bool]:
    """4-neighbor connected-component summary for a boolean passability mask."""
    passable = np.asarray(mask, dtype=bool)
    h, w = passable.shape
    seen = np.zeros_like(passable, dtype=bool)
    components = 0
    largest = 0
    largest_edges = (False, False, False, False)  # west, east, north, south
    total = int(passable.size)

    for z in range(h):
        for x in range(w):
            if not passable[z, x] or seen[z, x]:
                continue
            components += 1
            q: deque[tuple[int, int]] = deque([(z, x)])
            seen[z, x] = True
            size = 0
            west = east = north = south = False
            while q:
                cz, cx = q.popleft()
                size += 1
                west = west or cx == 0
                east = east or cx == w - 1
                north = north or cz == 0
                south = south or cz == h - 1
                for nz, nx in ((cz - 1, cx), (cz + 1, cx), (cz, cx - 1), (cz, cx + 1)):
                    if 0 <= nz < h and 0 <= nx < w and passable[nz, nx] and not seen[nz, nx]:
                        seen[nz, nx] = True
                        q.append((nz, nx))
            if size > largest:
                largest = size
                largest_edges = (west, east, north, south)

    west, east, north, south = largest_edges
    return {
        "component_count": components,
        "largest_frac": largest / float(total) if total else 0.0,
        "largest_crosses_we": bool(west and east),
        "largest_crosses_ns": bool(north and south),
        "largest_touches_edges": int(west) + int(east) + int(north) + int(south),
    }


def _grade(passable_frac: float, largest_frac: float, crosses_we: bool, crosses_ns: bool) -> str:
    crosses = crosses_we or crosses_ns
    if passable_frac >= 0.45 and largest_frac >= 0.35 and crosses:
        return "candidate"
    if passable_frac >= 0.25 and largest_frac >= 0.18:
        return "thin"
    return "blocked"


def audit_item(item: dict, scale: float, relief: float = 1.0) -> dict[str, object]:
    h = height_grid(item)
    scene_width_m = BASE_WORLD_SIZE_M * float(scale)
    scene_km = scene_width_m / 1000.0
    source_km = float(item.get("span_km", 0.0))
    slopes = slope_grid(h, scene_width_m, BASE_HEIGHT_SCALE_M * relief)
    passable = slopes <= PASSABLE_SLOPE
    easy = slopes <= EASY_SLOPE
    steep = slopes > STEEP_SLOPE
    cliff = slopes > CLIFF_SLOPE
    low_corridor = passable & (h <= np.percentile(h, 55.0))
    comps = component_stats(passable)
    low_comps = component_stats(low_corridor)
    passable_frac = float(np.mean(passable))
    largest_frac = float(comps["largest_frac"])
    return {
        "key": item.get("key", ""),
        "label": item.get("label", ""),
        "kind": item.get("kind", ""),
        "scale_x": scale,
        "source_km": source_km,
        "scene_km": scene_km,
        "source_scene_ratio": source_km / max(scene_km, 1e-9),
        "cell_m": scene_width_m / float(h.shape[0] - 1),
        "slope_mean": float(np.mean(slopes)),
        "slope_p50": float(np.percentile(slopes, 50.0)),
        "slope_p90": float(np.percentile(slopes, 90.0)),
        "slope_p95": float(np.percentile(slopes, 95.0)),
        "easy_frac": float(np.mean(easy)),
        "passable_frac": passable_frac,
        "steep_frac": float(np.mean(steep)),
        "cliff_frac": float(np.mean(cliff)),
        "pass_components": int(comps["component_count"]),
        "largest_passable_frac": largest_frac,
        "largest_crosses_we": bool(comps["largest_crosses_we"]),
        "largest_crosses_ns": bool(comps["largest_crosses_ns"]),
        "largest_touches_edges": int(comps["largest_touches_edges"]),
        "low_corridor_frac": float(np.mean(low_corridor)),
        "largest_low_corridor_frac": float(low_comps["largest_frac"]),
        "grade": _grade(passable_frac, largest_frac, bool(comps["largest_crosses_we"]), bool(comps["largest_crosses_ns"])),
    }


def audit_payload(payload: dict, scales: Iterable[float] = SCALE_PRESETS) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    for item in payload.get("items", []):
        for scale in scales:
            rows.append(audit_item(item, float(scale)))
    return rows


def _fmt_pct(value: object) -> str:
    return f"{float(value) * 100.0:.1f}%"


def write_csv(rows: list[dict[str, object]], path: Path = OUT_CSV) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if not rows:
        raise ValueError("no rows to write")
    with path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(rows)


def write_markdown(rows: list[dict[str, object]], path: Path = OUT_MD) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    synth = [r for r in rows if r["kind"] == "synth"]
    at_default = [r for r in synth if abs(float(r["scale_x"]) - 200.0) < 1e-6]
    at_default.sort(key=lambda r: (str(r["grade"]) != "candidate", -float(r["largest_passable_frac"]), -float(r["passable_frac"])))

    lines = [
        "# Rough-World Traversability / Scale Audit",
        "",
        "Review heuristic over `rough_world_3d.json`, using the same conditioned height grid displayed by",
        "`rough_world_review.tscn`. This is **not** a gameplay navmesh and not owner visual acceptance.",
        "",
        f"Slope bands: easy <= {EASY_SLOPE:.2f}, passable <= {PASSABLE_SLOPE:.2f}, steep > {STEEP_SLOPE:.2f}, cliff > {CLIFF_SLOPE:.2f}.",
        "Grade is a review triage label: `candidate` means the passable mask has enough connected coverage to inspect, not that it is production-ready.",
        "",
        "## Default 25.6 km Synth Ranking",
        "",
        "| variant | grade | passable | largest connected | low-corridor | crosses | slope p95 |",
        "|---|---:|---:|---:|---:|---:|---:|",
    ]
    for r in at_default:
        crosses = ("WE" if r["largest_crosses_we"] else "") + ("NS" if r["largest_crosses_ns"] else "")
        lines.append(
            f"| {r['label']} | {r['grade']} | {_fmt_pct(r['passable_frac'])} | "
            f"{_fmt_pct(r['largest_passable_frac'])} | {_fmt_pct(r['largest_low_corridor_frac'])} | "
            f"{crosses or '-'} | {float(r['slope_p95']):.3f} |"
        )

    lines += [
        "",
        "## Scale Sensitivity",
        "",
        "| variant | scene km | grade | passable | largest connected | slope p95 | source/scene |",
        "|---|---:|---:|---:|---:|---:|---:|",
    ]
    focus_keys = [r["key"] for r in at_default[:3]]
    for key in focus_keys:
        for r in [row for row in synth if row["key"] == key]:
            lines.append(
                f"| {r['label']} | {float(r['scene_km']):.2f} | {r['grade']} | "
                f"{_fmt_pct(r['passable_frac'])} | {_fmt_pct(r['largest_passable_frac'])} | "
                f"{float(r['slope_p95']):.3f} | {float(r['source_scene_ratio']):.2f}x |"
            )
    lines += [
        "",
        "## Read",
        "",
        "- Smaller scene spans are expected to look like different, rougher places because the same vertical review relief is spread over less horizontal distance.",
        "- The content/landform scale knob is useful and should stay explicit.",
        "- Runtime near-field scale is a separate future task: page span, page pixels, LOD spacing, and detail frequency are still coupled in the live clipmap stack.",
    ]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    payload = json.loads(DATA_PATH.read_text(encoding="utf-8"))
    rows = audit_payload(payload)
    write_csv(rows)
    write_markdown(rows)
    print(f"wrote {OUT_CSV}")
    print(f"wrote {OUT_MD}")
    synth_default = [r for r in rows if r["kind"] == "synth" and float(r["scale_x"]) == 200.0]
    for r in sorted(synth_default, key=lambda x: (-float(x["largest_passable_frac"]), -float(x["passable_frac"])))[:3]:
        print(
            f"{r['label']}: grade={r['grade']} passable={_fmt_pct(r['passable_frac'])} "
            f"largest={_fmt_pct(r['largest_passable_frac'])} p95={float(r['slope_p95']):.3f}"
        )


if __name__ == "__main__":
    main()
