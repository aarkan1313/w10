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
    # TWO structured fields (a stripes, b checker) + one flat, so a reintroduced order-dependent
    # fold would actually diverge between the two orderings (the weak single-structured version
    # collapsed to 'a' both ways and could not catch the bug).
    a = np.full((6, 6), 3.0); a[:, ::2] = 6.0          # vertical stripes
    b = np.full((6, 6), 2.0); b[::2, :] = 5.0          # horizontal stripes (different structure)
    c = np.full((6, 6), 0.0)                            # flat
    wa = np.full((6, 6), 0.5); wb = np.full((6, 6), 0.3); wc = np.full((6, 6), 0.2)
    cfg = bc.BlendConfig(mode="height_favored")
    out1 = bc.compose_biomes([a, b, c], [wa, wb, wc], cfg)
    out2 = bc.compose_biomes([c, b, a], [wc, wb, wa], cfg)
    assert np.allclose(out1, out2, atol=1e-9), "3-recipe compose must be order-independent"

def test_compose_biomes_determinism():
    a = np.full((4, 4), 5.0); b = np.full((4, 4), 1.0)
    w_a = np.full((4, 4), 0.5)
    cfg = bc.BlendConfig()
    o1 = bc.compose_biomes([a, b], [w_a, 1.0 - w_a], cfg)
    o2 = bc.compose_biomes([a, b], [w_a, 1.0 - w_a], cfg)
    assert np.array_equal(o1, o2)

def _structured(rng, shape):
    """A field with real LOCAL relief (so the gaussian relief-proxy is non-trivial and its
    boundary mode actually matters)."""
    return rng.standard_normal(shape) * 3.0

def _blend_with_mode(a, b, w, cfg, mode):
    """Reference reimplementation of _blend_height_favored with an EXPLICIT boundary mode, so the
    test can pin which mode the production function actually uses (nearest = apron-safe clamp)."""
    from scipy.ndimage import gaussian_filter
    a = np.asarray(a, float); b = np.asarray(b, float); w = np.asarray(w, float)
    ra = np.abs(a - gaussian_filter(a, sigma=cfg.relief_sigma_px, mode=mode))
    rb = np.abs(b - gaussian_filter(b, sigma=cfg.relief_sigma_px, mode=mode))
    t = ra + rb; favor = ra / (t + 1e-9); conf = t / (t + cfg.relief_confidence_floor)
    band = 1.0 - np.abs(2.0 * w - 1.0)
    w_adj = np.clip(w + (favor - 0.5) * cfg.favor_strength * band * conf, 0.0, 1.0)
    return w_adj * a + (1.0 - w_adj) * b

def test_height_favored_blur_uses_nearest_clamp_not_reflect():
    # F5 regression: the relief-proxy blur must use mode='nearest' (clamp-to-edge = the apron-safe
    # convention the seam-safe synths use), NOT scipy's default 'reflect'. nearest vs reflect only
    # diverge NEAR AN ARRAY EDGE (where the blur footprint runs off the array); in the interior
    # they agree. So we pin the mode by comparing the PRODUCTION function against two explicit
    # references and asserting it matches nearest and DIFFERS from reflect at the edge. If the code
    # reverts to reflect, the first assert FAILS -> not a tautology.
    rng = np.random.default_rng(99)
    N = 48
    a = _structured(rng, (N, N)); b = _structured(rng, (N, N))
    # w = 0.5 everywhere => the favor band is FULLY OPEN (band=1) at every column, INCLUDING the
    # edge, so the relief-proxy (and thus its boundary mode) genuinely affects the edge output.
    # (A ramp weight would be 0/1 at the edges, masking the proxy there via band->0.)
    w = np.full((N, N), 0.5)
    cfg = bc.BlendConfig(mode="height_favored")

    prod = bc._blend_height_favored(a, b, w, cfg)
    ref_nearest = _blend_with_mode(a, b, w, cfg, "nearest")
    ref_reflect = _blend_with_mode(a, b, w, cfg, "reflect")

    d_near = float(np.max(np.abs(prod - ref_nearest)))
    edge_gap = float(np.max(np.abs(ref_nearest[:, 0] - ref_reflect[:, 0])))  # modes differ at edge
    print(f"[F5] max|prod-nearest|={d_near:.3e}  edge|nearest-reflect|={edge_gap:.3e}")
    assert d_near < 1e-12, (
        "the relief-proxy blur must use mode='nearest' (apron-safe clamp); production output "
        f"diverges from the nearest reference by {d_near:.3e}. If this is large the code is using "
        "scipy's default 'reflect' (the F5 bug) which breaks seam-safety at window edges."
    )
    # Guard the guard: nearest and reflect MUST actually differ here, or the above would be vacuous.
    assert edge_gap > 1e-6, "edge fixture is degenerate: nearest and reflect must differ at the edge"

def test_height_favored_blur_is_apron_safe_when_apron_covers_footprint():
    # The constructive seam guarantee: an INDEPENDENT window blended on APRON-PADDED inputs (apron
    # >= the gaussian footprint) reproduces the big-field blend EXACTLY over the window's core --
    # because the apron feeds the blur real neighbour data instead of an invented edge. This is the
    # property the compose-big-then-slice model and the independent-window model both rely on, and
    # mode='nearest' (vs reflect) is required for it to hold consistently with the synths' convention.
    rng = np.random.default_rng(123)
    N = 160
    a_big = _structured(rng, (N, N)); b_big = _structured(rng, (N, N))
    cols = np.linspace(0.0, 1.0, N)[None, :].repeat(N, axis=0)
    w_big = np.clip(cols, 0.0, 1.0)                   # band straddles the seam (worst case)
    cfg = bc.BlendConfig(mode="height_favored")
    full = bc._blend_height_favored(a_big, b_big, w_big, cfg)

    # apron must COVER the footprint: scipy gaussian_filter truncates at 4*sigma -> radius
    # int(4*sigma + 0.5); pad beyond that. (A SMALLER apron cannot reproduce the big field in ANY
    # boundary mode -- the honest limit of independent-window blending, documented here.)
    apron = int(4.0 * cfg.relief_sigma_px + 0.5) + 4  # = 28 for sigma 6
    seam = N // 2
    lo, hi = seam - 24, seam + 24                     # the window's CORE columns [lo, hi)
    pa, pb = lo - apron, hi + apron                   # apron-padded input extent
    win_core = bc._blend_height_favored(a_big[:, pa:pb], b_big[:, pa:pb], w_big[:, pa:pb], cfg)[:, apron:-apron]
    truth_core = full[:, lo:hi]
    max_delta = float(np.max(np.abs(win_core - truth_core)))
    print(f"[F5] apron={apron} (>=footprint) core_cols=[{lo},{hi}) max|win_core - big_field|={max_delta:.3e}")
    assert win_core.shape == truth_core.shape
    assert max_delta < 1e-9, (
        f"apron-padded (apron>=footprint) window must match the big field over its core "
        f"(seam-exact); max delta {max_delta:.3e}."
    )
