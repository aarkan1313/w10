# WorldGen10 — Roadmap

Ordered milestones. Mark `[x]` only when the item meets the definition of done
in DESIGN.md §7.3 (perf gate + visual gate + manual confirmation, as
applicable). Update this file in place; do not create new plan docs.

**Update 2026-06-04:** this roadmap's detailed phase text is historical; the live truth source is
`docs/plans/STATUS.md` top plus
`docs/plans/MOUNTAIN_WORLD_LAYER_RUNTIME_CONTRACT_2026-06-04.md` plus the current
implementation audit
`docs/plans/WG10_IMPLEMENTATION_SPEC_AUDIT_AND_VALIDATION_PLAN_2026-06-04.md`.
Current work is Slice 4 stabilization and recovery:

- The scale-invariant biome producer work is implemented and gated (`flow_max_level`,
  windowed 576 parity, cross-level macro agreement, runtime mode gates).
- `REFERENCE` is the accepted mountain-network visual baseline streamed through the live page pool.
- `MOUNTAIN/network_ref` is now a reference-backed visual bridge
  (`single_mountain_world_layer_reference_bridge`): it uses the accepted payload for
  height/material/facts and matches REFERENCE in the latest capture, but it is **not final
  procedural biome synthesis**.
- Latest architectural checkpoint adds a JSON-ready mountain world-layer tile
  payload/exporter for the future runtime cache/producer contract. It builds on
  `2af7df4 fix(slice4): remove owner fly page settle`
  (`backup-slice4-no-page-settle-20260604-2af7df4`) and
  `067b14b refactor(slice4): expose mountain world-layer runtime tile`
  (`backup-slice4-runtime-world-layer-tile-20260604-067b14b`). The runtime tile
  work separates the accepted mountain world-layer facts/sampling boundary from
  review chunk JSON; the no-settle commit removes the parent-to-fine page fade
  that read as terrain lag/popping during owner fly movement.
- `WORLD` remains diagnostic until multi-biome composition is moved off the synchronous fly stream
  or given a cheaper preview contract.
- The runtime review presentation is now gated against the old accepted static
  mountain-network focus view by terrain silhouette and terrain color distance,
  so matching the footprint while drifting into a wrong/washed-out look is no
  longer allowed to pass `review_runtime_visual`.
- Current source-size audit found no Rust/GDScript/GLSL/Python source file over
  1000 lines in the active terrain/runtime/tooling paths; the next refactor risk
  is ownership and mode taxonomy, not one giant still-unsplit source file.
- Texture/art production is not part of the current acceptance bar. The runtime
  has a simple height/slope palette, debug modes, and low-resolution material
  facts for readability; it does not have final terrain textures. Do not chase
  texture quality before pass-network facts, generated mountain world-layer
  content, and facts/collision parity are proven.
- The latest owner-report fix adds the missing shared fly-camera
  `sync_mouse_from_rotation()` hook so review-camera reframing cannot leave
  stale mouse-look state. The follow-up owner-motion fix changes live clipmap
  binding to toroidal page slots, reducing progression-scene visible repages
  from `72` to `26` and same-frame repage bursts from `18` to `8` with zero
  hide/show/full events.
- The manual owner-stress gate now treats one-frame hitches as first-class
  failures: modes 1/2/3 with morph off/on must keep CPU p99/max and GPU p99 at
  or below `16.7 ms`, while preserving zero hide/show/full events and exact
  bridge captures where MOUNTAIN/network_ref and WORLD preview intentionally
  match REFERENCE. Latest recovery reduced accepted/reference-backed material
  fact page uploads to `page_px / 4` while leaving height full resolution; this
  fixes the strict manual-stress CPU max spike without changing the accepted
  visual bridge.
- `wg10_progression_review.tscn` is now the progression harness for the next
  chat: it replays REFERENCE, MOUNTAIN/network_ref, MOUNTAIN/close_debug, and
  WORLD/reference-preview as explicit steps with `review_progression` guarding
  runtime modes, contract kinds, scripted page-boundary motion, and fixed-camera
  visual repage deltas at L0/L1/L2 page-boundary crosses. The harness now also
  exports a gated feature manifest, per-step `source_display_report`,
  `material_fact_report`, and visible source/display plus material-fact
  overlays. The next pass-network, procedural world-layer, and facts/collision
  steps have explicit gates and promotion rules before work starts.

Next roadmap target: keep the recovered visual baseline stable while converting the
reference-backed mountain bridge into a generated/procedural world-layer producer with the same
pass-network, conditioning, material/fact, and facts/collision contract. The immediate implementation
shape is extending the progression scene in the audit doc, adding one roadmap feature at a time and
proving each layer before promotion. Broad refactors should stay attached to that target.

Last updated: 2026-05-31 (**Phase 5 ACTIVE — see the "▶ YOU ARE HERE" box under the Phase 5 header for the
plain-language current state.** Short version: M0–M4 DONE (engine machinery; gates cargo 121 · fast 6/6 · gpu
4/4 · m3 9/9 · dem_pack pytest 22). Phases 6–9 NOT started (gated on Phase 5). Phase 5 = prove the terrain
CONTENT offline before the Rust port; the geography engine + a frozen keeper exist; the keeper fork was
resolved into A/B/`keeper_v2` selectable variants; the real quality bar became guaranteed **traversability**
(**Tier-3**, current work). Tier-3 detection + verify-first no-op are BUILT + seam-safe (18 offline tests
green); the **carve is blocked** on a seam-stitched connected-corridor fact = the **Phase 7B pull-forward**
(planned escape hatch, not a detour). NEXT: spec connected-corridor routing → build offline → carve → owner
review → unblock Slice 3 (Rust port). Truth sources: STATUS.md top, LEDGER B7/B8, Tier-3 spec
`docs/superpowers/specs/2026-05-31-worldgen-tier3-guaranteed-traversability-design.md`.

[history] 2026-05-30 (**M0–M4 DONE; MAJOR PIVOT — height core being rebuilt.** Owner image/fly review showed
the old height content read blobby/placed/tiling/noisy; root causes rejected: `sample_kernel` tiled DEMs as
the whole height, spectral synthesis discarded phase/structure, scalar warped-noise tuning changed texture not
geography. Phase 5 realigned as an 85%-target geography-engine prototype before any Rust/GLSL port; old
M5/M6/M7 milestones superseded + re-sequenced. Specs: `…/2026-05-30-worldgen10-north-star-vision.md`,
`…/2026-05-30-worldgen-core-design.md`.))

[history] M3 slice 8 (pre-reset) — seam + geomorph + continuity gate; superseded by the reset above when a real fly exposed the multi-level assembly was still broken.

Legend: `[x]` done · `[~]` partially done (note inline) · `[ ]` not started.

---

## Milestone 0 — Project skeleton & rules

- [x] Godot 4.6 project created (`wg-10/`, Forward+, D3D12, Jolt, .NET `wg10`).
- [x] Three living docs created (DESIGN / ROADMAP / STATUS).
- [ ] Addon/folder layout decided (drop-in boundary): one terrain node + one
      config resource, narrow public API.
- [x] Native backend toolchain set up (**Rust GDExtension**, carried forward
      from WG9) and loads in Godot 4.6. (`wg10_terrain` crate builds; `Wg10Hash`
      registers and is callable headlessly — verified 2026-05-28.)
- [x] Test/gate runner skeleton (headless), so gates exist before features.
      (`tools/gate.py --suite fast`; renderer-backed suites come with M3.)

