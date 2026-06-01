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
