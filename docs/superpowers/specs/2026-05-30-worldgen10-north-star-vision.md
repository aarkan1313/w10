# WorldGen10 — North-Star Vision (re-vision, 2026-05-30)

**Status:** vision (owner-confirmed direction). NOT a milestone spec — this is the top-level picture the
milestone specs serve. Supersedes the `kernel-dna-synthesis` spectral direction (refuted, see §6).
**Predecessor:** written after the spectral-synthesis approach was disproven by the owner's eye, which
forced a full step-back re-evaluation of what WorldGen10 IS.

> This is a VISION doc. The 3 living docs (DESIGN/ROADMAP/STATUS) + HANDOFF stay current and are updated
> to point at this. Memory: `worldgen10-kernel-dna-synthesis` (carries the refutation + this pivot).

---

## 1. What WorldGen10 IS (owner-confirmed)

**A terrain framework/engine for ANY game the owner wants to make** — adaptable, huge-scope (beyond
what's roadmapped today), a combo of tech-demo and real usable thing. Proof points the architecture must
eventually serve, via easy adaptable knobs, with minimal tension:
- A **Space Engineers**-style planet (spherical, planet-scale, voxel-editable).
- A **Diablo 4**-style ARPG (bounded gorgeous zones, top-down).
- A **Sons of the Forest**-style survival island (one dense detailed bounded world, on-foot).

**Primary mode (non-negotiable, build FIRST): infinite + procedural, like No Man's Sky.** Handmade /
authored areas exist and are part of the process, but they LAYER ONTO the infinite procedural base — the
infinite procedural world comes first. Other modes (bounded, spherical) are knobs/adaptations of the core,
designed for but not built first.

## 2. The core realization (why this re-vision exists)

The terrain CONTENT read as "blobby, placed, not a contiguous landmass." Root-caused + probed this session:
- Kernels were sampled as TILING textures (`sample_kernel`, `rem_euclid` every footprint_m ~50–220 km) →
  repeating stamps, no continuous structure.
- The "fix it with spectral DNA synthesis" attempt was **REFUTED by the owner's eye** (offline, before any
  runtime rebuild): a real kernel hillshade looks amazing; value-noise, gradient-noise, AND even the
  spectrally-PERFECT iFFT field all look like "nothing / noise." **A power spectrum captures ROUGHNESS but
  discards PHASE — and phase is where STRUCTURE (ridgelines, drainage, carved form) lives.** No noise basis,
  however tuned, produces structure. (Huge save: learned for ~half a day of offline work, zero runtime cost.)
- **The deeper truth the step-back exposed:** real STRUCTURE (erosion-grade ridgelines/drainage) classically
  needs GLOBAL, ITERATIVE computation — fundamentally in tension with infinite per-page streaming + parity +
  live perf. You cannot have truly-infinite + global-erosion-quality + live all at once the naive way.

## 3. How the tension resolves (the architecture)

No Man's Sky proves infinite + procedural + live + good is achievable **without** global erosion, via
LOCAL deterministic functions + heavy materials/dressing. The owner's erosion insight ("run it, analyze
how it affects terrain, then simulate it effectively/close enough while maintaining performance") supplies
the missing piece. The resolved north-star architecture:

1. **Infinite procedural core (NMS-style):** terrain = local, deterministic `f(world position)` →
   height/structure, evaluated per-page, parity-safe. Structure comes from LOCAL structure-APPROXIMATING
   math (domain warping, ridged/billow noise, analytic uplift, flow-approximating functions) — fakes
   ridges/valleys/connectivity with local math that READS as structure, no global sim. DEMs INFORM the
   parameters of these functions (which structure character per biome) — NOT reduced to a spectrum, but
   used to tune the structure-generating machinery.
