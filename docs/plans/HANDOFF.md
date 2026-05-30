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

Updated: 2026-05-30 (**M0–M4 DONE, all gates green** — cargo 115, fast 6/6, gpu 4/4, m3 6/6. M3 render layer structurally done + folded into the real classes; M4 Facts API done (drop-in `Wg10Facts`: get_height + sparse Jolt collision + adaptable collidable edit seam + off-frame GPU bake). Added Milestone 8 (visible editable terrain — the other half of M4's edit seam). **NEXT: M5 — detail & masks.** Read STATUS.md for the M3/M4 detail + bug-list; this HANDOFF §6 is the one-line-per-milestone map. The remaining "squareness/blobby/LOD-pop" is test-scale + content (M5–M7), not a render bug.)

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

## 6. Where things stand (2026-05-30) — M0–M4 DONE

**All gates green: cargo 115 · fast 6/6 · gpu 4/4 · m3 6/6.** (STATUS.md has the blow-by-blow; this
is the one-line-per-milestone map.)

- **M0** toolchain · **M1** deterministic bedrock (hash/noise/fbm bit-exact vs WG9) + grammar +
  height + first real DEM pack (`packs/dem_v1`, 115 kernels / 12 families).
- **M2** GPU formula + CPU/GPU parity gate (family selection EXACT, height within f32 epsilon).
- **M3 render pipeline — structurally DONE.** `Wg10PagePool` (single RID owner) + `PagePolicy` (LRU/
  protected/budget, zero-churn eviction) + `SchedulePolicy`/`Wg10Streamer` (coarsest-first
  never-black, clamped velocity-lead) + N-level 3×3 clipmap (`Wg10ClipmapRings` + `Wg10TerrainView`,
  reads `get_resident_page` — NEVER computes on the render path) + `ring_displace.gdshader` +
  `page_compute` caching. p99 well under 6 ms at ~1000 m/s. Built via a prove-one-thing-at-a-time
  RESET (the stacked slices had been "a mess" under a real fly), folded into the real classes.
- **M4 Facts API — DONE.** Drop-in `Wg10Facts` (RefCounted): `get_height` = `clamp(base +
  edit-provider.delta, bedrock, ceiling)`; sparse `get_collision_field` (CPU, no readback,
  Jolt-ready — caller owns the body); adaptable circular-stamp edit seam (`apply_edit`/`clear_edits`/
  `set_bedrock`, pluggable provider); off-frame GPU `bake_collision_region`. Visible==collision
  parity 0.0009 m. Edits are COLLIDABLE but NOT yet visible (Milestone 8).

**Render-layer bugs found + fixed — DO NOT reintroduce (the expensive lessons):**
- **Page sampler defaulted to REPEAT wrap** → tile-edge vertices (uv=1) wrapped to the page's
  opposite edge → seams at EVERY tile boundary. Fix: `sampler2D ... : filter_linear, repeat_disable`
  (clamp-to-edge) in `ring_displace.gdshader`.
- **Velocity lead was unit-wrong + unclamped** (`lead_frames` × m/s → 64 km lead at sprint, ring
  flew off the camera). Fix: `lead_seconds` + `SchedulePolicy::coverage_center` clamps to
  ±(radius−0.5)·span (camera always in-ring); the view READS that clamped centre from the streamer.
- **Geomorph / fine-UV / page-gen convention** (M3): geomorph from the 3×3 NEIGHBORHOOD centre (not
  tile-local); fine page by TRUE WORLD UV (`page_origin`); texel-CORNER page gen (`u=px/(N-1)`) so
  abutting pages SHARE boundary samples. `height_at()` / `height::height` UNCHANGED (parity-critical
  — the M2 gpu suite + M4 facts depend on it; never touch the formula).
- **GPU-displaced flat meshes need a custom AABB** (`Wg10ClipmapRings` sets one) or they
  frustum-cull (tiles vanish on rotation). Coarsest level HOLDS LAST-GOOD on a miss (never blank the
  bottom blanket). "Loads then unloads" was view-distance > loaded extent → fixed with more levels +
  far-plane/fog matched to the loaded edge (config), not an unload bug.

**The "blue squares / blobby / LOD-pop" look is EXTREME DEM DATA + TEST SCALE, not a render bug**
(`dem_v1` has ~450 m cliffs over 500 m; deep blue = real low elevation; coarse mesh facets a cliff).
Fixed downstream by M5 detail + M6 materials/normals + M7 erosion + a saner pack relief — NOT in the
render layer. Big-picture intent: DEM kernels are a LIBRARY of real-world landform stamps; grammar
(where) + height field (blend) arrange them into a procedural generator that speaks in real
landforms. The foundation (gen + GPU-dense/CPU-sparse perf + parity + facts) is AAA-capable; the
LOOK is downstream.

**Counts:** cargo 115 · fast 6/6 · gpu 4/4 · m3 6/6. **`main` is ~170+ commits ahead of `origin/main`
(unpushed — the OWNER pushes).** (`COMPONENT_INVENTORY.md` was the M3-reset driver doc; it was
RETIRED into STATUS once the render layer landed — don't look for it.)

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

**NEXT MILESTONE: M5 — Detail & masks (GPU, render-only).** ROADMAP §"Milestone 5":
- Detail/displacement layer (bounded, shader-only, edge-safe) — adds the high-frequency detail the
  raw kernels lack (the biggest single lever on the "bare/blobby" look).
- Slope/curvature/debug + world-space masks.
Start it the same way every milestone has gone: **brainstorm (one question at a time) → spec
(`docs/superpowers/specs/`) → writing-plans → execute slice-by-slice with gates → audit + update
the living docs.** M5 is render-only (GPU, no parity contract on the *detail* itself, but it must
not break the visible-vs-collision agreement at the base — keep edits/facts reading the BASE height,
detail is a render-time displacement on top). Prove it on-screen (owner-flown), since look-quality
is the point and gates can't judge "looks good."

**Optional before M5 (owner's call):** the M3 §7.3 acceptance fly of `m3_review.tscn` (5 levels +
fog) — render gates are green so it's a formality, but it's the documented final sign-off. Launch:
`$env:GODOT_BIN --path "D:\workflows\worldgen10\wg-10" worldgen_terrain/harness/m3_review.tscn`
(WASD + Shift sprint + mouse-look + Space/C; M = morph heatmap, K = cull-toggle debug).

**Then, per ROADMAP:** M6 biomes/materials (normals fix the coarse-mesh facets — the real
"looks AAA" milestone) → M7 erosion (carves the extreme DEM cliffs) → M8 visible editable terrain
(the other half of M4's edit seam: make dug craters SEEN, not just collided). Each gets its own
spec → plan → execute → audit cycle. **Don't re-chase the "squareness/blobby/LOD-pop" in the render
layer — it's test-scale + content, fixed by M5–M7 + a saner pack.**

**Deferred but tracked** (don't build early): async/background page production — build it behind
`Wg10PagePool::acquire_page` IF heavy multi-pass pages (M5 detail, M6 biomes, M7 erosion) make
synchronous N-per-frame computes blow the frame budget. Async-ready by design ⇒ zero scheduler
change. (Not needed yet — caching solved the M3 spike.)

**Build/run reminder:** Rust rebuilds use `tools/build_rust.ps1` (do NOT kill the editor; it
releases the DLL on focus-loss — alt-tab + retry, or close it if you're not using it). GDScript +
shader changes hot-reload, no rebuild. The proving-ground scene + debug toggles (M/K, flip-log)
stay in for LOD/M5 tuning — harmless, off by default.
