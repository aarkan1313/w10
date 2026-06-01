r"""Large multi-scale render of the tuned seam-safe mountain vs legacy, for owner judgment at
proper resolution (the tune thumbnails were too small to judge a fly-able mountain).

Renders legacy (apron_px=0) vs seam-safe core (apron_px=80, cropped) at the SAME world coords,
at a WIDE view and a CLOSE crop, large + high-dpi. Hillshade only (no textures; Phase 6).

Run:    python tools/dem_pack/render_mountain_seamsafe_big.py
Writes: D:/tmp/wg10_biome_compose/mountain_seamsafe_big.png
"""
from __future__ import annotations
from pathlib import Path
import numpy as np
import matplotlib; matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.colors import LightSource
import geography_engine as geo
import mountain_synthesis as mountain

OUT = Path("D:/tmp/wg10_biome_compose/mountain_seamsafe_big.png")
CORE_N = 512                      # high-res core for a proper look
SEED = 7
SPAN_M = 90_000.0
FEATURE_SPAN_M = 90_000.0
OX, OZ = 60_000.0, 36_000.0
AP = mountain.MOUNTAIN_APRON_PX   # 80


def _legacy(core_n, span, ox, oz):
    wx, wz = geo.grid(core_n, span, ox=ox, oz=oz)
    return np.asarray(mountain.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M)["height"], float)


def _seamsafe(core_n, span, ox, oz):
    # Build an apron-padded grid: same cell size as the core, padded AP cells on every side.
    cell = span / (core_n - 1)
    padded_n = core_n + 2 * AP
    pad_span = cell * (padded_n - 1)
    pad_ox = ox - AP * cell
    pad_oz = oz - AP * cell
    wx, wz = geo.grid(padded_n, pad_span, ox=pad_ox, oz=pad_oz)
    h = mountain.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M, apron_px=AP)["height"]
    return np.asarray(h, float)


def _shade(ax, h, title):
    h = np.asarray(h, float)
    hn = (h - h.min()) / (np.ptp(h) + 1e-9)
    rgb = LightSource(azdeg=315, altdeg=45).shade(hn, cmap=plt.cm.terrain, vert_exag=2.0, blend_mode="soft")
    ax.imshow(rgb); ax.set_title(f"{title}\nptp={np.ptp(h):.2f} std={np.std(h):.2f}", fontsize=11); ax.axis("off")


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    # WIDE view (full 90 km)
    leg_wide = _legacy(CORE_N, SPAN_M, OX, OZ)
    ss_wide = _seamsafe(CORE_N, SPAN_M, OX, OZ)
    # CLOSE crop (~22 km window into the same area)
    close_span = 22_000.0
    close_ox, close_oz = OX + 30_000.0, OZ + 30_000.0
    leg_close = _legacy(CORE_N, close_span, close_ox, close_oz)
    ss_close = _seamsafe(CORE_N, close_span, close_ox, close_oz)

    fig, ax = plt.subplots(2, 2, figsize=(20, 20))
    _shade(ax[0, 0], leg_wide, "LEGACY — wide (90 km)")
    _shade(ax[0, 1], ss_wide, "SEAM-SAFE — wide (90 km)")
    _shade(ax[1, 0], leg_close, "LEGACY — close (22 km)")
    _shade(ax[1, 1], ss_close, "SEAM-SAFE — close (22 km)")
    fig.suptitle("Tuned seam-safe mountain vs legacy — large, two scales (hillshade, no textures)", fontsize=14)
    fig.tight_layout()
    fig.savefig(OUT, dpi=110)
    print(f"wrote {OUT}")
    print(f"  wide : legacy ptp={np.ptp(leg_wide):.2f}/std={np.std(leg_wide):.2f}  seamsafe ptp={np.ptp(ss_wide):.2f}/std={np.std(ss_wide):.2f}")
    print(f"  close: legacy ptp={np.ptp(leg_close):.2f}/std={np.std(leg_close):.2f}  seamsafe ptp={np.ptp(ss_close):.2f}/std={np.std(ss_close):.2f}")


if __name__ == "__main__":
    main()
