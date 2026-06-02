r"""FAST block-grid biome review (Python image render; NO JSON, NO Godot).

Owner-requested review layout: each of the 11 biomes gets a SOLID block of itself, with TRANSITION
zones between blocks -- so the owner can see (1) each biome being itself, (2) biome<->biome
transitions, (3) the whole composed system, all at once. 4x3 grid of 5x5-chunk blocks (one empty
cell), ~125 km per biome block.

Uses the seam-safe biomes via the registry (legacy apron_px=0 path = fast; seams already proven
per-biome). Per-biome relief makes tall biomes tower. Vertical scale is a tunable knob (DRAMATIC
alpine target). Renders straight to a hillshade PNG in seconds for fast look iteration.

Run:    python tools/dem_pack/render_biome_blocks_fast.py
Writes: D:/tmp/wg10_biome_compose/biome_blocks_fast.png
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
import biome_registry as br
import biome_compose as bc

OUT = Path("D:/tmp/wg10_biome_compose/biome_blocks_fast.png")

# --- layout: 4 cols x 3 rows of biome blocks (11 biomes, 1 empty) ---
LAYOUT = [
    ["mountain",  "volcanic",  "glacial",   "karst"],
    ["temperate", "rainforest","grassland", "tundra"],
    ["desert",    "coast",     "wetland",   None],
]
COLS, ROWS = 4, 3
# SCALE CONTRACT (on-foot default, real meters). Region ~30 km per biome.
REGION_SPAN_M = 30_000.0        # a biome region = ~30 km (holds ~8 features -> a real range)
PX_PER_BLOCK = 200              # render resolution per region
TRANSITION_FRAC = 0.28          # fraction of a region width over which neighbors blend at borders
SEED = 219

# PER-BIOME scale contract: feature_span (km, sets feature SIZE -> steepness) + relief_m (peak
# height in real metres). slope ratio ~= relief_m / feature_span_m. Mountain ~1000m/3.5km = ~0.29
# (dramatic alpine); lowlands wide+flat. These are the anchored numbers, not eyeball multipliers.
BIOME_SCALE = {
    # name:         (feature_span_m, relief_m)
    "mountain":     (3_500.0,  1000.0),   # slope ~0.29 dramatic alpine
    "volcanic":     (4_000.0,   850.0),   # ~0.21 big cones/shields
    "glacial":      (5_000.0,   700.0),   # ~0.14 broad troughs/icefields
    "karst":        (3_000.0,   550.0),   # ~0.18 towers/dolines, tight
    "rainforest":   (5_000.0,   450.0),   # ~0.09 rolling forested hills
    "temperate":    (6_000.0,   380.0),   # ~0.06 gentle folds
    "tundra":       (7_000.0,   280.0),   # ~0.04 low patterned ground
    "desert":       (5_000.0,   300.0),   # ~0.06 dunes/mesas
    "grassland":    (8_000.0,   220.0),   # ~0.03 swells
    "coast":        (6_000.0,   200.0),   # ~0.03 low coastal relief
    "wetland":      (9_000.0,   110.0),   # ~0.01 near-flat
}
VERT_EXAG = 1.0                 # display: NO fake exaggeration -- show TRUE relief:span (the contract's point)


def _smoothstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0 + 1e-9), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    world_px_x = COLS * PX_PER_BLOCK
    world_px_z = ROWS * PX_PER_BLOCK
    world_span_x = COLS * REGION_SPAN_M
    world_span_z = ROWS * REGION_SPAN_M
    print(f"Scale-contract block review: {COLS}x{ROWS} regions of {REGION_SPAN_M/1000:.0f} km; "
          f"world {world_span_x/1000:.0f}x{world_span_z/1000:.0f} km; render {world_px_x}x{world_px_z}")

    wx, wz = _grid_xy(world_px_x, world_px_z, world_span_x, world_span_z)

    bx = np.linspace(0, COLS, world_px_x, endpoint=False) + 0.5 / PX_PER_BLOCK
    bz = np.linspace(0, ROWS, world_px_z, endpoint=False) + 0.5 / PX_PER_BLOCK
    BX, BZ = np.meshgrid(bx, bz)

    biome_to_field = {}     # real-METRE height fields
    weight_fields = {}
    for r in range(ROWS):
        for c in range(COLS):
            name = LAYOUT[r][c]
            if name is None:
                continue
            wxw = _smoothstep(c - TRANSITION_FRAC, c + TRANSITION_FRAC, BX) * \
                  (1.0 - _smoothstep(c + 1 - TRANSITION_FRAC, c + 1 + TRANSITION_FRAC, BX))
            wzw = _smoothstep(r - TRANSITION_FRAC, r + TRANSITION_FRAC, BZ) * \
                  (1.0 - _smoothstep(r + 1 - TRANSITION_FRAC, r + 1 + TRANSITION_FRAC, BZ))
            weight_fields[name] = weight_fields.get(name, 0.0) + wxw * wzw
            if name not in biome_to_field:
                fspan, relief_m = BIOME_SCALE[name]
                print(f"  generate {name} (feature_span={fspan/1000:.1f}km relief={relief_m:.0f}m slope~{relief_m/fspan:.2f}) ...")
                h = np.asarray(br.get_recipe(name).generate(wx, wz, seed=SEED, feature_span_m=fspan), dtype=np.float64)
                # normalize to ~unit std, then scale to REAL METRES (peak-to-peak ~= relief_m)
                h = (h - h.mean()) / (h.std() + 1e-9)
                h = h * (relief_m / 4.0)          # std-1 field spans ~+-2 std -> ptp ~4*std -> relief_m
                biome_to_field[name] = h

    names = list(biome_to_field.keys())
    fields = [biome_to_field[n] for n in names]
    wsum = np.sum(np.stack([weight_fields[n] for n in names], axis=0), axis=0) + 1e-9
    weights = [weight_fields[n] / wsum for n in names]

    composed = bc.compose_biomes(fields, weights, bc.BlendConfig(mode="height_favored"))
    cm = float(np.ptp(composed))
    print(f"  composed real-metre range [{composed.min():.0f}m, {composed.max():.0f}m] ptp={cm:.0f}m")

    # TRUE-SCALE hillshade: dx/dz spacing in metres so slopes are real (vert_exag=1.0 = honest).
    cell_m = world_span_x / world_px_x
    fig, ax = plt.subplots(figsize=(22, 17))
    hn = (composed - composed.min()) / (cm + 1e-9)
    rgb = LightSource(azdeg=315, altdeg=45).shade(
        composed, cmap=plt.cm.terrain, vert_exag=VERT_EXAG, blend_mode="soft", dx=cell_m, dy=cell_m)
    ax.imshow(rgb); ax.axis("off")
    for r in range(ROWS):
        for c in range(COLS):
            name = LAYOUT[r][c]
            if name:
                fspan, relief_m = BIOME_SCALE[name]
                ax.text((c + 0.5) * PX_PER_BLOCK, (r + 0.10) * PX_PER_BLOCK,
                        f"{name}\n{relief_m:.0f}m / {fspan/1000:.0f}km",
                        color="black", fontsize=9, ha="center", va="top",
                        bbox=dict(boxstyle="round", fc="white", alpha=0.6, ec="none"))
    ax.set_title(f"Scale contract: {REGION_SPAN_M/1000:.0f} km regions, real-metre relief, TRUE slopes (vert_exag={VERT_EXAG}). "
                 f"World {world_span_x/1000:.0f} km. composed ptp {cm:.0f} m.", fontsize=13)
    fig.tight_layout()
    fig.savefig(OUT, dpi=90)
    print(f"  wrote {OUT}")


def _grid_xy(nx, nz, span_x, span_z, ox=60_000.0, oz=36_000.0):
    xs = np.linspace(0.0, span_x, nx) + ox
    zs = np.linspace(0.0, span_z, nz) + oz
    return np.meshgrid(xs, zs)


if __name__ == "__main__":
    main()
