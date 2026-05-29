# WorldGen10 — Real DEM Pack Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn WG9's 602-kernel user shortlist + its metric-driven family inferences into a real `worldgen10.terrain_pack.v1` (real DEM `normalized_height.npy` kernels) that loads + runs through the unchanged M1/M2 pipeline (loader → grammar → height → GPU parity), with a human tagging-review checkpoint between tagging and pack-build.

**Architecture:** Two Python tools in `tools/dem_pack/`. Phase A (`review_tags.py`) joins the WG9 shortlist with WG9's `kernel_inferred_tags.json`, emits a reviewable HTML+CSV artifact, and seeds an **approved family map** JSON the user edits/approves (the checkpoint). Phase B (`build_pack.py`) consumes the approved map + each kernel's `kernel.json` metadata, emits a `worldgen10.terrain_pack.v1` JSON + copies the chosen `.npy` kernels into the W10 repo, and self-validates. A small **gate-subset** pack drives new headless + GPU-parity gates (real 512×512 kernels — real-scale validation of the M2 atlas). The Rust crate is UNCHANGED — the real pack flows through existing interfaces.

**Tech Stack:** Python 3 + numpy (read `.npy` headers, copy arrays); the existing Rust crate (`pack.rs`/`grammar.rs`/`height.rs`/`gpu_compute.rs`, unchanged); Godot headless (`dem_pack_check.gd`) + windowed (`gpu_parity_dem_check.gd`) gates via `tools/gate.py`.

**Design source:** `docs/superpowers/specs/2026-05-29-real-dem-pack-design.md`. Read it first — especially §2 (NON-NEGOTIABLE: crate unchanged, same schema, WG9 read-only, the approved-map decoupling interface) and §4 (composition rules incl. the FIXED palette remainder rule).

---

## Scope & boundaries

**In scope:** Phase A tagging-review tool + the human checkpoint; Phase B pack-build tool (W10 schema JSON + `.npy` copy + `--validate`); a committed **gate-subset** real pack; new `dem_pack_check.gd` (property) + `gpu_parity_dem_check.gd` (real-pack parity) wired into `gate.py`; DESIGN/ROADMAP/STATUS updates.

**Out of scope (deferred):** the M3 render pipeline; visual tuning of `relief_m`/`footprint_m`; re-curating/re-tagging from scratch (consume WG9's); streaming/committing the FULL ~600MB kernel set (the gate uses a subset; full-set is M3's streaming problem — the full pack is *generated* but committing all of it is an explicit optional step, not gated).

## Interface constraints (from design §2 — enforce while building)
1. **W10 Rust crate UNCHANGED.** Real pack flows through existing `load_pack_dir`/grammar/height/`Wg10GpuCompute`. A needed Rust change = format was wrong; STOP.
2. **Same schema** `worldgen10.terrain_pack.v1` (top keys: `schema`, `version`, `grammar_constants{region_size_m, province_size_regions, palette_primary_pct, palette_compatible_pct, moderation_min, moderation_strength}`, `palettes[{id, families[3]}]`, `compatibility{id:[ids]}`, `families{id:{kernel, relief_m, footprint_m}}`). No schema bump.
3. **WG9 is READ-ONLY.** Tools read `d:/workflows/worldgen9/...`; never write there. All outputs in the W10 repo.
4. **Approved family map is the decoupling interface** (`{kernel_id→family}` + `excluded[]`). Phase B logic is identical regardless of how many kernels/families survive review.
5. **`.npy` binary + validated** (NumPy v1.0 C-order f32, 512×512). `*.npy binary` already pinned in `.gitattributes`.

## WG9 source paths (read-only; confirmed to exist)
- Shortlist: `D:/workflows/worldgen9/factory/reviews/user_shortlist_kernel_catalog.json` — `{kernels:[{kernel_id, terrain_family, sample_px, approx_sample_spacing_m, height_range_m, height_min_m, height_max_m, mean_slope_deg, slope_p50_deg, slope_p95_deg, coverage_fraction, quality_score, ...}]}` (602 entries).
- Inferences: `D:/workflows/worldgen9/factory/catalog/kernel_inferred_tags.json` — `{inferences:[{kernel_id, current_family, inferred_family, family_confidence, tag_status, rationale, tags, metrics}]}` (703 entries).
- Per-kernel dir: `D:/workflows/worldgen9/factory/kernels/<kernel_id>/` — `normalized_height.npy` (512×512 f32), `kernel.json`, `preview_height.png`. **`kernel_id` == dir name** (verified).

## File structure
```
tools/dem_pack/
  review_tags.py                   # Phase A
  build_pack.py                    # Phase B
  dem_pack_lib.py                  # shared pure helpers (join, palette composition, validation) — TESTABLE
  test_dem_pack_lib.py             # pytest unit tests for the pure helpers
  kernel_family_map.approved.json  # the reviewed family map (user-edited; committed)
  README.md                        # run A → review → run B
wg-10/worldgen_terrain/packs/dem_v1/
  terrain_pack.gate.json           # gate subset (committed; small)
  terrain_pack.json                # full pack (generated; committing the .npy is optional/separate)
  kernels/*.npy                    # gate-subset kernels (committed); full set optional
wg-10/worldgen_terrain/tests/
  dem_pack_check.gd                # real-pack property gate (subset)
  gpu_parity_dem_check.gd          # real-pack GPU parity (subset)
tools/gate.py                      # add dem_pack_check (fast) + gpu_parity_dem_check (gpu)
docs/plans/                        # DESIGN §3/§9, ROADMAP, STATUS
```

