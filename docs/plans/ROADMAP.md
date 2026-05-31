# WorldGen10 — Roadmap

Ordered milestones. Mark `[x]` only when the item meets the definition of done
in DESIGN.md §7.3 (perf gate + visual gate + manual confirmation, as
applicable). Update this file in place; do not create new plan docs.

Last updated: 2026-05-30 (**M0–M4 DONE; MAJOR PIVOT — height core being rebuilt.** Latest verified gates:
cargo 121 · fast 6/6 · gpu 4/4 · m3 9/9 · dem_pack pytest 22. M0-M4 (toolchain, deterministic gen, GPU
parity, render pipeline, Facts API) are real + done. **But owner image/fly review showed the old height
content reads blobby/placed/tiling/noisy rather than real geography.** Root causes already rejected:
`sample_kernel` tiled DEMs as the whole height; spectral synthesis preserved roughness but discarded
phase/structure; scalar warped-noise tuning changed texture more than geography. Phase 5 is now realigned
as an **85%-target geography-engine prototype**: hierarchical landform regimes, irregular ridge/drainage
skeletons, DEM-reference contact sheets, and explicit red/yellow/green expectation gates before any Rust/GLSL
port. The OLD M5-detail/M6-materials/M7-erosion milestones are SUPERSEDED + re-sequenced into the new plan.
Current truth: vision spec `docs/superpowers/specs/2026-05-30-worldgen10-north-star-vision.md`, height-core
spec `docs/superpowers/specs/2026-05-30-worldgen-core-design.md`, `docs/plans/LOOSE_ENDS_LEDGER.md`,
STATUS.md top. **NEXT: Slice 2A geography-engine prototype, offline/render-first, with real DEM references.**)

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
      for a better basis.
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
      we are getting better"). Skeleton v2 is rendered and pending owner verdict; it swaps D8-only routing for
      coarse multiple-flow accumulation, separates primary/tributary fields, damps basin/fan incision, and
      makes scenarios alter process weights/widths/smoothing. Owner selected **`SYN rough highlands`** as
      "great"; keep v2 offline and focus the next image work around that process family. Still no Rust/GLSL
      port until the accepted stack has an explicit parity/facts/render story. Next immediate step: a narrow
      rough-highlands focus pass with 200 km, 45 km, debug, and oblique scene-read sheets. This focus pass is
      now rendered. A Godot generated-world review scene (`rough_world_review.tscn`) supersedes the rejected
      tiny-tile comparison and lets the owner switch 90 km generated worlds in-place for scale/detail review.
      First scene review separated two facts: the rough-highlands shape is promising, but the old 128-unit
      block compressed 90 km so aggressively that it could not answer player-scale/traversability questions.
      The review scene now defaults to a 100x horizontal expansion and exposes 10/25/50/100/150/200x presets,
      independent relief, and a slope overlay. Treat scale as a required generator/runtime knob, not a baked
      constant: different games may want different landform density, and "too big" can be as bad as "too
      small" for player pacing.
      Non-visual hardening now includes deterministic export contract tests, skeleton-rough metric reports,
      and an earlier clean Godot import; the scale-control harness edit still needs an editor-closed import
      rerun for a clean native-extension log. Owner visual acceptance remains the blocking gate.
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
- [ ] **Slice 2C — gradient/noise feasibility gate (offline + small parity spike).** Only after 2A passes:
      design analytic value+gradient noise with one fade convention across Python/Rust/GLSL. This is the
      prerequisite for IQ/Jordan/Runevision-style slope filters. It is not a free two-line edit.
- [x] **Precondition before the RUST build:** close `LOOSE_ENDS_LEDGER.md` **B1/B2/B3**. DONE + verified:
      cargo 121, fast 6/6, gpu 4/4, m3 9/9 after an editor-closed rebuild. These are in the KEPT
      render/perf foundation and the rebuild sits on them: pool RID cleanup, structural never-black under
      capacity pressure, and terrain-vs-sky/detail-on-off perf gate.
- [ ] **Slice 3 — Rust generator core, accepted geography stack only.** Port the owner-accepted Phase-5
      stack to `height.rs` and replace `sample_kernel`. If the accepted offline result depends on a coarse
      regime/drainage skeleton, design that data model first instead of flattening it into ad-hoc noise.
      Do not include gradient filters unless 2C is green. Gates: determinism, boundedness, seam,
      non-repetition, Python-vs-Rust sample parity, and a regression render sheet matching the accepted
      offline look.
- [ ] **Slice 4 — GPU parity + integrate.** Mirror the accepted Rust generator in GLSL; remove the 25 MB
      kernel atlas from the render path; re-baseline GPU parity; wire render + facts so visible==collision
      still holds; run the hardened GPU-time perf gate.
- [ ] **Slice 5 — live scale tune + owner fly.** Tune scale toward the adaptable 1-10 m near-field target
      without losing flight-scale coherence. Confirm seamless biome transitions live. Owner acceptance bar:
      "Google Maps contiguity" with no chunks/squares/lines/repetition and enough structure to stop reading
      as uniform noise. Audit the result against all four pillars before moving to Phase 6.

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
      the first piece: fixed world-anchored routed skeleton windows with apron-cropped core facts and bounded
      adjacent-window seams (`geography_skeleton_windows.py`, `geography_skeleton_window_seams.{csv,md}`).
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

---

## Deferred / tracked follow-ups (not blocking; revisit conditions in LOOSE_ENDS_LEDGER.md)

- [x] **OpenTopo kernel-extraction methodology reviewed** (2026-05-28): sound, cache sufficient. Pack-build
      follow-ups: mask NoData holes; improve family tagging.
- [ ] **Async/background page production** — scheduler is async-ready; build the background producer behind
      `Wg10PagePool::acquire_page` IF the rebuild's per-page worldgen cost (or distilled erosion) blows the
      frame budget. Zero scheduler change required.
- [ ] **`spectral.py`** — kept as a documented NEGATIVE RESULT (spectrum=roughness, discards phase=structure);
      inert (nothing consumes it). Do not delete — it records the lesson.
