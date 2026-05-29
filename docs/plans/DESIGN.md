# WorldGen10 — Design

Source of truth for architecture, rules, and contracts. If something here
conflicts with code, either the code is wrong or this doc is — reconcile
immediately, do not let them drift. (That drift is why WorldGen9 is being
restarted.)

Last updated: 2026-05-28

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
   ground-follow.

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

- **OpenTopo kernel methodology review** (before relying on the DEM pack):
  read WG9's extraction/processing pipeline (`factory/`, `tools/`), eyeball a
  few sample kernel outputs, and confirm the processed cache contains
  everything the new generator needs — not a full 80 GB audit. Records its
  conclusion as a section update here, not a new doc.
- ~~Set the concrete frame-time budget.~~ **Decided: renderer p99 < 6 ms at
  ~1000 m/s** (§7.3).
- ~~Decide native backend language.~~ **Decided: Rust GDExtension** (faster;
  proven toolchain carried forward from WG9).
- **Tune finest-ring spacing + ring count** (§5.1) once real assets/textures
  exist to judge the near-detail radius against. Left as config; do not guess a
  locked number now.
