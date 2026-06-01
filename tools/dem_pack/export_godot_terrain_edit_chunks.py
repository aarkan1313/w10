r"""Export the mountain 9x9 chunk world WITH the terrain-edit framework's mountain_trail carved in, in the
SAME chunk format the mountain chunk scene reads (so it runs with real chunk streaming / walk mode / collision).

Mirrors export_godot_mountain_network_chunks.py exactly, but replaces the pass-network carve with the
terrain_edits framework's mountain_trail config. Carving the ONE big field then slicing -> the trail is
seam-exact across chunks by construction. Leaves all existing exporters untouched.

Run:
    python tools/dem_pack/export_godot_terrain_edit_chunks.py
Writes:
    wg-10/worldgen_terrain/generated/review/terrain_edit_chunks.json
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np

import mountain_synthesis as mountain
import terrain_edits as te
import terrain_edits.configs as cfg
import terrain_edits.apply as ap
from export_godot_mountain_world_chunks import (
    _condition, _corridor_mask, _stitch_grid,
    CHUNK_COUNT, CHUNK_N, SOURCE_CHUNK_SPAN_M, DISPLAY_CHUNK_SPAN_M, FEATURE_SPAN_M,
    WORLD_ORIGIN_X_M, WORLD_ORIGIN_Z_M, SEED, HEIGHT_SCALE_M, GENERATOR_VERSION, REVIEW_VARIANTS,
)


OUT = Path("wg-10/worldgen_terrain/generated/review/terrain_edit_chunks.json")


def build_terrain_edit_world(style, *, seed=SEED, chunk_count=CHUNK_COUNT, chunk_n=CHUNK_N,
                              source_chunk_span_m=SOURCE_CHUNK_SPAN_M, display_chunk_span_m=DISPLAY_CHUNK_SPAN_M,
                              origin_x_m=WORLD_ORIGIN_X_M, origin_z_m=WORLD_ORIGIN_Z_M) -> dict:
    step = int(chunk_n) - 1
    world_n = int(chunk_count) * step + 1
    source_world_span_m = float(chunk_count) * float(source_chunk_span_m)
    source_cell_m = float(source_chunk_span_m) / float(step)
    padded_n = world_n + 2
    padded_span_m = source_world_span_m + 2.0 * source_cell_m
    wx, wz = mountain.grid(padded_n, padded_span_m, ox=float(origin_x_m) - source_cell_m, oz=float(origin_z_m) - source_cell_m)
    result = mountain.generate(wx, wz, seed=int(seed), style=style, feature_span_m=FEATURE_SPAN_M)
    raw = np.asarray(result["height"], dtype=np.float64)

    # carve the terrain-edit trail into the RAW field (display span = per-chunk display span x chunk_count),
    # then condition+slice exactly like the base exporter -> seams exact by carving-then-slicing.
    display_total_m = float(display_chunk_span_m) * float(chunk_count)
    ctx = ap.EditContext(
        span_m=display_total_m,
        cell_m=display_total_m / (raw.shape[0] - 1),
        height_scale_m=HEIGHT_SCALE_M,
    )
    # mountain_trail_connected = the full-traversal config: 4 arms meet at a central waypoint -> one connected
    # network spanning all four edges (full L<->R + U<->D). Swap to cfg.mountain_trail() for the sparse single
    # pass, or cfg.mountain_trail(route_count=N) for a denser spread -- all tunable, same exporter.
    delta = te.apply_edits(raw, ctx, [cfg.mountain_trail_connected()])
    raw_carved = raw + delta

    # track carved fraction (cells where delta < -1m, normalised to height_scale_m)
    carved_frac = float(np.mean(delta < -1.0 / float(HEIGHT_SCALE_M)))

    height, stats = _condition(raw_carved)
    corridor = _corridor_mask(result)            # keep the existing valley-overlay mask
    chunks = []
    for z in range(int(chunk_count)):
        for x in range(int(chunk_count)):
            start_x = 1 + x * step
            start_z = 1 + z * step
            core = height[start_z : start_z + chunk_n, start_x : start_x + chunk_n]
            apron = height[start_z - 1 : start_z + chunk_n + 1, start_x - 1 : start_x + chunk_n + 1]
            core_corridor = corridor[start_z : start_z + chunk_n, start_x : start_x + chunk_n]
            apron_corridor = corridor[start_z - 1 : start_z + chunk_n + 1, start_x - 1 : start_x + chunk_n + 1]
            display_origin_x = (float(x) - float(chunk_count) * 0.5) * float(display_chunk_span_m)
            display_origin_z = (float(z) - float(chunk_count) * 0.5) * float(display_chunk_span_m)
            chunks.append({
                "source": "full_field_slice_with_terrain_edit_trail",
                "chunk_x": int(x), "chunk_z": int(z),
                "key": f"{style.key}_{x}_{z}", "label": f"{style.label} chunk {x},{z}",
                "n": int(chunk_n), "apron_n": int(chunk_n) + 2,
                "span_m": float(display_chunk_span_m), "source_span_m": float(source_chunk_span_m),
                "world_origin_x_m": float(origin_x_m) + float(x) * float(source_chunk_span_m),
                "world_origin_z_m": float(origin_z_m) + float(z) * float(source_chunk_span_m),
                "display_origin_x_m": display_origin_x, "display_origin_z_m": display_origin_z,
                "height": np.round(core, 4).astype(float).ravel().tolist(),
                "apron_height": np.round(apron, 4).astype(float).ravel().tolist(),
                "corridor": core_corridor.astype(int).ravel().tolist(),
                "apron_corridor": apron_corridor.astype(int).ravel().tolist(),
            })
    stitched = _stitch_grid(chunks, int(chunk_count), int(chunk_n), "height")
    return {
        "seed": int(seed), "label": style.label, "style_key": style.key,
        "corridor_height": float(np.percentile(stitched, 55.0)),
        "world_n": int(stitched.shape[0]), "stats": stats, "chunks": chunks,
        "terrain_edit": {"carved_frac": round(carved_frac, 4)},
    }


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    worlds = [build_terrain_edit_world(style) for style in mountain.STYLES]
    # SAME top-level shape as export_godot_mountain_world_chunks.build_payload (scene reads "seeds").
    payload = {
        "title": "WorldGen10 mountain 9x9 + terrain-edit trail",
        "generator_version": GENERATOR_VERSION + "_terrain_edit_trail",
        "source_scope": "coherent_full_field_carved_with_terrain_edit_trail_sliced_for_review",
        "chunk_count": int(CHUNK_COUNT),
        "chunk_n": int(CHUNK_N),
        "chunk_span_m": float(DISPLAY_CHUNK_SPAN_M),
        "source_chunk_span_m": float(SOURCE_CHUNK_SPAN_M),
        "feature_span_m": float(FEATURE_SPAN_M),
        "world_span_m": float(CHUNK_COUNT) * float(DISPLAY_CHUNK_SPAN_M),
        "source_world_span_m": float(CHUNK_COUNT) * float(SOURCE_CHUNK_SPAN_M),
        "source_scene_ratio": float(SOURCE_CHUNK_SPAN_M) / float(DISPLAY_CHUNK_SPAN_M),
        "world_origin_x_m": float(WORLD_ORIGIN_X_M),
        "world_origin_z_m": float(WORLD_ORIGIN_Z_M),
        "height_scale_m": float(HEIGHT_SCALE_M),
        "review_variants": list(REVIEW_VARIANTS),
        "seeds": worlds,
    }
    OUT.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    print(f"wrote {OUT}")
    for wd in worlds:
        te_info = wd["terrain_edit"]
        print(f"  {wd['style_key']}: carved_frac={te_info['carved_frac']}")


if __name__ == "__main__":
    main()
