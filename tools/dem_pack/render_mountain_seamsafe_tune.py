"""Tuning sweep: recover the accepted legacy mountain look in the seam-safe path.

Renders a row of variants at the SAME world coords:
  Panel 1: LEGACY (apron_px=0) — the target look.
  Panels 2-4: seam-safe core with 3 progressively stronger relief+incision tunings.

Each seam-safe variant overrides the module-level LOOK constants (which are the
real tuning surface), generates, then restores. The seam mechanism (apron +
nearest-mode blurs + crop + affine_remap) is untouched, so border delta stays 0.0.

Shading: LightSource(azdeg=315, altdeg=45), cmap terrain, vert_exag=2.0.
Output: D:/tmp/wg10_biome_compose/mountain_seamsafe_tune.png
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
from scipy.ndimage import gaussian_filter

import mountain_synthesis as ms

# ---------------------------------------------------------------------------
N_CORE = 96
S_CORE = 90_000.0
SEED   = 7
STYLE  = ms.STYLES[0]   # alpine_branching
APRON  = ms.MOUNTAIN_APRON_PX
cell = S_CORE / (N_CORE - 1)

# The LOOK levers that the sweep varies (names must match module attributes).
LOOK_KEYS = (
    "BASE_SCALE", "MASSIF_SCALE", "RIDGE_DETAIL_SCALE", "NEAR_DETAIL_SCALE",
    "FINAL_SCALE", "CHANNELS_DOG_SCALE",
    "DOG_TIGHT_SIGMA", "DOG_LOOSE_SIGMA",
    "PRIMARY_THRESH_LO", "PRIMARY_THRESH_HI",
    "TRIBUTARY_THRESH_LO", "TRIBUTARY_THRESH_HI",
    "SEAMSAFE_CARVE_GAIN", "SEAMSAFE_BRANCH_GAIN",
    "SEAMSAFE_RIDGE_GAIN", "SEAMSAFE_DETAIL_GAIN",
)

# Three progressively stronger relief + incision tunings.
# Variant B is the committed default; A is milder, C is stronger.
VARIANTS = {
    "A: mild": dict(
        BASE_SCALE=2.22, MASSIF_SCALE=0.70, RIDGE_DETAIL_SCALE=4.76, NEAR_DETAIL_SCALE=3.57,
        FINAL_SCALE=1.00, CHANNELS_DOG_SCALE=3.33,
        DOG_TIGHT_SIGMA=1.0, DOG_LOOSE_SIGMA=3.0,
        PRIMARY_THRESH_LO=0.22, PRIMARY_THRESH_HI=0.70,
        TRIBUTARY_THRESH_LO=0.24, TRIBUTARY_THRESH_HI=0.72,
        SEAMSAFE_CARVE_GAIN=1.15, SEAMSAFE_BRANCH_GAIN=1.08,
        SEAMSAFE_RIDGE_GAIN=1.10, SEAMSAFE_DETAIL_GAIN=1.06,
    ),
    "B: balanced (CHOSEN default)": dict(
        BASE_SCALE=2.28, MASSIF_SCALE=0.72, RIDGE_DETAIL_SCALE=4.85, NEAR_DETAIL_SCALE=3.60,
        FINAL_SCALE=0.80, CHANNELS_DOG_SCALE=3.50,
        DOG_TIGHT_SIGMA=0.9, DOG_LOOSE_SIGMA=2.8,
        PRIMARY_THRESH_LO=0.10, PRIMARY_THRESH_HI=0.50,
        TRIBUTARY_THRESH_LO=0.12, TRIBUTARY_THRESH_HI=0.54,
        SEAMSAFE_CARVE_GAIN=2.00, SEAMSAFE_BRANCH_GAIN=1.70,
        SEAMSAFE_RIDGE_GAIN=1.12, SEAMSAFE_DETAIL_GAIN=1.05,
    ),
    "C: strong": dict(
        BASE_SCALE=2.35, MASSIF_SCALE=0.74, RIDGE_DETAIL_SCALE=5.10, NEAR_DETAIL_SCALE=3.80,
        FINAL_SCALE=0.78, CHANNELS_DOG_SCALE=3.60,
        DOG_TIGHT_SIGMA=0.85, DOG_LOOSE_SIGMA=2.5,
        PRIMARY_THRESH_LO=0.08, PRIMARY_THRESH_HI=0.46,
        TRIBUTARY_THRESH_LO=0.10, TRIBUTARY_THRESH_HI=0.50,
        SEAMSAFE_CARVE_GAIN=2.40, SEAMSAFE_BRANCH_GAIN=2.00,
        SEAMSAFE_RIDGE_GAIN=1.25, SEAMSAFE_DETAIL_GAIN=1.15,
    ),
}


def _apply_look(d: dict) -> dict:
    """Set module LOOK constants from d; return the prior values for restore."""
    prior = {k: getattr(ms, k) for k in d}
    for k, v in d.items():
        setattr(ms, k, v)
    return prior


def _restore_look(prior: dict) -> None:
    for k, v in prior.items():
        setattr(ms, k, v)


def _padded_grid(seed_unused=None):
    padded_n = N_CORE + 2 * APRON
    total_cols = 2 * N_CORE + 2 * APRON - 1
    master_x = np.arange(total_cols, dtype=np.float64) * cell - APRON * cell
    master_z = np.arange(padded_n, dtype=np.float64) * cell - APRON * cell
    a_wx, a_wz = np.meshgrid(master_x[0:padded_n], master_z)
    b_wx, b_wz = np.meshgrid(master_x[N_CORE - 1:N_CORE - 1 + padded_n], master_z)
    return a_wx, a_wz, b_wx, b_wz


def _metrics(h):
    gy, gx = np.gradient(h)
    slope = np.sqrt(gx * gx + gy * gy)
    hp = float((h - gaussian_filter(h, sigma=2.0)).std())
    return dict(ptp=float(np.ptp(h)), std=float(h.std()),
                slope_p90=float(np.quantile(slope, 0.9)), hp=hp)


def _vdepth(res):
    z = res["height"]
    ch = np.maximum(res["primary_channels"], res["tributaries"])
    return float(z[ch <= np.quantile(ch, 0.4)].mean() - z[ch > np.quantile(ch, 0.85)].mean())


# --- Legacy target ---
wx0, wz0 = ms.grid(N_CORE, S_CORE)
res_legacy = ms.generate(wx0, wz0, seed=SEED, style=STYLE, apron_px=0)
h_legacy = res_legacy["height"]
m_legacy = _metrics(h_legacy)
v_legacy = _vdepth(res_legacy)
print(f"LEGACY: ptp={m_legacy['ptp']:.3f} std={m_legacy['std']:.3f} "
      f"slope_p90={m_legacy['slope_p90']:.4f} hp={m_legacy['hp']:.4f} vdepth={v_legacy:.3f}")

# --- Seam-safe variants ---
a_wx, a_wz, b_wx, b_wz = _padded_grid()
panels = [("LEGACY (apron_px=0)\nTARGET LOOK", h_legacy, m_legacy, v_legacy, None)]

for label, look in VARIANTS.items():
    prior = _apply_look(look)
    try:
        res_a = ms.generate(a_wx, a_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S_CORE)
        res_b = ms.generate(b_wx, b_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S_CORE)
    finally:
        _restore_look(prior)
    ha, hb = res_a["height"], res_b["height"]
    delta = float(np.max(np.abs(ha[:, -1] - hb[:, 0])))
    m = _metrics(ha)
    v = _vdepth(res_a)
    print(f"{label}: ptp={m['ptp']:.3f} std={m['std']:.3f} slope_p90={m['slope_p90']:.4f} "
          f"hp={m['hp']:.4f} vdepth={v:.3f} | seam delta={delta:.3e}")
    assert delta == 0.0, f"{label}: seam broke (delta={delta})"
    sub = (f"BASE_SCALE={look['BASE_SCALE']} FINAL_SCALE={look['FINAL_SCALE']}\n"
           f"carve×{look['SEAMSAFE_CARVE_GAIN']} ridge×{look['SEAMSAFE_RIDGE_GAIN']}\n"
           f"prim_thr=({look['PRIMARY_THRESH_LO']},{look['PRIMARY_THRESH_HI']}) "
           f"DoG=({look['DOG_TIGHT_SIGMA']},{look['DOG_LOOSE_SIGMA']})")
    panels.append((f"{label}\n{sub}", ha, m, v, delta))

# --- Render ---
ls = LightSource(azdeg=315, altdeg=45)
fig, axes = plt.subplots(1, 4, figsize=(24, 6.5))
fig.suptitle("Mountain seam-safe LOOK tuning sweep (all seam delta = 0.0)", fontsize=13, y=1.02)

for ax, (title, h, m, v, delta) in zip(axes, panels):
    rgb = ls.shade(h, cmap=plt.cm.terrain, vert_exag=2.0, blend_mode="soft")
    ax.imshow(rgb, origin="lower", interpolation="bilinear")
    ax.set_title(title, fontsize=8)
    ax.set_xticks([]); ax.set_yticks([])
    txt = (f"ptp={m['ptp']:.2f} std={m['std']:.2f}\n"
           f"slope_p90={m['slope_p90']:.3f}\nhp={m['hp']:.3f} vdep={v:.2f}")
    if delta is not None:
        txt += f"\nseam delta={delta:.1e}"
    ax.text(0.02, 0.02, txt, transform=ax.transAxes, fontsize=7, color="white",
            va="bottom", bbox=dict(facecolor="black", alpha=0.55, pad=2))

plt.tight_layout()
out_path = "D:/tmp/wg10_biome_compose/mountain_seamsafe_tune.png"
plt.savefig(out_path, dpi=150, bbox_inches="tight")
plt.close()
print(f"Saved: {out_path}")
assert os.path.exists(out_path)
print("Done.")
