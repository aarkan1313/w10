# WorldGen10 — Real DEM Pack Design

**Date:** 2026-05-29
**Status:** Approved (brainstorming) — pending writing-plans
**Layer:** Real terrain-pack data (replaces synthetic flat/ramp kernels) via WG9's curated DEM kernels
**Builds on:** M1 pack/grammar/height + M2 GPU formula+parity (all consume `worldgen10.terrain_pack.v1` unchanged)
**Followed by:** M3 render pipeline (consumes the real pack's GPU height pages)

> **ERRATUM (2026-05-31):** this historical spec incorrectly paired `normalized_height.npy`
> z-score kernels with `relief_m = height_range_m`. Validated source audit shows the old CPU/GPU path
> over-amplifies peak-to-peak relief by the z-score span (3.97–11.16×, median 5.56× in the shipped gate
> pack). If normalized kernels are used again, metres conversion is `z × height_std_m`, or the kernels must
> be rebaked to a documented bounded range before multiplying by a range scalar.

---

## 0. Framing

M1/M2 built the whole formula (pack loader → grammar → height → GPU parity) but
validated it only against **synthetic** toy kernels (flat=0.5, ramp 0..1, 4×4).
This plan wires the **real DEM kernels** WG9 already produced and curated into a
`worldgen10.terrain_pack.v1` that flows through the *unchanged* M1/M2 interfaces.

**WG9 already did the hard data work** (consumed, not redone here):
- `factory/reviews/user_shortlist_kernel_catalog.json` — 602 user-shortlisted
  kernels, each with full metadata (`height_range_m`, `approx_sample_spacing_m`,
  `sample_px`, slope/curvature/coverage/quality stats, `terrain_family`).
- `factory/catalog/kernel_inferred_tags.json` — 703 metric-driven family
  inferences (`inferred_family`, `family_confidence`, `tag_status` ∈
  {retained, suggested, unresolved}, `rationale`, `tags`). 112 retained, 265
  suggested, 326 unresolved.
- Per-kernel `factory/kernels/<id>/`: `normalized_height.npy` (512×512 f32,
  per-kernel mean/std normalized), `height_m.npy`, `residual_m.npy`,
  `kernel.json` (the metadata block), preview PNGs.

The W10 Rust crate (`pack.rs`/`grammar.rs`/`height.rs`/`gpu_compute.rs`) is
**unchanged** — if any Rust change is needed, the pack format was under-specified;
stop and reconsider.

---

## 1. Scope

**In scope:** a two-phase pipeline, one spec, with a human-review checkpoint:
- **Phase A — tagging review → approved family map:** a tool joins the 602
  shortlist with WG9's inferences, emits a reviewable artifact (HTML w/ preview
  thumbnails + CSV), and seeds an **approved family map** the user reviews/edits.
- **Phase B — deterministic pack build + wiring:** a tool consumes the approved
  map + each kernel's `kernel.json`, emits a `worldgen10.terrain_pack.v1` JSON +
  copies the chosen `normalized_height.npy` kernels into the W10 repo, validates
  it, and proves it loads + runs through the existing pipeline + gates (incl. GPU
  parity on the real 512×512 pack — real-scale validation of the M2 atlas).

**Out of scope (deferred):**
- The M3 render pipeline (page pool / scheduler / clipmap / fly-test). The real
  pack is validated by the gates (incl. one-off GPU readback), NOT yet streamed
  or rendered.
- **Visual tuning of `relief_m`/`footprint_m`** — physically-derived defaults
  now; visual tuning needs the renderer (M3). Honest-baseline: don't tune blind.
- Re-curating / re-tagging from scratch — WG9's shortlist + inferences are the
  source; we review them, not rebuild them.
- Streaming the FULL kernel set at scale — if the approved set is large, the
  parity/property gates use a representative subset; full-set streaming is M3.

---

## 2. Interface constraints — NON-NEGOTIABLE

1. **The W10 Rust crate is unchanged.** The real pack must flow through the
   existing `pack::load_pack_dir` → `grammar` → `height` → `Wg10GpuCompute`
   interfaces with zero code changes. The pack is *data*. (A needed Rust change =
   the format was wrong; stop.)
2. **Same pack schema.** The output is `worldgen10.terrain_pack.v1` — the exact
   schema M2's loader validates (families with `kernel`/`relief_m`/`footprint_m`,
   `grammar_constants` incl. `moderation_*`, palettes of exactly
   `FAMILIES_PER_PALETTE = 3` families, compatibility). No schema bump.
3. **WG9 is read-only.** The tools READ WG9's catalogs + kernel files; they never
   write into `d:/workflows/worldgen9`. All outputs land in the W10 repo.
4. **The approved family map is the decoupling interface.** Phase B reads a
   stable `{kernel_id → family}` + `excluded[]` JSON. B's build logic is identical
   regardless of how many kernels/families the review approves — only the *data*
   is determined at the checkpoint.
