r"""PROBE (faithful): does flow accumulation on the mountain synth's REAL base field converge to
float32-epsilon seams at a reasonable apron? Validates the assumption before swapping DoG->flow.

We monkeypatch mountain_synthesis._flow_channels_seam_safe to instead compute real flow accumulation
(skel._flow_accumulation_mfd) with the seam-safe fixed-max normalization, run the REAL generate on
two adjacent apron-padded windows, and measure the cropped-core border delta of the FINAL height
across apron sizes. If delta hits float32 epsilon (~1e-7) at a modest apron, the swap is validated.

Run: python tools/dem_pack/probe_flow_seam_real.py
"""
from __future__ import annotations
import numpy as np
from scipy.ndimage import gaussian_filter
import geography_engine as geo
import geography_skeleton as skel
import mountain_synthesis as mountain

SEED = 7
SPAN_M = 90_000.0
CORE_N = 129
OX, OZ = 60_000.0, 36_000.0
FEATURE_SPAN_M = 90_000.0

_orig = mountain._flow_channels_seam_safe


def _flow_real(surface, width_px, mode="nearest", **kw):
    """Drop-in for the DoG proxy: real flow accumulation + seam-safe fixed-max normalization."""
    acc = skel._flow_accumulation_mfd(gaussian_filter(surface, sigma=1.15, mode=mode), power=0.48 + 0.0 * width_px)
    discharge = np.clip(np.log1p(acc) / np.log1p(float(acc.size)), 0.0, 1.0)
    # spread channel extent like the original second blur, seam-safe (nearest)
    return np.clip(gaussian_filter(discharge, sigma=max(float(width_px), 0.1), mode=mode), 0.0, 1.0)


def _gen(core_n, span, ox, oz, apron_px):
    cell = span / (core_n - 1)
    pn = core_n + 2 * apron_px
    pspan = cell * (pn - 1)
    wx, wz = geo.grid(pn, pspan, ox=ox - apron_px * cell, oz=oz - apron_px * cell)
    h = mountain.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M, apron_px=apron_px)["height"]
    return np.asarray(h, float)


def main() -> None:
    mountain._flow_channels_seam_safe = _flow_real  # swap in real flow accumulation
    try:
        print("apron_px | final-height border_delta_max | mean   (real flow-accumulation valleys)")
        for ap in (40, 80, 128, 200):
            a = _gen(CORE_N, SPAN_M, OX, OZ, ap)
            b = _gen(CORE_N, SPAN_M, OX + SPAN_M, OZ, ap)
            dmax = float(np.max(np.abs(a[:, -1] - b[:, 0])))
            dmean = float(np.mean(np.abs(a[:, -1] - b[:, 0])))
            print(f"  {ap:5d}  | {dmax:.3e}                  | {dmean:.3e}")
    finally:
        mountain._flow_channels_seam_safe = _orig


if __name__ == "__main__":
    main()
