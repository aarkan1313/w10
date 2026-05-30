# WorldGen10 — Roadmap

Ordered milestones. Mark `[x]` only when the item meets the definition of done
in DESIGN.md §7.3 (perf gate + visual gate + manual confirmation, as
applicable). Update this file in place; do not create new plan docs.

Last updated: 2026-05-30 (**M0–M4 DONE, all gates green** — cargo 115, fast 6/6, gpu 4/4, m3 6/6. M3 render layer rebuilt prove-one-thing-at-a-time + folded into the real classes (structurally done; bug list + lessons in STATUS). M4 Facts API DONE: drop-in `Wg10Facts` — `get_height` = clamp(base + pluggable edit-provider, bedrock, ceiling), sparse Jolt-ready `get_collision_field` (CPU/no-readback), circular-stamp edit seam (collidable), off-frame GPU `bake_collision_region`; visible==collision parity 0.0009 m. Added **Milestone 8** (VISIBLE editable terrain — the other half of the M4 seam). **NEXT: M5 — detail & masks.** The "squareness/blobby/LOD-pop" remaining are TEST-SCALE + CONTENT (M5–M7), not render bugs; the foundation is AAA-capable, the look is downstream.)

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

## Milestone 5 — Detail & masks (GPU, render-only)

- [ ] Detail/displacement layer (bounded, shader-only, edge-safe). [Fixes the "bare/blobby"
      look — adds the high-frequency detail the raw kernels lack.]
- [ ] Slope/curvature/debug + world-space masks.

## Milestone 6 — Biomes & textures (data-driven)

- [ ] Stable world-space biome/material masks driven by terrain-family rules.
- [ ] Texture/material packs (swappable, like terrain packs). [**Fixes the coarse-mesh FACETS /
      "diamonds" / hard cliff edges** — normal-mapped materials make coarse geometry read smooth,
      the way every terrain renderer does it; also replaces the debug blue/yellow height coloring.]

## Milestone 7 — Erosion & hydrology

- [ ] River/pass routing facts.
- [ ] Erosion/hydrology, integrated without breaking determinism/parity. [**Carves the extreme
      DEM cliffs into believable slopes/drainage** — the data-level fix for the spiky height
      field, complementing a saner pack relief scale.]

## Milestone 8 — VISIBLE editable terrain (the other half of the M4 edit seam)

The M4 Facts API already makes edits (meteor crater / shovel / laser pit) **collidable** —
`Wg10Facts.apply_edit` dents `get_height`/`get_collision_field`, so a body falls into the hole. But
the GPU renderer draws from its own height pages, which DON'T see the edit — so right now **you fall
into a hole you can't see** (a documented, intentional M4 divergence). M8 closes that: make edits
appear in the rendered surface.

- [ ] **Edit store the render side can read.** The M4 edit provider is a CPU `delta(x,z)`; the GPU
      needs the same edits in a form a compute/vertex shader can sample (e.g. an edit texture/SSBO,
      or stamps uploaded as uniforms). Shared, deterministic, parity-preserving.
- [ ] **Compose the edit delta into the height pages.** `height_page.glsl` adds the edit delta when
      it generates a page (or a re-bake pass applies edits to resident pages) so the displaced
      surface matches `get_height` (clamp + bedrock included) — closing the collidable-vs-visible
      gap. Re-uses the M4 `bake_collision_region`/page path; the visible-vs-collision parity gate
      then extends to cover EDITED cells (no longer an exception).
- [ ] **Live edit → page refresh** (only the affected pages recompute; never a hot-path stall —
      the WG9 rule still holds; refresh is bounded/async like normal page production).
- [ ] **Edit persistence (save/load)** — optional sub-item; the M4 seam already isolates the edit
      store, so saving it is additive. Defer until a game needs it.

> Ordering note: M8 depends on the render pipeline (M3) + ideally materials (M6) so the dug surface
> shades correctly. It's listed last because nothing yet CONSUMES visible edits — but it is a real,
> tracked milestone (the owner wants meteor/shovel/laser long-term), NOT a forgotten footnote.

---

## Pre-work follow-up (not blocking M0/M1 doc work)

- [x] **Review OpenTopo kernel-extraction methodology** (done 2026-05-28,
      conclusion in DESIGN §9): methodology sound, cache sufficient. Pack-build
      follow-ups: mask NoData holes; improve family tagging (591/703
      uncategorized).
- [ ] **Async/background page production** (deferred pool-layer follow-up, tracked
      from M3 slice 3): scheduler is async-ready; build the background producer
      behind `Wg10PagePool::acquire_page` when synchronous N-per-frame computes blow
      the frame budget. **Trigger:** heavy multi-pass pages — M5 (detail/normals),
      M6 (biome masks), M7 (erosion/hydrology). Zero scheduler change required.
