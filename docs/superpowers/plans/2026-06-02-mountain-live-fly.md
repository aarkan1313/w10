# Mountain Live-Fly Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run the parity-proven mountain GPU recipe in the real `page_pool` streaming runtime on the global RenderingDevice, behind a flag, so the owner can fly an all-mountain world.

**Architecture:** Mirror the legacy producer seam. The legacy path = a `PageComputeContext` (compiled-once shader + buffers) built by `build_page_compute_context`, dispatched per page by `compute_page_cached` on the pool's global RD. We add a sibling `BiomePageComputeContext` + `compute_biome_page_cached` that host the proven `schedule_mountain` dispatch on the global RD (a refactor that splits "build context" from "dispatch page", which `run_inner` currently fuses), then a `use_biome_path` flag in `page_pool` that swaps the producer. A 576² cross-oracle parity gate (vs an independent Python f64 oracle) and a did-real-work live perf gate keep it honest.

**Tech Stack:** Rust GDExtension (`godot` crate) on the global RenderingDevice; GLSL compute (`#version 450`, base profile); Godot 4.6.2 windowed SceneTree checks (RD compute is null headless → skip rc 2); `tools/gate.py` suites; Python (numpy) for the f64 oracle.

**Spec:** `docs/superpowers/specs/2026-06-02-worldgen-mountain-live-fly-design.md`

---

## Conventions (read before starting)

- **Isolated cargo validation (editor-safe, no rebuild of the live dll):** `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test -p wg10_terrain` (from `wg-10/rust`). Use this for every Rust step.
- **Windowed gates** (the 576² parity gate + the live fly + `m3`) need a real GPU + the dll rebuilt into the DEFAULT target. **Do NOT kill the owner's editor** (memory `worldgen10-dont-kill-editor`). Rebuild = `$env:CARGO_TARGET_DIR=$null; cargo build -p wg10_terrain` from `wg-10/rust` with the editor CLOSED; run = `$env:GODOT_BIN="C:\Godot\v4.6.2\...\Godot_v4.6.2-stable_mono_win64_console.exe"; python tools/gate.py --suite <name>`. These steps are OWNER-RUN (or Claude-run with the editor closed, by arrangement). Never claim a windowed result unwatched (memory `worldgen10-profiling-must-be-real`).
- **Commit only when the owner says.** The commit steps stage explicit paths (the staging list); the owner triggers the actual commit cadence. **NEVER `git add -A`** (the tree has pre-existing dirty files that are not ours).
- **No non-ASCII in `print()`** on Windows (use `->`, not arrows). GDScript `String % args` does NOT substitute `%e`/`%g` — use `str()`/`%f`/`%d`/`%s` (memory `worldgen10-godot46-string-format`).
- **Verify artifacts, not reports** — after any fixture export, check mtime+size+keys.

---

## File Structure

**New Rust:**
- `wg-10/rust/src/biome_page_runtime.rs` — the runtime producer: `BiomePageComputeContext` struct (global-RD shader+pipeline+persistent apron buffers+gaussian kernels for mountain), `build_biome_page_context`, `free_biome_page_context`, and `compute_biome_page_cached`. This is the *runtime* sibling to `page_compute.rs`'s context/producer; the *math* (the pass schedule) is reused from `biome_page_compute.rs` by extracting the dispatch sequence into a shared fn.
- `wg-10/rust/src/biome_page_runtime_tests.rs` — pure-helper unit tests (no GPU).

**Modified Rust:**
- `wg-10/rust/src/biome_page_compute.rs` — extract the mountain pass-dispatch SEQUENCE out of `run_inner` into a `pub(crate)` fn `dispatch_mountain_schedule(scheduler, …)` that BOTH the test-harness `run_inner` and the new runtime producer call (DRY — one proven schedule, two hosts). No math change.
- `wg-10/rust/src/page_pool.rs` — add `use_biome_path` flag + the biome context option; `configure` builds it; `acquire_page` + the eviction-recompute path route to `compute_biome_page_cached` when the flag is on.
- `wg-10/rust/src/lib.rs` — register the new module + test mod.

**New GDScript (windowed):**
- `wg-10/worldgen_terrain/tests/biome_page_576_parity_check.gd` — 576² producer vs the 256-core Python f64 oracle.
- `wg-10/worldgen_terrain/tests/biome_fly_perf_check.gd` — did-real-work live perf gate on the biome-path streaming scene.

**New scene:**
- `wg-10/worldgen_terrain/harness/mountain_fly_review.tscn` (+ `.gd`) — the M3 streaming scene with `use_biome_path` on; an A/B toggle (legacy vs biome).

