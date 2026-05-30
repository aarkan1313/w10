import numpy as np
import pytest
import spectral


def test_analyze_signature_shape_and_keys():
    rng = np.random.default_rng(0)
    dem = rng.standard_normal((64, 64)).astype(np.float32)
    sig = spectral.analyze_signature(dem, spacing_m=90.0)
    assert set(sig.keys()) == {"amp_octaves", "base_freq_per_m", "relief_m"}
    assert len(sig["amp_octaves"]) == spectral.N_OCTAVES
    assert all(np.isfinite(a) and a >= 0.0 for a in sig["amp_octaves"])
    assert sig["base_freq_per_m"] > 0.0


def test_analyze_signature_smooth_has_low_high_octaves():
    xs = np.linspace(0, 2 * np.pi, 128)
    smooth = np.outer(np.sin(xs), np.sin(xs)).astype(np.float32)
    rng = np.random.default_rng(1)
    rough = rng.standard_normal((128, 128)).astype(np.float32)
    s_smooth = spectral.analyze_signature(smooth, spacing_m=90.0)["amp_octaves"]
    s_rough = spectral.analyze_signature(rough, spacing_m=90.0)["amp_octaves"]
    smooth_hi_ratio = s_smooth[-1] / max(s_smooth[0], 1e-9)
    rough_hi_ratio = s_rough[-1] / max(s_rough[0], 1e-9)
    assert rough_hi_ratio > smooth_hi_ratio


def test_analyze_signature_rejects_degenerate():
    with pytest.raises(ValueError):
        spectral.analyze_signature(np.zeros((32, 32), dtype=np.float32), spacing_m=90.0)
    with pytest.raises(ValueError):
        bad = np.full((32, 32), np.nan, dtype=np.float32)
        spectral.analyze_signature(bad, spacing_m=90.0)
    with pytest.raises(ValueError):
        spectral.analyze_signature(np.ones((32, 32), dtype=np.float32), spacing_m=0.0)
