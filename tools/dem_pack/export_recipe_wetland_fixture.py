"""Export the WETLAND recipe-level parity fixture for the Rust biome-recipe port.

Mirrors tools/dem_pack/export_recipe_fixtures.py (the MOUNTAIN template) exactly,
but runs the REAL wetland_synthesis.generate(apron_px > 0) seam-safe path.

It runs the Python recipe in SEAM-SAFE mode on an apron-padded world-coordinate
grid, and records the CORE-cropped output height grid at full float64 precision.
The Rust side (recipes_wetland.rs::wetland_seamsafe) rebuilds the SAME padded
wx/wz from grid + apron_px, runs the recipe, and must reproduce height within
a tight epsilon.

NOTE: wetland is a TERRAIN/MASK setup biome (water/flooding is later work); only
its HEIGHT generation is the parity oracle here (the diagnostic masks are not
recorded). Wetland is LOW-RELIEF/FLAT with connected channels, so the MFD flow
accumulation matters and exact-height ties are more likely than in steeper biomes;
the parity test prints the measured max |delta| to confirm any tie-order drift
stays at the f64 noise floor.

Fixture format is identical to recipe_mountain_fixture.json (generator_version
"recipe_fixtures/v1"), so the Rust test reuses the same Doc/Record structs.

CORE small (24x24) so JSON is compact; APRON real (WETLAND_APRON_PX = 160 ->
padded 344x344): the seam-safe pipeline (whole-array gaussians + MFD flow
accumulation on the padded grid) is exercised at full size, then cropped.

Run from repo root:  python tools/dem_pack/export_recipe_wetland_fixture.py
Writes:              tools/dem_pack/fixtures/recipe_wetland_fixture.json
"""

import json
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wetland_synthesis as ws  # noqa: E402

GENERATOR_VERSION = "recipe_fixtures/v1"


def _apron_grid(core_n, feature_span_m, apron_px, ox=0.0, oz=0.0):
    """Apron-padded world-coord grid (identical construction to the mountain exporter)."""
    n = int(core_n)
    a = int(apron_px)
    spacing = float(feature_span_m) / max(n - 1, 1)
    total = n + 2 * a
    xs = (np.arange(total, dtype=np.float64) - a) * spacing + float(ox)
    zs = (np.arange(total, dtype=np.float64) - a) * spacing + float(oz)
    wx, wz = np.meshgrid(xs, zs)  # shape (total, total)
    return wx, wz, spacing


def main():
    records = []

    core_n = 24
    apron_px = ws.WETLAND_APRON_PX  # 160 -> padded 344x344
    feature_span_m = 90_000.0  # fixed constant shared by adjacent windows
    style = ws.STYLES[0]  # delta_distributary

    seeds = (0, 7)  # two seeds so parity is not a one-off coincidence

    ox, oz = 12_000.0, -31_000.0
    for seed in seeds:
        wx, wz, spacing = _apron_grid(core_n, feature_span_m, apron_px, ox=ox, oz=oz)
        result = ws.generate(
            wx,
            wz,
            seed=seed,
            style=style,
            feature_span_m=feature_span_m,
            apron_px=apron_px,
        )
        height = np.asarray(result["height"], dtype=np.float64)
        assert height.shape == (core_n, core_n), (
            "unexpected core shape {0} (want {1})".format(height.shape, (core_n, core_n))
        )
        padded_rows, padded_cols = wx.shape
        records.append({
            "recipe": "wetland_seamsafe",
            "seed": int(seed),
            "feature_span_m": float(feature_span_m),
            "apron_px": int(apron_px),
            "style_key": style.key,
            "core_rows": int(core_n),
            "core_cols": int(core_n),
            "padded_rows": int(padded_rows),
            "padded_cols": int(padded_cols),
            "grid": {"spacing": float(spacing), "ox": float(ox), "oz": float(oz)},
            "height": [float(v) for v in height.ravel(order="C").tolist()],
        })

    out_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
    os.makedirs(out_dir, exist_ok=True)
    out_path = os.path.join(out_dir, "recipe_wetland_fixture.json")

    doc = {
        "generator_version": GENERATOR_VERSION,
        "source": (
            "wetland_seamsafe <- tools/dem_pack/wetland_synthesis.py::generate("
            "apron_px=WETLAND_APRON_PX) seam-safe path; "
            "core-cropped height is the parity oracle for "
            "wg-10/rust/src/recipes_wetland.rs::wetland_seamsafe"
        ),
        "note": (
            "Recipe-level parity oracle (f64). The apron-padded wx/wz grids are "
            "rebuilt analytically from grid={spacing,ox,oz} + apron_px on both "
            "sides (xs[c]=(c-apron_px)*spacing+ox, zs[r]=(r-apron_px)*spacing+oz, "
            "numpy meshgrid); height is the CORE-cropped output. Style = STYLES[0] "
            "(delta_distributary). Wetland is low-relief/flat: any drift above the "
            "f64 floor would come from MFD flow-accumulation tie ordering."
        ),
        "records": records,
    }
    with open(out_path, "w", encoding="ascii") as f:
        json.dump(doc, f)  # compact (no indent) -- fields are large

    print("wrote", out_path)
    print("records:", len(records))
    for r in records:
        hh = np.asarray(r["height"], dtype=np.float64)
        print(
            "  recipe={0} seed={1} core={2}x{3} padded={4}x{5} "
            "height[min={6:.4f} max={7:.4f} mean={8:.4f}]".format(
                r["recipe"], r["seed"], r["core_rows"], r["core_cols"],
                r["padded_rows"], r["padded_cols"],
                float(hh.min()), float(hh.max()), float(hh.mean()),
            )
        )


if __name__ == "__main__":
    main()
