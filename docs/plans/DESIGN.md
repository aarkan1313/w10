# WorldGen10 — Design

Source of truth for architecture, rules, and contracts. If something here
conflicts with code, either the code is wrong or this doc is — reconcile
immediately, do not let them drift. (That drift is why WorldGen9 is being
restarted.)

Last updated: 2026-05-29 (M2 era). **⚠️ PARTIALLY SUPERSEDED 2026-05-30 — read this notice first.**

> **HEIGHT-CORE ARCHITECTURE SUPERSEDED (2026-05-30).** §2.1 (Worldgen core) and §3 (Terrain packs)
> describe the original "kernels sampled as the height field" architecture. An owner fly showed that
> produces blobby/placed/tiling terrain; root-caused to `sample_kernel` reading DEMs as TILING textures.
> The height core is being REBUILT as **parameter-driven warped-noise**: distill real DEMs into per-biome
> structural PARAMS → grammar blends them → one warped-noise generator (domain warp + macro fBm landmass +
> ridged ridgelines + carved valleys) makes infinite seamless terrain; kernels become a DNA library, never
> sampled (no tiling). The CURRENT architecture is in `docs/superpowers/specs/2026-05-30-worldgen-core-
> design.md` + `…/2026-05-30-worldgen10-north-star-vision.md` (WG10 = a terrain FRAMEWORK, infinite-
> procedural-first). The `hash→noise→region/province→kernel→landform` flow in §1/§2.1 is replaced by
> `hash→noise→grammar(blends biome params)→warped-noise generate`. **STILL ACCURATE + KEPT** (do NOT treat
> as superseded): §1 the four pillars · §2.2 Facts API · §2.3/§5 the render pipeline · §2.4 the CPU/GPU
> parity split (the new generator must still honor it) · §4 determinism/parity contracts · §6 config/drop-in
> · §7 rules. The framework also now targets multiple game modes (infinite/bounded/spherical/handmade) via
> knobs — see the vision spec. This DESIGN doc will be rewritten to fold in the new core once the rebuild
> lands; until then, treat §2.1/§3 as historical and the two 2026-05-30 specs as the height-core truth.

---

## 1. What this is

A clean-restart, data-driven procedural terrain generator and GPU renderer.
The successor to WorldGen9. It is a **new codebase** that ports WorldGen9's
hard-won *knowledge* — the deterministic formulas, the contracts, and the
lessons — but not its code. WG9 lives at `d:/workflows/worldgen9` as a
reference to read, not a dependency.

Project root: `d:/workflows/worldgen10`
- `wg-10/` — the Godot 4.6 project (Forward+, D3D12, Jolt Physics, .NET `wg10`)
- `docs/plans/` — the three living docs (this file, ROADMAP.md, STATUS.md)

### The four pillars (in priority order, per the project owner)

1. **Adaptable / tunable** — easy to retune terrain feel and easy to drop into
   any game project. This is the top pillar.
2. **Performance** — must hold a high movement speed (target ~1000 m/s with
   heavy game overhead) with no stalls.
3. **Quality** — no black holes, no popping-to-nothing, graceful degradation.
4. **No shortcuts** — choose the durable answer, not the expedient one.

Every design decision below traces back to these.

---

## 2. Architecture: three layers, strict boundaries

```
+- WORLDGEN CORE (deterministic, engine-agnostic) ------------------+
|  hash -> noise -> region/province -> kernel -> landform           |
|  ONE formula. Pure functions of (world_x, world_z, seed, pack).   |
|  Reads a swappable TERRAIN PACK (data). Implemented twice,        |
|  parity-gated: CPU (native) + GPU (compute). Same math both sides.|
+----------------+---------------------------------+----------------+
                 |                                 |
   +-------------v--------------+   +--------------v-----------------+
   | FACTS API (CPU / native)   |   | RENDER PIPELINE (GPU)          |
   | sparse, authoritative:     |   | dense, render-only:            |
   |  ground height @ point     |   |  unified clipmap rings         |
   |  collision field @ area    |   |  resident page pool            |
   |  save / edit layer         |   |  stream-ahead scheduler        |
   |  determinism / parity      |   |  GPU height/normal/detail      |
   |  what GAMEPLAY reads        |   |  shader displace + LOD morph   |
   +----------------------------+   +--------------------------------+
```

