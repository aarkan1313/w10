"""Export the rough-highlands keeper v1 contract fixture.

This freezes the current Python/Godot review keeper as deterministic data for
the future Rust/GPU port. It is not a runtime port and it is not a claim of full
terrain/gameplay acceptance.

Run:
    python tools/dem_pack/export_rough_highlands_keeper_contract.py

Writes:
    tools/dem_pack/fixtures/rough_highlands_keeper_v1.json
"""

from __future__ import annotations

import hashlib
import io
import json
from pathlib import Path
from typing import Any

import numpy as np

import analyze_rough_world_traversability as trav
import export_godot_rough_world_chunks as chunks
import geography_skeleton_windows as win
import render_rough_world_chunks_review as chunk_render


KEEPER_ID = "rough_highlands_keeper_v1"
FIXTURE_PATH = Path("tools/dem_pack/fixtures/rough_highlands_keeper_v1.json")
FIXTURE_CHUNK_N = 33
FIXTURE_CONTACT_PANEL_PX = 96
FIXTURE_SEEDS = (133, 211)

SAMPLE_POINTS = (
    # seed, chunk_x, chunk_z, grid_x, grid_z
    (133, 1, 1, 0, 0),
    (133, 1, 1, 8, 12),
    (133, 1, 1, 16, 16),
    (133, 1, 1, 31, 16),
    (133, 2, 1, 0, 16),
    (211, 1, 1, 16, 16),
    (211, 1, 2, 20, 4),
)

HEIGHT_COMPOSITION_CONTRACT = {
    "domain_warp": {
        "warp_amount_core_frac": 0.055,
        "warp_freq_core_frac": 0.82,
        "steps": 2,
        "decay": 0.54,
        "freq_mul": 1.85,
        "seed_offset": 12000,
    },
    "material_fields": {
        "ridge_detail": {"freq_core_frac": 0.155, "octaves": 5, "seed_offset": 12010, "gain": 0.54},
        "shoulder_detail": {"freq_core_frac": 0.34, "octaves": 4, "seed_offset": 12020, "gain": 0.58},
        "route_texture": {"freq_core_frac": 0.42, "octaves": 5, "seed_offset": 12025, "gain": 0.57},
        "small_detail": {"freq_core_frac": 0.080, "octaves": 4, "seed_offset": 12030, "gain": 0.50},
    },
    "masks": {
        "crest_near_smoothstep_m_core_frac": [0.055, 0.34],
        "channel_near_smoothstep_m_core_frac": [0.025, 0.14],
        "route_axis_smoothstep": [0.50, 0.76],
        "routed_cut_weights": {"channel_axis": 0.58, "channel_near": 0.30, "tributary": 0.12},
        "routed_cut_channel_axis_smoothstep": [0.13, 0.62],
        "wet_floor_weights": {"channel_axis": 0.64, "discharge": 0.36},
        "wet_floor_discharge_smoothstep": [0.14, 0.54],
        "highland_mask_uplift_smoothstep": [0.36, 0.78],
    },
    "height_terms": {
        "uplift_minus_0_46": 1.52,
        "crest_near_highland": 0.34,
        "shoulder_detail_highland": 0.22,
        "ridge_detail_highland": 0.14,
        "small_detail_not_wet_floor": 0.065,
        "routed_cut_depth": [0.52, 0.42],
        "route_axis_depth": [0.18, 0.70, 0.30],
        "wet_floor_lowland_depth": 0.18,
        "ridge_detail_unrouted": 0.09,
        "final_tanh_gain": 1.18,
    },
    "corridor_mask": {
        "channel_axis_threshold": 0.16,
        "channel_distance_spacing_mul": 6.0,
        "route_axis_threshold": 0.22,
    },
}


def _round(value: Any, digits: int = 6) -> Any:
    if isinstance(value, (np.floating, float)):
        return round(float(value), digits)
    if isinstance(value, (np.integer, int)):
        return int(value)
    if isinstance(value, (np.bool_, bool)):
        return bool(value)
    return value


