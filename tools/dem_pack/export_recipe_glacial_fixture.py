"""Export GLACIAL recipe-level parity fixture for the Rust biome-recipe port.

Parity oracle for `wg-10/rust/src/recipes_glacial.rs`. Runs the REAL Python
recipe (`tools/dem_pack/glacial_synthesis.py::generate`) in its SEAM-SAFE mode
(apron_px > 0) on an apron-padded world-coordinate grid, and records the
CORE-cropped output height grid at full float64 precision.

Format matches export_recipe_fixtures.py (recipe_fixtures/v1):

    {
      "generator_version": "recipe_fixtures/v1",
      "records": [
        {
          "recipe": "glacial_seamsafe",
          "seed": <int>,
          "feature_span_m": <float>,
          "apron_px": <int>,
          "style_key": "fjorded_troughs",   # STYLES[0]
          "core_rows": <int>, "core_cols": <int>,
          # apron-padded world-coord grid rebuilt analytically on both sides:
          #   xs[i] = (i - apron_px) * spacing + ox   for i in 0..padded_cols
          #   zs[j] = (j - apron_px) * spacing + oz   for j in 0..padded_rows
          #   wx[r][c] = xs[c];  wz[r][c] = zs[r]     (numpy meshgrid)
          "grid": {"spacing": <float>, "ox": <float>, "oz": <float>},
          "padded_rows": <int>, "padded_cols": <int>,
          "height": [...]   # core-cropped, row-major, length core_rows*core_cols
        },
        ...
      ]
    }

Run from repo root:  python tools/dem_pack/export_recipe_glacial_fixture.py
Writes:              tools/dem_pack/fixtures/recipe_glacial_fixture.json
"""

import json
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import glacial_synthesis as gs  # noqa: E402

GENERATOR_VERSION = "recipe_fixtures/v1"


def _apron_grid(core_n, feature_span_m, apron_px, ox=0.0, oz=0.0):
    """Build an apron-padded world-coordinate grid for the seam-safe path.

    CORE is core_n x core_n spanning [ox, ox+feature_span_m] in x (likewise z) at
    spacing = feature_span_m / (core_n - 1). APRON extends by apron_px cells per side
    at the SAME spacing, in REAL world coordinates. Purely linear so the Rust port
    rebuilds bit-identical grids from (spacing, ox, oz, apron_px).
    """
    n = int(core_n)
    a = int(apron_px)
    spacing = float(feature_span_m) / max(n - 1, 1)
    total = n + 2 * a
    xs = (np.arange(total, dtype=np.float64) - a) * spacing + float(ox)
    zs = (np.arange(total, dtype=np.float64) - a) * spacing + float(oz)
    wx, wz = np.meshgrid(xs, zs)
    return wx, wz, spacing


def main():
    records = []

    # CORE small so JSON is compact; APRON real (GLACIAL_APRON_PX = 160).
    core_n = 24
    apron_px = gs.GLACIAL_APRON_PX  # 160 -> padded 344x344
    feature_span_m = 90_000.0  # fixed constant shared by adjacent windows
    style = gs.STYLES[0]  # fjorded_troughs (the reference style)

    seeds = (0, 7)  # two seeds so parity is not a one-off coincidence

    ox, oz = 12_000.0, -31_000.0
    for seed in seeds:
        wx, wz, spacing = _apron_grid(
            core_n, feature_span_m, apron_px, ox=ox, oz=oz
        )
        result = gs.generate(
            wx,
            wz,
            seed=seed,
            style=style,
            feature_span_m=feature_span_m,
            apron_px=apron_px,
        )
        height = np.asarray(result["height"], dtype=np.float64)
        assert height.shape == (core_n, core_n), (
            "unexpected core shape {0} (want {1})".format(
                height.shape, (core_n, core_n)
            )
        )
        padded_rows, padded_cols = wx.shape
        records.append({
            "recipe": "glacial_seamsafe",
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
    out_path = os.path.join(out_dir, "recipe_glacial_fixture.json")

    doc = {
        "generator_version": GENERATOR_VERSION,
        "source": (
            "glacial_seamsafe <- tools/dem_pack/glacial_synthesis.py::generate("
            "apron_px=GLACIAL_APRON_PX) seam-safe path; "
            "core-cropped height is the parity oracle for "
            "wg-10/rust/src/recipes_glacial.rs::glacial_seamsafe"
        ),
        "note": (
            "Recipe-level parity oracle (f64). The apron-padded wx/wz grids are "
            "rebuilt analytically from grid={spacing,ox,oz} + apron_px on both "
            "sides (xs[c]=(c-apron_px)*spacing+ox, zs[r]=(r-apron_px)*spacing+oz, "
            "numpy meshgrid); height is the CORE-cropped output. Style = STYLES[0] "
            "(fjorded_troughs)."
        ),
        "records": records,
    }
    with open(out_path, "w", encoding="ascii") as f:
        json.dump(doc, f)  # compact (no indent) -- fields are large

    print("wrote", out_path)
    print("records:", len(records))
    for r in records:
        h = np.asarray(r["height"], dtype=np.float64)
        print(
            "  recipe={0} seed={1} core={2}x{3} padded={4}x{5} "
            "height[min={6:.4f} max={7:.4f} mean={8:.4f}]".format(
                r["recipe"], r["seed"], r["core_rows"], r["core_cols"],
                r["padded_rows"], r["padded_cols"],
                float(h.min()), float(h.max()), float(h.mean()),
            )
        )


if __name__ == "__main__":
    main()
