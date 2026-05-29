# WorldGen10 — Execution Handoff

Operational kickoff for a fresh session. This is NOT a plan or design doc (those
are DESIGN.md / ROADMAP.md / STATUS.md / the dated plan). It tells the next
session exactly how to start executing.

Written: 2026-05-28

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

## Your task this session

Execute `docs/plans/2026-05-28-m0-m1-foundation.md` — the M0+M1 foundation plan.
It is a 10-task TDD plan (Task 0–9) that:
1. Vendors WG9's hash reference fixture (parity ground truth).
2. Stands up a Rust GDExtension crate that builds.
3. Ports WG9's deterministic hash/noise/fbm into pure Rust, parity-tested
   against the fixture's known u32 outputs.
4. Adds a negative-axis seam (floor-semantics) guard test.
5. Exposes the hash to Godot via a thin binding.
6. Wires the extension into the Godot project.
7. Adds headless Godot checks (hash parity + determinism).
8. Adds a Python gate runner.
9. Updates ROADMAP/STATUS.

**Recommended execution mode:** subagent-driven-development (fresh subagent per
task, review between tasks) — or executing-plans for inline. Start by invoking
the chosen skill, then work the plan task-by-task in order.

## Environment (verified 2026-05-28)

- Project root: `D:\workflows\worldgen10`
- Godot project: `D:\workflows\worldgen10\wg-10` (Godot 4.6, mono/.NET `wg10`,
  Forward+, D3D12, Jolt). Currently an empty default project — no terrain code.
- Godot binary (set this in the shell before running checks):
  ```powershell
  $env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
  ```
- Rust: `cargo 1.94.1` is on PATH. **gdext has NOT been built yet** — Task 1 is
  the first real test of the toolchain.
- Git: `worldgen10` is its own repo. HEAD = `a1b909c` (plan committed). Commit
  per task as the plan specifies.
- Shell: Windows PowerShell. Use `;` chaining / `if ($?)`, not `&&`.

## First real-risk checkpoints (watch these)

These are the spots where the plan meets reality and may need a small in-task
fix (the plan flags them inline):

1. **Task 1 Step 4 — gdext build + dll name.** Confirm the actual output name
   under `wg-10/rust/target/debug/` (e.g. `wg10_terrain.dll`). Task 5's
   `.gdextension` `[libraries]` path must match it exactly.
2. **Task 5 — `entry_symbol`.** Plan uses `gdext_rust_init` (gdext 0.5 default).
   If the extension fails to load, verify the symbol gdext actually exports for
   this version and correct the `.gdextension`.
3. **Task 6 — fixture case shape.** The hash fixture may contain non-int / >4
   value cases the simple binding can't represent. Plan says filter to int-only
   cases and log skipped ones. That is sufficient parity for this layer; don't
   over-engineer the binding here.

## Definition of done for this plan

- `cargo test --manifest-path wg-10/rust/Cargo.toml` passes (hash parity +
  determinism + seam tests).
- `python tools\gate.py --suite fast` returns `fail=0` (headless Godot hash
  parity vs WG9 fixture + determinism, through the loaded native lib).
- ROADMAP/STATUS updated to reflect the green foundation.
- Each task committed separately.

(Note: this layer's "done" is parity + determinism gates — the perf+visual+
manual acceptance rule in DESIGN §7.3 applies to the *render pipeline* milestone,
not this pure-math foundation.)

## Hard rules (from DESIGN §7 — enforce while executing)

- Separation of concerns; ~600-line soft cap per file. `hash.rs` stays pure (no
  `godot` imports); only `bind_worldgen.rs` touches Godot types.
- Only three living docs + dated plans. Do NOT spawn new standalone docs;
  updates go into DESIGN/ROADMAP/STATUS.
- Port knowledge from w9, never copy code.
- TDD: failing test first, minimal impl, verify pass, commit.

## What comes after this plan (do NOT start without a new plan)

Next plan = M1 continued: region/province decisions + kernels + landform +
terrain-pack format (the kernel review in DESIGN §9 informs the pack format).
After that, the render pipeline (the hard part). Write each as its own dated
plan via the writing-plans skill before executing.
