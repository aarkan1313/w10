import json
from pathlib import Path

import numpy as np
import pytest

import mountain_world_layer as layer
import mountain_synthesis as mountain


NETWORK_PAYLOAD = Path("wg-10/worldgen_terrain/generated/review/mountain_network_chunks.json")
DISPLAY_PAGE_SPAN_M = 8192.0
SAMPLE_N = 65


def _load_network_payload() -> dict:
    if not NETWORK_PAYLOAD.exists():
        pytest.skip(
            "generated mountain_network_chunks.json is not checked in; run "
            "python tools/dem_pack/export_godot_mountain_network_chunks.py"
        )
    return json.loads(NETWORK_PAYLOAD.read_text(encoding="utf-8"))


def _accepted_reference_page(payload: dict, *, display_origin_x: float, display_origin_z: float) -> np.ndarray:
    return layer.sample_payload_page(
        payload,
        page_span_m=DISPLAY_PAGE_SPAN_M,
        sample_n=SAMPLE_N,
        display_origin_x_m=display_origin_x,
        display_origin_z_m=display_origin_z,
    )


def _live_seamsafe_page(payload: dict, *, display_origin_x: float, display_origin_z: float) -> np.ndarray:
    ratio = float(payload["source_scene_ratio"])
    source_span_m = DISPLAY_PAGE_SPAN_M * ratio
    source_origin_x, source_origin_z = layer.source_origin_for_display(
        payload,
        display_origin_x_m=display_origin_x,
        display_origin_z_m=display_origin_z,
    )
    spacing_m = source_span_m / float(SAMPLE_N - 1)
    apron_px = int(mountain.MOUNTAIN_APRON_PX)
    padded_n = SAMPLE_N + 2 * apron_px
    padded_span_m = source_span_m + 2.0 * float(apron_px) * spacing_m
    wx, wz = mountain.grid(
        padded_n,
        padded_span_m,
        ox=source_origin_x - float(apron_px) * spacing_m,
        oz=source_origin_z - float(apron_px) * spacing_m,
    )
    result = mountain.generate(
        wx,
        wz,
        seed=int(payload["seeds"][0]["seed"]),
        style=mountain.STYLES[0],
        feature_span_m=float(payload["feature_span_m"]),
        apron_px=apron_px,
        spacing_m=spacing_m,
        flow_on=True,
    )
    return np.asarray(result["height"], dtype=np.float64)


def _gap_metrics(reference: np.ndarray, live: np.ndarray) -> dict[str, float]:
    ref = np.asarray(reference, dtype=np.float64)
    got = np.asarray(live, dtype=np.float64)
    delta = np.abs(ref - got)
    corr = float(np.corrcoef(ref.ravel(), got.ravel())[0, 1])
    return {
        "mean_abs": float(np.mean(delta)),
        "p95_abs": float(np.percentile(delta, 95.0)),
        "peak_abs": float(np.max(delta)),
        "corr": corr,
        "ref_ptp": float(np.ptp(ref)),
        "live_ptp": float(np.ptp(got)),
    }


def test_mountain_world_layer_builder_declares_accepted_contract():
    payload = layer.build_network_payload(styles=mountain.STYLES[:1], chunk_count=3, chunk_n=33)

    assert payload["generator_version"] == layer.NETWORK_GENERATOR_VERSION
    assert payload["source_scope"] == "coherent_full_field_carved_with_pass_network_sliced_for_review"
    assert payload["chunk_count"] == 3
    assert payload["chunk_n"] == 33
    assert payload["feature_span_m"] == layer.FEATURE_SPAN_M
    assert payload["height_scale_m"] == layer.HEIGHT_SCALE_M
    assert np.isclose(payload["source_scene_ratio"], layer.SOURCE_CHUNK_SPAN_M / layer.DISPLAY_CHUNK_SPAN_M)
    assert len(payload["seeds"]) == 1
    assert len(payload["seeds"][0]["chunks"]) == 9
    assert payload["seeds"][0]["pass_network"]["routes"] > 0
    assert payload["seeds"][0]["material_hints"]["low_pass_hint_coverage"] > 0.0
    assert payload["seeds"][0]["material_hints"]["floor_hint_coverage"] > 0.0
    assert payload["seeds"][0]["material_hints"]["rock_hint_coverage"] > 0.0
    assert payload["seeds"][0]["material_hints"]["snow_hint_coverage"] > 0.0
    chunk = payload["seeds"][0]["chunks"][0]
    for field in layer.MATERIAL_HINT_FIELDS:
        values = np.asarray(chunk[field], dtype=np.float64)
        apron_values = np.asarray(chunk[f"apron_{field}"], dtype=np.float64)
        assert values.shape == (33 * 33,)
        assert apron_values.shape == (35 * 35,)
        assert np.all(np.isfinite(values))
        assert np.all((values >= 0.0) & (values <= 1.0))


