# M4 — Facts API (authoritative, sparse) + adaptable edit seam — Design

**Status:** spec
**Date:** 2026-05-30
**Milestone:** M4 (DESIGN §2.2, §4, §6.2). Brainstormed with the owner one question at a time.

## Goal

Package the already-proven, parity-gated base height as the **drop-in authoritative Facts API**
(DESIGN §6.2): the sparse CPU queries gameplay reads (`get_height`, `get_collision_field`), plus
an **adaptable edit seam** so terrain can be modified (meteor craters / shovel holes / laser pits)
— collidable now, visibly-rendered in a later milestone. Build it as slices, CPU first.

## What M4 is NOT (scope guards / YAGNI)

- **Not recomputing height.** `Wg10Height::height(x,z,seed)` already exists and is CPU/GPU
  parity-gated (M2). M4 *surfaces* it; it does not touch the formula.
- **Not VISIBLE edits.** Making a dug hole appear in the GPU-rendered surface (composing the edit
  delta into the height pages) is a render-pipeline change deferred to its own later milestone. M4
  ships **collidable-but-not-visible** edits — a known, documented temporary divergence.
- **Not persistence.** Saving/loading edits is out of scope; edits live in memory this milestone.

## Architecture

**One formula, two access patterns, two perf contracts** (the WG9-safe split):

```
height(x, z) = clamp( base_height(x,z) + edit_provider.delta(x,z),  bedrock_floor, ceiling )
```
- `base_height` = `Wg10Height::height` (parity-gated, untouched).
- `edit_provider` = a pluggable trait `delta(x,z) -> f32`; default `NoEdits` returns 0 (zero cost).
  M4 ships ONE concrete provider: `StampEdits` (circular stamps). The interface is what makes the
  spectrum adaptable — no edits / shallow-to-bedrock / unlimited caves later all swap the provider
  with ZERO consumer rework.
- `bedrock_floor` / `ceiling` = config clamp (per-game: bedrock at −2 m, or −∞ for deep caves).

**Sparse / hot path (CPU, NEVER stalls):** `get_height`, `get_collision_field`. Per-frame
gameplay/collision. Microseconds, no GPU, no readback — this IS the performance model (DESIGN
§2.2: "cheap because sparse"). The hard rule (the thing WG9 violated): no GPU readback on the hot
path, ever.

**Bulk / cold path (GPU, deliberate readback, OFF-FRAME ONLY):** `bake_collision_region`. Large-
area collision baked at load/one-shot via `Wg10GpuCompute`'s batch path. Named `bake_*` +
documented blocking/async so it cannot reach the hot path. Same formula underneath (parity holds).

**Engine-agnostic core preserved (§6.3):** the height+edit+clamp math and the grid sampler are
pure Rust (no Godot); the Godot node is a thin `#[func]` wrapper returning plain numbers. Collision
returns a height-sample array — no Jolt dependency in the core; the *caller* builds the body.

## Components / files (pure core + thin wrapper; follow existing pattern, files < 600 lines)

**Pure Rust core (no Godot — unit-tested, engine-agnostic):**
- `wg-10/rust/src/edit_layer.rs` — `EditProvider` trait (`delta(x,z) -> f32`); `NoEdits` (always
  0); `StampEdits { stamps: Vec<Stamp> }` where `Stamp { cx, cz, radius, depth, falloff }` and
  `delta` sums the contribution of every stamp overlapping (x,z). Pure, deterministic, unit-tested.
- `wg-10/rust/src/facts.rs` — the composition: `height(base_fn, provider, clamp, x, z)` and
  `collision_field(... , center, world_size, n) -> Vec<f32>` (row-major; loops `height` over the
  n×n grid). Pure; takes the base height fn + provider + clamp config. Unit-tested.

**Thin Godot wrapper (the drop-in node, §6.2):**
- `wg-10/rust/src/facts_api.rs` — `Wg10Facts` (RefCounted, like `Wg10Height` — it is a pure query
  object, no scene-tree behavior). Holds the pack/seed (loaded like `Wg10Height`), an
  `EditProvider`, and the clamp config. `#[func]`s:
  - `configure(pack_dir, pack_file, seed) -> String` (error string or "" — mirrors the load
    contract). **Intentional:** `Wg10Facts` loads its OWN pack/seed, independent of the render
    pool / `Wg10Height` — so the Facts node is a true standalone drop-in (a game can use facts
    with no renderer, and vice versa). The in-memory grammar constants are tiny, so the
    independence is free; loading the same pack in both the renderer and facts is fine and keeps
    the boundary clean (no shared-state coupling).
  - `get_height(x, z) -> f64`
  - `get_collision_field(center_x, center_z, world_size, samples_per_side) -> PackedFloat32Array`
  - `apply_edit(cx, cz, radius, depth, falloff)` · `clear_edits()`
  - `set_bedrock(floor, ceiling)`
  - (Slice 4) `bake_collision_region(...) -> PackedFloat32Array` — calls `Wg10GpuCompute`; doc'd
    off-frame.

