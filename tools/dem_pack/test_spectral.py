import os

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


def test_synthesize_field_deterministic_and_nonflat():
    sig = {"amp_octaves": [1.0, 0.5, 0.25, 0.12, 0.06, 0.03, 0.015, 0.007],
           "base_freq_per_m": 1.0e-4, "relief_m": 1000.0}
    a = spectral.synthesize_field(sig, size=64, spacing_m=90.0, seed=7)
    b = spectral.synthesize_field(sig, size=64, spacing_m=90.0, seed=7)
    assert a.shape == (64, 64)
    assert np.allclose(a, b)
    assert float(a.max() - a.min()) > 0.0


def test_spectral_fidelity_roundtrip():
    rng = np.random.default_rng(3)
    n = 128
    xs = np.linspace(0, 2 * np.pi, n)
    dem = np.zeros((n, n), dtype=np.float64)
    for k, amp in [(1, 1.0), (2, 0.5), (4, 0.25), (8, 0.12), (16, 0.06)]:
        ph = rng.uniform(0, 2 * np.pi)
        dem += amp * np.outer(np.sin(k * xs + ph), np.sin(k * xs + ph))
    src = spectral.analyze_signature(dem, spacing_m=90.0)
    field = spectral.synthesize_field(src, size=256, spacing_m=90.0, seed=11)
    syn = spectral.analyze_signature(field, spacing_m=90.0)
    u = np.array(src["amp_octaves"]); v = np.array(syn["amp_octaves"])
    cos = float(np.dot(u, v) / (np.linalg.norm(u) * np.linalg.norm(v) + 1e-12))
    assert cos > 0.9, f"spectral fidelity too low: cos={cos:.3f} src={u} syn={v}"


REAL_KERNEL = os.path.join(
    os.path.dirname(__file__), "..", "..",
    "wg-10", "worldgen_terrain", "packs", "dem_v1", "kernels",
    "badlands__cop30_badlands_grand_canyon_112_1_36_1.npy")


@pytest.mark.skipif(not os.path.exists(REAL_KERNEL), reason="real kernel .npy not present")
def test_fidelity_on_real_kernel():
    dem = np.load(REAL_KERNEL).astype(np.float64)
    src = spectral.analyze_signature(dem, spacing_m=90.0)
    field = spectral.synthesize_field(src, size=256, spacing_m=90.0, seed=5)
    syn = spectral.analyze_signature(field, spacing_m=90.0)
    u = np.array(src["amp_octaves"]); v = np.array(syn["amp_octaves"])
    cos = float(np.dot(u, v) / (np.linalg.norm(u) * np.linalg.norm(v) + 1e-12))
    print(f"\n[real-kernel fidelity] grand_canyon cos={cos:.3f}\n  src={np.round(u,3)}\n  syn={np.round(v,3)}")
    assert cos > 0.85, f"real-kernel spectral fidelity cos={cos:.3f} (src={u} syn={v})"
