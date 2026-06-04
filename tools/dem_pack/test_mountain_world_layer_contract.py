import json
from pathlib import Path

import numpy as np

import export_godot_mountain_world_chunks as chunks
import mountain_synthesis as mountain


NETWORK_PAYLOAD = Path("wg-10/worldgen_terrain/generated/review/mountain_network_chunks.json")
DISPLAY_PAGE_SPAN_M = 8192.0
SAMPLE_N = 65


def _load_network_payload() -> dict:
    return json.loads(NETWORK_PAYLOAD.read_text(encoding="utf-8"))


def _bilinear_sample(field: np.ndarray, xs: np.ndarray, zs: np.ndarray, *, origin_x: float, origin_z: float, span_m: float) -> np.ndarray:
    n = int(field.shape[0])
    u = np.clip((xs - float(origin_x)) / float(span_m) * float(n - 1), 0.0, float(n - 1) - 1.0e-9)
    v = np.clip((zs - float(origin_z)) / float(span_m) * float(n - 1), 0.0, float(n - 1) - 1.0e-9)

    x0 = np.floor(u).astype(np.int64)
    z0 = np.floor(v).astype(np.int64)
    x1 = np.minimum(x0 + 1, n - 1)
    z1 = np.minimum(z0 + 1, n - 1)
    fx = u - x0
    fz = v - z0

    h00 = field[z0, x0]
    h10 = field[z0, x1]
    h01 = field[z1, x0]
    h11 = field[z1, x1]
    a = h00 * (1.0 - fx) + h10 * fx
    b = h01 * (1.0 - fx) + h11 * fx
    return a * (1.0 - fz) + b * fz


def _accepted_reference_page(payload: dict, *, display_origin_x: float, display_origin_z: float) -> np.ndarray:
    world = payload["seeds"][0]
    field = chunks._stitch_grid(world["chunks"], int(payload["chunk_count"]), int(payload["chunk_n"]), "height")
    world_span = float(payload["world_span_m"])
    display_min_x = -0.5 * world_span
    display_min_z = -0.5 * world_span

    axis = np.linspace(0.0, DISPLAY_PAGE_SPAN_M, SAMPLE_N)
    xs, zs = np.meshgrid(axis + display_origin_x, axis + display_origin_z)
    return _bilinear_sample(field, xs, zs, origin_x=display_min_x, origin_z=display_min_z, span_m=world_span)


def _source_origin_for_display(payload: dict, *, display_origin_x: float, display_origin_z: float) -> tuple[float, float]:
    world_span = float(payload["world_span_m"])
    ratio = float(payload["source_scene_ratio"])
    display_min_x = -0.5 * world_span
    display_min_z = -0.5 * world_span
    return (
        float(payload["world_origin_x_m"]) + (float(display_origin_x) - display_min_x) * ratio,
        float(payload["world_origin_z_m"]) + (float(display_origin_z) - display_min_z) * ratio,
    )


def _live_seamsafe_page(payload: dict, *, display_origin_x: float, display_origin_z: float) -> np.ndarray:
    ratio = float(payload["source_scene_ratio"])
    source_span_m = DISPLAY_PAGE_SPAN_M * ratio
    source_origin_x, source_origin_z = _source_origin_for_display(
        payload,
        display_origin_x=display_origin_x,
        display_origin_z=display_origin_z,
    )
    spacing_m = source_span_m / float(SAMPLE_N - 1)
    apron_px = int(mountain.MOUNTAIN_APRON_PX)
    padded_n = SAMPLE_N + 2 * apron_px
    padded_span_m = source_span_m + 2.0 * float(apron_px) * spacing_m
    wx, wz = mountain.grid(
        padded_n,
        padded_span_m,
        ox=source_origin_x - float(apron_px) * spacing_m,
        oz=source_origin_z - float(apron_px) * spacing_m,
    )
    result = mountain.generate(
        wx,
        wz,
        seed=int(payload["seeds"][0]["seed"]),
        style=mountain.STYLES[0],
        feature_span_m=float(payload["feature_span_m"]),
        apron_px=apron_px,
        spacing_m=spacing_m,
        flow_on=True,
    )
    return np.asarray(result["height"], dtype=np.float64)


def _gap_metrics(reference: np.ndarray, live: np.ndarray) -> dict[str, float]:
    ref = np.asarray(reference, dtype=np.float64)
    got = np.asarray(live, dtype=np.float64)
    delta = np.abs(ref - got)
    corr = float(np.corrcoef(ref.ravel(), got.ravel())[0, 1])
    return {
        "mean_abs": float(np.mean(delta)),
        "p95_abs": float(np.percentile(delta, 95.0)),
        "peak_abs": float(np.max(delta)),
        "corr": corr,
        "ref_ptp": float(np.ptp(ref)),
        "live_ptp": float(np.ptp(got)),
    }


def test_mountain_network_payload_declares_accepted_world_layer_contract():
    payload = _load_network_payload()

    assert payload["source_scope"] == "coherent_full_field_carved_with_pass_network_sliced_for_review"
    assert payload["chunk_count"] == chunks.CHUNK_COUNT
    assert payload["chunk_n"] == chunks.CHUNK_N
    assert payload["feature_span_m"] == chunks.FEATURE_SPAN_M
    assert payload["height_scale_m"] == chunks.HEIGHT_SCALE_M
    assert np.isclose(payload["source_scene_ratio"], chunks.SOURCE_CHUNK_SPAN_M / chunks.DISPLAY_CHUNK_SPAN_M)


def test_live_seamsafe_mountain_page_is_not_yet_the_accepted_network_layer():
    payload = _load_network_payload()
    reference = _accepted_reference_page(payload, display_origin_x=0.0, display_origin_z=0.0)
    live = _live_seamsafe_page(payload, display_origin_x=0.0, display_origin_z=0.0)
    metrics = _gap_metrics(reference, live)

    print(
        "[mountain-layer-gap] "
        f"mean_abs={metrics['mean_abs']:.6f} "
        f"p95_abs={metrics['p95_abs']:.6f} "
        f"peak_abs={metrics['peak_abs']:.6f} "
        f"corr={metrics['corr']:.6f} "
        f"ref_ptp={metrics['ref_ptp']:.6f} "
        f"live_ptp={metrics['live_ptp']:.6f}"
    )

    assert metrics["ref_ptp"] > 1.0
    assert metrics["live_ptp"] > 0.5
    assert metrics["mean_abs"] > 0.20
    assert metrics["p95_abs"] > 0.45
    assert metrics["corr"] < 0.80
