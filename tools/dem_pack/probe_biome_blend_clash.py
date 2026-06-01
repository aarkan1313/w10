r"""PROBE (Slice A de-risk): does the height_favored blend survive a CLASHING biome pair?

mountain<->desert dunes — dune-train directionality vs ridge orientation is the stress test the
gentle mountain<->grassland probe did not cover. Renders field / feathered / height_favored side
by side for the owner's eye. If height_favored ghosts or fights here, adjust before building.

Run:   python tools/dem_pack/probe_biome_blend_clash.py
Writes: D:/tmp/wg10_biome_compose/probe_biome_blend_clash.png
"""
from __future__ import annotations
from pathlib import Path
import numpy as np
import matplotlib; matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.colors import LightSource
from scipy.ndimage import gaussian_filter
import geography_engine as geo
import mountain_synthesis as mountain
import desert_synthesis as desert

OUT = Path("D:/tmp/wg10_biome_compose/probe_biome_blend_clash.png")
N, SEED, SPAN_M, FEATURE_SPAN_M, BAND_FRAC = 320, 133, 60_000.0, 90_000.0, 0.16

def _smoothstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0 + 1e-9), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)

def _shade(ax, h, title):
    h = np.asarray(h, float); hn = (h - h.min()) / (np.ptp(h) + 1e-9)
    rgb = LightSource(azdeg=315, altdeg=45).shade(hn, cmap=plt.cm.terrain, vert_exag=2.0, blend_mode="soft")
    ax.imshow(rgb); ax.set_title(title, fontsize=9); ax.axis("off")
    ax.axvline(h.shape[1] * 0.5, color="red", lw=0.6, alpha=0.5)

def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    wx, wz = geo.grid(N, SPAN_M, ox=60_000.0, oz=36_000.0)
    mtn = np.asarray(mountain.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M)["height"], float)
    des = np.asarray(desert.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M)["height"], float)
    mtn = (mtn - mtn.mean()) / (mtn.std() + 1e-9)
    des = (des - des.mean()) / (des.std() + 1e-9)
    u = np.linspace(0.0, 1.0, N)[None, :].repeat(N, axis=0)
    w_mtn = 1.0 - _smoothstep(0.5 - BAND_FRAC, 0.5 + BAND_FRAC, u)
    field = w_mtn * mtn + (1.0 - w_mtn) * des
    thin = 0.045
    w_thin = 1.0 - _smoothstep(0.5 - thin, 0.5 + thin, u)
    feathered = w_thin * mtn + (1.0 - w_thin) * des
    relief_m = np.abs(mtn - gaussian_filter(mtn, sigma=6.0))
    relief_d = np.abs(des - gaussian_filter(des, sigma=6.0))
    favor = relief_m / (relief_m + relief_d + 1e-9)
    w_fav = np.clip(w_mtn + (favor - 0.5) * 0.9 * (1.0 - np.abs(2 * w_mtn - 1)), 0.0, 1.0)
    height_fav = w_fav * mtn + (1.0 - w_fav) * des
    fig, ax = plt.subplots(2, 3, figsize=(16, 11))
    _shade(ax[0, 0], mtn, "mountain"); _shade(ax[0, 1], des, "desert dunes"); _shade(ax[0, 2], w_mtn, "weight (white=mtn)")
    _shade(ax[1, 0], field, "1. field-blend"); _shade(ax[1, 1], feathered, "2. feathered"); _shade(ax[1, 2], height_fav, "3. height-favored")
    fig.suptitle("PROBE Slice A: CLASH pair mountain<->desert dunes. Red = boundary.", fontsize=12)
    fig.tight_layout(); fig.savefig(OUT, dpi=92); print(f"wrote {OUT}")

if __name__ == "__main__":
    main()
