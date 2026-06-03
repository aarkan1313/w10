r"""Export a 256-core / 576-padded mountain f64 oracle for the production-scale GPU parity gate.
The biome fixtures are 24-core/344-padded (fast exact parity); the RUNTIME producer renders at
256-core/576-padded. A scale-dependent math divergence (audit gap #6) would hide from 344 but show
here. Same recipe (mountain_synthesis.generate, apron_px=160) the Rust port is machine-exact against.

Run:    python tools/dem_pack/export_mountain_576_oracle.py
Writes: wg-10/worldgen_terrain/fixtures/mountain_576_oracle.json
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
import geography_engine as geo
import mountain_synthesis as mountain

OUT = Path(__file__).resolve().parents[2] / "wg-10/worldgen_terrain/fixtures/mountain_576_oracle.json"
CORE_PX = 256
APRON_PX = 160
PADDED = CORE_PX + 2 * APRON_PX        # 576
FEATURE_SPAN_M = 90000.0
SEED = 0
SPACING = FEATURE_SPAN_M / CORE_PX     # production per-px density
OX, OZ = 0.0, 0.0

def main() -> None:
    cell = SPACING
    pad_span = cell * (PADDED - 1)
    pad_ox = OX - APRON_PX * cell
    pad_oz = OZ - APRON_PX * cell
    wx, wz = geo.grid(PADDED, pad_span, ox=pad_ox, oz=pad_oz)
    res = mountain.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M, apron_px=APRON_PX, spacing_m=SPACING)
    h = np.asarray(res["height"], float)          # core-cropped 256x256, normalized pre-relief
    assert h.shape == (CORE_PX, CORE_PX), f"expected ({CORE_PX},{CORE_PX}) got {h.shape}"
    flat = h.reshape(-1)
    rec = {
        "recipe": "mountain_seamsafe", "style_key": "alpine_branching",
        "seed": SEED, "feature_span_m": FEATURE_SPAN_M, "apron_px": APRON_PX,
        "core_rows": CORE_PX, "core_cols": CORE_PX, "padded_rows": PADDED, "padded_cols": PADDED,
        "grid": {"spacing": SPACING, "ox": OX, "oz": OZ},
        "height": flat.tolist(),
    }
    doc = {"generator_version": "recipe_fixtures/v1", "source": "export_mountain_576_oracle.py",
           "note": "256-core/576-padded production-scale mountain oracle for the live-fly parity gate",
           "records": [rec]}
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(doc))
    print(f"wrote {OUT} core={CORE_PX} padded={PADDED} ptp={float(np.ptp(h)):.4f}")

if __name__ == "__main__":
    main()
