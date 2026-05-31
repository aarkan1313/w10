import numpy as np
import keeper_v2 as v2

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