def _chunk_window_arrays(seed: int, chunk_x: int, chunk_z: int) -> dict[str, Any]:
    spec = chunks._window_spec(FIXTURE_CHUNK_N, chunks.CHUNK_SPAN_M)
    origin_x = chunks.WORLD_ORIGIN_X_M + float(chunk_x) * chunks.CHUNK_SPAN_M
    origin_z = chunks.WORLD_ORIGIN_Z_M + float(chunk_z) * chunks.CHUNK_SPAN_M
    window = win.build_skeleton_window(origin_x, origin_z, int(seed), spec)
    full_height, full_corridor = chunks._compose_windowed_height(window, int(seed), spec)
    core_slice = win._core_slice(spec)
    return {
        "spec": spec,
        "origin_x": origin_x,
        "origin_z": origin_z,
        "height": full_height[core_slice, core_slice],
        "corridor": full_corridor[core_slice, core_slice],
        "facts": win.core_facts(window, spec),
    }


def _sample_records() -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    cache: dict[tuple[int, int, int], dict[str, Any]] = {}
    for seed, chunk_x, chunk_z, grid_x, grid_z in SAMPLE_POINTS:
        key = (int(seed), int(chunk_x), int(chunk_z))
        if key not in cache:
            cache[key] = _chunk_window_arrays(int(seed), int(chunk_x), int(chunk_z))
        entry = cache[key]
        spec: win.SkeletonWindowSpec = entry["spec"]
        world_x = float(entry["origin_x"]) + float(grid_x) * float(spec.spacing_m)
        world_z = float(entry["origin_z"]) + float(grid_z) * float(spec.spacing_m)
        facts = {
            field: _round(np.asarray(entry["facts"][field], dtype=np.float64)[grid_z, grid_x])
            for field in win.FACT_FIELDS
        }
        h = float(np.asarray(entry["height"], dtype=np.float64)[grid_z, grid_x])
        records.append(
            {
                "seed": int(seed),
                "chunk_x": int(chunk_x),
                "chunk_z": int(chunk_z),
                "grid_x": int(grid_x),
                "grid_z": int(grid_z),
                "world_x_m": _round(world_x, 3),
                "world_z_m": _round(world_z, 3),
                "height_normalized": _round(h),
                "height_m_review": _round(h * trav.BASE_HEIGHT_SCALE_M, 4),
                "corridor": bool(np.asarray(entry["corridor"], dtype=bool)[grid_z, grid_x]),
                "facts": facts,
            }
        )
    return records


def _contact_sheet_digest(payload: dict[str, Any]) -> dict[str, Any]:
    panels = chunk_render.panels_for_payload(payload, panel_px=FIXTURE_CONTACT_PANEL_PX)
    sheet = chunk_render.contact_sheet(panels, cols=4, gutter=4)
    data = io.BytesIO()
    sheet.save(data, format="PNG")
    return {
        "renderer": "render_rough_world_chunks_review.contact_sheet",
        "panel_px": FIXTURE_CONTACT_PANEL_PX,
        "cols": 4,
        "gutter": 4,
        "size_px": [int(sheet.size[0]), int(sheet.size[1])],
        "png_sha256": hashlib.sha256(data.getvalue()).hexdigest(),
    }


def _summary(payload: dict[str, Any]) -> dict[str, Any]:
    seam_rows = chunks.seam_rows(payload)
    visual_rows = chunks.visual_seam_rows(payload)
    variation_rows = chunks.variation_rows(payload)
    travel_rows = chunks.virtual_travel_summary_rows(seeds=FIXTURE_SEEDS, chunk_count=5, chunk_n=FIXTURE_CHUNK_N)
    return {
        "seams": {
            "rows": len(seam_rows),
            "height_max_abs_delta": _round(max(float(row["height_max_abs_delta"]) for row in seam_rows)),
            "corridor_min_match_frac": _round(min(float(row["corridor_match_frac"]) for row in seam_rows)),
        },
        "visual_seams": {
            "rows": len(visual_rows),
            "height_max_delta_m": _round(max(float(row["height_max_delta_m"]) for row in visual_rows)),
            "normal_max_angle_deg": _round(max(float(row["normal_max_angle_deg"]) for row in visual_rows)),
            "slope_max_abs_delta": _round(max(float(row["slope_max_abs_delta"]) for row in visual_rows)),
            "terrain_color_max_delta": _round(max(float(row["terrain_color_max_delta"]) for row in visual_rows)),
            "corridor_edge_mismatch_count": int(max(int(row["corridor_edge_mismatch_count"]) for row in visual_rows)),
        },
        "variation": [
            {
                "kind": row["kind"],
                "seed": str(row["seed"]),
                "a": row["a"],
                "b": row["b"],
                "mean_abs_delta": _round(row["mean_abs_delta"]),
                "corrcoef": _round(row["corrcoef"]),
            }
            for row in variation_rows
        ],
        "virtual_travel": [
            {
                "seed": int(row["seed"]),
                "chunk_count": int(row["chunk_count"]),
                "world_span_km": _round(row["world_span_km"], 3),
                "seams_count": int(row["seams_count"]),
                "height_max_abs_delta": _round(row["height_max_abs_delta"]),
                "corridor_min_match_frac": _round(row["corridor_min_match_frac"]),
                "adjacent_mean_abs_delta_median": _round(row["adjacent_mean_abs_delta_median"]),
                "adjacent_corrcoef_max": _round(row["adjacent_corrcoef_max"]),
            }
            for row in travel_rows
        ],
    }


