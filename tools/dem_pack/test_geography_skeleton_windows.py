import numpy as np

import analyze_geography_skeleton_windows as report
import geography_skeleton_windows as win


def _small_spec() -> win.SkeletonWindowSpec:
    return win.SkeletonWindowSpec(core_span_m=45000.0, apron_m=18000.0, spacing_m=1500.0)


def test_window_origin_is_fixed_to_world_coordinates():
    spec = _small_spec()
    assert win.window_origin_for(100.0, 100.0, spec) == (0.0, 0.0)
    assert win.window_origin_for(44999.0, -1.0, spec) == (0.0, -45000.0)
    assert win.window_origin_for(45000.0, 45000.0, spec) == (45000.0, 45000.0)


def test_skeleton_window_is_deterministic_finite_and_nonflat():
    spec = _small_spec()
    a = win.build_skeleton_window(0.0, 0.0, seed=211, spec=spec)
    b = win.build_skeleton_window(0.0, 0.0, seed=211, spec=spec)
    for field in win.FACT_FIELDS:
        av = np.asarray(a[field])
        bv = np.asarray(b[field])
        assert np.all(np.isfinite(av))
        assert np.allclose(av, bv)
        assert float(np.ptp(av)) > 0.01


def test_core_facts_crop_out_apron_and_keep_shared_shape():
    spec = _small_spec()
    window = win.build_skeleton_window(-45000.0, 90000.0, seed=212, spec=spec)
    core = win.core_facts(window, spec)
    expected_n = int(round(spec.core_span_m / spec.spacing_m)) + 1
    for field in win.FACT_FIELDS:
        assert core[field].shape == (expected_n, expected_n)


def test_adjacent_window_seams_are_bounded_for_local_facts():
    spec = _small_spec()
    for axis in ("x", "z"):
        deltas = win.adjacent_seam_deltas(seed=213, spec=spec, axis=axis)
        assert deltas["uplift"] < 0.030
        assert deltas["routed_surface"] < 0.055
        assert deltas["discharge"] < 0.130
        assert deltas["tributary"] < 0.220
        assert deltas["channel_axis"] < 0.220


def test_distance_fact_seams_are_scaled_not_pixel_artifacts():
    spec = _small_spec()
    deltas = win.adjacent_seam_deltas(seed=214, spec=spec, axis="x")
    # Distance fields can legitimately differ near a boundary when a crest/channel centerline falls just
    # outside one window's apron. They still need to stay within a small coarse-window fraction.
    assert deltas["crest_dist"] / spec.core_span_m < 0.24
    assert deltas["channel_dist"] / spec.core_span_m < 0.24


def test_window_seam_report_rows_are_finite_and_complete():
    spec = _small_spec()
    rows = report.seam_rows(spec=spec, seeds=(215,), origins=((0.0, 0.0),))
    assert len(rows) == 2
    for row in rows:
        for field in win.FACT_FIELDS:
            assert np.isfinite(float(row[field]))
        assert 0.0 <= float(row["crest_dist_core_frac"]) < 1.0
        assert 0.0 <= float(row["channel_dist_core_frac"]) < 1.0


def test_default_window_seam_report_is_within_current_phase7b_thresholds():
    rows = report.seam_rows(seeds=(211, 213), origins=((0.0, 0.0), (90000.0, -90000.0)))
    for row in rows:
        assert float(row["uplift"]) <= 0.001
        assert float(row["routed_surface"]) <= 0.001
        assert float(row["discharge"]) < 0.020
        assert float(row["tributary"]) < 0.035
        assert float(row["channel_axis"]) < 0.050
        assert float(row["crest_dist_core_frac"]) < 0.001
        assert float(row["channel_dist_core_frac"]) < 0.001
