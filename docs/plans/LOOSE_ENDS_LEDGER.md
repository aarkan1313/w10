# WorldGen10 — Loose-Ends Ledger (2026-05-30)

Single source of truth for **everything started-but-not-closed** as we pivot to the worldgen-core
rebuild (north-star vision + WG9 recipe). Built so nothing is silently dropped. Each item is tagged:
- **FIX-NOW** — close before the rebuild (real bug / cheap / would corrupt the rebuild if left).
- **FOLD-IN** — gets absorbed/resolved BY the worldgen rebuild (don't fix separately).
- **TABLED** — deliberately parked, recorded here so it's not forgotten (with why + when to revisit).
- **KEEP** — done + proven, stays as foundation.

Sources: this session's work + `docs/plans/FINDINGS_2026-05-30.md` (owner's fresh-chat audit — two
independent reviews, every claim re-verified against source + re-run gates).

---

## CODE BUGS (from FINDINGS — verified against source)

| ID | Item | Severity | Decision | Why |
|----|------|----------|----------|-----|
| **B1** | `Wg10PagePool` leaks GPU RIDs — no `Drop`/teardown; `m3_review.gd` + `m5_detail_check.gd` have FACTUALLY WRONG comments claiming auto-free | HIGH | **FIX-NOW** | Real leak, cheap structural fix (add `Drop`→`free_all`), and the wrong comments are a footgun the rebuild would inherit. The render pipeline is KEPT, so its bugs matter. |
| **B2** | Never-black "hold-last-good" can show stale terrain (page-A geometry + page-B pixels) under pool eviction; guarantee is capacity-dependent, not structural | HIGH | **FIX-NOW** | The render pipeline is the KEPT foundation; a non-structural never-black is a latent corruption the rebuild sits on. Fix = protect held coarse pages + re-validate held RID + capacity-pressure gate. |
| **B3** | Hardened perf gate hole: 100%-sky frame scores `nonblack=1.0` (sky is bright); detail not on/off-tested | MEDIUM | **FIX-NOW** | This is MY gate, built to honor "is profiling real?" — and it has the exact hole that rule forbids. The rebuild will lean on this gate to measure worldgen perf; it must be trustworthy first. Fix = terrain-vs-sky nonblack + detail on/off assert. |

### Added 2026-05-31 (validated code-path audit; not a visual verdict)

