import numpy as np
import biome_registry as br
import geography_engine as geo

def test_registry_has_mountain_and_grassland():
    assert "mountain" in br.REGISTRY
    assert "grassland" in br.REGISTRY

def test_recipe_returns_bare_height_array():
    wx, wz = geo.grid(48, 60_000.0, ox=60_000.0, oz=36_000.0)
    h = br.get_recipe("mountain").generate(wx, wz, seed=133, feature_span_m=90_000.0)
    assert isinstance(h, np.ndarray)
    assert h.shape == (48, 48)

def test_recipe_is_deterministic():
    wx, wz = geo.grid(32, 60_000.0, ox=60_000.0, oz=36_000.0)
    r = br.get_recipe("grassland")
    a = r.generate(wx, wz, seed=7, feature_span_m=90_000.0)
    b = r.generate(wx, wz, seed=7, feature_span_m=90_000.0)
    assert np.array_equal(a, b)

def test_unknown_recipe_raises():
    import pytest
    with pytest.raises(KeyError):
        br.get_recipe("not_a_biome")

def test_all_registered_recipes_run_and_return_2d_float():
    wx, wz = geo.grid(24, 60_000.0, ox=60_000.0, oz=36_000.0)
    for name in br.REGISTRY:
        h = br.get_recipe(name).generate(wx, wz, seed=3, feature_span_m=90_000.0)
        assert isinstance(h, np.ndarray) and h.ndim == 2, f"{name} did not return a 2D array"
        assert np.issubdtype(h.dtype, np.floating), f"{name} returned non-float dtype"
        assert np.all(np.isfinite(h)), f"{name} returned non-finite values"
