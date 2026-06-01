r"""PROBE: how seam-exact is apron'd flow-accumulation drainage across adjacent windows?

The mountain pilot replaced flow_accumulation (global) with a local DoG proxy to get bit-exact
seams (delta 0.0) — but that lost connected valleys. Owner wants seam-safe CONNECTED drainage.
The skeleton-windows layer already computes flow accumulation on world-anchored windows and reports
corridor match_frac=1.00 (visually joined). BUT flow accumulation depends on upstream area beyond
the apron, so adjacent windows may NOT be bit-exact at the border. This probe MEASURES the border
delta of apron'd flow-accumulation discharge for two adjacent windows at increasing apron sizes —
to decide whether the seam-safe-connected-drainage bar is "bit-exact" (delta->0) or only
"visually joined" (small-but-nonzero).

Run:    python tools/dem_pack/probe_flow_seam.py
"""
from __future__ import annotations
import numpy as np
from scipy.ndimage import gaussian_filter
import geography_engine as geo
import worldgen_proto as wg
import geography_skeleton as skel

SEED = 7
SPAN_M = 90_000.0
CORE_N = 129
OX, OZ = 60_000.0, 36_000.0


def _surface(core_n, span, ox, oz, apron_px):
    """A mountain-ish routed surface on an apron-padded world window, cropped back to core."""
    cell = span / (core_n - 1)
    pn = core_n + 2 * apron_px
    pspan = cell * (pn - 1)
    pox = ox - apron_px * cell
    poz = oz - apron_px * cell
    wx, wz = geo.grid(pn, pspan, ox=pox, oz=poz)
    w_x, w_z = wg.recursive_domain_warp(wx, wz, warp_amount=span * 0.05, warp_freq=1.0 / (span * 0.72),
                                        seed=SEED + 10, steps=3, decay=0.58, freq_mul=1.75)
    surf = wg.fbm(w_x, w_z, 1.0 / (span * 0.88), 5, SEED + 20, gain=0.56)
    acc = skel._flow_accumulation_mfd(gaussian_filter(surf, sigma=0.7), power=1.45)
    # seam-safe normalization: fixed theoretical max (acc.size), NOT per-window max
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    return discharge[apron_px:pn - apron_px, apron_px:pn - apron_px] if apron_px else discharge


def main() -> None:
    print("apron_px | border_delta_max | border_delta_mean | (discharge range ~0..1)")
    for ap in (0, 16, 40, 80, 160):
        # Window A core [OX, OX+SPAN]; window B core [OX+SPAN, OX+2*SPAN] (adjacent, shared border x=OX+SPAN)
        a = _surface(CORE_N, SPAN_M, OX, OZ, ap)
        b = _surface(CORE_N, SPAN_M, OX + SPAN_M, OZ, ap)
        # A's right core column and B's left core column are the SAME world x only if the core grids abut.
        # geo.grid uses linspace [0,span]; A's last col world-x = OX+SPAN, B's first col world-x = OX+SPAN. Shared.
        dmax = float(np.max(np.abs(a[:, -1] - b[:, 0])))
        dmean = float(np.mean(np.abs(a[:, -1] - b[:, 0])))
        print(f"  {ap:5d}  | {dmax:.6e}    | {dmean:.6e}")


if __name__ == "__main__":
    main()
