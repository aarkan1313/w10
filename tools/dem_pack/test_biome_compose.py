import numpy as np
import biome_compose as bc

def test_field_blend_is_weighted_lerp():
    a = np.full((4, 4), 2.0)
    b = np.full((4, 4), 0.0)
    w = np.full((4, 4), 0.25)          # 0.25 weight on 'a'
    cfg = bc.BlendConfig(mode="field")
    out = bc._blend_field(a, b, w)
    assert np.allclose(out, 0.25 * 2.0 + 0.75 * 0.0)  # = 0.5 everywhere

def test_height_favored_biases_toward_higher_relief_in_band():
    a = np.zeros((8, 8)); a[:, ::2] = 3.0       # high-frequency stripes = strong local relief
    b = np.zeros((8, 8))                        # flat = no relief
    w = np.full((8, 8), 0.5)                    # neutral band weight
    cfg = bc.BlendConfig(mode="height_favored")
    favored = bc._blend_height_favored(a, b, w, cfg)
    plain = bc._blend_field(a, b, w)
    assert float(np.mean(np.abs(favored))) > float(np.mean(np.abs(plain)))
