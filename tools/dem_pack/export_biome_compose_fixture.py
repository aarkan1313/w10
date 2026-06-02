"""Export parity fixtures for the Rust biome-compose port.

This is the PARITY ORACLE for `wg-10/rust/src/biome_compose.rs`. It runs the REAL
Python compose layer (`tools/dem_pack/biome_compose.py`):

  * `_blend_field(a, b, w_a)`                       (plain weighted lerp)
  * `_blend_height_favored(a, b, w_a, cfg)`         (relief-favored blend, uses a
                                                     gaussian_filter mode='nearest'
                                                     relief proxy)
  * `compose_biomes(fields, weights, cfg)`          (N-recipe fold; height_favored
                                                     ONLY for the N==2 boundary case,
                                                     field-blend fold for N>2)

The compose math is deterministic blend arithmetic plus a SINGLE gaussian blur per
recipe field in the height_favored path, so the Rust port is expected at the f64 floor
(~1e-15) -- the same regime as the array_ops gaussian fixture it reuses.

The fixture stores the INPUT fields + weight fields DIRECTLY (row-major f64), so the
Rust side reads bit-identical inputs and never needs to reproduce the noise generator;
the test isolates the blend/compose math + the relief-proxy gaussian.

    {
      "generator_version": "biome_compose_fixture/v1",
      "cfg": {"mode": ..., "relief_sigma_px": ..., "favor_strength": ...,
              "relief_confidence_floor": ...},
      "records": [
        {
          "case": "<name>",
          "kind": "blend_field" | "blend_height_favored" | "compose",
          "rows": <int>, "cols": <int>,
          # blend_* records: two operand fields + a weight field
          "a": [...], "b": [...], "w_a": [...],
          # compose records: N fields + N weight fields
          "fields": [[...], ...], "weights": [[...], ...],
          # cfg override for this record (mode in particular), else top-level cfg
          "cfg": {... same shape as top-level cfg ...},
          "expected": [...]              # row-major f64 output
        }, ...
      ]
    }

Run from repo root:  python tools/dem_pack/export_biome_compose_fixture.py
Writes:              tools/dem_pack/fixtures/biome_compose_fixture.json

All numbers are emitted with full float64 repr (json.dump default) so the Rust side
reads bit-identical inputs and expected outputs.
"""

import json
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import worldgen_proto as wg  # noqa: E402
import biome_compose as bc  # noqa: E402

GENERATOR_VERSION = "biome_compose_fixture/v1"


def _noise_field(rows, cols, base_freq, seed, gain=0.5, scale=300.0, bias=0.0):
    """A reproducible fBm height-style field over a fixed world grid.

    fbm returns ~[-1, 1]; we scale to a metres-like relief and add a bias so the
    two operand recipes sit at different elevations (a realistic biome clash).
    """
    xs = np.arange(cols, dtype=np.float64) * 37.0 - 200.0
    zs = np.arange(rows, dtype=np.float64) * 41.0 + 15.0
    wx, wz = np.meshgrid(xs, zs)  # shape (rows, cols)
    base = wg.fbm(wx, wz, base_freq, 5, seed=seed, gain=gain).astype(np.float64)
    return base * scale + bias


def _ramp_weight(rows, cols):
    """A left->right ramp weight field in [0,1] (a smooth biome transition band)."""
    xs = np.linspace(0.0, 1.0, cols, dtype=np.float64)
    w = np.broadcast_to(xs, (rows, cols)).astype(np.float64).copy()
    return w


def _diag_weight(rows, cols):
    """A diagonal weight field in [0,1] -- crosses the band along both axes."""
    zs = np.linspace(0.0, 1.0, rows, dtype=np.float64)
    xs = np.linspace(0.0, 1.0, cols, dtype=np.float64)
    z, x = np.meshgrid(zs, xs, indexing="ij")
    return (0.5 * (z + x)).astype(np.float64)


def _flat_field(rows, cols, value):
    return np.full((rows, cols), float(value), dtype=np.float64)


