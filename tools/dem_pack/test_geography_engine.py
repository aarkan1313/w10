import numpy as np

import geography_engine as geo


def test_geography_engine_is_deterministic_finite_and_nonflat():
    wx, wz = geo.grid(96, 90000.0)
    a = geo.compose_height(wx, wz, seed=17, scenario=geo.SCENARIOS[0])
    b = geo.compose_height(wx, wz, seed=17, scenario=geo.SCENARIOS[0])
    za = a["height"]
    zb = b["height"]
    assert za.shape == wx.shape
    assert np.all(np.isfinite(za))
    assert np.allclose(za, zb)
    assert float(np.ptp(za)) > 0.25


def test_geography_engine_regime_weights_sum_and_mix():
    wx, wz = geo.grid(80, 90000.0)
    result = geo.compose_height(wx, wz, seed=18, scenario=geo.SCENARIOS[1])
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


def test_geography_engine_scenarios_change_landform_distribution():
    wx, wz = geo.grid(80, 90000.0)
    lowland = geo.compose_height(wx, wz, seed=19, scenario=geo.SCENARIOS[4])
    badlands = geo.compose_height(wx, wz, seed=19, scenario=geo.SCENARIOS[5])
    assert not np.allclose(lowland["height"], badlands["height"])
    lowland_range = float(np.mean(lowland["weights"][4]))
    badlands_badland = float(np.mean(badlands["weights"][5]))
    assert lowland_range < 0.35
    assert badlands_badland > float(np.mean(lowland["weights"][5]))


def test_straight_artifact_score_is_bounded_red_flag_metric():
    wx, wz = geo.grid(80, 90000.0)
    z = geo.compose_height(wx, wz, seed=20, scenario=geo.SCENARIOS[0])["height"]
    score = geo.straight_artifact_score(z)
    assert np.isfinite(score)
    assert 0.0 <= score <= 1.0

