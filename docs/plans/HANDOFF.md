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

Updated: 2026-05-29 (M3 slice 2 built; slice 3 stream-ahead scheduler spec'd, about to be planned + built).

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

**Spec'd, not yet built:** **M3 slice 3 — stream-ahead scheduler**
(`docs/superpowers/specs/2026-05-29-m3-slice3-design.md`, approved). `SchedulePolicy`
(pure Rust: `coverage`/`plan_frame`/`coarser_fallback`, multi-level, bounded
acquires, never-black property) + `Wg10Streamer` (godot frame-loop driver) +
`page_pool.resident_keys()` + `m3_stream_check.gd`. Synchronous page production this
slice; the scheduler↔pool seam is async-ready so background production drops in later
with **zero scheduler change**.

**Counts:** cargo 81 green · fast 5 / gpu 2 / m3 2, all `fail=0`. **`main` is ~68
commits ahead of `origin/main` (unpushed).**

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
  OWNER runs `git push origin main`**. `main` is currently ~68 commits ahead.
- **WG9 (`d:/workflows/worldgen9`) is READ-ONLY** — read for knowledge, never write.
- **OpenTopo API key** is in the `OPENTOPOGRAPHY_API_KEY` env var (docs at
  `D:/assets/docs/reference/`); used by `tools/dem_pack/` to fetch DEMs.
- **Commit trailer:** `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

## 9. What to do next

**Immediate:** the slice-3 spec is approved and committed. Next steps in the loop:

1. **Owner reviews the slice-3 spec** (`docs/superpowers/specs/2026-05-29-m3-slice3-design.md`)
   — the brainstorming gate before planning.
2. **`superpowers:writing-plans`** → a dated TDD plan for slice 3 in
   `docs/superpowers/plans/`.
3. **`superpowers:subagent-driven-development`** → execute task-by-task with the
   two-stage review; verify the `m3` suite goes to 3 checks `fail=0` + cargo green.
4. **Audit** against spec + pillars; update STATUS/ROADMAP; commit each task.

**After slice 3 (remaining M3 slices, in order):** clipmap rings (concentric,
persistent meshes, recenter, L↔L+1 morph) → modular harness components
(camera/movement, diagnostics/profiling, UI overlay) → manual fly-test scene →
**the M3 acceptance gate** (renderer p99 < 6 ms + no black/holes, manually confirmed
at ~1000 m/s — gate green is necessary, not sufficient; the owner flies it).

**Deferred but tracked** (don't forget, don't build early): async/background page
production — build it behind `Wg10PagePool::acquire_page` when heavy multi-pass
pages (M5 detail/normals, M6 biomes, M7 erosion) make synchronous N-per-frame
computes blow the frame budget. Async-ready by design ⇒ zero scheduler change.

Then M4 (Facts API + Jolt collision) → M5 (detail/masks) → M6 (biomes/textures) →
M7 (erosion/hydrology). Each gets its own spec → plan → execute → audit cycle.
