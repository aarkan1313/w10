r"""FAST multi-biome compose look-iteration (Python image render; NO JSON, NO Godot).

The JSON->Godot loop was minutes/iteration. This renders a big organic multi-biome world straight
to a hillshade PNG in seconds so the owner can iterate the LOOK on stills (render-images-first
methodology). Two fixes vs the first compose attempt:
  1. PER-BIOME RELIEF: each biome carries its own relief multiplier (mountain tall, wetland flat) so
     biomes keep their individuality and mountains tower instead of being averaged to sameness.
  2. BIG REGIONS: a large world span so each biome occupies real area before blending into the next.
Seams are NOT tested here (already proven per-biome); we use the fast legacy generate (apron_px=0)
just for look iteration. Final 3D acceptance still goes through a Godot fly.

Run:    python tools/dem_pack/render_biome_compose_fast.py
Writes: D:/tmp/wg10_biome_compose/biome_compose_fast.png
"""
from __future__ import annotations

import sys
try:
    sys.stdout.reconfigure(encoding="utf-8")
except (AttributeError, ValueError):
    pass

from pathlib import Path
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.colors import LightSource

import geography_engine as geo
import worldgen_proto as wg
import biome_registry as br
import biome_compose as bc

OUT = Path("D:/tmp/wg10_biome_compose/biome_compose_fast.png")

# --- config (tune these by eye) ---
SEED = 219
N = 640                          # render resolution
WORLD_SPAN_M = 200_000.0         # CLOSER crop (200 km) so biomes read at a believable scale
FEATURE_SPAN_M = 90_000.0
OX, OZ = 60_000.0, 36_000.0
WEIGHT_FREQ = 1.0 / 70_000.0     # region size ~70 km -> a few biomes across a 200 km crop, each breathes
SOFTMAX_TEMP = 0.16

BIOMES = ["mountain", "volcanic", "glacial", "grassland", "desert", "wetland"]

# Three relief-separation variants (mountain anchored at 1.0). Higher contrast = mountains tower more,
# lowlands flatter. The fix for "biomes lose individuality / mountains not tall".
RELIEF_VARIANTS = {
    "moderate":  {"mountain": 1.00, "volcanic": 0.80, "glacial": 0.60, "grassland": 0.30, "desert": 0.38, "wetland": 0.18},
    "strong":    {"mountain": 1.00, "volcanic": 0.72, "glacial": 0.50, "grassland": 0.16, "desert": 0.24, "wetland": 0.08},
    "dramatic":  {"mountain": 1.00, "volcanic": 0.62, "glacial": 0.40, "grassland": 0.09, "desert": 0.15, "wetland": 0.04},
}


def _organic_weights(wx, wz):
    aff = [wg.fbm(wx, wz, WEIGHT_FREQ, 4, SEED + 300 + 17 * i, gain=0.55) for i in range(len(BIOMES))]
    s = np.stack(aff, axis=0) / SOFTMAX_TEMP
    s = s - s.max(axis=0, keepdims=True)
    e = np.exp(s)
    w = e / (np.sum(e, axis=0, keepdims=True) + 1e-9)
    return [w[i] for i in range(len(BIOMES))]


def _shade(ax, composed, title):
    hn = (composed - composed.min()) / (np.ptp(composed) + 1e-9)
    rgb = LightSource(azdeg=315, altdeg=45).shade(hn, cmap=plt.cm.terrain, vert_exag=3.0, blend_mode="soft")
    ax.imshow(rgb); ax.axis("off"); ax.set_title(title, fontsize=11)


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    wx, wz = geo.grid(N, WORLD_SPAN_M, ox=OX, oz=OZ)
    print(f"FAST multi-biome compose: {N}x{N}, {WORLD_SPAN_M/1000:.0f} km crop, biomes={BIOMES}")

    # generate each biome ONCE (normalized to std 1); relief is applied per-variant after.
    base = {}
    for name in BIOMES:
        print(f"  generate {name} ...")
        h = np.asarray(br.get_recipe(name).generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M), dtype=np.float64)
        base[name] = (h - h.mean()) / (h.std() + 1e-9)
    weights = _organic_weights(wx, wz)
    dom = np.argmax(np.stack(weights, axis=0), axis=0)

    fig, ax = plt.subplots(2, 2, figsize=(20, 20))
    for k, (vname, relief) in enumerate(RELIEF_VARIANTS.items()):
        fields = [base[name] * relief[name] for name in BIOMES]
        composed = bc.compose_biomes(fields, weights, bc.BlendConfig(mode="height_favored"))
        r = k // 2; c = k % 2
        _shade(ax[r][c], composed, f"relief={vname}  std={composed.std():.2f}  (mtn x1.0 ... wetland x{relief['wetland']})")
        print(f"  {vname}: std={composed.std():.3f} range[{composed.min():.2f},{composed.max():.2f}]")

    # biome region map in the 4th panel
    import matplotlib.patches as mpatches
    ax[1][1].imshow(dom, cmap="tab10", vmin=0, vmax=9); ax[1][1].axis("off")
    ax[1][1].set_title("dominant biome region", fontsize=11)
    handles = [mpatches.Patch(color=plt.cm.tab10(i / 9.0), label=BIOMES[i]) for i in range(len(BIOMES))]
    ax[1][1].legend(handles=handles, loc="upper right", fontsize=9)

    fig.suptitle(f"Multi-biome compose @ {WORLD_SPAN_M/1000:.0f} km -- relief-separation variants (mountains should tower)", fontsize=13)
    fig.tight_layout()
    fig.savefig(OUT, dpi=100)
    print(f"  wrote {OUT}")


if __name__ == "__main__":
    main()
