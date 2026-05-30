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


import worldgen_proto as wg  # for the bounds + non-repetition checks


def _metrics(relief=1200.0, ridge=0.8, incis=300.0, aniso=0.7, wl=6000.0, slope=20.0):
    return {
        "relief_real_m": relief,
        "amp_profile": [1.0, 0.5, 0.25, 0.12, 0.06, 0.03],
        "ridge_linearity": ridge,
        "incision_depth_m": incis,
        "anisotropy": aniso,
        "dominant_wavelength_m": wl,
        "slope_bias_deg": slope,
    }


def test_params_from_metrics_has_all_generator_keys():
    p = bd.params_from_metrics(_metrics())
    for k in ("relief_m", "octave_amps", "ridge_strength", "valley_depth", "warp_amount",
              "base_freq", "ridge_freq", "valley_freq", "warp_freq", "slope_bias"):
        assert k in p, f"missing {k}"
    assert len(p["octave_amps"]) == bd.N_OCTAVES
    assert np.isclose(p["octave_amps"][0], 1.0)


def test_params_from_metrics_in_domain_and_finite():
    p = bd.params_from_metrics(_metrics())
    assert 0.0 <= p["ridge_strength"] <= bd.RIDGE_STRENGTH_MAX
    assert 0.0 <= p["valley_depth"] <= bd.VALLEY_DEPTH_MAX
    assert p["base_freq"] > 0 and p["ridge_freq"] > 0 and p["valley_freq"] > 0 and p["warp_freq"] > 0
    assert p["relief_m"] > 0
    for k, v in p.items():
        assert np.all(np.isfinite(np.asarray(v, dtype=float))), f"{k} not finite"
    # parity-readiness: every scalar f32-representable (round-trip stable)
    for k, v in p.items():
        if isinstance(v, (int, float)):
            assert float(np.float32(v)) == pytest.approx(v, rel=1e-5), f"{k} not f32-representable"


def test_freqs_derive_from_dominant_wavelength():
    p = bd.params_from_metrics(_metrics(wl=6000.0))
    assert np.isclose(p["base_freq"], 1.0 / 6000.0)
    assert np.isclose(p["ridge_freq"], bd.RIDGE_FREQ_RATIO / 6000.0)
    assert np.isclose(p["valley_freq"], bd.VALLEY_FREQ_RATIO / 6000.0)


def test_more_ridged_metrics_give_more_ridge_strength():
    lo = bd.params_from_metrics(_metrics(ridge=0.1))
    hi = bd.params_from_metrics(_metrics(ridge=0.9))
    assert hi["ridge_strength"] > lo["ridge_strength"]


def test_aggregate_median_is_per_metric_median():
    ms = [_metrics(relief=1000.0, ridge=0.2), _metrics(relief=2000.0, ridge=0.8),
          _metrics(relief=1500.0, ridge=0.5)]
    agg = bd.aggregate_median(ms)
    assert agg["relief_real_m"] == 1500.0          # median of [1000,2000,1500]
    assert agg["ridge_linearity"] == 0.5
    assert len(agg["amp_profile"]) == bd.N_OCTAVES  # per-band median


def test_generated_params_are_bounded():
    # the produced params must satisfy worldgen_proto's closed-form ceiling
    p = bd.params_from_metrics(_metrics())
    ii = np.linspace(0, 40000.0, 96)
    wx, wz = np.meshgrid(ii, ii)
    h = wg.generate(wx, wz, p, seed=5)
    ceiling = (sum(p["octave_amps"]) + p["ridge_strength"] + p["valley_depth"]) * p["relief_m"]
    assert np.all(np.abs(h) <= ceiling * 1.01)


def test_distilled_params_do_not_tile():
    # non-repetition (the owner's "no chunks/squares/lines" bar) on a REAL distilled-param field
    p = bd.params_from_metrics(_metrics())
    n = 4096
    span = 400000.0
    xs = np.linspace(0, span, n)
    wx = xs.reshape(1, -1); wz = np.zeros_like(wx)
    line = wg.generate(wx, wz, p, seed=5).ravel()
    line = line - line.mean()
    ac = np.correlate(line, line, mode="full")[n - 1:]
    ac = ac / ac[0]
    step = span / n
    for period_m in (8192.0, 16384.0, 50000.0, 100000.0):
        lag = int(round(period_m / step))
        if 2 <= lag < n:
            assert ac[lag] < 0.5, f"autocorr spike {ac[lag]:.2f} at {period_m} m -> tiling!"
