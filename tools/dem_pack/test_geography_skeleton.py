import numpy as np

import geography_skeleton as skel
import geography_engine as geo


def test_skeleton_coarse_fields_are_deterministic_and_nonflat():
    wx, wz = geo.grid(96, 90000.0)
    a = skel.build_coarse_skeleton(wx, wz, seed=31, coarse_n=64)
    b = skel.build_coarse_skeleton(wx, wz, seed=31, coarse_n=64)
    for key in ("uplift", "discharge", "crest_dist", "channel_dist", "basin_seed"):
        assert np.all(np.isfinite(a[key]))
        assert np.allclose(a[key], b[key])
        assert float(np.ptp(a[key])) > 0.05


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

