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
    a = np.zeros((16, 16)); a[:, ::2] = 3.0   # stripe = high local relief
    b = np.zeros((16, 16))                     # flat
    w = np.full((16, 16), 0.5)
    cfg = bc.BlendConfig(mode="height_favored")
    favored = bc._blend_height_favored(a, b, w, cfg)
    plain = bc._blend_field(a, b, w)
    assert float(np.mean(favored)) > float(np.mean(plain)), "bias should increase weight on high-relief recipe"
    assert np.allclose(bc._blend_height_favored(a, b, np.ones((16, 16)), cfg), a), "w=1 -> pure a"
    assert np.allclose(bc._blend_height_favored(a, b, np.zeros((16, 16)), cfg), b), "w=0 -> pure b"

def test_height_favored_flat_flat_degrades_to_lerp():
    a = np.full((8, 8), 1.0); b = np.full((8, 8), 2.0)   # both flat, different levels
    w = np.full((8, 8), 0.5)
    cfg = bc.BlendConfig(mode="height_favored")
    favored = bc._blend_height_favored(a, b, w, cfg)
    plain = bc._blend_field(a, b, w)
    assert np.allclose(favored, plain, atol=0.05), "flat+flat: no relief signal -> lerp, not a jump to b"

def test_compose_biomes_two_recipes_reduces_to_pure_at_ends():
    a = np.full((4, 4), 5.0); b = np.full((4, 4), 1.0)
    w_a = np.array([[1.0, 1.0, 0.0, 0.0]] * 4)   # left 2 cols pure a, right 2 cols pure b
    cfg = bc.BlendConfig(mode="height_favored")
    out = bc.compose_biomes([a, b], [w_a, 1.0 - w_a], cfg)
    assert np.allclose(out[:, 0], 5.0)            # pure a
    assert np.allclose(out[:, -1], 1.0)           # pure b

def test_compose_biomes_three_recipe_order_independent():
    a = np.full((6, 6), 3.0); a[:, ::2] = 6.0   # structured
    b = np.full((6, 6), 1.0); c = np.full((6, 6), 0.0)
    w = [np.full((6, 6), 0.5), np.full((6, 6), 0.3), np.full((6, 6), 0.2)]
    cfg = bc.BlendConfig(mode="height_favored")
    out1 = bc.compose_biomes([a, b, c], w, cfg)
    out2 = bc.compose_biomes([c, b, a], [w[2], w[1], w[0]], cfg)
    assert np.allclose(out1, out2), "3-recipe compose must be order-independent"

def test_compose_biomes_determinism():
    a = np.full((4, 4), 5.0); b = np.full((4, 4), 1.0)
    w_a = np.full((4, 4), 0.5)
    cfg = bc.BlendConfig()
    o1 = bc.compose_biomes([a, b], [w_a, 1.0 - w_a], cfg)
    o2 = bc.compose_biomes([a, b], [w_a, 1.0 - w_a], cfg)
    assert np.array_equal(o1, o2)
