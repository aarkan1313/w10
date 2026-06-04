# WorldGen10 — Post-Roadmap (Phase 10–14)

**What this is:** the design-level continuation that picks up *after* `ROADMAP.md` Phase 9. It is deliberately
a separate doc, not an edit of the live ROADMAP, because the live ROADMAP is mid-rebuild around Phase 5 and
should stay focused there. Nothing here is committed or scheduled. Each phase is gated behind owner
acceptance of the phases it depends on, exactly like the Phase 6/7 specs gate themselves behind Phase 5.

**How it was derived:** a read-only audit (2026-05-31) of every plan/spec, then an adversarial pass that
tried to prove each candidate "after-roadmap" idea was *already covered* by Phases 5–9, the Deferred
follow-ups, or the LOOSE_ENDS_LEDGER TABLED items. Only items that survived that refutation are promoted to
phases here. The full reasoning is in memory `worldgen10-post-roadmap-next-steps`.

> **Status of every phase below: design-direction only.** No implementation, no schedule. The hard blocker
> for all of it is still an owner-accepted Phase 5 height keeper. These exist so that when the foundation
> lands, the "what next" conversation starts from a vetted, pillar-traced map instead of a blank page.

---

## The four pillars (the lens every phase is judged by)

Carried from `DESIGN.md §1`, in the owner's priority order. Every phase below states which pillars it serves
and how, because a phase that does not trace to a pillar should not exist.

1. **Adaptable / tunable** — knobs, config packs, no magic numbers; serves multiple games.
2. **Performance** — holds high movement speed with heavy game overhead, no stalls; expensive work is offline.
3. **Quality** — no black holes, no popping, graceful degradation, reads as real.
4. **No shortcuts** — the durable answer, not the expedient one; honest approximations, never faked-from-nothing.

## The three proof-point games (the adaptability test)

From `2026-05-30-worldgen10-north-star-vision.md §1`. A phase earns its place partly by which proof point it
unblocks:

- **SE** — a Space-Engineers planet (spherical, planet-scale, voxel-editable).
- **D4** — a Diablo-4 ARPG (bounded gorgeous zones, top-down, authored + procedural).
- **SotF** — a Sons-of-the-Forest survival island (one dense detailed bounded world, on-foot).

---

## Where the current roadmap ends

`ROADMAP.md` runs M0–M4 (done: toolchain, deterministic generation, GPU parity, render pipeline, Facts API),
then the forward plan:

- **Phase 5** — worldgen core rebuild (ACTIVE; 85%-geography target; offline-first; no Rust port until owner
  accepts a keeper).
- **Phase 6** — materials & surfacing (SurfaceDescriptor, material packs, **scatter v0** = deterministic tags).
- **Phase 7** — erosion & drainage (7A local filters, 7B connected drainage skeleton).
- **Phase 8** — framework modes (bounded / island / spherical / handmade-blend knobs).
- **Phase 9** — visible editable terrain (the other half of M4's collidable-not-visible edit seam;
  save/load is its optional final slice).

Phase 9 Slice 4 is the last checkbox in the live roadmap. There is no explicit "the framework is shippable"
capstone. Phases 10–14 fill that gap.

---

## What was REJECTED as a new phase (already covered — fold in, don't re-invent)

The audit's most useful output is what *not* to build as new work. Spinning these up as fresh phases would be
rework and would violate pillar 4's "no shortcuts" (in the sense of: don't duplicate a contract that already
exists). Each maps to existing coverage:

| Tempting "new" idea | Already covered by | Correct action |
|---|---|---|
| Public API stabilization | M4 Facts API + `DESIGN §6.2` (`set_config`/`get_height`/`get_collision_field`) + pack v1 versioning (`§3`) | Hardening follow-up, fold into Phase 14 |
| Determinism / parity regression CI | Phase 5 Slice 3 already promises "regression render sheet … determinism, boundedness, seam, Python-vs-Rust parity" | Add a **committed golden baseline** to Slice 3's definition-of-done; surface it in Phase 14 |
| DEM pack-build / ingestion pipeline | Phase 5 Slice 2B + existing `2026-05-29-real-dem-pack-design.md` tools (`build_pack.py`, `distill_biomes.py`) | Finalize in Slice 2B; refinements already in Deferred follow-ups |
| Config validation & rejection gates | `DESIGN §6.1` (config-as-source) + pack loader rejects malformed packs + Phase 5 Slice 5 scale-knob gating | Per-layer validation as built; collect into Phase 14 sweep only if edges remain |
| World persistence / save-load | `ROADMAP` Phase 9 Slice 4 (optional) | Phase 9; only multiplayer determinism would be a genuinely-new extension |

