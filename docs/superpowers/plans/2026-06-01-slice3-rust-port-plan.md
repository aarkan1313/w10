# Phase 5 Slice 3/4 Rust and GPU Port Plan

> Planning only. Do not implement from this document until the offline Fork-B
> biome-composition stack is owner-accepted. This plan does not authorize edits
> to `tools/dem_pack/`, Rust, GLSL, Godot harness scenes, or generated review
> assets.

**Goal:** Port the accepted offline biome-composition stack into the live
WorldGen10 runtime without regressing determinism, CPU/GPU parity, facts/render
agreement, streaming performance, or visual seam quality.

**Dependency:** This plan is ready to execute once the offline biome-composition
layer is owner-accepted. The port target is not "one shared height engine." Per
Fork B, the target is:

- the seam-safe per-biome recipes accepted offline;
- the grammar/biome weight field that chooses and blends recipes;
- the tunable `compose_biomes` blend, with `height_favored` primary and `field`
  fallback unless the accepted offline stack says otherwise;
- any accepted coarse facts needed by those recipes, including drainage/flow
  facts if the offline stack depends on them.

**Existing runtime shape to preserve:** `wg-10/rust/src/hash.rs`,
`grammar.rs`, `height.rs`, and `parity.rs` are pure engine-agnostic math.
Godot-facing classes live behind thin wrappers (`bind_worldgen.rs`,
`facts_api.rs`, `page_compute.rs`, `page_pool.rs`). The current height path is
the old pack/kernel model: grammar family weights feed `height::sample_kernel`,
and the render path caches shader/pipeline/static pack buffers, including the
approximately 25 MB kernel atlas. Slice 3 replaces that runtime formula; Slice
4 removes the atlas from the render path.

---

## Non-Negotiables

- **Rust/GPU first:** Offline Python is look-proving scaffolding. Anything that
  can reasonably run in Rust or on the GPU must be designed for that from the
  start, not left as a Python-side dependency.
- **CPU first, then GPU:** Port the accepted Python formula to pure Rust first,
  with committed Python <-> Rust fixtures. Port to GLSL only after the Rust
  model is deterministic, bounded, and fixture-green.
- **Parity is a contract, not a vibe:** The Rust CPU formula is the authority
  for facts/collision. The GLSL formula mirrors it and is proven by parity
  gates. Any accepted epsilon must be documented in metres and justified by f32
  arithmetic.
- **Seams are visually seamless, not globally bit-exact:** Local stateless
  functions should remain exact at shared coordinates. Global or iterative
  drainage/flow cannot be bit-exact across arbitrary streaming windows. Its bar
  is small visual delta relative to relief, with sub-metre game-scale seam error
  as the initial target.
- **No hidden readback hot path:** GPU readback is allowed for parity gates and
  explicit off-frame bake paths only. Runtime pages must remain fire-and-forget
  compute into pool-owned textures.
- **Do not preserve the old atlas by inertia:** `sample_kernel` and the 25 MB
  kernel atlas are scaffolding from the pre-reset terrain path. If a temporary
  compatibility shim is needed, it must be time-boxed and excluded from the
  final Slice 4 acceptance.

---

## Port Boundaries

### Python -> Rust: CPU Authority

Port these into pure Rust modules first:

- accepted recipe parameter structs and recipe identifiers;
- seam-safe recipe implementations, including noise, warp, ridge, dune,
  glacial, karst, wetland, and rough-highlands primitives as actually accepted;
- `compose_biomes` and `BlendConfig`;
- grammar biome-weight evaluation, or the narrow adapter from existing grammar
  families to accepted biome weights;
- deterministic coarse facts needed by recipes, such as skeleton windows,
  drainage-channel facts, distance-to-channel, catchment/discharge summaries, or
  baked fact sampling;
- bounded height composition used by both render and facts;
- fixture loaders used only by tests.

Keep Rust module boundaries close to the current crate:

- `hash.rs`: keep shared deterministic hash/noise foundations here unless the
  accepted stack needs a new clearly named noise module.
- `grammar.rs`: evolve from family weights to biome/recipe weights, preserving
  bounded fixed-capacity outputs where practical.
- `height.rs`: replace `sample_kernel` composition with the accepted recipe
  dispatch and blend.
- `facts.rs` / `facts_api.rs`: keep sparse authoritative CPU queries pointed at
  the same Rust base height, then compose edits/clamps as today.
- New pure modules are acceptable when they isolate real concepts, for example
  `recipe.rs`, `biome_blend.rs`, or `drainage_facts.rs`.

### Rust -> GLSL: Runtime Bulk Compute

Port the Rust formula to GLSL only after CPU fixtures are green:

- page shader height evaluation for recipe dispatch and `compose_biomes`;
- bounded fixed-size recipe/weight buffers suitable for `std430`;
- any accepted stateless recipe primitives;
- flow/drainage fact sampling if using baked/coarse facts;
- optional live flow accumulation only after the early feasibility gate passes.

The GLSL port should keep the current page path shape: a cached compute context
with persistent static resources, per-page image binding, per-page push
constants, no readback, and page-pool ownership of texture RIDs.

---

## Early De-Risk Gate: Live GPU Flow Accumulation

This gate must run before committing to live routed-drainage generation in the
runtime formula.

**Why:** Flow accumulation is iterative and stateful. It is not comparable to
stateless `f(x,z)` noise. It can require repeated relaxation, ordering,
prefix-style propagation, or multi-pass neighborhood exchange, and can become
the p99 frame-time tail. Do not assume it ports cheaply because noise and ridges
do.

**Question:** Can the accepted drainage/flow requirement be computed live on the
GPU inside the existing streaming page budget?

**Prototype scope:**

- representative page resolution: current `PAGE_PX=256` and one higher stress
  setting if the accepted stack needs it;
- representative clipmap motion: scripted travel at approximately 1000 m/s;
- real page scheduling pressure: same pool/scheduler shape used by M3/M5 gates;
- representative worst-case biome mix: include the most drainage-heavy accepted
  recipe and a transition band;
- enough iterations or passes to match the offline drainage look within the
  visual seam tolerance.

**Pass criteria:**

- real GPU p99 < 6 ms at approximately 1000 m/s, using measured GPU time when
  available;
- no single live-drainage frame exceeds the existing stall ceiling used by the
  hardened perf gate unless the ceiling is explicitly re-baselined;
- did-real-work assertions prove pages streamed, terrain rendered, and drainage
  contribution was present;
- adjacent authority windows produce visually seamless drainage-height effects:
  target sub-metre shared-edge delta at game scale, with failures documented by
  relief-relative magnitude;
- CPU facts and GPU render agree for sampled heights within the accepted parity
  epsilon after drainage is applied.

**Fail criteria:**

- GPU p99 >= 6 ms at the target travel speed;
- drainage compute requires sync/readback on the render path;
- seam deltas are visually obvious or exceed the agreed relief-relative budget;
- the iteration count is data-dependent in a way that makes frame cost
  unpredictable.

**Fallbacks if live flow is too expensive:**

- capped-reach or local-catchment flow: bounded neighborhood, fixed pass count,
  approximate discharge, visually tuned for gullies and connected valleys but
  not claiming global hydrology;
- baked/precomputed drainage facts: world-anchored coarse facts generated
  offline or asynchronously, then sampled by Rust/GLSL at runtime;
- hybrid: CPU/Rust authority-window cache computes coarse drainage facts
  off-frame, GPU pages sample those facts and apply local incision/detail;
- biome-specific downgrade: recipes that need true connected drainage use baked
  facts, while stateless recipes stay fully live.

Do not proceed with a live-flow runtime architecture until this gate has a
recorded pass. If it fails, choose one fallback and update the Slice 3/4 plan
before porting the dependent recipes.