def build_contract() -> dict[str, Any]:
    payload = chunks.build_payload(
        seeds=FIXTURE_SEEDS,
        chunk_count=3,
        chunk_n=FIXTURE_CHUNK_N,
        chunk_span_m=chunks.CHUNK_SPAN_M,
        origin_x_m=chunks.WORLD_ORIGIN_X_M,
        origin_z_m=chunks.WORLD_ORIGIN_Z_M,
    )
    spec = chunks._window_spec(FIXTURE_CHUNK_N, chunks.CHUNK_SPAN_M)
    return {
        "schema_version": 1,
        "keeper_id": KEEPER_ID,
        "status": "candidate_contract_owner_direction_and_seams_accepted_not_runtime_port",
        "generator_version": chunks.GENERATOR_VERSION,
        "scenario_key": chunks.SCENARIO.key,
        "scenario_label": chunks.SCENARIO.label,
        "source_modules": {
            "height_contract": "tools/dem_pack/export_godot_rough_world_chunks.py::_compose_windowed_height",
            "skeleton_window": "tools/dem_pack/geography_skeleton_windows.py",
            "review_scene": "wg-10/worldgen_terrain/harness/rough_world_chunks_review.tscn",
        },
        "constants": {
            "seeds": list(FIXTURE_SEEDS),
            "chunk_span_m": chunks.CHUNK_SPAN_M,
            "review_chunk_n": chunks.CHUNK_N,
            "fixture_chunk_n": FIXTURE_CHUNK_N,
            "fixture_spacing_m": _round(spec.spacing_m, 3),
            "window_apron_m": chunks.WINDOW_APRON_M,
            "world_origin_x_m": chunks.WORLD_ORIGIN_X_M,
            "world_origin_z_m": chunks.WORLD_ORIGIN_Z_M,
            "review_height_scale_m": trav.BASE_HEIGHT_SCALE_M,
            "review_relief_policy": "k0_fixed_vertical_relief_for_chunk_scene",
        },
        "facts_contract": {
            "public_runtime_candidates": list(win.FACT_FIELDS),
            "height_private_material_fields": ["ridge_detail", "shoulder_detail", "route_texture", "small_detail"],
            "review_only_overlays": ["terrain_color", "slope_bands", "seam_guides"],
            "authoritative_window_rule": "sample from world-anchored core window; apron is for routing/smoothing/normals, not an owner of neighboring core samples",
        },
        "height_composition_contract": HEIGHT_COMPOSITION_CONTRACT,
        "sample_points": _sample_records(),
        "chunk_contract_summary": _summary(payload),
        "golden_review_contact_sheet": _contact_sheet_digest(payload),
    }


def main() -> None:
    FIXTURE_PATH.parent.mkdir(parents=True, exist_ok=True)
    contract = build_contract()
    FIXTURE_PATH.write_text(json.dumps(contract, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"wrote {FIXTURE_PATH}")
    print(f"keeper_id={contract['keeper_id']} generator={contract['generator_version']}")
    print(f"contact_sha256={contract['golden_review_contact_sheet']['png_sha256']}")


if __name__ == "__main__":
    main()
