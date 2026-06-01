import numpy as np
import mountain_synthesis as ms
import terrain_edits.placement as pl
import terrain_edits.profile as pr
import terrain_edits.apply as ap
import analyze_rough_world_traversability as trav


def _mountain():
    CHUNK=3; SRC=30000.0; FEAT=90000.0; step=128
    sc=SRC/step; pspan=CHUNK*SRC+2*sc; pn=CHUNK*step+1+2
    wx,wz=ms.grid(pn,pspan,ox=60000.0-sc,oz=36000.0-sc)
    h=np.asarray(ms.generate(wx,wz,seed=3,style=ms.STYLES[0],feature_span_m=FEAT)["height"],dtype=np.float64)
    span=(25600.0/3.0)*CHUNK
    return h, ap.EditContext(span_m=span, cell_m=span/(h.shape[0]-1), height_scale_m=1700.0)


def test_thin_climbing_trail_is_walkable_along_route_and_bounded():
    h, ctx = _mountain()
    route = pl.low_corridor_route(h, ctx, pl.LowCorridorParams(low_pref=8.0), axis="x")
    tp = pr.ThinTrailParams(floor_grade_frac=0.5, trail_width_m=ctx.span_m*0.004, blend_width_m=ctx.span_m*0.005, depth_cap_m=4000.0)
    delta = pr.thin_climbing_trail(h, route, ctx, tp)        # height-units delta (<=0), same shape as h
    assert delta.shape == h.shape
    assert np.all(delta <= 1e-12)                            # cut only
    final = h + delta
    sl = trav.slope_grid(final, scene_width_m=ctx.span_m, height_scale_m=ctx.height_scale_m)
    over = np.mean([sl[r, c] > ctx.slope_budget for r, c in route])
    assert over <= 0.10                                      # walkable along the route
    assert np.max(np.abs(delta)) <= tp.depth_cap_m / ctx.height_scale_m + 1e-9   # bounded