5. **`.npy` kept binary, validated.** Copied kernels are real NumPy v1.0 C-order
   f32 (the existing reader's format); pinned `*.npy binary` in `.gitattributes`
   (already done). Every referenced `.npy` validated to parse as 512×512 f32
   before Godot loads it.

---

## 3. Phase A — Tagging review (`tools/dem_pack/review_tags.py`)

**Inputs (read-only):** the 602 shortlist + `kernel_inferred_tags.json` (absolute
WG9 paths, configurable via args; defaults documented).

**Join:** for each shortlist kernel, attach its inference
(`inferred_family`, `family_confidence`, `tag_status`, `rationale`, `tags`) and
key metrics (`height_range_m`, `mean_slope_deg`, `slope_p95_deg`,
`coverage_fraction`, `quality_score`) + its `preview_height.png` path. Kernels in
the shortlist with no inference are flagged.

**Outputs:**
- `dem_tag_review.html` — a single self-contained static HTML file: kernels
  grouped by `inferred_family`, sorted by confidence desc, each row showing the
  preview thumbnail (file:// link to the WG9 preview PNG), id, current vs inferred
  family, confidence, rationale, metrics. (HTML because the review is of terrain
  *images* — a text table can't show 600 thumbnails. No server; open in a
  browser.)
- `dem_tag_review.csv` — same data as rows, for bulk editing in a spreadsheet.
- `kernel_family_map.approved.json` — **seeded**, then user-edited. Seed policy:
  `tag_status` retained/suggested with `family_confidence >= 0.7` → accepted to
  `inferred_family`; everything else (low-confidence, unresolved) → `excluded`.
  Shape:
  ```json
  {
    "version": 1,
    "source_shortlist": "<path>",
    "source_inferences": "<path>",
    "map": { "<kernel_id>": "<family>", ... },
    "excluded": ["<kernel_id>", ...]
  }
  ```

**The checkpoint:** the user reviews `dem_tag_review.html`, edits the approved-map
JSON (directly, or edits the CSV and re-runs `review_tags.py --from-csv` to
regenerate the JSON), and confirms it reflects intent. **Phase B does not run
until the user approves the map.** (In the plan this is an explicit pause.)

**Re-runnable:** `review_tags.py` is idempotent — re-running regenerates the HTML/
CSV and (unless `--from-csv`) re-seeds the map; it never clobbers a user-edited
approved map without `--reseed`.

---

## 4. Phase B — Pack build (`tools/dem_pack/build_pack.py`)

**Inputs:** `kernel_family_map.approved.json` + each approved kernel's
`kernel.json` (for metadata) + `normalized_height.npy` (the array). WG9 paths
read-only.

**Output:** `wg-10/worldgen_terrain/packs/dem_v1/`:
- `terrain_pack.json` — a `worldgen10.terrain_pack.v1`.
- `kernels/<family_id>.npy` — the chosen `normalized_height.npy` files, copied
  into the W10 repo (so the pack is self-contained, not pointing at WG9).

**Composition rules:**
- **One W10 family id per kernel.** W10's schema is one kernel per family id
  (M2 `FamilyKernel`). Multiple DEM kernels of the same terrain type become
  distinct family ids (e.g. `badlands__grand_canyon`, `badlands__death_valley`),
  preserving variety. The family id is the kernel_id (already unique).
- **`relief_m` historical note:** this spec originally used the kernel's
  `height_range_m` here. That is wrong for `normalized_height.npy` z-score kernels;
  use `height_std_m` for z-score-to-metres conversion, or rebake the kernel to a
  documented bounded distribution before applying a range scalar.
- **`footprint_m`** = `approx_sample_spacing_m × sample_px` (the kernel's true
  ground extent). The shipped gate pack is not uniformly ~50 km; it is ~37.7–222.6 km
  (median ~194.3 km). A pack-time `footprint_scale` exists, but it is not a live
  runtime scale solution.
- **`.npy` choice:** `normalized_height.npy` (per-kernel z-score normalized). It does
  not match `height_range_m` scaling by construction; the conversion scalar must be
  documented and gated if this path is used again.
- **Palettes (exactly 3 families):** group family ids by terrain TYPE (the
  approved `family`); within each type, sort ids lexicographically (deterministic)
  and chunk into palettes of 3. **Remainder rule (fixed, single):** if a type's
  count is not divisible by 3, the LAST palette is padded by repeating that type's
  earliest ids (cycling from the front) until it has 3 — so every palette is
  same-type and exactly 3, no cross-type mixing. A type with fewer than 3 total
  ids still forms one palette by repeating its ids to fill 3 (a 1-kernel type →
  `[id, id, id]`). Compatibility: each palette lists the other palettes of the
  same terrain type, plus one default cross-type neighbor (deterministic by
  sorted type order) so the grammar's compatible-roll has somewhere to go.
- **`grammar_constants`:** region/province sizes + pct thresholds + `moderation_*`
  carried from a sensible default (reuse the height_pack values; tunable later).

**`--validate`:** before emitting, confirm every approved kernel's `.npy` exists,
parses as 512×512 f32, `height_range_m > 0`, `approx_sample_spacing_m > 0`; every
palette has 3 resolvable families; the `family_ids ⟺ kernels` 1:1 invariant holds
(M2's loader requires it). Fail loudly with the offending kernel id.

**Size reality:** 512×512 f32 = 1 MB/kernel; the approved set (≈100–600) → up to
~600 MB of `.npy`. The pack JSON references them; the GPU atlas uploads them. The
parity/property GATES use a representative SUBSET (a `--gate-subset N` pack
variant, e.g. 24 kernels across families) — the gate proves CORRECTNESS, not
scale. Full-set scale is M3's streaming problem. `*.npy` are git-committed binary
(repo grows; acceptable for a curated set — note it).

---

## 5. Validation & gates

- **Loader/grammar (existing, must stay green):** the real pack passes
  `pack::load_pack_dir` validation + grammar property gates.
- **New `dem_pack_check.gd`** (added to a suite): load the real DEM pack (the
  gate subset), assert `height` finite, bounded by max `relief_m`, deterministic,
  region+province seam-continuous, and varied across a grid (real terrain ≠ flat).
- **GPU parity on the real pack:** `gpu_parity_check.gd` extended (or a sibling
  `gpu_parity_dem_check.gd`) runs against the DEM gate-subset pack — Tier-1 family
  signatures EXACT, Tier-2 height within the f32 epsilon — on real 512×512
  kernels. **This is the real-scale validation of the M2 kernel atlas.**
- **Python `--validate`** (§4) gates the build before Godot.
- **Done:** the user has approved the family map; `build_pack.py --validate`
  passes; `cargo test` green (unchanged crate); `fast` + the DEM gate(s) green;
  GPU parity green on the real pack subset; DESIGN/ROADMAP/STATUS updated;
  committed in phases (tools, then approved map + pack, then gates + docs).

---

## 6. Module boundaries & repo layout
```
tools/dem_pack/
  review_tags.py                  # Phase A: shortlist ⋈ inferences → HTML/CSV review + seed approved map
  build_pack.py                   # Phase B: approved map + kernel.json → W10 pack JSON + .npy copy + --validate
  kernel_family_map.approved.json # the reviewed family map (user-edited source of truth)
  README.md                       # how to run A, review, then B
wg-10/worldgen_terrain/packs/dem_v1/
  terrain_pack.json               # the real worldgen10.terrain_pack.v1 (full set)
  terrain_pack.gate.json          # gate subset (small, for the property/parity gates)
  kernels/*.npy                   # chosen normalized_height kernels (binary)
wg-10/worldgen_terrain/tests/
  dem_pack_check.gd               # real-pack property gate (subset)
  gpu_parity_dem_check.gd         # real-pack GPU parity (subset) — or extend gpu_parity_check.gd
tools/gate.py                     # add dem_pack_check (fast suite) + dem parity (gpu suite)
docs/plans/                       # DESIGN §3 (real pack), ROADMAP M1 pack line, STATUS
```
Each tool one job; the Rust crate untouched. `review_tags.py` and `build_pack.py`
are independent (A's output is B's only input — the approved map).

---

## 7. DESIGN.md updates this plan must make
- §3 (terrain packs): record that the FIRST REAL DEM pack exists (`packs/dem_v1`),
  built from WG9's 602-kernel shortlist + reviewed family inferences; families =
  one-per-kernel grouped into 3-family palettes by terrain type; `relief_m` from
  `height_range_m`, `footprint_m` from real ground extent (tunable for M3). The
  ROADMAP "terrain-pack format defined and loadable (first pack = DEM)" line can
  finally go `[x]` (the format was M1; the real DEM pack is now wired).
- §9: note the **footprint/relief visual-tuning** deferral (M3); the **pack size**
  (~600MB committed binary if full set) reality; and that GPU parity is validated
  on a real-512×512 subset (the M2 atlas-at-scale risk is now exercised, uniform
  size, no redesign needed).

## 8. Named risks (do not solve now)
- **Tagging accuracy:** WG9's inferences are heuristic; the human review (Phase A)
  is the mitigation. Mis-tags that survive review surface as odd terrain in M3 —
  re-review then.
- **Pack size at full scale:** committing ~600MB of `.npy` is heavy; the gate uses
  a subset. If full-set git weight becomes a problem, revisit (LFS / external
  fetch) — not now.
- **footprint_m physical vs game scale:** true ground extent (~50km/kernel) may
  be too large/small for desired gameplay feel; `footprint_scale` constant exists
  for M3 tuning. Decide visually when the renderer exists.