Layers communicate through narrow interfaces. The render pipeline can be
replaced without touching worldgen; each layer is testable alone.

### 2.1 Worldgen core

- Pure deterministic math. No Godot dependency in the core formula, so it is
  portable and unit-testable in isolation.
- A single source of truth for terrain *shape*. The CPU and GPU
  implementations of the formula must agree (parity-gated, see §4).
- **Data-driven via terrain packs** (§3). The core knows how to *consume* a
  pack; it bakes in no assumptions about a specific data source.

### 2.2 Facts API (CPU / native, authoritative)

- Answers only the **sparse** questions gameplay actually asks:
  `get_height(x, z)`, `get_collision_field(area)`, save/edit reads.
- Authoritative source of truth for anything the simulation reads back:
  collision, physics (Jolt `HeightMapShape3D`), AI/nav ground facts, save.
- Cheap *because* it is sparse: it computes the handful of points requested,
  never the visible world. No readback from the GPU in any hot path.

### 2.3 Render pipeline (GPU, render-only)

- Produces the dense visible terrain. Never authoritative.
- Detailed in §5. The one inviolable property: **it never blocks a frame and
  never shows black/holes** — it degrades to coarser-but-valid instead.

### 2.4 The CPU/GPU split rule

> GPU is the default for ALL render-consumed terrain data — dense, parallel,
> no-readback, streamed ahead (height pages, normals, morph, detail, masks).
> CPU computes only the sparse authoritative facts gameplay reads. Both sides
> run the same deterministic formula and are parity-gated.

