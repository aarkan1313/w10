#!/usr/bin/env python3
"""Phase A — tagging review. Join the WG9 user shortlist with WG9's metric-driven
family inferences; emit a reviewable HTML + CSV; seed an approved family map the
USER edits before Phase B. WG9 is read-only. Run from repo root.

  python tools/dem_pack/review_tags.py            # generate review + seed map
  python tools/dem_pack/review_tags.py --reseed   # overwrite an existing approved map
"""
from __future__ import annotations
import argparse
import csv
import html
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import dem_pack_lib as lib  # noqa: E402

WG9 = "D:/workflows/worldgen9"
DEFAULT_SHORTLIST = f"{WG9}/factory/reviews/user_shortlist_kernel_catalog.json"
DEFAULT_INFERENCES = f"{WG9}/factory/catalog/kernel_inferred_tags.json"
KERNELS_DIR = f"{WG9}/factory/kernels"

OUT_HTML = os.path.join(HERE, "dem_tag_review.html")
OUT_CSV = os.path.join(HERE, "dem_tag_review.csv")
OUT_MAP = os.path.join(HERE, "kernel_family_map.approved.json")


def load(path):
    with open(path) as f:
        return json.load(f)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--shortlist", default=DEFAULT_SHORTLIST)
    ap.add_argument("--inferences", default=DEFAULT_INFERENCES)
    ap.add_argument("--threshold", type=float, default=0.7)
    ap.add_argument("--reseed", action="store_true",
                    help="overwrite an existing approved map (else preserve it)")
    args = ap.parse_args()

    shortlist = load(args.shortlist)["kernels"]
    inferences = load(args.inferences)["inferences"]
    inf_by_id = {x["kernel_id"]: x for x in inferences}
    shortlist_ids = [k["kernel_id"] for k in shortlist]

    # rows: shortlist ⋈ inference, with preview path + metrics
    rows = []
    for k in shortlist:
        kid = k["kernel_id"]
        x = inf_by_id.get(kid, {})
        rows.append({
            "kernel_id": kid,
            "shortlist_family": k.get("terrain_family", ""),
            "inferred_family": x.get("inferred_family", ""),
            "confidence": x.get("family_confidence", ""),
            "tag_status": x.get("tag_status", "no_inference"),
            "rationale": " | ".join(x.get("rationale", []) or []),
            "tags": " ".join(x.get("tags", []) or []),
            "height_range_m": k.get("height_range_m", ""),
            "mean_slope_deg": k.get("mean_slope_deg", ""),
            "slope_p95_deg": k.get("slope_p95_deg", ""),
            "coverage_fraction": k.get("coverage_fraction", ""),
            "quality_score": k.get("quality_score", ""),
            "preview": f"{KERNELS_DIR}/{kid}/preview_height.png",
        })

    # CSV (bulk-edit friendly)
    with open(OUT_CSV, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        w.writeheader()
        w.writerows(rows)

    # HTML grouped by inferred_family, sorted by confidence desc
    def conf_key(r):
        try:
            return -float(r["confidence"])
        except (TypeError, ValueError):
            return 1.0
    groups = {}
    for r in rows:
        groups.setdefault(r["inferred_family"] or "(none)", []).append(r)
    parts = ["<!doctype html><meta charset=utf-8><title>DEM tag review</title>",
             "<style>body{font:13px system-ui;margin:1em}img{height:120px;border:1px solid #ccc}"
             "table{border-collapse:collapse}td{border-bottom:1px solid #eee;padding:4px;vertical-align:top}"
             "h2{margin-top:1.5em}.c{color:#888}</style>",
             f"<h1>DEM tag review — {len(rows)} kernels</h1>",
             "<p>Review inferred families. Edit "
             "<code>kernel_family_map.approved.json</code> (or the CSV, then "
             "<code>review_tags.py --from-csv</code> if implemented) before Phase B.</p>"]
    for fam in sorted(groups):
        rs = sorted(groups[fam], key=conf_key)
        parts.append(f"<h2>{html.escape(fam)} <span class=c>({len(rs)})</span></h2><table>")
        for r in rs:
            prev = r["preview"].replace("\\", "/")
            parts.append(
                f"<tr><td><img loading=lazy src='file:///{html.escape(prev)}'></td>"
                f"<td><b>{html.escape(r['kernel_id'])}</b><br>"
                f"conf={html.escape(str(r['confidence']))} status={html.escape(r['tag_status'])}<br>"
                f"<span class=c>shortlist_family={html.escape(r['shortlist_family'])}</span><br>"
                f"range={html.escape(str(r['height_range_m']))}m "
                f"slope50/95={html.escape(str(r['mean_slope_deg']))}/{html.escape(str(r['slope_p95_deg']))}<br>"
                f"<span class=c>{html.escape(r['rationale'])}</span></td></tr>")
        parts.append("</table>")
    with open(OUT_HTML, "w", encoding="utf-8") as f:
        f.write("\n".join(parts))

    # Seed (or preserve) the approved map
    if os.path.exists(OUT_MAP) and not args.reseed:
        print(f"[review] approved map exists, preserved: {OUT_MAP} (use --reseed to overwrite)")
    else:
        seed = lib.seed_family_map(shortlist_ids, inferences, threshold=args.threshold)
        out = {"version": 1, "source_shortlist": args.shortlist,
               "source_inferences": args.inferences, "threshold": args.threshold,
               **seed}
        with open(OUT_MAP, "w") as f:
            json.dump(out, f, indent=1)
            f.write("\n")
        print(f"[review] seeded approved map: {len(seed['map'])} accepted, "
              f"{len(seed['excluded'])} excluded -> {OUT_MAP}")
    print(f"[review] wrote {OUT_HTML} and {OUT_CSV} ({len(rows)} kernels)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