## Data flow (sparse hot path)

```
game character controller
  └─ Wg10Facts.get_collision_field(cx, cz, world_size, n)   [per patch, off-frame-ish]
       └─ facts::collision_field → loops facts::height over the n×n grid
            └─ clamp(Wg10Height::height + provider.delta, floor, ceil)
       → PackedFloat32Array (row-major, raw metres)
  └─ caller builds HeightMapShape3D(n, n, array) + StaticBody3D, owns its lifetime
  └─ Wg10Facts.get_height(x, z)  [per-frame, one point under the entity] → f64
```
Edits: `apply_edit` pushes a `Stamp`; next query sees it (no cache to invalidate). Deterministic.

## Error handling (validate-and-reject, no silent defaults — pillar 4)

- Not configured → `get_height` returns an error sentinel + `godot_error!` (no garbage compute).
- `get_collision_field`: `samples_per_side < 2` or `world_size <= 0` → `godot_error!` + empty
  array (Jolt needs ≥ 2).
- `set_bedrock` with `floor > ceiling` → rejected, config unchanged + `godot_error!`.
- `apply_edit` with `radius <= 0` → no-op + `godot_warn!`.
- NaN/inf input guards (defensive, like existing checks).

## Testing

**Pure Rust unit tests (lowest regression target, headless):**
- `edit_layer_tests`: `NoEdits.delta == 0`; one stamp = full depth at center, 0 at edge (falloff),
  0 outside radius; overlapping stamps sum; determinism.
- `facts_tests`: `height == base` with no edits; a stamp lowers `height` by its delta; bedrock
  floors a deep stamp; ceiling clamps a tall mound; `collision_field` cell (i,j) == `height` at
  that exact world point; rejects bad args.

**Godot gates (added to `tools/gate.py`):**
- `facts_check.gd` (fast/headless): no-edit `get_height` == `Wg10Height.height` (authoritative-base
  parity); `apply_edit` → `get_height` shows the dent; bedrock holds; `get_collision_field` array
  == point `get_height` calls; error cases return sentinels, not crashes.
- `facts_collision_parity_check.gd` (windowed): DESIGN §4 contract ("entities don't float/sink").
  Over a sample grid, compare the GPU-rendered surface height (gate-only readback from a resident
  page) vs `get_height` on BASE terrain; assert |Δ| < epsilon (M2-parity scale). Edited cells
  EXCLUDED and asserted as the *intentional* collidable-not-visible divergence.

**Manual sanity (§7.3 spirit):** a tiny scene drops a Jolt `StaticBody3D` built from
`get_collision_field` + a `RigidBody3D` ball — it rests on the visible ground and rolls into a
stamped crater.

## Slice order (one provable step at a time — the lesson from M3)

1. **CPU seam** — `Wg10Facts.configure/get_height` = base + `NoEdits` + clamp. Gate: no-edit parity.
2. **Stamps + bedrock** — `StampEdits`, `apply_edit`/`clear_edits`, `set_bedrock`. The diggable
   collidable hole.
3. **Sparse collision** — `get_collision_field` + Jolt `HeightMapShape3D` wiring (in the test
   scene/caller) + the visible-vs-collision parity gate.
4. **GPU bulk bake** — `bake_collision_region` (off-frame readback via `Wg10GpuCompute`). Last;
   most concurrency-subtle.

## Pillars check

- **Adaptable:** edit representation is a pluggable provider; bedrock/ceiling/clamp + collision
  resolution are config. No magic numbers; the no-edits → bedrock → unlimited-caves spectrum is all
  config + provider swap.
- **Performance:** sparse path is CPU-only, no readback, no stall (the WG9-safe model); the only
  GPU readback is the explicitly off-frame `bake_*`.
- **Quality:** authoritative facts match the rendered surface (parity gate); never-silent-default
  error handling; base parity preserved (formula untouched).
- **No shortcuts:** validates+rejects bad input; the collidable-not-visible edit divergence is
  documented and gate-asserted as intentional, not hidden.
