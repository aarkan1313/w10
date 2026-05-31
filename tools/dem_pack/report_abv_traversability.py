r"""Tier-1 traversability gate for the A | B | v2 switcher.

Runs the (already-tested) rough-world traversability analyzer over the A|B|v2 review payload
so each variant gets a per-scale verdict: structural grade (flat / diffuse / blocked / thin /
candidate), passable fraction, the largest LOW-corridor fraction, and whether that corridor
actually CROSSES the block (WE/NS). This is the "is it playable?" readout the contact sheets
can't give — it flags, e.g., a too-spiky variant whose slopes leave no crossing route.

It does NOT modify terrain or guarantee routes (that is Tier 3 / Phase 7B); it MEASURES and
GATES, so a variant/tuning without a crossing corridor is visible before any acceptance.

Run:
    python tools/dem_pack/report_abv_traversability.py
Writes:
    D:/tmp/wg10_geography_engine/abv_traversability.{csv,md}
"""

from __future__ import annotations

import json
from pathlib import Path

import analyze_rough_world_traversability as trav

PAYLOAD = Path("wg-10/worldgen_terrain/generated/review/rough_world_abv.json")
OUT_DIR = Path("D:/tmp/wg10_geography_engine")
OUT_CSV = OUT_DIR / "abv_traversability.csv"
OUT_MD = OUT_DIR / "abv_traversability.md"

# The scales worth judging playability at: a near-field play block, a mid, and the wide review.
SCALES = (25.0, 100.0, 200.0)


def main() -> None:
    if not PAYLOAD.exists():
        raise SystemExit(f"missing {PAYLOAD} — run tools/dem_pack/export_godot_rough_world_abv.py first")
    payload = json.loads(PAYLOAD.read_text(encoding="utf-8"))
    # audit_payload sweeps 3 relief_exponent policies per scale; the review scene's actual policy
    # is k=0 (fixed vertical relief as span changes), so report that row for a clean 1-per-(variant,scale).
    rows = [r for r in trav.audit_payload(payload, scales=SCALES) if abs(float(r["relief_exponent"])) < 1e-9]
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    trav.write_csv(rows, path=OUT_CSV)
    trav.write_markdown(rows, path=OUT_MD)
    print(f"wrote {OUT_CSV}")
    print(f"wrote {OUT_MD}")
    # console summary: per variant per scale (k=0 policy), the playability verdict
    print("\nvariant            scale   grade      passable  low-corr  crosses  slope_p90")
    for r in rows:
        crosses = ("WE" if r["largest_low_corridor_crosses_we"] else "") + \
                  ("NS" if r["largest_low_corridor_crosses_ns"] else "")
        print(
            f"{str(r['label'])[:18]:18s} {float(r['scale_x']):5.0f}x  "
            f"{str(r['grade']):9s}  {float(r['passable_frac'])*100:6.1f}%  "
            f"{float(r['largest_low_corridor_frac'])*100:6.1f}%  {crosses or '-':4s}    "
            f"{float(r['slope_p90']):.3f}"
        )


if __name__ == "__main__":
    main()