2. **Distilled erosion (the owner's resolution to the hard tension):** OFFLINE run REAL hydraulic/hydrology
   erosion on sample terrain (no perf limit) → ANALYZE how it transforms the heightfield (as a function of
   local slope/flow/curvature) → DISTILL that into a CHEAP, LOCAL operator (analytic and/or learned) →
   apply it ONLINE per-page, in-budget, parity-safe. Erosion-LOOKING terrain, infinite, fast. (Same
   offline-heavy → online-cheap pattern that solved GPU timing + the bake insight this session.)
3. **Materials / biomes / dressing:** much of "looks amazing" in NMS-class terrain is the SHADING +
   scattered objects (rocks, plants, color) — not the raw heightfield. Materials/normals/biome-driven
   surfacing + object scatter are first-class, not an afterthought. (The current unshaded debug color is
   WHY today's build reads as "a heightmap" — confirmed by owner fly.)
4. **Handmade / authored areas:** layer onto the infinite procedural base (blend an authored region into
   the procedural field). Designed-for; built after the procedural core.
5. **Modes as knobs:** infinite (primary) / bounded / spherical-planet are adaptations of the core via
   config — designed-for so the framework flexes (Diablo/SE/SotF), not separate engines.

**Keep (proven, on-pillar):** the infinite streaming clipmap RENDER PIPELINE (never-black, p99 < budget,
real-GPU-time gate), the GRAMMAR (where biomes go), the FACTS/collision API + relief_scale (visible==
collision). These are the strong foundation; the rebuild is the height CONTENT + structure + materials.

## 4. The pillars, applied to this vision

1. **Adaptable/tunable** — DEMs inform structure-function + erosion-operator PARAMETERS (knobs), not hard
   geometry; modes are config; handmade areas blend via knobs. Everything tunable, no magic numbers.
2. **Performance** — the expensive parts (real erosion, structure analysis) run OFFLINE; runtime is local
   per-page cheap evaluation. The hardened real-GPU-time gate guards every online operator. Infinite +
   in-budget is preserved because nothing global runs live.
3. **Quality** — structure (the thing the spectrum lacked) comes from structure-generating machinery +
   distilled-real-erosion + materials. Deterministic, parity-able (local functions), bounded, no collapse.
4. **No shortcuts** — the erosion operator is DISTILLED FROM REAL SIMULATION (honest approximation, not
   faked-from-nothing); structure functions are tuned to real DEM character; validate/reject bad config.
   We do NOT pretend noise is structure (the spectral lesson).

## 5. What this means for the roadmap (re-sequencing)

The OLD M5–M7 (detail/biomes/erosion as separate late milestones) and the spectral-synthesis pivot are
superseded by this integrated picture. The likely new milestone shape (to be designed properly, this is the
sketch):
- **Structure core:** replace `sample_kernel` tiling with NMS-style local structure functions, DEM-informed.
  (This is the "make it a contiguous structured landmass" milestone — the heart of the rebuild.)
- **Materials/surfacing:** real normals + biome materials + dressing (the "stops looking like a heightmap"
  milestone — the owner's fly reaction).
- **Distilled erosion:** offline erosion → cheap online operator (the "looks carved/real" layer).
- **Relief_scale (DONE), render pipeline (DONE), grammar (DONE), facts (DONE)** remain the foundation.
- **Modes (bounded/spherical) + handmade-area blending:** later, as the framework matures.
Sequencing within this is a future design decision (likely: structure → materials → erosion, each proven
on-screen owner-flown; materials may come early since it's a big cheap look-win).

## 6. What's DEAD / kept as reference
- **Spectral kernel-DNA synthesis: DEAD** (refuted by eye — spectrum has no structure). `tools/dem_pack/
  spectral.py` + its tests are KEPT as a documented negative result (proves spectrum→roughness-not-
  structure), not deleted — they cost nothing and record the lesson. The `kernel-dna-synthesis` spec is
  superseded by this vision; its analysis tooling is inert (nothing runtime consumes signatures).
- **`relief_scale`, the render pipeline, grammar, facts: KEPT** (all proven, on-pillar).

## 7. Immediate next step (not yet decided — for the owner)
This vision is the picture; the FIRST concrete piece to design+build is a fresh decision. Candidates:
(a) research NMS-class local structure functions + how DEMs inform them, then design the structure core;
(b) start with materials/surfacing (big cheap look-win, independent of the structure rebuild);
(c) research erosion-distillation feasibility (de-risk the operator before relying on it).
The owner picks the entry point; each gets the full brainstorm → spec → plan → slice treatment, pillars
throughout, owner-flown acceptance (look-quality is judged by eye, not gates).

---

## Lessons locked from the road here (do not relearn)
- **Spectrum ≠ structure.** Power spectra discard phase; phase IS the ridgelines/drainage. Proven by eye.
- **Probe-first offline saved a runtime rebuild.** The spectral approach died on rendered images in half a
  day, before any GLSL/Rust. Keep de-risking research-flavored bets offline first.
- **Offline-heavy → online-cheap** is the recurring resolution (GPU timing, bake insight, erosion operator).
  When something's too expensive live, ask what can move offline and be distilled into a cheap online read.
- **Infinite is a MODE the framework serves, not a sacred constraint** — but it IS the primary mode, built
  first, NMS as the reference.
- **Look-quality is owner-judged.** Gates prove invariants (parity/perf/bounds); they cannot judge "looks
  like terrain." Every look milestone ends in an owner fly.
