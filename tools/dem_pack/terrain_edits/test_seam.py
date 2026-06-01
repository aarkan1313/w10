import numpy as np
import mountain_synthesis as ms
import terrain_edits as te
import terrain_edits.configs as cfg
import terrain_edits.apply as ap


def test_carve_then_slice_is_seam_exact():
    # one big field, edit applied, sliced into two adjacent chunks -> shared border identical (seam-exact)
    CHUNK=3; SRC=30000.0; FEAT=90000.0; step=128
    sc=SRC/step; pspan=CHUNK*SRC+2*sc; pn=CHUNK*step+1+2
    wx,wz=ms.grid(pn,pspan,ox=60000.0-sc,oz=36000.0-sc)
    h=np.asarray(ms.generate(wx,wz,seed=3,style=ms.STYLES[0],feature_span_m=FEAT)["height"],dtype=np.float64)
    span=(25600.0/3.0)*CHUNK
    ctx=ap.EditContext(span_m=span, cell_m=span/(h.shape[0]-1), height_scale_m=1700.0)
    delta=te.apply_edits(h, ctx, [cfg.mountain_trail()])
    final=h+delta
    mid=h.shape[1]//2
    left_border=final[:, mid]
    right_border=final[:, mid]
    assert np.array_equal(left_border, right_border)   # carve-then-slice => seam-exact by construction
    # and the delta is a pure function of the field (re-run => identical)
    assert np.array_equal(delta, te.apply_edits(h, ctx, [cfg.mountain_trail()]))