## Build/run notes (every task)
> **Python:** run from repo root `D:/workflows/worldgen10`. numpy available (2.4.4).
> **Cargo (only if a gate needs a rebuild — it shouldn't, crate is unchanged):** `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo build`.
> **Godot:** `GODOT_BIN="C:/tmp/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64_console.exe"`. `fast` suite headless; `gpu`/dem-parity suite WINDOWED (RenderingDevice null under --headless). gate.py runs the `--import` pass.
> **pytest:** `python -m pytest tools/dem_pack/test_dem_pack_lib.py -v` (from repo root).

---

## Task 0: Shared pure helpers + tests (`dem_pack_lib.py`) (TDD)

**Files:**
- Create: `tools/dem_pack/dem_pack_lib.py`
- Create: `tools/dem_pack/test_dem_pack_lib.py`

The pure, testable core both tools use: the family-map join, palette composition (the FIXED remainder rule), and pack-dict assembly. No file I/O in the tested functions (they take parsed dicts).

- [ ] **Step 1: Write failing tests** — create `tools/dem_pack/test_dem_pack_lib.py`:
```python
import dem_pack_lib as lib


def test_seed_family_map_accepts_high_confidence_suggested():
    inferences = [
        {"kernel_id": "a", "inferred_family": "glacial", "family_confidence": 0.8, "tag_status": "suggested"},
        {"kernel_id": "b", "inferred_family": "mountain", "family_confidence": 0.6, "tag_status": "suggested"},
        {"kernel_id": "c", "inferred_family": "coast", "family_confidence": 0.9, "tag_status": "retained"},
        {"kernel_id": "d", "inferred_family": "desert", "family_confidence": 0.5, "tag_status": "unresolved"},
    ]
    shortlist_ids = ["a", "b", "c", "d"]
    m = lib.seed_family_map(shortlist_ids, inferences, threshold=0.7)
    assert m["map"] == {"a": "glacial", "c": "coast"}      # >=0.7 suggested/retained
    assert set(m["excluded"]) == {"b", "d"}                # below threshold or unresolved


def test_seed_family_map_excludes_kernel_with_no_inference():
    m = lib.seed_family_map(["x"], [], threshold=0.7)
    assert m["map"] == {}
    assert m["excluded"] == ["x"]


def test_compose_palettes_chunks_by_three_same_type():
    # 4 badlands ids -> 2 palettes; last padded by cycling front ids.
    fam_of = {"b1": "badlands", "b2": "badlands", "b3": "badlands", "b4": "badlands"}
    pals = lib.compose_palettes(fam_of)
    # palette 0 = first 3 sorted; palette 1 = [b4, b1, b2] (pad by cycling front)
    ids = sorted(fam_of)  # b1,b2,b3,b4
    assert pals[0]["families"] == ["b1", "b2", "b3"]
    assert pals[1]["families"] == ["b4", "b1", "b2"]
    assert all(len(p["families"]) == 3 for p in pals)


def test_compose_palettes_single_kernel_type_repeats():
    fam_of = {"t1": "tundra"}
    pals = lib.compose_palettes(fam_of)
    one = [p for p in pals if p["id"].startswith("tundra")]
    assert len(one) == 1
    assert one[0]["families"] == ["t1", "t1", "t1"]


def test_compose_palettes_palette_ids_unique_and_deterministic():
    fam_of = {"m1": "mountain", "m2": "mountain", "m3": "mountain", "c1": "coast", "c2": "coast", "c3": "coast"}
    a = lib.compose_palettes(fam_of)
    b = lib.compose_palettes(fam_of)
    assert a == b                                          # deterministic
    pids = [p["id"] for p in a]
    assert len(pids) == len(set(pids))                    # unique ids


def test_build_pack_dict_shape_and_family_fields():
    fam_of = {"m1": "mountain", "m2": "mountain", "m3": "mountain"}
    meta = {
        "m1": {"height_range_m": 1000.0, "approx_sample_spacing_m": 100.0, "sample_px": 512},
        "m2": {"height_range_m": 800.0, "approx_sample_spacing_m": 90.0, "sample_px": 512},
        "m3": {"height_range_m": 1200.0, "approx_sample_spacing_m": 110.0, "sample_px": 512},
    }
    pack = lib.build_pack_dict(fam_of, meta, footprint_scale=1.0)
    assert pack["schema"] == "worldgen10.terrain_pack.v1"
    assert pack["version"] == 1
    assert set(pack["families"]) == {"m1", "m2", "m3"}
    # relief = height_range_m; footprint = spacing * px * scale
    assert pack["families"]["m1"]["relief_m"] == 1000.0
    assert pack["families"]["m1"]["footprint_m"] == 100.0 * 512 * 1.0
    assert pack["families"]["m1"]["kernel"] == "kernels/m1.npy"
    # every palette family resolves to a real family + has 3
    fam_ids = set(pack["families"])
    for p in pack["palettes"]:
        assert len(p["families"]) == 3
        assert all(f in fam_ids for f in p["families"])
    # compatibility references real palette ids
    pal_ids = {p["id"] for p in pack["palettes"]}
    for k, v in pack["compatibility"].items():
        assert k in pal_ids
        assert all(x in pal_ids for x in v)


def test_build_pack_dict_rejects_bad_relief():
    fam_of = {"m1": "mountain", "m2": "mountain", "m3": "mountain"}
    meta = {"m1": {"height_range_m": 0.0, "approx_sample_spacing_m": 100.0, "sample_px": 512},
            "m2": {"height_range_m": 800.0, "approx_sample_spacing_m": 90.0, "sample_px": 512},
            "m3": {"height_range_m": 1200.0, "approx_sample_spacing_m": 110.0, "sample_px": 512}}
    try:
        lib.build_pack_dict(fam_of, meta, footprint_scale=1.0)
        assert False, "should reject relief<=0"
    except ValueError as e:
        assert "relief" in str(e) and "m1" in str(e)
```

- [ ] **Step 2: Run; verify FAIL** — `cd /d/workflows/worldgen10 && python -m pytest tools/dem_pack/test_dem_pack_lib.py -v` → FAIL (module/functions missing). (pytest finds `dem_pack_lib` because the test imports it; run with `cd tools/dem_pack && python -m pytest test_dem_pack_lib.py -v` OR add `import sys; sys.path.insert(0, os.path.dirname(__file__))` — use the cd form to keep imports simple: `cd /d/workflows/worldgen10/tools/dem_pack && python -m pytest test_dem_pack_lib.py -v`.)

- [ ] **Step 3: Implement** — create `tools/dem_pack/dem_pack_lib.py`:
```python
"""Pure helpers for the WorldGen10 real DEM pack tools. No file I/O here — these
take parsed dicts so they are unit-testable. (review_tags.py / build_pack.py do
the I/O.) See docs/superpowers/specs/2026-05-29-real-dem-pack-design.md."""
from __future__ import annotations

SCHEMA = "worldgen10.terrain_pack.v1"
FAMILIES_PER_PALETTE = 3

# grammar_constants carried from the height_pack default (tunable later).
DEFAULT_GRAMMAR_CONSTANTS = {
    "region_size_m": 32768.0,
    "province_size_regions": 4,
    "palette_primary_pct": 72,
    "palette_compatible_pct": 22,
    "moderation_min": 0.4,
    "moderation_strength": 0.5,
}


def seed_family_map(shortlist_ids, inferences, threshold=0.7):
    """Seed an approved family map from WG9 inferences. retained/suggested with
    confidence >= threshold are accepted to inferred_family; everything else
    (low-confidence, unresolved, or no inference) is excluded. The USER then
    edits this before Phase B."""
    inf_by_id = {x["kernel_id"]: x for x in inferences}
    accepted = {}
    excluded = []
    for kid in shortlist_ids:
        x = inf_by_id.get(kid)
        if x is None:
            excluded.append(kid)
            continue
        status = x.get("tag_status")
        conf = float(x.get("family_confidence") or 0.0)
        fam = x.get("inferred_family")
        if status in ("retained", "suggested") and conf >= threshold and fam and fam != "uncategorized":
            accepted[kid] = fam
        else:
            excluded.append(kid)
    return {"map": accepted, "excluded": excluded}


def compose_palettes(fam_of):
    """Compose exactly-3-family palettes from {kernel_id -> family}. Group by
    family type; within a type sort ids lexicographically and chunk by 3; pad the
    last chunk by cycling the type's earliest ids (so every palette is same-type,
    exactly 3). Deterministic. Returns [{id, families:[3]}]."""
    by_fam = {}
    for kid, fam in fam_of.items():
        by_fam.setdefault(fam, []).append(kid)
    palettes = []
    for fam in sorted(by_fam):
        ids = sorted(by_fam[fam])
        n = len(ids)
        # chunk into groups of 3, padding the final group by cycling from front.
        idx = 0
        chunk_no = 0
        while idx < n:
            group = ids[idx:idx + FAMILIES_PER_PALETTE]
            i = 0
            while len(group) < FAMILIES_PER_PALETTE:
                group.append(ids[i % n])
                i += 1
            palettes.append({"id": f"{fam}_{chunk_no}", "families": group})
            idx += FAMILIES_PER_PALETTE
            chunk_no += 1
    return palettes


def _compose_compatibility(palettes):
    """Each palette is compatible with the other palettes of the same terrain
    type, plus one default cross-type neighbor (the next palette in sorted order,
    cyclic) so the grammar's compatible-roll always resolves."""
    pid_order = [p["id"] for p in palettes]
    type_of = {p["id"]: p["id"].rsplit("_", 1)[0] for p in palettes}
    compat = {}
    for i, p in enumerate(palettes):
        pid = p["id"]
        same = [q for q in pid_order if q != pid and type_of[q] == type_of[pid]]
        cross = pid_order[(i + 1) % len(pid_order)]
        lst = same[:]
        if cross != pid and cross not in lst:
            lst.append(cross)
        compat[pid] = lst
    return compat


def build_pack_dict(fam_of, meta, footprint_scale=1.0):
    """Assemble a worldgen10.terrain_pack.v1 dict from {kernel_id->family} +
    {kernel_id->kernel.json metadata}. relief_m=height_range_m;
    footprint_m=approx_sample_spacing_m*sample_px*footprint_scale. Raises
    ValueError naming the offending kernel on bad metadata."""
    families = {}
    for kid, fam in fam_of.items():
        m = meta.get(kid)
        if m is None:
            raise ValueError(f"kernel {kid!r}: no metadata")
        relief = float(m.get("height_range_m") or 0.0)
        spacing = float(m.get("approx_sample_spacing_m") or 0.0)
        px = int(m.get("sample_px") or 0)
        if relief <= 0.0:
            raise ValueError(f"kernel {kid!r}: relief (height_range_m) must be > 0, got {relief}")
        if spacing <= 0.0 or px <= 0:
            raise ValueError(f"kernel {kid!r}: footprint inputs must be > 0 (spacing={spacing}, px={px})")
        families[kid] = {
            "kernel": f"kernels/{kid}.npy",
            "relief_m": relief,
            "footprint_m": spacing * px * footprint_scale,
        }
    palettes = compose_palettes(fam_of)
    if not palettes:
        raise ValueError("no palettes composed (empty family map)")
    compatibility = _compose_compatibility(palettes)
    return {
        "schema": SCHEMA,
        "version": 1,
        "grammar_constants": dict(DEFAULT_GRAMMAR_CONSTANTS),
        "palettes": palettes,
        "compatibility": compatibility,
        "families": families,
    }
```

- [ ] **Step 4: Run; verify PASS** — `cd /d/workflows/worldgen10/tools/dem_pack && python -m pytest test_dem_pack_lib.py -v` → all pass (8 tests).

- [ ] **Step 5: Commit**
```bash
cd /d/workflows/worldgen10
git add tools/dem_pack/dem_pack_lib.py tools/dem_pack/test_dem_pack_lib.py
git commit -m "feat(dem-pack): pure helpers (family-map seed, palette compose, pack assembly) + tests"
```

---

## Task 1: Phase A — tagging review tool (`review_tags.py`)

**Files:**
- Create: `tools/dem_pack/review_tags.py`
- Create: `tools/dem_pack/README.md`

Reads the WG9 shortlist + inferences (read-only), emits `dem_tag_review.html` + `dem_tag_review.csv`, and seeds `kernel_family_map.approved.json` via `dem_pack_lib.seed_family_map`. Idempotent.

- [ ] **Step 1: Implement `review_tags.py`** — create `tools/dem_pack/review_tags.py`:
```python
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
```

- [ ] **Step 2: Write the README** — create `tools/dem_pack/README.md`:
```markdown
# WorldGen10 real DEM pack tools

Two-phase: review tags (human checkpoint), then build the pack.

## Phase A — review tags
    python tools/dem_pack/review_tags.py
Open `dem_tag_review.html` in a browser; review the inferred families.
Edit `kernel_family_map.approved.json` (the source of truth): the `map`
(`{kernel_id: family}`) is what gets built; move ids to/from `excluded` as needed.
`--reseed` regenerates the seed (overwriting your edits); omit it to preserve.

## Phase B — build the pack (after you approve the map)
    python tools/dem_pack/build_pack.py --validate          # full pack
    python tools/dem_pack/build_pack.py --gate-subset 24 --validate   # gate subset
Writes `wg-10/worldgen_terrain/packs/dem_v1/` (terrain_pack.json + kernels/*.npy).

WG9 (`D:/workflows/worldgen9`) is read-only; all outputs land in this repo.
```

- [ ] **Step 3: Run it; verify the artifacts** — 
```bash
cd /d/workflows/worldgen10
python tools/dem_pack/review_tags.py
```
Expected: prints `[review] seeded approved map: N accepted, M excluded` and `[review] wrote ... (602 kernels)`. Confirm the three files exist:
```bash
ls -la tools/dem_pack/dem_tag_review.html tools/dem_pack/dem_tag_review.csv tools/dem_pack/kernel_family_map.approved.json
python -c "import json; m=json.load(open('tools/dem_pack/kernel_family_map.approved.json')); print('accepted', len(m['map']), 'excluded', len(m['excluded']), 'total', len(m['map'])+len(m['excluded']))"
```
Expected: total == 602; accepted ≈ the count of retained/suggested ≥0.7 tagged kernels.

- [ ] **Step 4: Commit the tool (NOT the generated review artifacts yet — the approved map is committed at the checkpoint in Task 2)**
```bash
cd /d/workflows/worldgen10
git add tools/dem_pack/review_tags.py tools/dem_pack/README.md
git commit -m "feat(dem-pack): Phase A tagging-review tool (HTML+CSV review, seed approved map)"
```

---

## Task 2: HUMAN CHECKPOINT — review tags & approve the family map

**Files:**
- Modify (by the USER): `tools/dem_pack/kernel_family_map.approved.json`
- Commit: the approved map

**This task is a PAUSE for the human partner.** The executing agent must STOP here and hand control to the user — do not auto-proceed to Phase B with the seeded-but-unreviewed map.

- [ ] **Step 1: Surface the review to the user.** Report: the seeded counts (accepted/excluded), the path to `dem_tag_review.html` (open in a browser), and the family distribution of the accepted map:
```bash
cd /d/workflows/worldgen10
python -c "
import json, collections
m=json.load(open('tools/dem_pack/kernel_family_map.approved.json'))
print('accepted:', len(m['map']), ' excluded:', len(m['excluded']))
print('accepted family distribution:')
for f,c in collections.Counter(m['map'].values()).most_common(): print(f'   {c:4d}  {f}')
print('review HTML: tools/dem_pack/dem_tag_review.html (open in browser)')
"
```

- [ ] **Step 2: WAIT for the user.** The user reviews the HTML, edits `kernel_family_map.approved.json` (moves ids between `map` and `excluded`, fixes wrong families), and confirms it reflects their intent. The agent does NOT continue until the user explicitly says the map is approved.

- [ ] **Step 3: Commit the approved map** (after user approval):
```bash
cd /d/workflows/worldgen10
git add tools/dem_pack/kernel_family_map.approved.json
git commit -m "chore(dem-pack): approved kernel family map (human-reviewed)"
```

---

## Task 3: Phase B — pack build tool (`build_pack.py`)

**Files:**
- Create: `tools/dem_pack/build_pack.py`

Consumes the approved map + each kernel's `kernel.json`, emits the W10 pack JSON + copies `.npy` kernels, self-validates. Reuses `dem_pack_lib`.

- [ ] **Step 1: Implement `build_pack.py`** — create `tools/dem_pack/build_pack.py`:
```python
#!/usr/bin/env python3
"""Phase B — build the real DEM terrain pack from the approved family map.
Emits wg-10/worldgen_terrain/packs/dem_v1/{terrain_pack.json, kernels/*.npy}.
WG9 is read-only. Run from repo root AFTER approving the family map.

  python tools/dem_pack/build_pack.py --validate
  python tools/dem_pack/build_pack.py --gate-subset 24 --validate
"""
from __future__ import annotations
import argparse
import json
import os
import shutil
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import dem_pack_lib as lib  # noqa: E402

REPO = os.path.dirname(os.path.dirname(HERE))  # tools/dem_pack -> repo root
WG9_KERNELS = "D:/workflows/worldgen9/factory/kernels"
MAP_PATH = os.path.join(HERE, "kernel_family_map.approved.json")
OUT_DIR = os.path.join(REPO, "wg-10", "worldgen_terrain", "packs", "dem_v1")


def load_meta(kernel_id):
    with open(f"{WG9_KERNELS}/{kernel_id}/kernel.json") as f:
        return json.load(f)


def validate_npy(path):
    """Confirm a .npy parses as a 512x512 (or NxN) C-order float32 array."""
    a = np.load(path, mmap_mode="r")
    if a.dtype != np.dtype("<f4") and a.dtype != np.float32:
        raise ValueError(f"{path}: dtype {a.dtype} not float32")
    if a.ndim != 2:
        raise ValueError(f"{path}: ndim {a.ndim} not 2")
    return a.shape


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--validate", action="store_true")
    ap.add_argument("--gate-subset", type=int, default=0,
                    help="if >0, build a small subset pack (terrain_pack.gate.json) "
                         "with N kernels spread across families, for the gates")
    ap.add_argument("--footprint-scale", type=float, default=1.0)
    args = ap.parse_args()

    approved = json.load(open(MAP_PATH))
    fam_of_full = dict(approved["map"])
    if not fam_of_full:
        raise SystemExit("[build] approved map is empty — review/approve it first (Phase A)")

    # subset: take up to ceil(N/num_families) per family, deterministic by sorted id
    if args.gate_subset > 0:
        by_fam = {}
        for kid, fam in sorted(fam_of_full.items()):
            by_fam.setdefault(fam, []).append(kid)
        per = max(1, -(-args.gate_subset // len(by_fam)))  # ceil
        fam_of = {}
        for fam in sorted(by_fam):
            for kid in by_fam[fam][:per]:
                fam_of[kid] = fam
        out_json = "terrain_pack.gate.json"
    else:
        fam_of = fam_of_full
        out_json = "terrain_pack.json"

    meta = {kid: load_meta(kid) for kid in fam_of}
    pack = lib.build_pack_dict(fam_of, meta, footprint_scale=args.footprint_scale)

    os.makedirs(os.path.join(OUT_DIR, "kernels"), exist_ok=True)
    # copy kernels
    copied = 0
    for kid in fam_of:
        src = f"{WG9_KERNELS}/{kid}/normalized_height.npy"
        dst = os.path.join(OUT_DIR, "kernels", f"{kid}.npy")
        shutil.copyfile(src, dst)
        copied += 1
    with open(os.path.join(OUT_DIR, out_json), "w") as f:
        json.dump(pack, f, indent=1)
        f.write("\n")

    if args.validate:
        # every family's .npy exists + parses; every palette family resolves
        fam_ids = set(pack["families"])
        for kid, fam in pack["families"].items():
            shape = validate_npy(os.path.join(OUT_DIR, fam["kernel"].replace("/", os.sep)))
            if shape[0] != shape[1]:
                raise ValueError(f"{kid}: non-square kernel {shape}")
        for p in pack["palettes"]:
            if len(p["families"]) != lib.FAMILIES_PER_PALETTE:
                raise ValueError(f"palette {p['id']}: not {lib.FAMILIES_PER_PALETTE} families")
            for fid in p["families"]:
                if fid not in fam_ids:
                    raise ValueError(f"palette {p['id']}: family {fid} not in families")
        print(f"[build] validate OK: {len(fam_ids)} families, {len(pack['palettes'])} palettes")

    print(f"[build] wrote {out_json} ({len(fam_of)} kernels copied) -> {OUT_DIR}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 2: Build the GATE SUBSET pack + validate** —
```bash
cd /d/workflows/worldgen10
python tools/dem_pack/build_pack.py --gate-subset 24 --validate
```
Expected: `[build] validate OK: K families, P palettes` then `[build] wrote terrain_pack.gate.json (K kernels copied)`. Confirm:
```bash
python -c "import json; p=json.load(open('wg-10/worldgen_terrain/packs/dem_v1/terrain_pack.gate.json')); print('schema',p['schema'],'families',len(p['families']),'palettes',len(p['palettes']))"
ls wg-10/worldgen_terrain/packs/dem_v1/kernels/ | wc -l
```

- [ ] **Step 3: Commit the tool + the GATE-SUBSET pack (the small one — gate kernels + json).**
```bash
cd /d/workflows/worldgen10
git add tools/dem_pack/build_pack.py wg-10/worldgen_terrain/packs/dem_v1/terrain_pack.gate.json wg-10/worldgen_terrain/packs/dem_v1/kernels/
git commit -m "feat(dem-pack): Phase B build tool + committed gate-subset real DEM pack"
```
(The FULL pack — `terrain_pack.json` + its larger kernel set — is generated on demand; committing the full ~600MB is a separate optional decision, NOT done here.)

---

## Task 4: Real-pack property gate (`dem_pack_check.gd`)

**Files:**
- Create: `wg-10/worldgen_terrain/tests/dem_pack_check.gd`

Loads the gate-subset real pack through `Wg10Height`, asserts height finite / bounded by max relief / deterministic / varied. Mirrors `height_check.gd` but points at the DEM pack + a relief bound read from the pack.

- [ ] **Step 1: Write the check** — create `wg-10/worldgen_terrain/tests/dem_pack_check.gd` (TABS):
```gdscript
extends SceneTree

# Real DEM pack property check (gate subset) through the native Wg10Height lib.
# finite, bounded, deterministic, varied — NOT visual. Same family of properties
# as height_check.gd, but on real 512x512 DEM kernels.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10Height"):
		push_error("Wg10Height native class not registered")
		return 1
	var os_dir: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var h: Object = ClassDB.instantiate("Wg10Height")
	var err: String = str(h.call("load_pack_dir", os_dir, PACK_FILE))
	if err != "":
		push_error("dem pack load failed: %s" % err)
		return 1
	# max relief across the pack's families (bound for height) — read the json.
	var f := FileAccess.open(PACK_RES_DIR + "/" + PACK_FILE, FileAccess.READ)
	if f == null:
		push_error("cannot read pack json")
		return 1
	var pack: Dictionary = JSON.parse_string(f.get_as_text())
	var max_relief := 0.0
	for fid in pack["families"]:
		max_relief = maxf(max_relief, float(pack["families"][fid]["relief_m"]))

	var errors: Array[String] = []
	var coords := [Vector2(0, 0), Vector2(-1024.5, 2048.25), Vector2(1e6, -1e6), Vector2(40000.0, 9000.0)]
	for c in coords:
		var v: float = h.call("height", c.x, c.y, 1337)
		if not is_finite(v):
			errors.append("non-finite @ %s: %s" % [str(c), str(v)])
		if v < -1.0 or v > max_relief + 1.0:
			errors.append("out of bounds @ %s: %s (max_relief %s)" % [str(c), str(v), str(max_relief)])
		var v2: float = h.call("height", c.x, c.y, 1337)
		if v != v2:
			errors.append("non-deterministic @ %s" % str(c))

	var seen := {}
	for ix in range(-8, 8):
		for iz in range(-8, 8):
			var hv: float = h.call("height", float(ix) * 40000.0, float(iz) * 40000.0, 1337)
			seen[snappedf(hv, 0.01)] = true
	if seen.size() < 2:
		errors.append("height variety collapsed: %d" % seen.size())

	if not errors.is_empty():
		for e in errors: push_error(e)
		print("[wg10-dem-pack] status=fail errors=%d" % errors.size())
		return 1
	print("[wg10-dem-pack] status=pass coords=%d variety=%d max_relief=%s" % [coords.size(), seen.size(), str(max_relief)])
	return 0
```

- [ ] **Step 2: Import + run (headless — height is CPU, no GPU needed)** —
```bash
export GODOT_BIN="C:/tmp/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64_console.exe"
"$GODOT_BIN" --headless --import --path "D:/workflows/worldgen10/wg-10" 2>&1 | tail -2
"$GODOT_BIN" --headless --path "D:/workflows/worldgen10/wg-10" --script "res://worldgen_terrain/tests/dem_pack_check.gd"; echo "RC=$?"
```
Expected: `[wg10-dem-pack] status=pass coords=4 variety=N max_relief=...`, RC=0. If load fails, the error string says why (path / kernel-less family / bad relief) — debug, don't fake.

- [ ] **Step 3: Commit** (+ .uid sidecar if generated)
```bash
cd /d/workflows/worldgen10
git add wg-10/worldgen_terrain/tests/dem_pack_check.gd
git add wg-10/worldgen_terrain/tests/dem_pack_check.gd.uid 2>/dev/null || true
git commit -m "test: real DEM pack property gate (finite, bounded, deterministic, varied)"
```

---

## Task 5: Real-pack GPU parity gate (`gpu_parity_dem_check.gd`)

**Files:**
- Create: `wg-10/worldgen_terrain/tests/gpu_parity_dem_check.gd`

Same two-tier CPU/GPU parity as `gpu_parity_check.gd`, but on the real DEM gate-subset pack — the real-scale validation of the M2 kernel atlas (512×512 kernels). Windowed.

- [ ] **Step 1: Write the check** — create `wg-10/worldgen_terrain/tests/gpu_parity_dem_check.gd` (TABS) — identical structure to `gpu_parity_check.gd` but pointing at the DEM pack:
```gdscript
extends SceneTree

# CPU/GPU parity on the REAL DEM pack (gate subset) — real-scale validation of
# the M2 kernel atlas. Tier 1 family signatures EXACT, Tier 2 height within f32
# epsilon. Windowed (RenderingDevice compute needs a device).

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const SHADER_RES := "res://worldgen_terrain/shaders/height_field.glsl"
const ABS_EPS := 1.0e-2
const REL_EPS := 1.0e-5

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10Height") or not ClassDB.class_exists("Wg10GpuCompute"):
		push_error("native classes not registered")
		return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-gpu-parity-dem] status=skip reason=no-gpu")
		return 2
	probe.free()
	var os_dir: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var os_glsl: String = ProjectSettings.globalize_path(SHADER_RES)
	var cpu: Object = ClassDB.instantiate("Wg10Height")
	var gpu: Object = ClassDB.instantiate("Wg10GpuCompute")
	var e1: String = str(cpu.call("load_pack_dir", os_dir, PACK_FILE))
	var e2: String = str(gpu.call("load_pack_dir", os_dir, PACK_FILE, os_glsl))
	if e1 != "" or e2 != "":
		push_error("pack load failed: cpu=%s gpu=%s" % [e1, e2])
		return 1

	var xs := PackedFloat64Array(); var zs := PackedFloat64Array()
	for ix in range(-12, 12):
		for iz in range(-12, 12):
			xs.append(float(ix) * 12345.0 + 17.0)
			zs.append(float(iz) * 9876.0 - 31.0)
	var n := xs.size()
	var gpu_h: PackedFloat64Array = gpu.call("heights", xs, zs, 1337)
	var gpu_s: PackedInt64Array = gpu.call("signatures", xs, zs, 1337)
	if gpu_h.size() != n or gpu_s.size() != n:
		push_error("gpu output size mismatch")
		return 1

	# max relief for the relative tolerance term
	var f := FileAccess.open(PACK_RES_DIR + "/" + PACK_FILE, FileAccess.READ)
	var pack: Dictionary = JSON.parse_string(f.get_as_text())
	var max_relief := 1.0
	for fid in pack["families"]:
		max_relief = maxf(max_relief, float(pack["families"][fid]["relief_m"]))

	var sig_mismatch := 0; var height_fail := 0; var max_dh := 0.0
	for i in range(n):
		var cs: int = cpu.call("family_signature", xs[i], zs[i], 1337)
		if cs != gpu_s[i]:
			sig_mismatch += 1
			if sig_mismatch <= 3:
				push_error("Tier1 sig mismatch @ (%s,%s): cpu=%d gpu=%d" % [str(xs[i]), str(zs[i]), cs, gpu_s[i]])
		var ch: float = cpu.call("height", xs[i], zs[i], 1337)
		var dh: float = absf(ch - float(gpu_h[i]))
		if dh > max_dh: max_dh = dh
		var tol := maxf(ABS_EPS, REL_EPS * max_relief)
		if dh > tol:
			height_fail += 1
			if height_fail <= 3:
				push_error("Tier2 height delta @ (%s,%s): d=%s tol=%s" % [str(xs[i]), str(zs[i]), str(dh), str(tol)])

	if sig_mismatch > 0 or height_fail > 0:
		print("[wg10-gpu-parity-dem] status=fail sig_mismatch=%d height_fail=%d maxd=%s" % [sig_mismatch, height_fail, str(max_dh)])
		return 1
	print("[wg10-gpu-parity-dem] status=pass coords=%d families_exact=true maxd=%s" % [n, str(max_dh)])
	return 0
