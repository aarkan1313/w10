r"""PROBE (throwaway, render-first de-risk): is a principled CROSS-RECIPE biome blend tractable?

Fork B's one unsolved piece: blending the OUTPUTS of two distinct biome recipes (mountain keeps
oriented ridges, grassland keeps swells/draws) at a boundary WITHOUT the mushy "averaged ghost"
that field-blend gives. We render a mountain<->grassland transition on one shared big field with
3 blend mechanisms side by side, for the owner's eye:
  1. field-blend         w*mtn + (1-w)*grass        (the baseline we suspect is mushy)
  2. feathered dominant  pick dominant recipe, feather only a thin seam band
  3. height-favored      blend weighted toward the HIGHER-relief recipe near the band, so the
                         mountain structure is not ghosted down into a half-height mound

Pure look judgment; seam-exactness across independent windows is a separate slice. Compose-big-
field is fine here (one field, sliced conceptually) — the question is ONLY transition naturalness.

Run:   python tools/dem_pack/probe_biome_blend.py
Writes: D:/tmp/wg10_biome_compose/probe_biome_blend.png
"""
from __future__ import annotations

from pathlib import Path

import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.colors import LightSource

import geography_engine as geo
import mountain_synthesis as mountain
import grassland_synthesis as grass

OUT = Path("D:/tmp/wg10_biome_compose/probe_biome_blend.png")
N = 320
SEED = 133
SPAN_M = 60_000.0          # one big field spanning both biomes left->right
FEATURE_SPAN_M = 90_000.0
BAND_FRAC = 0.16           # transition band half-width as a fraction of the field width


def _smoothstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0 + 1e-9), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def _shade(ax, h, title):
    h = np.asarray(h, dtype=np.float64)
    hn = (h - h.min()) / (np.ptp(h) + 1e-9)
    ls = LightSource(azdeg=315, altdeg=45)
    rgb = ls.shade(hn, cmap=plt.cm.terrain, vert_exag=2.0, blend_mode="soft")
    ax.imshow(rgb)
    ax.set_title(title, fontsize=9)
    ax.axis("off")
    # mark the biome-boundary column
    ax.axvline(h.shape[1] * 0.5, color="red", lw=0.6, alpha=0.5)


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    wx, wz = geo.grid(N, SPAN_M, ox=60_000.0, oz=36_000.0)

    # Both recipes over the WHOLE field (same coords), then we blend left=mountain, right=grassland.
    mtn = np.asarray(mountain.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M)["height"], dtype=np.float64)
    grs = np.asarray(grass.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M)["height"], dtype=np.float64)
    # put both on a comparable scale (each recipe self-normalizes differently)
    mtn = (mtn - mtn.mean()) / (mtn.std() + 1e-9)
    grs = (grs - grs.mean()) / (grs.std() + 1e-9)

    # weight field: 1.0 = full mountain (left), 0.0 = full grassland (right), smooth across a band at center
    u = np.linspace(0.0, 1.0, N)[None, :].repeat(N, axis=0)   # 0..1 left->right
    center = 0.5
    w_mtn = 1.0 - _smoothstep(center - BAND_FRAC, center + BAND_FRAC, u)

    # 1. field-blend
    field = w_mtn * mtn + (1.0 - w_mtn) * grs

    # 2. feathered dominant: hard pick outside a thin seam, smoothstep only inside it
    thin = 0.045
    w_thin = 1.0 - _smoothstep(center - thin, center + thin, u)
    feathered = w_thin * mtn + (1.0 - w_thin) * grs

    # 3. height-favored: bias the weight toward whichever recipe is locally higher-relief, so the
    #    mountain's structure dominates the band instead of being averaged into a low mound.
    #    local relief proxy = abs deviation from a blurred self
    from scipy.ndimage import gaussian_filter
    relief_m = np.abs(mtn - gaussian_filter(mtn, sigma=6.0))
    relief_g = np.abs(grs - gaussian_filter(grs, sigma=6.0))
    favor = relief_m / (relief_m + relief_g + 1e-9)          # ~1 where mountain has the structure
    w_fav = np.clip(w_mtn + (favor - 0.5) * 0.9 * (1.0 - np.abs(2 * w_mtn - 1)), 0.0, 1.0)
    height_fav = w_fav * mtn + (1.0 - w_fav) * grs

    fig, axes = plt.subplots(2, 3, figsize=(16, 11))
    _shade(axes[0, 0], mtn, "mountain recipe (full field)")
    _shade(axes[0, 1], grs, "grassland recipe (full field)")
    _shade(axes[0, 2], w_mtn, "blend weight (white=mtn)")
    _shade(axes[1, 0], field, "1. FIELD-BLEND (suspect mushy)")
    _shade(axes[1, 1], feathered, "2. feathered dominant (thin seam)")
    _shade(axes[1, 2], height_fav, "3. height-favored blend")
    fig.suptitle("PROBE: cross-recipe biome blend (mountain<->grassland). Red line = biome boundary.", fontsize=12)
    fig.tight_layout()
    fig.savefig(OUT, dpi=92)
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
