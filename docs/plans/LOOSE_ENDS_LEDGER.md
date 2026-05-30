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

## DOC DRIFT (from FINDINGS — no code change, reconcile to the north-star)

| ID | Item | Decision |
|----|------|----------|
| A-distinct | docs say `distinct=18`, gate reports `15` (relief 0.25 → fewer buckets) | **FIX-NOW** (doc pass) |
| A-relief-arith | STATUS uses dead `0.35` multiplier; shipped is `RELIEF_SCALE 0.25` | **FIX-NOW** (doc pass) |
| A-m3-count | headlines say `m3 6/6`; actual `8/8` | **FIX-NOW** (doc pass) |
| A-DESIGN-stale | `DESIGN.md` predates M4/M5/synthesis/vision; carries a source-of-truth clause | **FIX-NOW** — point DESIGN at the north-star vision (or mark superseded-pending). Critical: it's the "locked architecture" doc and it's stale. |
| A-STATUS-M5-2-states | STATUS holds M5 S1 as both "ACCEPTED" and "GATED not accepted"; 2 `## M5` headers | **FIX-NOW** (doc pass) — collapse to one current state |
| A-spec-ownership | shaded-scale + kernel-dna specs fight over M5 S2-S4 / mesh-density ownership | **FIX-NOW** (doc pass) — reconcile to north-star; mark superseded |
| A-memory-branch | `worldgen10-build-gotchas` memory says branch `master`; actual `main` | **FIX-NOW** (one-line memory edit) |
| A-C1 | spectral gate was circular (tested iFFT path, not shipping basis) | **TABLED/CLOSED** — spectral dead; recorded as negative result, lesson kept |

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

## Suggested close-out order before the rebuild
1. **B1** (pool Drop + fix 2 sites + delete wrong comments) — small, removes leak + footgun.
2. **B3** (perf-gate terrain-vs-sky + detail on/off) — make the rebuild's measuring stick trustworthy.
3. **B2** (structural never-black + capacity-pressure gate) — the KEPT render foundation must be sound.
4. **Doc pass** (the A-* drift) — reconcile to north-star so "what's true" is unambiguous before building.
5. **THEN** brainstorm the worldgen core.
(Owner may choose to fold some of these INTO the rebuild instead of before — triage decision pending.)
