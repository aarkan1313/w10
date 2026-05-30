import numpy as np
import pytest
import worldgen_proto as wg


def _grid(n=64, span=4000.0, ox=0.0, oz=0.0):
    ii = np.linspace(0, span, n)
    return np.meshgrid(ii + ox, ii + oz)


def test_value_noise_range_and_determinism():
    wx, wz = _grid()
    a = wg.value_noise(wx, wz, seed=1)
    b = wg.value_noise(wx, wz, seed=1)
    assert a.shape == wx.shape
    assert np.allclose(a, b)                       # deterministic
    assert a.min() >= -1.0001 and a.max() <= 1.0001  # in [-1,1]
    assert float(a.max() - a.min()) > 0.1          # not flat


def test_fbm_is_multi_octave_and_bounded():
    wx, wz = _grid()
    h = wg.fbm(wx, wz, base_freq=1.0/2000.0, octaves=6, seed=2)
    assert h.shape == wx.shape
    assert np.all(np.isfinite(h))
    assert h.min() >= -1.5 and h.max() <= 1.5      # normalized fbm stays bounded


def test_ridged_fbm_is_nonnegative_and_ridgey():
    # ridged = 1-|noise| -> in [0,1], biased high (ridge crests), distinct from plain fbm.
    wx, wz = _grid()
    r = wg.ridged_fbm(wx, wz, base_freq=1.0/2000.0, octaves=4, seed=3)
    assert r.min() >= -0.0001 and r.max() <= 1.0001
    assert float(r.mean()) > 0.3                    # ridged noise sits high (crest-biased)


def test_domain_warp_displaces_the_field():
    # warping the coords must CHANGE the sampled field (vs unwarped), proving warp is active.
    wx, wz = _grid()
    plain = wg.fbm(wx, wz, base_freq=1.0/2000.0, octaves=4, seed=4)
    wxx, wzz = wg.domain_warp(wx, wz, warp_amount=1500.0, warp_freq=1.0/6000.0, seed=4)
    warped = wg.fbm(wxx, wzz, base_freq=1.0/2000.0, octaves=4, seed=4)
    assert wxx.shape == wx.shape
    assert not np.allclose(plain, warped)           # warp actually bent space
    # warp_amount=0 is a no-op (back-compat / off switch)
    wx0, wz0 = wg.domain_warp(wx, wz, warp_amount=0.0, warp_freq=1.0/6000.0, seed=4)
    assert np.allclose(wx0, wx) and np.allclose(wz0, wz)


MOUNTAIN = {
    "relief_m": 1200.0,
    "octave_amps": [1.0, 0.5, 0.25, 0.12, 0.06, 0.03],
    "ridge_strength": 0.9, "valley_depth": 0.5, "warp_amount": 2500.0,
    "base_freq": 1.0/3000.0, "ridge_freq": 1.0/1500.0,
    "valley_freq": 1.0/2500.0, "warp_freq": 1.0/8000.0,
}
PLAINS = {
    "relief_m": 180.0,
    "octave_amps": [1.0, 0.4, 0.18, 0.08, 0.03, 0.01],
    "ridge_strength": 0.05, "valley_depth": 0.15, "warp_amount": 1200.0,
    "base_freq": 1.0/4000.0, "ridge_freq": 1.0/1500.0,
    "valley_freq": 1.0/3000.0, "warp_freq": 1.0/9000.0,
}


def test_generate_deterministic_finite_relief_scaled():
    wx, wz = _grid(96, span=20000.0)
    a = wg.generate(wx, wz, MOUNTAIN, seed=5)
    b = wg.generate(wx, wz, MOUNTAIN, seed=5)
    assert a.shape == wx.shape
    assert np.allclose(a, b)                          # deterministic
    assert np.all(np.isfinite(a))
    # mountain (relief 1200, ridge_strength 0.9) has much more vertical range than plains
    p = wg.generate(wx, wz, PLAINS, seed=5)
    assert float(np.ptp(a)) > 3.0 * float(np.ptp(p))


def test_generate_bounded_by_closed_form():
    # |h| before relief <= Σoctave_amps + ridge_strength + valley_depth ; ×relief is the ceiling.
    wx, wz = _grid(96, span=20000.0)
    a = wg.generate(wx, wz, MOUNTAIN, seed=5)
    ceiling = (sum(MOUNTAIN["octave_amps"]) + MOUNTAIN["ridge_strength"] + MOUNTAIN["valley_depth"]) * MOUNTAIN["relief_m"]
    assert np.all(np.abs(a) <= ceiling * 1.01)


def test_generate_no_tiling_autocorrelation():
    # NON-REPETITION (the owner's "no chunks/squares/lines" bar): sample a long 1-D world transect
    # and assert its autocorrelation has NO strong peak at any candidate tiling period (e.g. the old
    # 8192 m page span or the kernel footprints). A tiled field would spike at its period.
    n = 4096
    span = 400000.0                                   # 400 km transect
    xs = np.linspace(0, span, n)
    wx = xs.reshape(1, -1); wz = np.zeros_like(wx)
    line = wg.generate(wx, wz, MOUNTAIN, seed=5).ravel()
    line = line - line.mean()
    ac = np.correlate(line, line, mode="full")[n-1:]  # autocorr, lags 0..n-1
    ac = ac / ac[0]
    step = span / n                                   # metres per lag
    # check candidate tiling periods (page span 8192 m, kernel footprints ~50-220 km)
    for period_m in (8192.0, 16384.0, 50000.0, 100000.0):
        lag = int(round(period_m / step))
        if 2 <= lag < n:
            assert ac[lag] < 0.5, f"autocorr spike {ac[lag]:.2f} at {period_m} m -> tiling/repeat!"
