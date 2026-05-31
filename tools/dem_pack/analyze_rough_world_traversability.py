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
REFERENCE_SCALE = 200.0
REFERENCE_WORLD_SIZE_M = BASE_WORLD_SIZE_M * REFERENCE_SCALE
RELIEF_EXPONENTS = (0.0, 0.5, 1.0)

# Slope = rise/run. These are review bands, not engine constants.
EASY_SLOPE = 0.12
PASSABLE_SLOPE = 0.28
STEEP_SLOPE = 0.45
CLIFF_SLOPE = 0.80

# Structural route review bands. These intentionally reject a flat world: the
# target is passable structure through relief, not gentle terrain everywhere.
MIN_STRUCTURAL_SLOPE_P90 = 0.08
MAX_DIFFUSE_LOW_CORRIDOR_FRAC = 0.68
MAX_CANDIDATE_LOW_COMPONENTS = 80
MAX_THIN_LOW_COMPONENTS = 180


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


def height_scale_for(
    scene_width_m: float,
    relief: float = 1.0,
    relief_exponent: float = 0.0,
    reference_width_m: float = REFERENCE_WORLD_SIZE_M,
) -> float:
    """Return review vertical scale for a horizontal span.

    relief_exponent=0 is today's Godot review behavior: changing scale only
    stretches X/Z, so slopes fall as 1/span. relief_exponent=1 is slope
    invariant for the same height field, useful for separating "content density"
    from "everything got gentle."
    """
    if scene_width_m <= 0.0:
        raise ValueError("scene_width_m must be positive")
    if reference_width_m <= 0.0:
        raise ValueError("reference_width_m must be positive")
    return BASE_HEIGHT_SCALE_M * float(relief) * (float(scene_width_m) / float(reference_width_m)) ** float(relief_exponent)


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


def _legacy_grade(passable_frac: float, largest_frac: float, crosses_we: bool, crosses_ns: bool) -> str:
    crosses = crosses_we or crosses_ns
    if passable_frac >= 0.45 and largest_frac >= 0.35 and crosses:
        return "candidate"
    if passable_frac >= 0.25 and largest_frac >= 0.18:
        return "thin"
    return "blocked"


def _structural_corridor_grade(
    *,
    low_corridor_frac: float,
    largest_low_corridor_frac: float,
    low_component_count: int,
    low_touches_edges: int,
    low_crosses_we: bool,
    low_crosses_ns: bool,
    slope_p90: float,
) -> str:
    if slope_p90 < MIN_STRUCTURAL_SLOPE_P90:
        return "flat"
    if low_corridor_frac > MAX_DIFFUSE_LOW_CORRIDOR_FRAC:
        return "diffuse"
    low_crosses = low_crosses_we or low_crosses_ns
    if (
        largest_low_corridor_frac >= 0.25
        and low_touches_edges >= 2
        and low_component_count <= MAX_CANDIDATE_LOW_COMPONENTS
    ):
        return "candidate"
    if (
        largest_low_corridor_frac >= 0.12
        and (low_touches_edges >= 1 or low_crosses)
        and low_component_count <= MAX_THIN_LOW_COMPONENTS
    ):
        return "thin"
    return "blocked"


def _structural_corridor_score(
    low_corridor_frac: float,
    largest_low_corridor_frac: float,
    low_component_count: int,
    low_touches_edges: int,
    low_crosses_we: bool,
    low_crosses_ns: bool,
    slope_p90: float,
) -> float:
    corridor = min(largest_low_corridor_frac / 0.35, 1.0)
    exits = 1.0 if (low_crosses_we or low_crosses_ns) else min(low_touches_edges / 2.0, 1.0) * 0.70
    bounded = max(0.0, min(1.0, (MAX_THIN_LOW_COMPONENTS - low_component_count) / float(MAX_THIN_LOW_COMPONENTS)))
    relief_signal = max(0.0, min(1.0, (slope_p90 - MIN_STRUCTURAL_SLOPE_P90) / max(PASSABLE_SLOPE - MIN_STRUCTURAL_SLOPE_P90, 1e-9)))
    diffuse_penalty = max(0.0, min(1.0, (low_corridor_frac - MAX_DIFFUSE_LOW_CORRIDOR_FRAC) / max(1.0 - MAX_DIFFUSE_LOW_CORRIDOR_FRAC, 1e-9)))
    return 0.40 * corridor + 0.25 * exits + 0.20 * bounded + 0.15 * relief_signal - 0.25 * diffuse_penalty


