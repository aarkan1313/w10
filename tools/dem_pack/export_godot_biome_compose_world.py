r"""Slice D: whole-world MULTI-BIOME compose for the owner fly (Fork-B acceptance gate).

Lays the 11 seam-safe biomes across one world via an ORGANIC noise-driven weight field (a
stand-in for the eventual Rust grammar), composes them with biome_compose.compose_biomes
(height_favored blend), and emits TRUE-SCALE chunks in the schema the ACCEPTED chunk scene
(mountain_world_chunks_review.tscn) consumes -- so the owner flies a real contiguous multi-biome
world at true scale and judges (a) do biomes read as themselves, (b) do transitions read
believably, (c) are biome-internal AND biome-boundary seams invisible.

SEAM-SAFETY = compose-big-field-then-slice (the proven pattern; the accepted mountain 9x9 used it):
generate each biome ONCE over the whole padded span, build the weight field over the same span,
compose ONCE, then slice into chunks. Seam-exact by construction -- no per-chunk blur-edge issue,
no synth changes. (Independent-window streaming seam-safety is the Rust/Slice-3 concern; each biome's
apron_px path already proves it per-biome.)

Run:    python tools/dem_pack/export_godot_biome_compose_world.py
Writes: wg-10/worldgen_terrain/generated/review/mountain_world_chunks_3x3.json  (the chunk scene's path)
"""
from __future__ import annotations

import sys
try:
    sys.stdout.reconfigure(encoding="utf-8")
except (AttributeError, ValueError):
    pass

import json
from pathlib import Path

import numpy as np
from scipy.ndimage import gaussian_filter

import geography_engine as geo
import worldgen_proto as wg
import biome_registry as br
import biome_compose as bc

OUT = Path("wg-10/worldgen_terrain/generated/review/mountain_world_chunks_3x3.json")

# --- config ---
SEED = 219
BIOMES = ["mountain", "volcanic", "glacial", "grassland", "desert", "wetland"]
# PER-BIOME RELIEF (validated on stills, render_biome_compose_fast.py "dramatic" variant): each
# biome normalized to std-1 then scaled so mountains TOWER and lowlands stay flat -> biomes keep
# their individuality instead of being averaged to sameness. The fix for the first compose's
# "mountains not tall / biomes lose individuality".
BIOME_RELIEF = {
    "mountain": 1.00, "volcanic": 0.62, "glacial": 0.40,
    "grassland": 0.09, "desert": 0.15, "wetland": 0.04,
}
FEATURE_SPAN_M = 90_000.0          # FIXED for all biomes (seam-safe requirement)
K = 9                              # 9x9 chunk grid (bigger world so regions breathe)
CHUNK_N = 129                      # core cells per chunk
CHUNK_SPAN_M = 25_000.0            # true ground span per chunk (9*25 = 225 km)
APRON_PX = 160                     # the biomes' calibrated seam-safe apron (max over biomes)
WEIGHT_FREQ = 1.0 / 70_000.0       # ~70 km regions -> biomes breathe (validated on stills)
SOFTMAX_TEMP = 0.16
BLEND = bc.BlendConfig(mode="height_favored")
BASE_HEIGHT_SCALE = 1700.0         # the chunk scene's vertical scale


def _organic_weights(wx: np.ndarray, wz: np.ndarray) -> list[np.ndarray]:
    """A smooth per-pixel partition-of-unity over BIOMES from low-freq world-coord noise.

    Each biome gets a noise 'affinity' field; softmax over affinities -> weights that sum to 1
    and vary smoothly (organic regions meeting at natural boundaries). Pure f(world pos) -> seam-safe.
    """
    affinities = []
    for i, name in enumerate(BIOMES):
        f = wg.fbm(wx, wz, WEIGHT_FREQ, 4, SEED + 300 + 17 * i, gain=0.55)
        affinities.append(f)
    stack = np.stack(affinities, axis=0) / SOFTMAX_TEMP
    stack = stack - stack.max(axis=0, keepdims=True)
    e = np.exp(stack)
    w = e / (np.sum(e, axis=0, keepdims=True) + 1e-9)
    return [w[i] for i in range(len(BIOMES))]


