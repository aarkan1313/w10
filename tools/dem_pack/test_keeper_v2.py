import numpy as np
import keeper_v2 as v2
import geography_skeleton_windows as win
import export_godot_rough_world_chunks as ex


def _window():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    w = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 133, spec)
    return w, spec


def test_v2_compose_is_finite_bounded_nonflat_deterministic():
    w, spec = _window()
    p = v2.KeeperV2Params()
    h1 = v2.compose_windowed_height_v2(w, 133, spec, p)
    h2 = v2.compose_windowed_height_v2(w, 133, spec, p)
    assert h1.shape[0] == h1.shape[1]
    assert np.all(np.isfinite(h1))
    assert np.ptp(h1) > 0.05
    assert np.allclose(h1, h2)

def test_apron_blur_crop_returns_core_shape():
    rng = np.random.default_rng(0)
    world = rng.standard_normal((40, 80))
    apron = 6
    # true 2D apron-padded windows: core 20x20, apron 6 on every side -> 32x32 input
    left_full = world[4:36, 4:36]
    right_full = world[4:36, 24:56]
    left_core = v2.apron_blur_crop(left_full, apron_px=apron, sigma=1.5)   # -> (20,20)
    right_core = v2.apron_blur_crop(right_full, apron_px=apron, sigma=1.5)
    assert left_core.shape == (20, 20)
    assert right_core.shape == (20, 20)

def test_apron_blur_exact_overlap_matches():
    rng = np.random.default_rng(1)
    world = rng.standard_normal((30, 100))
    apron = 8
    sigma = 2.0
    a_core = v2.apron_blur_crop(world[:, 12:68], apron_px=apron, sigma=sigma)   # cols 20..59
    b_core = v2.apron_blur_crop(world[:, 32:88], apron_px=apron, sigma=sigma)   # cols 40..79
    overlap_a = a_core[:, 20:40]   # shared core band cols [40,60)
    overlap_b = b_core[:, 0:20]
    assert np.allclose(overlap_a, overlap_b, atol=1e-9)

def test_affine_remap_is_data_independent_and_shared_border_safe():
    a = np.array([[0.0, 0.5, 1.0]])
    b = np.array([[1.0, -0.5, -1.0]])   # shares value 1.0 with a[:, -1] vs b[:, 0]
    ra = v2.affine_remap(a, center=0.0, scale=0.5)
    rb = v2.affine_remap(b, center=0.0, scale=0.5)
    assert ra[0, 2] == rb[0, 0]          # shared 1.0 maps identically (no per-array min/max)
    assert np.allclose(ra, (a - 0.0) * 0.5)

def test_v2_params_defaults_present_and_overridable():
    p = v2.KeeperV2Params()
    for name in ("softmax_temp", "relief_amplitude", "incision_gain",
                 "range_texture_gain", "badland_gain", "fine_gain",
                 "blur_radius_m", "remap_center", "remap_scale", "weight_blur_m"):
        assert hasattr(p, name)
    import dataclasses
    p2 = dataclasses.replace(p, relief_amplitude=2.0)
    assert p2.relief_amplitude == 2.0 and p.relief_amplitude != 2.0
