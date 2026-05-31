r"""Export adjacent rough-highlands chunks for Godot review.

This is a render-first proof artifact, not a Rust/GLSL runtime port. It builds
each 25.6 km chunk from its own deterministic world-coordinate skeleton window,
crops an authoritative core, and stores one-sample height/corridor aprons for
seam-stable review. The legacy rough keeper's isolated-window diagnostic stays
in the report because it explains why the earlier super-window split was not a
true infinite-window contract.

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
from scipy.ndimage import label

import analyze_rough_world_traversability as trav
import geography_engine as geo
import geography_skeleton as skel
import geography_skeleton_windows as win
import worldgen_proto as wg
from export_godot_rough_world_review import _condition
from render_geography_skeleton_focus import FOCUS


OUT = Path("wg-10/worldgen_terrain/generated/review/rough_world_chunks_3x3.json")
REPORT_DIR = Path("D:/tmp/wg10_geography_engine")
REPORT_CSV = REPORT_DIR / "rough_world_chunks_3x3_seams.csv"
REPORT_MD = REPORT_DIR / "rough_world_chunks_3x3_seams.md"
TRAVEL_REPORT_CSV = REPORT_DIR / "rough_world_chunks_virtual_travel.csv"
TRAVEL_REPORT_MD = REPORT_DIR / "rough_world_chunks_virtual_travel.md"
VISUAL_SEAM_REPORT_CSV = REPORT_DIR / "rough_world_chunks_visual_seams.csv"
VISUAL_SEAM_REPORT_MD = REPORT_DIR / "rough_world_chunks_visual_seams.md"

GENERATOR_VERSION = "rough_world_chunks_v2_independent_windows"
CHUNK_COUNT = 3
CHUNK_N = 129
CHUNK_SPAN_M = 25_600.0
WINDOW_APRON_M = 25_600.0
WORLD_ORIGIN_X_M = 60_000.0
WORLD_ORIGIN_Z_M = 36_000.0
SEEDS = (133, 211)
SCENARIO = next(scenario for scenario in FOCUS if scenario.key == "rough_anchor")


def _window_spec(chunk_n: int, chunk_span_m: float, apron_m: float = WINDOW_APRON_M) -> win.SkeletonWindowSpec:
    spacing = float(chunk_span_m) / float(int(chunk_n) - 1)
    return win.SkeletonWindowSpec(
        core_span_m=float(chunk_span_m),
        apron_m=float(apron_m),
        spacing_m=spacing,
    )


def _compose_windowed_height(window: dict[str, object], seed: int, spec: win.SkeletonWindowSpec) -> tuple[np.ndarray, np.ndarray]:
    """Compose review height from world-window facts without per-window normalization.

    The routed skeleton supplies the organizing facts; world-coordinate noise is
    only local material. The transform is fixed, so the same seed+coordinate
    yields the same height regardless of which chunk requested it.
    """
    wx = np.asarray(window["wx"], dtype=np.float64)
    wz = np.asarray(window["wz"], dtype=np.float64)
    core = float(spec.core_span_m)
    uplift = np.asarray(window["uplift"], dtype=np.float64)
    discharge = np.asarray(window["discharge"], dtype=np.float64)
    tributary = np.asarray(window["tributary"], dtype=np.float64)
    channel_axis = np.asarray(window["channel_axis"], dtype=np.float64)
    crest_dist = np.asarray(window["crest_dist"], dtype=np.float64)
    channel_dist = np.asarray(window["channel_dist"], dtype=np.float64)

    mat_x, mat_z = wg.recursive_domain_warp(
        wx,
        wz,
        warp_amount=core * 0.055,
        warp_freq=1.0 / (core * 0.82),
        seed=int(seed) + 12000,
        steps=2,
        decay=0.54,
        freq_mul=1.85,
    )
    ridge_detail = wg.ridged_multifractal(mat_x, mat_z, 1.0 / (core * 0.155), 5, int(seed) + 12010, gain=0.54)
    shoulder_detail = wg.ridged_multifractal(mat_x, mat_z, 1.0 / (core * 0.34), 4, int(seed) + 12020, gain=0.58)
    route_texture = wg.ridged_multifractal(mat_x, mat_z, 1.0 / (core * 0.42), 5, int(seed) + 12025, gain=0.57)
    small_detail = wg.fbm(mat_x, mat_z, 1.0 / (core * 0.080), 4, int(seed) + 12030, gain=0.50)

    crest_near = 1.0 - geo.smoothstep(core * 0.055, core * 0.34, crest_dist)
    channel_near = 1.0 - geo.smoothstep(core * 0.025, core * 0.14, channel_dist)
    route_axis = geo.smoothstep(0.50, 0.76, route_texture)
    routed_cut = np.clip(0.58 * geo.smoothstep(0.13, 0.62, channel_axis) + 0.30 * channel_near + 0.12 * tributary, 0.0, 1.0)
    routed_cut = np.maximum(routed_cut, 0.62 * route_axis)
    wet_floor = np.clip(0.64 * channel_axis + 0.36 * geo.smoothstep(0.14, 0.54, discharge), 0.0, 1.0)
    wet_floor = np.maximum(wet_floor, 0.55 * route_axis)
    highland_mask = geo.smoothstep(0.36, 0.78, uplift)

    height = (
        1.52 * (uplift - 0.46)
        + 0.34 * crest_near * (0.45 + 0.55 * highland_mask)
        + 0.22 * shoulder_detail * highland_mask
        + 0.14 * ridge_detail * (0.35 + 0.65 * highland_mask)
        + 0.065 * small_detail * (1.0 - 0.58 * wet_floor)
    )
    height -= routed_cut * (0.52 + 0.42 * highland_mask)
    height -= 0.18 * route_axis * (0.70 + 0.30 * highland_mask)
    height -= 0.18 * wet_floor * (1.0 - highland_mask)
    height += 0.09 * (ridge_detail - 0.50) * (1.0 - routed_cut)
    height = np.tanh(height * 1.18)

    corridor = win.corridor_mask(
        {
            "channel_axis": channel_axis,
            "channel_dist": channel_dist,
        },
        spec,
        channel_axis_threshold=0.16,
        channel_distance_m=float(spec.spacing_m) * 6.0,
    )
    corridor = np.asarray(corridor, dtype=bool) | (route_axis >= 0.22)
    return height.astype(np.float64), np.asarray(corridor, dtype=bool)


def _build_independent_chunk(
    seed: int,
    *,
    chunk_x: int,
    chunk_z: int,
    chunk_count: int,
    chunk_n: int,
    chunk_span_m: float,
    world_origin_x_m: float,
    world_origin_z_m: float,
) -> dict[str, object]:
    spec = _window_spec(chunk_n, chunk_span_m)
    chunk_world_x = float(world_origin_x_m) + float(chunk_x) * float(chunk_span_m)
    chunk_world_z = float(world_origin_z_m) + float(chunk_z) * float(chunk_span_m)
    window = win.build_skeleton_window(chunk_world_x, chunk_world_z, int(seed), spec)
    full_height, full_corridor = _compose_windowed_height(window, int(seed), spec)
    core_slice = win._core_slice(spec)
    start = int(core_slice.start)
    stop = int(core_slice.stop)
    core = full_height[start:stop, start:stop]
    corridor = full_corridor[start:stop, start:stop]
    apron = full_height[start - 1 : stop + 1, start - 1 : stop + 1]
    corridor_apron = full_corridor[start - 1 : stop + 1, start - 1 : stop + 1]
    display_origin_x = (float(chunk_x) - float(chunk_count) * 0.5) * float(chunk_span_m)
    display_origin_z = (float(chunk_z) - float(chunk_count) * 0.5) * float(chunk_span_m)
    return {
        "source": "independent_window",
        "chunk_x": int(chunk_x),
        "chunk_z": int(chunk_z),
        "key": f"{chunk_x}_{chunk_z}",
        "label": f"chunk {chunk_x},{chunk_z}",
        "n": int(chunk_n),
        "apron_n": int(chunk_n) + 2,
        "span_m": float(chunk_span_m),
        "world_origin_x_m": chunk_world_x,
        "world_origin_z_m": chunk_world_z,
        "display_origin_x_m": display_origin_x,
        "display_origin_z_m": display_origin_z,
        "height": np.round(core, 4).astype(float).ravel().tolist(),
        "apron_height": np.round(apron, 4).astype(float).ravel().tolist(),
        "corridor": corridor.astype(int).ravel().tolist(),
        "apron_corridor": corridor_apron.astype(int).ravel().tolist(),
    }


def _stitch_grid(chunks: list[dict[str, object]], chunk_count: int, chunk_n: int, field: str) -> np.ndarray:
    step = int(chunk_n) - 1
    world_n = int(chunk_count) * step + 1
    out = np.zeros((world_n, world_n), dtype=np.float64)
    for chunk in chunks:
        x = int(chunk["chunk_x"])
        z = int(chunk["chunk_z"])
        arr = np.asarray(chunk[field], dtype=np.float64).reshape((chunk_n, chunk_n))
        out[z * step : z * step + chunk_n, x * step : x * step + chunk_n] = arr
    return out


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
    # Kept for the previous test/export call shape; the independent-window path
    # derives spacing directly from chunk_n and chunk_span_m.
    _ = coarse_n
    chunks = [
        _build_independent_chunk(
            int(seed),
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
    conditioned = _stitch_grid(chunks, int(chunk_count), int(chunk_n), "height")
    corridor = _stitch_grid(chunks, int(chunk_count), int(chunk_n), "corridor")
    stats = {
        "min": float(np.min(conditioned)),
        "max": float(np.max(conditioned)),
        "mean": float(np.mean(conditioned)),
        "std": float(np.std(conditioned)),
    }
    return {
        "seed": int(seed),
        "label": f"seed {seed}",
        "height": np.round(conditioned, 4).astype(float).ravel().tolist(),
        "corridor": corridor.astype(int).ravel().tolist(),
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
        "window_apron_m": float(WINDOW_APRON_M),
        "window_spacing_m": float(chunk_span_m) / float(int(chunk_n) - 1),
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


def _corridor_array(chunk: dict[str, object]) -> np.ndarray:
    n = int(chunk["n"])
    return np.asarray(chunk["corridor"], dtype=bool).reshape((n, n))


def _apron_array(chunk: dict[str, object]) -> np.ndarray:
    n = int(chunk["apron_n"])
    return np.asarray(chunk["apron_height"], dtype=np.float64).reshape((n, n))


def _corridor_mask(world_height: np.ndarray, world_span_m: float) -> np.ndarray:
    slopes = trav.slope_grid(world_height, scene_width_m=float(world_span_m), height_scale_m=trav.BASE_HEIGHT_SCALE_M)
    return (slopes <= trav.PASSABLE_SLOPE) & (world_height <= np.percentile(world_height, 55.0))


def _world_corridor(seed_world: dict[str, object], chunk_count: int, chunk_n: int) -> np.ndarray:
    world_n = int(seed_world["world_n"])
    if "corridor" in seed_world:
        return np.asarray(seed_world["corridor"], dtype=bool).reshape((world_n, world_n))
    world_height = np.asarray(seed_world["height"], dtype=np.float64).reshape((world_n, world_n))
    return _corridor_mask(world_height, float(chunk_count) * float(CHUNK_SPAN_M))


def _component_crossing_count(strip: np.ndarray, seam_index: int) -> tuple[int, int]:
    """Count seam corridor samples whose component reaches both chunk interiors."""
    mask = np.asarray(strip, dtype=bool)
    seam = int(seam_index)
    edge = mask[:, seam]
    entering = int(np.count_nonzero(edge))
    if entering == 0:
        return 0, 0
    labels, _ = label(mask, structure=np.ones((3, 3), dtype=np.int8))
    left_labels = set(np.unique(labels[:, :seam][labels[:, :seam] > 0]).astype(int).tolist())
    right_labels = set(np.unique(labels[:, seam + 1 :][labels[:, seam + 1 :] > 0]).astype(int).tolist())
    crossing = left_labels & right_labels
    if not crossing:
        return entering, 0
    edge_labels = labels[:, seam]
    matched = int(np.count_nonzero(edge & np.isin(edge_labels, list(crossing))))
    return entering, matched


def _apron_sample(apron: np.ndarray, x: int, z: int) -> float:
    ax = int(np.clip(int(x) + 1, 0, apron.shape[1] - 1))
    az = int(np.clip(int(z) + 1, 0, apron.shape[0] - 1))
    return float(apron[az, ax])


def _slope_at(apron: np.ndarray, x: int, z: int, cell_m: float, height_scale_m: float = trav.BASE_HEIGHT_SCALE_M) -> float:
    hl = _apron_sample(apron, x - 1, z)
    hr = _apron_sample(apron, x + 1, z)
    hd = _apron_sample(apron, x, z - 1)
    hu = _apron_sample(apron, x, z + 1)
    dx = ((hr - hl) * float(height_scale_m)) / max(float(cell_m) * 2.0, 0.001)
    dz = ((hu - hd) * float(height_scale_m)) / max(float(cell_m) * 2.0, 0.001)
    return float(np.sqrt(dx * dx + dz * dz))


def _normal_at(apron: np.ndarray, x: int, z: int, cell_m: float, height_scale_m: float = trav.BASE_HEIGHT_SCALE_M) -> np.ndarray:
    hl = _apron_sample(apron, x - 1, z)
    hr = _apron_sample(apron, x + 1, z)
    hd = _apron_sample(apron, x, z - 1)
    hu = _apron_sample(apron, x, z + 1)
    x_vec = np.array([float(cell_m) * 2.0, (hr - hl) * float(height_scale_m), 0.0], dtype=np.float64)
    z_vec = np.array([0.0, (hu - hd) * float(height_scale_m), float(cell_m) * 2.0], dtype=np.float64)
    normal = np.cross(z_vec, x_vec)
    return normal / max(float(np.linalg.norm(normal)), 1e-12)


def _lerp_color(a: np.ndarray, b: np.ndarray, t: float) -> np.ndarray:
    return a + (b - a) * float(np.clip(t, 0.0, 1.0))


def _terrain_color_rgb(h: float) -> np.ndarray:
    t = float(np.clip((float(h) + 1.0) * 0.5, 0.0, 1.0))
    low = np.array([0.40, 0.48, 0.38], dtype=np.float64)
    mid = np.array([0.62, 0.56, 0.40], dtype=np.float64)
    high = np.array([0.74, 0.70, 0.58], dtype=np.float64)
    crest = np.array([0.82, 0.79, 0.68], dtype=np.float64)
    if t < 0.58:
        return _lerp_color(low, mid, t / 0.58)
    if t < 0.90:
        return _lerp_color(mid, high, (t - 0.58) / 0.32)
    return _lerp_color(high, crest, (t - 0.90) / 0.10)


def visual_seam_rows(payload: dict[str, object], height_scale_m: float = trav.BASE_HEIGHT_SCALE_M) -> list[dict[str, object]]:
    """Mirror Godot edge normal/slope/color math to estimate visible seam risk."""
    chunk_count = int(payload["chunk_count"])
    chunk_n = int(payload["chunk_n"])
    rows: list[dict[str, object]] = []
    for seed_world in payload["seeds"]:
        seed = int(seed_world["seed"])
        grid = _chunk_grid(seed_world, chunk_count)
        for z in range(chunk_count):
            for x in range(chunk_count - 1):
                left = grid[z][x]
                right = grid[z][x + 1]
                rows.append(_visual_pair_row(seed, "x", f"{x},{z}", f"{x + 1},{z}", left, right, height_scale_m))
        for z in range(chunk_count - 1):
            for x in range(chunk_count):
                top = grid[z][x]
                bottom = grid[z + 1][x]
                rows.append(_visual_pair_row(seed, "z", f"{x},{z}", f"{x},{z + 1}", top, bottom, height_scale_m))
    return rows


def _visual_pair_row(
    seed: int,
    axis: str,
    a_label: str,
    b_label: str,
    a_chunk: dict[str, object],
    b_chunk: dict[str, object],
    height_scale_m: float,
) -> dict[str, object]:
    n = int(a_chunk["n"])
    cell = float(a_chunk["span_m"]) / float(n - 1)
    ah = _height_array(a_chunk)
    bh = _height_array(b_chunk)
    aa = _apron_array(a_chunk)
    ba = _apron_array(b_chunk)
    ac = _corridor_array(a_chunk)
    bc = _corridor_array(b_chunk)

    if axis == "x":
        a_heights = ah[:, -1]
        b_heights = bh[:, 0]
        a_corridor = ac[:, -1]
        b_corridor = bc[:, 0]
        coords = [(n - 1, i, 0, i) for i in range(n)]
    else:
        a_heights = ah[-1, :]
        b_heights = bh[0, :]
        a_corridor = ac[-1, :]
        b_corridor = bc[0, :]
        coords = [(i, n - 1, i, 0) for i in range(n)]

    normal_dots: list[float] = []
    slope_deltas: list[float] = []
    color_deltas: list[float] = []
    for ax, az, bx, bz in coords:
        an = _normal_at(aa, ax, az, cell, height_scale_m)
        bn = _normal_at(ba, bx, bz, cell, height_scale_m)
        normal_dots.append(float(np.clip(np.dot(an, bn), -1.0, 1.0)))
        a_slope = _slope_at(aa, ax, az, cell, height_scale_m)
        b_slope = _slope_at(ba, bx, bz, cell, height_scale_m)
        slope_deltas.append(abs(a_slope - b_slope))
        color_deltas.append(float(np.linalg.norm(_terrain_color_rgb(a_heights[len(slope_deltas) - 1]) - _terrain_color_rgb(b_heights[len(slope_deltas) - 1]))))

    min_dot = min(normal_dots) if normal_dots else 1.0
    return {
        "kind": "visual_seam",
        "seed": int(seed),
        "axis": axis,
        "a": a_label,
        "b": b_label,
        "height_max_abs_delta": float(np.max(np.abs(a_heights - b_heights))),
        "height_max_delta_m": float(np.max(np.abs(a_heights - b_heights)) * float(height_scale_m)),
        "normal_min_dot": float(min_dot),
        "normal_max_angle_deg": float(np.degrees(np.arccos(np.clip(min_dot, -1.0, 1.0)))),
        "slope_max_abs_delta": float(np.max(slope_deltas)) if slope_deltas else 0.0,
        "terrain_color_max_delta": float(np.max(color_deltas)) if color_deltas else 0.0,
        "corridor_edge_mismatch_count": int(np.count_nonzero(a_corridor != b_corridor)),
    }


def seam_rows(payload: dict[str, object]) -> list[dict[str, object]]:
    chunk_count = int(payload["chunk_count"])
    chunk_n = int(payload["chunk_n"])
    step = chunk_n - 1
    rows: list[dict[str, object]] = []
    for seed_world in payload["seeds"]:
        seed = int(seed_world["seed"])
        grid = _chunk_grid(seed_world, chunk_count)
        corridors = _world_corridor(seed_world, chunk_count, chunk_n)
        for z in range(chunk_count):
            for x in range(chunk_count - 1):
                left = _height_array(grid[z][x])
                right = _height_array(grid[z][x + 1])
                boundary = (x + 1) * step
                strip = corridors[z * step : z * step + chunk_n, x * step : (x + 2) * step + 1]
                entering, matched = _component_crossing_count(strip, step)
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
                strip = corridors[z * step : (z + 2) * step + 1, x * step : x * step + chunk_n].T
                entering, matched = _component_crossing_count(strip, step)
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


def adjacent_pair_variation_rows(payload: dict[str, object]) -> list[dict[str, object]]:
    chunk_count = int(payload["chunk_count"])
    rows: list[dict[str, object]] = []
    for seed_world in payload["seeds"]:
        seed = int(seed_world["seed"])
        grid = _chunk_grid(seed_world, chunk_count)
        for z in range(chunk_count):
            for x in range(chunk_count - 1):
                a = _height_array(grid[z][x])
                b = _height_array(grid[z][x + 1])
                rows.append({
                    "kind": "adjacent_pair",
                    "seed": seed,
                    "axis": "x",
                    "a": f"{x},{z}",
                    "b": f"{x + 1},{z}",
                    "mean_abs_delta": float(np.mean(np.abs(a - b))),
                    "corrcoef": float(np.corrcoef(a.ravel(), b.ravel())[0, 1]),
                })
        for z in range(chunk_count - 1):
            for x in range(chunk_count):
                a = _height_array(grid[z][x])
                b = _height_array(grid[z + 1][x])
                rows.append({
                    "kind": "adjacent_pair",
                    "seed": seed,
                    "axis": "z",
                    "a": f"{x},{z}",
                    "b": f"{x},{z + 1}",
                    "mean_abs_delta": float(np.mean(np.abs(a - b))),
                    "corrcoef": float(np.corrcoef(a.ravel(), b.ravel())[0, 1]),
                })
    return rows


def virtual_travel_summary_rows(
    seeds: Iterable[int] = SEEDS,
    *,
    chunk_count: int = 5,
    chunk_n: int = 65,
    chunk_span_m: float = CHUNK_SPAN_M,
    origin_x_m: float = WORLD_ORIGIN_X_M - 2.0 * CHUNK_SPAN_M,
    origin_z_m: float = WORLD_ORIGIN_Z_M - 2.0 * CHUNK_SPAN_M,
) -> list[dict[str, object]]:
    """Stress the independent-window contract across a wider bounded travel lattice."""
    payload = build_payload(
        seeds=seeds,
        chunk_count=chunk_count,
        chunk_n=chunk_n,
        chunk_span_m=chunk_span_m,
        origin_x_m=origin_x_m,
        origin_z_m=origin_z_m,
    )
    seam_by_seed: dict[int, list[dict[str, object]]] = {}
    for row in seam_rows(payload):
        seam_by_seed.setdefault(int(row["seed"]), []).append(row)
    variation_by_seed: dict[int, list[dict[str, object]]] = {}
    for row in adjacent_pair_variation_rows(payload):
        variation_by_seed.setdefault(int(row["seed"]), []).append(row)

    summaries: list[dict[str, object]] = []
    for seed in [int(s) for s in seeds]:
        seams = seam_by_seed.get(seed, [])
        variations = variation_by_seed.get(seed, [])
        mean_deltas = np.asarray([float(row["mean_abs_delta"]) for row in variations], dtype=np.float64)
        corrcoefs = np.asarray([float(row["corrcoef"]) for row in variations], dtype=np.float64)
        summaries.append({
            "kind": "virtual_travel_summary",
            "seed": seed,
            "chunk_count": int(chunk_count),
            "chunk_n": int(chunk_n),
            "world_span_km": float(chunk_count) * float(chunk_span_m) / 1000.0,
            "seams_count": len(seams),
            "height_max_abs_delta": max(float(row["height_max_abs_delta"]) for row in seams) if seams else 0.0,
            "corridor_min_match_frac": min(float(row["corridor_match_frac"]) for row in seams) if seams else 1.0,
            "corridor_entering_count": sum(int(row["corridor_entering_count"]) for row in seams),
            "adjacent_pair_count": len(variations),
            "adjacent_mean_abs_delta_min": float(np.min(mean_deltas)) if mean_deltas.size else 0.0,
            "adjacent_mean_abs_delta_median": float(np.median(mean_deltas)) if mean_deltas.size else 0.0,
            "adjacent_corrcoef_max": float(np.max(corrcoefs)) if corrcoefs.size else 0.0,
        })
    return summaries


def independent_window_diagnostic_rows(
    seeds: Iterable[int] = (133,),
    *,
    chunk_n: int = 49,
    chunk_span_m: float = CHUNK_SPAN_M,
    origin_x_m: float = WORLD_ORIGIN_X_M,
    origin_z_m: float = WORLD_ORIGIN_Z_M,
    coarse_n: int = 64,
) -> list[dict[str, object]]:
    """Show why the current keeper is not yet safe as independent runtime windows."""
    rows: list[dict[str, object]] = []
    for seed in seeds:
        wx_a, wz_a = skel.geo.grid(chunk_n, chunk_span_m, ox=origin_x_m, oz=origin_z_m)
        wx_b, wz_b = skel.geo.grid(chunk_n, chunk_span_m, ox=origin_x_m + chunk_span_m, oz=origin_z_m)
        wx_c, wz_c = skel.geo.grid(chunk_n, chunk_span_m, ox=origin_x_m, oz=origin_z_m + chunk_span_m)
        raw_a = np.asarray(skel.compose_height(wx_a, wz_a, seed=int(seed), scenario=SCENARIO, coarse_n=coarse_n)["height"], dtype=np.float64)
        raw_b = np.asarray(skel.compose_height(wx_b, wz_b, seed=int(seed), scenario=SCENARIO, coarse_n=coarse_n)["height"], dtype=np.float64)
        raw_c = np.asarray(skel.compose_height(wx_c, wz_c, seed=int(seed), scenario=SCENARIO, coarse_n=coarse_n)["height"], dtype=np.float64)
        cond_a, _ = _condition(raw_a)
        cond_b, _ = _condition(raw_b)
        cond_c, _ = _condition(raw_c)
        pairs = (
            ("x", raw_a[:, -1], raw_b[:, 0], cond_a[:, -1], cond_b[:, 0]),
            ("z", raw_a[-1, :], raw_c[0, :], cond_a[-1, :], cond_c[0, :]),
        )
        for axis, raw_left, raw_right, cond_left, cond_right in pairs:
            rows.append({
                "kind": "independent_window_diagnostic",
                "seed": int(seed),
                "axis": axis,
                "chunk_n": int(chunk_n),
                "raw_height_max_abs_delta": float(np.max(np.abs(raw_left - raw_right))),
                "raw_height_mean_abs_delta": float(np.mean(np.abs(raw_left - raw_right))),
                "conditioned_height_max_abs_delta": float(np.max(np.abs(cond_left - cond_right))),
                "conditioned_height_mean_abs_delta": float(np.mean(np.abs(cond_left - cond_right))),
            })
    return rows


def write_reports(
    payload: dict[str, object],
    rows: list[dict[str, object]],
    visual_rows: list[dict[str, object]],
    variation: list[dict[str, object]],
    independent_diagnostics: list[dict[str, object]],
    travel_summaries: list[dict[str, object]],
) -> None:
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    csv_rows = rows + visual_rows + variation + independent_diagnostics + travel_summaries
    keys = sorted({key for row in csv_rows for key in row.keys()})
    with REPORT_CSV.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=keys)
        writer.writeheader()
        writer.writerows(csv_rows)
    lines = [
        "# Rough-World 3x3 Chunk Seam Report",
        "",
        f"Generator: `{payload['generator_version']}`; scenario: `{payload['scenario_key']}`; chunk span: {float(payload['chunk_span_m'])/1000.0:.1f} km.",
        f"Window authority: each chunk is generated from its own world-coordinate skeleton window with {float(payload['window_apron_m'])/1000.0:.1f} km aprons at {float(payload['window_spacing_m']):.1f} m spacing.",
        "This is still an offline/Godot proof artifact, not a Rust/GLSL runtime streaming port.",
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
        "## Visual Seam Risk",
        "",
        "This mirrors the Godot review mesh's default-height, normal, slope, and terrain-color edge math.",
        "",
        "| seed | axis | a | b | height m | normal max deg | slope max delta | terrain color max delta | corridor mismatches |",
        "|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for row in visual_rows:
        lines.append(
            f"| {row['seed']} | {row['axis']} | {row['a']} | {row['b']} | "
            f"{float(row['height_max_delta_m']):.4f} | {float(row['normal_max_angle_deg']):.4f} | "
            f"{float(row['slope_max_abs_delta']):.6f} | {float(row['terrain_color_max_delta']):.6f} | "
            f"{row['corridor_edge_mismatch_count']} |"
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
    lines += [
        "",
        "## Independent Window Diagnostic",
        "",
        "This intentionally runs the legacy rough-highlands keeper path as separate adjacent 25.6 km windows.",
        "Nonzero seam deltas here are why the old path could not be used as the independent-window contract.",
        "",
        "| seed | axis | raw max | raw mean | conditioned max | conditioned mean |",
        "|---:|---:|---:|---:|---:|---:|",
    ]
    for row in independent_diagnostics:
        lines.append(
            f"| {row['seed']} | {row['axis']} | {float(row['raw_height_max_abs_delta']):.4f} | "
            f"{float(row['raw_height_mean_abs_delta']):.4f} | {float(row['conditioned_height_max_abs_delta']):.4f} | "
            f"{float(row['conditioned_height_mean_abs_delta']):.4f} |"
        )
    lines += [
        "",
        "## Virtual Travel Summary",
        "",
        "This builds a wider 5x5 lattice from independent world windows at lower report resolution.",
        "It is a bounded stress probe for movement in all directions, not a streaming runtime.",
        "",
        "| seed | chunks | span km | seams | height max | corridor min | adjacent min delta | adjacent median delta | adjacent max corr |",
        "|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for row in travel_summaries:
        lines.append(
            f"| {row['seed']} | {row['chunk_count']}x{row['chunk_count']} | {float(row['world_span_km']):.1f} | "
            f"{row['seams_count']} | {float(row['height_max_abs_delta']):.6f} | "
            f"{float(row['corridor_min_match_frac']):.3f} | {float(row['adjacent_mean_abs_delta_min']):.4f} | "
            f"{float(row['adjacent_mean_abs_delta_median']):.4f} | {float(row['adjacent_corrcoef_max']):.4f} |"
        )
    REPORT_MD.write_text("\n".join(lines) + "\n", encoding="utf-8")

    visual_keys = list(visual_rows[0].keys()) if visual_rows else []
    with VISUAL_SEAM_REPORT_CSV.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=visual_keys)
        writer.writeheader()
        writer.writerows(visual_rows)
    visual_lines = [
        "# Rough-World Visual Seam Risk",
        "",
        "Offline mirror of the Godot review mesh's edge height, normal, slope, default terrain-color, and corridor-edge comparisons.",
        "This is a gate-style risk probe; owner fly review remains the visual authority.",
        "",
        "| seed | axis | a | b | height m | normal max deg | slope max delta | terrain color max delta | corridor mismatches |",
        "|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for row in visual_rows:
        visual_lines.append(
            f"| {row['seed']} | {row['axis']} | {row['a']} | {row['b']} | "
            f"{float(row['height_max_delta_m']):.4f} | {float(row['normal_max_angle_deg']):.4f} | "
            f"{float(row['slope_max_abs_delta']):.6f} | {float(row['terrain_color_max_delta']):.6f} | "
            f"{row['corridor_edge_mismatch_count']} |"
        )
    VISUAL_SEAM_REPORT_MD.write_text("\n".join(visual_lines) + "\n", encoding="utf-8")

    travel_keys = list(travel_summaries[0].keys()) if travel_summaries else []
    with TRAVEL_REPORT_CSV.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=travel_keys)
        writer.writeheader()
        writer.writerows(travel_summaries)
    travel_lines = [
        "# Rough-World Virtual Travel Summary",
        "",
        "Bounded stress probe over independently generated chunks. This supports the infinite-world direction, but it is not a runtime streaming/cache proof.",
        "",
        "| seed | chunks | span km | seams | height max | corridor min | corridor entering | adjacent pairs | adjacent min delta | adjacent median delta | adjacent max corr |",
        "|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for row in travel_summaries:
        travel_lines.append(
            f"| {row['seed']} | {row['chunk_count']}x{row['chunk_count']} | {float(row['world_span_km']):.1f} | "
            f"{row['seams_count']} | {float(row['height_max_abs_delta']):.6f} | "
            f"{float(row['corridor_min_match_frac']):.3f} | {row['corridor_entering_count']} | "
            f"{row['adjacent_pair_count']} | {float(row['adjacent_mean_abs_delta_min']):.4f} | "
            f"{float(row['adjacent_mean_abs_delta_median']):.4f} | {float(row['adjacent_corrcoef_max']):.4f} |"
        )
    TRAVEL_REPORT_MD.write_text("\n".join(travel_lines) + "\n", encoding="utf-8")


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    payload = build_payload()
    OUT.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    rows = seam_rows(payload)
    visual_rows = visual_seam_rows(payload)
    variation = variation_rows(payload)
    independent_diagnostics = independent_window_diagnostic_rows(seeds=(SEEDS[0],))
    travel_summaries = virtual_travel_summary_rows()
    write_reports(payload, rows, visual_rows, variation, independent_diagnostics, travel_summaries)
    max_seam = max(float(row["height_max_abs_delta"]) for row in rows)
    min_corridor = min(float(row["corridor_match_frac"]) for row in rows)
    max_normal_angle = max(float(row["normal_max_angle_deg"]) for row in visual_rows)
    print(f"wrote {OUT}")
    print(f"wrote {REPORT_CSV}")
    print(f"wrote {REPORT_MD}")
    print(f"wrote {TRAVEL_REPORT_CSV}")
    print(f"wrote {TRAVEL_REPORT_MD}")
    print(f"wrote {VISUAL_SEAM_REPORT_CSV}")
    print(f"wrote {VISUAL_SEAM_REPORT_MD}")
    print(f"max seam delta={max_seam:.6f} min corridor match={min_corridor:.3f} max normal angle={max_normal_angle:.4f}")


if __name__ == "__main__":
    main()
