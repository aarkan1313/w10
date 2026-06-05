"""Emit the G-seam measurement fixture for two adjacent regions A and B.

Region B's origin = Region A's origin + SOURCE_SPAN_M in X (same Z), so A's
right edge abuts B's left edge.  For each region we run the SEAM-SAFE mountain
macro -> pass-network carve and write ONLY the carved fields (raw + delta).
The Rust side runs condition_world itself and measures the border seam.

Run from repo root:
    python tools/dem_pack/export_region_seam_fixture.py
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

import mountain_synthesis as mountain
import mountain_pass_network as mpn
import corridor_router as cr
import geography_skeleton_windows as win

HERE = Path(__file__).resolve().parent
OUT_FIXTURE = HERE / "fixtures" / "region_seam_fixture.json"

# --- constants (match export_bake_region_fixture.py, smaller grid for speed) ---
SAMPLE_N = 129
FEATURE_SPAN_M = 90_000.0
HEIGHT_SCALE_M = 1700.0
SEED = 177
SOURCE_SPAN_M = 270_000.0
SOURCE_ORIGIN_X = 207_000.0
SOURCE_ORIGIN_Z = 176_000.0


def _bake_region(origin_x: float, origin_z: float) -> np.ndarray:
    """Run the seam-safe mountain macro -> pass-network carve for one region.

    Returns the carved field (raw + delta) as a 2-D float64 array of shape
    (SAMPLE_N, SAMPLE_N).  Mirrors export_bake_region_fixture.py exactly.
    """
    # ------------------------------------------------------------------
    # 1) SEAM-SAFE mountain macro (mirror _live_seamsafe_page exactly).
    # ------------------------------------------------------------------
    apron_px = int(mountain.MOUNTAIN_APRON_PX)
    spacing_m = SOURCE_SPAN_M / float(SAMPLE_N - 1)
    padded_n = SAMPLE_N + 2 * apron_px
    padded_span_m = SOURCE_SPAN_M + 2.0 * float(apron_px) * spacing_m
    wx, wz = mountain.grid(
        padded_n,
        padded_span_m,
        ox=origin_x - float(apron_px) * spacing_m,
        oz=origin_z - float(apron_px) * spacing_m,
    )
    result = mountain.generate(
        wx,
        wz,
        seed=SEED,
        style=mountain.STYLES[0],
        feature_span_m=FEATURE_SPAN_M,
        apron_px=apron_px,
        spacing_m=spacing_m,
        flow_on=True,
    )
    raw = np.asarray(result["height"], dtype=np.float64)  # CORE (apron already cropped)
    n = raw.shape[0]
    assert n == SAMPLE_N, f"expected core {SAMPLE_N}, got {n} (apron crop mismatch)"
    assert raw.shape == (SAMPLE_N, SAMPLE_N), f"raw not square SAMPLE_N: {raw.shape}"

    # ------------------------------------------------------------------
    # 2) Carve a connected pass network on the RAW core (BEFORE conditioning).
    #    Replicate the _core / _core_slice shim from export_bake_region_fixture.py.
    # ------------------------------------------------------------------
    pp = mpn.PassNetworkParams()  # defaults: n_we=4, n_ns=4, coarse_n=193
    saved_core, saved_slice = cr._core, win._core_slice
    cr._core = lambda full, spec: np.asarray(full)
    win._core_slice = lambda spec: slice(0, n)
    try:
        carved = mpn.carve_pass_network(raw, span_m=SOURCE_SPAN_M, height_scale_m=HEIGHT_SCALE_M, pp=pp)
    finally:
        cr._core, win._core_slice = saved_core, saved_slice

    delta = np.asarray(carved["delta"], dtype=np.float64)
    return raw + delta


def main() -> None:
    print("[seam-fixture] baking region A ...")
    carved_a = _bake_region(SOURCE_ORIGIN_X, SOURCE_ORIGIN_Z)

    print("[seam-fixture] baking region B ...")
    carved_b = _bake_region(SOURCE_ORIGIN_X + SOURCE_SPAN_M, SOURCE_ORIGIN_Z)

    out = {
        "n": int(SAMPLE_N),
        "height_scale_m": float(HEIGHT_SCALE_M),
        "span_m": float(SOURCE_SPAN_M),
        "carved_a": carved_a.ravel().tolist(),
        "carved_b": carved_b.ravel().tolist(),
    }
    OUT_FIXTURE.write_text(json.dumps(out))

    a_min, a_max = float(np.min(carved_a)), float(np.max(carved_a))
    b_min, b_max = float(np.min(carved_b)), float(np.max(carved_b))
    print(
        f"[seam-fixture] wrote {OUT_FIXTURE} n={SAMPLE_N} "
        f"carved_a_range=[{a_min:.6f},{a_max:.6f}] "
        f"carved_b_range=[{b_min:.6f},{b_max:.6f}]"
    )


if __name__ == "__main__":
    main()
