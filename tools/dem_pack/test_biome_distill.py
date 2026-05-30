import numpy as np
import pytest
import biome_distill as bd


def _ridged(n=128, period=16):
    # parallel linear ridges along z (1-|sin|) -> strongly linear/anisotropic, high crests
    x = np.arange(n)
    line = 1.0 - np.abs(np.sin(2 * np.pi * x / period))
    return np.tile(line.reshape(1, -1), (n, 1)).astype(np.float32)


def _flat_noise(n=128, seed=0):
    rng = np.random.default_rng(seed)
    return rng.standard_normal((n, n)).astype(np.float32) * 0.01  # near-flat, isotropic


def _carved(n=128):
    # a single deep valley trench down the middle, flat elsewhere -> high incision
    a = np.zeros((n, n), dtype=np.float32)
    a[:, n // 2 - 2:n // 2 + 2] = -1.0
    return a


def test_to_metres_rescales_span_to_height_range():
    z = np.array([[-2.0, 0.0], [1.0, 3.0]], dtype=np.float32)  # span = 5 sigma
    m = bd.to_metres(z, height_range_m=1000.0)
    assert np.isclose(float(m.max() - m.min()), 1000.0)        # span == real range
    assert np.all(np.isfinite(m))


def test_ridge_linearity_high_for_ridges_low_for_noise():
    r = bd.ridge_linearity(_ridged())
    f = bd.ridge_linearity(_flat_noise())
    assert 0.0 <= f <= 1.0 and 0.0 <= r <= 1.0
    assert r > f + 0.2                                          # ridges read as more linear


def test_incision_depth_high_for_carved_low_for_flat():
    c = bd.incision_depth(_carved(), spacing_m=90.0)
    fl = bd.incision_depth(np.zeros((128, 128), np.float32), spacing_m=90.0)
    assert c > fl                                               # carved trench has incision, flat has ~none


def test_anisotropy_high_for_directional_low_for_isotropic():
    a = bd.anisotropy_flow(_ridged())
    i = bd.anisotropy_flow(_flat_noise())
    assert 0.0 <= i <= 1.0 and 0.0 <= a <= 1.0
    assert a > i + 0.2                                          # directional terrain is more anisotropic


def test_bandpass_amp_profile_is_len6_normalized_finite():
    p = bd.bandpass_amp_profile(_ridged(), n_octaves=6)
    assert len(p) == 6
    assert np.all(np.isfinite(p))
    assert np.isclose(p[0], 1.0)                                # normalized so band 0 == 1.0
    assert np.all(np.asarray(p) >= 0.0)


_META = {"height_range_m": 1801.0, "approx_sample_spacing_m": 90.0, "mean_slope_deg": 12.5}


def test_metrics_deterministic_and_finite():
    z = _ridged()
    m1 = bd.metrics_for_dem(z, _META)
    m2 = bd.metrics_for_dem(z, _META)
    assert m1 == m2                                             # deterministic (pure)
    for k, v in m1.items():
        arr = np.asarray(v, dtype=float)
        assert np.all(np.isfinite(arr)), f"{k} not finite"


def test_metrics_use_metadata_for_relief_and_slope():
    # relief + slope come straight from the vetted metadata; structure is computed from z.
    z = _ridged()
    m = bd.metrics_for_dem(z, _META)
    assert m["relief_real_m"] == 1801.0          # from meta height_range_m, not computed
    assert m["slope_bias_deg"] == 12.5           # from meta mean_slope_deg, not computed
    # structure metrics ARE computed (present + in range)
    assert 0.0 <= m["ridge_linearity"] <= 1.0
    assert len(m["amp_profile"]) == bd.N_OCTAVES
