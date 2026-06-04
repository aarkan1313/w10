# Scale-Invariant Biome Producer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the mountain biome producer scale-invariant so a coarse clipmap page is a true world-space low-pass of its children (kills the LOD geomorph "ground shifts weirdly" warp), and run flow-carved drainage only on the finest levels (coarse pages cheap → also attacks the movement hitch).

**Architecture:** Two coupled changes. (A) World-anchor every gaussian sigma: the recipe converts cell-sigmas to world-distance sigmas via `sigma_cells = sigma_world_m / spacing`, so the macro structure is identical in world space across levels. (B) Near-field-only flow: the carve (primary/tributary drainage) runs only on levels finer than a `flow_max_level` threshold; coarse levels bake macro-only, so the carve becomes a near-field fade-in detail. Parity is re-established Rust↔Python by world-anchoring the Python oracle too and REGENERATING the fixtures (a single reference spacing can't byte-match two different-spacing fixtures — see the spec §4.1).

**Tech Stack:** Python (numpy/scipy — `mountain_synthesis.py` is the oracle) · Rust GDExtension (`recipes.rs`/`array_ops.rs`/`biome_page_compute.rs`/`page_pool.rs`) · GLSL compute (`biome_page.glsl`) · Godot 4.6.2 windowed gates · `tools/gate.py`.

**Spec:** `docs/superpowers/specs/2026-06-03-worldgen-scale-invariant-biome-producer-design.md`

---

## Current Pickup Status - 2026-06-04

- T1-T5 are implemented and committed through the GPU producer path: Python oracle anchoring,
  fixture/oracle regeneration, Rust parity, flow-off macro parity, and per-level runtime
  kernel anchoring with `flow_max_level`.
- Editor-safe validation after the T6 wiring edit: `cargo test -p wg10_terrain --lib` =
  **217 passed / 0 failed**.
- T6 source wiring is now present: `generate_runtime_page_flow(..., flow_on)` plus
  `worldgen_terrain/tests/biome_crosslevel_check.gd`, wired into the `biome_fly` suite.
- Still pending: editor-closed/windowed `biome_fly` run for T5/T6, then owner re-fly of
  `mountain_fly_review.tscn` for T7. Do not mark this plan complete until those are recorded.

---

## Conventions (read first)

- Isolated cargo (editor-safe): `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test -p wg10_terrain` from `wg-10/rust`. Currently **214 passed**.
- Windowed gates (576 parity, the new cross-level + flow-off gates, m3, a re-fly capture) need the editor CLOSED + the default dll rebuilt: `$env:CARGO_TARGET_DIR=$null; cargo build -p wg10_terrain` (from wg-10/rust), then `$env:GODOT_BIN="C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"; python tools/gate.py --suite <name>` (or `& $env:GODOT_BIN --path "D:\workflows\worldgen10\wg-10" --script "res://...gd"`). **Do NOT kill the owner's editor**; these are owner-run or run with the editor closed by arrangement.
- **Commit only when the owner says.** Stage BY NAME, NEVER `git add -A`.
- No non-ASCII in `print()`. GDScript: `str()`/`%d`/`%f` (this build doesn't substitute `%e`/`%g`).
- `S_REF` (the reference spacing, metres/px) is a SHARED constant that must be IDENTICAL in Python and Rust. **Definition:** `S_REF = 32.0` (the live scene's L0 spacing `8192/(256-1) ≈ 32.1`, rounded — so the near render level is ~unchanged). Document it once in each side; a mismatch silently breaks parity.

---

## File Structure

**Python (the oracle — change FIRST so the fixtures regenerate consistently):**
- `tools/dem_pack/mountain_synthesis.py` — `generate()` gains a `spacing_m: float | None` param + a `flow_on: bool = True`. World-anchor every `gaussian_filter(sigma=...)` call: `sigma_cells = sigma_world_m / spacing` where `sigma_world_m = sigma_cell × S_REF`. When `flow_on=False`, zero the drainage chain (primary/tributary masks → 0 → no carve).
- `tools/dem_pack/export_mountain_576_oracle.py` — regenerate at the production spacing (pass `spacing_m`).
- `tools/dem_pack/export_recipe_fixtures.py` (or whichever emits `recipe_mountain_fixture.json`) — regenerate the 344 fixture with `spacing_m` = that fixture's grid spacing.
- NEW `tools/dem_pack/export_mountain_macro_oracle.py` — a flow-OFF 256-core oracle (for the §4.3 gate).
- NEW `tools/dem_pack/export_mountain_crosslevel_oracle.py` — bakes a region at level L and level L+1 spacings, FLOW OFF, for the §4.2 cross-level macro-agreement gate.

**Rust:**
- `wg-10/rust/src/recipes.rs` — `mountain::generate_seamsafe` + `mountain_seamsafe` gain `spacing_m: f64` + `flow_on: bool`; world-anchor the sigmas; gate the carve.
- `wg-10/rust/src/recipes.rs::helpers::flow_channels_seam_safe` — its internal sigmas (pre-blur 1.15, spread width_px) world-anchor too.
- `wg-10/rust/src/biome_page_compute.rs` — the per-sigma kernel build + `schedule_mountain` gain spacing + a flow-on/off branch; `build_biome_page_context` + `compute_biome_page_cached` thread `spacing_m` + `flow_on`.
- `wg-10/rust/src/page_pool.rs` — `compute_biome_page_cached` call sites pass per-level `spacing` (`ws/(ppx-1)`) + `flow_on = (level < flow_max_level)`; `configure_biome` gains `flow_max_level`.
- `wg-10/worldgen_terrain/shaders/biome_page.glsl` — the gaussian passes already take `kradius`/`koffset` (CPU-built kernel) so world-anchoring is CPU-side (no GLSL change for blurs); the flow-off branch = the schedule simply not dispatching the flow/carve passes (CPU-side). VERIFY no GLSL constant hardcodes a sigma.

**Tests/fixtures:**
- `wg-10/rust/src/recipes_tests.rs` — update `mountain_seamsafe_matches_python_oracle` + `mountain_seamsafe_matches_576_oracle` calls for the new args (pass each fixture's spacing + flow_on=true); add a flow-off macro parity test + a cross-level macro-agreement test.
- `wg-10/worldgen_terrain/tests/biome_crosslevel_check.gd` (NEW) — windowed: bake two levels, assert macro agreement.
- `tools/gate.py` — wire the new checks.

---

## Task 1: World-anchor the PYTHON recipe sigmas (oracle first) + flow_on flag

**Files:**
- Modify: `tools/dem_pack/mountain_synthesis.py`

The Python recipe is the oracle the Rust mirrors; change it first so fixtures regenerate consistently.

- [ ] **Step 1: Read the current sigma sites + carve**

Read `tools/dem_pack/mountain_synthesis.py`: `generate()` (~line 360), `_flow_channels_seam_safe` (~305, sigmas 1.15 + width_px), and the assembly (~line 420-490): `range_envelope = smoothstep(.., gaussian(ranges, 5.0))`, `massif_inner += 0.28*gaussian(ranges, 1.8)`, `massif = gaussian(massif, 2.0)`, `broad_range = gaussian(range_field, 7.0)`, `floor_mask` uses `gaussian(valley_mask, 1.2)`, `floor = gaussian(height, max(floor_smooth_px,0.2))`. The CARVE = `height -= carve_g*(..)*primary_mask` + `branch_g*(..)*tributary_mask`, where primary/tributary come from `_flow_channels_seam_safe` (flow).

- [ ] **Step 2: Add a module constant + helper**

Add near the top of `mountain_synthesis.py` (after the imports):

```python
# Reference spacing (metres/pixel) for world-anchored blur sigmas. A blur whose sigma is `sc` CELLS
# at this spacing covers `sc * S_REF` METRES; at any other spacing the cell-sigma is rescaled so the
# blur covers the SAME world distance -> the macro structure is identical across clipmap levels.
# MUST equal the Rust S_REF (recipes.rs). 32.0 = the live scene's L0 spacing (8192/(256-1)).
S_REF: float = 32.0

def _sigma_cells(sigma_cell_ref: float, spacing_m: float) -> float:
    """Convert a reference CELL sigma to the cell sigma at `spacing_m` so the WORLD extent is fixed.
    sigma_world_m = sigma_cell_ref * S_REF;  sigma_cells = sigma_world_m / spacing_m."""
    return (sigma_cell_ref * S_REF) / max(spacing_m, 1e-6)
```

- [ ] **Step 3: Thread `spacing_m` + `flow_on` through `generate()` and `_flow_channels_seam_safe`**

Change `generate(...)` signature to add `spacing_m: float | None = None` and `flow_on: bool = True`. At the top of `generate`, default: `spacing_m = float(spacing_m) if spacing_m is not None else S_REF` (so existing callers that pass nothing get the reference = unchanged behavior at the reference spacing). Replace EVERY `gaussian_filter(X, sigma=K, ...)` with `gaussian_filter(X, sigma=_sigma_cells(K, spacing_m), ...)` (K = the current literal: 5.0, 1.8, 2.0, 7.0, 1.2). For the floor: `sigma=_sigma_cells(max(style.floor_smooth_px,0.2), spacing_m)`. Pass `spacing_m` into `_flow_channels_seam_safe` (add the param there too: its `sigma=1.15` → `_sigma_cells(1.15, spacing_m)`, its `width_px` spread → `_sigma_cells(max(width_px,0.1)... )` — NOTE width_px is already a "px" value; treat it as a reference cell sigma → `_sigma_cells(width_px, spacing_m)`).

For `flow_on`: wrap the carve. When `flow_on=False`, set `primary_mask = np.zeros_like(base)` and `tributary_mask = np.zeros_like(base)` (skip the `_flow_channels_seam_safe` calls entirely — they're the expensive part), so `valley_mask=0`, the two `height -=` carve lines subtract 0, and `floor_mask` reduces to the `0.24*lowland` term. This yields the MACRO surface (base + ridge/near detail + floor blend, NO drainage).

- [ ] **Step 4: Smoke-test it runs + the reference is unchanged**

Run (from `tools/dem_pack`):
```bash
python -c "
import numpy as np, geography_engine as geo, mountain_synthesis as m
wx,wz=geo.grid(80, 79*32.0, ox=0,oz=0)  # spacing ~32 = S_REF
a=np.asarray(m.generate(wx,wz,seed=0,feature_span_m=90000.0)['height'])           # no spacing -> S_REF
b=np.asarray(m.generate(wx,wz,seed=0,feature_span_m=90000.0,spacing_m=32.0)['height'])
print('ref-vs-explicit-Sref maxdiff', float(np.max(np.abs(a-b))))  # expect 0.0 (both = reference)
c=np.asarray(m.generate(wx,wz,seed=0,feature_span_m=90000.0,spacing_m=64.0)['height'])
print('Sref-vs-2x ptp_a', float(np.ptp(a)), 'ptp_c', float(np.ptp(c)), 'differ?', float(np.max(np.abs(a-c))))
d=np.asarray(m.generate(wx,wz,seed=0,feature_span_m=90000.0,flow_on=False)['height'])
print('flow_on=False ptp', float(np.ptp(d)))  # macro only, should be smoother (no carve)
"
```
Expected: ref-vs-explicit-Sref maxdiff = 0.0 (default == S_REF); the 2x-spacing differs (scale-anchoring active); flow_off runs + is smoother.

- [ ] **Step 5: Commit (staging list)**

```bash
git add tools/dem_pack/mountain_synthesis.py
git commit -m "scale-inv(py): world-anchor mountain gaussian sigmas (S_REF) + flow_on flag"
```

---

## Task 2: Regenerate the 344 fixture + 576 oracle from the world-anchored Python

**Files:**
- Modify: `tools/dem_pack/export_mountain_576_oracle.py`
- Modify/run: the 344 fixture exporter (`tools/dem_pack/export_recipe_fixtures.py` or whichever emits `recipe_mountain_fixture.json` — find it)
- Regenerated: `wg-10/worldgen_terrain/fixtures/mountain_576_oracle.json`, `tools/dem_pack/fixtures/recipe_mountain_fixture.json`

- [ ] **Step 1: Open the 344 fixture exporter (`export_recipe_fixtures.py`) + add spacing**

The mountain 344 fixture is emitted by `tools/dem_pack/export_recipe_fixtures.py` (the per-biome ones are `export_recipe_<biome>_fixture.py`; mountain is the base file). It builds the apron grid with `spacing = feature_span_m / (n-1)` (stored in `grid.spacing`) and calls `mountain.generate(..., apron_px=160)`. ADD `spacing_m=spacing` to that generate call (the fixture's OWN computed spacing — so the fixture bakes at its spacing, world-anchored, and Rust parity at `r.grid.spacing` holds). VERIFY: `grep -n "mountain.generate\|spacing" tools/dem_pack/export_recipe_fixtures.py` to find the exact call + the spacing local.

- [ ] **Step 2: Update the 576 oracle exporter to pass spacing**

In `export_mountain_576_oracle.py`, the generate call becomes `mountain.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M, apron_px=APRON_PX, spacing_m=SPACING)` (SPACING already computed = `FEATURE_SPAN_M/CORE_PX`... NO — use the GRID spacing it stores, `SPACING = FEATURE_SPAN_M / CORE_PX`; confirm that's the m/px the fixture records as `grid.spacing` and pass THAT).

- [ ] **Step 3: Regenerate both + verify**

Run (from repo root):
```bash
cd tools/dem_pack && python export_mountain_576_oracle.py && python export_recipe_fixtures.py
```
Verify (artifact, not report): both JSONs rewrote (mtime fresh), `records[0].height` length unchanged (65536 / 576), values FINITE. The values CHANGED vs the old fixtures (the recipe is now spacing-aware) — that's expected.

- [ ] **Step 4: Commit (staging list)**

```bash
git add tools/dem_pack/export_mountain_576_oracle.py export_recipe_fixtures.py wg-10/worldgen_terrain/fixtures/mountain_576_oracle.json tools/dem_pack/fixtures/recipe_mountain_fixture.json
git commit -m "scale-inv(fixtures): regenerate 344 + 576 mountain oracles from world-anchored recipe"
```

---

## Task 3: World-anchor the RUST recipe (mirror Python) + flow_on

**Files:**
- Modify: `wg-10/rust/src/recipes.rs` (`mountain::generate_seamsafe`, `mountain_seamsafe`, `helpers::flow_channels_seam_safe`)
- Modify: `wg-10/rust/src/recipes_tests.rs` (the two parity tests' calls)

- [ ] **Step 1: Add the Rust S_REF + sigma helper**

In `recipes.rs` (near the mountain module top), add:

```rust
/// Reference spacing (metres/pixel) for world-anchored blur sigmas. MUST equal the Python S_REF
/// (mountain_synthesis.py). A blur of `sc` cells at this spacing covers `sc * S_REF` metres; at any
/// other spacing the cell sigma rescales so the WORLD extent is fixed -> macro structure identical
/// across clipmap levels.
pub const S_REF: f64 = 32.0;

/// Convert a reference CELL sigma to the cell sigma at `spacing_m` (fixed world extent).
#[inline]
pub fn sigma_cells(sigma_cell_ref: f64, spacing_m: f64) -> f64 {
    (sigma_cell_ref * S_REF) / spacing_m.max(1e-6)
}
```

- [ ] **Step 2: Thread `spacing_m` + `flow_on` through generate_seamsafe + flow_channels_seam_safe**

`mountain::generate_seamsafe` + the public `mountain_seamsafe` gain `spacing_m: f64` and `flow_on: bool` params (append them). Replace every `gaussian_filter_nearest(X, rows, cols, K, TRUNCATE)` with `gaussian_filter_nearest(X, rows, cols, sigma_cells(K, spacing_m), TRUNCATE)` (K = the literal: 5.0, 1.8, 2.0, 7.0, 1.2, floor_smooth). `flow_channels_seam_safe` (helpers) gains `spacing_m`: its internal pre-blur 1.15 → `sigma_cells(1.15, spacing_m)`, its spread `width_px` → `sigma_cells(width_px, spacing_m)`. When `flow_on=false`, set `primary_mask`/`tributary_mask` to all-zeros and SKIP the `flow_channels_seam_safe` calls (the expensive part), mirroring Python exactly.

- [ ] **Step 3: Update the parity test calls + run (RED then GREEN against regenerated fixtures)**

In `recipes_tests.rs`, the two tests (`mountain_seamsafe_matches_python_oracle` + `mountain_seamsafe_matches_576_oracle`) call `mountain_seamsafe(...)`. Add the new args: `spacing_m = r.grid.spacing` (each record's own spacing) and `flow_on = true`. (The fixtures were regenerated in Task 2 WITH that spacing, so parity holds.)

Run: `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test -p wg10_terrain mountain_seamsafe 2>&1 | tail -10`
Expected: BOTH parity tests PASS at the tight bar (1e-9 / 1e-12) — Rust world-anchored == Python world-anchored. If they FAIL, the Rust sigma-anchoring diverges from Python (compare `sigma_cells` arithmetic + the exact sigma list — a missed site). Full suite green (214).

- [ ] **Step 4: Commit (staging list)**

```bash
git add wg-10/rust/src/recipes.rs wg-10/rust/src/recipes_tests.rs
git commit -m "scale-inv(rust): world-anchor mountain recipe sigmas + flow_on (parity vs regenerated oracle)"
```

---

## Task 4: Flow-off MACRO parity gate (prove flow_on=false == macro-no-carve)

**Files:**
- Create: `tools/dem_pack/export_mountain_macro_oracle.py`
- Create (generated): `wg-10/worldgen_terrain/fixtures/mountain_macro_oracle.json`
- Modify: `wg-10/rust/src/recipes_tests.rs` (add the flow-off parity test)

- [ ] **Step 1: Export a flow-OFF 256-core oracle**

Create `tools/dem_pack/export_mountain_macro_oracle.py` — a copy of `export_mountain_576_oracle.py` but the generate call passes `flow_on=False` and the output filename is `mountain_macro_oracle.json` (note in the JSON `"flow_on": false`). Run it; verify the artifact (65536 finite values).

- [ ] **Step 2: Add the Rust flow-off parity test**

In `recipes_tests.rs`, add `mountain_macro_matches_oracle` modeled on `mountain_seamsafe_matches_576_oracle`, but call `mountain_seamsafe(..., spacing_m=r.grid.spacing, flow_on=false)` and compare to `mountain_macro_oracle.json`. Bar 1e-9.

Run: `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test -p wg10_terrain mountain_macro 2>&1 | tail -5`
Expected: PASS (the Rust flow-off path == the Python flow-off oracle).

- [ ] **Step 3: Commit (staging list)**

```bash
git add tools/dem_pack/export_mountain_macro_oracle.py wg-10/worldgen_terrain/fixtures/mountain_macro_oracle.json wg-10/rust/src/recipes_tests.rs
git commit -m "scale-inv(parity): flow-off macro oracle + Rust parity (flow_on=false == macro no-carve)"
```

---

## Task 5: Thread spacing + flow_on through the GPU producer + page_pool

**Files:**
- Modify: `wg-10/rust/src/biome_page_compute.rs` (kernel build per-spacing, schedule flow-on/off, context + dispatch args)
- Modify: `wg-10/rust/src/page_pool.rs` (per-level spacing + flow_max_level)

- [ ] **Step 1: Per-spacing kernel build + schedule flow branch**

In `biome_page_compute.rs`: the per-sigma gaussian kernels are built CPU-side (a port of `gaussian_kernel1d`). The schedule must build each kernel from the WORLD-anchored sigma `sigma_cells(K, spacing_m)` (add `S_REF` + `sigma_cells` consts mirroring recipes.rs, or `use crate::recipes::{S_REF, sigma_cells}`). `schedule_mountain` gains awareness of `flow_on`: when false, SKIP the `flow_discharge`/`flow_channels` + carve passes (dispatch only the macro pointwise/gaussian/assemble passes). Thread `spacing_m: f64` + `flow_on: bool` onto `BiomePageComputeContext` + `build_biome_page_context` + `compute_biome_page_cached` (like `relief_m` was threaded). NOTE: the 576 parity readback entry (`generate_runtime_page_576`) must pass `spacing_m` = the record's spacing + `flow_on=true` (so it still matches the regenerated oracle).

- [ ] **Step 2: page_pool passes per-level spacing + flow decision**

In `page_pool.rs`: `configure_biome` gains `flow_max_level: i64` (store it). At the two `compute_biome_page_cached` call sites (acquire fresh + eviction-recompute), compute `spacing = ws / (ppx - 1)` (the texel-corner spacing for THIS level's page) and `flow_on = (level as i64) < self.biome_flow_max_level`, and pass both. (The biome context is built once with a FIXED spacing — but spacing varies per level. DECISION: spacing + flow_on must be PER-DISPATCH args to `compute_biome_page_cached`, NOT baked into the context. So move them out of `build_biome_page_context` into the per-page call. The kernel build then happens per-page from the dispatch spacing — OR cache kernels per distinct spacing. Simplest correct: build the kernels per-dispatch from `spacing` (a handful of small 1D kernels, cheap). If the per-dispatch kernel rebuild measurably costs, cache by spacing later.)

- [ ] **Step 3: Update the GDScript configure_biome callers**

`mountain_fly_review.gd`, `biome_fly_capture.gd`, `biome_fly_perf_check.gd`, `biome_runtime_isolate.gd`: add `flow_max_level` to the `configure_biome` call (start: `2` → flow on levels 0,1; off 2,3,4). (The `relief_m` arg is already there; append `flow_max_level` per the Rust signature order.)

- [ ] **Step 4: Isolated cargo + re-prove 576 parity green**

Run: `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test -p wg10_terrain 2>&1 | tail -3` (green).
Then WINDOWED (owner/editor-closed): `python tools/gate.py --suite biome_fly` — the 576 parity must still PASS (the producer at the oracle's spacing + flow_on=true == the regenerated oracle). If it fails, the GPU sigma-anchoring or the per-dispatch spacing diverges — debug vs the CPU `mountain_seamsafe` at the same spacing.

- [ ] **Step 5: Commit (staging list)**

```bash
git add wg-10/rust/src/biome_page_compute.rs wg-10/rust/src/page_pool.rs wg-10/worldgen_terrain/harness/mountain_fly_review.gd wg-10/worldgen_terrain/tests/biome_fly_capture.gd wg-10/worldgen_terrain/tests/biome_fly_perf_check.gd wg-10/worldgen_terrain/tests/biome_runtime_isolate.gd
git commit -m "scale-inv(gpu): per-level spacing + flow_max_level through the biome producer + pool"
```

---

## Task 6: Cross-level macro-agreement gate (the "did we kill the warp" proof)

**Files:**
- Create: `wg-10/worldgen_terrain/tests/biome_crosslevel_check.gd`
- Modify: `tools/gate.py` (add to the `biome_fly` suite)

The KEY new gate: bake level L and level L+1 over the same world region (FLOW OFF on both → pure macro), sample both at identical world XZ, assert they agree within a small bar (the seed `biome_level_surface_diff_check.gd` measured 73%; this asserts macro agreement < e.g. 2% of relief).

- [ ] **Step 1: Write the windowed cross-level check**

Create `wg-10/worldgen_terrain/tests/biome_crosslevel_check.gd`. Use `generate_runtime_page_576`-style readback (or a dedicated dump entry) to bake a page at spacing `S` and another at spacing `2S` covering the same world region, FLOW OFF (`flow_on=false`), then compare the OVERLAP at matched world XZ (world-resample the coarser to the finer, nearest or bilinear). Assert `max_abs_diff / relief < CROSS_EPS` (start CROSS_EPS = 0.05; tighten to the achieved). Print the achieved ratio. WINDOWED skip rc 2. Model the readback + compare shape on `biome_level_surface_diff_check.gd` (the seed harness already in the tree). Add a `#[func]` dump entry if needed (macro page at an arbitrary spacing) mirroring `generate_runtime_page_576` with `flow_on=false`.

- [ ] **Step 2: Wire into gate.py + run WINDOWED**

Add `worldgen_terrain/tests/biome_crosslevel_check.gd` to the `biome_fly` suite. Run (owner/editor-closed): `python tools/gate.py --suite biome_fly`.
Expected: `[wg10-crosslevel] macro agreement ratio=<small> < 0.05 status=pass`. If the ratio is still large, the world-anchoring didn't take (a missed sigma, or the flow-off path still carves) — debug which field diverges (extend the dump to per-field).

- [ ] **Step 3: Commit (staging list)**

```bash
git add wg-10/worldgen_terrain/tests/biome_crosslevel_check.gd wg-10/rust/src/biome_page_compute.rs tools/gate.py
git commit -m "scale-inv(gate): cross-level macro-agreement gate (proves coarse == world low-pass of fine)"
```

---

## Task 7: Owner re-fly + docs

**Files:**
- Modify: `docs/plans/STATUS.md`, `docs/plans/HANDOFF.md`, `docs/plans/LOOSE_ENDS_LEDGER.md`

- [ ] **Step 1: Re-capture + owner fly**

Rebuild the dll (editor closed). Re-run `biome_fly_capture.gd` → PNG: the morph should no longer warp; valleys should FADE IN as detail near the camera. Then **OWNER flies `mountain_fly_review.tscn`**: confirm (a) the "ground shifts weirdly" warp is GONE, (b) the movement hitch is reduced (coarse pages skip flow), (c) the look still holds (R/F relief knob, B legacy A/B). Owner judges. Note the flow-off transition level visibility (spec §5) if any.

- [ ] **Step 2: Update STATUS/HANDOFF/LEDGER**

Record: scale-invariant producer done (world-anchored sigmas + near-field-only flow); geomorph warp fixed (cross-level macro agreement = `<ratio>`); hitch reduced (coarse skips flow; measure the new fly p99/update); parity re-established Rust↔Python on regenerated fixtures (344=`<n>`, 576=`<n>`, macro=`<n>`); `flow_max_level` default = `<n>`. Remaining: the other 10 biomes inherit the pattern (not done); the drainage off-frame bake if the hitch persists. Owner look verdict = `<v>`.

- [ ] **Step 3: Commit + push (owner-triggered)**

```bash
git add docs/plans/STATUS.md docs/plans/HANDOFF.md docs/plans/LOOSE_ENDS_LEDGER.md
git commit -m "docs: scale-invariant biome producer (warp fixed, hitch reduced, parity re-established)"
# push only when the owner says
```

---

## Notes for the implementer

- **S_REF must be byte-identical Python ↔ Rust (32.0).** A mismatch silently breaks parity AND scale-invariance. It's the single shared magic number; both sides cite the other.
- **Change Python FIRST, regenerate fixtures, THEN Rust.** The fixtures are the oracle; Rust parity is "matches the regenerated Python," not "matches the old bytes" (the recipe legitimately changed — spec §4.1).
- **The carve is the ONLY flow-derived term** (primary_mask/tributary_mask → valley_mask → the two `height -=` lines + floor_mask). flow_on=false zeroes those two masks; everything else is macro and unchanged. Verify the floor_mask reduces cleanly (its `0.24*lowland` term survives).
- **Spacing is PER-LEVEL, so spacing + flow_on are PER-DISPATCH** args to `compute_biome_page_cached`, not baked into the once-built context (Task 5 Step 2). Kernels rebuild per-dispatch from spacing (cheap); cache by spacing only if measured to matter.
- **The 576 parity gate must stay green** at the oracle's spacing + flow_on=true — it's the proof the recipe MATH survived the world-anchoring. If it breaks, a sigma site was missed or the arithmetic diverged.
- **Windowed gates are owner-run / editor-closed.** Never claim a windowed pass unwatched.
- **Don't kill the owner's editor.** Isolated cargo for all Rust validation; coordinate the windowed runs.