## Milestone 1 — Worldgen core (CPU) + parity foundation

- [x] Port the deterministic formula: hash → noise → region/province → kernel →
      landform, as pure engine-agnostic math. **DONE: hash → value-noise → fbm →
      region/province + family grammar → kernel + landform** (`hash.rs`,
      `grammar.rs`, `npy.rs`, `height.rs`): `height(x,z,seed,pack)` pipeline
      green; bit-exact vs WG9 hash fixture + grammar + height gates green. First
      **real DEM pack** (`packs/dem_v1`) now wired (2026-05-29) — property gate
      + GPU-parity gate green on real 512×512 kernels.
- [x] Terrain-pack format defined and loadable (first pack = DEM/OpenTopo
      kernels). **DONE: format v1 + loader + validation** (`pack.rs`) + **kernel
      loading + `.npy` reader** (`npy.rs`, `pack.rs` loaders `load_pack_with_base`
      / `load_pack_dir`); rejects malformed packs; grammar reads in-memory `Pack`;
      `Pack` carries `FamilyKernel`. **First real DEM pack (`packs/dem_v1`) now
      wired and gated** (2026-05-29): 115-kernel approved map across 12 families,
      built by `tools/dem_pack/` from WG9 shortlist + metric inferences; loads
      through unchanged M1/M2 pipeline; property gate + GPU-parity gate green on
      real 512×512 kernels. Full-set streaming and visual relief/footprint tuning
      are M3 work.
- [~] Parity fixtures (hash, noise, provider decisions, sample grids) committed
      **to git**. **DONE: hash/noise fixture** (`hash_reference.json` vendored);
      provider-decision + sample-grid fixtures come with later layers.
- [x] Determinism gate (same coord → same value across callers/runs).
      (`determinism_check.gd`, in the fast suite.)
- [x] Seam gate including **x=0 / z=0 axis-crossing** exact-zero edges.
      (Rust `value_noise_is_continuous_across_zero_axis` locks floor semantics.)

## Milestone 2 — GPU formula + parity

- [x] GPU compute implementation of the same formula (no readback in production).
      Done: synthetic-kernel formula (hash→grammar→height) ported to GLSL compute
      (`height_field.glsl`), dispatched by `Wg10GpuCompute` (RenderingDevice,
      windowed). Readback exists ONLY in the parity gate (one-off compare), not in
      the eventual render path (M3).
- [x] CPU/GPU parity gate (bit-close; documented epsilon only if profiled).
      Done: Tier-1 family selection EXACT (bit-exact `family_signature` over 576
      coords); Tier-2 height within f32 epsilon (ABS_EPS=1e-2 m, observed max
      delta 7.67e-5 m — 130× headroom). Verified on D3D12/RTX 5090 Laptop GPU.
      `gpu` gate suite runs windowed; `fast` stays headless (now 5 checks,
      fail=0); `gpu` suite now 2 checks, fail=0 (synthetic parity + DEM
      parity). 67 Rust unit/property tests green. M2 is a CPU-math + parity
      milestone — its definition of done is the parity gate, not a visual/fly-test
      gate (that applies to the render pipeline, M3).

## Milestone 3 — Render pipeline at speed (the hard part)

**[~] Slice 1 DONE (2026-05-29):** `Wg10PageCompute` (native Rust class, global
RenderingDevice) runs `height_page.glsl` to write one DEM height page into an
R32F `Texture2DRD` (no readback). `ring_displace.gdshader` samples it in
`vertex()` to displace a flat ring mesh. Result captured to `m3_slice1.png` and
gated by `m3_slice1_check.gd` (`m3` suite, WINDOWED): distinct quantized colors
= 18, nonblack_frac = 1.0. Clear mountain/ridge/valley relief visible. The
Texture2DRD → material → displaced-mesh path is proven. ONE static page, ONE
ring, ONE frame — no streaming, no movement, no multi-ring. M3 milestone OPEN.

**[~] Slice 2 DONE (2026-05-29):** `PagePolicy` (pure Rust, no godot) — the
eviction bookkeeping: fixed-capacity slots, (level,origin)→slot map, LRU order,
protected set. Returns DECISIONS (Reuse/Allocate/AllocateEvicting/Full); owns no
RIDs. 11 headless cargo tests: protected pages NEVER evicted, budget NEVER
exceeded, cache hits reuse the slot, all-protected→Full (no panic), release makes
slot evictable, re-acquire re-protects, `rollback(key)` on producer failure (no
phantom slot, no stale content). `Wg10PagePool` (godot) — THE single owner of all
page RIDs; asks PagePolicy what to do; the ONLY texture_create/free_rid for pages
(3 internal free sites). Eviction reuses the slot's texture (same dims → zero
mid-run RID churn). `Wg10PageCompute` refactored to a stateless producer:
`compute_into_texture` writes height into a pool-provided RID — no longer creates
or owns textures. Slice-1 regression-guarded: m3_slice1_check acquires via the
pool; distinct=18 byte-identical PNG (rendering preserved). New
`m3_pool_check.gd` (`m3` suite, WINDOWED): drives acquire/release on a
capacity-2 pool, asserts RIDs reuse on hit (created stays 2), budget never exceeded
(resident≤2), protected page survives over-budget acquire, Full returns null
(full_events≥1), eviction reuses slot, pooled page renders (distinct=18). m3 suite
now 2 checks, fail=0. Cargo tests: was 70, now 81 (+9 PagePolicy +2 rollback).
Pool driven by explicit acquire/release — NOT a live frame loop. M3 OPEN.

Remaining slices (NOT done):

- [x] `page_scheduler`: velocity-aware stream-ahead, bounded computes/frame,
      coarser-page fallback (never black, never stall). **DONE (2026-05-29, slice 3).**
      `SchedulePolicy` (pure Rust, no godot: `coverage` velocity-led multi-level ring,
      `coarser_fallback` never-black ancestor walk, `plan_frame` bounded
      **coarsest-first** acquire/release — 14 cargo tests incl. a 2000-sample
      never-black property test) + `Wg10Streamer` (godot §5.4 frame-loop driver,
      delegates all math, owns no RIDs) + `Wg10PagePool::resident_keys()` (only pool
      change) + `m3_stream_check.gd` (m3 suite → 3 checks, WINDOWED). Gate passes over
      a 60-frame 6000 m/s sweep: bounded, budget-safe, never-black, deterministic,
      non-vacuous (fallback genuinely fires). Coarsest-first priority + lead/budget
      tuning make never-black STRUCTURAL — the windowed gate falsified the original
      finest-first design (see spec §2.3). Synchronous produce this slice; the
      scheduler↔pool seam is async-ready (zero scheduler change when background
      production lands — trigger = heavy multi-pass pages, M5–M7).
- [x] `clipmap_rings`: fixed concentric rings, persistent meshes, recenter on
      move, shader displace + L↔L+1 morph. **DONE (2026-05-29, slice 4).**
      `ring_geometry` (pure Rust: `RingLayout` level spans + `band_mesh` filled grid /
      hollow ring bands, gapless tiling, 7 cargo tests incl. consistent-winding +
      grid_res%4 guard) + `Wg10ClipmapRings` (godot Node3D — first non-RefCounted class:
      N persistent ArrayMesh children, quantized `recenter` that never rebuilds,
      `bind_page` for per-level height + coarser-neighbor textures; owns no RIDs) +
      L↔L+1 **geomorph** in `ring_displace.gdshader` (blend finer edge toward the coarser
      surface at the same world point, `t=1` at the seam → crack-free; backward-compatible
      no-morph default keeps slice-1/2 gates passing). `m3_rings_check.gd` (m3 suite →
      4 checks, WINDOWED): top-down ortho asserts no holes, real relief, seam continuity,
      morph continuity, recenter-no-rebuild; PNG eyeballed. One-band-one-page binding
      (scheduler radius_pages=0); transient Texture2DRD-second-sampler startup warning is
      benign (render correct, not per-frame).