**New Python:**
- `tools/dem_pack/export_mountain_576_oracle.py` — emits the 256-core f64 oracle fixture for the 576² gate.

**Modified fixtures/tooling:**
- `wg-10/worldgen_terrain/fixtures/mountain_576_oracle.json` (new, generated).
- `tools/gate.py` — add `biome_fly` suite (the 576 parity + the perf gate); wire the checks.

---

## Task 1: Extract the mountain dispatch schedule (DRY refactor, no math change)

**Files:**
- Modify: `wg-10/rust/src/biome_page_compute.rs` (extract the mountain pass sequence from `run_inner` into `dispatch_mountain_schedule`)

The runtime producer (Task 3) must run the EXACT same pass sequence the parity-proven test harness runs. To avoid two copies (which would drift), extract the sequence into one `pub(crate)` fn both call. This task is a pure refactor: after it, `biome_page` parity MUST still be byte-identical (re-proven in Task 2's re-run note).

- [ ] **Step 1: Read the current mountain dispatch in `run_inner`**

Read `wg-10/rust/src/biome_page_compute.rs` around the `match biome { "mountain" => … }` schedule dispatch inside `run_inner` (the `schedule_mountain`-style sequence of `Scheduler` calls — pointwise passes, gaussians, the two `flow_channels`, assemble, floor, final, crop). Note the exact `Scheduler` method calls and order. This is the block to extract.

- [ ] **Step 2: Define the extracted fn signature**

Add to `biome_page_compute.rs` (near the other schedule helpers):

```rust
/// The mountain pass-dispatch SCHEDULE, extracted so the readback test harness (`run_inner`)
/// AND the runtime producer (`biome_page_runtime::compute_biome_page_cached`) run the SAME
/// proven sequence (DRY — one schedule, two hosts). Operates on an already-built `Scheduler`
/// bound to the apron buffers; pure dispatch, no rd/buffer ownership. `flow_iters` is the flow
/// PULL-relaxation step count (STABLE_ITERS for the 344 fixture; the production convergence count
/// at 576). No math change vs the inline block it replaces.
pub(crate) fn dispatch_mountain_schedule(s: &mut Scheduler) {
    // <-- the exact sequence moved verbatim from run_inner's "mountain" arm -->
}
```

(If the inline block reads `flow_iters` or other locals, pass them via the `Scheduler` it already holds — confirm `Scheduler` carries `flow_iters` per `biome_page_compute.rs:711`; it does. So the fn needs only `&mut Scheduler`.)

- [ ] **Step 3: Move the block + call the new fn from `run_inner`**

Cut the mountain dispatch sequence out of `run_inner`'s `"mountain"` arm and replace it with `dispatch_mountain_schedule(&mut scheduler);`. Paste the cut lines verbatim into the new fn body. Change NOTHING about the sequence.

- [ ] **Step 4: Isolated cargo check (compiles + existing tests pass)**

Run: `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test -p wg10_terrain 2>&1 | tail -5`
Expected: `test result: ok. 210 passed; 0 failed` (the refactor adds no tests yet; count unchanged). If it fails to compile, the extracted block referenced a local not on `Scheduler` — pass it through or hoist it onto `Scheduler`.

- [ ] **Step 5: Commit (staging list)**

```bash
git add wg-10/rust/src/biome_page_compute.rs
git commit -m "refactor(biome): extract dispatch_mountain_schedule (DRY: one schedule, two hosts)"
```

> **WINDOWED re-prove (owner, after the build):** `--suite biome_page` mountain parity MUST still be ~1.89e-6 (the refactor must not change the math). This is the gate that proves the extraction was verbatim. Do not proceed past Task 3 without it green.

---

## Task 2: Python 256-core f64 oracle for the 576² parity gate

**Files:**
- Create: `tools/dem_pack/export_mountain_576_oracle.py`
- Create (generated): `wg-10/worldgen_terrain/fixtures/mountain_576_oracle.json`

The existing fixtures are 24-core/344-padded. The runtime producer runs at 256-core/576-padded. This exports the SAME mountain recipe from the Python oracle (`mountain_synthesis.py`, the source the Rust port is machine-exact against) at 256-core, apron 160, so the windowed gate can cross-check the GPU producer at PRODUCTION scale (audit gap #6).

- [ ] **Step 1: Write the exporter**

Create `tools/dem_pack/export_mountain_576_oracle.py`:

```python
r"""Export a 256-core / 576-padded mountain f64 oracle for the production-scale GPU parity gate.
The biome fixtures are 24-core/344-padded (fast exact parity); the RUNTIME producer renders at
256-core/576-padded. A scale-dependent math divergence (audit gap #6) would hide from 344 but show
here. Same recipe (mountain_synthesis.generate, apron_px=160) the Rust port is machine-exact against.

Run:    python tools/dem_pack/export_mountain_576_oracle.py
Writes: wg-10/worldgen_terrain/fixtures/mountain_576_oracle.json
"""
from __future__ import annotations
import sys
try:
    sys.stdout.reconfigure(encoding="utf-8")
except (AttributeError, ValueError):
    pass
import json
from pathlib import Path
import numpy as np
import geography_engine as geo
import mountain_synthesis as mountain

OUT = Path(__file__).resolve().parents[2] / "wg-10/worldgen_terrain/fixtures/mountain_576_oracle.json"
CORE_PX = 256
APRON_PX = 160
PADDED = CORE_PX + 2 * APRON_PX        # 576
FEATURE_SPAN_M = 90000.0
SEED = 0
# spacing so 256 core spans the feature extent at production density (matches flow path lengths)
SPACING = FEATURE_SPAN_M / CORE_PX
OX, OZ = 0.0, 0.0

def main() -> None:
    cell = SPACING
    pad_span = cell * (PADDED - 1)
    pad_ox = OX - APRON_PX * cell
    pad_oz = OZ - APRON_PX * cell
    wx, wz = geo.grid(PADDED, pad_span, ox=pad_ox, oz=pad_oz)
    res = mountain.generate(wx, wz, seed=SEED, feature_span_m=FEATURE_SPAN_M, apron_px=APRON_PX)
    h = np.asarray(res["height"], float)          # core-cropped 256x256, normalized pre-relief
    assert h.size == CORE_PX * CORE_PX, f"expected {CORE_PX*CORE_PX} got {h.size}"
    rec = {
        "recipe": "mountain_seamsafe", "style_key": "alpine_branching",
        "seed": SEED, "feature_span_m": FEATURE_SPAN_M, "apron_px": APRON_PX,
        "core_rows": CORE_PX, "core_cols": CORE_PX, "padded_rows": PADDED, "padded_cols": PADDED,
        "grid": {"spacing": SPACING, "ox": OX, "oz": OZ},
        "height": h.tolist(),
    }
    doc = {"generator_version": "recipe_fixtures/v1", "source": "export_mountain_576_oracle.py",
           "note": "256-core/576-padded production-scale mountain oracle for the live-fly parity gate",
           "records": [rec]}
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(doc))
    print(f"wrote {OUT} core={CORE_PX} padded={PADDED} ptp={float(np.ptp(h)):.4f}")

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Run it + verify the artifact**

Run: `cd tools/dem_pack && python export_mountain_576_oracle.py`
Expected: `wrote .../mountain_576_oracle.json core=256 padded=576 ptp=<~1>`
Then verify (artifact, not report): `python -c "import json; d=json.load(open('../../wg-10/worldgen_terrain/fixtures/mountain_576_oracle.json')); r=d['records'][0]; print(sorted(r.keys()), len(r['height']))"`
Expected: keys include `grid,height,padded_rows,core_rows,apron_px,seed,feature_span_m` and `len=65536`.

- [ ] **Step 3: Commit (staging list)**

```bash
git add tools/dem_pack/export_mountain_576_oracle.py wg-10/worldgen_terrain/fixtures/mountain_576_oracle.json
git commit -m "feat(parity): 256-core/576-padded mountain f64 oracle for the live-fly production-scale gate"
```

---

## Task 3: The runtime producer — `BiomePageComputeContext` + `compute_biome_page_cached`

**Files:**
- Create: `wg-10/rust/src/biome_page_runtime.rs`
- Create: `wg-10/rust/src/biome_page_runtime_tests.rs`
- Modify: `wg-10/rust/src/lib.rs` (register module + test mod)

This is the heart. Mirror `page_compute.rs`'s `PageComputeContext` / `build_page_compute_context` / `compute_page_cached`, but: (a) the shader is the mountain biome concat (primitives + machine + mountain fragment); (b) the buffers are the apron field set + gaussian kernels `run_inner` builds today; (c) the per-page dispatch calls `dispatch_mountain_schedule` (Task 1) on the global rd, then crops the core into the target texture with the texel-CORNER mapping. The context is built ONCE (no per-page recompile — the whole point vs the test harness).

- [ ] **Step 1: Write the pure-helper unit test (no GPU)**

Create `wg-10/rust/src/biome_page_runtime_tests.rs`:

```rust
use crate::biome_page_runtime::{biome_apron_dim, core_to_apron_index};

#[test]
fn apron_dim_matches_576_production() {
    assert_eq!(biome_apron_dim(256, 160), 576);
}

#[test]
fn core_to_apron_index_offsets_by_apron() {
    // core (r,c)=(0,0) maps to apron (apron, apron); (core_n-1,core_n-1) to (apron+core_n-1, ...)
    assert_eq!(core_to_apron_index(0, 0, 256, 160), (160, 160));
    assert_eq!(core_to_apron_index(255, 255, 256, 160), (415, 415));
}
```

Add to `lib.rs`:

```rust
mod biome_page_runtime;          // near the other mod lines
#[cfg(test)]
mod biome_page_runtime_tests;    // near the other test mods
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test -p wg10_terrain biome_page_runtime 2>&1 | tail -10`
Expected: FAIL — `biome_page_runtime` module / fns not found.

- [ ] **Step 3: Write the context + pure helpers**

Create `wg-10/rust/src/biome_page_runtime.rs`. Mirror `page_compute.rs` closely. Start with the struct + pure helpers, then the build/free/dispatch on the global rd. Use the existing apron-buffer allocation + gaussian-kernel build from `biome_page_compute.rs::run_inner` (factor a shared builder if clean; otherwise inline the same allocation — the field set is the named buffers 0..N + the 16-slot pool, per `biome_page_compute.rs:249` POOL_SLOTS=16).

```rust
//! WorldGen10 mountain RUNTIME producer (live-fly). Sibling to page_compute.rs's
//! PageComputeContext/compute_page_cached, but hosts the parity-proven mountain biome schedule
//! (dispatch_mountain_schedule) on the GLOBAL rd with a compiled-ONCE context (no per-page
//! recompile -- the difference from the readback test harness Wg10BiomePageCompute). The MATH is
//! unchanged (same schedule); this is the runtime plumbing. Behind the page_pool use_biome_path flag.

use godot::prelude::*;
use godot::classes::RenderingDevice;
// ... (RdShaderSource, RdUniform, ShaderStage, UniformType as in page_compute.rs)

/// Apron working-grid dim for a page: core + apron each side (mountain: 256 + 2*160 = 576).
pub fn biome_apron_dim(core_px: usize, apron_px: usize) -> usize { core_px + 2 * apron_px }

/// Map a core (row,col) to its index in the apron grid (offset by apron on each axis).
pub fn core_to_apron_index(r: usize, c: usize, _core_px: usize, apron_px: usize) -> (usize, usize) {
    (r + apron_px, c + apron_px)
}

pub struct BiomePageComputeContext {
    pub shader: Rid,
    pub pipeline: Rid,
    // persistent apron field buffers (named set + 16-slot pool) + gaussian kernel buffers,
    // built once -- the same set biome_page_compute.rs::run_inner allocates per call, here cached.
    pub fields: Vec<Rid>,
    pub kernels: Vec<Rid>,
    pub core_out: Rid,
    pub apron_dim: usize,
    pub apron_px: usize,
    pub flow_iters: usize,
}

/// Build the mountain runtime context ONCE on the global rd. `concat` = primitives+machine+mountain
/// fragment (the proven concat, via concat_glsl_hoist_version). `flow_iters` = the production
/// convergence count at the apron dim.
pub(crate) fn build_biome_page_context(
    rd: &mut Gd<RenderingDevice>,
    primitives_src: &str, machine_src: &str, mountain_fragment_src: &str,
    core_px: usize, apron_px: usize, flow_iters: usize,
) -> Result<BiomePageComputeContext, String> {
    // 1. concat + hoist #version + strip #[...]; compile; pipeline_create (mirror build_page_compute_context)
    // 2. allocate the apron field buffers + the per-sigma gaussian kernels (mountain sigmas:
    //    1.15,1.20,1.80,2.00,5.00,7.00,valley_width,floor_smooth) -- the same set run_inner builds.
    //    Build kernels CPU-side via the array_ops gaussian_kernel1d port already used in run_inner.
    // 3. return the context (all RIDs owned here; freed by free_biome_page_context).
    todo!("mirror build_page_compute_context + run_inner's allocation; see those two for the exact calls")
}

pub(crate) fn free_biome_page_context(rd: &mut Gd<RenderingDevice>, ctx: &BiomePageComputeContext) {
    for &r in ctx.fields.iter().chain(ctx.kernels.iter()) { rd.free_rid(r); }
    rd.free_rid(ctx.core_out);
    rd.free_rid(ctx.pipeline);
    rd.free_rid(ctx.shader);
}

/// Produce ONE page into `target_rid` (R32F image, binding 0) on the global rd. Builds a Scheduler
/// bound to the cached context buffers, runs dispatch_mountain_schedule, crops the core into the
/// target using the texel-CORNER mapping (height_page.glsl:183-195: texel0->origin, N-1->origin+span).
#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_biome_page_cached(
    rd: &mut Gd<RenderingDevice>,
    ctx: &BiomePageComputeContext,
    target_rid: Rid,
    origin_x: f64, origin_z: f64, world_span: f64, page_px: i64,
    feature_span_m: f64, seed: i64,
) -> Result<(), String> {
    // 1. spacing = world_span / (page_px - 1)  [texel-CORNER: denom = page_px-1]
    //    apron origin = origin - apron_px*spacing (apron-padded grid, same as the fixture path)
    // 2. build a Scheduler over ctx.fields/kernels/pipeline with ctx.flow_iters, set the push constant
    //    (rows=cols=apron_dim, apron_px, seed, spacing, ox, oz, feature_span_m, flow_power=...)
    // 3. crate::biome_page_compute::dispatch_mountain_schedule(&mut scheduler)
    // 4. a CROP pass writes the core region into target_rid at binding 0 (imageStore), mapping
    //    core (r,c) -> apron (r+apron, c+apron) -> target texel (c, r). Reuse the machine's crop pass.
    // 5. barriers between dependent passes; submit on the global rd (the pool drives submit timing).
    todo!("mirror compute_page_cached's bind+push+dispatch, but run the mountain schedule + crop")
}
```

The `todo!()`s are the real work — they are not the deliverable; the engineer fills them by mirroring the two cited functions (`build_page_compute_context`, `compute_page_cached`) and `run_inner`'s allocation/dispatch. The pure helpers (`biome_apron_dim`, `core_to_apron_index`) are fully specified and Task-1-style tested.

- [ ] **Step 4: Implement the build/dispatch bodies (mirror the cited fns)**

Replace the `todo!()`s. `build_biome_page_context` mirrors `build_page_compute_context` (`page_compute.rs:111`) for compile+pipeline, plus `run_inner`'s buffer/kernel allocation. `compute_biome_page_cached` mirrors `compute_page_cached` (`page_compute.rs:169`) for the bind/push/dispatch shape, but runs `dispatch_mountain_schedule` + crop instead of the kernel pass. Keep all RID ownership in the context (free in `free_biome_page_context`) — the B1 leak lesson.

- [ ] **Step 5: Run the unit tests to verify they pass**

Run: `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test -p wg10_terrain biome_page_runtime 2>&1 | tail -10`
Expected: PASS (2 pure-helper tests). Full suite: `... cargo test -p wg10_terrain 2>&1 | tail -3` -> `212 passed` (210 + 2).

- [ ] **Step 6: Commit (staging list)**

```bash
git add wg-10/rust/src/biome_page_runtime.rs wg-10/rust/src/biome_page_runtime_tests.rs wg-10/rust/src/lib.rs
git commit -m "feat(runtime): mountain biome page producer on the global RD (cached context, proven schedule)"
```

---

## Task 4: The 576² cross-oracle parity gate (windowed)

**Files:**
- Create: `wg-10/worldgen_terrain/tests/biome_page_576_parity_check.gd`
- Modify: `tools/gate.py` (add a `biome_fly` suite)

Gate the runtime producer at 256-core/576-padded against the Task-2 Python f64 oracle. This is audit gap #6 closed at production scale. It needs a `#[func]` test entry on the producer that runs ONE page and reads back the core (readback ONLY in this test entry, never the render path).

- [ ] **Step 1: Add a readback test entry to the producer**

In `biome_page_runtime.rs`, add a thin `#[func]`-exposed wrapper class (or extend `Wg10BiomePageCompute` with a `generate_runtime_page_576(...) -> PackedFloat64Array` that builds a context, produces one page into a scratch texture, reads it back, frees). Model the readback on `biome_page_compute.rs`'s existing `generate_core_page` readback. Register in lib.rs.

- [ ] **Step 2: Write the windowed parity check**

Create `wg-10/worldgen_terrain/tests/biome_page_576_parity_check.gd` (copy the skip/two-tier shape of `biome_page_parity_check.gd`):

```gdscript
extends SceneTree

# Production-scale (256-core/576-padded) cross-oracle parity: the RUNTIME mountain producer vs the
# independent Python f64 oracle (mountain_576_oracle.json). Closes audit gap #6 (the 344 fixture
# proved math only at fixture scale; this proves it at the dim the live fly actually renders).
# WINDOWED only (local RD null headless -> skip rc 2).

const ORACLE := "res://worldgen_terrain/fixtures/mountain_576_oracle.json"
const NORM_EPS := 1.0e-4   # the proven biome bar; record achieved maxd, tighten/justify per discipline

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10BiomePageCompute"):
		push_error("[wg10-576-parity] Wg10BiomePageCompute not registered"); return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-576-parity] status=skip reason=no-gpu"); return 2
	probe.free()
	var f := FileAccess.open(ORACLE, FileAccess.READ)
	if f == null: push_error("[wg10-576-parity] missing oracle %s" % ORACLE); return 1
	var doc: Dictionary = JSON.parse_string(f.get_as_text())
	var rec: Dictionary = doc["records"][0]
	var grid: Dictionary = rec["grid"]
	var core_n := int(rec["core_rows"])
	var expected: Array = rec["height"]
	var gpu: Object = ClassDB.instantiate("Wg10BiomePageCompute")
	gpu.call("load_shaders",
		ProjectSettings.globalize_path("res://worldgen_terrain/shaders/recipe_primitives.glsl"),
		ProjectSettings.globalize_path("res://worldgen_terrain/shaders/biome_page.glsl"))
	var got: PackedFloat64Array = gpu.call("generate_runtime_page_576",
		float(grid["spacing"]), float(grid["ox"]), float(grid["oz"]),
		int(rec["padded_rows"]), int(rec["padded_cols"]), int(rec["apron_px"]),
		int(rec["seed"]), float(rec["feature_span_m"]),
		ProjectSettings.globalize_path("res://worldgen_terrain/shaders/biome_mountain.glsl"))
	if got.size() != core_n * core_n:
		push_error("[wg10-576-parity] size got=%d exp=%d" % [got.size(), core_n*core_n]); return 1
	var max_d := 0.0; var fails := 0
	for i in range(got.size()):
		var d: float = absf(got[i] - float(expected[i]))
		max_d = maxf(max_d, d)
		if d > NORM_EPS:
			fails += 1
			if fails <= 5: push_error("[wg10-576-parity] core[%d] gpu=%f exp=%f d=%s" % [i, got[i], expected[i], str(d)])
	if max_d != max_d: push_error("[wg10-576-parity] NaN delta (degenerate)"); return 1
	if fails > 0:
		print("[wg10-576-parity] status=fail core=%d fails=%d maxd=%s" % [got.size(), fails, str(max_d)]); return 1
	print("[wg10-576-parity] status=pass core=%d maxd=%s eps=%s" % [got.size(), str(max_d), str(NORM_EPS)]); return 0
```

(If the runtime producer uses a different flow_iters than the oracle's convergence, the channel regions will diverge — that's the under-convergence signal §6 names; set the producer's `flow_iters` to the `flow_converge` mountain count.)

- [ ] **Step 3: Add the `biome_fly` suite to `tools/gate.py`**

Mirror the `biome_page` entry (windowed, skip-allowed):

```python
"biome_fly": [
    "worldgen_terrain/tests/biome_page_576_parity_check.gd",
    "worldgen_terrain/tests/biome_fly_perf_check.gd",   # added in Task 6
],
```

Add `"biome_fly"` to the windowed (non-headless) set on the `headless = args.suite not in (...)` line.

- [ ] **Step 4: Isolated cargo check (the new #[func] compiles)**

Run: `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test -p wg10_terrain 2>&1 | tail -3`
Expected: green (count unchanged or +0; the readback entry has no new unit test here).

- [ ] **Step 5: WINDOWED run (OWNER / editor closed)**

Run: `env -u CARGO_TARGET_DIR GODOT_BIN=<godot-4.6.2> python tools/gate.py --suite biome_fly` (will skip the perf check until Task 6 exists, or run only the parity check first).
Expected: `[wg10-576-parity] status=pass core=65536 maxd=<= 1e-4>`. If maxd is large ONLY in channel regions -> raise the producer's flow_iters (under-converged). If large everywhere -> a crop/mapping or pass bug; debug against the 344 path which is known-good.

- [ ] **Step 6: Commit (staging list)**

```bash
git add wg-10/rust/src/biome_page_runtime.rs wg-10/rust/src/lib.rs wg-10/worldgen_terrain/tests/biome_page_576_parity_check.gd tools/gate.py
git commit -m "feat(parity): 576 production-scale cross-oracle gate (runtime mountain producer vs Python f64)"
```

---

## Task 5: Flag the biome producer into `page_pool`

**Files:**
- Modify: `wg-10/rust/src/page_pool.rs` (add `use_biome_path` + the biome context; route `configure` + `acquire_page`)

- [ ] **Step 1: Add the flag + biome context option to the struct + `configure`**

In `page_pool.rs`, add a field `use_biome_path: bool` (default false) and `biome_ctx: Option<biome_page_runtime::BiomePageComputeContext>`. Add a setter `#[func] pub fn set_use_biome_path(&mut self, on: bool)` (must be called BEFORE `configure`). In `configure` (`page_pool.rs:107`), after the existing pack/glsl load, branch: if `use_biome_path`, build the biome context (load the three shader sources, call `build_biome_page_context` with core_px=page_px, apron_px=160, flow_iters=<mountain convergence count>) and store it in `biome_ctx`; else build the legacy `compute_ctx` as today. Free `biome_ctx` in `free_all_impl` / `reset_configured_state` alongside `compute_ctx` (the B1/F8 lifecycle discipline — `page_pool.rs:556`).

- [ ] **Step 2: Route `acquire_page` (both the fresh + eviction-recompute sites)**

At the two producer call sites (`page_pool.rs:272` fresh + `:310` eviction), branch on `use_biome_path`:

```rust
let result = if self.use_biome_path {
    let bctx = self.biome_ctx.as_ref().unwrap();
    crate::biome_page_runtime::compute_biome_page_cached(
        &mut rd, bctx, tex_rid, ox, oz, ws, ppx, FEATURE_SPAN_M_MOUNTAIN, sd,
    )
} else {
    let ctx = self.compute_ctx.as_ref().unwrap();
    compute_page_cached(&mut rd, ctx, &self.pack.as_ref().unwrap().grammar_constants,
        self.pack_buffers.as_ref().unwrap().num_palettes, tex_rid, ox, oz, ws, ppx, sd)
};
```

(Define `const FEATURE_SPAN_M_MOUNTAIN: f64 = 90000.0;` near the top — the mountain feature span the all-mountain world uses.)

- [ ] **Step 3: Guard the `is_configured` check for the biome path**

`is_configured()` / the `acquire_page` guard currently require `compute_ctx.is_some()`. Extend so the biome path is "configured" when `biome_ctx.is_some()` (and the legacy path when `compute_ctx.is_some()`). A wrong guard here re-introduces the F7 acquire-panic; mirror that fix's shape.

- [ ] **Step 4: Isolated cargo check + existing pool tests**

Run: `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test -p wg10_terrain page_pool 2>&1 | tail -10`
Expected: the existing page_pool unit tests pass (the flag defaults off -> legacy path unchanged). Full suite green.

- [ ] **Step 5: Commit (staging list)**

```bash
git add wg-10/rust/src/page_pool.rs
git commit -m "feat(pool): use_biome_path flag routes the producer to the mountain biome path"
```

---

## Task 6: The fly scene + did-real-work live perf gate (windowed)

**Files:**
- Create: `wg-10/worldgen_terrain/harness/mountain_fly_review.tscn` + `mountain_fly_review.gd`
- Create: `wg-10/worldgen_terrain/tests/biome_fly_perf_check.gd`

- [ ] **Step 1: Build the fly scene (A/B toggle)**

Create `mountain_fly_review.tscn` by copying `wg-10/worldgen_terrain/harness/m3_review.tscn` (the existing M3 streaming scene — clipmap rings + terrain view + page pool). In its `.gd`, before configuring the pool, call `pool.set_use_biome_path(true)`. Add an input toggle (key `B`) that reconfigures with the flag off/on for live A/B (legacy kernel vs mountain biome). Wire a `fly` camera at the ~1000 m/s target (reuse `fly_camera.gd`).

- [ ] **Step 2: Write the did-real-work perf gate**

Create `wg-10/worldgen_terrain/tests/biome_fly_perf_check.gd` (model `m5_perf_hardened_check.gd` + memory `worldgen10-real-gpu-time`):

```gdscript
extends SceneTree

# Live did-real-work perf gate for the mountain biome streaming path. Anti-fooling: a green p99 with
# zero streamed pages / a black frame / a silent legacy fallback is FORBIDDEN -- all are asserted. The
# p99 is RECORDED (not asserted-pass blindly): inline 576 flow ~6.45ms may legitimately be over the
# half-budget under fast motion -- that is DATA (spec 5), the gate fails only on degenerate/no-work.
# WINDOWED only. Uses RenderingServer.viewport_get_measured_render_time_gpu (real, not wall).

const BUDGET_MS := 6.0
const MIN_PAGES := 1            # must stream at least one biome page under motion

func _init() -> void:
	quit(_run())

func _run() -> int:
	# ... set up the biome-path streaming scene, fly N frames at ~1000 m/s, collect:
	#   - real GPU-time samples (viewport_get_measured_render_time_gpu)
	#   - streamed page count (pool.stats)
	#   - non-black + terrain-vs-sky frac (B3 discipline)
	#   - biome_path_active assertion (pool reports the flag on)
	# Anti-fooling fails (return 1): pages_streamed < MIN_PAGES, black frame, biome path NOT active.
	# Otherwise RECORD p99 (print over/under budget) and return 0 (measurement, like page_measure).
	push_error("[wg10-biome-fly] IMPLEMENT: see m5_perf_hardened_check.gd for the harness shape")
	return 1
```

Fill the body from `m5_perf_hardened_check.gd` (the proven hardened-perf harness): reuse its terrain-vs-sky `_terrain_frac`, its GPU-time sampling, and add the `pages_streamed` (from `pool.stats()`) + `biome_path_active` assertions. The gate FAILS on degenerate/no-work; it RECORDS the p99 verdict.

- [ ] **Step 3: WINDOWED run (OWNER / editor closed)**

Run: `env -u CARGO_TARGET_DIR GODOT_BIN=<godot-4.6.2> python tools/gate.py --suite biome_fly`
Expected: `[wg10-576-parity] status=pass` + `[wg10-biome-fly] ... pages_streamed=<N> biome_path_active=true p99=<ms> verdict=<under|over>-budget status=pass`. An OVER-budget p99 is a valid pass (it's the §5 measurement); a zero-pages / black / legacy-fallback result is a FAIL.

- [ ] **Step 4: Commit (staging list)**

```bash
git add wg-10/worldgen_terrain/harness/mountain_fly_review.tscn wg-10/worldgen_terrain/harness/mountain_fly_review.gd wg-10/worldgen_terrain/tests/biome_fly_perf_check.gd
git commit -m "feat(fly): mountain biome streaming scene (A/B) + did-real-work live perf gate"
```

---

## Task 7: No-regression + owner fly review + docs

**Files:**
- Verify: `facts_collision_parity_check.gd`, the `m3` suite (unchanged paths)
- Modify: `docs/plans/STATUS.md`, `docs/plans/HANDOFF.md`, `docs/plans/LOOSE_ENDS_LEDGER.md`

- [ ] **Step 1: WINDOWED no-regression (OWNER / editor closed)**

Run: `env -u CARGO_TARGET_DIR GODOT_BIN=<godot-4.6.2> python tools/gate.py --suite m3` (with the scene's default flag OFF, so this proves the legacy path is untouched) AND `--suite biome_page` (re-prove the 344 parity post-refactor).
Expected: `m3` green (legacy unchanged), `biome_page` mountain ~1.89e-6 (refactor verbatim).

- [ ] **Step 2: OWNER FLY REVIEW (acceptance authority)**

Owner launches `mountain_fly_review.tscn`, flies the all-mountain world, toggles A/B (key `B`). The owner judges the surfaced/in-motion look (spec §4.4). Do NOT self-approve. Record the owner's verdict + any perf observation (stall/smooth) — the §5 measurement that decides drainage-bake priority.

- [ ] **Step 3: Update STATUS/HANDOFF/LEDGER**

Record: mountain runs LIVE in the streaming runtime behind `use_biome_path` (576 cross-oracle parity green = maxd `<n>`; live perf p99 `<n>` = under/over budget; owner look verdict `<v>`). Legacy still the default flag-off. The §5 drainage-priority finding (inline flow tolerable / needs bake) -> the next slice. Other 10 biomes + PART B + atlas-removal still deferred.

- [ ] **Step 4: Commit + push (OWNER-triggered)**

```bash
git add docs/plans/STATUS.md docs/plans/HANDOFF.md docs/plans/LOOSE_ENDS_LEDGER.md
git commit -m "docs: mountain live in the streaming runtime behind use_biome_path (parity+perf+owner verdict)"
# push only when the owner says
```

---

## Notes for the implementer

- **The math is already proven.** Tasks 1/3 must not change the mountain pass sequence — `dispatch_mountain_schedule` is a verbatim extraction, and the 344 `biome_page` parity (~1.89e-6) is the proof it stayed verbatim. If that number moves, the extraction changed the math — revert and re-extract.
- **Texel-CORNER mapping is load-bearing** (`height_page.glsl:183-195`): denom = `page_px-1`, texel 0 -> origin. Get this wrong and the clipmap seams; the `m3` moving-camera gate catches it.
- **RID lifecycle** (B1 lesson): the biome context owns all its RIDs; free them in `free_biome_page_context`, called from the pool's `free_all_impl`/reconfigure. A leak here exhausts the device over a long fly.
- **The over-budget perf result is a feature** (spec §5): it's the real measurement that was previously a spike. Don't "fix" it by widening the budget — record it and let it drive the drainage-bake decision.
- **Windowed steps are owner-run or editor-closed.** Never claim a windowed pass you didn't watch.