---

## Parity and Acceptance Strategy

### Python <-> Rust Fixtures

Commit fixtures generated from the owner-accepted offline stack:

- fixed seeds, coordinates, and biome-weight cases;
- pure recipe interiors for each accepted recipe;
- transition bands for gentle and clashing biome pairs;
- triple-point or N-way blend cases if accepted offline;
- bounds extrema and representative high-relief samples;
- drainage/flow fact samples if present;
- visual review sheet hashes only as supporting evidence, not as the main
  numeric oracle.

Fixture rules:

- fixture metadata records generator version, recipe names, blend mode, seed,
  coordinate convention, scale, and units;
- fixture samples are world-coordinate based, never window-index based;
- update fixtures only when the accepted offline stack intentionally changes.

### Rust CPU Gates

Required gates before any GLSL work:

- determinism: same `(x,z,seed,config)` returns identical output across runs;
- boundedness: heights, masks, blend weights, and drainage facts stay in
  documented ranges;
- seam behavior: stateless recipes are exact at shared coordinates; iterative or
  global facts meet the visual seam epsilon;
- non-repetition: no obvious tile period or repeating stamp signature over the
  accepted travel scale;
- fixture parity: Python <-> Rust samples green within the documented epsilon;
- facts parity: `Wg10Facts.get_height` and collision grids use the same Rust
  base height and preserve visible==collision on base terrain.

### Rust <-> GLSL Gates

Required gates before integration:

- Tier 1 exact integer decisions where possible: recipe set, dominant recipe,
  grammar/weight signatures, or equivalent stable signature;
- Tier 2 height parity: f32-vs-f64 metre epsilon documented from observed max
  delta, starting from the existing 1e-2 m budget unless the accepted stack
  justifies a different one;
- page parity: read back test pages only in a gate and compare CPU samples at
  texel-corner coordinates;
- blend parity: transition-band samples include the accepted `height_favored`
  behavior, not just pure interiors;
- drainage parity: if using live or baked flow facts, parity includes those
  facts and the final composed height after their effect.

### Visual Seam Bar

Treat seams as a visual and gameplay contract:

- no chunk squares, hard lines, or popping during streaming travel;
- shared-edge height deltas for stateless functions should be zero or f32
  epsilon;
- global-flow effects are accepted if the visible height delta is small relative
  to relief and initially below one metre at game scale;
- route/channel continuity must look connected across authority windows even
  when the exact accumulation counts differ.

---

## Slice 3: Rust CPU Port Tasks

### Task 1: Freeze the Accepted Offline Contract

- [ ] Record the owner-accepted offline commit/fixture version.
- [ ] List recipes included in the first runtime port.
- [ ] Record `BlendConfig` defaults and allowed fallback modes.
- [ ] Record whether drainage/flow facts are required for acceptance.
- [ ] Record all units: normalized recipe height, metres scale, feature span,
  relief scale, and biome-weight conventions.

Exit: a small contract note in the plan or status docs says exactly what Rust is
porting. No code starts before this is known.

### Task 2: Build Python Fixture Export

- [ ] Add fixed coordinate sample sets for recipe interiors and blend bands.
- [ ] Add accepted-stack metadata and expected outputs.
- [ ] Include seam pairs using shared world coordinates.
- [ ] Include stress samples for high relief, transition bands, and drainage if
  present.

Exit: fixtures are committed and reproducible from the accepted offline stack.

### Task 3: Introduce Rust Data Model Behind Tests

- [ ] Add pure Rust config structs for recipes, biome weights, blend mode, and
  runtime scale.
- [ ] Add loader/parsing only if the accepted runtime config needs data files.
- [ ] Keep Godot bindings out of this task.

Exit: unit tests prove configs parse/construct deterministically and reject bad
values.

### Task 4: Port Stateless Shared Primitives

