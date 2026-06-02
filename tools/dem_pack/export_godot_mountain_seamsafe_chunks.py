r"""Export seam-safe mountain windows as SEPARATE TRUE-SCALE CHUNKS.

Each of the K*K windows is placed at its REAL world position so the owner
can fly a genuinely large-scale mountain world -- chunk i+1 begins exactly
where chunk i ends.  No squishing into a fixed display box.

Output is written to the path that mountain_world_chunks_review.tscn already
loads, overwriting the legacy compose-big-then-slice artifact.

Run from REPO ROOT::

    python tools/dem_pack/export_godot_mountain_seamsafe_chunks.py

Writes::

    wg-10/worldgen_terrain/generated/review/mountain_world_chunks_3x3.json

Open in Godot::

    wg-10/worldgen_terrain/harness/mountain_world_chunks_review.tscn
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

# Windows consoles default to cp1252 -- force UTF-8 so no non-ASCII char can abort a bake.
try:
    sys.stdout.reconfigure(encoding="utf-8")
except (AttributeError, ValueError):
    pass

import numpy as np

import mountain_synthesis as mountain

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------
SEED: int = 177
STYLE = mountain.STYLES[0]          # alpine_branching
FEATURE_SPAN_M: float = 90_000.0   # FIXED -- shared by ALL windows (seam-safe requirement)
CORE_N: int = 129                   # core grid points per window
CORE_SPAN_M: float = 25_000.0      # metres of real ground per chunk
K: int = 7                          # K*K chunks; 7x7 = 49 windows, 175 km world

# Apron padding (cells each side).
# AP=160 drives every internal seam to < 1e-3 normalized (visually seamless bar).
# The seam-safe fly exporter confirmed this empirically across a full 5x5 world.
AP: int = 160

HEIGHT_SCALE_M: float = 1700.0     # matches base_height_scale in the review scene

# Output path -- MUST match data_path in mountain_world_chunks_review.tscn.
OUT = Path("wg-10/worldgen_terrain/generated/review/mountain_world_chunks_3x3.json")

GENERATOR_VERSION = "mountain_seamsafe_chunks_v1_true_scale"

# Seam gate: normalized delta < 1e-3 is < ~1.7 m at game scale -- invisible.
SEAM_BAR: float = 1e-3


# ---------------------------------------------------------------------------
# Core generation
# ---------------------------------------------------------------------------

def _make_padded_grid(i: int, j: int) -> tuple[np.ndarray, np.ndarray]:
    """Build the AP-padded world-coord grid for chunk (col=i, row=j).

    Core origin in world coords: (i*CORE_SPAN_M, j*CORE_SPAN_M).
    The grid is extended by AP cells on each side (padded_n = CORE_N + 2*AP).

    Both the core call (apron_px=AP -> crops to CORE_N x CORE_N)
    and the apron call  (apron_px=AP-1 -> crops to (CORE_N+2) x (CORE_N+2))
    use this SAME padded grid.  The extra one-cell border crops differently per
    call -- but the actual height computations are bit-identical at those cells.
    """
    cell_m = CORE_SPAN_M / (CORE_N - 1)
    padded_n = CORE_N + 2 * AP
    ox_core = float(i) * CORE_SPAN_M
    oz_core = float(j) * CORE_SPAN_M
    ox_pad = ox_core - AP * cell_m
    oz_pad = oz_core - AP * cell_m
    padded_span_m = cell_m * (padded_n - 1)
    return mountain.grid(padded_n, padded_span_m, ox=ox_pad, oz=oz_pad)


def _generate_chunk(i: int, j: int) -> dict[str, object]:
    """Generate one seam-safe chunk and return the full chunk payload dict.

    Apron strategy
    --------------
    The GD scene (mountain_world_chunks_review.gd _make_mesh) needs:
      - height:       (CORE_N, CORE_N)   -- the core grid
      - apron_height: (CORE_N+2, CORE_N+2) -- core + ONE extra ring each side
        used by _apron_height_at(apron, apron_n, x, z):
          ax = clampi(x + 1, 0, apron_n - 1)  ->  apron[az*apron_n + ax]

    mountain.generate(wx, wz, apron_px=a) crops the output to [a:-a, a:-a]:
      apron_px=AP   -> crops [AP:-AP]        -> shape (CORE_N,   CORE_N)    (core)
      apron_px=AP-1 -> crops [(AP-1):-(AP-1)] -> shape (CORE_N+2, CORE_N+2) (apron ring)

    Both calls operate on the same AP-padded grid and both use the seam-safe
    path (apron_px > 0 activates affine_remap fixed-constant normalization, never
    per-window zscore).  The height values at the overlapping core cells are
    bit-identical (verified: max delta = 0.000e+00 during development).
    """
    wx_pad, wz_pad = _make_padded_grid(i, j)

    # Core: (CORE_N, CORE_N)
    result_core = mountain.generate(
        wx_pad, wz_pad,
        seed=SEED,
        style=STYLE,
        feature_span_m=FEATURE_SPAN_M,
        apron_px=AP,
    )
    core: np.ndarray = np.asarray(result_core["height"], dtype=np.float64)
    assert core.shape == (CORE_N, CORE_N), (
        f"chunk ({i},{j}): expected core ({CORE_N},{CORE_N}), got {core.shape}"
    )

    # Apron ring: (CORE_N+2, CORE_N+2)
    apron_n = CORE_N + 2
    result_apron = mountain.generate(
        wx_pad, wz_pad,
        seed=SEED,
        style=STYLE,
        feature_span_m=FEATURE_SPAN_M,
        apron_px=AP - 1,
    )
    apron_ring: np.ndarray = np.asarray(result_apron["height"], dtype=np.float64)
    assert apron_ring.shape == (apron_n, apron_n), (
        f"chunk ({i},{j}): expected apron ({apron_n},{apron_n}), got {apron_ring.shape}"
    )

    # Sanity: core must appear bit-exactly in middle of apron ring (both affine_remap).
    delta = float(np.max(np.abs(core - apron_ring[1:-1, 1:-1])))
    assert delta < 1e-12, (
        f"chunk ({i},{j}): core/apron consistency check FAIL: delta={delta:.3e} "
        f"(expect 0.0 -- both calls use identical seam-safe affine_remap constants)"
    )

    # corridor: zeros (GD uses chunk.get("corridor", []))
    corridor = [0] * (CORE_N * CORE_N)

    # display origin: true-scale world position (edge-to-edge tiling)
    display_origin_x = float(i) * CORE_SPAN_M
    display_origin_z = float(j) * CORE_SPAN_M

    return {
        "chunk_x": int(i),
        "chunk_z": int(j),
        "n": int(CORE_N),
        "apron_n": int(apron_n),
        "span_m": float(CORE_SPAN_M),
        "display_origin_x_m": display_origin_x,
        "display_origin_z_m": display_origin_z,
        "height": np.round(core, 4).astype(float).ravel().tolist(),
        "apron_height": np.round(apron_ring, 4).astype(float).ravel().tolist(),
        "corridor": corridor,
    }


# ---------------------------------------------------------------------------
# Seam measurement
# ---------------------------------------------------------------------------

def _measure_seams(chunks: list[dict[str, object]]) -> float:
    """Measure max normalized border delta across all adjacent chunk pairs.

    Checks every adjacent pair:
      - right (x+1): right column of (i,j) vs left column of (i+1,j)
      - bottom (z+1): bottom row of (i,j) vs top row of (i,j+1)

    Returns the worst (highest) normalized delta.
    """
    index: dict[tuple[int, int], np.ndarray] = {}
    for c in chunks:
        h = np.asarray(c["height"], dtype=np.float64).reshape(CORE_N, CORE_N)
        index[(int(c["chunk_x"]), int(c["chunk_z"]))] = h

    worst = 0.0
    worst_pair = ("?", "?")
    worst_axis = "?"

    for j in range(K):
        for i in range(K):
            h_arr = index[(i, j)]

            if i + 1 < K:
                d = float(np.max(np.abs(h_arr[:, -1] - index[(i + 1, j)][:, 0])))
                if d > worst:
                    worst = d
                    worst_pair = (f"({i},{j})", f"({i+1},{j})")
                    worst_axis = "x"

            if j + 1 < K:
                d = float(np.max(np.abs(h_arr[-1, :] - index[(i, j + 1)][0, :])))
                if d > worst:
                    worst = d
                    worst_pair = (f"({i},{j})", f"({i},{j+1})")
                    worst_axis = "z"

    print(f"Worst seam: axis={worst_axis}  pair={worst_pair[0]} -> {worst_pair[1]}")
    return worst


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    n_windows = K * K
    total_span_km = K * CORE_SPAN_M / 1000.0
    print(f"Generating {K}x{K}={n_windows} seam-safe mountain chunks ...")
    print(f"  style={STYLE.key}  seed={SEED}  feature_span={FEATURE_SPAN_M/1000:.0f} km")
    print(f"  CORE_N={CORE_N}  CORE_SPAN={CORE_SPAN_M/1000:.1f} km/chunk  AP={AP}")
    print(f"  True-scale world: {total_span_km:.0f} km x {total_span_km:.0f} km")
    print(f"  Padded N per window = {CORE_N + 2*AP}  (2 generate calls per chunk: apron_px=AP and AP-1)")
    print()

    t0 = time.time()
    chunks: list[dict[str, object]] = []
    for j in range(K):
        for i in range(K):
            done = j * K + i
            print(f"  chunk ({i},{j}) ... [{done + 1}/{n_windows}]", flush=True)
            chunks.append(_generate_chunk(i, j))
    gen_secs = time.time() - t0

    print(f"\nGenerated {n_windows} chunks in {gen_secs:.1f}s ({gen_secs/n_windows:.1f}s/chunk).")
    print()

    # --- seam verification ---
    print("Measuring seam deltas across all adjacent chunk pairs ...")
    worst_delta = _measure_seams(chunks)
    worst_m = worst_delta * HEIGHT_SCALE_M

    print(f"  Worst normalized delta: {worst_delta:.3e}")
    print(f"  At base_height_scale={HEIGHT_SCALE_M:.0f} m that = {worst_m:.4f} m")

    if worst_delta >= SEAM_BAR:
        print(
            f"\nERROR: worst seam delta {worst_delta:.3e} >= {SEAM_BAR:.0e} bar "
            f"({worst_m:.2f} m at game scale -- VISIBLE seam)."
        )
        print("Do NOT write JSON. Check AP / feature_span_m / origin math.")
        sys.exit(1)

    print(
        f"\nSeam gate PASS: worst delta {worst_delta:.3e} < {SEAM_BAR:.0e} "
        f"({worst_m:.4f} m -- invisible at {HEIGHT_SCALE_M:.0f} m scale)."
    )
    print()

    # --- corridor_height: 55th percentile of all core heights ---
    all_heights = np.concatenate([
        np.asarray(c["height"], dtype=np.float64) for c in chunks
    ])
    corridor_height = float(np.percentile(all_heights, 55.0))

    # --- payload (schema mirrors export_godot_mountain_world_chunks.py) ---
    seed_entry: dict[str, object] = {
        "seed": SEED,
        "label": f"seamsafe {K}x{K} chunks ({STYLE.label})",
        "style_key": STYLE.key,
        "corridor_height": corridor_height,
        "chunks": chunks,
    }

    world_span_m = float(K) * CORE_SPAN_M
    payload: dict[str, object] = {
        "title": f"WorldGen10 mountain seam-safe {K}x{K} true-scale chunks",
        "generator_version": GENERATOR_VERSION,
        "chunk_count": int(K),
        "chunk_n": int(CORE_N),
        "chunk_span_m": float(CORE_SPAN_M),
        "source_chunk_span_m": float(CORE_SPAN_M),
        "feature_span_m": float(FEATURE_SPAN_M),
        "world_span_m": world_span_m,
        "source_world_span_m": world_span_m,
        "source_scene_ratio": 1.0,
        "height_scale_m": HEIGHT_SCALE_M,
        "seeds": [seed_entry],
    }

    # --- write ---
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    size_mb = OUT.stat().st_size / 1_048_576
    print(f"Wrote {OUT}  ({size_mb:.2f} MB)")
    print(f"  seeds[0].chunks length: {len(chunks)}  (expected {n_windows})")
    print(f"  chunk schema keys emitted per chunk:")
    print(f"    chunk_x, chunk_z, n, apron_n, span_m,")
    print(f"    display_origin_x_m, display_origin_z_m,")
    print(f"    height (n*n floats), apron_height (apron_n*apron_n floats), corridor (n*n zeros)")
    print()
    print(f"Generation time: {gen_secs:.1f}s  ({gen_secs/60:.1f} min)")
    print(f"World span: {total_span_km:.0f} km x {total_span_km:.0f} km  ({K} x {CORE_SPAN_M/1000:.0f} km chunks)")
    print()
    print("To fly: open Godot -> wg-10/worldgen_terrain/harness/mountain_world_chunks_review.tscn")
    print("  T = cycle worlds, B = seam guides, N = jump to seam")
    print("  WASD/Space/C fly, +/- relief, P overlay, F focus, G overview")


if __name__ == "__main__":
    main()
