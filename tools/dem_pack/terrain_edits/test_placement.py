import numpy as np
import mountain_synthesis as ms
import terrain_edits.placement as pl
import terrain_edits.apply as ap


def _mountain():
    CHUNK=3; SRC=30000.0; FEAT=90000.0; step=128
    sc=SRC/step; pspan=CHUNK*SRC+2*sc; pn=CHUNK*step+1+2
    wx,wz=ms.grid(pn,pspan,ox=60000.0-sc,oz=36000.0-sc)
    h=np.asarray(ms.generate(wx,wz,seed=3,style=ms.STYLES[0],feature_span_m=FEAT)["height"],dtype=np.float64)
    span=(25600.0/3.0)*CHUNK
    return h, ap.EditContext(span_m=span, cell_m=span/(h.shape[0]-1), height_scale_m=1700.0)


def test_low_corridor_route_crosses_edge_to_edge_and_deterministic():
    h, ctx = _mountain()
    p = pl.LowCorridorParams(low_pref=8.0)
    r1 = pl.low_corridor_route(h, ctx, p, axis="x")
    r2 = pl.low_corridor_route(h, ctx, p, axis="x")
    assert r1 == r2                                          # deterministic
    cols = [c for _, c in r1]
    assert min(cols) == 0 and max(cols) == h.shape[1] - 1    # spans west->east
    steps = [abs(r1[i][0]-r1[i+1][0]) + abs(r1[i][1]-r1[i+1][1]) for i in range(len(r1)-1)]
    assert all(s <= 2 for s in steps)                        # 4/8-connected path


def test_low_corridor_route_z_axis_spans_ns():
    h, ctx = _mountain()
    p = pl.LowCorridorParams(low_pref=8.0)
    r1 = pl.low_corridor_route(h, ctx, p, axis="z")
    r2 = pl.low_corridor_route(h, ctx, p, axis="z")
    assert r1 == r2                                          # deterministic
    rows = [r for r, _ in r1]
    assert min(rows) == 0 and max(rows) == h.shape[0] - 1    # spans north->south
    steps = [abs(r1[i][0]-r1[i+1][0]) + abs(r1[i][1]-r1[i+1][1]) for i in range(len(r1)-1)]
    assert all(s <= 2 for s in steps)                        # 4/8-connected path
