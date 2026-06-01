"""Owner-eye still for the mountain_trail config: oblique pre/post + top-down walkable (green) map.
NOT a gate. Run: python render_terrain_edit_review.py -> D:/tmp/wg10_mountain_synthesis/terrain_edit_review.png"""
from __future__ import annotations
from pathlib import Path
import numpy as np
from scipy.ndimage import zoom
import mountain_synthesis as ms
import terrain_edits as te
import terrain_edits.configs as cfg
import terrain_edits.apply as ap
import analyze_rough_world_traversability as trav
from render_geography_skeleton_focus import oblique_panel, labeled
from render_geography_engine import contact_sheet
from PIL import Image

OUT = Path("D:/tmp/wg10_mountain_synthesis/terrain_edit_review.png")


def main():
    CHUNK=3; SRC=30000.0; FEAT=90000.0; step=128
    sc=SRC/step; pspan=CHUNK*SRC+2*sc; pn=CHUNK*step+1+2
    wx,wz=ms.grid(pn,pspan,ox=60000.0-sc,oz=36000.0-sc)
    h=np.asarray(ms.generate(wx,wz,seed=3,style=ms.STYLES[0],feature_span_m=FEAT)["height"],dtype=np.float64)
    span=(25600.0/3.0)*CHUNK; HS=1700.0
    ctx=ap.EditContext(span_m=span, cell_m=span/(h.shape[0]-1), height_scale_m=HS)
    final=h + te.apply_edits(h, ctx, [cfg.mountain_trail()])
    n=h.shape[0]
    def walk(z):
        sl=trav.slope_grid(z, scene_width_m=span, height_scale_m=HS); pa=sl<=ctx.slope_budget
        img=np.zeros((n,n,3),np.uint8); img[pa]=(70,150,70); img[~pa]=(60,45,40)
        return labeled(Image.fromarray(img).resize((430,430),Image.Resampling.NEAREST), "walkable", sub="green=walkable")
    def ds(a,N=160): s=N/a.shape[0]; return zoom(a,(s,s),order=1)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    contact_sheet(OUT, [oblique_panel(ds(h),"mountain"), oblique_panel(ds(final),"+ trail (thin, preserves)"),
                        walk(h), walk(final)], cols=2)
    print(f"wrote {OUT}  carved%={float(np.mean((final-h)!=0))*100:.1f}")


if __name__ == "__main__":
    main()
