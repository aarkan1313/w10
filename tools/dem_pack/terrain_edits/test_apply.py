import numpy as np
import terrain_edits.apply as ap


def test_blend_edges_smoothsteps_to_zero_at_band_edge():
    dist = np.array([[0.0, 5.0, 10.0, 20.0]])      # metres to route
    raw = np.full_like(dist, -100.0)
    out = ap.blend_edges(raw, dist, flat_to=5.0, blend_to=10.0)
    assert out[0, 0] == -100.0                      # on route: full
    assert out[0, 3] == 0.0                         # beyond blend: zero (no cliff)
    assert -100.0 < out[0, 1] < 0.0 or out[0, 1] == -100.0   # within flat: still full-ish
    assert -100.0 < out[0, 2] <= 0.0                # in blend band: tapered


def test_bound_depth_caps_lowering():
    raw = np.array([-50.0, -300.0, 10.0])           # deltas (negative = cut)
    out = ap.bound_depth(raw, cap_m=200.0)
    assert out[0] == -50.0                           # under cap untouched
    assert out[1] == -200.0                          # capped
    assert out[2] == 0.0                             # positive cut clamped to 0 (cuts only here)


def test_combine_min_takes_deepest_cut():
    a = np.array([-10.0, 0.0, -5.0])
    b = np.array([0.0, -8.0, -3.0])
    out = ap.combine([a, b], mode="min")             # cuts: deepest wins
    assert list(out) == [-10.0, -8.0, -5.0]


def test_edit_context_holds_window_geometry():
    ctx = ap.EditContext(span_m=76800.0, cell_m=200.0, height_scale_m=1700.0, slope_budget=0.28)
    assert ctx.span_m == 76800.0 and ctx.cell_m == 200.0
    assert ctx.height_scale_m == 1700.0 and ctx.slope_budget == 0.28