def _build_world():
    """Generate all biomes + weights over the whole padded span, compose once, return core world."""
    step = CHUNK_N - 1
    core_world_n = K * step + 1                       # stitched core grid size
    cell_m = CHUNK_SPAN_M / step
    padded_n = core_world_n + 2 * APRON_PX
    padded_span_m = cell_m * (padded_n - 1)
    ox = -APRON_PX * cell_m
    oz = -APRON_PX * cell_m
    wx, wz = geo.grid(padded_n, padded_span_m, ox=ox, oz=oz)

    print(f"  whole-world padded grid {padded_n}x{padded_n} ({padded_span_m/1000:.1f} km), generating {len(BIOMES)} biomes...")
    fields = []
    for name in BIOMES:
        print(f"    generate {name} (relief x{BIOME_RELIEF[name]}) ...")
        h = np.asarray(br.get_recipe(name).generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M), dtype=np.float64)
        h = (h - h.mean()) / (h.std() + 1e-9)        # normalize each biome to std 1...
        h = h * BIOME_RELIEF[name]                    # ...then apply per-biome relief (mountains tower)
        fields.append(h)

    print("  building organic weight field + composing...")
    weights = _organic_weights(wx, wz)
    composed = bc.compose_biomes(fields, weights, BLEND)              # full padded composed height
    # dominant biome per pixel (for optional per-biome coloring/diagnostics)
    dominant = np.argmax(np.stack(weights, axis=0), axis=0).astype(np.int32)

    # crop to core
    a = APRON_PX
    core = composed[a:a + core_world_n, a:a + core_world_n]
    core_dom = dominant[a:a + core_world_n, a:a + core_world_n]
    return core, core_dom, core_world_n


def _chunk_payload(world: np.ndarray, dom: np.ndarray, cx: int, cz: int) -> dict:
    step = CHUNK_N - 1
    x0, z0 = cx * step, cz * step
    core = world[z0:z0 + CHUNK_N, x0:x0 + CHUNK_N]
    core_dom = dom[z0:z0 + CHUNK_N, x0:x0 + CHUNK_N]
    # apron_n = n+2 (one ring) for edge normals, from the padded world (pad core by 1 with edge replicate)
    padded = np.pad(world, 1, mode="edge")
    apron = padded[z0:z0 + CHUNK_N + 2, x0:x0 + CHUNK_N + 2]
    return {
        "chunk_x": cx, "chunk_z": cz,
        "n": CHUNK_N, "apron_n": CHUNK_N + 2,
        "span_m": CHUNK_SPAN_M,
        "display_origin_x_m": cx * CHUNK_SPAN_M,
        "display_origin_z_m": cz * CHUNK_SPAN_M,
        "height": np.round(core, 4).ravel().tolist(),
        "apron_height": np.round(apron, 4).ravel().tolist(),
        "corridor": [0] * (CHUNK_N * CHUNK_N),
        "biome_index": core_dom.ravel().tolist(),
    }


def _measure_seams(world: np.ndarray) -> float:
    """Worst shared-edge delta between adjacent chunk cores (slice-from-one-field => should be ~0)."""
    step = CHUNK_N - 1
    worst = 0.0
    for cz in range(K):
        for cx in range(K):
            x0, z0 = cx * step, cz * step
            if cx < K - 1:
                right = world[z0:z0 + CHUNK_N, x0 + CHUNK_N - 1]
                left_next = world[z0:z0 + CHUNK_N, x0 + CHUNK_N - 1]  # shared column (slice-from-one-field)
                worst = max(worst, float(np.max(np.abs(right - left_next))))
    return worst


def main() -> None:
    print(f"Slice D: whole-world multi-biome compose ({K}x{K} chunks, {K*CHUNK_SPAN_M/1000:.0f} km, biomes={BIOMES})")
    world, dom, core_world_n = _build_world()
    print(f"  composed core world {core_world_n}x{core_world_n}; height range [{world.min():.3f}, {world.max():.3f}] std={world.std():.3f}")

    chunks = [_chunk_payload(world, dom, cx, cz) for cz in range(K) for cx in range(K)]
    payload = {
        "title": "WorldGen10 multi-biome compose (Fork-B) review",
        "generator_version": "biome_compose_world_v1_organic_7x7",
        "chunk_count": K,
        "chunk_n": CHUNK_N,
        "chunk_span_m": CHUNK_SPAN_M,
        "source_chunk_span_m": CHUNK_SPAN_M,
        "feature_span_m": FEATURE_SPAN_M,
        "world_span_m": K * CHUNK_SPAN_M,
        "source_world_span_m": K * CHUNK_SPAN_M,
        "biomes": BIOMES,
        "seeds": [{
            "seed": SEED,
            "label": "multi-biome compose: " + " ".join(BIOMES),
            "corridor_height": 0.0,
            "chunks": chunks,
        }],
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    size_mb = OUT.stat().st_size / 1_048_576
    print(f"  wrote {OUT}  ({size_mb:.2f} MB), {len(chunks)} chunks")
    print(f"  fly: mountain_world_chunks_review.tscn (true-scale {K*CHUNK_SPAN_M/1000:.0f} km, compose-then-slice = seam-exact by construction)")


if __name__ == "__main__":
    main()
