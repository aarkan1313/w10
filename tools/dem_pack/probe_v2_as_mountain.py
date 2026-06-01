r"""PROBE (throwaway, render-first de-risk): can the v2 engine's PARAM SPACE express a mountain?

Decides the biome-composition Fork A vs B: is "biomes = presets of ONE v2 engine" feasible
(Fork A), or do biomes need to keep their own recipes (Fork B)? We render, on the SAME window:
  - the real mountain_synthesis output (the accepted mountain look), and
  - v2 with default params + a few param sets cranked toward "mountain" (relief/ridge/peakiness up,
    final blur down).
If some v2 param set reads as a believable mountain range next to the synth, Fork A is viable.
If v2 can't reach it no matter the knobs, Fork B (keep per-biome recipes) is the honest call.

Run:  python tools/dem_pack/probe_v2_as_mountain.py
Writes: D:/tmp/wg10_biome_compose/probe_v2_as_mountain.png
"""
from __future__ import annotations

from pathlib import Path

import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.colors import LightSource

import keeper_v2 as v2
import geography_skeleton_windows as win
import export_godot_rough_world_chunks as ex
import mountain_synthesis as mountain

OUT = Path("D:/tmp/wg10_biome_compose/probe_v2_as_mountain.png")
N = 257
SEED = 133


def _shade(ax, h, title):
    h = np.asarray(h, dtype=np.float64)
    hn = (h - h.min()) / (np.ptp(h) + 1e-9)
    ls = LightSource(azdeg=315, altdeg=45)
    rgb = ls.shade(hn, cmap=plt.cm.terrain, vert_exag=2.0, blend_mode="soft")
    ax.imshow(rgb)
    ax.set_title(f"{title}\nptp={np.ptp(h):.3f}", fontsize=9)
    ax.axis("off")


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    spec = ex._window_spec(N, ex.CHUNK_SPAN_M)
    w = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, SEED, spec)

    # Real mountain synth on the SAME world coords as the v2 window.
    wx = np.asarray(w["wx"]); wz = np.asarray(w["wz"])
    core = win._core_slice(spec)
    msynth = mountain.generate(wx, wz, seed=SEED, feature_span_m=90_000.0)["height"]
    msynth = np.asarray(msynth)[core, core]

    # v2 param sets: default, then cranked toward "mountain".
    sets = {
        "v2 default": v2.KeeperV2Params(),
        "v2 mountain-ish A": v2.KeeperV2Params(
            relief_amplitude=3.2, range_texture_gain=0.62, post_tanh_gain=1.6,
            final_blur_mix=0.08, incision_gain=1.3),
        "v2 mountain-ish B": v2.KeeperV2Params(
            relief_amplitude=4.0, range_texture_gain=0.85, post_tanh_gain=2.1,
            final_blur_mix=0.04, incision_gain=1.5, fine_gain=0.18),
    }
    heights = {name: v2.compose_windowed_height_v2(w, SEED, spec, p) for name, p in sets.items()}

    fig, axes = plt.subplots(2, 2, figsize=(11, 11))
    _shade(axes[0, 0], msynth, "REAL mountain_synthesis (target look)")
    _shade(axes[0, 1], heights["v2 default"], "v2 default params")
    _shade(axes[1, 0], heights["v2 mountain-ish A"], "v2 cranked toward mountain (A)")
    _shade(axes[1, 1], heights["v2 mountain-ish B"], "v2 cranked toward mountain (B)")
    fig.suptitle("PROBE: can v2's param space express a mountain? (Fork A feasibility)", fontsize=12)
    fig.tight_layout()
    fig.savefig(OUT, dpi=96)
    print(f"wrote {OUT}")
    for name, h in heights.items():
        print(f"  {name:22s} ptp={np.ptp(h):.3f} std={np.std(h):.3f}")
    print(f"  {'REAL mountain synth':22s} ptp={np.ptp(msynth):.3f} std={np.std(msynth):.3f}")


if __name__ == "__main__":
    main()