These are tracked as **Phase 14 capstone inputs**, not standalone phases.

---

## Phase 10 — Editable-terrain physics & voxel-edit loop

**Survived refutation: YES (genuinely new).** Phase 9 makes edits *visible == collidable*, but the M4 spec
(`2026-05-30-m4-facts-api-design.md §scope`) explicitly excludes the physics-feedback layer, and nothing in
6–9 adds it. This is the physics response *on top of* a visible/collidable edit.

**Scope:** Jolt rigid-body notification when ground height changes under or near a body; constraint
propagation (resting bodies slide / tumble / settle rather than clipping); recompute-on-edit scheduling so a
brush stroke or explosion never stalls the hot path (the WG9 rule); sculpt interaction model (instant stamp
vs. brush vs. continuous); and the sub-grid physics-detail question for `HeightMapShape3D`.

**Why after, not during:** it requires a coherent visible+collidable edited surface to react to, which only
exists at the end of Phase 9. M4 ships edits collidable-but-not-visible and deliberately defers physics
feedback.

**Pillars:** Performance (bounded, never-stall recompute is the whole challenge); No-shortcuts (real physics
response, not a teleport-the-body cheat); Quality (no clipping/jitter after an edit).
**Proof point:** **SE** — this is the *sole* unlock for the voxel-edit proof point; it is the
highest-leverage genuinely-new item. **Depends on:** Phase 9 Slice 1 (edit store + visibility).
**Spec:** `docs/superpowers/specs/2026-05-31-worldgen-phase10-edit-physics-design.md`.

## Phase 11 — Ecosystem & vegetation rendering

**Survived refutation: YES.** Phase 6 commits only **"scatter v0"** — deterministic placement *tags* + a
boundary-pop gate (`2026-05-31-worldgen-phase6-surfacing-design.md §6`, "sparse hero props only after base
scatter is deterministic"). The rendering subsystem is unbuilt.

**Scope:** the rendering + ecosystem stack over Phase 6's deterministic tags: multi-LOD (mesh → impostor →
billboard) with seamless falloff; GPU instancing / multimesh at density (grass, trees, rocks); ecosystem
placement rules (spacing, slope-appropriate undergrowth, shade-cast thinning, altitude/biome density curves);
wind/animation; budget under full load. It **consumes the Phase 6 SurfaceDescriptor** — no rederived slope or
masks (Phase 6 non-goal).

**Why after, not during:** Phase 6 explicitly stops at deterministic tags + the boundary-pop gate. LOD models,
instancing strategy, and ecosystem simulation are a separate subsystem the roadmap never scopes.

**Pillars:** Adaptable (per-biome density packs, swappable like material packs); Performance (instancing
budget guarded by the hardened GPU-time gate); Quality (the biggest cheap "stops looking like a heightmap"
win after materials). **Proof point:** **SotF** (dense forest), **D4** (lush zones).
**Depends on:** Phase 6 (descriptor + scatter v0 determinism gates).
**Spec:** `docs/superpowers/specs/2026-05-31-worldgen-phase11-ecosystem-rendering-design.md`.

## Phase 12 — Water & hydrography

**Survived refutation: PARTIAL — unbuilt but dependent.** Water *surface rendering* is genuinely undesigned in
Phases 5–9; it is not orthogonal, it *consumes* Phase 7B discharge/channel facts and Phase 8 mode masks. So it
is real after-roadmap work, gated on 7B + 8 rather than free-standing.

**Scope:** a coarse bathymetry / depth field (where water sits, keyed by world coords + mode); shoreline mesh /
transition band; a water-surface shader (flow vectors from 7B `discharge`/`channel_axis`, shore→deep
gradient); rivers that reach the sea via the routed skeleton (fake-connectivity where true global hydrology is
impossible, per the north-star honesty rule); water composed into the **Facts API** so gameplay/collision can
query "is this point underwater / how deep."