```
NOTE: the Tier-2 tolerance uses `REL_EPS * max_relief` (real relief can be ~2000m, so the relative term matters more than in the synthetic pack where relief was 1000). If Tier-2 fails with a delta just over tol, REPORT the maxd + tol — do NOT loosen blindly (DESIGN: epsilon widened only if profiled). A Tier-1 mismatch is a real bug (same as M2) — report cpu vs gpu sigs.

- [ ] **Step 2: Import + run WINDOWED** —
```bash
export GODOT_BIN="C:/tmp/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64_console.exe"
"$GODOT_BIN" --headless --import --path "D:/workflows/worldgen10/wg-10" 2>&1 | tail -2
"$GODOT_BIN" --path "D:/workflows/worldgen10/wg-10" --script "res://worldgen_terrain/tests/gpu_parity_dem_check.gd"; echo "RC=$?"
```
Expected: `[wg10-gpu-parity-dem] status=pass coords=576 families_exact=true maxd=<small>`, RC=0.

- [ ] **Step 3: Commit** (+ .uid)
```bash
cd /d/workflows/worldgen10
git add wg-10/worldgen_terrain/tests/gpu_parity_dem_check.gd
git add wg-10/worldgen_terrain/tests/gpu_parity_dem_check.gd.uid 2>/dev/null || true
git commit -m "test: real DEM pack GPU parity gate (M2 atlas at real 512x512 scale)"
```

---

## Task 6: Wire the DEM gates into the runner

**Files:**
- Modify: `tools/gate.py`

- [ ] **Step 1: Add the DEM checks.** In `tools/gate.py`, add `dem_pack_check.gd` to the `"fast"` list (it's headless CPU) and `gpu_parity_dem_check.gd` to the `"gpu"` list:
```python
CHECKS = {
    "fast": [
        "worldgen_terrain/tests/hash_parity_check.gd",
        "worldgen_terrain/tests/determinism_check.gd",
        "worldgen_terrain/tests/grammar_check.gd",
        "worldgen_terrain/tests/height_check.gd",
        "worldgen_terrain/tests/dem_pack_check.gd",
    ],
    "gpu": [
        "worldgen_terrain/tests/gpu_parity_check.gd",
        "worldgen_terrain/tests/gpu_parity_dem_check.gd",
    ],
}
```

- [ ] **Step 2: Run both suites** —
```bash
export GODOT_BIN="C:/tmp/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64_console.exe"
cd /d/workflows/worldgen10
python tools/gate.py --suite fast; echo "FAST_EXIT=$?"
python tools/gate.py --suite gpu 2>&1 | grep -E "gate\]|status=|maxd"; echo "GPU_EXIT=$?"
```
Expected: fast → 5 checks fail=0; gpu → 2 checks fail=0 (both parity packs pass).

- [ ] **Step 3: Commit**
```bash
cd /d/workflows/worldgen10
git add tools/gate.py
git commit -m "feat: add real DEM pack checks to fast + gpu gate suites"
```

---

## Task 7: Update the living docs

**Files:**
- Modify: `docs/plans/DESIGN.md` (§3 + §9), `docs/plans/ROADMAP.md`, `docs/plans/STATUS.md`

- [ ] **Step 1: DESIGN §3** — record that the FIRST REAL DEM pack exists (`wg-10/worldgen_terrain/packs/dem_v1/`), built by `tools/dem_pack/` from WG9's 602-kernel user shortlist + WG9's reviewed family inferences (human-approved map). Families = one-per-kernel grouped into 3-family palettes by terrain type; `relief_m`=`height_range_m`, `footprint_m`=real ground extent (`spacing×px`, `footprint_scale` knob for M3). `normalized_height.npy` (512×512 f32). The Rust crate is unchanged — real pack flows through M1/M2 interfaces. A committed gate-subset drives the property + GPU-parity gates; the full ~600MB set is generated on demand (not all committed).

- [ ] **Step 2: DESIGN §9** — note: **footprint/relief visual tuning** deferred to M3 (`footprint_scale`); **GPU parity now validated on real 512×512 kernels** (the M2 kernel-atlas-at-scale risk is exercised — uniform 512×512, no redesign needed); **pack size** reality (full set ~600MB; gate uses a subset; committing the full set is deferred); **tagging accuracy** rests on the human-reviewed inference map (mis-tags surface as odd terrain in M3, re-review then).

- [ ] **Step 3: ROADMAP** — under Milestone 1, mark **"Terrain-pack format defined and loadable (first pack = DEM/OpenTopo kernels)"** as `[x]` — the format was M1; the real DEM pack is now wired + gated. Note the full-set streaming + visual tuning are M3. Update "Last updated:" to 2026-05-29 with a short note (real DEM pack wired; gates green).

- [ ] **Step 4: STATUS** — bump "Last updated:" to 2026-05-29. Current state: the first REAL DEM terrain pack is wired (`packs/dem_v1`, from WG9's 602-shortlist + human-approved family map via `tools/dem_pack/`); loads through the unchanged M1/M2 pipeline; real-512×512 kernels validated by a property gate + GPU parity gate (Tier-1 exact, Tier-2 epsilon). What works: add the DEM property + DEM GPU-parity gates; note fast=5 checks, gpu=2 checks, fail=0; note the cargo test count unchanged (crate untouched). What's next: M3 render pipeline (consumes the real pack's GPU height pages, no readback); full-set streaming + visual relief/footprint tuning happen there. Honest-baseline: nothing rendered yet; gate uses a kernel subset; relief/footprint are physically-derived, not visually tuned.

- [ ] **Step 5: Commit**
```bash
cd /d/workflows/worldgen10
git add docs/plans/DESIGN.md docs/plans/ROADMAP.md docs/plans/STATUS.md
git commit -m "docs: first real DEM pack wired (WG9 shortlist + approved tags); gates green"
```

---

## Self-review notes (already applied)
- **Spec coverage:** §1 two-phase (Task 1 A + Task 3 B), §2 constraints (crate unchanged — no Rust task; same schema — Task 0 build_pack_dict emits exact keys; WG9 read-only — tools only read; approved-map interface — Task 2 checkpoint; .npy validated — Task 3 --validate), §3 review tool (Task 1), §4 composition incl. FIXED palette remainder rule (Task 0 compose_palettes), §5 gates (Tasks 4-6), §6 layout, §7 DESIGN updates (Task 7).
- **The human checkpoint (Task 2) is an explicit PAUSE** — the executing agent stops and hands to the user; Phase B (Task 3) consumes only the approved map.
- **Crate-unchanged enforced:** no task edits Rust. The real pack flows through existing `load_pack_dir`/grammar/height/`Wg10GpuCompute`. If a gate fails because the loader rejects the pack, that's a DATA problem (fix the pack/tool), not a crate change.
- **Type/interface consistency:** `seed_family_map`/`compose_palettes`/`build_pack_dict` signatures match across Task 0 tests + Task 1/3 callers; the pack dict keys match the exact `worldgen10.terrain_pack.v1` schema M2's loader reads; the gates call `Wg10Height.load_pack_dir/height/family_signature` + `Wg10GpuCompute.load_pack_dir(dir,file,glsl)/heights/signatures` exactly as M2 exposed them; gate-subset pack file `terrain_pack.gate.json` referenced consistently by Tasks 3/4/5.
- **Gate uses the SUBSET** (`terrain_pack.gate.json`, ~24 kernels) so the gates are fast + the committed `.npy` set is small; the full pack is opt-in.
- **Epsilon discipline:** Tier-2 uses `REL_EPS*max_relief` (real relief ~2000m); do not loosen to force a pass (stated in Task 5).
- **Determinism:** `compose_palettes`/`seed_family_map`/subset selection all sort by id — deterministic, reproducible packs.
```
