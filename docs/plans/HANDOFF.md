# WorldGen10 — Orchestrator / Owner Handoff

**A fresh-session pickup guide.** Read this first. It gives you (a new session, or a
new owner) the whole picture: what this project is, what we're building, the
pillars that decide every call, the methodology we run, how to build/run/gate, and
exactly where things stand and what's next. It is an *onboarding guide*, not a plan
or design doc — the source-of-truth docs are DESIGN.md / ROADMAP.md / STATUS.md and
the dated specs/plans under `docs/superpowers/`.

> **The 3-living-docs rule (DESIGN §7.2):** there are EXACTLY three living docs —
> DESIGN, ROADMAP, STATUS. Updates go *into* those three. Do NOT spawn new standalone
> plan docs. This HANDOFF is the one onboarding pointer (it predates the rule and is
> kept as the session-pickup guide); keep it pointing at the three, don't let it
> grow a fourth source of truth. WG9 died partly of ~20 contradictory docs — that is
> the failure we are actively avoiding.

Updated: 2026-05-29 (**M3 RENDER-LAYER RESET in progress.** Slices 1–8 built but the multi-level render looked "a mess" under a real fly, so the PRESENTATION layer is being rebuilt prove-one-thing-at-a-time in `proving_ground.tscn`, owner-flown, keeping the proven leaves. Steps 1–6 owner-confirmed; Step 7 (3rd level + p99 = M3 acceptance) next. Read `docs/plans/COMPONENT_INVENTORY.md` FIRST for the inventory + step plan + the bugs found. The "squareness/lines" are extreme DEM data (M6/M7 fix), not a render bug.)

---

## 1. What this project is

WorldGen10 is a **clean restart** of a procedural terrain generator. Its
predecessor, `worldgen9` at `d:/workflows/worldgen9`, is kept **READ-ONLY** as a
knowledge reference — port *knowledge* (formulas, contracts, lessons), **never copy
its code**. WG9's render layer is the cautionary tale: it created a GPU height page
*synchronously, per chunk, inside the build-during-motion path* (~128 ms each),
which produced black slabs and ~5 fps when flying fast. **Everything in WG10's
architecture is shaped to make that failure structurally impossible.**

- **Engine:** Godot 4.6 (Forward+, D3D12, Jolt, .NET `wg10`) at `wg-10/`.
- **Backend:** Rust GDExtension (`gdext` 0.5.3, `godot` crate `api-4-6`), crate at
  `wg-10/rust/`, addon `wg-10/worldgen_terrain/`.
- **Project root:** `D:\workflows\worldgen10` — its own git repo, commits on `main`.

## 2. What we're building (the end state)

A drop-in terrain system: **one node + one config resource**, copyable into any
Godot 4.6 game, with a tiny public API (`set_config`, `get_height(x,z)`,
`get_collision_field(area)`). Under the hood:

- A **unified GPU clipmap renderer** that streams height *pages* ahead of the
  camera, never blocks a frame, and degrades to **coarser-but-valid** terrain
  instead of going black.
- Fed by **authoritative sparse CPU facts** (the deterministic worldgen formula:
  hash → noise → region/province grammar → kernel → landform → height).
- Driven by **data-driven terrain packs** (first pack = real DEM/OpenTopo kernels);
  the core makes no assumptions about the data source.
- **Determinism + CPU/GPU parity** as hard contracts: same `(x,z,seed,pack)` ⇒ same
  height, on CPU and GPU, seam-exact at axis crossings.

Read `docs/plans/DESIGN.md` for the locked architecture (it is the source of truth):
§1 pillars, §2.4 CPU/GPU split, §3 packs, §4 determinism/parity, §5 render pipeline
(rings / page pool / stream-ahead scheduler / frame loop), §6 config + drop-in
boundary, §7 the rules, §9 open items.

## 3. The four pillars (DESIGN §1 — these decide every call)

1. **Adaptable / tunable** — config-driven, no magic numbers, drops into any game.
   Code reads tunables *only* from the one config resource.
2. **Performance** — GPU-shaped, no readback in the render path, **frame p99 < 6 ms
   at ~1000 m/s**. This is the headline number WG9 failed.
3. **Quality** — bounded, correct, parity-gated, no collapse (no flat planes, no
   single-palette collapse, no black holes).
4. **No shortcuts** — validate and reject, don't assume. Malformed pack ⇒ descriptive
   error, never a silent default.

When a decision is unclear, it is resolved by asking which pillar it serves. The
"never black, never stall" guarantee (pillar 2 + 3) is the spine of the render design.

## 4. How we work (the methodology — run this loop)

We advance milestone-by-milestone along ROADMAP using the **superpowers** skills, in
this rhythm. As orchestrator you run the loop; you do not hand-write feature code
inline.