def audit_item(item: dict, scale: float, relief: float = 1.0, relief_exponent: float = 0.0) -> dict[str, object]:
    h = height_grid(item)
    scene_width_m = BASE_WORLD_SIZE_M * float(scale)
    scene_km = scene_width_m / 1000.0
    source_km = float(item.get("span_km", 0.0))
    height_scale_m = height_scale_for(scene_width_m, relief=relief, relief_exponent=relief_exponent)
    slopes = slope_grid(h, scene_width_m, height_scale_m)
    passable = slopes <= PASSABLE_SLOPE
    easy = slopes <= EASY_SLOPE
    steep = slopes > STEEP_SLOPE
    cliff = slopes > CLIFF_SLOPE
    low_corridor = passable & (h <= np.percentile(h, 55.0))
    comps = component_stats(passable)
    low_comps = component_stats(low_corridor)
    passable_frac = float(np.mean(passable))
    largest_frac = float(comps["largest_frac"])
    low_corridor_frac = float(np.mean(low_corridor))
    largest_low_corridor_frac = float(low_comps["largest_frac"])
    low_component_count = int(low_comps["component_count"])
    low_touches_edges = int(low_comps["largest_touches_edges"])
    low_crosses_we = bool(low_comps["largest_crosses_we"])
    low_crosses_ns = bool(low_comps["largest_crosses_ns"])
    slope_p90 = float(np.percentile(slopes, 90.0))
    structural_grade = _structural_corridor_grade(
        low_corridor_frac=low_corridor_frac,
        largest_low_corridor_frac=largest_low_corridor_frac,
        low_component_count=low_component_count,
        low_touches_edges=low_touches_edges,
        low_crosses_we=low_crosses_we,
        low_crosses_ns=low_crosses_ns,
        slope_p90=slope_p90,
    )
    structural_score = _structural_corridor_score(
        low_corridor_frac,
        largest_low_corridor_frac,
        low_component_count,
        low_touches_edges,
        low_crosses_we,
        low_crosses_ns,
        slope_p90,
    )
    return {
        "key": item.get("key", ""),
        "label": item.get("label", ""),
        "kind": item.get("kind", ""),
        "scale_x": scale,
        "relief_exponent": relief_exponent,
        "source_km": source_km,
        "scene_km": scene_km,
        "source_scene_ratio": source_km / max(scene_km, 1e-9),
        "cell_m": scene_width_m / float(h.shape[0] - 1),
        "height_scale_m": height_scale_m,
        "slope_mean": float(np.mean(slopes)),
        "slope_p50": float(np.percentile(slopes, 50.0)),
        "slope_p90": slope_p90,
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
        "low_corridor_frac": low_corridor_frac,
        "low_corridor_components": low_component_count,
        "largest_low_corridor_frac": largest_low_corridor_frac,
        "largest_low_corridor_crosses_we": low_crosses_we,
        "largest_low_corridor_crosses_ns": low_crosses_ns,
        "largest_low_corridor_touches_edges": low_touches_edges,
        "diffuse_passable_frac": max(0.0, passable_frac - low_corridor_frac),
        "legacy_grade": _legacy_grade(passable_frac, largest_frac, bool(comps["largest_crosses_we"]), bool(comps["largest_crosses_ns"])),
        "grade": structural_grade,
        "structural_score": structural_score,
    }


