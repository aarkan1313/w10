import numpy as np
import mountain_synthesis as ms
import terrain_edits as te
import terrain_edits.configs as cfg
import terrain_edits.apply as ap


def _mountain():
    CHUNK=3; SRC=30000.0; FEAT=90000.0; step=128
    sc=SRC/step; pspan=CHUNK*SRC+2*sc; pn=CHUNK*step+1+2
    wx,wz=ms.grid(pn,pspan,ox=60000.0-sc,oz=36000.0-sc)
    h=np.asarray(ms.generate(wx,wz,seed=3,style=ms.STYLES[0],feature_span_m=FEAT)["height"],dtype=np.float64)
    span=(25600.0/3.0)*CHUNK
    return h, ap.EditContext(span_m=span, cell_m=span/(h.shape[0]-1), height_scale_m=1700.0)


def test_mountain_trail_config_carves_a_thin_bounded_trail():
    h, ctx = _mountain()
    edit = cfg.mountain_trail()
    delta = te.apply_edits(h, ctx, [edit])
    assert np.all(delta <= 1e-12)
    carved_frac = float(np.mean(delta != 0.0))
    assert 0.0 < carved_frac < 0.15           # thin (a trail, not a gash)


def test_sketches_instantiate_and_emit_a_delta_through_the_framework():
    h, ctx = _mountain()
    for make in (cfg.road, cfg.river, cfg.lake, cfg.poi):
        edit = make()
        delta = te.apply_edits(h, ctx, [edit])   # proves the abstraction runs end-to-end
        assert delta.shape == h.shape
        assert np.all(delta <= 1e-12)            # cut-only (no fill shortcut)
        assert np.count_nonzero(delta) > 0       # non-trivial (the abstraction actually carved)


def test_mountain_trail_connected_gives_full_traversal_network():
    # the FULL-TRAVERSAL knob: ONE connected carved network reaching all four edges (meet-in-the-middle), so
    # the trail spans fully left<->right AND up<->down. (vs the sparse default which can skirt one side.)
    # GEOMETRIC guarantee = the carved network connects all 4 edges. (Walkability of every metre is the honest
    # depth-cap-vs-walkable tension -- a tunable, asserted as a high fraction, not 100%; see spec section 8.)
    from scipy.ndimage import label
    h, ctx = _mountain()
    delta = te.apply_edits(h, ctx, [cfg.mountain_trail_connected()])
    carved = np.abs(delta) > 1e-9
    lab, _ = label(carved, structure=np.ones((3, 3)))            # 8-connectivity (trails move diagonally)
    sizes = np.bincount(lab.ravel()); sizes[0] = 0
    biggest = lab == int(sizes.argmax())
    assert biggest[:, 0].any() and biggest[:, -1].any()          # one network reaches West AND East -> full L<->R
    assert biggest[0, :].any() and biggest[-1, :].any()          # ... AND North AND South -> full U<->D


def test_route_count_knob_spreads_and_carves_more():
    # route_count > 1 places more crossings -> more of the range covered (addresses "trail skirts one side").
    h, ctx = _mountain()
    one = te.apply_edits(h, ctx, [cfg.mountain_trail(route_count=1)])
    three = te.apply_edits(h, ctx, [cfg.mountain_trail(route_count=3)])
    assert float(np.mean(three != 0.0)) > float(np.mean(one != 0.0))   # denser coverage