def test_generated_mountain_network_payload_declares_accepted_world_layer_contract():
    payload = _load_network_payload()

    assert payload["source_scope"] == "coherent_full_field_carved_with_pass_network_sliced_for_review"
    assert payload["chunk_count"] == layer.CHUNK_COUNT
    assert payload["chunk_n"] == layer.CHUNK_N
    assert payload["feature_span_m"] == layer.FEATURE_SPAN_M
    assert payload["height_scale_m"] == layer.HEIGHT_SCALE_M
    assert np.isclose(payload["source_scene_ratio"], layer.SOURCE_CHUNK_SPAN_M / layer.DISPLAY_CHUNK_SPAN_M)


def test_runtime_page_sampler_owns_source_mapping_and_material_fields():
    payload = _load_network_payload()

    source_x, source_z = layer.source_origin_for_display(
        payload,
        display_origin_x_m=0.0,
        display_origin_z_m=0.0,
    )
    assert np.isclose(source_x, 207000.0)
    assert np.isclose(source_z, 176000.0)

    page = layer.sample_payload_page(
        payload,
        page_span_m=DISPLAY_PAGE_SPAN_M,
        sample_n=SAMPLE_N,
        display_origin_x_m=0.0,
        display_origin_z_m=0.0,
    )
    via_world = layer.sample_world_page(
        payload["seeds"][0],
        chunk_count=int(payload["chunk_count"]),
        chunk_n=int(payload["chunk_n"]),
        display_chunk_span_m=float(payload["chunk_span_m"]),
        page_span_m=DISPLAY_PAGE_SPAN_M,
        sample_n=SAMPLE_N,
        display_origin_x_m=0.0,
        display_origin_z_m=0.0,
    )
    floor = layer.sample_payload_page(
        payload,
        field="floor_hint",
        page_span_m=DISPLAY_PAGE_SPAN_M,
        sample_n=SAMPLE_N,
        display_origin_x_m=0.0,
        display_origin_z_m=0.0,
    )
    rock = layer.sample_payload_page(
        payload,
        field="rock_hint",
        page_span_m=DISPLAY_PAGE_SPAN_M,
        sample_n=SAMPLE_N,
        display_origin_x_m=0.0,
        display_origin_z_m=0.0,
    )

    assert page.shape == (SAMPLE_N, SAMPLE_N)
    assert np.max(np.abs(page - via_world)) <= 1.0e-12
    for field in (floor, rock):
        assert field.shape == (SAMPLE_N, SAMPLE_N)
        assert np.all(np.isfinite(field))
        assert np.min(field) >= 0.0
        assert np.max(field) <= 1.0
    assert np.mean(floor) > 0.0
    assert np.mean(rock) > 0.0


def test_live_seamsafe_mountain_page_is_not_yet_the_accepted_network_layer():
    payload = _load_network_payload()
    reference = _accepted_reference_page(payload, display_origin_x=0.0, display_origin_z=0.0)
    live = _live_seamsafe_page(payload, display_origin_x=0.0, display_origin_z=0.0)
    metrics = _gap_metrics(reference, live)

    print(
        "[mountain-layer-gap] "
        f"mean_abs={metrics['mean_abs']:.6f} "
        f"p95_abs={metrics['p95_abs']:.6f} "
        f"peak_abs={metrics['peak_abs']:.6f} "
        f"corr={metrics['corr']:.6f} "
        f"ref_ptp={metrics['ref_ptp']:.6f} "
        f"live_ptp={metrics['live_ptp']:.6f}"
    )

    assert metrics["ref_ptp"] > 1.0
    assert metrics["live_ptp"] > 0.5
    assert metrics["mean_abs"] > 0.20
    assert metrics["p95_abs"] > 0.45
    assert metrics["corr"] < 0.80


def test_material_hints_are_world_layer_fields_not_page_local_rederives():
    world = layer.build_network_world(mountain.STYLES[0], chunk_count=3, chunk_n=33)
    floor = layer.stitch_grid(world["chunks"], 3, 33, "floor_hint")
    rock = layer.stitch_grid(world["chunks"], 3, 33, "rock_hint")
    snow = layer.stitch_grid(world["chunks"], 3, 33, "snow_hint")
    low_pass = layer.stitch_grid(world["chunks"], 3, 33, "low_pass_hint")
    corridor = layer.stitch_grid(world["chunks"], 3, 33, "corridor")

    for field in (floor, rock, snow, low_pass):
        assert field.shape == floor.shape
        assert np.all(np.isfinite(field))
        assert np.min(field) >= 0.0
        assert np.max(field) <= 1.0

    assert np.mean(low_pass[corridor > 0.5]) > 0.99
    assert np.mean(floor[corridor > 0.5]) > 0.90
    assert np.mean(rock >= 0.5) > 0.01
    assert np.mean(snow >= 0.5) > 0.01