Do not force inherently-serial or readback-dependent work onto the GPU (that
is what made WG9's near path cost 128 ms/chunk). Do not make the GPU
authoritative for facts the simulation reads.

**M2 status (2026-05-29):** The same deterministic formula (hash→grammar→height)
now runs on both CPU and GPU via a **GPU-portable integer hash**
`hash::stable_hash_ints(salt: u32, &[i64]) -> u32` — pure u32-wrapping FNV-1a
fold, bit-identical on CPU and GLSL `uint`. A hand-ported GLSL compute shader
(`height_field.glsl`) is dispatched by `Wg10GpuCompute` (RenderingDevice,
windowed — headless RenderingDevice returns null on this D3D12 setup).

**CPU/GPU parity verified on real hardware (D3D12, RTX 5090):** Tier 1 — family
selection matches EXACTLY (bit-exact `family_signature`); Tier 2 — height within
f32 epsilon (observed max delta 7.67e-5 m; ABS_EPS=1e-2 m tolerance, 130x
headroom). The parity gate (`gpu_parity_check.gd`) **reads back** GPU output for
comparison; this readback exists ONLY in the gate (a one-off diagnostic compare).
**Production render streaming is no-readback — that is the M3 render pipeline.**

**Grammar-rolls integer-hash refactor:** the 5 roll sites in `grammar.rs` were
switched from string-join hashing to `stable_hash_ints` with distinct integer
salts. Consequence: grammar rolls are a new seed-space (accepted; WG10 grammar was
never a WG9 parity contract). The WG9-bit-exact bedrock (`hash_grid` / `value_noise`
/ `fbm` vs `hash_reference.json`) is **untouched**. All grammar property tests still
pass unchanged.

---

## 3. Terrain packs (the adaptability mechanism)

Worldgen shape is **data-driven**. A *terrain pack* is the swappable data that
defines what kind of world the generator produces.

- **First pack: DEM / OpenTopo kernels** (ported from WG9's processed kernel
  caches, pending a methodology review — see ROADMAP).
- **Future packs are an explicit goal**: other planets, cellular structures,
  plantlife, brain scans — any source of height/structure data. The core must
  treat the source as swappable data **from day one**. Baking OpenTopo
  assumptions into the core is a design violation.

A pack defines (at minimum): the kernel/source data, landform rules, region
grammar, height ranges, and the parameters the formula consumes. Swapping the
pack changes the world; the renderer and core code are unchanged.

**Terrain-pack v1 is now defined and loadable (2026-05-28).** Schema string
`worldgen10.terrain_pack.v1`, version field 1. The loader validates on load and
**rejects malformed packs** with descriptive errors — bad schema string,
unsupported version, `region_size_m <= 0`, `province_size_regions <= 0`, palette
pct sum > 100, empty palettes, palette with != 3 families, palette referencing an
unknown family. No silent defaults. The grammar reads an in-memory `Pack` struct;
it **never parses JSON** (the loader owns deserialization). `FAMILIES_PER_PALETTE
= 3` is fixed — a palette with a different count is a load error. This expresses
interface constraint #1 (bounded, GPU-shaped output) in data: the grammar always
produces weights across exactly 3 families, which is what the GPU kernel side
expects. Height-relevant family fields (height ranges etc.) are present in the
pack schema but are **not loaded yet** — the height plan loads them.

**Kernel loading added (2026-05-29, still schema v1, additive).** Each family MAY
carry `kernel` (relative path), `relief_m`, and `footprint_m`; `grammar_constants`
gains `moderation_min`/`moderation_strength` (serde-defaulted 0.4/0.5, validated
in range). Kernel data is loaded by a pure-Rust NumPy-v1.0 `.npy` reader
(`npy.rs`): parses C-order `<f4`/`<f8` 2-D arrays into a `Kernel{rows,cols,data:
Vec<f32>}`; rejects bad magic, version≠1, non-float dtype, Fortran order, non-2D
shape, zero dims, and overflowing shape — descriptive errors, no silent defaults.
`Pack` carries `family_kernels: BTreeMap<String, FamilyKernel>` (loaded
array + relief + footprint) accessible via `family_kernel(id)`. New loaders
`load_pack_with_base`/`load_pack_dir` resolve kernel paths relative to a base dir
and validate ABSOLUTE or `..`-traversing paths as errors; `load_pack_str` stays
kernel-free (grammar path only). Kernel loading is **opt-in per family** — a
`{}` grammar-only family still loads. When loading WITH kernels, the pack rejects
any palette referencing a kernel-less family.

**First REAL DEM terrain pack built and wired (2026-05-29).** Pack at
`wg-10/worldgen_terrain/packs/dem_v1/` (`terrain_pack.gate.json` + `kernels/*.npy`).
Built by `tools/dem_pack/` (Python) from WG9's 602-kernel user shortlist and WG9's
metric-driven family inferences (`kernel_inferred_tags.json`); the Rust crate is
**unchanged** — the real pack flows through the existing M1/M2 loader/grammar/height
interfaces. Key design points:

- **Approved family map:** `tools/dem_pack/kernel_family_map.approved.json` — 115
  kernels across 12 families (coast, badlands, grassland, karst, glacial, mountain,
  rainforest, desert, volcanic, wetland, temperate, tundra), 6–13 each. One kernel
  per family in the grammar; grammar groups families into 3-family palettes by
  terrain type (unchanged `FAMILIES_PER_PALETTE = 3`).
- **`relief_m`** = `height_range_m` from the kernel's own stats (real elevation span
  of that DEM, ~990–2765 m). **`footprint_m`** = `approx_sample_spacing_m × sample_px`
  (real ground footprint, ~50 km). A `footprint_scale` knob exists for visual tuning
  at M3 — not yet tuned; the renderer does not exist yet.
- **Kernel normalization:** kernels are **Z-SCORE normalized** (mean 0, std 1) per
  WG9 dem_factory. Height output legitimately goes negative and can exceed `relief_m`
  — this is NOT a bug; the generator uses `relief_m` as amplitude and the Z-score
  distribution spans roughly ±3σ. Do not assume height is in [0,1].
- **Build-time spike filter:** `build_pack.py` drops any kernel whose normalized
  Z-score exceeds `MAX_ABS_ZSCORE=12` (corrupt spike pixels, not real terrain — e.g.
  Mekong delta z=44, Sahel Chad z=14, South Georgia z=12 were dropped; those would
  have injected ~kilometer height artifacts). Dropped at build; the gate subset does
  not contain them.
- **Committed subset vs. full pack:** the gate-committed subset (`terrain_pack.gate.json`)
  covers the kernels needed by the property + GPU-parity gates. The full ~115-kernel
  set is generated on demand via `build_pack.py`; it is NOT committed (large .npy
  files). Manual thumbnail review of the tagging was DEFERRED — tooling (`review_tags.py`
  HTML/CSV artifact, `--reseed` knob) remains available.