1. **Brainstorm** (`superpowers:brainstorming`) — one question at a time, propose
   2–3 approaches, present the design in sections, get approval. Output: a dated
   spec in `docs/superpowers/specs/YYYY-MM-DD-<topic>-design.md`, committed.
2. **Write the plan** (`superpowers:writing-plans`) — verbatim, bite-sized TDD tasks
   (failing test → minimal impl → verify → commit), exact file paths, complete code,
   no placeholders. Output: a dated plan in `docs/superpowers/plans/`.
3. **Execute** (`superpowers:subagent-driven-development`) — a **fresh
   general-purpose subagent per task**, then **two-stage review**: spec-compliance
   reviewer first, then code-quality reviewer; the implementer fixes and we
   re-review until both pass. Continuous — don't stop between tasks to check in.
4. **Verify before claiming done** (`superpowers:verification-before-completion`) —
   fresh gate evidence (cargo + the relevant `tools/gate.py` suite) before any "done".
5. **Audit** — independently review the slice against its spec + the pillars before
   moving on.

Supporting habits: **probe-first de-risking** (throw a quick probe at an unknown —
GPU compute, offscreen render, hash portability — before building on it);
**honest-baseline docs** (STATUS says what is *actually true*, separating "passed a
counter gate" from "accepted"). TDD always: where a WG9 fixture exists, assert
**exact** values, not just bounds (a width-mismatch hash bug once passed a property
test and was only caught by exact-value parity).

## 5. Gates (the evidence; run before any "done")

`tools/gate.py` runs `*_check.gd` scripts through Godot (after one `--import` pass so
the GDExtension is registered). Plus `cargo test` for the pure Rust modules.

- `python tools/gate.py --suite fast` — **headless**, 5 checks: hash parity,
  determinism, grammar, height, DEM pack. (Pure-CPU layers.)
- `python tools/gate.py --suite gpu` — **WINDOWED**, 2 checks: CPU/GPU parity
  (synthetic) + DEM parity (real 512×512 kernels). Global RenderingDevice is null
  under `--headless` on this D3D12 box, so GPU/render gates must run windowed.
- `python tools/gate.py --suite m3` — **WINDOWED**, currently 2 checks (slice-1 render
  + slice-2 pool); slice 3 adds a 3rd (`m3_stream_check.gd`).
- `cargo test` (from `wg-10/rust`, with `CARGO_TARGET_DIR` unset — see §7): 81 tests
  green as of slice 2.
- **Exit codes:** 0 pass / 1 fail / 2 skip. A no-GPU/headless box returns SKIP (2) on
  gpu/m3 — never miscounted as a pass.

## 6. Where things stand (2026-05-29)

**Built & gated green:** M0 toolchain · M1 deterministic bedrock (hash/noise/fbm,
bit-exact vs WG9) + grammar + height + first real DEM pack (`packs/dem_v1`, 115
kernels / 12 families) · M2 GPU formula + CPU/GPU parity gate (family selection
EXACT, height within f32 epsilon, validated at real 512×512 scale) · **M3 slice 1**
(first rendered page: compute → R32F Texture2DRD → vertex displace, real relief
visible) · **M3 slice 2** (`Wg10PagePool` — the single RID owner; `PagePolicy` —
LRU + protected + budget, 11 headless tests, WG9-killer rules proven; zero-churn
eviction).

…plus **M3 slices 3–8 all built & gated:** slice 3 stream-ahead scheduler
(`SchedulePolicy` + `Wg10Streamer`, coarsest-first never-black, async-ready seam) ·
slice 4 clipmap ring geometry · slice 5a/5b 3×3 page-tile clipmap + live read-only
view (`Wg10TerrainView` uses `get_resident_page` — NEVER computes on the render path) ·
slice 6 fly harness (`m3_review.tscn` + fly camera + profiler + overlay) · slice 7
page-compute caching (`PageComputeContext` → p99 GREEN, async NOT needed) · **slice 8
visual stability** (texel-corner page gen + world-UV fine sampling + neighborhood-center
geomorph → inter-tile seam = 0 and the per-tile morph lattice gone; `m3_continuity_check`
reads back real production pages to prove `seam=0.0` + a perspective morph-banding ceiling).

**The 3 render-time sampling defects slice 8 fixed (so a future session doesn't reintroduce them):**
(1) geomorph must use distance to the 3×3 NEIGHBORHOOD center (`level_center` uniform,
normalized to `1.5·span`), NOT tile-local `VERTEX.xz` — tile-local fires the morph at all 9
tile edges. (2) The fine page is sampled by TRUE WORLD UV (`page_origin` uniform), NOT
`VERTEX.xz/span+0.5` — the latter hits texture borders, a half-texel off the texel centers.
(3) `height_page.glsl` uses a texel-CORNER convention (`u=px/(N-1)`: texel 0→origin,
N-1→origin+span) so abutting pages SHARE boundary samples — texel-center leaves them a texel
apart → a seam. `height_at()` is UNCHANGED (parity-critical; the M2 gpu suite never exercises
the page pixel→world mapping). Page textures carry CAN_COPY_FROM so the gate can read them
back (no render-path cost; p99 unaffected).

**THEN the slices-4→8 multi-level render turned out to be "a mess" under a REAL fly** (overlapping
sheets, seams, switching). Why the gates missed it: they proved *properties* (p99, never-black,
data-seam=0) but never *perceptual continuity in a flown perspective POV*. **So we are RESETTING
the presentation layer prove-one-thing-at-a-time** — keep the proven leaves (pool, page_policy,
schedule_policy, streamer, ring_geometry, page_compute — verified clean, one-directional deps),
rebuild ONLY `Wg10TerrainView` + `Wg10ClipmapRings` + `ring_displace.gdshader` step by step in
`proving_ground.tscn` (a STEP const flips on one component at a time), each owner-flown + approved
before the next. **`docs/plans/COMPONENT_INVENTORY.md` is the driving doc for this** (inventory of
every part + what proves it + the 7-step sequence). Steps owner-confirmed: 1 one page · 2 two-page
seam · 3 static 3×3 · 4 streamer-driven 3×3 · 5 coarse never-black blanket · 6 geomorph. Next:
Step 7 (3rd level + p99<6 ms at ~1000 m/s = the M3 acceptance), then fold the proven presentation
back into the real classes + reconcile the old m3 gates (they still exercise the OLD view path).

**Real bugs the reset found + fixed (each owner-confirmed; don't reintroduce):**
- **Page sampler defaulted to REPEAT wrap** → tile-edge vertices (uv=1) wrapped to the page's
  opposite edge → seams at EVERY tile boundary (the dominant "overlapping sheets"). Fix:
  `uniform sampler2D height_tex : filter_linear, repeat_disable;` (clamp-to-edge) in the shader.
- **Velocity lead was unit-wrong + unclamped.** `lead_frames` × velocity-in-m/s gave up to 64 km
  of lead at sprint → the 3×3 flew off the camera (pop-in, terrain-lag-under-you, churn). Fix:
  renamed `lead_seconds`; `SchedulePolicy::coverage_center` CLAMPS the offset to
  ±(radius_pages−0.5)·base_span so the camera is ALWAYS inside its ring at any speed; the view +
  proving ground READ that clamped centre from the streamer (no view/scheduler desync).
- **Step-5 "LOD line"** was the morph being OFF (each fine tile bound its OWN page as the morph
  target = nothing to blend). Step 6 wires each fine tile to the REAL coarse parent page
  (coarse_height_tex/coarse_origin/coarse_span) + the fine-neighborhood centre, then blends.

**The slice-8 3 sampling fixes (still valid, carried into the reset):** geomorph from the 3×3
NEIGHBORHOOD center (not tile-local); fine page by TRUE WORLD UV (`page_origin`); texel-CORNER
page gen (`u=px/(N-1)`) so abutting pages SHARE boundary samples. `height_at()` UNCHANGED
(parity-critical). Page textures carry CAN_COPY_FROM for gate readback (no render-path cost).

**The "blue squares / hard lines" the owner still sees are EXTREME DEM DATA, not a render bug**
(diagnosed: dem_v1 height field has ~450 m cliffs over 500 m; deep blue = real low elevation;
coarse mesh draws a cliff as a flat facet + hard edge). Fixes live in data/material/erosion
(M6 normals, M7 erosion, saner pack relief) — see COMPONENT_INVENTORY. Do NOT chase in the render
layer. Also recorded the big-picture intent there: DEM kernels are a LIBRARY of real-world
landform stamps; the grammar (where) + height field (blend) arrange them into a procedural
generator that speaks in real landforms.

**Counts:** cargo 103 green · fast 5 / gpu 2 (the m3 suite predates the reset — it exercises the
OLD view path + now uses LEAD_SECONDS; reconcile when the rebuilt view lands). **`main` is ~150
commits ahead of `origin/main` (unpushed — the OWNER pushes).**

## 7. Build / run gotchas (these bit prior sessions — read before touching the toolchain)

1. **`CARGO_TARGET_DIR` is set globally on this machine** (`D:\cargo-target-kalshi`)
   and OVERRIDES the crate's `.cargo/config.toml`. Unset it per-invocation or the dll
   lands in the global dir and Godot can't find it:
   `$env:CARGO_TARGET_DIR=$null; cargo build` (run from `wg-10/rust`).
2. **GDExtension only loads after an editor `--import` pass** writes
   `.godot/extension_list.cfg`. `tools/gate.py` does this; any new check must too.
3. **`.gdextension` lib path is `res://rust/target/debug/wg10_terrain.dll`** —
   resolved from the PROJECT ROOT, not the file's folder. `res://` can't escape root.
4. **Never `--quit` without a main scene** — it pops a blocking ALERT even headless.
   Use `--script` (SceneTree) for checks.
5. **GPU compute is windowed-only** on this D3D12 setup (headless RenderingDevice is
   null). `Wg10GpuCompute`, `Wg10PageCompute`, `Wg10PagePool`, and all gpu/m3 gates
   need a windowed run.
6. **DEM kernels are Z-SCORE normalized** (mean 0, std 1) — height legitimately goes
   negative and can exceed `relief_m`. Not a bug. A build-time filter drops |Z|>12
   spikes. Any shader consuming pages must expect this.
7. **Godot binary** (set in the shell before gates):
   `$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"`
8. **Shell is Windows PowerShell** — `;` chaining / `if ($?)`, not `&&`.

## 8. Operational constraints (important)

- **You (the assistant) cannot `git push`** — the harness blocks pushing to the
  external remote and the shell has no credentials. Commit locally always; **the
  OWNER runs `git push origin main`**. `main` is currently ~150 commits ahead.
- **WG9 (`d:/workflows/worldgen9`) is READ-ONLY** — read for knowledge, never write.
- **OpenTopo API key** is in the `OPENTOPOGRAPHY_API_KEY` env var (docs at
  `D:/assets/docs/reference/`); used by `tools/dem_pack/` to fetch DEMs.
- **Commit trailer:** `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

## 9. What to do next

**Immediate — the ONE box left to close M3: the owner's ACCEPTANCE FLY of `m3_review.tscn`.**
The render-layer RESET is DONE and FOLDED BACK: the proven prove-one-at-a-time model (steps 1–7,
in `proving_ground.tscn`) now lives in the REAL `Wg10TerrainView` + `Wg10ClipmapRings`, so
`m3_review.tscn` flies the shippable components. All gates green on the rebuilt path (m3 6/6 —
accept p99=3.94 ms<6; gpu 2/2; fast 5/5; cargo 103). Launch:
```
$env:GODOT_BIN --path "D:\workflows\worldgen10\wg-10" worldgen_terrain/harness/m3_review.tscn
```
Fly: WASD + Shift sprint (~1000 m/s) + mouse-look + Space/C, ESC frees the mouse. Confirm no
stalls, no black/holes, no inter-tile seam, no switching. On sign-off, **M3 closes.**

**Then:** retire `proving_ground.{gd,tscn}` (its job is done — the logic is in the real classes
now) and fold `COMPONENT_INVENTORY.md` into STATUS/ROADMAP + delete it (3-living-docs rule). THEN
M4 — Facts API (`get_height` + Jolt collision) via brainstorm→spec→plan→execute→audit. After M4:
M5 detail → M6 biomes/textures → M7 erosion.

**FIXED post-fold-back:** tiles vanishing on rotation AND a chunk blinking in/out on slow creep
to a boundary were the SAME frustum-cull bug — flat tile meshes + GPU vertex displacement, no
custom AABB, so Godot culled on the flat y=0 box. `Wg10ClipmapRings::configure` now sets a tall
custom AABB per tile (±8000 m Y); gates still green (p99=1.87 ms). LESSON: GPU-displaced meshes
ALWAYS need a custom AABB. **Squareness/lines = content (extreme DEM), fixed in M6 normals + M7
erosion + saner pack relief — NOT render.** Tracked in COMPONENT_INVENTORY.

**Squareness/lines = content, NOT render** (don't re-chase): diagnosed as extreme dem_v1 data
(~450 m cliffs over 500 m; deep blue = real low elevation; coarse mesh facets a cliff). Fixed by
M6 normals + M7 erosion + a saner pack relief, tracked in ROADMAP/COMPONENT_INVENTORY.

**Deferred but tracked** (don't forget, don't build early): async/background page
production — build it behind `Wg10PagePool::acquire_page` when heavy multi-pass
pages (M5 detail/normals, M6 biomes, M7 erosion) make synchronous N-per-frame
computes blow the frame budget. Async-ready by design ⇒ zero scheduler change.

Then M4 (Facts API + Jolt collision) → M5 (detail/masks) → M6 (biomes/textures) →
M7 (erosion/hydrology). Each gets its own spec → plan → execute → audit cycle.
