import numpy as np

import export_godot_rough_world_chunks as chunks
import render_rough_world_chunks_review as chunk_render


def _small_payload():
    return chunks.build_payload(
        seeds=(133, 211),
        chunk_count=3,
        chunk_n=49,
        chunk_span_m=25_600.0,
        coarse_n=64,
    )


def test_chunk_payload_is_deterministic_and_seed_variable():
    a = _small_payload()
    b = _small_payload()
    assert a["generator_version"] == chunks.GENERATOR_VERSION
    assert a["generator_version"].endswith("independent_windows")
    assert a["chunk_count"] == 3
    assert a["chunk_span_m"] == 25_600.0
    assert a["window_apron_m"] == 25_600.0
    assert np.allclose(a["seeds"][0]["height"], b["seeds"][0]["height"])
    assert np.allclose(a["seeds"][0]["corridor"], b["seeds"][0]["corridor"])

    first = np.asarray(a["seeds"][0]["height"], dtype=np.float64)
    second = np.asarray(a["seeds"][1]["height"], dtype=np.float64)
    assert float(np.mean(np.abs(first - second))) > 0.02


def test_adjacent_chunks_are_different_but_share_exact_edges():
    payload = _small_payload()
    grid = chunks._chunk_grid(payload["seeds"][0], int(payload["chunk_count"]))
    center = chunks._height_array(grid[1][1])
    east = chunks._height_array(grid[1][2])
    south = chunks._height_array(grid[2][1])
    center_corridor = np.asarray(grid[1][1]["corridor"], dtype=np.float64).reshape(center.shape)
    east_corridor = np.asarray(grid[1][2]["corridor"], dtype=np.float64).reshape(east.shape)
    south_corridor = np.asarray(grid[2][1]["corridor"], dtype=np.float64).reshape(south.shape)

    assert grid[1][1]["source"] == "independent_window"
    assert not np.allclose(center, east)
    assert not np.allclose(center, south)
    assert np.allclose(center[:, -1], east[:, 0])
    assert np.allclose(center[-1, :], south[0, :])
    assert np.allclose(center_corridor[:, -1], east_corridor[:, 0])
    assert np.allclose(center_corridor[-1, :], south_corridor[0, :])


def test_same_world_coordinate_is_independent_of_request_origin():
    a = chunks._build_independent_chunk(
        133,
        chunk_x=1,
        chunk_z=2,
        chunk_count=3,
        chunk_n=33,
        chunk_span_m=25_600.0,
        world_origin_x_m=60_000.0,
        world_origin_z_m=36_000.0,
    )
    b = chunks._build_independent_chunk(
        133,
        chunk_x=0,
        chunk_z=0,
        chunk_count=5,
        chunk_n=33,
        chunk_span_m=25_600.0,
        world_origin_x_m=85_600.0,
        world_origin_z_m=87_200.0,
    )
    assert a["world_origin_x_m"] == b["world_origin_x_m"]
    assert a["world_origin_z_m"] == b["world_origin_z_m"]
    assert np.allclose(a["height"], b["height"])
    assert np.allclose(a["apron_height"], b["apron_height"])
    assert np.allclose(a["corridor"], b["corridor"])


def test_chunk_seam_report_proves_height_and_corridor_continuity():
    payload = _small_payload()
    rows = chunks.seam_rows(payload)
    assert len(rows) == 24
    assert max(float(row["height_max_abs_delta"]) for row in rows) <= 1e-9
    assert min(float(row["corridor_match_frac"]) for row in rows) >= 0.90


def test_visual_seam_report_matches_godot_edge_math():
    payload = _small_payload()
    rows = chunks.visual_seam_rows(payload)
    assert len(rows) == 24
    assert max(float(row["height_max_delta_m"]) for row in rows) <= 1e-6
    assert max(float(row["normal_max_angle_deg"]) for row in rows) <= 0.01
    assert max(float(row["slope_max_abs_delta"]) for row in rows) <= 1e-6
    assert max(float(row["terrain_color_max_delta"]) for row in rows) <= 1e-6
    assert max(int(row["corridor_edge_mismatch_count"]) for row in rows) == 0


def test_chunk_contact_sheet_renderer_builds_expected_panels():
    payload = _small_payload()
    panels = chunk_render.panels_for_payload(payload, panel_px=96)
    assert len(panels) == 8
    for panel in panels:
        assert panel.size == (96, 96)
    sheet = chunk_render.contact_sheet(panels, cols=4, gutter=4)
    assert sheet.size == (404, 204)


def test_variation_report_distinguishes_adjacent_chunks_and_seeds():
    payload = _small_payload()
    rows = chunks.variation_rows(payload)
    assert {row["kind"] for row in rows} == {"adjacent_chunk", "seed_pair"}
    for row in rows:
        assert np.isfinite(float(row["mean_abs_delta"]))
        assert float(row["mean_abs_delta"]) > 0.02
        assert -1.0 <= float(row["corrcoef"]) <= 1.0


def test_virtual_travel_audit_extends_beyond_review_3x3():
    rows = chunks.virtual_travel_summary_rows(seeds=(133, 211), chunk_count=5, chunk_n=33)
    assert len(rows) == 2
    for row in rows:
        assert row["kind"] == "virtual_travel_summary"
        assert row["chunk_count"] == 5
        assert row["world_span_km"] == 128.0
        assert row["seams_count"] == 40
        assert float(row["height_max_abs_delta"]) <= 2e-4
        assert float(row["corridor_min_match_frac"]) >= 0.90
        assert float(row["adjacent_mean_abs_delta_min"]) > 0.02
        assert float(row["adjacent_corrcoef_max"]) < 0.98


def test_independent_window_diagnostic_exposes_current_boundary():
    rows = chunks.independent_window_diagnostic_rows(seeds=(133,), chunk_n=33, coarse_n=64)
    assert len(rows) == 2
    for row in rows:
        assert row["kind"] == "independent_window_diagnostic"
        assert float(row["conditioned_height_max_abs_delta"]) > 0.10
        assert float(row["conditioned_height_mean_abs_delta"]) > 0.03
