r"""PROBE (throwaway tuning sweep): find clash-blend settings that suppress mountain/dune overlap.

Renders a grid of height_favored variants plus a mask-handoff alternative so the human can
pick the cleanest defaults for the mountain<->desert boundary.

Run:   python tools/dem_pack/probe_biome_blend_clash_tune.py
Writes: D:/tmp/wg10_biome_compose/probe_biome_blend_clash_tune.png
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

OUT = Path("D:/tmp/wg10_biome_compose/probe_biome_blend_clash_tune.png")
N, SEED, SPAN_M, FEATURE_SPAN_M = 320, 133, 60_000.0, 90_000.0


def _smoothstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0 + 1e-9), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def _make_w_a(u, band_frac):
    """Build mountain weight field (1=mountain, 0=desert) with given band width."""
    return 1.0 - _smoothstep(0.5 - band_frac, 0.5 + band_frac, u)


def height_favored(a, b, w_a, band_frac, favor_strength, relief_sigma):
    """Height-favored blend: boost whichever terrain has higher local relief in the transition."""
    relief_a = np.abs(a - gaussian_filter(a, sigma=relief_sigma))
    relief_b = np.abs(b - gaussian_filter(b, sigma=relief_sigma))
    favor = relief_a / (relief_a + relief_b + 1e-9)
    band = 1.0 - np.abs(2.0 * w_a - 1.0)
    w_adj = np.clip(w_a + (favor - 0.5) * favor_strength * band, 0.0, 1.0)
    return w_adj * a + (1.0 - w_adj) * b


def mask_handoff(a, b, u, seam_frac):
    """Alternative: pick A where w_a>0.5 else B, feathered only in a thin seam."""
    # For pixels firmly in A-territory (u < 0.5-seam/2) use pure A.
    # For pixels firmly in B-territory (u > 0.5+seam/2) use pure B.
    # Within the thin seam around u=0.5, smoothstep-blend.
    w = 1.0 - _smoothstep(0.5 - seam_frac, 0.5 + seam_frac, u)
    return w * a + (1.0 - w) * b


def _shade(ax, h, title):
    h = np.asarray(h, float)
    hn = (h - h.min()) / (np.ptp(h) + 1e-9)
    rgb = LightSource(azdeg=315, altdeg=45).shade(
        hn, cmap=plt.cm.terrain, vert_exag=2.0, blend_mode="soft"
    )
    ax.imshow(rgb)
    ax.set_title(title, fontsize=8, wrap=True)
    ax.axis("off")
    ax.axvline(h.shape[1] * 0.5, color="red", lw=0.7, alpha=0.6)


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)

    # --- Generate terrain fields (same as reference probe) ---
    wx, wz = geo.grid(N, SPAN_M, ox=60_000.0, oz=36_000.0)
    mtn = np.asarray(mountain.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M)["height"], float)
    des = np.asarray(desert.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M)["height"], float)
    # z-score each
    mtn = (mtn - mtn.mean()) / (mtn.std() + 1e-9)
    des = (des - des.mean()) / (des.std() + 1e-9)

    # Horizontal blend coordinate (0=left/mountain → 1=right/desert)
    u = np.linspace(0.0, 1.0, N)[None, :].repeat(N, axis=0)

    # ----------------------------------------------------------------
    # Define all variants:
    #   Row 0: reference panels (mountain raw, desert raw, + variant)
    #   Rows 1-3: tuning grid
    # Layout: 4 rows x 3 cols = 12 panels
    # ----------------------------------------------------------------

    # Panel definitions: (row, col, height_field, label)
    panels = []

    # Row 0 col 0: raw mountain
    panels.append((0, 0, mtn, "RAW mountain"))
    # Row 0 col 1: raw desert
    panels.append((0, 1, des, "RAW desert dunes"))
    # Row 0 col 2: current default (baseline reference)
    bf, fs, rs = 0.16, 0.9, 6.0
    w_a = _make_w_a(u, bf)
    h = height_favored(mtn, des, w_a, bf, fs, rs)
    panels.append((0, 2, h, f"current default\nband={bf} favor={fs} sigma={rs}"))

    # Row 1: Narrower bands — band_frac varies, favor_strength=0.9, sigma=6.0
    for col, bf in enumerate([0.16, 0.09, 0.05]):
        w_a = _make_w_a(u, bf)
        h = height_favored(mtn, des, w_a, bf, favor_strength=0.9, relief_sigma=6.0)
        label = f"narrow band\nband={bf} favor=0.9 σ=6"
        if bf == 0.16:
            label = f"band sweep (ref)\nband={bf} favor=0.9 σ=6"
        panels.append((1, col, h, label))

    # Row 2: Stronger favoring — favor_strength varies, band_frac=0.09, sigma=6.0
    for col, fs in enumerate([0.9, 1.4, 2.0]):
        bf = 0.09
        w_a = _make_w_a(u, bf)
        h = height_favored(mtn, des, w_a, bf, favor_strength=fs, relief_sigma=6.0)
        panels.append((2, col, h, f"favor sweep\nband=0.09 favor={fs} σ=6"))

    # Row 3: Alternative mechanism — mask handoff thin seam, plus two more variants
    # Col 0: mask handoff thin seam (band_frac=0.03)
    h_mask = mask_handoff(mtn, des, u, seam_frac=0.03)
    panels.append((3, 0, h_mask, "mask handoff (thin seam)\nseam=0.03, no overlap"))

    # Col 1: height_favored, tightest band + max favoring (most aggressive suppression)
    bf, fs, rs = 0.05, 2.0, 6.0
    w_a = _make_w_a(u, bf)
    h = height_favored(mtn, des, w_a, bf, fs, rs)
    panels.append((3, 1, h, f"aggressive\nband={bf} favor={fs} σ=6"))

    # Col 2: height_favored with finer sigma (more local relief sensitivity)
    bf, fs, rs = 0.09, 1.4, 3.0
    w_a = _make_w_a(u, bf)
    h = height_favored(mtn, des, w_a, bf, fs, rs)
    panels.append((3, 2, h, f"fine relief σ\nband={bf} favor={fs} σ={rs}"))

    # --- Render ---
    nrows, ncols = 4, 3
    fig, axes = plt.subplots(nrows, ncols, figsize=(16, 22))
    for row, col, h, title in panels:
        _shade(axes[row, col], h, title)

    fig.suptitle(
        "TUNE: mountain↔desert clash-blend variants. Red = boundary.\n"
        "Row 0: raw refs + current default | Row 1: band sweep | Row 2: favor sweep | Row 3: alternatives",
        fontsize=11,
    )
    fig.tight_layout()
    fig.savefig(OUT, dpi=92)
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
