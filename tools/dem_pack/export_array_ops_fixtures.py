"""Export parity fixtures for the two whole-array WorldGen10 operators.

These fixtures are the PARITY ORACLE for the Rust port
(`wg-10/rust/src/array_ops.rs`). They capture, at f64 precision, the exact
output of the two non-per-point array operators that every biome recipe needs:

1. gaussian_filter with mode='nearest' -- exactly what
   `seam_safe.apron_blur_crop` runs internally
   (`scipy.ndimage.gaussian_filter(field, sigma=s, mode='nearest', truncate=t)`).
   The Rust `gaussian_filter_nearest` must reproduce scipy's separable kernel
   (radius = int(truncate*sigma + 0.5); phi_x = exp(-0.5/sigma^2 * x^2) for
   x in [-radius, radius], normalized to sum 1; applied along axis 0 then axis 1
   with 'nearest'/clamp-to-edge boundary).

2. flow_accumulation_mfd -- the exact whole-grid sequential sorted sweep
   `geography_skeleton._flow_accumulation_mfd(surface, power)`
   (acc=ones; order=argsort(-hflat) high->low; for each cell distribute acc to
   downhill 8-neighbors weighted by (drop/dist)^power).

Run from repo root:  python tools/dem_pack/export_array_ops_fixtures.py
Writes:              tools/dem_pack/fixtures/array_ops_fixtures.json

Fields are kept small (16x16 / 24x24 and below) so the JSON stays compact.
All numbers are emitted with full float64 repr (json.dump default) so the
Rust side reads bit-identical inputs and expected outputs.
"""

import json
import os
import sys

import numpy as np
from scipy.ndimage import gaussian_filter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import worldgen_proto as wg  # noqa: E402
from geography_skeleton import _flow_accumulation_mfd  # noqa: E402

GENERATOR_VERSION = "array_ops_fixtures/v1"


def _noise_field(rows, cols, base_freq, seed, gain=0.5):
    """A reproducible fBm field over a fixed world grid (incl. negative values).

    Uses worldgen_proto.fbm on a meshgrid of world coordinates so the input is
    a "realistic" biome-style field rather than a contrived array. fbm returns
    ~[-1, 1] so the field naturally contains negatives.
    """
    xs = np.arange(cols, dtype=np.float64) * 37.0 - 200.0
    zs = np.arange(rows, dtype=np.float64) * 41.0 + 15.0
    wx, wz = np.meshgrid(xs, zs)  # shape (rows, cols)
    return wg.fbm(wx, wz, base_freq, 5, seed=seed, gain=gain).astype(np.float64)


def _peak_field(rows, cols):
    """Single central peak (a smooth bump) -- exercises monotone downhill flow."""
    zs = np.arange(rows, dtype=np.float64)
    xs = np.arange(cols, dtype=np.float64)
    cz = (rows - 1) / 2.0
    cx = (cols - 1) / 2.0
    z, x = np.meshgrid(zs, xs, indexing="ij")
    r2 = (z - cz) ** 2 + (x - cx) ** 2
    return (10.0 * np.exp(-r2 / (2.0 * (rows / 4.0) ** 2))).astype(np.float64)


def _ramp_field(rows, cols):
    """A tilted plane -- strictly monotone surface, deterministic flow, no ties
    across the gradient direction (rows weighted larger than cols so there are
    no exact-tie cells)."""
    zs = np.arange(rows, dtype=np.float64)
    xs = np.arange(cols, dtype=np.float64)
    z, x = np.meshgrid(zs, xs, indexing="ij")
    return (3.0 * z + 1.0 * x).astype(np.float64)


def main():
    records = []

    def add(op, inputs, field, expected):
        rows, cols = field.shape
        records.append({
            "op": op,
            "inputs": inputs,
            "rows": int(rows),
            "cols": int(cols),
            "field": [float(v) for v in field.ravel(order="C").tolist()],
            "expected": [float(v) for v in np.asarray(expected).ravel(order="C").tolist()],
        })

    # ---- input fields (small, row-major) -------------------------------------
    fields = {
        # name: (field, rows, cols)
        "noise16": _noise_field(16, 16, 1.0 / 380.0, seed=7, gain=0.55),
        "noise24": _noise_field(24, 24, 1.0 / 520.0, seed=42, gain=0.5),
        "noise20x12": _noise_field(20, 12, 1.0 / 300.0, seed=1337, gain=0.5),
        "flat8": np.full((8, 8), -2.5, dtype=np.float64),
        "peak16": _peak_field(16, 16),
        "ramp12x10": _ramp_field(12, 10),
        # a tiny hand-fixed array with negatives + an edge spike (boundary clamp test)
        "fixed6x6": np.array([
            [-3.0, -1.0, 0.0, 2.0, 5.0, -4.0],
            [-2.0, 1.5, 3.0, 0.5, -1.0, 4.0],
            [0.0, 2.0, -5.0, 1.0, 2.0, 0.0],
            [4.0, -2.0, 1.0, -3.0, 0.0, 1.0],
            [1.0, 0.0, 2.0, 3.0, -1.0, -2.0],
            [-1.0, 6.0, -6.0, 0.0, 1.0, 3.0],
        ], dtype=np.float64),
    }

    # ---- gaussian_filter (mode='nearest') ------------------------------------
    truncate = 4.0
    for sigma in (0.7, 1.15, 2.0, 5.0):
        for name, fld in fields.items():
            out = gaussian_filter(fld, sigma=float(sigma), mode="nearest",
                                  truncate=float(truncate))
            add("gaussian_filter_nearest",
                {"field_name": name, "sigma": float(sigma), "truncate": float(truncate)},
                fld, out)

    # ---- flow_accumulation_mfd -----------------------------------------------
    # Surfaces should have a meaningful relief; flat field is included to verify
    # the all-equal / no-downhill branch (every cell keeps acc=1).
    flow_fields = {
        "noise16": fields["noise16"],
        "noise24": fields["noise24"],
        "noise20x12": fields["noise20x12"],
        "peak16": fields["peak16"],
        "ramp12x10": fields["ramp12x10"],
        "flat8": fields["flat8"],
        "fixed6x6": fields["fixed6x6"],
    }
    for power in (0.48, 1.45):
        for name, fld in flow_fields.items():
            acc = _flow_accumulation_mfd(fld, power=float(power))
            add("flow_accumulation_mfd",
                {"field_name": name, "power": float(power)},
                fld, acc)

    out_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
    os.makedirs(out_dir, exist_ok=True)
    out_path = os.path.join(out_dir, "array_ops_fixtures.json")

    by_op = {}
    for r in records:
        by_op[r["op"]] = by_op.get(r["op"], 0) + 1

    doc = {
        "generator_version": GENERATOR_VERSION,
        "source": (
            "gaussian_filter_nearest <- scipy.ndimage.gaussian_filter("
            "mode='nearest') via tools/dem_pack/seam_safe.py::apron_blur_crop; "
            "flow_accumulation_mfd <- tools/dem_pack/geography_skeleton.py::"
            "_flow_accumulation_mfd"
        ),
        "note": (
            "Whole-array parity oracle for wg-10/rust/src/array_ops.rs (f64). "
            "Fields are row-major (order='C')."
        ),
        "counts": by_op,
        "records": records,
    }
    with open(out_path, "w", encoding="ascii") as f:
        json.dump(doc, f)  # compact (no indent) -- fields are large

    print("wrote", out_path)
    print("total records:", len(records))
    for k in sorted(by_op):
        print("  {0:24s} {1}".format(k, by_op[k]))


if __name__ == "__main__":
    main()
