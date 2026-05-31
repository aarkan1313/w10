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


def test_component_stats_reports_crossing_largest_component():
    mask = np.zeros((5, 6), dtype=bool)
    mask[2, :] = True
    mask[0, 0] = True
    stats = trav.component_stats(mask)
    assert stats["component_count"] == 2
    assert stats["largest_crosses_we"]
    assert not stats["largest_crosses_ns"]
    assert stats["largest_touches_edges"] == 2


def test_audit_item_is_finite_and_grades_easy_flat_world_candidate():
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
    assert row["grade"] == "candidate"
    assert row["passable_frac"] == 1.0
    assert row["largest_passable_frac"] == 1.0
    assert row["largest_crosses_we"]
    assert row["largest_crosses_ns"]
