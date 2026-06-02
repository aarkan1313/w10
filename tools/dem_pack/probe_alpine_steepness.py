r"""FAST steepness sweep on a single MOUNTAIN block, to dial DRAMATIC ALPINE before applying to all
biomes. Steepness = vertical relief / horizontal feature width; the dominant lever is FEATURE_SPAN
(smaller span -> tighter, steeper peaks). Sweeps feature_span x vertical_exag at a fixed ~75 km
block view (a believable block size), straight to a PNG in seconds.

Run:    python tools/dem_pack/probe_alpine_steepness.py
Writes: D:/tmp/wg10_biome_compose/alpine_steepness.png
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

OUT = Path("D:/tmp/wg10_biome_compose/alpine_steepness.png")
N = 420
BLOCK_SPAN_M = 75_000.0     # a believable ~75 km block view (vs the too-wide 125 km)
SEED = 219
FEATURE_SPANS = [90_000.0, 45_000.0, 25_000.0]   # wide -> tight (tighter = steeper alpine)
VERT_EXAGS = [3.0, 5.0, 8.0]                       # display steepness


def _shade(ax, h, title, ve):
    hn = (h - h.min()) / (np.ptp(h) + 1e-9)
    rgb = LightSource(azdeg=315, altdeg=45).shade(hn, cmap=plt.cm.terrain, vert_exag=ve, blend_mode="soft")
    ax.imshow(rgb); ax.axis("off"); ax.set_title(title, fontsize=9)


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    wx, wz = geo.grid(N, BLOCK_SPAN_M, ox=60_000.0, oz=36_000.0)
    print(f"alpine steepness sweep: {N}x{N} over {BLOCK_SPAN_M/1000:.0f} km block")
    # cache one generate per feature_span (vertical exag is just display)
    fields = {}
    for fs in FEATURE_SPANS:
        print(f"  generate mountain feature_span={fs/1000:.0f} km ...")
        h = np.asarray(mountain.generate(wx, wz, seed=SEED, feature_span_m=fs)["height"], float)
        fields[fs] = (h - h.mean()) / (h.std() + 1e-9)

    fig, ax = plt.subplots(len(FEATURE_SPANS), len(VERT_EXAGS), figsize=(18, 18))
    for r, fs in enumerate(FEATURE_SPANS):
        for c, ve in enumerate(VERT_EXAGS):
            _shade(ax[r][c], fields[fs], f"feature_span={fs/1000:.0f}km  vert_exag={ve}", ve)
    fig.suptitle(f"DRAMATIC ALPINE sweep -- mountain block @ {BLOCK_SPAN_M/1000:.0f} km. "
                 f"Rows: feature_span (wide->TIGHT=steeper). Cols: vertical exaggeration.", fontsize=13)
    fig.tight_layout()
    fig.savefig(OUT, dpi=92)
    print(f"  wrote {OUT}")


if __name__ == "__main__":
    main()