- [x] **Slice-4 carry-forward fixes — DONE (slice 5a):** (1) geomorph **coarse_origin**
      uniform — `ring_displace` samples the coarse page corner-relative
      `(world.xz − coarse_origin)/coarse_span` (was origin-centered, reopened the seam off
      origin); `bind_page` + the view pass the coarser page's corner. (2) **per-level page
      span** — `Wg10PagePool::acquire_page` dispatches a level-L page over `world_span·2^level`
      (was flat). Both proven by the slice-4 rings gate (distinct=41) under the new
      convention. ALSO landed: read-only `Wg10PagePool::get_resident_page` (+ `PagePolicy::
      slot_of`) — a consumer fetches a resident page WITHOUT triggering compute (the anti-WG9
      render-path rule; the streamer remains the sole producer).
- [x] **3×3 ring tiling + rings↔streamer live wiring — DONE (slice 5b).** Each clipmap level
      is a **3×3 page neighborhood** that surrounds the camera: `Wg10ClipmapRings` rebuilt to
      N levels × 9 one-page tiles (finer-on-top overlap via `render_priority`; gapless by
      construction; per-tile `bind_tile`). `Wg10TerrainView` drives the live loop — per level
      per tile fetch the page via the read-only `get_resident_page` (never computes) + coarser
      fallback, place + bind. Shared page-key `floor(cam/span)·span + (dx,dz)·span` (= the
      scheduler's `coverage(radius_pages=1)`). `m3_view_check` proves it WINDOWED over a
      5-position moving sweep: full coverage (nonblack≥0.98 — surrounds the camera, fixing 5a's
      0.25), no z-fight, never-black, zero view-compute, tile↔page mapping. PNG eyeballed.
      Retired the one-page `m3_rings_check`. Faint tile-edge lines = visual polish (not a gap);
      the overlap overdraw is an explicit input to the p99 acceptance gate.
- [x] Modular harness components: camera/movement, diagnostics/profiling, UI
      overlay (live fps/stats). **DONE (slice 6):** Wg10FlyCamera (free-fly rig),
      Wg10Profiler (frame p99/mean/max ring buffer), Wg10DiagnosticsOverlay (HUD) —
      §6.4 self-contained/narrow/config/composable.
- [x] Manual fly-test scene: WASD + Shift speed + mouse look + Space/C vertical,
      free-fly. **DONE (slice 6):** `harness/m3_review.tscn` — thin assembly of
      {Wg10TerrainView + fly camera + profiler + overlay}; the owner launches + flies it.
      (Ground-follow rig deferred — YAGNI.)
- [x] Renderer-backed acceptance gate: no large black/missing component AND
      **renderer frame p99 < 6 ms**, in motion at ~1000 m/s. **GREEN (slice 7).**
      `m3_accept_check` scripted ~1000 m/s flight, vsync off: **p99=2.41 ms** (budget 6),
      max=3.29 ms, compute-frame max=2.90 ms, render-only ≤2.66 ms, no-black, never-stall.
      A `compute_ms_max<6 ms` ceiling locks in the caching win. (Slice 6 built it RED at
      p99=16.7 ms; slice 7's page-compute caching eliminated the 90 ms spike.)
- [x] **Async / background page production — NOT NEEDED (resolved by slice-7 caching).**
      The slice-6 90 ms "compute" spike was redundant per-page CPU setup (recompile shader +
      re-upload the 25 MB atlas every page — the dispatch is fire-and-forget), NOT GPU-blocking
      or genuinely-expensive compute. Caching the shader+pipeline+buffers once (`PageComputeContext`)
      dropped it to 2.9 ms. Threading would have been the wrong fix. The async-ready seam stays
      available for the future (M5–M7 multi-pass pages may re-fire the trigger with a real
      per-page cost — then it's the lever), but M3 doesn't need it.
- [x] **Visual stability (slice 8) — seam + geomorph fixed; continuity gate added.** The
      owner's first fly found "crazy switching": (1) tile-LOCAL geomorph fired at every one of
      the 9 tiles' edges → now from the 3×3 NEIGHBORHOOD center (engages only at the level's
      true outer ring); (2) fine UV mapped edge vertices onto the texture border → now sampled
      by true world UV (`page_origin` uniform); (3) texel-CENTER page generation left abutting
      pages' boundary samples a texel apart → texel-CORNER (`u=px/(N-1)`) so abutting pages
      SHARE boundary samples (seam zero by construction). `height_at()` unchanged → parity
      intact. New `m3_continuity_check` (windowed) reads back real production pages
      (`seam=0.0`) + a perspective morph-banding ceiling (`jump_frac=0.0`); CAN_COPY_FROM on
      page textures enables readback at no render-path cost. m3 suite 6/6, p99=1.88 ms.
- [x] **RENDER-LAYER RESET (prove-one-at-a-time, owner-flown) — DONE + FOLDED BACK.** The slices
      above stacked without proving live continuity; a real fly exposed a broken multi-level
      assembly, so the presentation was rebuilt step-by-step in `proving_ground.tscn`, keeping the
      proven leaves. Bugs found+fixed: REPEAT sampler → seams (clamp-to-edge); lead unit/clamp
      (lead_seconds + camera-in-ring clamp); morph-off LOD line (wire fine→real coarse parent).
      Steps 1–7 owner-confirmed/probed (1 page · 2-page seam · static 3×3 · streamed 3×3 · coarse
      never-black blanket · geomorph · 3 levels+full speed, probe p99=1.63 ms). **The proven model
      is now FOLDED into the real `Wg10TerrainView` + `Wg10ClipmapRings`** (every level full 3×3,
      hide-on-miss so coarse shows through, morph to real parent, clamp sampler/lead). All gates
      green on the rebuilt path: m3 6/6 (accept p99=3.94 ms), gpu 2/2, fast 5/5, cargo 103.
- [ ] Tune finest-ring spacing + ring count + GRID_RES against real assets (config; no magic
      numbers). The faint mesh-facet creases at grazing angles are a tessellation-density knob,
      hidden for real by M6 normal mapping — NOT a seam.
- [x] **RENDER LAYER STRUCTURALLY DONE (owner-flown).** The reset's proven model is folded into the
      real `Wg10TerrainView`/`Wg10ClipmapRings`; m3_review flies them (5 levels + fog). Post-fold-back
      fixes: custom AABB (frustum-cull of displaced meshes), coarsest hold-last-good (boundary-cross
      blank), and the "loads then unloads" was view-distance > loaded extent (→ 5 levels + matched
      far/fog), not an unload bug (page is always resident when wanted). All gates green. The full
      bug list + lessons live in STATUS (COMPONENT_INVENTORY retired into it). Remaining LOD-detail-
      pop / "squareness" are TEST-RIG SCALE + CONTENT (M6/M7), not render — see below.

> **Diagnosed, fixed ELSEWHERE (not M3):** the "blue squares / hard lines" the owner sees are
> EXTREME DEM DATA — the `dem_v1` pack height field has ~450 m cliffs over 500 m; deep blue is
> real low elevation in the debug color map; the coarse mesh renders a cliff as a flat facet +
> hard edge. The render layer is correct. Fixes belong to the data/material/erosion layers below
> (saner pack relief, M6 materials+normals, M7 erosion), tracked there — do NOT chase in M3.

## Milestone 4 — Facts API (authoritative, sparse) + adaptable edit seam

Brainstormed design 2026-05-30 (spec: `docs/superpowers/specs/2026-05-30-m4-facts-api-design.md`).
The base `Wg10Height::height(x,z)` already exists + is parity-gated — M4 PACKAGES it as the drop-in
Facts API (§6.2), adds collision, and builds an adaptable edit SEAM. Core stays engine-agnostic
(pure Rust; numbers out, no Godot/Jolt dep). The query everything funnels through:
`height = clamp(base + edit_provider.delta(x,z), bedrock_floor, ceiling)` — provider pluggable
(none = 0 cost; one concrete = circular stamps), clamp + bedrock are config (adaptable: no edits /
shallow-to-bedrock / unlimited caves). Built as SLICES (CPU first, GPU bulk last):

- [x] **Slice 1 — CPU seam:** `Wg10Facts.configure` + `get_height(x,z)` = clamp(base + NoEdits, floor,
      ceil). Drop-in RefCounted node, loads its own pack/seed. Gate `facts_check`: no-edit parity
      with `Wg10Height` (Facts can't alter base terrain).
- [x] **Slice 2 — stamps + bedrock:** `StampEdits` (`apply_edit(cx,cz,radius,depth,falloff)` with
      cosine falloff, summed; `clear_edits`) + `set_bedrock(floor,ceil)`. The diggable collidable
      hole; gate asserts dig/clamp/clear.
- [x] **Slice 3 — sparse collision:** `get_collision_field(cx,cz,world_size,samples_per_side) ->`
      PackedFloat32Array (CPU, hot-path, no readback); caller builds the Jolt `HeightMapShape3D`+body.
      Gate `facts_collision_parity_check`: visible(GPU)-vs-collision(CPU) parity on BASE terrain =
      **maxd 0.0009 m** (§4 contract — entities don't float/sink). Edited cells a known collidable-
      not-visible exception until the visible-edits milestone.
- [x] **Slice 4 — GPU bulk bake (off-frame only):** `bake_collision_region(gpu, ...)` — large-area
      collision via `Wg10GpuCompute.heights` (GPU batch) + a DELIBERATE readback; edits/clamp
      composed CPU-side. `bake_*` name + doc = the off-frame contract (never hot-path; the WG9
      readback rule). Gate `facts_bake_check`: GPU bake == CPU collision (maxd 0.0070 m over 33×33).
      **M4 COMPLETE — cargo 115, fast 6/6, gpu 4/4, m3 6/6 all green.**

> **Deferred to M8 (NOT M4):** making edits VISIBLE in the GPU render (the meteor crater you SEE,
> not just collide). That composes the edit delta into the height pages — a render-pipeline change.
> M4 ships collidable-but-not-visible edits; the visible half is **Milestone 8** below, once the
> edit store is proven. (Owner wants editable terrain — meteor/shovel/laser — long-term; M4 built
> the cheap adaptable seam so M8 is low-rework. See M8 for the tracked task list.)

> **The big picture (owner-confirmed intent, 2026-05-29):** the DEM kernels are NOT "the terrain"
> — they are a LIBRARY of real-world landform stamps (extracted from real elevation data). The
> procedural generator arranges them: the GRAMMAR (M1, provinces/palettes) decides WHICH kernel
> families belong WHERE; the HEIGHT FIELD samples + weights them by world position. So WorldGen is
> a procedural generator that speaks in real landforms (infinite, deterministic, geology-grounded
> rather than pure noise). M5–M7 below are the systems that MODULATE/REFINE how the kernels
> combine — and are where the current "squareness / spiky / extreme" look gets fixed.

## ▶ THE FORWARD PLAN (re-sequenced 2026-05-30 after the worldgen pivot)

> The M0-M4 milestones above are DONE + accurate (kept as history). Everything that WAS framed as
> "M5 detail / M6 biomes+materials / M7 erosion" assumed the kernel-as-height architecture that's now
> being replaced; that superseded detail lives in the dated specs + `LOOSE_ENDS_LEDGER.md` and is NOT
> repeated here. This is the clean forward map. Current direction (owner-confirmed): **WorldGen10 is a
> terrain FRAMEWORK, infinite-procedural-first (No Man's Sky reference), adaptable to any game via knobs.**

Legend: `[x]` done · `[~]` in progress · `[ ]` not started. Each phase = its own brainstorm → spec →
plan → slice-by-slice → owner-flown acceptance cycle. Look-quality is owner-judged; gates prove invariants.

### Phase 5 — Worldgen core rebuild (ACTIVE) — 85%-target geography engine

> **▶ YOU ARE HERE (2026-06-02, late) — Phase 5 / Slice 4: ALL 11 BIOMES + COMPOSE run on the GPU,
> hardware-parity-proven; runtime-drainage DECIDED; PART B + drainage build + 4c remain.** Plain-language state
> (STATUS.md top = the live detail; this is the roadmap pointer):
> - **Slices 1–3 DONE** (engine machinery + the CPU Rust port of all 11 seam-safe recipes + array_ops/recipe_noise
>   + compose, parity-exact). **Slice 4 (GPU page integration) is the active body of work.**
> - **Slice 4a + 4b: every accepted biome now GENERATES on the GPU** as a per-biome page pipeline, each
>   parity-exact to its f64 oracle on real hardware (RTX 5090/D3D12, `biome_page` suite green; maxd 3e-7…1.3e-5).
>   Architecture: a generic GLSL pass-MACHINE + per-biome FRAGMENT (concat-selected) + a `Scheduler` dispatch seam
>   + a scratch POOL + small additive flow hooks; volcanic's PCG64 vents computed CPU-side (no RNG in GLSL). The
>   COMPOSE layer (blend/favored/fold) is also GPU-proven (a real flat-field f32 bug was caught+fixed by the
>   windowed gate). cargo 210/0.
> - **Runtime drainage DECIDED (data-grounded).** Live per-page flow is too slow at production scale (576² needs
>   ~192 relax iters = 6.45ms, MEASURED); coarse shortcuts are proven-wrong (~800m valley-misplacement); the exact
>   log-step solver is GPU-heavyweight (parked). Owner priority procedural-first/baking-fine → **on-demand FULL-RES
>   flow bake off the hot frame, per-region drainage-fact cache (rides M3 page-pool LRU), pages sample it, evict
>   far.** Spec `…/specs/2026-06-02-worldgen-runtime-drainage-design.md` (owner-review-pending).
> - **NEXT:** owner reviews the drainage spec → 4b.11 PART B (grammar weight field → compose ACTIVE biomes per
>   page; recommended first — unblocks real multi-biome terrain) → build the drainage subsystem → Slice 4c (flip
>   runtime + remove the 25MB atlas + perf gate + owner fly). Branch `slice4-gpu-page-integration`. Memories:
>   `worldgen10-slice4a-proven`, `worldgen10-flow-convergence-production`, `worldgen10-coarse-drainage-refuted`.
>
> **(history) ▶ 2026-06-01, late — BIOME-COMPOSITION LAYER (Fork B) + SCALE CONTRACT done; Slice-3 UNBLOCKED.**
> Built the edit-free layer turning "grammar places biome(s)" → one seam-exact height (`biome_compose`/
> `biome_registry`/`seam_safe`); all 11 biomes seam-safe in Python (full suite 238); scale contract resolved
> (on-foot real-metre anchor; broad-swell mountains correct, "towering" = future detail layer). Then ported all
> 11 to Rust (Slice 3) + to the GPU (Slice 4, above). Memory `worldgen10-biome-composition-layer`.
>
> **(history) ▶ 2026-06-01 — tunable TERRAIN-EDIT framework BUILT + OWNER-ACCEPTED (traversability is its first use).** Plain-language state:
> - **Milestones 0–4 are DONE** (engine machinery: toolchain, CPU worldgen, GPU parity, render pipeline, Facts
>   API). Phases 6–9 have NOT started — they're all gated on Phase 5 accepting a live height core.
> - **Phase 5 is about the terrain CONTENT** (what the height *looks like*), proven offline in Python before any
>   Rust/GLSL port. Slices 1–2A built the geography engine; **Slice 2A-close froze a keeper** (`keeper_v1`).
> - **The keeper fork (B7):** "rough_highlands" turned out to name 3 different formulas; we built **`keeper_v2`**
>   (best-of-both, seam-exact) and kept A/B/v2 as selectable variants.
> - **The real quality bar = traversability (B8):** owner direction shifted to "guarantee you can cross a
>   barrier region." That's **Tier-3** (current active work, offline Python).
> - **Tier-3 traversability: BUILT.** Connected-corridor router + `carve_ramp` resolve a real mountain wall;
>   connected pass NETWORK runs in the real mountain 9x9 chunk scene (seam-exact by carve-big-field-then-slice).
>   Commits 4252bcd/75dd5fb. (`mountain_synthesis` landed — `MOUNTAIN_BIOME_PROMOTION_2026-05-31.md`.)
> - **Owner flew it → the work GENERALIZED into a tunable TERRAIN-EDIT FRAMEWORK** (owner: "make it tunable, it
>   won't just be used for this" — roads/POIs/rivers/lakes later). An edit = (Placement WHERE + Profile WHAT) →
>   seam-exact world-local delta at the M4 edit-provider seam; edits READ facts, stay separate.
> - **BUILT + OWNER-ACCEPTED (2026-06-01):** `tools/dem_pack/terrain_edits/` (13 tests green) — mountain_trail
>   config (thin Fellowship trails, preserve the mountain) + a placement spectrum (`route_count` spread /
>   `mountain_trail_connected` = 4 arms meeting at a central waypoint → full L↔R + U↔D traversal) + road/river/
>   lake/POI sketches. Wired into the real mountain 9x9 chunk scene (`terrain_edit_chunks_review.tscn`, fly+walk
>   +collision). Owner flew + accepted (connected net geometrically full; ~66% walkable, rest short scrambles —
>   a `depth_cap` tunable). Commits ed7d03b…77c3828. Spec §9:
>   `docs/superpowers/specs/2026-06-01-worldgen-terrain-edit-framework-design.md`.
> - **NEXT (deferred, owner-gated):** flesh out road/river/lake/POI editors as needed; runtime sample/bake split
>   + Rust port (Slice 3, still gated on the accepted live stack); cross-chunk seam-exactness for independent-
>   window streaming. STATUS.md top = live. Memory `worldgen10-tier3-corridor-built-mountain-gap`.

Replaces `height::height`/`sample_kernel` (the tiling) with a deterministic generator, but the target is no
longer "better warped noise." The target is an **85%-class geography read**: at normal game/fly-camera
distances the terrain should read as plausible real geography, with connected ridges, basins, valleys,
drainage-shaped corridors, and local variation that follows landform history. It is not expected to be
indistinguishable from a real USGS DEM under expert GIS inspection.

The structure research and the owner's matrix review changed the order: **do not port the current Slice-2
tuned output to Rust yet.** The distillation tooling is useful, but the current generator basis still reads
as "same noise" to the owner. Phase 5 now uses a stricter geography-engine order: prove a hierarchical
landform/regime prototype in offline images, use real DEM kernels as side-by-side references, then refine the
metric/schema, then port only the owner-accepted stack.

Hard line from the research: local `f(x,z)` can make terrain read as coherent and drainage-shaped, but it
cannot produce globally-correct hydrology. True river/discharge connectivity is Phase 7B, not a promise of
Phase 5. However, if Phase 5 cannot produce a convincing offline geography sheet without a coarse routed
field, **pull the Phase 7B drainage-skeleton design forward before any Rust port** rather than shipping a
local-noise compromise. Phase 5's job is the AAA foundation: seamless, fast, tunable, parity-safe terrain
that reads as a contiguous landmass under flight.

**85% expectation contract:**
- **Green / on track:** a generated sheet has at least one patch the owner reads as real geography, not
  "nice noise"; basins/ranges/valleys/ridges have recognizable logic; no visible straight scaffolding,
  cells, chunks, or repeated stamps; 200 km, 40 km, and close crops all hold together; reference DEM kernels
  are shown beside synth output.
- **Yellow / uncertain:** stills improve but another scale breaks; ridges exist but drainage is decorative;
  biomes differ but transitions feel averaged or pasted.
- **Red / realign:** combo sheets all look basically the same; the best result is only "least bad"; weird
  procedural lines/cells/masks are visible; the argument becomes "with tuning this might work."

**Measurement stack for every accepted offline step:** owner image verdict first, then cheap objective
sanity metrics against real kernels: local relief ratio at multiple windows, slope distribution moments,
curvature-sign balance, ridge/valley spacing, patch-size distribution, drainage/channel spacing where a
network exists, and non-repetition / no-straight-line artifact checks. Metrics guide tuning; they do not
override the owner's visual rejection.

Spec baseline: `docs/superpowers/specs/2026-05-30-worldgen-core-design.md`.
Research extract: `STRUCTURE_AUDIT_EXTRACT.md`.

- [x] **Slice 1 — generator prototype (offline Python).** `worldgen_proto.py` + render images;
      OWNER-ACCEPTED as a direction ("pretty good, a little noisy"; contiguous, no grid/repeat).
- [~] **Slice 2 — biome distillation tooling.** Tooling is BUILT + kept, but the LOOK is NOT accepted.
      Current metrics fixed some dead knobs, but the generator basis still lacks enough structure. Do not
      continue scalar tuning as if this slice is on track; treat it as a useful parameter pipeline waiting
      for a better basis. **SUPERSEDED AS PARAM SOURCE (2026-06-01):** the biome-param SOURCE is now the
      owner-accepted hand-authored biome SYNTHS (`*_synthesis.py`), not DEM-distillation (whose look was never
      accepted). DEM-distillation is kept as a superseded-but-available refinement that can feed the SAME
      `BiomeParams` interface later. See `docs/superpowers/specs/2026-06-01-worldgen-biome-composition-layer-design.md`.
- [~] **Slice 2A — geography-engine prototype (offline Python, render-first).** Replace "try all noise
      combos" with a hierarchical landform composition prototype before any runtime work:
      - explicit coarse landform regimes: basin floor, alluvial fan, foothill, range core, plateau, badlands,
        plain/grassland, and optional glacial/karst families;
      - irregular ridge/uplift skeletons and basin/range frames that cannot reveal straight segment,
        Voronoi-cell, or mask artifacts in the final height;
      - **7B-lite pull-forward (offline proof):** build a coarse world-anchored uplift/ridge skeleton, route
        flow on that coarse skeleton, derive regimes from crest distance / flow accumulation / slope breaks,
        and carve channels causally into the height. This is still Python/render-first only; if it wins, the
        runtime version becomes a real Phase-7B subsystem with fixed flow windows, stitching, and facts/
        collision design.
      - per-regime process/detail: smoother basin fill, fan aprons, rough range cores, incised badlands,
        foothill transition zones, and close-scale detail that follows the coarse structure;
      - DEM-reference contact sheets every run: real kernels beside synth for 200 km, 40 km, and close crops.
      Owner eye decides whether the geography read is worth continuing. A "least bad" sheet is not enough.
      Current checkpoint: skeleton-first v1 was owner-reviewed as **Yellow+ / keep** ("looks pretty good tbh,
      we are getting better"). Skeleton v2 swapped D8-only routing for
      coarse multiple-flow accumulation, separates primary/tributary fields, damps basin/fan incision, and
      makes scenarios alter process weights/widths/smoothing. Owner selected **`SYN rough highlands`** as
      "great"; keep v2 offline and focus the next image work around that process family. Still no Rust/GLSL
      port until the accepted stack has an explicit parity/facts/render story. Next immediate step: a narrow
      rough-highlands focus pass with 200 km, 45 km, debug, and oblique scene-read sheets. This focus pass is
      now rendered. A Godot generated-world review scene (`rough_world_review.tscn`) supersedes the rejected
      tiny-tile comparison and lets the owner switch 90 km generated worlds in-place for scale/detail review.
      First scene review separated two facts: the rough-highlands shape is promising, but the old 128-unit
      block compressed 90 km so aggressively that it could not answer player-scale/traversability questions.
      The review scene now defaults to the owner-preferred 200x horizontal expansion, about a 25.6 km scene
      block, and exposes 10/25/50/100/150/200x presets, independent relief, a `K` relief-policy cycle
      (`k=0` / `0.5` / `1.0`), and a terrain/slope/corridor overlay cycle. Treat scale
      as a required generator/runtime knob, not a baked constant: different games may want different landform
      density, and "too big" can be as bad as "too small" for player pacing.
      Owner scale/traversal rule: tall mountains and high relief are allowed; the required production quality
      is traversable structure, not flattened terrain. Validate valley floors, passes, ramps, shelves,
      basin/fan corridors, and route continuity as explicit acceptance signals.
      Non-visual hardening now includes deterministic export contract tests, skeleton-rough metric reports,
      a traversability/scale audit over the rough-world review mesh, a corridor overlay for low-passable route
      review, and a clean Godot import for the scale/no-fog/default-25km review harness. The audit now separates
      legacy passability from structural corridor quality and adds relief-policy probes: `k=0` is today's
      fixed-height scene behavior where slope falls as 1/span, while `k=1` is a slope-invariant review control
      around the 25.6 km reference span. The same policy probe is now flyable in the Godot review scene.
      Current k=0 read: ~6.4 km blocked, ~12.8 km old-passability
      candidate but structural `thin`, and ~19.2-25.6 km structural candidates. This is still not a visual gate.
      AFK continuity proof now adds a 3x3 generated-world review:
      `tools/dem_pack/export_godot_rough_world_chunks.py` exports two seeded 3x3 sets of adjacent 25.6 km
      chunks to `rough_world_chunks_3x3.json`, and `rough_world_chunks_review.tscn` lets the owner fly across
      those chunk borders. The current payload is `rough_world_chunks_v2_independent_windows`: each chunk is
      generated from its own deterministic world-coordinate skeleton window with a 25.6 km apron, then cropped
      to the authoritative core; a fixed route/corridor mask is exported for visual review, and the scene has
      default-off seam guides (`B`) plus next-seam camera focus (`N`) for deliberate boundary inspection. Current
      non-visual evidence: exact shared-border height continuity and minimum structural-corridor component
      match fraction 0.917. This is still offline Python + static Godot JSON, not a final arbitrary infinite
      streaming architecture or Rust/GLSL port. Proof report:
      `docs/plans/CHUNK_CONTINUITY_PROOF_2026-05-31.md`. The report now quantifies the independent-window
      failure case for the legacy keeper path: separate adjacent 25.6 km windows produce conditioned seam max
      deltas of 0.661 on x and 1.442 on z for seed 133, so a real infinite implementation must remove
      window-local normalization/authority before porting. A wider non-rendered virtual-travel audit now builds
      a 5x5 / 128 km lattice from independent windows for both seeds; 40 seams per seed have height max 0.0,
      corridor min 0.971/1.000, and adjacent max corr 0.341/0.389. This supports the M3-backed streaming
      direction but does not replace a live streaming/cache/player-travel review. A visual seam audit now mirrors the
      Godot review mesh's edge normal/slope/default-color/corridor math and reports zero discontinuity across
      all 3x3 shared edges for both seeds; this is gate evidence only, not owner acceptance. A static contact
      sheet (`rough_world_chunks_review_contact.png`) renders both seeds in terrain, seam-guide, corridor, and
      slope views for quick scan before flying.
      Owner visual seam verdict on the opened Godot scene: seams look good visually. Treat that as acceptance
      of the bounded 3x3 seam-visibility proof only; full terrain/gameplay quality, live streaming/cache,
      player-travel pacing, and Rust/GLSL runtime acceptance remain blocking gates before porting.
      Follow-up correction: the 30x30 static JSON scene is now only a bounded distance proxy
      (`rough_world_distance_proxy.tscn`), not an infinite-world proof. Do not expand static super-windows as
      the roadmap path. The real next implementation target is an M3-backed rough-highlands streaming spike:
      a deterministic world-coordinate provider feeding the existing page/streamer/cache shape, with gates for
      cache-order independence, seams, seed/version determinism, route continuity, no-black, and perf.
- [x] **Slice 2A-close - candidate keeper freeze / implementation bridge.** The rough-highlands keeper is now
      frozen as `rough_highlands_keeper_v1` rather than only review scripts. Contract:
      `docs/superpowers/specs/2026-05-31-worldgen-rough-highlands-keeper-contract.md`. Fixture/export/tests:
      `tools/dem_pack/export_rough_highlands_keeper_contract.py`,
      `tools/dem_pack/fixtures/rough_highlands_keeper_v1.json`, and
      `tools/dem_pack/test_rough_highlands_keeper_contract.py`. It freezes generator version, skeleton facts,
      thresholds, conditioning, scale/relief policy, corridor mask, deterministic sample fixtures, and a
      reproducible contact-sheet hash. This closes the roadmap-adherence gap between "owner likes the
      direction / seams look good" and "there is a precise implementation target." It still does **not** open
      the Rust/GPU port gate; full terrain/gameplay travel acceptance remains open.
      **⚠ DRIFT (2026-05-31):** the frozen formula (`_compose_windowed_height`, call it B) does NOT reproduce
      the shape the owner actually approved on the 90 km `rough_world_review.tscn` scene (`compose_height`, call
      it A). Verified on identical world coords: `corr(A,B) = +0.13` and B relief = 35% of A — B is a different,
      much flatter terrain, only loosely related to A. B is a legitimate seam-safe rewrite (A's per-window
      `_condition()` normalization broke seams), but "owner accepted the direction/seams" silently hardened into
      the frozen *formula* B without re-validating it against A. So "owner likes the direction" must NOT be read
      as "owner approved this height formula." Resolve before Slice 3 (see the Slice 3 block + memory
      `worldgen10-keeper-formula-fork`).
- [ ] **Slice 2A-lite fallback — parity-clean local basis only if useful.** Multifractal weighting,
      stronger recursive warp, ridge/uplift-coupled valleys, and Worley/cellular branches remain allowed as
      components inside the geography engine, but they are not the milestone by themselves. They must serve
      the landform/regime hierarchy and pass the same reference-sheet bar.
- [ ] **Slice 2B — metric/schema correction (offline Python).** Keep the distillation pipeline, but replace
      the weak metric assumptions:
      - verify actual per-family variance of live `anisotropy`; if clustered, stop using it as the primary
        `warp_amount` driver;
      - add cheap geomorphometric metrics that should vary: hypsometric integral/curve moments, slope
        moments, curvature-sign stats, VRM/roughness-at-slope, windowed relief ratio, patch-size/regime
        proportions, and ridge/valley spacing;
      - defer expensive flow-routed metrics (drainage density, slope-area theta, TWI) until there is a
        network/drainage primitive for them to tune;
      - empirically test spline-of-noise control curves before promoting splines to the pack schema.
      Current partial groundwork: the cheap metric comparator now covers the rough-highlands skeleton-focus
      variants as well as the older v5 candidates, but this is a diagnostic report, not Slice 2B completion.
      A schema audit over the approved WG9 kernels now verifies live metric spread:
      `anisotropy` is not dead but should not be the sole `warp_amount` driver, while the current `vrm_7px`
      implementation is effectively dead at this normalization/scale. Reports live in
      `D:\tmp\wg10_geography_engine\geography_metric_schema_audit_*`.
      Validated code-audit precondition: if any `normalized_height.npy` kernel or residual layer survives into
      the new stack, its metres contract must be explicit. Z-score kernels use `height_std_m` (or a rebaked
      documented range), not old `relief_m=height_range_m`; the old gate pack over-amplifies ptp relief by
      3.97-11.16x (median 5.56x).
- [ ] **Slice 2C — gradient/noise feasibility gate (offline + small parity spike).** Only after 2A passes:
      design analytic value+gradient noise with one fade convention across Python/Rust/GLSL. This is the
      prerequisite for IQ/Jordan/Runevision-style slope filters. It is not a free two-line edit.
- [x] **Precondition before the RUST build:** close `LOOSE_ENDS_LEDGER.md` **B1/B2/B3**. DONE + verified:
      cargo 121, fast 6/6, gpu 4/4, m3 9/9 after an editor-closed rebuild. These are in the KEPT
      render/perf foundation and the rebuild sits on them: pool RID cleanup, structural never-black under
      capacity pressure, and terrain-vs-sky/detail-on-off perf gate.
- [~] **Slice 3 — Rust generator core. ✅ UNBLOCKED + IN PROGRESS (2026-06-02).** The port target EVOLVED past
      "v2 keeper": v2 acceptance closed the keeper fork, but the height core is now the **biome-composition layer**
      (11 seam-safe biomes + `compose_biomes`), the accepted offline stack. Contract FROZEN (Slice-3 plan Task 1,
      commit 08adceb): all 11 biomes, real-metre scale contract, GPU-flow gate EARLY. **CPU foundation PORTED +
      parity machine-exact:** `recipe_noise.rs` (worldgen_proto primitives — a DIFFERENT hash than the WG9
      `hash.rs`, ported separately; d2cfd04/6d728ab) + `array_ops.rs` (gaussian_filter mode=nearest + the
      sequential flow_accumulation_mfd; 50c1592). cargo 132 green. **GPU-FLOW GATE PASSED (4b392b6):** the #1
      risk — can the sequential flow sweep run live on GPU? — is retired. Iterative pull-relaxation GLSL compute,
      measured on real hardware = ~1.9 ms/256-page at 128 iters (bit-stable), under the 6 ms budget → drainage
      goes LIVE on GPU, no baked-facts fallback needed. **ALL 11 RECIPE COMPOSITIONS PORTED (b556fa7…1fa2568):**
      every biome's seam-safe apron-grid pipeline in Rust, parity machine-exact vs Python (1e-12..1e-16),
      fixture-gated, wired — full cargo test 148 passed. **NEXT in the port:** port `compose_biomes` + grammar
      biome-weight field → replace `sample_kernel` in `height.rs` → CPU integration parity gate → then GLSL
      (Slice 4). Plan:
      `docs/superpowers/plans/2026-06-01-slice3-rust-port-plan.md`. The older "frozen-stack / don't-port-any-keeper"
      language below is HISTORY — the stack is the biome layer, frozen, and the port is underway.

      [history] Once unblocked: port the owner-accepted Phase-5
      The A/B drift (frozen `keeper_v1` = formula B ≠ owner-approved formula A; `corr(A,B)=+0.13`, B 35% of A's
      relief) has been ACTED ON (2026-05-31): `keeper_v2` (best-of-both: A's regimes on B's seam-safe substrate)
      is BUILT + seam-exact + committed (`tools/dem_pack/keeper_v2.py`, 23 tests); an A|B|v2 in-place switcher
      scene + a Tier-1 traversability gate are committed. Owner direction shifted from "pick one keeper" to
      **keep all three as selectable variants AND pursue guaranteed regime-aware traversability** (Tier-3) as
      the real quality bar — v2 is the current traversability front-runner (the only variant with a crossing
      corridor at play scales; A is too spiky, no crossing route). So the single port target is not yet frozen:
      it depends on the Tier-3 guaranteed-traversability outcome (design approved; **spec written**:
      `docs/superpowers/specs/2026-05-31-worldgen-tier3-guaranteed-traversability-design.md`; next = writing-plans
      → offline Python build; memory `worldgen10-tier3-guaranteed-traversability`). Do not port any keeper as-is
      until that lands and the owner accepts a final stack. (Memory `worldgen10-keeper-formula-fork`, `worldgen10-too-flat-decomposition`.)

      Once unblocked: port the owner-accepted Phase-5
      stack to `height.rs` and replace `sample_kernel`. If the accepted offline result depends on a coarse
      regime/drainage skeleton, design that data model first instead of flattening it into ad-hoc noise.
      Do not include gradient filters unless 2C is green. Do not inherit the old DEM-pack z-score/range bug;
      either remove kernel sampling from the runtime path or gate the correct z-score-to-metres conversion.
      First target should be Rust CPU skeleton facts + composed-height samples against
      `rough_highlands_keeper_v1`, not GPU first. Gates: determinism, boundedness, seam, non-repetition,
      committed Python-vs-Rust sample parity fixtures,
      and a committed/reproducible regression render sheet matching the accepted offline look. These become a
      real Phase-5 gate suite before the port is considered done; current `fast/gpu/m3` gates only prove the
      kept M0-M4/render foundation.
- [ ] **Slice 4 — GPU parity + integrate.** Mirror the accepted Rust generator in GLSL; remove the 25 MB
      kernel atlas from the render path; re-baseline GPU parity; wire render + facts so visible==collision
      still holds; run the hardened GPU-time perf gate.
- [ ] **Slice 5 — live scale tune + owner fly.** Separate two scale knobs and gate both. First, the
      generator/content knob: horizontal landform density is allowed to change the feel dramatically (the
      same 90 km source block at ~6 km vs ~26 km reads like a different place), so final games need this
      tunable. The current rough-world traversability audit supports this split: smaller spans can have many
      passable cells but poor route connectivity, while larger spans connect across the block. Second, the
      runtime-resolution knob: current `BASE_SPAN=8192`, `PAGE_PX=256`, 2^L spans, and
      shader detail frequency are still coupled; reaching a true 1-10 m near-field needs per-level
      span/page-px/detail-frequency policy instead of one global cascade. Tune this without losing
      flight-scale coherence. Confirm seamless biome transitions live. Owner acceptance bar: "Google Maps
      contiguity" with no chunks/squares/lines/repetition and enough structure to stop reading as uniform
      noise, plus traversable valleys/passes/ramps through high relief. Audit the result against all four
      pillars before moving to Phase 6.

### Phase 6 — Materials & surfacing (AAA read, not height cheating)

Start after Phase 5 has an owner-accepted live height core. Do not wait for perfect hydrology before making
the system look like terrain; AAA read comes from height + normals + materials + dressing together. Current
design spec: `docs/superpowers/specs/2026-05-31-worldgen-phase6-surfacing-design.md`. It is design-ready
only; implementation remains blocked until Phase 5 accepts a live height core.
- [ ] Analytic normals from the generated field (or a parity-safe sampled derivative path) → real lighting;
      retire the unshaded debug height color as the review surface.
- [ ] Biome material packs: slope/height/curvature/biome → albedo, roughness, normal detail; swappable via
      config so the framework remains game-adaptable.
- [ ] Surface descriptor seam: one shared descriptor function for slope, curvature, height band, moisture/
      biome hooks. Materials, scatter, and later erosion consume it; do not rederive these in three places.
- [ ] Object scatter/dressing: rocks, talus, vegetation, debris. NMS-class terrain relies heavily on this
      layer. Keep it data/config driven and perf-gated.

### Phase 7 — Erosion and drainage (split local illusion from true structure)

The old "distilled erosion" wording over-promised local operators. The research makes the boundary sharp:
local filters can make drainage-shaped gullies and erosional texture; true river/discharge connectivity needs
upstream area, hence a coarse/global field. Phase 7 is split so we do not confuse the two.

- [ ] **Phase 7A — local drainage-shaped filters.** After Phase 5/6:
      - use the analytic-gradient gate from Phase 5C;
      - prototype IQ/Jordan derivative damping as texture only;
      - prototype Runevision/Phacelle-style slope-aligned gully filtering as a filter over the accepted height
        field, with clear warnings that gullies can dead-end and are not hydrology;
      - port only if CPU/GPU order, hash-grid pivots, gradients, and perf are gated.
      Current design spec: `docs/superpowers/specs/2026-05-31-worldgen-phase7a-local-erosion-filters-design.md`.
      It is design-ready only; implementation remains blocked on Phase 5/6 acceptance and the analytic
      gradient feasibility gate.
- [ ] **Phase 7B — true connected drainage milestone / pull-forward escape hatch.** If Phase 5 cannot hit
      the 85%-class geography read without real routed structure, pull this before the Rust port. Design a
      world-anchored deterministic coarse drainage field:
      - fixed seed/world-anchored flow windows, never camera-relative;
      - deterministic routing/accumulation and seam/stitch strategy;
      - fine pages sample discharge/distance-to-channel and apply local incision;
      - CPU/facts/collision story defined up front.
      De-risk in offline images first. This is a new subsystem, not current clipmap reuse.
      Current non-visual groundwork is captured in the Slice 2A spec port gate: world-anchored skeleton
      windows, apron/stitching, facts queries for skeleton fields, Python-vs-Rust fixtures, GPU sampling, and
      cache/order-independence gates are required before any Rust/GLSL port. An offline Python spike now proves
      the first piece: fixed world-anchored routed skeleton windows with apron-cropped core facts, bounded
      adjacent-window seams, and coarse corridor continuity across neighboring window edges
      (`geography_skeleton_windows.py`, `geography_skeleton_window_seams.{csv,md}`).
      The 3x3 chunk review has now been owner-accepted for seam visibility. The next bridge is reviewing the
      true infinite/player-travel version before any port. That review must cover a real authority-window cache,
      independent requests across authority boundaries, seed/version determinism, cache eviction, route
      continuity beyond 3x3, and gameplay pacing at travel speeds.
      Runtime design spec:
      `docs/superpowers/specs/2026-05-31-worldgen-phase7b-drainage-skeleton-design.md`. It is design-ready
      only; implementation is still blocked on Phase 5 keeper acceptance.
- [ ] **Offline learning is allowed only as a parameter/distillation tool by default.** Learn transfer curves
      or coefficients for analytic operators before considering a runtime neural/stencil path. A page-stencil
      or CNN runtime breaks pure `f(x,z)` and needs apron, seam, parity, and collision plans before it can
      enter the roadmap.

### Phase 8 — Framework modes (the adaptability payoff)

The framework-flex milestones — designed-for via knobs from day 1, built as games need them.
- [ ] Bounded mode (Diablo-style zone): sample a finite region / lock a biome.
- [ ] Island mode (SotF-style): a falloff/island-mask knob over the infinite field.
- [ ] Spherical-planet mode (Space Engineers-style): feed sphere-surface coords to the SAME generator
      (a coordinate-domain swap, not a rewrite — the param-driven design makes this cheap).
- [ ] Handmade / authored-area blending: blend an authored param-set or heightfield into the field
      (same blend mechanism as biome borders).

### Phase 9 — Visible editable terrain (the other half of the M4 edit seam)

M4 made edits COLLIDABLE but not VISIBLE (you fall into a hole you can't see). Make them appear in the
rendered surface. Unchanged from the old "Milestone 8" plan; tracked, built when a game needs editing.
- [ ] Edit store the render side can read (edit texture/SSBO/uniforms; shared, deterministic, parity).
- [ ] Compose the edit delta into the generated height (page-gen or re-bake) → visible==collision on edits.
- [ ] Live edit → bounded/async page refresh (never a hot-path stall — the WG9 rule).
- [ ] Edit persistence (save/load) — optional; the M4 seam isolates the store, so saving is additive.

### After Phase 9 — see `docs/plans/POST_ROADMAP.md`

The forward plan above ends at Phase 9. What comes after — edit-physics/voxel-edit loop, ecosystem & vegetation
rendering, water & hydrography, authored-area composition, and the productization capstone (Phases 10–14), plus
a revisit-conditioned Horizon backlog — is designed in `docs/plans/POST_ROADMAP.md`, with per-phase spec sheets
in `docs/superpowers/specs/2026-05-31-worldgen-phase{10,11,12,13}-*.md`. All design-direction only; every phase
is gated behind owner acceptance of the phases it depends on, and the hard blocker for the whole tail is still
an owner-accepted Phase 5 keeper.

---

## Deferred / tracked follow-ups (not blocking; revisit conditions in LOOSE_ENDS_LEDGER.md)

- [x] **OpenTopo kernel-extraction methodology reviewed** (2026-05-28): sound, cache sufficient. Pack-build
      follow-ups: mask NoData holes; improve family tagging.
- [ ] **Async/background page production** — scheduler is async-ready; build the background producer behind
      `Wg10PagePool::acquire_page` IF the rebuild's per-page worldgen cost (or distilled erosion) blows the
      frame budget. Zero scheduler change required.
- [ ] **`spectral.py`** — kept as a documented NEGATIVE RESULT (spectrum=roughness, discards phase=structure);
      inert (nothing consumes it). Do not delete — it records the lesson.
