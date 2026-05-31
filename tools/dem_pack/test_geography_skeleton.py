import numpy as np

import compare_geography_metrics as metrics
import export_godot_rough_world_review as rough_export
import geography_skeleton as skel
import geography_engine as geo
from render_geography_skeleton_focus import FOCUS as ROUGH_FOCUS


def test_skeleton_coarse_fields_are_deterministic_and_nonflat():
    wx, wz = geo.grid(96, 90000.0)
    a = skel.build_coarse_skeleton(wx, wz, seed=31, coarse_n=64)
    b = skel.build_coarse_skeleton(wx, wz, seed=31, coarse_n=64)
    for key in ("uplift", "discharge", "tributary", "channel_axis", "crest_dist", "channel_dist", "basin_seed"):
        assert np.all(np.isfinite(a[key]))
        assert np.allclose(a[key], b[key])
        assert float(np.ptp(a[key])) > 0.05


def test_skeleton_flow_uses_multi_flow_distribution_not_d8_integer_routing():
    wx, wz = geo.grid(96, 90000.0)
    skeleton = skel.build_coarse_skeleton(wx, wz, seed=35, coarse_n=64)
    acc = skeleton["flow_accum"]
    fractional = np.abs(acc - np.round(acc))
    assert float(np.mean(fractional > 1e-6)) > 0.10


def test_skeleton_height_is_deterministic_finite_and_nonflat():
    wx, wz = geo.grid(96, 90000.0)
    a = skel.compose_height(wx, wz, seed=32, scenario=skel.SCENARIOS[0], coarse_n=64)
    b = skel.compose_height(wx, wz, seed=32, scenario=skel.SCENARIOS[0], coarse_n=64)
    za = a["height"]
    zb = b["height"]
    assert za.shape == wx.shape
    assert np.all(np.isfinite(za))
    assert np.allclose(za, zb)
    assert float(np.ptp(za)) > 0.25


def test_skeleton_regime_weights_sum_and_are_skeleton_derived():
    wx, wz = geo.grid(80, 90000.0)
    result = skel.compose_height(wx, wz, seed=33, scenario=skel.SCENARIOS[2], coarse_n=64)
    weights = result["weights"]
    total = np.zeros_like(wx, dtype=np.float64)
    significant = np.zeros_like(wx, dtype=np.int32)
    for weight in weights:
        assert weight.shape == wx.shape
        assert np.all(weight >= -1e-9)
        total += weight
        significant += weight > 0.08
    assert np.allclose(total, 1.0)
    assert float(np.mean(significant)) >= 2.0
    # Range weight should respond to skeleton uplift/crest fields, not just independent texture.
    uplift = result["skeleton"]["uplift"]
    range_w = weights[4]
    corr = np.corrcoef(uplift.ravel(), range_w.ravel())[0, 1]
    assert corr > 0.25


def test_skeleton_scenarios_change_output():
    wx, wz = geo.grid(80, 90000.0)
    base = skel.compose_height(wx, wz, seed=34, scenario=skel.SCENARIOS[0], coarse_n=64)["height"]
    rough = skel.compose_height(wx, wz, seed=34, scenario=skel.SCENARIOS[5], coarse_n=64)["height"]
    assert not np.allclose(base, rough)
    assert 0.0 <= skel.straight_artifact_score(base) <= 1.0


def test_skeleton_scenarios_change_process_weights_not_only_height_contrast():
    wx, wz = geo.grid(80, 90000.0)
    fan = skel.compose_height(wx, wz, seed=36, scenario=skel.SCENARIOS[1], coarse_n=64)["weights"]
    badlands = skel.compose_height(wx, wz, seed=36, scenario=skel.SCENARIOS[2], coarse_n=64)["weights"]
    filled = skel.compose_height(wx, wz, seed=36, scenario=skel.SCENARIOS[4], coarse_n=64)["weights"]
    assert float(np.mean(fan[1])) > float(np.mean(badlands[1]))
    assert float(np.mean(badlands[5])) > float(np.mean(fan[5]))
    assert float(np.mean(filled[0])) > float(np.mean(fan[0]))


def test_rough_focus_metrics_are_finite_for_every_variant():
    rows = metrics.synth_rows_skeleton_rough(45000.0, 80, 45000.0 / 79.0, coarse_n=64)
    assert len(rows) == len(ROUGH_FOCUS)
    assert {row["source"] for row in rows} == {scenario.key for scenario in ROUGH_FOCUS}
    numeric_keys = (
        "hypsometric_integral",
        "ptp_z",
        "relief_2km",
        "relief_10km",
        "relief_ratio_2_10",
        "slope_mean",
        "slope_p95",
        "slope_std",
        "slope_skew",
        "vrm_7px",
        "curv_abs_mean",
        "ridge_spacing_m",
        "valley_spacing_m",
        "highpass_std",
        "straight_score",
        "basin_prop",
        "fan_prop",
        "foothill_prop",
        "plateau_prop",
        "range_prop",
        "badlands_prop",
        "regime_entropy",
    )
    for row in rows:
        for key in numeric_keys:
            assert np.isfinite(float(row[key]))
        assert 0.0 <= float(row["straight_score"]) <= 1.0


def test_rough_world_export_item_contract_is_bounded_and_finite():
    z = np.linspace(-3.0, 4.0, 48 * 48, dtype=np.float64).reshape((48, 48))
    item = rough_export._item("unit", "Unit", "synth", z, 90000.0, "test")
    h = np.asarray(item["height"], dtype=np.float64)
    assert item["n"] == rough_export.N
    assert len(h) == rough_export.N * rough_export.N
    assert np.all(np.isfinite(h))
    assert float(np.min(h)) >= -1.0
    assert float(np.max(h)) <= 1.0
    assert item["stats"]["source_ptp"] > 0.0
