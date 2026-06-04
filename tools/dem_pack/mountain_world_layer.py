r"""Accepted mountain world-layer construction.

This module owns the source contract behind the accepted
``mountain_network_chunks_review.tscn`` artifact:

1. Build one coherent mountain source field.
2. Carve a sparse connected pass network into the raw field.
3. Apply one whole-field conditioning transform.
4. Slice the conditioned world into review/runtime chunks.

Exporters write payloads; tests and future runtime ports should depend on this
module instead of reaching into one exporter from another.
"""

from __future__ import annotations

from typing import Iterable

import numpy as np
from scipy.ndimage import gaussian_filter

import mountain_pass_network as mpn
import mountain_synthesis as mountain


GENERATOR_VERSION = "mountain_synthesis_v0_9x9_original_scene_scale_review"
NETWORK_GENERATOR_VERSION = GENERATOR_VERSION + "_pass_network"
CHUNK_COUNT = 9
CHUNK_N = 129
SOURCE_CHUNK_SPAN_M = 30_000.0
DISPLAY_CHUNK_SPAN_M = 25_600.0 / 3.0
FEATURE_SPAN_M = 90_000.0
WORLD_ORIGIN_X_M = 72_000.0
WORLD_ORIGIN_Z_M = 41_000.0
SEED = 177
HEIGHT_SCALE_M = 1700.0
MATERIAL_HINT_FIELDS = ("low_pass_hint", "floor_hint", "rock_hint", "snow_hint")
WORLD_LAYER_FIELDS = ("height", "corridor", *MATERIAL_HINT_FIELDS)

REVIEW_VARIANTS = (
    {"id": "mountain_dressed", "label": "mountain dressed", "relief": 1.0, "dressing": "review_biome"},
    {"id": "mountain_plain", "label": "plain terrain", "relief": 1.0, "dressing": "plain"},
    {"id": "mountain_tall", "label": "tall mountain dressed", "relief": 1.15, "dressing": "review_biome"},
)


def condition_world(z: np.ndarray) -> tuple[np.ndarray, dict[str, float]]:
    z = np.asarray(z, dtype=np.float64)
    p05 = float(np.percentile(z, 5.0))
    p50 = float(np.percentile(z, 50.0))
    p95 = float(np.percentile(z, 95.0))
    robust = (z - p50) / (p95 - p05 + 1.0e-9) * 2.10
    shaped = np.tanh(gaussian_filter(robust, sigma=0.55))
    return shaped, {
        "source_min": float(np.min(z)),
        "source_max": float(np.max(z)),
        "source_ptp": float(np.ptp(z)),
        "p05": p05,
        "p50": p50,
        "p95": p95,
        "conditioned_min": float(np.min(shaped)),
        "conditioned_max": float(np.max(shaped)),
        "conditioned_ptp": float(np.ptp(shaped)),
    }


