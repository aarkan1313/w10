# WorldGen10 — Execution Handoff

Operational kickoff for a fresh session. This is NOT a plan or design doc (those
are DESIGN.md / ROADMAP.md / STATUS.md / the dated plans). It tells the next
session exactly how to start executing.

Written: 2026-05-28
Updated: 2026-05-28 (M0 + M1 hash/noise/fbm foundation landed green)

---

## One-paragraph context

WorldGen10 is a clean restart of a terrain generator (predecessor `worldgen9` at
`d:/workflows/worldgen9`, kept only as a knowledge reference — do NOT copy its
code). The architecture is locked and documented: a unified GPU clipmap renderer
(streams pages ahead, never blocks a frame, degrades to coarser-but-valid
instead of going black), fed by authoritative *sparse* CPU facts, with
data-driven terrain packs. The lag/black-slab disease in w9 came from creating
GPU height pages synchronously per-chunk during motion (~128 ms each); the new
design fixes that structurally. Read `docs/plans/DESIGN.md` first — it is the
source of truth.

## Where things stand (done so far)

The **M0 toolchain + M1 deterministic bedrock** are built and green (plan
`docs/plans/2026-05-28-m0-m1-foundation.md`, all 10 tasks committed):

- `wg10_terrain` Rust GDExtension builds and loads in Godot 4.6; `Wg10Hash`
  (RefCounted) is registered and callable headlessly.
- `wg-10/rust/src/hash.rs` — pure, engine-agnostic FNV-1a `stable_hash`,
  `hash_grid`, `value_noise`, `fbm`, `fade`, `smoothstep_unit`. **Bit-exact**
  vs the vendored WG9 fixture `wg-10/worldgen_terrain/fixtures/hash_reference.json`
  (stable_hash, hash_grid, value_noise, fbm — all exact to 1e-15).
- Gates: `cargo test` (7 tests) and `python tools/gate.py --suite fast`
  (headless hash parity + determinism through the native lib) both green.

See STATUS.md "Current state" / "What works" for the authoritative snapshot, and
STATUS.md "Build / run gotchas" before touching the toolchain.

## Your task this session

**Write, then execute, the M1-continued plan.** The hash/noise bedrock is done;
the next layer up is the rest of the deterministic CPU formula plus the data
format it consumes:

- region / province decisions (the grammar that sits on top of the noise),
- kernels (the DEM/OpenTopo kernel review in DESIGN §9 informs this),
- landform profiles,
- the **terrain-pack format** (defined + loadable; first pack = DEM/OpenTopo
  kernels; the core consumes the pack — no source assumptions baked in).

There is no dated plan for this yet. **Start by writing one** with the
superpowers:writing-plans skill (a new `docs/plans/2026-05-DD-m1-continued-*.md`),
in the same TDD shape as the foundation plan: parity fixtures committed to git,
failing test first, minimal impl, exact-value parity against WG9 where a fixture
exists. Then execute it with superpowers:executing-plans (inline) or
superpowers:subagent-driven-development (fresh subagent per task).

Do NOT start the render pipeline (M3) — that is a later, separate plan and is the
hard part.

## Environment (verified 2026-05-28)

- Project root: `D:\workflows\worldgen10` — its own git repo, work committed on
  `master`, per-task commits.
- Godot project: `D:\workflows\worldgen10\wg-10` (Godot 4.6, mono/.NET `wg10`,
  Forward+, D3D12, Jolt). Now has the `wg10_terrain` addon wired in.
- Rust crate: `wg-10/rust/` (gdext 0.5.3, pinned in Cargo.lock).
- Godot binary (set this in the shell before running checks):
  ```powershell
  $env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
  ```
- Shell: Windows PowerShell. Use `;` chaining / `if ($?)`, not `&&`.

## Build / run gotchas (MUST read — these bit the last session)

1. **`CARGO_TARGET_DIR` is set globally on this machine** (`D:\cargo-target-kalshi`)
   and OVERRIDES the crate's `.cargo/config.toml`. Unset it per-invocation or the
   dll lands in the global dir and Godot can't find it:
   ```powershell
   $env:CARGO_TARGET_DIR = $null; cargo test    # run from wg-10/rust
   ```
   (The committed `.cargo/config.toml` makes a clean machine work without this.)
2. **GDExtension only loads after an editor import pass** writes
   `.godot/extension_list.cfg`. A bare `--headless --script` run on a clean
   checkout will NOT register `Wg10Hash`. Run `godot --headless --import` first
   (which `tools/gate.py` already does) — do the same for any new check.
3. **`.gdextension` lib path is `res://rust/target/debug/wg10_terrain.dll`** —
   resolved from the PROJECT ROOT, not the file's folder. `res://` cannot escape
   the root with `..`.
4. **Never use `--quit` without a main scene** — it pops a blocking ALERT dialog
   even headless. Use `--script` (SceneTree) for checks.
5. Headless is fine for this pure-CPU layer; GPU work (M2+) will NOT run headless.

## Definition of done (for the M1-continued plan you write)

Same bedrock discipline as the foundation: parity fixtures committed to git;
`cargo test` green; the relevant headless gate(s) green via `tools/gate.py`
(add a suite or extend `fast` as needed); ROADMAP/STATUS updated; each task
committed separately. (The perf+visual+manual acceptance rule in DESIGN §7.3
applies to the *render pipeline* milestone, not the pure-math CPU layers.)

## Hard rules (from DESIGN §7 — enforce while executing)

- Separation of concerns; ~600-line soft cap per file. Pure deterministic math
  stays free of `godot` imports; only the thin binding (`bind_worldgen.rs`)
  touches Godot types.
- Only three living docs + dated plans. Do NOT spawn new standalone docs;
  updates go into DESIGN/ROADMAP/STATUS.
- Port knowledge from w9, never copy code.
- TDD: failing test first, minimal impl, verify pass, commit. Where a WG9
  fixture exists, assert **exact** values, not just bounds — the foundation's
  `hash_grid` bug (a u32-vs-int64 width mismatch) passed a property test and was
  only caught by exact-value parity.

## What comes after (do NOT start without a new plan)

After M1-continued: the **GPU formula + CPU/GPU parity** (M2), then the **render
pipeline** (M3 — the hard part). Write each as its own dated plan via the
writing-plans skill before executing.
