"""Render a side-by-side comparison of the mountain synth before and after seam-safe changes.

Panels:
  Left  (1): new generate with apron_px=0 (legacy mode, no apron)
  Middle (2): new generate with apron_px=MOUNTAIN_APRON_PX (seam-safe, core crop)
  Right  (3): two adjacent seam-safe windows stitched horizontally (no visible seam)

Shading: matplotlib LightSource (azdeg=315, altdeg=45, cmap terrain, vert_exag=2.0).
Output: D:/tmp/wg10_biome_compose/mountain_seamsafe_compare.png
"""
from __future__ import annotations

import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__)))

import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.colors import LightSource

import geography_engine as geo
import mountain_synthesis as ms

# ---------------------------------------------------------------------------
# Common settings
# ---------------------------------------------------------------------------
N_CORE = 96            # core grid cells
S_CORE = 90_000.0      # core world span in metres
SEED   = 7
STYLE  = ms.STYLES[0]  # alpine_branching
APRON  = ms.MOUNTAIN_APRON_PX

cell = S_CORE / (N_CORE - 1)

# ---------------------------------------------------------------------------
# Panel 1: legacy mode (apron_px=0) — same code path as before the change
# ---------------------------------------------------------------------------
wx0, wz0 = ms.grid(N_CORE, S_CORE)
res_legacy = ms.generate(wx0, wz0, seed=SEED, style=STYLE, apron_px=0)
h_legacy = res_legacy["height"]
print(f"Legacy (apron=0): shape={h_legacy.shape}, "
      f"min={h_legacy.min():.3f}, max={h_legacy.max():.3f}, std={h_legacy.std():.3f}")

# ---------------------------------------------------------------------------
# Panel 2: seam-safe core (apron_px=APRON) — same world extent as panel 1
# Build using master-array approach so panel-3's A grid shares exact border floats.
# ---------------------------------------------------------------------------
padded_n = N_CORE + 2 * APRON

# Master 1-D arrays for X and Z spanning both windows + aprons (used for panels 2 and 3)
total_cols = 2 * N_CORE + 2 * APRON - 1
master_x = np.arange(total_cols, dtype=np.float64) * cell - APRON * cell  # origin at 0
master_z = np.arange(padded_n, dtype=np.float64) * cell - APRON * cell

# Window A padded grid (panel 2 = same as panel 3 window A)
a_x_1d = master_x[0:padded_n]
z_1d   = master_z
a_wx_p2, a_wz_p2 = np.meshgrid(a_x_1d, z_1d)
res_apron = ms.generate(a_wx_p2, a_wz_p2, seed=SEED, style=STYLE, apron_px=APRON,
                        feature_span_m=S_CORE)
h_apron = res_apron["height"]
print(f"Seam-safe core (apron={APRON}): shape={h_apron.shape}, "
      f"min={h_apron.min():.3f}, max={h_apron.max():.3f}, std={h_apron.std():.3f}")

# ---------------------------------------------------------------------------
# Panel 3: two adjacent seam-safe windows stitched (verifies zero-seam visually)
# ---------------------------------------------------------------------------
# Window A: x in [0, S_CORE]  Window B: x in [S_CORE, 2*S_CORE]
# Both slice from the same master array → bit-identical border.
b_x_1d = master_x[N_CORE - 1:N_CORE - 1 + padded_n]

a_wx2, a_wz2 = a_wx_p2, a_wz_p2   # reuse panel 2 grid for window A
b_wx2, b_wz2 = np.meshgrid(b_x_1d, z_1d)

res_a = res_apron   # already computed above (same grid as a_wx2/a_wz2)
res_b = ms.generate(b_wx2, b_wz2, seed=SEED, style=STYLE, apron_px=APRON,
                    feature_span_m=S_CORE)

ha = res_a["height"]
hb = res_b["height"]

# Verify border delta is zero before stitching
border_delta = float(np.max(np.abs(ha[:, -1] - hb[:, 0])))
print(f"Stitched seam border delta = {border_delta:.6e}  (must be 0.0)")
assert border_delta == 0.0, f"Seam not exact: delta={border_delta}"

# Stitch A and B side-by-side (share the border column → use A's right = B's left)
h_stitched = np.concatenate([ha, hb[:, 1:]], axis=1)
print(f"Stitched: shape={h_stitched.shape}")

# ---------------------------------------------------------------------------
# Render
# ---------------------------------------------------------------------------
ls = LightSource(azdeg=315, altdeg=45)

fig, axes = plt.subplots(1, 3, figsize=(18, 6))
fig.suptitle("Mountain synth — seam-safe comparison", fontsize=13, y=1.01)

titles = [
    f"Legacy (apron_px=0)",
    f"Seam-safe core (apron_px={APRON})",
    "Two adjacent windows stitched\n(seam border delta = 0.0)",
]
fields = [h_legacy, h_apron, h_stitched]

for ax, h, title in zip(axes, fields, titles):
    rgb = ls.shade(h, cmap=plt.cm.terrain, vert_exag=2.0, blend_mode="soft")
    ax.imshow(rgb, origin="lower", interpolation="bilinear")
    # Mark the seam position in panel 3
    if "stitched" in title.lower():
        ax.axvline(x=N_CORE - 1, color="red", linewidth=1.2, linestyle="--", alpha=0.7,
                   label=f"seam at col {N_CORE-1}")
        ax.legend(fontsize=8, loc="upper right")
    ax.set_title(title, fontsize=10)
    ax.set_xlabel(f"cols: {h.shape[1]}")
    ax.set_ylabel(f"rows: {h.shape[0]}")
    # Add height stats as text
    ax.text(0.02, 0.04, f"min={h.min():.2f}  max={h.max():.2f}  std={h.std():.2f}",
            transform=ax.transAxes, fontsize=7, color="white",
            bbox=dict(facecolor="black", alpha=0.5, pad=2))

plt.tight_layout()
out_path = "D:/tmp/wg10_biome_compose/mountain_seamsafe_compare.png"
plt.savefig(out_path, dpi=150, bbox_inches="tight")
plt.close()
print(f"Saved: {out_path}")
assert os.path.exists(out_path), f"PNG not found at {out_path}"
print("Done.")