def _smoothstep(edge0: float, edge1: float, x: np.ndarray) -> np.ndarray:
    t = np.clip((np.asarray(x, dtype=np.float64) - float(edge0)) / (float(edge1) - float(edge0) + 1.0e-9), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def material_hint_fields(
    height: np.ndarray,
    corridor: np.ndarray,
    *,
    span_m: float,
    height_scale_m: float = HEIGHT_SCALE_M,
) -> dict[str, np.ndarray]:
    """Derive page-stable material hints from the accepted world-layer facts.

    These are world-layer descriptors, not final materials. They are generated
    over the coherent conditioned field before slicing so chunks/pages inherit a
    stable low-pass/floor/rock/snow signal instead of re-deriving hints from a
    local page window.
    """
    h = np.asarray(height, dtype=np.float64)
    corridor_bool = np.asarray(corridor, dtype=bool)
    if h.ndim != 2:
        raise ValueError("material_hint_fields: height must be a 2D field")
    if corridor_bool.shape != h.shape:
        raise ValueError("material_hint_fields: corridor shape must match height")
    if min(h.shape) < 2:
        raise ValueError("material_hint_fields: height dimensions must be >= 2")

    cell_m = float(span_m) / float(max(h.shape[0], h.shape[1]) - 1)
    dz, dx = np.gradient(h * float(height_scale_m), cell_m, cell_m)
    slope = np.hypot(dx, dz)

    h35 = float(np.percentile(h, 35.0))
    h55 = float(np.percentile(h, 55.0))
    h78 = float(np.percentile(h, 78.0))
    h94 = float(np.percentile(h, 94.0))
    slope45 = float(np.percentile(slope, 45.0))
    slope88 = float(np.percentile(slope, 88.0))

    corridor_hint = corridor_bool.astype(np.float64)
    low_height = 1.0 - _smoothstep(h35, h78, h)
    gentle = 1.0 - _smoothstep(slope45, slope88, slope)
    floor_hint = np.clip(np.maximum(corridor_hint, low_height * gentle), 0.0, 1.0)
    snow_hint = np.clip(_smoothstep(h78, h94, h) * (1.0 - 0.65 * corridor_hint), 0.0, 1.0)
    rock_hint = np.clip(
        _smoothstep(slope45, slope88, slope)
        * (0.35 + 0.65 * _smoothstep(h55, h94, h))
        * (1.0 - 0.55 * floor_hint),
        0.0,
        1.0,
    )
    return {
        "low_pass_hint": corridor_hint,
        "floor_hint": floor_hint,
        "rock_hint": rock_hint,
        "snow_hint": snow_hint,
    }


def material_hint_summary(hints: dict[str, np.ndarray]) -> dict[str, float]:
    out: dict[str, float] = {}
    for name in MATERIAL_HINT_FIELDS:
        arr = np.asarray(hints[name], dtype=np.float64)
        out[f"{name}_mean"] = float(np.mean(arr))
        out[f"{name}_coverage"] = float(np.mean(arr >= 0.5))
    return out


def corridor_mask(result: dict[str, np.ndarray | mountain.MountainStyle]) -> np.ndarray:
    lowland = np.asarray(result["lowland"], dtype=np.float64)
    channels = np.maximum(
        np.asarray(result["primary_channels"], dtype=np.float64),
        np.asarray(result["tributaries"], dtype=np.float64),
    )
    low_cut = float(np.quantile(lowland, 0.62))
    channel_cut = float(np.quantile(channels, 0.82))
    return (lowland >= low_cut) | (channels >= channel_cut)


def stitch_grid(chunks: list[dict[str, object]], chunk_count: int, chunk_n: int, field: str) -> np.ndarray:
    step = int(chunk_n) - 1
    world_n = int(chunk_count) * step + 1
    out = np.zeros((world_n, world_n), dtype=np.float64)
    for chunk in chunks:
        x = int(chunk["chunk_x"])
        z = int(chunk["chunk_z"])
        arr = np.asarray(chunk[field], dtype=np.float64).reshape((chunk_n, chunk_n))
        out[z * step : z * step + chunk_n, x * step : x * step + chunk_n] = arr
    return out


def source_origin_for_display(
    payload: dict[str, object],
    *,
    display_origin_x_m: float,
    display_origin_z_m: float,
) -> tuple[float, float]:
    """Map a display page origin into the accepted source-world coordinate frame."""
    return _source_origin_for_display_values(
        world_span_m=float(payload["world_span_m"]),
        source_scene_ratio=float(payload["source_scene_ratio"]),
        world_origin_x_m=float(payload["world_origin_x_m"]),
        world_origin_z_m=float(payload["world_origin_z_m"]),
        display_origin_x_m=float(display_origin_x_m),
        display_origin_z_m=float(display_origin_z_m),
    )


def _source_origin_for_display_values(
    *,
    world_span_m: float,
    source_scene_ratio: float,
    world_origin_x_m: float,
    world_origin_z_m: float,
    display_origin_x_m: float,
    display_origin_z_m: float,
) -> tuple[float, float]:
    display_min_x = -0.5 * float(world_span_m)
    display_min_z = -0.5 * float(world_span_m)
    return (
        float(world_origin_x_m) + (float(display_origin_x_m) - display_min_x) * float(source_scene_ratio),
        float(world_origin_z_m) + (float(display_origin_z_m) - display_min_z) * float(source_scene_ratio),
    )


def _bilinear_sample(
    field: np.ndarray,
    xs: np.ndarray,
    zs: np.ndarray,
    *,
    origin_x_m: float,
    origin_z_m: float,
    span_m: float,
) -> np.ndarray:
    arr = np.asarray(field, dtype=np.float64)
    if arr.ndim != 2:
        raise ValueError("sample_runtime_page: field must be 2D")
    if min(arr.shape) < 2:
        raise ValueError("sample_runtime_page: field dimensions must be >= 2")
    n_z, n_x = arr.shape
    u = np.clip((xs - float(origin_x_m)) / float(span_m) * float(n_x - 1), 0.0, float(n_x - 1) - 1.0e-9)
    v = np.clip((zs - float(origin_z_m)) / float(span_m) * float(n_z - 1), 0.0, float(n_z - 1) - 1.0e-9)

    x0 = np.floor(u).astype(np.int64)
    z0 = np.floor(v).astype(np.int64)
    x1 = np.minimum(x0 + 1, n_x - 1)
    z1 = np.minimum(z0 + 1, n_z - 1)
    fx = u - x0
    fz = v - z0

    h00 = arr[z0, x0]
    h10 = arr[z0, x1]
    h01 = arr[z1, x0]
    h11 = arr[z1, x1]
    a = h00 * (1.0 - fx) + h10 * fx
    b = h01 * (1.0 - fx) + h11 * fx
    return a * (1.0 - fz) + b * fz


def sample_runtime_page(
    field: np.ndarray,
    *,
    field_origin_x_m: float,
    field_origin_z_m: float,
    field_span_m: float,
    display_origin_x_m: float,
    display_origin_z_m: float,
    page_span_m: float,
    sample_n: int,
) -> np.ndarray:
    """Sample a world-layer field into one runtime page using texel-corner coordinates."""
    if int(sample_n) < 2:
        raise ValueError("sample_runtime_page: sample_n must be >= 2")
    axis = np.linspace(0.0, float(page_span_m), int(sample_n))
    xs, zs = np.meshgrid(axis + float(display_origin_x_m), axis + float(display_origin_z_m))
    return _bilinear_sample(
        field,
        xs,
        zs,
        origin_x_m=float(field_origin_x_m),
        origin_z_m=float(field_origin_z_m),
        span_m=float(field_span_m),
    )


def sample_world_page(
    world: dict[str, object],
    *,
    chunk_count: int,
    chunk_n: int,
    page_span_m: float,
    sample_n: int,
    field: str = "height",
    display_chunk_span_m: float = DISPLAY_CHUNK_SPAN_M,
    display_origin_x_m: float = 0.0,
    display_origin_z_m: float = 0.0,
) -> np.ndarray:
    """Sample a stitched accepted world-layer field into one runtime page."""
    stitched = stitch_grid(world["chunks"], int(chunk_count), int(chunk_n), field)
    world_span_m = float(display_chunk_span_m) * float(chunk_count)
    return sample_runtime_page(
        stitched,
        field_origin_x_m=-0.5 * world_span_m,
        field_origin_z_m=-0.5 * world_span_m,
        field_span_m=world_span_m,
        display_origin_x_m=float(display_origin_x_m),
        display_origin_z_m=float(display_origin_z_m),
        page_span_m=float(page_span_m),
        sample_n=int(sample_n),
    )


def sample_payload_page(
    payload: dict[str, object],
    *,
    page_span_m: float,
    sample_n: int,
    seed_index: int = 0,
    field: str = "height",
    display_origin_x_m: float = 0.0,
    display_origin_z_m: float = 0.0,
) -> np.ndarray:
    """Sample a generated mountain-network payload into one runtime page."""
    world = payload["seeds"][int(seed_index)]
    return sample_world_page(
        world,
        chunk_count=int(payload["chunk_count"]),
        chunk_n=int(payload["chunk_n"]),
        field=field,
        display_chunk_span_m=float(payload["chunk_span_m"]),
        display_origin_x_m=float(display_origin_x_m),
        display_origin_z_m=float(display_origin_z_m),
        page_span_m=float(page_span_m),
        sample_n=int(sample_n),
    )


def _world_layer_fields_from_chunks(world: dict[str, object], chunk_count: int, chunk_n: int) -> dict[str, np.ndarray]:
    return {
        field: stitch_grid(world["chunks"], int(chunk_count), int(chunk_n), field)
        for field in WORLD_LAYER_FIELDS
    }


def build_runtime_world_layer_tile(
    world: dict[str, object],
    *,
    chunk_count: int,
    chunk_n: int,
    source_chunk_span_m: float = SOURCE_CHUNK_SPAN_M,
    display_chunk_span_m: float = DISPLAY_CHUNK_SPAN_M,
    origin_x_m: float = WORLD_ORIGIN_X_M,
    origin_z_m: float = WORLD_ORIGIN_Z_M,
) -> dict[str, object]:
    """Package an accepted mountain world into a runtime-cacheable tile.

    The tile is the boundary the live runtime should consume: coherent fields
    plus their source/display mapping and world-layer facts. JSON exporters can
    still slice chunks for review assets, but runtime sampling should not
    re-derive page-local mountains from the seam-safe recipe.
    """
    fields = _world_layer_fields_from_chunks(world, int(chunk_count), int(chunk_n))
    display_world_span_m = float(chunk_count) * float(display_chunk_span_m)
    source_world_span_m = float(chunk_count) * float(source_chunk_span_m)
    pass_network = dict(world.get("pass_network", {}))
    material_hints = dict(world.get("material_hints", {}))
    stats = dict(world.get("stats", {}))
    return {
        "source_scope": "generated_mountain_world_layer_tile_for_runtime_cache",
        "generator_version": NETWORK_GENERATOR_VERSION,
        "seed": int(world["seed"]),
        "style_key": str(world["style_key"]),
        "label": str(world["label"]),
        "chunk_count": int(chunk_count),
        "chunk_n": int(chunk_n),
        "field_n": int(next(iter(fields.values())).shape[0]),
        "field_origin_x_m": -0.5 * display_world_span_m,
        "field_origin_z_m": -0.5 * display_world_span_m,
        "field_span_m": display_world_span_m,
        "source_origin_x_m": float(origin_x_m),
        "source_origin_z_m": float(origin_z_m),
        "source_span_m": source_world_span_m,
        "source_scene_ratio": float(source_chunk_span_m) / float(display_chunk_span_m),
        "height_scale_m": float(HEIGHT_SCALE_M),
        "stats": stats,
        "pass_network": pass_network,
        "material_hints": material_hints,
        "facts": {
            "has_pass_network": int(pass_network.get("routes", 0)) > 0,
            "has_world_conditioning": float(stats.get("conditioned_ptp", 0.0)) > 0.0,
            "has_material_hint_fields": all(name in fields for name in MATERIAL_HINT_FIELDS),
        },
        "fields": fields,
    }


def source_origin_for_world_layer_tile(
    tile: dict[str, object],
    *,
    display_origin_x_m: float,
    display_origin_z_m: float,
) -> tuple[float, float]:
    """Map a runtime tile display page origin into its source-world coordinates."""
    return _source_origin_for_display_values(
        world_span_m=float(tile["field_span_m"]),
        source_scene_ratio=float(tile["source_scene_ratio"]),
        world_origin_x_m=float(tile["source_origin_x_m"]),
        world_origin_z_m=float(tile["source_origin_z_m"]),
        display_origin_x_m=float(display_origin_x_m),
        display_origin_z_m=float(display_origin_z_m),
    )


def sample_world_layer_tile_page(
    tile: dict[str, object],
    *,
    page_span_m: float,
    sample_n: int,
    field: str = "height",
    display_origin_x_m: float = 0.0,
    display_origin_z_m: float = 0.0,
) -> np.ndarray:
    """Sample a runtime-cacheable accepted world-layer tile into one page."""
    fields = tile["fields"]
    if field not in fields:
        raise KeyError(f"sample_world_layer_tile_page: unknown field {field!r}")
    return sample_runtime_page(
        fields[field],
        field_origin_x_m=float(tile["field_origin_x_m"]),
        field_origin_z_m=float(tile["field_origin_z_m"]),
        field_span_m=float(tile["field_span_m"]),
        display_origin_x_m=float(display_origin_x_m),
        display_origin_z_m=float(display_origin_z_m),
        page_span_m=float(page_span_m),
        sample_n=int(sample_n),
    )


def build_network_world(
    style: mountain.MountainStyle,
    *,
    seed: int = SEED,
    chunk_count: int = CHUNK_COUNT,
    chunk_n: int = CHUNK_N,
    source_chunk_span_m: float = SOURCE_CHUNK_SPAN_M,
    display_chunk_span_m: float = DISPLAY_CHUNK_SPAN_M,
    origin_x_m: float = WORLD_ORIGIN_X_M,
    origin_z_m: float = WORLD_ORIGIN_Z_M,
    network: mpn.PassNetworkParams | None = None,
) -> dict[str, object]:
    step = int(chunk_n) - 1
    world_n = int(chunk_count) * step + 1
    source_world_span_m = float(chunk_count) * float(source_chunk_span_m)
    source_cell_m = float(source_chunk_span_m) / float(step)
    padded_n = world_n + 2
    padded_span_m = source_world_span_m + 2.0 * source_cell_m
    wx, wz = mountain.grid(
        padded_n,
        padded_span_m,
        ox=float(origin_x_m) - source_cell_m,
        oz=float(origin_z_m) - source_cell_m,
    )
    result = mountain.generate(wx, wz, seed=int(seed), style=style, feature_span_m=FEATURE_SPAN_M)
    raw = np.asarray(result["height"], dtype=np.float64)

    display_total_m = float(display_chunk_span_m) * float(chunk_count)
    pp = network if network is not None else mpn.PassNetworkParams(n_we=6, n_ns=6, coarse_n=257)
    carved = mpn.carve_pass_network(raw, span_m=display_total_m, height_scale_m=HEIGHT_SCALE_M, pp=pp)
    raw_carved = raw + carved["delta"]

    height, stats = condition_world(raw_carved)
    corridor = corridor_mask(result)
    display_cell_m = display_total_m / float(world_n - 1)
    material_hints = material_hint_fields(
        height,
        corridor,
        span_m=display_total_m + 2.0 * display_cell_m,
        height_scale_m=HEIGHT_SCALE_M,
    )
    out_chunks: list[dict[str, object]] = []
    for z in range(int(chunk_count)):
        for x in range(int(chunk_count)):
            start_x = 1 + x * step
            start_z = 1 + z * step
            core = height[start_z : start_z + chunk_n, start_x : start_x + chunk_n]
            apron = height[start_z - 1 : start_z + chunk_n + 1, start_x - 1 : start_x + chunk_n + 1]
            core_corridor = corridor[start_z : start_z + chunk_n, start_x : start_x + chunk_n]
            apron_corridor = corridor[start_z - 1 : start_z + chunk_n + 1, start_x - 1 : start_x + chunk_n + 1]
            core_hints = {
                name: material_hints[name][start_z : start_z + chunk_n, start_x : start_x + chunk_n]
                for name in MATERIAL_HINT_FIELDS
            }
            apron_hints = {
                name: material_hints[name][start_z - 1 : start_z + chunk_n + 1, start_x - 1 : start_x + chunk_n + 1]
                for name in MATERIAL_HINT_FIELDS
            }
            display_origin_x = (float(x) - float(chunk_count) * 0.5) * float(display_chunk_span_m)
            display_origin_z = (float(z) - float(chunk_count) * 0.5) * float(display_chunk_span_m)
            chunk: dict[str, object] = {
                "source": "full_field_slice_with_pass_network",
                "chunk_x": int(x),
                "chunk_z": int(z),
                "key": f"{style.key}_{x}_{z}",
                "label": f"{style.label} chunk {x},{z}",
                "n": int(chunk_n),
                "apron_n": int(chunk_n) + 2,
                "span_m": float(display_chunk_span_m),
                "source_span_m": float(source_chunk_span_m),
                "world_origin_x_m": float(origin_x_m) + float(x) * float(source_chunk_span_m),
                "world_origin_z_m": float(origin_z_m) + float(z) * float(source_chunk_span_m),
                "display_origin_x_m": display_origin_x,
                "display_origin_z_m": display_origin_z,
                "height": np.round(core, 4).astype(float).ravel().tolist(),
                "apron_height": np.round(apron, 4).astype(float).ravel().tolist(),
                "corridor": core_corridor.astype(int).ravel().tolist(),
                "apron_corridor": apron_corridor.astype(int).ravel().tolist(),
            }
            for name in MATERIAL_HINT_FIELDS:
                chunk[name] = np.round(core_hints[name], 4).astype(float).ravel().tolist()
                chunk[f"apron_{name}"] = np.round(apron_hints[name], 4).astype(float).ravel().tolist()
            out_chunks.append(chunk)
    stitched = stitch_grid(out_chunks, int(chunk_count), int(chunk_n), "height")
    stitched_hints = {
        name: stitch_grid(out_chunks, int(chunk_count), int(chunk_n), name)
        for name in MATERIAL_HINT_FIELDS
    }
    return {
        "seed": int(seed),
        "label": style.label,
        "style_key": style.key,
        "corridor_height": float(np.percentile(stitched, 55.0)),
        "world_n": int(stitched.shape[0]),
        "stats": stats,
        "material_hints": material_hint_summary(stitched_hints),
        "chunks": out_chunks,
        "pass_network": {
            "routes": len(carved["routes"]),
            "band_walkable_frac": round(carved["band_passable_frac"], 3),
            "carved_frac": round(carved["carved_frac"], 3),
        },
    }


def build_network_payload(
    *,
    styles: Iterable[mountain.MountainStyle] = mountain.STYLES,
    seed: int = SEED,
    chunk_count: int = CHUNK_COUNT,
    chunk_n: int = CHUNK_N,
    source_chunk_span_m: float = SOURCE_CHUNK_SPAN_M,
    display_chunk_span_m: float = DISPLAY_CHUNK_SPAN_M,
    origin_x_m: float = WORLD_ORIGIN_X_M,
    origin_z_m: float = WORLD_ORIGIN_Z_M,
) -> dict[str, object]:
    worlds = [
        build_network_world(
            style,
            seed=seed,
            chunk_count=chunk_count,
            chunk_n=chunk_n,
            source_chunk_span_m=source_chunk_span_m,
            display_chunk_span_m=display_chunk_span_m,
            origin_x_m=origin_x_m,
            origin_z_m=origin_z_m,
        )
        for style in styles
    ]
    return {
        "title": "WorldGen10 mountain 9x9 + connected pass network",
        "generator_version": NETWORK_GENERATOR_VERSION,
        "source_scope": "coherent_full_field_carved_with_pass_network_sliced_for_review",
        "chunk_count": int(chunk_count),
        "chunk_n": int(chunk_n),
        "chunk_span_m": float(display_chunk_span_m),
        "source_chunk_span_m": float(source_chunk_span_m),
        "feature_span_m": float(FEATURE_SPAN_M),
        "world_span_m": float(chunk_count) * float(display_chunk_span_m),
        "source_world_span_m": float(chunk_count) * float(source_chunk_span_m),
        "source_scene_ratio": float(source_chunk_span_m) / float(display_chunk_span_m),
        "world_origin_x_m": float(origin_x_m),
        "world_origin_z_m": float(origin_z_m),
        "height_scale_m": float(HEIGHT_SCALE_M),
        "review_variants": list(REVIEW_VARIANTS),
        "seeds": worlds,
    }
