import numpy as np
import biome_compose as bc

def test_field_blend_is_weighted_lerp():
    a = np.full((4, 4), 2.0)
    b = np.full((4, 4), 0.0)
    w = np.full((4, 4), 0.25)          # 0.25 weight on 'a'
    cfg = bc.BlendConfig(mode="field")
    out = bc._blend_field(a, b, w)
    assert np.allclose(out, 0.25 * 2.0 + 0.75 * 0.0)  # = 0.5 everywhere
