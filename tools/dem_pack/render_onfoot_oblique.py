r"""FAST on-foot oblique render: stand ~eye-level in a mountain valley and look up at the peaks, to
judge whether the scale-contract mountain (1000 m over ~3.5 km base, slope ~0.29) TOWERS on foot.
Overview top-downs can't show 'towering' -- it's an on-foot property. This renders a 3D surface of a
real-metre mountain patch from a low oblique camera, with a HUMAN-height reference marker for scale.

Run:    python tools/dem_pack/render_onfoot_oblique.py
Writes: D:/tmp/wg10_biome_compose/onfoot_oblique.png
"""
from __future__ import annotations
import sys
try:
    sys.stdout.reconfigure(encoding="utf-8")
except (AttributeError, ValueError):
    pass
from pathlib import Path
import numpy as np
import matplotlib; matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.colors import LightSource
import geography_engine as geo
import mountain_synthesis as mountain

OUT = Path("D:/tmp/wg10_biome_compose/onfoot_oblique.png")
PATCH_SPAN_M = 12_000.0     # a 12 km patch -> a few mountains across (region ~30km holds ~8)
N = 300
FEATURE_SPAN_M = 3_500.0    # contract: mountain feature span
RELIEF_M = 1000.0           # contract: mountain peak height
SEED = 219


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    wx, wz = geo.grid(N, PATCH_SPAN_M, ox=60_000.0, oz=36_000.0)
    h = np.asarray(mountain.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M)["height"], float)
    h = (h - h.mean()) / (h.std() + 1e-9)
    h = h * (RELIEF_M / 4.0)                       # real metres, ptp ~= RELIEF_M
    h = h - h.min()                                # valley floor at 0
    print(f"patch {PATCH_SPAN_M/1000:.0f} km, peak {h.max():.0f} m, feature_span {FEATURE_SPAN_M/1000:.1f} km")

    xs = np.linspace(0, PATCH_SPAN_M, N)
    zs = np.linspace(0, PATCH_SPAN_M, N)
    X, Z = np.meshgrid(xs, zs)

    ls = LightSource(azdeg=315, altdeg=30)
    fig = plt.figure(figsize=(20, 12))

    # two oblique views: a low 'valley-floor looking up' angle, and a higher 3/4 view
    for i, (elev, azim, title) in enumerate([
        (6,  -75, "ON-FOOT: ~eye-level in the valley looking up (elev 6deg)"),
        (22, -60, "Low oblique 3/4 view (elev 22deg)"),
    ]):
        ax = fig.add_subplot(1, 2, i + 1, projection="3d")
        rgb = ls.shade(h, cmap=plt.cm.terrain, vert_exag=1.0, blend_mode="soft", dx=PATCH_SPAN_M / N, dy=PATCH_SPAN_M / N)
        ax.plot_surface(X, Z, h, facecolors=rgb, rstride=2, cstride=2, linewidth=0, antialiased=False, shade=False)
        ax.set_box_aspect((1, 1, 0.45))            # TRUE proportions: 12km x 12km x ~1km -> z is small (honest)
        ax.view_init(elev=elev, azim=azim)
        # human reference: a 1.8 m marker at the valley floor (a tiny vertical line; will be ~invisible = correct)
        fx, fz = PATCH_SPAN_M * 0.5, PATCH_SPAN_M * 0.08
        ax.plot([fx, fx], [fz, fz], [0, 1.8], color="red", lw=3)
        ax.text(fx, fz, 60, "1.8 m human\n(red)", color="red", fontsize=9)
        ax.set_title(title, fontsize=11)
        ax.set_xlabel("x (m)"); ax.set_ylabel("z (m)"); ax.set_zlabel("height (m)")
        ax.set_zlim(0, max(h.max(), PATCH_SPAN_M * 0.45))

    fig.suptitle(f"On-foot scale check: mountain {RELIEF_M:.0f} m peaks / {FEATURE_SPAN_M/1000:.1f} km features, "
                 f"slope ~{RELIEF_M/FEATURE_SPAN_M:.2f}, TRUE proportions (12 km patch). Does it tower over the 1.8 m human?", fontsize=12)
    fig.savefig(OUT, dpi=90)
    print(f"  wrote {OUT}")


if __name__ == "__main__":
    main()