def audit_payload(
    payload: dict,
    scales: Iterable[float] = SCALE_PRESETS,
    relief_exponents: Iterable[float] = RELIEF_EXPONENTS,
) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    for item in payload.get("items", []):
        for scale in scales:
            for relief_exponent in relief_exponents:
                rows.append(audit_item(item, float(scale), relief_exponent=float(relief_exponent)))
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
    current_policy = [r for r in synth if abs(float(r["relief_exponent"]) - 0.0) < 1e-6]
    at_default = [r for r in current_policy if abs(float(r["scale_x"]) - 200.0) < 1e-6]
    at_default.sort(key=lambda r: (str(r["grade"]) != "candidate", -float(r["structural_score"]), -float(r["largest_low_corridor_frac"])))

    lines = [
        "# Rough-World Traversability / Scale Audit",
        "",
        "Review heuristic over `rough_world_3d.json`, using the same conditioned height grid displayed by",
        "`rough_world_review.tscn`. This is **not** a gameplay navmesh and not owner visual acceptance.",
        "",
        f"Slope bands: easy <= {EASY_SLOPE:.2f}, passable <= {PASSABLE_SLOPE:.2f}, steep > {STEEP_SLOPE:.2f}, cliff > {CLIFF_SLOPE:.2f}.",
        f"Relief policy: `k=0` is today's scene behavior; `k=1` scales vertical relief with horizontal span around the {REFERENCE_WORLD_SIZE_M/1000.0:.1f} km reference span.",
        "Grade is a structural-corridor triage label: it rewards connected low/passable corridors through actual relief and rejects flat or diffuse passability.",
        "",
        "## Current Scene Policy (k=0), 25.6 km Synth Ranking",
        "",
        "| variant | grade | legacy | passable | low largest | low exits | low comps | slope p90 | score |",
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for r in at_default:
        exits = str(r["largest_low_corridor_touches_edges"])
        lines.append(
            f"| {r['label']} | {r['grade']} | {r['legacy_grade']} | {_fmt_pct(r['passable_frac'])} | "
            f"{_fmt_pct(r['largest_low_corridor_frac'])} | {exits} | {r['low_corridor_components']} | "
            f"{float(r['slope_p90']):.3f} | {float(r['structural_score']):.3f} |"
        )

    lines += [
        "",
        "## Current Scene Policy Scale Sensitivity (k=0)",
        "",
        "| variant | scene km | grade | legacy | passable | low largest | low comps | slope p90 | source/scene |",
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    focus_keys = [r["key"] for r in at_default[:3]]
    for key in focus_keys:
        for r in [row for row in current_policy if row["key"] == key]:
            lines.append(
                f"| {r['label']} | {float(r['scene_km']):.2f} | {r['grade']} | "
                f"{r['legacy_grade']} | {_fmt_pct(r['passable_frac'])} | {_fmt_pct(r['largest_low_corridor_frac'])} | "
                f"{r['low_corridor_components']} | {float(r['slope_p90']):.3f} | {float(r['source_scene_ratio']):.2f}x |"
            )

    policy_rows = [
        r for r in synth
        if r["key"] == "rough_anchor" and float(r["scale_x"]) in (10.0, 50.0, 100.0, 200.0)
    ]
    policy_rows.sort(key=lambda r: (float(r["relief_exponent"]), float(r["scale_x"])))
    lines += [
        "",
        "## Relief Policy Probe (rough_anchor)",
        "",
        "| k | scene km | height scale m | grade | passable | low largest | low comps | slope p50 | slope p90 |",
        "|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for r in policy_rows:
        lines.append(
            f"| {float(r['relief_exponent']):.1f} | {float(r['scene_km']):.2f} | {float(r['height_scale_m']):.1f} | "
            f"{r['grade']} | {_fmt_pct(r['passable_frac'])} | {_fmt_pct(r['largest_low_corridor_frac'])} | "
            f"{r['low_corridor_components']} | {float(r['slope_p50']):.3f} | {float(r['slope_p90']):.3f} |"
        )
    lines += [
        "",
        "## Read",
        "",
        "- Today's k=0 scene behavior is useful as a content-density review knob, but its slope grades are dominated by the 1/span law.",
        "- k=1 is the control probe for slope-invariant review of the same height field; it is not a runtime decision yet.",
        "- Structural corridor grade is deliberately stricter than legacy passability: a flat world can be passable everywhere and still fail.",
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
    synth_default = [r for r in synth_default if abs(float(r["relief_exponent"]) - 0.0) < 1e-6]
    for r in sorted(synth_default, key=lambda x: (-float(x["structural_score"]), -float(x["largest_low_corridor_frac"])))[:3]:
        print(
            f"{r['label']}: grade={r['grade']} legacy={r['legacy_grade']} "
            f"low_largest={_fmt_pct(r['largest_low_corridor_frac'])} p90={float(r['slope_p90']):.3f}"
        )


if __name__ == "__main__":
    main()
