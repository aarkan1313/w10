import numpy as np

import analyze_rough_world_traversability as trav


def test_slope_grid_flat_is_zero():
    h = np.zeros((9, 9), dtype=np.float64)
    s = trav.slope_grid(h, scene_width_m=80.0, height_scale_m=10.0)
    assert np.allclose(s, 0.0)


def test_slope_grid_ramp_matches_rise_over_run():
    n = 11
    scene_width = 100.0
    cell = scene_width / (n - 1)
    target_slope = 0.25
    height_scale = 20.0
    x = np.arange(n, dtype=np.float64)
    h = np.tile(x * target_slope * cell / height_scale, (n, 1))
    s = trav.slope_grid(h, scene_width_m=scene_width, height_scale_m=height_scale)
    assert np.allclose(s[:, 1:-1], target_slope)


def test_relief_exponent_controls_scale_slope_law():
    n = 17
    x = np.arange(n, dtype=np.float64)
    h = np.tile(x / float(n - 1), (n, 1))
    small = trav.BASE_WORLD_SIZE_M * 10.0
    large = trav.BASE_WORLD_SIZE_M * 200.0

    small_k0 = np.median(trav.slope_grid(h, small, trav.height_scale_for(small, relief_exponent=0.0)))
    large_k0 = np.median(trav.slope_grid(h, large, trav.height_scale_for(large, relief_exponent=0.0)))
    assert np.isclose(small_k0 / large_k0, 20.0)

    small_k1 = np.median(trav.slope_grid(h, small, trav.height_scale_for(small, relief_exponent=1.0)))
    large_k1 = np.median(trav.slope_grid(h, large, trav.height_scale_for(large, relief_exponent=1.0)))
    assert np.isclose(small_k1 / large_k1, 1.0)


def test_component_stats_reports_crossing_largest_component():
    mask = np.zeros((5, 6), dtype=bool)
    mask[2, :] = True
    mask[0, 0] = True
    stats = trav.component_stats(mask)
    assert stats["component_count"] == 2
    assert stats["largest_crosses_we"]
    assert not stats["largest_crosses_ns"]
    assert stats["largest_touches_edges"] == 2


def test_audit_item_rejects_flat_world_as_structural_candidate():
    n = 17
    item = {
        "key": "flat",
        "label": "Flat",
        "kind": "synth",
        "span_km": 90.0,
        "n": n,
        "height": np.zeros((n, n), dtype=np.float64).ravel().tolist(),
    }
    row = trav.audit_item(item, scale=200.0)
    assert row["legacy_grade"] == "candidate"
    assert row["grade"] == "flat"
    assert row["passable_frac"] == 1.0
    assert row["largest_passable_frac"] == 1.0
    assert row["largest_crosses_we"]
    assert row["largest_crosses_ns"]


def test_structural_corridor_grade_accepts_low_route_through_relief():
    n = 41
    yy, xx = np.mgrid[0:n, 0:n].astype(np.float64)
    x = (xx / float(n - 1)) * 2.0 - 1.0
    z = (yy / float(n - 1)) * 2.0 - 1.0
    valley = 0.75 * np.abs(z) + 0.20 * np.sin(x * np.pi)
    ridges = 0.55 * np.abs(x)
    h = (valley + ridges)
    h = (h - np.mean(h)) / max(np.std(h), 1e-9)
    item = {
        "key": "valley",
        "label": "Valley",
        "kind": "synth",
        "span_km": 25.6,
        "n": n,
        "height": h.ravel().tolist(),
    }
    row = trav.audit_item(item, scale=200.0)
    assert row["grade"] in {"candidate", "thin"}
    assert row["largest_low_corridor_frac"] > 0.12
    assert row["slope_p90"] >= trav.MIN_STRUCTURAL_SLOPE_P90