**Why after, not during:** 7B provides discharge facts but no render contract for a water surface; Phase 8
decides how water is *scoped* per mode (bounded lake / infinite ocean with falloff / spherical water mask).
Water needs both before it can be specified.

**Pillars:** Adaptable (mode-scoped water is a config choice over one system); Quality; No-shortcuts (honest
about what coarse routing can and cannot guarantee — no claim of exact continental hydrology).
**Proof point:** **SotF** (island ocean + rivers-to-sea), **D4** (lakes / channels).
**Depends on:** Phase 7B (discharge facts in the keeper) + Phase 8 (mode masks).
**Spec:** `docs/superpowers/specs/2026-05-31-worldgen-phase12-water-hydrography-design.md`.

## Phase 13 — Authored-area composition

**Survived refutation: YES.** Phase 8 names "handmade / authored-area blending" in a single knob line and
TABLES it (`LOOSE_ENDS_LEDGER` Handmade row, "Layers onto the infinite procedural base"); no spec covers the
implementation. This is distinct from M4/Phase-9 *stamp* edits — authored areas are large baked/painted
regions, not gameplay carves.

**Scope:** authored-heightfield import (image / sculpted mesh / Godot terrain → world-anchored field); the
seam-blend (reuse the same blend mechanism as biome borders, so the procedural→authored transition is
seamless and determinism survives the seam); a zone-composition workflow (mark a region, choose
authored-vs-procedural, blend width); and the determinism + Facts/collision parity contract across the
authored seam (visible == collision must still hold inside and across an authored region).

**Why after, not during:** Phase 8's mode knobs establish *where* terrain is scoped, but provide no import,
blend math, or authoring workflow. The current Phases 5–9 assume all terrain is procedural.

**Pillars:** Adaptable (authored areas are how a game adds hand-crafted set-pieces over the infinite base);
No-shortcuts (determinism + parity preserved across the seam, not special-cased away).
**Proof point:** **D4** (authored zones / dungeons with procedural fill), **SotF** (handcrafted island center).
**Depends on:** Phase 8 (mode knobs), Phase 9 (edit-store mechanics it can reuse).
**Spec:** `docs/superpowers/specs/2026-05-31-worldgen-phase13-authored-area-design.md`.

## Phase 14 — Productization capstone (the "framework, not tech demo" milestone)

**Not a new feature phase — an assembly phase.** This is where the REJECTED-as-new items above are collected
into the milestone that makes WG10 shippable by someone other than the owner. No separate spec; it points back
at the existing contracts.

**Scope:**
- **Committed determinism golden-baseline:** the regression artifact promised in Phase 5 Slice 3, formally
  checked into git/CI so same-seed/same-coordinate terrain cannot drift across patches (the thing that would
  have caught the B4 z-score regression end-to-end).
- **Locked public API reference:** document and freeze the Facts API, SurfaceDescriptor, and pack format
  (v1→v2 migration path), building on `DESIGN §6.2`.
- **Config validation sweep:** the cross-knob validator with authored, user-readable error messages, only if
  Phases 5–9 leave constraint edges the per-layer loaders don't already cover.
- **Example projects (the one genuinely-new doc deliverable):** a minimal fly-explore demo, a bounded ARPG
  zone, and a voxel-edit planet — each showing how to adapt the framework to a mode + art direction.

**Pillars:** all four; this is the payoff of the "adaptable framework" north star.
**Depends on:** Phases 5–13 (the contracts must be discovered and stable first).
**Spec:** none — tracked here; assembles existing phase outputs.

---

## Pressure-test verdict (2026-05-31)

Each spec was pressure-tested from four independent lenses — pillar alignment, achievability/AAA realism,
long-term/architecture soundness, and "what's missing for AAA" — and the resulting expansions were folded back
into the specs. Summary verdict per phase:

| Phase | Pillar-aligned? | Achievable? | AAA-ready (after expansions)? | Where it needed the most strengthening |
|---|---|---|---|---|
| **10** edit-physics | Yes (no-stall, last-good, determinism, swappable sculpt) | Yes — ~medium-risk, on top of un-shipped Phase 9 | Yes, once the gates exist | Sizing the bake budget (§5.1), the render↔physics sync race (§6), affected-set determinism freeze (§4.2), and naming the Jolt/Godot wake API |
| **11** ecosystem | Yes (one descriptor seam, data-driven, offline-heavy) | Yes — Slices 2–3 are the real risk | Close — depends on near-field grass + cross-fade | Concrete seeded placement/species algorithms, the MultiMesh/SSBO instancing pipeline + temporal-dither cross-fade, grass near-field falloff, family-taxonomy contract |
| **12** water | Yes (seam-safe, no live sim, honest limits) | Yes — *after* a Phase 7B `basin_id` amendment | Close | The lake-fill algorithm + basin identity (reaches back into 7B), the visible==queryable parity audit, the depth-field cost model, the Phase 8 mode-mask contract |
| **13** authored-area | Yes (layers on procedural, reuses Phase 9 store, world-anchored) | Yes — after design-before-slicing gaps close | Yes | Blend-mask perf (precomputed cache, not inline), composition-order determinism under versioning, CPU/GPU composed-parity, three-way seam-continuity split |

Common pattern across all four: the *design direction* was sound and pillar-faithful, but the AAA-critical
parts — concrete perf budgets, parity gates for stochastic/composed fields, and named algorithms/engine APIs —
were under-quantified. The folded-in expansions convert those from "to implementation" hand-waves into bounded,
gated decisions. Two cross-phase contracts emerged and are now documented in both ends: the **Phase 9 ↔ Phase
10 render-settle sync** (edits visible and collidable in the same frame) and the **Phase 8 → Phase 12 mode-mask
interface** (deterministic, world-anchored, fails loud). One **Phase 7B amendment** is now a prerequisite for
Phase 12: emit a deterministic `basin_id` so lakes have a stable identity.

## Horizon backlog (named, not committed — revisit-conditions only)

Reasonable adjacent expansions the phases above imply. Tracked the way `LOOSE_ENDS_LEDGER` tracks TABLED
items: named so they are not forgotten, with a revisit condition, but **not** promoted to phases until a game
demands them. Most are downstream of materials/water/modes existing.

- **Runtime near-field 1–10 m detail (LEDGER B5).** The current clipmap is one coupled 2^L cascade
  (`BASE_SPAN=8192`, `PAGE_PX=256`, one detail-frequency curve). Several phases above *assume* a near-field
  detail capability that the runtime scale rework (Phase 5 Slice 5) only begins. **Revisit:** when a proof-point
  game needs on-foot 1–10 m detail; this is the structural bridge, flag it before relying on it.
- **Weather / atmosphere / sky.** Fog, clouds, time-of-day, precipitation. **Revisit:** after Phase 6 materials
  + Phase 12 water give it surfaces to interact with; it is a look-layer, downstream of height+materials.
- **Climate-driven biome variation.** Temperature/moisture fields that shift biome weights and material/scatter
  response by latitude/altitude. **Revisit:** after the grammar + material packs + ecosystem rendering exist to
  be driven; couples Phase 11 density curves to a climate field.
- **Multiplayer / network determinism.** Same world, multiple clients, coherent edits under latency. **Revisit:**
  only if a game requires it; builds on Phase 9 persistence + Phase 14 determinism baseline; independent of the
  mode and erosion phases.
- **GI / advanced lighting.** Beyond Phase 6's analytic normals + lighting. **Revisit:** when the look bar moves
  past readable-and-correct toward cinematic; a renderer concern more than a worldgen one.

---

## Sequencing summary

Recommended order, each gated on its dependency:

```
Phase 9  (visible editable terrain — last live-roadmap phase)
  └─ Phase 10  Edit physics / voxel loop      ← unlocks SE, highest leverage
  └─ Phase 11  Ecosystem & vegetation render   ← biggest look-win (SotF/D4)
     └─ Phase 12  Water & hydrography          ← needs 7B facts + Phase 8 modes
        └─ Phase 13  Authored-area composition ← needs Phase 8 modes + Phase 9 store
           └─ Phase 14  Productization capstone ← assembles 5–13 into "shippable framework"
                          + Horizon backlog (revisit-conditioned, uncommitted)
```

10 and 11 are independent of each other and can run in either order or in parallel; 11→12→13 has a soft data
dependency chain (descriptor → water consumes facts → authored areas reuse modes+store). 14 is last by
definition. Every arrow is also a gate: do not start a phase until the owner accepts what it depends on.
