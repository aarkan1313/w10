import numpy as np

import export_godot_rough_world_chunks as chunks


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
    assert a["chunk_count"] == 3
    assert a["chunk_span_m"] == 25_600.0
    assert np.allclose(a["seeds"][0]["height"], b["seeds"][0]["height"])

    first = np.asarray(a["seeds"][0]["height"], dtype=np.float64)
    second = np.asarray(a["seeds"][1]["height"], dtype=np.float64)
    assert float(np.mean(np.abs(first - second))) > 0.02


def test_adjacent_chunks_are_different_but_share_exact_edges():
    payload = _small_payload()
    grid = chunks._chunk_grid(payload["seeds"][0], int(payload["chunk_count"]))
    center = chunks._height_array(grid[1][1])
    east = chunks._height_array(grid[1][2])
    south = chunks._height_array(grid[2][1])

    assert not np.allclose(center, east)
    assert not np.allclose(center, south)
    assert np.allclose(center[:, -1], east[:, 0])
    assert np.allclose(center[-1, :], south[0, :])


def test_chunk_seam_report_proves_height_and_corridor_continuity():
    payload = _small_payload()
    rows = chunks.seam_rows(payload)
    assert len(rows) == 24
    assert max(float(row["height_max_abs_delta"]) for row in rows) <= 1e-9
    assert min(float(row["corridor_match_frac"]) for row in rows) >= 0.90


def test_variation_report_distinguishes_adjacent_chunks_and_seeds():
    payload = _small_payload()
    rows = chunks.variation_rows(payload)
    assert {row["kind"] for row in rows} == {"adjacent_chunk", "seed_pair"}
    for row in rows:
        assert np.isfinite(float(row["mean_abs_delta"]))
        assert float(row["mean_abs_delta"]) > 0.02
        assert -1.0 <= float(row["corrcoef"]) <= 1.0


def test_independent_window_diagnostic_exposes_current_boundary():
    rows = chunks.independent_window_diagnostic_rows(seeds=(133,), chunk_n=33, coarse_n=64)
    assert len(rows) == 2
    for row in rows:
        assert row["kind"] == "independent_window_diagnostic"
        assert float(row["conditioned_height_max_abs_delta"]) > 0.10
        assert float(row["conditioned_height_mean_abs_delta"]) > 0.03
