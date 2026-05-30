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


def _metrics(relief=1200.0, incis=120.0, slope=10.0, aniso=0.7, wl=10000.0):
    # incis/relief = carving SHAPE (height-independent); slope = steepness character.
    return {
        "relief_real_m": relief,
        "amp_profile": [1.0, 0.5, 0.25, 0.12, 0.06, 0.03],
        "ridge_linearity": 0.3,            # DEAD metric — present but must NOT drive ridge_strength
        "incision_depth_m": incis,
        "anisotropy": aniso,
        "dominant_wavelength_m": wl,        # DEAD metric — present but must NOT drive base_freq
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


def test_valley_depth_from_height_normalized_incision():
    # deeper carving RELATIVE TO RELIEF -> more valley_depth; height alone does not.
    shallow = bd.params_from_metrics(_metrics(relief=2000.0, incis=40.0))   # incis/relief = 0.02
    deep = bd.params_from_metrics(_metrics(relief=2000.0, incis=240.0))     # incis/relief = 0.12
    assert deep["valley_depth"] > shallow["valley_depth"] + 0.3
    # SAME shape ratio at DIFFERENT heights -> SAME valley_depth (height-independent)
    small = bd.params_from_metrics(_metrics(relief=500.0, incis=30.0))      # ratio 0.06
    big = bd.params_from_metrics(_metrics(relief=4000.0, incis=240.0))      # ratio 0.06
    assert abs(small["valley_depth"] - big["valley_depth"]) < 0.05


def test_ridge_strength_from_slope_not_height():
    # steeper -> more ridge_strength
    gentle = bd.params_from_metrics(_metrics(slope=2.0))
    steep = bd.params_from_metrics(_metrics(slope=20.0))
    assert steep["ridge_strength"] > gentle["ridge_strength"] + 0.3
    # THE TRAP GATE: a tall-but-smooth biome (high relief, LOW slope, low carving) must NOT read as ridgy
    tall_smooth = bd.params_from_metrics(_metrics(relief=4000.0, slope=2.0, incis=80.0))  # ratio 0.02
    assert tall_smooth["ridge_strength"] < 0.3      # height does not buy ridge_strength
    assert tall_smooth["valley_depth"] < 0.2        # nor valley_depth


def test_freqs_positive_and_derived_from_base():
    p = bd.params_from_metrics(_metrics())
    assert p["base_freq"] > 0
    assert np.isclose(p["ridge_freq"], bd.RIDGE_FREQ_RATIO * p["base_freq"])
    assert np.isclose(p["valley_freq"], bd.VALLEY_FREQ_RATIO * p["base_freq"])
    assert p["warp_freq"] > 0


def test_dead_metrics_do_not_drive_params():
    # changing the DEAD metrics (ridge_linearity, dominant_wavelength_m) must NOT change any param
    base = bd.params_from_metrics(_metrics())
    m2 = _metrics(); m2["ridge_linearity"] = 0.95; m2["dominant_wavelength_m"] = 99999.0
    changed = bd.params_from_metrics(m2)
    assert base == changed, "dead metrics still influence params — they must not"


def test_aggregate_median_is_per_metric_median():
    ms = [_metrics(relief=1000.0, slope=5.0), _metrics(relief=2000.0, slope=15.0),
          _metrics(relief=1500.0, slope=10.0)]
    agg = bd.aggregate_median(ms)
    assert agg["relief_real_m"] == 1500.0          # median of [1000,2000,1500]
    assert agg["slope_bias_deg"] == 10.0           # median of [5,15,10]
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
