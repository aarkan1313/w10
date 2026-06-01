import numpy as np
import mountain_synthesis as ms
import terrain_edits as te
import terrain_edits.apply as ap
import terrain_edits.placement as pl
import terrain_edits.profile as pr


def _mountain():
    CHUNK=3; SRC=30000.0; FEAT=90000.0; step=128
    sc=SRC/step; pspan=CHUNK*SRC+2*sc; pn=CHUNK*step+1+2
    wx,wz=ms.grid(pn,pspan,ox=60000.0-sc,oz=36000.0-sc)
    h=np.asarray(ms.generate(wx,wz,seed=3,style=ms.STYLES[0],feature_span_m=FEAT)["height"],dtype=np.float64)
    span=(25600.0/3.0)*CHUNK
    return h, ap.EditContext(span_m=span, cell_m=span/(h.shape[0]-1), height_scale_m=1700.0)


def test_apply_edits_runs_placement_and_profile_and_composites():
    h, ctx = _mountain()
    edit = te.TerrainEdit(
        placement=pl.low_corridor_route, placement_params=pl.LowCorridorParams(low_pref=8.0),
        axes=("x", "z"),
        profile=pr.thin_climbing_trail, profile_params=pr.ThinTrailParams(),
    )
    delta = te.apply_edits(h, ctx, [edit])
    assert delta.shape == h.shape
    assert np.all(delta <= 1e-12)                 # cuts only
    assert np.count_nonzero(delta) > 0            # something carved
    assert np.array_equal(delta, te.apply_edits(h, ctx, [edit]))   # deterministic