---

## 4. Determinism & parity contracts (ported from WG9, non-negotiable)

These held in WG9 and must hold here.

- **Determinism:** `height(world_x, world_z)` returns the same value for the
  same `(x, z, seed, pack)` regardless of which ring/page/query asks, across
  runs. No per-chunk normalization, no per-grid mean subtraction, no per-call
  min/max.
- **Seam-exactness:** adjacent coverage sampling a shared world coordinate must
  agree to **exact zero** delta. Includes seams crossing the `x=0` / `z=0`
  axes (the floor-vs-truncate failure mode — WG9 had a coverage gap here that
  was later closed; cover it from the start here).
- **CPU/GPU parity:** the GPU formula must match the CPU formula bit-closely
  (a small documented epsilon only if profiling proves it necessary). A parity
  gate enforces this; if they diverge, the render and the facts disagree, which
  is a correctness failure.
- **Parity fixtures:** keep reference fixtures (hash, noise, provider
  decisions, sample grids) as the lowest-level regression target, as WG9 did.
  These reference files are **tracked in git** (WG9's mistake was gitignoring
  its ground-truth fixtures under `factory/`).
- **Rendered surface vs. collision parity:** the GPU-displaced surface you SEE
  and the authoritative CPU height you COLLIDE against (Jolt `HeightMapShape3D`)
  must agree closely enough that entities neither float above nor sink into the
  visible ground. Because both derive from the same formula (§2.4) this should
  follow from CPU/GPU parity, but it is called out as its own contract because a
  violation is felt directly in gameplay. A gate samples visible-vs-collision
  height deltas.

---

## 5. Render pipeline internals (the part WG9 got wrong)

Three cooperating units, each its own file.

### 5.1 Clipmap rings (`clipmap_rings`)

- A **fixed** set of N concentric square rings (≈6–7) centered on the camera.
  Ring 0 = finest spacing, each outer ring doubles spacing and area; the
  outermost reaches the horizon. There is no separate "near" vs "far" system —
  one unified clipmap covers all distances.
- **Finest-ring spacing and ring count are config values, NOT locked here.**
  They set the high-detail radius around the camera and the detail falloff with
  distance. The right values depend on the eventual asset/texture scale, which
  does not exist yet — so they are deliberately left as a tuning decision (see
  §9) and must be driven from config, never hardcoded. Doubling-per-ring is the
  default falloff shape; that too can be revisited if review shows the near
  detail zone is too small.
- Each ring is a **persistent flat mesh** created once, never rebuilt.
- Movement = rings **recenter** (translate, quantized to ring spacing), not
  rebuild. Recenter is a cheap position update plus a "these page slots
  changed" notice to the scheduler.
- Because ring count is fixed, render cost is **constant regardless of view
  distance or speed** — this is what makes the 1000 m/s target tractable.

### 5.2 Resident page pool (`page_pool`)

- A bounded pool of GPU-resident height + normal textures (`Texture2DRD`),
  keyed by `(level, page_origin)`. Fixed memory budget; LRU eviction; pages
  currently sampled by a ring are protected from eviction.