| ID | Item | Severity | Decision | Why |
|----|------|----------|----------|-----|
| **B4** | OLD `dem_v1` kernels-as-height over-amplify peak-to-peak relief by the z-score span: median **5.56×**, range **3.97–11.16×**. Verified against source: `height.rs`, `height_field.glsl`, and `height_page.glsl` all do z-score `sample × relief_m`; the shipped `.npy` arrays have std≈1 and ptp exactly equal to WG9 `height_range_m / height_std_m`. Correct metres for `normalized_height.npy` = `z × height_std_m` (or rebake to a documented bounded range), not `z × height_range_m`. | HIGH | **FIX-NOW-or-FOLD-IN** | This is a code/metadata contract bug in the old DEM-pack height path, not a visual finding about the current Python skeleton review scene. It distorts old-engine A/Bs, can exceed the ±8 km render AABB at `RELIEF_SCALE=0.25`, and would re-infect any Rust/GLSL port or kernel-detail layer that copies the `relief_m=height_range_m` contract. Fix/document before Slice 3 or before using kernels as runtime detail. Also verified: shipped gate pack has only **24** kernel files and every palette is `[A,B,A]`. |
| **B5** | Runtime scale is locked to one 2^L clipmap cascade (`BASE_SPAN=8192`, `PAGE_PX=256`, one shader detail-frequency curve). The Godot review scene's 6 km vs 26 km span proves horizontal content scale is valuable as a creative knob, but it is not the same thing as near-field runtime resolution. | HIGH | **FOLD-IN** | Keep **landform/content scale** as an explicit generator knob because the same terrain density can feel like a different place at different spans. Separately, Slice 5 needs a real runtime scale rework: per-level span/page-px/detail-frequency policy, plus gates, so 1–10 m near-field detail does not drag the whole hierarchy or break flight-scale coherence. |
| **B6** | Rough-highlands keeper existed as working Python/Godot review code, but not as a frozen implementation contract. | HIGH | **CLOSED for Slice-2A-close** | Frozen as `rough_highlands_keeper_v1`: contract spec, deterministic sample fixture, golden contact-sheet hash, fact boundaries, scale/relief policy, and fixture regression tests. This gives Slice 3 a precise implementation target if/when owner terrain/travel acceptance opens the port gate. |
| **B7** | **Keeper formula fork — "rough_highlands" names THREE different height formulas, and the owner-approved one is NOT the frozen one.** **A** = `geography_skeleton.compose_height` (6-regime softmax blend, behind the 90 km `rough_world_review.tscn` the owner liked). **B** = `export_godot_rough_world_chunks._compose_windowed_height` (`rough_highlands_keeper_v1`, a from-scratch seam-safe rewrite on a *different* skeleton generator). **C** = `height_page_rough.glsl` (closed-form GLSL approx of B, the M3 streaming spike). Reproducible check (run from `tools/dem_pack`, compose A and B on identical core coords seed 133 / rough_anchor / 129²): `corr(A,B) = +0.13`, B relief = 35% of A — B is a different, much flatter terrain only loosely related to A. (NB: an audit agent reported "−0.13 / inverted" — that was a SIGN ERROR; verified `+0.13`, not an inversion.) | HIGH | **ACTED ON (2026-05-31); now folds into B8** | `keeper_v2` (best-of-both: A's regimes on B's seam-safe substrate) BUILT + seam-exact (border delta 0.0) + committed (`tools/dem_pack/keeper_v2.py`, 23 tests); A\|B\|v2 switcher scene + Tier-1 traversability gate committed. Owner reviewed all three, direction shifted from "pick one" to **keep all three as variants + pursue guaranteed traversability** (B8). v2 is the traversability front-runner (only variant with a crossing corridor at play scales). Slice 3 port still blocked until a final stack is owner-accepted post-Tier-3. Full trace: memory `worldgen10-keeper-formula-fork`; STATUS fork-resolution session update. |
| **B8** | **Guaranteed regime-aware traversability (the real quality bar, owner-directed 2026-05-31).** All three generators are local `f(x,z)` — none guarantees a connected route across a region. Tier 1 (measure/gate) DONE (`report_abv_traversability.py`); Tier 2 (bias knobs) later; **Tier 3 = guaranteed routes through barrier regions** is the target: regime-aware, VERIFY-THEN-CARVE, seam-safe, offline→online. | HIGH | **SPEC+PLAN WRITTEN; BUILD STARTED — CARVE BLOCKED (2026-05-31)** | Spec + plan written (`docs/superpowers/specs/2026-05-31-worldgen-tier3-guaranteed-traversability-design.md` §1.2, `docs/superpowers/plans/2026-05-31-tier3-guaranteed-traversability.md`). Built + seam-safe: barrier detection (height-derived, scale/relief-aware — regime weight caps ~0.32 so it's NOT the detector) + verify-first no-op (`tools/dem_pack/traverse_corridor.py`, 9 tests green). **CARVE BLOCKED (proven w/ data):** global least-cost-path carve can't be seam-exact (border delta 0.62≠0); local seam-exact operators don't guarantee a connected crossing; channel-anchored is seam-exact but incomplete. Needs a cross-seam-stitched **connected-corridor fact = unbuilt connectivity half of Phase 7B**. Module is honest (real barriers → `carve_pending`, never falsely resolved). Next (owner, pillar-judged): pull 7B connected corridor forward / scope to channel-where-available / Tier-2 param-bias. Memory: `worldgen10-tier3-seam-exact-carve`, `worldgen10-tier3-barrier-measurements`, `worldgen10-tier3-guaranteed-traversability`. |

## DOC DRIFT (from FINDINGS) — ✅ MOSTLY DONE in the 2026-05-30 doc-reconciliation pass

| ID | Item | Status |
|----|------|--------|
| A-distinct | docs said `distinct=18`, gate reports `15` | **✅ DONE** — headline docs now say 15 (any remaining 18 is in superseded-history sections). |
| A-relief-arith | STATUS used dead `0.35`; shipped `RELIEF_SCALE 0.25` | **✅ DONE** — STATUS superseded-divider note states 0.25 shipped + "× 0.35 below is dead"; the 0.35 lines are in [SUPERSEDED] M5 history. |
| A-m3-count | headlines said `m3 6/6`; current suite is `m3 9/9` after the B2 capacity gate | **✅ DONE** — HANDOFF/ROADMAP/STATUS now distinguish historical counts from the current 9-check suite. |
| A-DESIGN-stale | `DESIGN.md` predates the pivot | **✅ DONE (interim)** — DESIGN top now carries a superseded-notice (§2.1/§3 historical; the kept sections marked; points at the two 2026-05-30 specs). Full rewrite deferred until the worldgen core lands (DESIGN will be re-folded then). |
| A-STATUS-M5-2-states | STATUS held M5 in 2 contradictory states | **✅ DONE** — one current section + "EVERYTHING BELOW IS SUPERSEDED HISTORY" divider; all old sections demoted to [SUPERSEDED — history]. |
| A-spec-ownership | shaded-scale + kernel-dna specs fought over M5 S2-S4 | **✅ DONE** — both superseded by the worldgen-core spec; STATUS/ROADMAP reconciled. (Folder-level: the specs themselves have no at-a-glance "superseded" banner — minor, a reader learns which 2 are live from STATUS/this ledger.) |
| A-memory-branch | memory said branch `master`; actual `main` | **✅ DONE** — `worldgen10-build-gotchas` now says `main`. |
| A-C1 | spectral gate was circular (iFFT path, not shipping basis) | **CLOSED** — spectral dead; recorded as negative result, lesson kept. |
| A-pytest-count | HANDOFF said "15 dem_pack tests"; actual **22** | **✅ DONE** — fixed to 22 (caught by the 2nd cold-read test; suite grew with worldgen_proto). |

## IN-FLIGHT MILESTONE WORK (this session)

| Item | State | Decision | Notes |
|------|-------|----------|-------|
| **M5 detail S1** (fBm uniform detail) | code done, owner-confirmed VISIBLE, gated | **FOLD-IN** | The detail SEAM is real; but detail amplitude/freq get re-decided in the worldgen rebuild (it's a layer of the new height composition). Don't build M5 S2/S3 as-was. |
| **M5 detail S2 (LOD fade), S3 (surface descriptor)** | PLANNED (specs/plans written), NOT built | **TABLED** | Superseded by the rebuild. The surface-descriptor idea (slope/curvature) is REUSABLE in the rebuild (normals, modulation) — revisit then. Plans kept as history. |
| **Shaded-scale S1 (relief_scale)** | DONE + reviewed, parity-proven (visible==collision 0.000233m) | **KEEP** | One authoritative relief knob; the rebuild's height composition multiplies by it. Solid. |
| **Shaded-scale S2 (normals/lighting), S3 (mesh density)** | PLANNED, NOT built | **TABLED** | Owner: "IDC about textures/materials until heightmap works." Normals/materials come AFTER the worldgen core is good. Mesh-density folds into the rebuild's scale tuning (the 1-10m target). |
| **Kernel-DNA spectral synthesis (Slice 1)** | `spectral.py` built, 6 tests green, but REFUTED | **TABLED (negative result)** | Spectrum=roughness, discards phase=structure. `spectral.py` is INERT (only its own test imports it; no signature in live pack — verified). Kept as documented dead-end. Do NOT delete (records the lesson, costs nothing). |
| **Hardened perf gate** | built, green, in m3 suite | **KEEP** | The perf instrument for the rebuild. Real GPU-time; B3's terrain-vs-sky and detail-on/off assertions are active and verified. |
| **m3_accept wall-time gate** | flaky (phantom 77ms stalls) | **TABLED** | Superseded by the hardened GPU-time gate. Still in the suite. Consider removing/demoting later; not urgent. |

## TABLED — bigger things deliberately parked (recorded so they're not forgotten)

| Item | Why parked | When to revisit |
|------|-----------|-----------------|
| **Erosion / hydrology — DISTILLED (the Grand-Canyon-grade enhancement)** | **OWNER-CONFIRMED 2026-05-30 as a BIG LATER roadmap item.** The warped-noise worldgen core (S1, owner-accepted as "pretty good, a little noisy") looks like PLAUSIBLE terrain but NOT real connected erosion (the Grand-Canyon look = a specific river carving over eons = real-world history, which pure procedural noise approximates but never truly replicates). The bridge = owner's distilled-erosion insight: OFFLINE run real hydraulic erosion → learn how it carves (vs slope/flow) → distill a CHEAP LOCAL operator → apply online per-page (infinite+fast). This is THE path to Grand-Canyon-ish AND truly-procedural. NOT needed for the worldgen-core foundation; it's the headline "looks AAA-real" enhancement. | After the worldgen core (noise tier) ships end-to-end (Rust/GPU/biomes). A major milestone of its own (brainstorm→spec→plan). |
| **Materials / normals / biome surfacing / dressing** | Owner: only worldgen/height matters until kernels+heightmap good. Much of NMS "amazing" is shading, but it's downstream of a good height field. | After the worldgen core hits WG9-parity look. |
| **Modes: bounded / spherical-planet** | Framework goal; infinite-procedural is PRIMARY, built first. | After infinite core is good + adaptable. |
| **Handmade / authored-area blending** | Layers onto the infinite procedural base. | After the infinite core. |
| **M8 visible editable terrain** (M4 edit seam's other half) | Edits are collidable-not-visible; tracked since M4. | After render/worldgen settled. |
| **Async/background page production** | Deferred since M3; caching solved the spike. Trigger = heavy multi-pass pages. | If the rebuild's per-page worldgen cost blows budget. |
| **Kernel family tagging — 665 `uncategorized` (2026-05-31)** | Full auto-tag NOT reliably achievable: single-hillshade classification is confident-but-not-accurate (the rubric picks a catch-all; v1 → badlands/rainforest 47%, v2 → mountain 64%; passes disagree 470/631). Stats-from-metadata tagging is dead (3/40 visual agreement). **Trustworthy result = 161 cross-run-consensus tags + 34 bathymetry exclusions** (`D:\tmp\wg10_relief_audit\kernel_tags_consolidated.csv`, additive-only, fills `uncategorized`). 470 contested → human review. | When the grammar actually needs more than the 130+161 families — then human-review the contested CSV (both guesses + span included) OR adopt a COARSER taxonomy (badlands↔mountain↔temperate↔rainforest aren't separable from one hillshade at mixed scale). |

## THE NEW PRIORITY (owner-confirmed)
**WORLDGEN / height-field core is the ONLY active priority.** Bar = parity-or-better than WG9.
Scale target (owner "vibes, not literal"): WG10 currently reads ~1km-zoomed-out, WG9 ~250m, **goal
adaptable down to 1-10m near-field detail.** Recipe = the WG9 blueprint (macro fBm landmass + ridged
ranges + carved valleys + DEMOTED kernel overlay), built adaptable (every layer a knob), fitting the
KEPT clipmap/parity architecture. See memory `worldgen10-wg9-height-recipe` + `worldgen10-north-star-vision`.

> **See `docs/plans/SESSION_HANDOFF_2026-05-30.md` for the point-in-time pickup. Current addendum:
> B1/B2/B3 are source-fixed and gate-verified; the remaining active work is structure-first Slice 2A.**

## Close-out status (updated 2026-05-30, late)
- **Doc-drift (the A-* items): ✅ DONE** (the doc-reconciliation pass — HANDOFF/DESIGN/ROADMAP/STATUS/
  memory + 2 cold-read validations).
- **Worldgen brainstorm/spec/Slice-1: ✅ DONE** (S1 owner-accepted).
- **Worldgen Slice 2 (offline distillation tooling): ✅ BUILT + GATED, but the LOOK is NOT accepted** —
  tooling (biome_distill/distill_biomes/attach_biome_params/render_biomes) committed + pytest-green;
  owner verdict on the renders = "still not terrain, same noise, not real world." **PAUSED pending a
  structure-approach research/review** (plain warped/ridged noise = roughness, not connected ridgeline/
  drainage STRUCTURE — same truth as the spectral refutation). The distillation half is KEPT; the
  GENERATOR's structure stage is what's under research. See STATUS "Slice 2" section.
- **CODE bugs B1/B2/B3 — CLOSED for the rebuild precondition:**
  1. **B1** (pool RID leak) — ✅ **SOURCE FIXED + rebuilt + windowed-gate-verified**. Rust `Drop` routes to
     `free_all_impl`; m3/gpu/fast reruns are green after the editor-closed rebuild.
  2. **B3** (perf-gate terrain-vs-sky + detail on/off) — ✅ **SOURCE FIXED + verified**.
     `m5_perf_hardened_check.gd` now has `SKY`/`MIN_TERRAIN_FRAC`/`_terrain_frac` plus
     `DETAIL_DELTA_MIN`/`_detail_on_off_delta`; m3 passes with `terrain_frac_min=1.000` and
     `detail_delta=0.53739`.
  3. **B2** (structural never-black + capacity-pressure gate) — ✅ **SOURCE FIXED + unit-tested + verified**.
     `cargo test` isolated target = **121 passed / 0 failed**. The new m3 capacity-pressure gate passes
     non-vacuously (`full_delta=3`, `pressure_held=3`, `resident=9`) and proves displayed coarsest pages stay
     pinned under tight capacity.
- **Precondition for Slice 3 (first runtime build):** B1/B2/B3 are no longer blocking. Latest gates:
  **fast 6/6 · gpu 4/4 · m3 9/9 · cargo 121**.
(Owner may choose to fold some of these INTO the structure rebuild instead of before — triage pending the
research outcome.)
- **NEW 2026-05-31 (validated code-path audit):** **B4** (old dem_v1 z-score/range contract bug), **B5**
  (content-scale knob vs runtime scale-cascade rework), and **B6** (rough-highlands keeper contract freeze)
  added above. Source NOT changed. B4 is a Slice-3/kernel-detail contract risk and old-engine A/B distortion;
  B5 is folded into Phase 5 scale work; B6 is now closed by the Slice-2A-close contract. Kernel-
  tagging result (161 consensus tags + 34 bathymetry exclusions) parked in TABLED. Raw audit artifacts remain
  in `D:\tmp\wg10_relief_audit\`; do not treat them as repo truth without re-validation.
