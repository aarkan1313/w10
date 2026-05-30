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

Updated: 2026-05-30 (**MAJOR PIVOT — the height CORE is being rebuilt; M5/M6/M7 as originally framed are
superseded.** M0–M4 (toolchain, deterministic gen, GPU parity, render pipeline, Facts API) DONE; gates
cargo 115 · fast 6/6 · gpu 4/4 · **m3 8/8** (NOT the old "6/6" — headlines were stale). **What changed:** an
owner fly showed the terrain reads "blobby/placed/not a contiguous landmass." Root-caused: `sample_kernel`
reads DEM kernels as TILING textures and makes the tiled kernel the whole height. A spectral-synthesis fix
was REFUTED by the owner's eye (a power spectrum captures roughness but discards PHASE = structure). After a
full step-back re-vision, the new direction is locked: **WorldGen10 is a terrain FRAMEWORK, infinite-
procedural-first (No Man's Sky reference); the height core becomes PARAMETER-DRIVEN WARPED-NOISE** — distill
real DEMs into per-biome structural PARAMS, the grammar blends them, one warped-noise generator (domain warp
+ macro fBm landmass + ridged ridgelines + carved valleys) makes infinite seamless terrain (kernels = a DNA
library, never sampled → no tiling). **Worldgen Slice 1 (offline generator prototype) is OWNER-ACCEPTED**
("pretty good, a little noisy" — reads as contiguous structured terrain, no grid/repeat). **NEXT: Slice 2
(distill real DEMs → biome params, offline) → close loose ends (B1/B2/B3 + doc drift, see LOOSE_ENDS_LEDGER)
before the Rust build (Slice 3) → Rust core → GPU parity → live fly.** READ THESE for the current truth:
`docs/superpowers/specs/2026-05-30-worldgen10-north-star-vision.md` (the framework vision),
`…/2026-05-30-worldgen-core-design.md` (the height-core architecture), `docs/plans/LOOSE_ENDS_LEDGER.md`
(everything in-flight/tabled), and memory `worldgen10-north-star-vision` / `worldgen10-wg9-height-recipe`.
STATUS.md top = the live state. The old M5-detail/M6-materials/M7-erosion framing below is SUPERSEDED — see
§9 + ROADMAP for the re-sequenced plan.)

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
- Fed by **authoritative sparse CPU facts** (a deterministic worldgen formula). NOTE: the
  formula is being REBUILT (§9). OLD (being replaced): `hash → grammar → kernel SAMPLED as a
  tiling texture → height` (this is what produced the blobby/tiling look). NEW (the rebuild):
  `hash → grammar (blends per-biome PARAMS) → warped-noise GENERATE → height` — kernels are
  distilled into a DNA library, never sampled, so nothing tiles. See the worldgen-core spec.
- Driven by **data-driven packs** — but the pack now stores per-biome PARAM SETS (distilled
  from the real DEMs), not raw kernel pixels. The core makes no assumption about the data source.
- **Determinism + CPU/GPU parity** as hard contracts: same `(x,z,seed,pack)` ⇒ same height,
  CPU and GPU, seam-exact at axis crossings. (The new generator must still honor this.)
- **WorldGen10 is also a FRAMEWORK** (owner-confirmed re-vision): infinite-procedural FIRST
  (No Man's Sky reference), adaptable to bounded / spherical-planet / handmade-area modes via
  knobs. See `docs/superpowers/specs/2026-05-30-worldgen10-north-star-vision.md`.

Read `docs/plans/DESIGN.md` for the architecture — BUT note its top superseded-notice: §2.1
(worldgen core) + §3 (packs) describe the OLD kernel-as-height design (historical); the CURRENT
height-core truth is the two 2026-05-30 specs (vision + worldgen-core). DESIGN §1 pillars, §2.2
facts, §2.3/§5 render pipeline, §2.4 parity split, §4 contracts, §6 config/drop-in, §7 rules are
STILL accurate + kept.

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

Supporting habits (each earned the hard way): **probe-first de-risking** (throw a quick
probe at an unknown — GPU compute, offscreen render, a noise basis — before building on
it); **render-images-first for look work** (render the terrain to images the OWNER judges
BEFORE any runtime — this killed the spectral approach + a sandpaper-scale bug offline, for
~half a day each, instead of a wasted runtime rebuild); **look-quality is OWNER-judged** —
gates prove invariants (parity/perf/non-repetition/bounds), they CANNOT judge "looks like
terrain," so every look milestone ends in an owner fly/eyeball; **honest-baseline docs**
(STATUS says what is *actually true*, separating "passed a gate" from "owner-accepted").
TDD always; assert **exact** values where a fixture exists (a width-mismatch hash bug once
passed a property test, caught only by exact-value parity). And: a gate that round-trips
its own assumption proves nothing (the spectral gate tested the iFFT path, not the shipping
basis) — make gates exercise the REAL thing.

## 5. Gates (the evidence; run before any "done")

`tools/gate.py` runs `*_check.gd` scripts through Godot (after one `--import` pass so
the GDExtension is registered). Plus `cargo test` for the pure Rust modules.

- `python tools/gate.py --suite fast` — **headless**, **6 checks** (hash parity,
  determinism, grammar, height, DEM pack, facts). Pure-CPU layers.
- `python tools/gate.py --suite gpu` — **WINDOWED**, **4 checks** (CPU/GPU parity
  synthetic + DEM parity + facts-collision-parity + facts-bake). Global RenderingDevice
  is null under `--headless` on this D3D12 box, so GPU/render gates must run windowed.
- `python tools/gate.py --suite m3` — **WINDOWED**, **8 checks** (slice1 render, pool,
  stream, view, accept, continuity, m5_detail, m5_perf_hardened).
- `cargo test` (from `wg-10/rust`, `CARGO_TARGET_DIR` unset — see §7): **115 tests** green.
- `python -m pytest` (from `tools/dem_pack/`): **22 tests** — the offline pack + worldgen-
  prototype tools (`worldgen_proto.py`, `spectral.py` [a kept negative result], `dem_pack_lib`).
- **Exit codes:** 0 pass / 1 fail / 2 skip. A no-GPU/headless box returns SKIP (2) on
  gpu/m3 — never miscounted as a pass. (A single windowed run can exit non-zero on teardown;
  trust the `[gate] suite=… fail=0` summary line, re-run once to confirm.)

## 6. Where things stand (2026-05-30) — M0–M4 DONE

**All gates green: cargo 115 · fast 6/6 · gpu 4/4 · m3 8/8 · dem_pack pytest 22.** (STATUS.md has the blow-by-blow; this
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

**⚠️ The "blobby / placed / not-a-contiguous-landmass" look is the HEIGHT CONTENT, not the render
layer — and it is what the worldgen-core rebuild (§9) fixes.** (An EARLIER belief, now DISPROVEN, was
"it's extreme DEM data + test scale, fixed downstream by M5 detail / M6 materials / M7 erosion." That
was wrong — do NOT re-chase it.) ROOT CAUSE, owner-confirmed: `sample_kernel` reads the DEM kernels as
TILING textures and makes the tiled kernel the WHOLE height → repeating stamps, no continuous structure.
The fix is NOT detail/materials/erosion on top — it's rebuilding the height core so it generates a
contiguous structured landmass (param-driven warped-noise; §9). The render pipeline + parity + facts
foundation is solid and KEPT; the *height content* is what's being rebuilt.

**Counts:** cargo 115 · fast 6/6 · gpu 4/4 · m3 8/8 · dem_pack pytest 22. **`main` is in sync with `origin/main`**
(the assistant CAN push — see §8). (`COMPONENT_INVENTORY.md` was the M3-reset driver doc, RETIRED into
STATUS — don't look for it.)

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

- **`git push` WORKS from the assistant** (corrected 2026-05-30 — previously believed
  blocked). The Windows Git Credential Manager (`credential.helper = manager`) has a
  cached GitHub token from a prior interactive login, so pushing through the Bash tool
  succeeds: `GIT_TERMINAL_PROMPT=0 git push origin main` (the env var makes it fail fast
  instead of hanging on a prompt if the token is ever missing/expired — in which case the
  owner re-auths interactively once). Pushed all 174 backlog commits this way; `main` is
  now in sync with `origin/main` (`aarkan1313/w10`). Still commit locally as you go; push
  when work is at a sync point (or when the owner asks).
- **WG9 (`d:/workflows/worldgen9`) is READ-ONLY** — read for knowledge, never write.
- **OpenTopo API key** is in the `OPENTOPOGRAPHY_API_KEY` env var (docs at
  `D:/assets/docs/reference/`); used by `tools/dem_pack/` to fetch DEMs.
- **Commit trailer:** `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

## 9. What to do next (the WORLDGEN-CORE REBUILD — supersedes the old M5–M7)

The active work is the **worldgen height-core rebuild** (spec: `docs/superpowers/specs/2026-05-30-worldgen-
core-design.md`). It REPLACES `height::height`/`sample_kernel` (the tiling) with a parameter-driven warped-
noise generator. The old "M5 detail / M6 materials / M7 erosion" milestones are RE-SEQUENCED into this (see
ROADMAP). Methodology unchanged: **brainstorm → spec → writing-plans → execute slice-by-slice with gates →
owner-flown acceptance → update living docs.** Look-quality is owner-judged (render images + fly); gates
prove invariants (parity/perf/non-repetition), not "looks good."

**The slice plan (worldgen core):**
- **Slice 1 — generator prototype (offline Python): DONE, owner-accepted.** `tools/dem_pack/worldgen_proto.py`
  + render images. Warped-noise reads as contiguous structured terrain, no grid/repeat.
- **Slice 2 — biome distillation (offline Python): NEXT.** Distill the 115 real DEMs → per-biome structural
  param-sets (ridge_strength/valley_depth/warp/octave_amps/relief — STRUCTURAL metrics, NOT a power spectrum
  — that was refuted). Render each biome from its DISTILLED params; owner judges per-biome fidelity.
- **PRECONDITION before Slice 3 (the first RUNTIME build): close the loose ends** in
  `docs/plans/LOOSE_ENDS_LEDGER.md` — **B1** (Wg10PagePool GPU-RID leak: no Drop impl + 2 wrong comments),
  **B3** (hardened perf gate: a sky frame scores nonblack=1.0; add terrain-vs-sky + detail on/off), **B2**
  (never-black is capacity-dependent not structural: protect held coarse pages + capacity-pressure gate),
  and the **doc-drift pass** (distinct 18→15, dead 0.35→0.25, m3 6/6→8/8, DESIGN stale, STATUS M5 two-states).
  These live in the KEPT render pipeline + perf gate the rebuild sits on — fix before building on them.
- **Slice 3 — Rust generator core:** port `generate` + `blend_biome_params` to `height.rs` (replace
  `sample_kernel`); gates determinism/bounded/seam/non-repetition.
- **Slice 4 — GPU parity + integrate:** mirror `generate` in GLSL, REMOVE the 25 MB kernel atlas, re-baseline
  `gpu_parity_check`, wire into render + facts (relief_scale, visible==collision hold), hardened perf gate.
- **Slice 5 — scale tune + live blend + the owner FLY:** dial scale toward the 1-10 m adaptable target,
  confirm seamless biome transitions live; "Google-Maps contiguity" acceptance fly (where the "a little
  noisy" gets its honest judgment). Audit vs pillars; update living docs.

**BIG LATER roadmap item (tracked, NOT now): distilled erosion** (Grand-Canyon-grade). The warped-noise core
is PLAUSIBLE terrain, not real connected erosion. The bridge = the owner's insight: OFFLINE run real hydraulic
erosion → distill a cheap LOCAL operator → apply online (infinite+fast). A major milestone after the noise-
tier core ships. See LOOSE_ENDS_LEDGER.

**Other tabled work (recorded, not lost):** materials/normals/biome-surfacing (after the height core looks
good); modes (bounded/spherical-planet/handmade-area blending); M8 visible-editable-terrain; async page
production. All in the LEDGER with revisit conditions.

**KEPT, proven, don't rebuild:** the infinite streaming clipmap render pipeline, the grammar (where biomes
go), facts/collision + relief_scale, the parity-gated noise primitives, the hardened GPU-time perf gate.

**Build/run reminder:** Rust rebuilds use `tools/build_rust.ps1` (do NOT kill the editor; it
releases the DLL on focus-loss — alt-tab + retry, or close it if you're not using it). GDScript +
shader changes hot-reload, no rebuild. The proving-ground scene + debug toggles (M/K, flip-log)
stay in for LOD/M5 tuning — harmless, off by default.
