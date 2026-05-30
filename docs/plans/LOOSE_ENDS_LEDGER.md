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

## DOC DRIFT (from FINDINGS) — ✅ MOSTLY DONE in the 2026-05-30 doc-reconciliation pass

| ID | Item | Status |
|----|------|--------|
| A-distinct | docs said `distinct=18`, gate reports `15` | **✅ DONE** — headline docs now say 15 (any remaining 18 is in superseded-history sections). |
| A-relief-arith | STATUS used dead `0.35`; shipped `RELIEF_SCALE 0.25` | **✅ DONE** — STATUS superseded-divider note states 0.25 shipped + "× 0.35 below is dead"; the 0.35 lines are in [SUPERSEDED] M5 history. |
| A-m3-count | headlines said `m3 6/6`; actual `8/8` | **✅ DONE** — HANDOFF/ROADMAP/STATUS all say m3 8/8. |
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
| **Hardened perf gate** | built, green, in m3 suite | **KEEP** (after B3 fix) | The perf instrument for the rebuild. Real GPU-time. Fix B3 first. |
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

## THE NEW PRIORITY (owner-confirmed)
**WORLDGEN / height-field core is the ONLY active priority.** Bar = parity-or-better than WG9.
Scale target (owner "vibes, not literal"): WG10 currently reads ~1km-zoomed-out, WG9 ~250m, **goal
adaptable down to 1-10m near-field detail.** Recipe = the WG9 blueprint (macro fBm landmass + ridged
ranges + carved valleys + DEMOTED kernel overlay), built adaptable (every layer a knob), fitting the
KEPT clipmap/parity architecture. See memory `worldgen10-wg9-height-recipe` + `worldgen10-north-star-vision`.

> **See `docs/plans/SESSION_HANDOFF_2026-05-30.md` for the latest point-in-time state + the exact remaining
> steps (rebuild-verify B1/B2/B3, write the B2 capacity-pressure gate, structure research).**

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
- **CODE bugs B1/B2/B3 — status corrected after a source audit (the 'did the findings get worked on?' check):**
  1. **B1** (pool RID leak) — ✅ **SOURCE FIXED + committed `be9c4f2`** (Rust `Drop` impl → `free_all_impl`;
     `_exit_tree`+`_pool` in m3_review.gd; `free_all` at the 3 pool-owning returns in m5_detail_check.gd;
     the 2 wrong comments deleted). `cargo check` clean. **NOT yet gate-verified** — needs the editor closed
     to rebuild the real DLL + run `--suite m3` (8 checks) to confirm no regression. ← the ONE B1 to-do left.
  2. **B3** (perf-gate terrain-vs-sky + detail on/off) — ❌ **STILL OPEN** (an earlier note in this session
     wrongly said "done" off a corrupted file read; a `git show HEAD` audit corrected it). The committed
     `m5_perf_hardened_check.gd` still has the hole: nonblack counts any `c.r/g/b > 0.03` so a 100%-sky frame
     scores 1.0 (no `SKY`/`MIN_TERRAIN_FRAC`/`_terrain_frac`), and detail is set once, never on/off-asserted
     (`DETAIL_DELTA_MIN` absent). GDScript-only fix → can do without a rebuild. Fix = terrain-vs-sky nonblack
     (count a pixel real only if it differs from the sky color) + a detail-on-vs-off frame-delta assertion.
  3. **B2** (structural never-black + capacity-pressure gate) — ❌ **STILL OPEN** (Rust). Protect held coarse
     pages from eviction + re-validate the held RID maps to its key before display + a capacity-pressure gate.
     Best batched with the B1 rebuild in one editor-close window.
- **Precondition for Slice 3 (first runtime build):** B1 gate-verified + B2 done + B3 done. (Plan: do B3
  GDScript-only now, then one editor-close window for B1 rebuild + B2 + full gate run verifies all three.)
(Owner may choose to fold some of these INTO the structure rebuild instead of before — triage pending the
research outcome.)