- [ ] Port accepted noise, warp, ridge, mask, remap, and blend helper functions.
- [ ] Lock floor/rem_euclid behavior across negative coordinates.
- [ ] Add tests against Python primitive fixtures where precision matters.

Exit: primitives are deterministic, bounded, and ready for recipe assembly.

### Task 5: Flow-Accumulation Decision Gate

- [ ] If the accepted stack does not require routed flow, record "not required"
  and skip to Task 6.
- [ ] If it does, run the live GPU feasibility spike before designing runtime
  flow into the main formula.
- [ ] Choose live GPU, capped local flow, baked facts, or hybrid facts based on
  the gate result.

Exit: drainage architecture is chosen with a measured pass/fail record.

### Task 6: Port Recipes One at a Time

- [ ] Start with rough-highlands/v2 or the simplest accepted recipe.
- [ ] Then port one clashing/high-structure recipe, for example mountain.
- [ ] Continue biome-by-biome with one fixture gate per recipe.
- [ ] Keep recipe dispatch explicit and bounded; avoid dynamic maps in hot math.

Exit per recipe: Python <-> Rust samples green, bounds green, seam samples green.

### Task 7: Port `compose_biomes`

- [ ] Implement the accepted `field` fallback.
- [ ] Implement the accepted primary blend, expected to be `height_favored`.
- [ ] Add two-recipe and N-way tests, including transition-band fixtures.
- [ ] Add deterministic fold/order tests if N-way blend remains pairwise.

Exit: Rust composed samples match Python fixtures and pure-recipe endpoints are
unchanged.

### Task 8: Replace `height::sample_kernel` as the Runtime Formula

- [ ] Move the old kernel path behind a legacy test-only or temporary feature if
  needed.
- [ ] Route `height::height` through recipe weights, recipe dispatch, and
  `compose_biomes`.
- [ ] Keep `Wg10Height` and `Wg10Facts` public shape stable unless a narrow API
  change is required.

Exit: Rust tests and Godot CPU facts checks use the new formula, not the atlas
formula.

### Task 9: CPU Integration Gates

- [ ] Run cargo tests for pure Rust.
- [ ] Run fast headless Godot checks that exercise `Wg10Height` and `Wg10Facts`.
- [ ] Add a CPU sample-grid regression gate for the accepted stack.
- [ ] Confirm collision/facts sampling uses the same composed height.

Exit: Slice 3 CPU port is complete, but GPU/render integration has not started.

---

## Slice 4: GPU Parity and Render Integration Tasks

### Task 10: Design GPU Buffer Layout

- [ ] Replace atlas buffers with compact recipe, parameter, biome-weight, and
  optional fact buffers.
- [ ] Keep fixed-capacity arrays for hot decisions.
- [ ] Keep push constants limited to per-page origin/span/seed/config IDs.
- [ ] Document std430 layout beside Rust buffer builders.

Exit: CPU-side buffer builder tests lock lengths, offsets, and alignment.

### Task 11: Port Recipe Primitives to GLSL

- [ ] Mirror Rust helper functions in GLSL.
- [ ] Keep integer hash and coordinate-floor semantics aligned with Rust.
- [ ] Add a parity shader for explicit coordinate samples before page rendering.

Exit: CPU/GPU primitive and recipe parity is green for explicit sample coords.

### Task 12: Port `compose_biomes` to GLSL

- [ ] Implement the accepted blend modes.
- [ ] Include transition-band parity samples.
- [ ] Preserve pure recipe endpoint behavior exactly or within f32 epsilon.

Exit: CPU/GPU composed-height parity is green.

### Task 13: Integrate Page Compute Without Atlas

- [ ] Update page compute context to upload recipe/static fact buffers instead
  of the six pack/kernel atlas buffers.
- [ ] Update page shader bindings accordingly.
- [ ] Keep page-pool ownership and cached context lifetime unchanged.
- [ ] Delete or bypass atlas keepalive hacks from the active render path.

Exit: runtime page production no longer uploads or binds the 25 MB kernel atlas.

