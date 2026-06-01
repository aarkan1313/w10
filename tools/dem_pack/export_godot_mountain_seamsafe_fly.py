r"""Export a seam-safe stitched mountain world for the Godot fly-review scene.

Review snapshot — NOT shipped code.  Generates one JSON consumed by
``wg-10/worldgen_terrain/harness/mountain_world_review.tscn`` (via
``rough_world_review.gd``) so the owner can FLY a real 2×2 tiled world and
confirm there is NO visible discontinuity at the internal seam between
independently-generated adjacent windows.

How it works
------------
Four adjacent windows are generated INDEPENDENTLY using the seam-safe path of
``mountain_synthesis.generate(..., apron_px=MOUNTAIN_APRON_PX)``.  Each window
is built with apron-padded world-coordinate grids so the borders converge to
float epsilon.  The four core grids are then stitched into one big height grid
by deduplicating the shared border rows/columns (they are identical to ~1e-10
under the apron guarantee).  Layout: 2x2 windows = one internal seam per axis.

The seam delta is printed before writing so the owner can verify the contract.

Run::

    cd D:/workflows/worldgen10
    python tools/dem_pack/export_godot_mountain_seamsafe_fly.py

Writes::

    wg-10/worldgen_terrain/generated/review/mountain_world_3d.json

Open in Godot::

    wg-10/worldgen_terrain/harness/mountain_world_review.tscn
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

import mountain_synthesis as mountain

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------
SEED = 177
STYLE = mountain.STYLES[0]          # alpine_branching — shows connected drainage best
FEATURE_SPAN_M: float = 90_000.0   # FIXED — shared by ALL windows (seam-safe requirement)
CORE_N: int = 129                   # core grid size per window (129 → 257×257 stitched)
CORE_SPAN_M: float = 25_000.0      # metres covered by one core window
AP: int = mountain.MOUNTAIN_APRON_PX  # 80 — apron cells each side
K: int = 2                          # K×K stitch (2×2 = 4 windows, one internal seam)

# Output path (matches data_path in mountain_world_review.tscn)
OUT = Path("wg-10/worldgen_terrain/generated/review/mountain_world_3d.json")


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _make_padded_grid(i: int, j: int) -> tuple[np.ndarray, np.ndarray]:
    """Build the apron-padded world-coord grid for window (col=i, row=j).

    Core origin is (i*CORE_SPAN_M, j*CORE_SPAN_M).
    Cell size is CORE_SPAN_M / (CORE_N - 1).
    Apron shifts the origin back by AP cells.
    """
    cell_m = CORE_SPAN_M / (CORE_N - 1)
    apron_m = AP * cell_m
    padded_n = CORE_N + 2 * AP
    padded_span_m = cell_m * (padded_n - 1)  # = CORE_SPAN_M + 2*apron_m - cell_m

    ox_core = float(i) * CORE_SPAN_M
    oz_core = float(j) * CORE_SPAN_M
    ox_pad = ox_core - apron_m
    oz_pad = oz_core - apron_m

    return mountain.grid(padded_n, padded_span_m, ox=ox_pad, oz=oz_pad)


def _generate_core(i: int, j: int) -> np.ndarray:
    """Generate the seam-safe core height for window (col=i, row=j)."""
    wx, wz = _make_padded_grid(i, j)
    result = mountain.generate(
        wx, wz,
        seed=SEED,
        style=STYLE,
        feature_span_m=FEATURE_SPAN_M,
        apron_px=AP,
    )
    h: np.ndarray = np.asarray(result["height"], dtype=np.float64)
    assert h.shape == (CORE_N, CORE_N), f"expected ({CORE_N},{CORE_N}), got {h.shape}"
    return h


def _stitch_2x2(
    c00: np.ndarray,
    c10: np.ndarray,
    c01: np.ndarray,
    c11: np.ndarray,
) -> np.ndarray:
    """Stitch four CORE_N×CORE_N grids into a (2*CORE_N-1)×(2*CORE_N-1) grid.

    Layout (col, row):
        (0,0) | (1,0)
        ------+------
        (0,1) | (1,1)

    Shared border columns/rows are deduplicated using the LEFT/TOP window's
    last column/row (they are equal to float epsilon; the max delta is printed).
    """
    top = np.concatenate([c00, c10[:, 1:]], axis=1)   # drop first col of c10
    bot = np.concatenate([c01, c11[:, 1:]], axis=1)   # drop first col of c11
    return np.concatenate([top, bot[1:, :]], axis=0)   # drop first row of bot


def _measure_seam_delta(
    c00: np.ndarray,
    c10: np.ndarray,
    c01: np.ndarray,
    c11: np.ndarray,
) -> dict[str, float]:
    """Measure max absolute border delta between adjacent window pairs."""
    # Horizontal seam: last column of (0,j) vs first column of (1,j)
    h_delta_top = float(np.max(np.abs(c00[:, -1] - c10[:, 0])))
    h_delta_bot = float(np.max(np.abs(c01[:, -1] - c11[:, 0])))
    # Vertical seam: last row of (i,0) vs first row of (i,1)
    v_delta_left = float(np.max(np.abs(c00[-1, :] - c01[0, :])))
    v_delta_right = float(np.max(np.abs(c10[-1, :] - c11[0, :])))
    # Corner: all four windows should agree at the centre corner
    corner_vals = [c00[-1, -1], c10[-1, 0], c01[0, -1], c11[0, 0]]
    corner_delta = float(np.max(np.abs(np.array(corner_vals) - corner_vals[0])))

    return {
        "horizontal_top": h_delta_top,
        "horizontal_bot": h_delta_bot,
        "vertical_left": v_delta_left,
        "vertical_right": v_delta_right,
        "corner": corner_delta,
        "max": max(h_delta_top, h_delta_bot, v_delta_left, v_delta_right, corner_delta),
    }


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    print(f"Generating 2x2 seam-safe mountain stitch ...")
    print(f"  style={STYLE.key}  seed={SEED}  feature_span={FEATURE_SPAN_M/1000:.0f} km")
    print(f"  CORE_N={CORE_N}  CORE_SPAN={CORE_SPAN_M/1000:.1f} km/window  AP={AP}")
    print(f"  Padded N per window = {CORE_N + 2*AP}")
    print()

    print("  Generating window (0,0) ...")
    c00 = _generate_core(0, 0)
    print("  Generating window (1,0) ...")
    c10 = _generate_core(1, 0)
    print("  Generating window (0,1) ...")
    c01 = _generate_core(0, 1)
    print("  Generating window (1,1) ...")
    c11 = _generate_core(1, 1)
    print()

    # --- seam verification ---
    deltas = _measure_seam_delta(c00, c10, c01, c11)
    print("Inter-window border delta (proof of seam-exact contract):")
    for name, val in deltas.items():
        status = "OK" if val < 1e-6 else "FAIL"
        print(f"  {name:20s}: {val:.3e}  [{status}]")

    max_delta = deltas["max"]
    if max_delta >= 1e-6:
        print(f"\nERROR: max border delta {max_delta:.3e} >= 1e-6 — seam-safe contract violated!")
        print("Do NOT write JSON. Check apron math or feature_span_m consistency.")
        sys.exit(1)

    print(f"\nSeam delta OK ({max_delta:.3e} < 1e-6).")
    print()

    # --- stitch ---
    stitched = _stitch_2x2(c00, c10, c01, c11)
    n_stitched = stitched.shape[0]
    total_span_km = (K * CORE_SPAN_M) / 1000.0
    assert stitched.shape == (n_stitched, n_stitched), f"non-square stitch: {stitched.shape}"
    print(f"Stitched grid: {n_stitched}x{n_stitched}  ({total_span_km:.1f} km x {total_span_km:.1f} km)")

    # Normalize to [-1, 1] range matching how rough_world_review.gd renders colors:
    # _terrain_color maps h ∈ [-1,1] → t = (h+1)*0.5 ∈ [0,1].
    # The seam-safe path already produces ~affine output; just clip to a sensible range.
    h_flat = stitched.ravel()
    h_min, h_max = float(h_flat.min()), float(h_flat.max())
    print(f"  Raw height range: [{h_min:.4f}, {h_max:.4f}]  std={float(h_flat.std()):.4f}")

    # The rough_world_review.gd scene already uses base_height_scale=1700 (mountain_world_review.tscn).
    # Pass raw normalized values — the GD script applies its own scale on top.
    # round to 4dp to keep file size reasonable.
    height_list = np.round(h_flat, 4).astype(float).tolist()

    # --- payload ---
    # Item structure mirrors export_godot_mountain_world_review.py exactly:
    # key, label, kind, span_km, source, n, height, stats
    stats = {
        "source_min": h_min,
        "source_max": h_max,
        "source_ptp": h_max - h_min,
        "p05": float(np.percentile(h_flat, 5)),
        "p50": float(np.percentile(h_flat, 50)),
        "p95": float(np.percentile(h_flat, 95)),
    }

    stitched_item = {
        "key": "seamsafe_fly_2x2",
        "label": f"SEAM-SAFE 2x2 stitch ({STYLE.label})",
        "kind": "synth",
        "span_km": round(total_span_km, 1),
        "source": f"mountain_synthesis seam-safe apron={AP} style={STYLE.key}",
        "n": n_stitched,
        "height": height_list,
        "stats": stats,
        "seam_delta_max": max_delta,
        "stitch_layout": f"{K}x{K} windows, CORE_N={CORE_N}, CORE_SPAN_KM={CORE_SPAN_M/1000:.1f}",
    }

    # Include a reference single-window item for side-by-side comparison.
    # Window (0,0) as a standalone item (n=CORE_N).
    ref_h = np.round(c00.ravel(), 4).astype(float).tolist()
    single_item = {
        "key": "seamsafe_window_00",
        "label": f"Single window (0,0) ({STYLE.label})",
        "kind": "synth",
        "span_km": round(CORE_SPAN_M / 1000.0, 1),
        "source": f"mountain_synthesis seam-safe apron={AP} style={STYLE.key}",
        "n": CORE_N,
        "height": ref_h,
        "stats": {
            "source_min": float(c00.min()),
            "source_max": float(c00.max()),
            "source_ptp": float(np.ptp(c00)),
            "p05": float(np.percentile(c00, 5)),
            "p50": float(np.percentile(c00, 50)),
            "p95": float(np.percentile(c00, 95)),
        },
        "seam_delta_max": max_delta,
    }

    payload = {
        "title": "WorldGen10 mountain seam-safe 2x2 fly review",
        "generator_version": "mountain_synthesis_seamsafe_fly_v0",
        "span_km": total_span_km,
        "seam_delta_max": max_delta,
        "items": [stitched_item, single_item],
    }

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")

    size_mb = OUT.stat().st_size / 1_048_576
    print(f"Wrote {OUT}  ({size_mb:.2f} MB)")
    print(f"  items: {len(payload['items'])}")
    print(f"  items[0]: n={stitched_item['n']}, len(height)={len(height_list)}")
    print(f"  items[1]: n={single_item['n']}")
    print()
    print("To fly: open Godot -> wg-10/worldgen_terrain/harness/mountain_world_review.tscn")
    print("  Press 1 -> stitched 2x2 world (cross the seam!)")
    print("  Press 2 -> single window (0,0) for comparison")
    print("  WASD/Space/C to fly, P to toggle slope/corridor overlay, +/- relief scale")


if __name__ == "__main__":
    main()