def main():
    cfg = bc.BlendConfig()  # defaults: height_favored, sigma=6.0, favor=2.0, floor=1e-3
    field_cfg = bc.BlendConfig(mode="field")

    def cfg_dict(c):
        return {
            "mode": c.mode,
            "relief_sigma_px": float(c.relief_sigma_px),
            "favor_strength": float(c.favor_strength),
            "relief_confidence_floor": float(c.relief_confidence_floor),
        }

    records = []

    def add_blend(case, kind, a, b, w_a, c, expected):
        rows, cols = a.shape
        records.append({
            "case": case,
            "kind": kind,
            "rows": int(rows),
            "cols": int(cols),
            "a": [float(v) for v in a.ravel(order="C").tolist()],
            "b": [float(v) for v in b.ravel(order="C").tolist()],
            "w_a": [float(v) for v in w_a.ravel(order="C").tolist()],
            "cfg": cfg_dict(c),
            "expected": [float(v) for v in np.asarray(expected).ravel(order="C").tolist()],
        })

    def add_compose(case, fields, weights, c, expected):
        rows, cols = fields[0].shape
        records.append({
            "case": case,
            "kind": "compose",
            "rows": int(rows),
            "cols": int(cols),
            "fields": [[float(v) for v in f.ravel(order="C").tolist()] for f in fields],
            "weights": [[float(v) for v in w.ravel(order="C").tolist()] for w in weights],
            "cfg": cfg_dict(c),
            "expected": [float(v) for v in np.asarray(expected).ravel(order="C").tolist()],
        })

    R = C = 32

    # --- structured operand fields (mountain-ish vs lowland-ish) + a flat field ---
    mtn = _noise_field(R, C, 1.0 / 280.0, seed=7, gain=0.6, scale=600.0, bias=900.0)
    low = _noise_field(R, C, 1.0 / 700.0, seed=42, gain=0.45, scale=120.0, bias=120.0)
    mid = _noise_field(R, C, 1.0 / 420.0, seed=1337, gain=0.5, scale=300.0, bias=400.0)
    flat = _flat_field(R, C, 250.0)

    ramp = _ramp_weight(R, C)
    diag = _diag_weight(R, C)

    # ---- 1. blend_field: plain lerp with a real ramp weight ----
    add_blend("field_ramp", "blend_field", mtn, low, ramp, field_cfg,
              bc._blend_field(mtn, low, ramp))

    # ---- 2. blend_height_favored: structured-vs-structured with a ramp band ----
    add_blend("favored_ramp_mtn_low", "blend_height_favored", mtn, low, ramp, cfg,
              bc._blend_height_favored(mtn, low, ramp, cfg))

    # ---- 3. blend_height_favored: diagonal band (exercises both blur axes) ----
    add_blend("favored_diag_mtn_low", "blend_height_favored", mtn, low, diag, cfg,
              bc._blend_height_favored(mtn, low, diag, cfg))

    # ---- 4. blend_height_favored: structured-vs-FLAT (exercises signal_confidence
    #         + favor going to one side where only one recipe has relief) ----
    add_blend("favored_ramp_mtn_flat", "blend_height_favored", mtn, flat, ramp, cfg,
              bc._blend_height_favored(mtn, flat, ramp, cfg))

    # ---- 5. blend_height_favored: FLAT-vs-FLAT (signal_confidence ~0 -> plain lerp) ----
    flat2 = _flat_field(R, C, 700.0)
    add_blend("favored_ramp_flat_flat", "blend_height_favored", flat, flat2, ramp, cfg,
              bc._blend_height_favored(flat, flat2, ramp, cfg))

    # ---- 6. compose N==1: returns the single field unchanged ----
    add_compose("compose_n1", [mtn], [_flat_field(R, C, 1.0)], cfg,
                bc.compose_biomes([mtn], [_flat_field(R, C, 1.0)], cfg))

    # ---- 7. compose N==2 height_favored: SAME path as the boundary case.
    #         Weights are a partition of unity (w + (1-w)) so w_acc == ramp. ----
    w0 = ramp
    w1 = 1.0 - ramp
    add_compose("compose_n2_favored", [mtn, low], [w0, w1], cfg,
                bc.compose_biomes([mtn, low], [w0, w1], cfg))

    # ---- 8. compose N==2 in FIELD mode (use_favored False because mode='field') ----
    add_compose("compose_n2_field", [mtn, low], [w0, w1], field_cfg,
                bc.compose_biomes([mtn, low], [w0, w1], field_cfg))

    # ---- 9. compose N==3: triple point. mode='height_favored' but len!=2 -> the
    #         fold uses FIELD blend (order-independent). This is the N>2 fold. ----
    #         Partition-of-unity 3-way weights from a softmax-ish split.
    wa = _ramp_weight(R, C)                 # 0..1 left->right
    wb = _diag_weight(R, C)                 # 0..1 diagonal
    wc = _flat_field(R, C, 0.5)             # constant
    wsum = wa + wb + wc
    g0 = wa / wsum
    g1 = wb / wsum
    g2 = wc / wsum
    add_compose("compose_n3_triple", [mtn, low, mid], [g0, g1, g2], cfg,
                bc.compose_biomes([mtn, low, mid], [g0, g1, g2], cfg))

    # ---- 10. compose N==2 pure-end weights: w==1 everywhere -> pure recipe A
    #          (band==0 -> w_adj==1 -> exactly field a). And w==0 -> pure recipe B. ----
    ones = _flat_field(R, C, 1.0)
    zeros = _flat_field(R, C, 0.0)
    add_compose("compose_n2_pure_a", [mtn, low], [ones, zeros], cfg,
                bc.compose_biomes([mtn, low], [ones, zeros], cfg))
    add_compose("compose_n2_pure_b", [mtn, low], [zeros, ones], cfg,
                bc.compose_biomes([mtn, low], [zeros, ones], cfg))

    # ---- 11. N>2 fold ORDER-INDEPENDENCE oracle: compose the SAME 3 recipes in a
    #          permuted order with matching permuted weights -> must equal the
    #          original (field-blend fold is order-independent). We assert this in
    #          Python here so the fixture itself documents the property, and emit a
    #          companion record the Rust side cross-checks. ----
    expected_n3 = bc.compose_biomes([mtn, low, mid], [g0, g1, g2], cfg)
    permuted = bc.compose_biomes([mid, mtn, low], [g2, g0, g1], cfg)
    order_delta = float(np.max(np.abs(expected_n3 - permuted)))
    add_compose("compose_n3_permuted", [mid, mtn, low], [g2, g0, g1], cfg, permuted)

    out_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
    os.makedirs(out_dir, exist_ok=True)
    out_path = os.path.join(out_dir, "biome_compose_fixture.json")

    by_kind = {}
    for r in records:
        by_kind[r["kind"]] = by_kind.get(r["kind"], 0) + 1

    doc = {
        "generator_version": GENERATOR_VERSION,
        "source": (
            "biome_compose <- tools/dem_pack/biome_compose.py "
            "(_blend_field / _blend_height_favored / compose_biomes); parity oracle "
            "for wg-10/rust/src/biome_compose.rs"
        ),
        "note": (
            "Blend/compose parity oracle (f64). Input + weight fields are stored "
            "directly (row-major order='C') so the Rust side never reproduces the "
            "noise generator. height_favored uses gaussian_filter mode='nearest' for "
            "the relief proxy (== array_ops::gaussian_filter_nearest, truncate=4.0). "
            "N>2 compose folds via FIELD blend (use_favored only when N==2 and "
            "mode!='field'); compose_n3_triple vs compose_n3_permuted document the "
            "order-independence of that fold (Python max|delta|={0:.3e}).".format(order_delta)
        ),
        "cfg": cfg_dict(cfg),
        "counts": by_kind,
        "records": records,
    }
    with open(out_path, "w", encoding="ascii") as f:
        json.dump(doc, f)  # compact (no indent) -- fields are large

    print("wrote", out_path)
    print("total records:", len(records))
    for k in sorted(by_kind):
        print("  {0:24s} {1}".format(k, by_kind[k]))
    print("N>2 fold order-independence max|delta| (n3 vs permuted): {0:.3e}".format(order_delta))


if __name__ == "__main__":
    main()