### Task 14: Page and Facts Parity

- [ ] Read back pages only in a gate and compare CPU samples at texel-corner
  coordinates.
- [ ] Verify `Wg10Facts.get_height` matches rendered base terrain within epsilon.
- [ ] Verify relief scale and edit composition still behave as before.

Exit: visible==collision on base terrain remains true for the new formula.

### Task 15: Hardened Performance Gate

- [ ] Run a scripted approximately 1000 m/s flight with real GPU-time p99.
- [ ] Assert p99 < 6 ms.
- [ ] Assert did-real-work: terrain rendered, pages streamed, recipe/blend work
  contributed, and no atlas-bound compatibility path was used.
- [ ] If drainage is live, include a drainage-heavy travel path and record
  drainage-specific timings.

Exit: Slice 4 is integrated and performance-accepted.

### Task 16: Owner Fly Review

- [ ] Present the live review scene after gates are green.
- [ ] Confirm no visible chunks/squares/lines/repetition.
- [ ] Confirm biome transitions are believable in motion.
- [ ] Confirm traversable/drainage features remain connected enough for the
  accepted gameplay bar.

Exit: owner acceptance, not test success alone, closes the port.

---

## Atlas Removal Plan

The old render path uploads these static buffers from the pack model:

- palettes;
- compatibility offsets and flat compatibility list;
- kernel records;
- kernel parameters;
- kernel data atlas, currently the large buffer;
- grammar constants in push constants.

Slice 4 replaces them with:

- recipe parameter buffer;
- recipe dispatch table or fixed recipe slots;
- biome/grammar parameter buffer;
- optional coarse fact or drainage fact buffers;
- no `KData` atlas binding in active shaders;
- no dummy atlas keepalive binding in production shaders.

Acceptance requires a grep/audit gate:

- no active render shader samples `KData`;
- no active page compute context creates the kernel atlas buffer for the new
  path;
- legacy atlas code, if retained, is clearly named legacy and not called by the
  review/runtime path;
- memory and configure-time logs show the atlas is gone from the active path.

---

## Main Risks

- **Flow accumulation cost:** Highest risk. It is iterative and may not fit live
  GPU budgets. Mitigate with the early gate and fallback to capped/local or
  baked facts.
- **Fork-B complexity:** Recipes may have different primitive vocabularies and
  nonuniform parameter needs. Mitigate by porting one recipe at a time with
  fixtures and explicit dispatch.
- **Blend parity:** `height_favored` uses a local relief proxy. If the proxy
  depends on neighborhood blur or finite windows, GLSL and Rust must share the
  same sampling/apron rule or use a cheaper local proxy.
- **N-way blends:** Pairwise fold order can matter. Gate triple points and
  document the accepted order.
- **Seam expectations:** Stateless functions can be exact; global flow cannot
  always be. Keep the documented visual seam bar visible in tests and owner
  reviews.
- **Facts/render drift:** The GPU may get ahead of CPU facts if drainage or
  blends are implemented differently. Keep CPU as authority and gate page
  readback against it.
- **Performance hidden by empty work:** Use the hardened did-real-work pattern,
  not wall-clock p99 alone.
- **Scope creep:** Do not tune final biome art during the port. Port the
  accepted stack, then tune later under separate owner-reviewed work.

---

## Done Criteria

Slice 3 is done when:

- the accepted offline stack has committed fixtures;
- Rust CPU reproduces those fixtures;
- facts/collision use the new composed height;
- deterministic, boundedness, seam, non-repetition, and recipe/blend tests are
  green;
- the flow/drainage architecture decision has a recorded gate result.

Slice 4 is done when:

- GLSL matches Rust under the parity gates;
- page rendering uses the new formula;
- the 25 MB kernel atlas is absent from the active render path;
- visible==collision base parity still holds;
- hardened GPU p99 is < 6 ms at approximately 1000 m/s;
- owner fly review accepts the live biome-composed terrain.