- **Single owner of all page RIDs.** One place creates, one place frees. (WG9
  scattered RID lifecycle across files, which produced a dead-RID "black far
  ring" bug when one path freed textures another still referenced.)
- Pages are written *into* the pool by the producer (GPU compute default,
  native worker fallback). The shader samples the page that owns the world
  position under each ring vertex.

### 5.3 Stream-ahead scheduler (`page_scheduler`) — the unit WG9 lacked

- Each update, given camera position **and velocity**, compute the pages the
  rings will need soon (current coverage + a velocity-biased lead margin).
  Diff against the resident set.
- Enqueue missing pages by priority (nearest + most-ahead-of-motion first) and
  **dispatch a bounded number of page computes per frame** (e.g. ≤2). This caps
  per-frame production cost no matter how fast the camera moves. Completed
  pages drop into the pool asynchronously.
- **Graceful fallback (the core anti-WG9 guarantee):** if a ring needs a page
  not yet resident, it samples the best available **coarser** resident page —
  the next ring out always covers the area. Result: briefly lower-detail but
  correct terrain. **Never black, never a stall.**

### 5.4 The frame loop (trivial and bounded by construction)

```
each frame:
  rings.recenter(camera_pos)              # cheap translate
  scheduler.update(camera_pos, camera_vel) # enqueue + dispatch <= N computes
  page_pool.commit_completed()            # attach finished pages
  # rings render: sample resident page, else coarser fallback; shader morphs L<->L+1
```

No synchronous page creation. No per-chunk work. Worst case under impossible
speed: briefly coarser terrain that then refines — frame time stays bounded.

### Why this fixes WG9 concretely

WG9's 128 ms came from creating a page *inside* the build-during-motion path,
synchronously, per chunk. Here, page creation is decoupled into a rate-limited
background dispatch, and the renderer is allowed to show coarser-but-valid data
while it catches up. The two things WG9 structurally could not do — "don't
block the frame" and "don't go black" — are both guaranteed here by design,
not by tuning.

---

## 6. Configuration & transferability (top pillar, by construction)

### 6.1 One config resource, no magic numbers

Every tunable lives in **one typed config resource** editable in the Godot
inspector or a file: ring count, spacing, page budget, view distance, worldgen
params (seed, pack, landform scalars), movement feel. Code reads values **only**
from config — no scattered constants. (WG9 started this with quality profiles
then hardcoded values around them; the rule here is config is the *only* source
of those values.)

### 6.2 Clean drop-in boundary

The whole system is **one node + one config resource**. Public API is tiny:

- `set_config(config)` / inspector-editable config
- `get_height(x, z) -> float` (authoritative)
- `get_collision_field(area) -> ...` (authoritative, Jolt-ready)

A game using it never touches internals. The addon folder can be copied into
any Godot 4.6 project and work.

### 6.3 Engine-agnostic core

The worldgen formula has no Godot dependency, so it is portable beyond Godot
and trivially unit-testable.

### 6.4 Modular harness components (camera, movement, diagnostics, profiling, UI)

The same drop-in discipline as §6.2 applies to **everything around** the terrain,
not just the terrain node. The review scene (§7.4) is the *first consumer* of
these, but it must only **assemble** them — it must not *contain* their logic.

Each of these is its own self-contained Godot component (addon/scene + script)
with a narrow interface and **no project-specific dependencies**:

- **Camera** — free-fly + optional ground-follow rig.
- **Movement / controller** — WASD + Shift speed + mouse look + Space/C vertical;
  control bindings are config, not hardcoded (touch-mappable, per §7.4).
- **Diagnostics overlay** — fps + the few stats that matter; reads stats through
  a narrow interface, knows nothing about terrain internals.
- **Profiling** — frame-time/p99 capture; a component anything can attach, not
  wired into one scene.
- **UI** — overlay/HUD chrome, decoupled from what it displays.

Rules for all harness components (so they transfer scene-to-scene and project-to-
project, like the terrain addon):

1. **Self-contained:** drop the component's folder into any Godot 4.6 project and
   it works; no hidden dependency on this project's autoloads, paths, or other
   components. Each usable in isolation.
2. **Narrow interface:** a component exposes a small typed surface (signals /
   exported config / a couple of methods). Consumers wire to that, never to
   internals. Diagnostics/profiling consume **data through an interface**; they do
   not reach into terrain or renderer internals.
3. **Config-driven, no magic numbers** (§6.1): movement feel, camera speeds,
   key/touch bindings, overlay layout all live in config.
4. **Composable, not coupled:** components do not depend on each other. The review
   scene assembles {terrain node + camera + movement + diagnostics + profiling +
   UI}; remove or swap any one without breaking the rest.

This is the §1 pillars (adaptable / drop-into-any-game) applied to the harness, so
the tooling is reusable and the review scene stays a thin assembly point. **Built
when the review scene is built (M3), not before** — captured here so M3 cannot
quietly grow a monolithic, project-locked scene.

---

## 7. Rules (lessons baked in from WG9)

1. **Separation of concerns; ~600-line soft cap per file.** Many small focused
   units with clear interfaces. Large files only when something is genuinely
   unsplittable. No 3000-line files. (Helps both engineering and LLM
   readability.)
2. **Exactly three living docs:** DESIGN.md, ROADMAP.md, STATUS.md. Updates go
   *into* these three. **No new standalone plan docs.** (WG9 drifted into ~20
   contradictory docs.)
3. **Definition of "done" = perf gate + visual gate + manual confirmation.**
   - A renderer-backed gate must prove, in motion at target speed (~1000 m/s),
     BOTH: no large black/missing region in captured frames, AND **renderer
     frame time p99 < 6 ms** (aggressive on purpose — leaves ~10 ms of a 60 fps
     frame for game logic/overhead on top). This is the renderer's own budget,
     measured in the review scene before game systems are added.
   - The project owner's **manual live-fly confirmation** is the final
     acceptance authority (automated vision is a regression catcher, not the
     judge — scene/camera variability vs real interaction makes it untrusted as
     the sole signal).
   - **Counter-only gates (residency, queue depth, missing-count) NEVER count
     as acceptance.** WG9's core failure was green counters over a black,
     5 fps screen.
4. **The manual review scene is the acceptance surface.** It must be
   human-controllable with: WASD move, Shift speed-up, mouse look, Space = up,
   C = down, and a live diagnostics overlay (fps + the few stats that matter).
   Phone/touch is a target to keep in mind (don't design controls that can't map
   to touch) but is not a first-slice requirement. Free-fly + optional
   ground-follow. **The scene only *assembles* modular harness components
   (camera, movement, diagnostics, profiling, UI) per §6.4 — it must not contain
   their logic, so each transfers scene-to-scene and project-to-project.**

---

## 8. Build order

Proves the hard part (the render pipeline at speed) before building everything
on it. Detailed milestones live in ROADMAP.md.

1. Worldgen core (CPU) + parity fixtures + seam/determinism gates.
2. GPU formula + CPU/GPU parity gate.
3. Render pipeline: page pool + stream-ahead scheduler + clipmap rings +
   manual review scene + diagnostics overlay. **Acceptance: fly ~1000 m/s with
   zero stalls and zero black/holes, manually confirmed.**
4. Facts API (sparse height query, collision field, Jolt integration).
5. Detail / displacement / masks (GPU).
6. Biomes / textures (data-driven, stable world-space).
7. Erosion / hydrology.

Each step is not "done" until it meets §7.3.

---

## 9. Open items / follow-ups

- **OpenTopo kernel methodology review — DONE 2026-05-28.** Verdict: the
  methodology is sound and the processed cache is **sufficient to build the new
  pack without re-touching the 80 GB raw DEMs.** Pipeline (`tools/dem_factory/
  dem_factory.py`) reads GeoTIFFs at reduced resolution (bilinear), separates
  macro (low-pass) from residual (detail) height, normalizes per-kernel, and
  writes per-kernel artifacts: `height_m.npy` (raw), `normalized_height.npy`,
  `residual_m.npy`, previews, and a rich stats block (slope percentiles,
  curvature, ridge/valley density, orientation, anisotropy, roughness, quality).
  Findings (none blocking):
  1. NoData pixels are mean-filled then treated as real terrain (flat plate);
     `coverage_fraction` flags it. Checked: of 703 accepted kernels only 2 are
     <0.99 coverage, both `uncategorized`. Non-issue for curated families. New
     pipeline should still mask holes properly / require coverage >= ~0.99.
  2. `approx_sample_spacing_m` uses the coarser axis for non-square tiles, so
     slope/curvature are slightly off there. Normalize spacing properly next
     time.
  3. Curvature is a raw pixel-unit Laplacian (comparative, not metric). Fine for
     ranking; don't treat as physical curvature.
  4. `normalized_height` is per-kernel mean/std (correct for a reusable kernel;
     the generator must re-impose its own amplitude. Raw height is retained, so
     nothing is lost).
  - **Curation gap (matters for the pack):** 591 of 703 accepted kernels are
    `uncategorized`; only ~112 are family-tagged (coast 14, volcanic/badlands 12
    each, karst 11, grassland/rainforest/glacial/mountain ~10, tundra/temperate
    1 each). The data is good but family tagging is incomplete and a few biomes
    are thin. Improve tagging / fill thin families when building the pack.
- ~~Set the concrete frame-time budget.~~ **Decided: renderer p99 < 6 ms at
  ~1000 m/s** (§7.3).
- ~~Decide native backend language.~~ **Decided: Rust GDExtension** (faster;
  proven toolchain carried forward from WG9).
- **Tune finest-ring spacing + ring count** (§5.1) once real assets/textures
  exist to judge the near-detail radius against. Left as config; do not guess a
  locked number now.
- **Family-source seam (M6 inheritance constraint):** `families_for_region` in
  `grammar.rs` is the **single narrow function** that M6's continuous
  climate-field source replaces. The blend math (`family_weights` corner blend,
  normalization, seam-continuity) must not change when that replacement happens —
  only the source of family ids fed into it changes. Design constraint #2.
- **Grammar↔kernel coupling — RESOLVED 2026-05-29.** Moderation lives in the
  height layer, amplitude-only: `clamp(1 - strength × slope, min, 1)` scales
  contribution weight after the grammar produces its blend; it does not feed back
  into family selection. The grammar still never reads kernel data. The
  weights/height seam holds.
  - *Real DEM pack wiring — DONE 2026-05-29* (see §3): `packs/dem_v1` loaded
    through the unchanged M1/M2 pipeline; property gate + GPU-parity gate green.
  - *Deferred follow-up — anti-repetition / kernel variety tuning:* naive
    single-kernel tiling produces visible creases at footprint seam boundaries
    (C0 continuity, not C1). This is expected and deferred until the renderer can
    show it.
- **GPU kernel-atlas for varied sizes — CLOSED 2026-05-29.** The named risk was:
  real OpenTopo DEMs have varied sizes; non-uniform kernel sizes may require an
  atlas layout redesign. **Validated closed:** `gpu_parity_dem_check.gd` dispatched
  a real 512×512 kernel atlas (~25 MB GPU buffer) on D3D12/RTX 5090 and read back
  successfully. Tier-1 family signatures EXACT; Tier-2 height maxd=0.040 m on
  ~6 km relief (within gate tolerance). The M2-flagged atlas-at-scale risk is
  resolved; no layout redesign needed for 512×512.
- **DEM kernel Z-score normalization (noted 2026-05-29):** all WG9/dem_v1 kernels
  are Z-SCORE normalized (mean 0, std 1). Height legitimately goes negative and can
  exceed `relief_m`. Do NOT assume [0,1]. The generator uses `relief_m` as amplitude;
  the Z-score distribution spans roughly ±3σ. Build-time spike filter in `build_pack.py`
  drops kernels with |Z|>12 to catch corrupt artifact pixels (3 dropped; non-issue
  for the accepted pack).
- **Visual tuning of `footprint_m` / `relief_m` — deferred to M3.** Physical values
  derived from DEM stats are in place (correct ground truth). Visual feel — whether
  the footprint scale looks right from a flying camera — requires the renderer (M3).
  A `footprint_scale` knob exists in the pack for then.
- **Full-pack kernel commit — deferred.** The gate-committed subset covers gates.
  Full ~115-kernel .npy set is generated on demand; committing all kernels deferred
  (large binary files, not needed until M3 streaming is wired).
- **Tagging manual review — deferred.** The approved family map was seeded from
  confidence≥0.7 metric inferences. Manual thumbnail review of all 115 kernels was
  deferred by the owner. `review_tags.py` HTML/CSV artifact and `--reseed` knob
  remain available for when it is done.
- **CPU/GPU parity epsilon (2026-05-29):** ABS_EPS=1e-2 m, REL_EPS=1e-5,
  justified by f32 mantissa limits. Observed max delta on D3D12/RTX 5090:
  7.67e-5 m synthetic (130× headroom); 0.040 m on real 512×512 DEM kernels (within
  gate tolerance). Widen only if future hardware profile requires it — do not widen
  speculatively.
- **GPU compute is windowed-only:** `Wg10GpuCompute` uses a local
  RenderingDevice which returns null under `--headless` on this D3D12 setup.
  The `gpu` gate suite therefore runs windowed; the `fast` suite stays headless.
  The `gpu` gate returns a distinct SKIP code (2) on a no-GPU / headless box so
  a skip is never miscounted as a pass.
