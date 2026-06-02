# Slice 4: GPU Page Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the legacy kernel-tiling page formula (`height_page.glsl` + 25 MB atlas) with the accepted 11-biome composition stack running ON the GPU in the grid-shaped page path, parity-gated against the f64 oracle, then flip the runtime and delete the atlas.

**Architecture:** Three sub-slices. **4a** proves the architecture on ONE biome (mountain): measure the real per-page GPU cost FIRST (it decides the page pipeline), then port the noise/array primitives + the mountain recipe to GLSL, parity-gate the GPU page against the committed f64 fixture, all behind a flag with the legacy path still the runtime default. **4b** generalizes to the other 10 recipes + `compose_biomes` + the grammar biome-weight field. **4c** flips the runtime default, removes the atlas, re-runs the hardened perf gate, and gets the owner fly review.

**Tech Stack:** Rust GDExtension (`godot` crate) on the global RenderingDevice; GLSL compute (`#version 450`, base profile — u32/f32 only, no 64-bit ints); Godot 4.6 windowed SceneTree checks (RD compute is null headless on this D3D12 box → skip rc 2, never false-pass); `tools/gate.py` suites.

**Parity bar (two-tier, from the spec §4):**
- **Tier 1 — EXACT structural decisions** (grammar/biome-weight selection, recipe dispatch): integer/threshold logic must match the CPU bit-for-bit.
- **Tier 2 — composed HEIGHT within a documented f32 tolerance, relief-relative:** start from the existing M2 budget `ABS_EPS = 1.0e-2` m (see `gpu_parity_check.gd:10`). The flow contribution is the approximated part and falls under Tier 2. Widen ONLY with a recorded justification from observed f32 + flow-approximation error.

**Parity ORACLE decision (owner-approved 2026-06-02):** the GPU page is compared against the **committed f64 fixtures** the CPU port is already proven against (`tools/dem_pack/fixtures/recipe_*_fixture.json`, `biome_compose_fixture.json`), NOT a new `#[func]` CPU bridge. The fixture stores the apron-meshgrid params (`spacing, ox, oz, apron_px`) + the f64 core-cropped height; the GPU rebuilds the same meshgrid, generates the core, and is compared to the stored height. This ties the GPU directly to the f64 oracle with zero new Godot-facing surface.

---

## File Structure

**New GLSL (the GPU mirror of the Rust stack):**
- `wg-10/worldgen_terrain/shaders/recipe_primitives.glsl` — GLSL mirror of `recipe_noise.rs` + `array_ops.rs`'s gaussian (per-point noise + the separable gaussian; NOT flow, which is the relaxation shader). Included by the page shaders. One file: these are the shared leaf primitives every recipe needs.
- `wg-10/worldgen_terrain/shaders/biome_page_4a.glsl` — the 4a single-biome (mountain) apron page pipeline shader (or multi-dispatch set, per the §3.1 decision the measurement makes). Mountain recipe math + flow + crop.
- `wg-10/worldgen_terrain/shaders/biome_page.glsl` (4b) — generalized: all recipes + `compose_biomes` + grammar weights. May supersede `biome_page_4a.glsl` or grow from it.

**New Rust:**
- `wg-10/rust/src/page_measure.rs` (4a) — `Wg10PageMeasure` `#[func]` class: the real-per-page-cost measurement spike (extends `flow_spike.rs`'s honest local-RD wall-differential method to 576² apron + the mountain recipe work). MEASUREMENT-ONLY, not wired to render.
- `wg-10/rust/src/biome_page_compute.rs` (4a→4c) — `Wg10BiomePageCompute` `#[func]` class: the new biome page producer on the global RD, behind a flag. The 4c runtime replacement for `page_compute.rs`'s kernel path.

**Modified Rust:**
- `wg-10/rust/src/lib.rs` — register the new classes + their test mods.
- `wg-10/rust/src/page_pool.rs` (4c) — switch the configured producer from the kernel context to the biome context behind the flag; drop the atlas buffers from the new path.
- `wg-10/rust/src/page_compute.rs` (4c) — mark legacy, ensure not called on the new path.

**New GDScript checks (windowed; copy `gpu_parity_check.gd` / `flow_spike_check.gd` shape):**
- `wg-10/worldgen_terrain/tests/page_measure_check.gd` (4a) — drives `Wg10PageMeasure`, prints real per-page ms, VERDICT fits/over budget.
- `wg-10/worldgen_terrain/tests/biome_page_parity_check.gd` (4a→4b) — GPU page vs committed fixture, two-tier.
- `wg-10/worldgen_terrain/tests/biome_page_perf_check.gd` (4c) — hardened GPU-time p99 < 6 ms with did-real-work assertions.

**Modified fixtures/tooling:**
- `tools/dem_pack/export_recipe_*_fixture.py` — extend the mountain fixture export to also emit GLSL-parity sample metadata if not already present (apron params are already stored — verify in Task 4a.4).
- `tools/gate.py` — add `page_measure`, `biome_page` suites; wire the new checks.

**Decision doc (updated as 4a resolves):**
- `docs/superpowers/specs/2026-06-02-worldgen-slice4-gpu-page-integration-design.md` §3.1 — record the measured number + the chosen pipeline.

---

## SLICE 4a — prove the architecture on ONE biome (mountain), behind a flag

### Task 4a.1: Measure the real per-page GPU cost (spike) — Rust class

**Files:**
- Create: `wg-10/rust/src/page_measure.rs`
- Modify: `wg-10/rust/src/lib.rs` (register class + test mod)
- Create: `wg-10/rust/src/page_measure_tests.rs`

This task ports the flow-spike measurement to TRUE per-page dimensions so the §3.1 pipeline decision is made on a real number, not an assumption. It measures: a `core_px + 2*apron` working grid (mountain `apron_px = 160`, so `256 + 320 = 576²`), the full flow relaxation at the stable iteration count the flow spike found, PLUS representative recipe work (the warp/ridge/fbm/gaussian load). The honest metric is the wall-differential across two grid sizes / iteration counts (the flow-spike finding: `get_captured_timestamp_gpu_time` is unreliable on local RD; wall-differential cancels fixed submit overhead — see `flow_spike_check.gd:104-118`).

- [ ] **Step 1: Write the failing unit test (pure helpers)**

Create `wg-10/rust/src/page_measure_tests.rs`:

```rust
use crate::page_measure::{apron_dim, recipe_load_field};

#[test]
fn apron_dim_adds_two_aprons() {
    // core 256 + 2*160 apron = 576
    assert_eq!(apron_dim(256, 160), 576);
    assert_eq!(apron_dim(256, 0), 256);
}

#[test]
fn recipe_load_field_is_finite_and_right_size() {
    let f = recipe_load_field(64, 7);
    assert_eq!(f.len(), 64 * 64);
    assert!(f.iter().all(|v| v.is_finite()));
}

#[test]
fn recipe_load_field_deterministic() {
    assert_eq!(recipe_load_field(48, 3), recipe_load_field(48, 3));
}
```

Add to `wg-10/rust/src/lib.rs` test mods (near the other `mod *_tests;` lines, e.g. after line 85):

```rust
#[cfg(test)]
mod page_measure_tests;
```

And register the module (near the other `mod` lines, after `mod recipes_wetland;` ~line 34):

```rust
mod page_measure;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test -p wg10_terrain page_measure_tests 2>&1 | tail -20`
Expected: FAIL — `page_measure` module / `apron_dim` not found.

- [ ] **Step 3: Write the spike class with the pure helpers**

Create `wg-10/rust/src/page_measure.rs`. Model the GPU dispatch closely on `flow_spike.rs::run_inner` (local RD, compile shader, ping-pong acc buffers, GPU timestamps + wall-clock, free + `rd.free()`), but: (a) the height field fed to flow is `recipe_load_field` (a representative recipe surface, not the spike's `make_ridged_field`), and (b) the dimension is the apron dim. Reuse `flow_accum_spike.glsl` for the flow passes — this task measures cost, it does NOT need recipe-exact GLSL yet (that's Task 4a.3).

```rust
//! WorldGen10 Slice-4a MEASUREMENT spike: real per-page GPU cost at apron dimensions.
//!
//! Answers the spec §3.1 OPEN question: does a per-page LIVE pipeline (apron grid +
//! flow relaxation + recipe work) fit the frame budget, or must we fall back to a
//! coarse-drainage-fact cache? Extends `flow_spike.rs` from the 256² flow-only spike
//! to the TRUE per-page working grid (core_px + 2*apron) with a representative recipe
//! load. MEASUREMENT-ONLY, never wired to the render path. WINDOWED only (local RD is
//! null headless on this box).
//!
//! Honest metric = WALL-clock differential across grid sizes / iteration counts (the
//! flow-spike finding: get_captured_timestamp_gpu_time is unreliable on local RD; the
//! differential cancels fixed per-submit overhead). See `page_measure_check.gd`.

use godot::prelude::*;
use godot::classes::{
    RenderingServer, RdShaderSource, RdUniform, RenderingDevice,
    rendering_device::{UniformType, ShaderStage},
};

/// Working-grid dimension for a page: core + an apron on each side.
pub fn apron_dim(core_px: usize, apron_px: usize) -> usize {
    core_px + 2 * apron_px
}

/// Representative recipe SURFACE for the flow pass to route through: a multi-octave
/// ridged sum (mirrors the structure the real mountain recipe feeds into flow_channels).
/// Row-major f32, length dim*dim. This stands in for the recipe's `base` field so the
/// measured flow cost is on a realistic surface; it is NOT recipe-exact (Task 4a.3 is).
pub fn recipe_load_field(dim: usize, seed: i32) -> Vec<f32> {
    // Reuse the flow-spike's ridged generator structure (representative rough surface).
    crate::flow_spike::make_ridged_field(dim, seed)
}

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10PageMeasure {
    glsl_source: Option<String>,
    last_wall_us: f64,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10PageMeasure {
    fn init(base: Base<RefCounted>) -> Self {
        Self { glsl_source: None, last_wall_us: 0.0, base }
    }
}

#[godot_api]
impl Wg10PageMeasure {
    /// Load the flow GLSL (reuse flow_accum_spike.glsl for the cost measurement).
    #[func]
    pub fn load_shader(&mut self, glsl_path: GString) -> GString {
        match std::fs::read_to_string(glsl_path.to_string()) {
            Ok(s) => { self.glsl_source = Some(s); GString::new() }
            Err(e) => GString::from(format!("glsl: {e}").as_str()),
        }
    }

    /// Run `iters` flow-relaxation steps on a `dim`×`dim` representative recipe surface.
    /// Returns wall-clock MILLISECONDS around submit()+sync() (the honest upper bound on
    /// real GPU work; the check takes a differential across dims/iters). Negative on error.
    #[func]
    pub fn run(&mut self, dim: i64, iters: i64, power: f64, seed: i64) -> f64 {
        match self.run_inner(dim as usize, iters as usize, power as f32, seed as i32) {
            Ok(wall_us) => { self.last_wall_us = wall_us; wall_us / 1000.0 }
            Err(e) => { godot_error!("Wg10PageMeasure::run error: {e}"); -1.0 }
        }
    }

    #[func]
    pub fn last_wall_us(&self) -> f64 { self.last_wall_us }

    fn run_inner(&self, dim: usize, iters: usize, power: f32, seed: i32) -> Result<f64, String> {
        if iters == 0 { return Err("iters must be >= 1".into()); }
        let glsl = self.glsl_source.as_deref().ok_or("no GLSL source loaded")?;
        let n = dim * dim;
        let mut rd: Gd<RenderingDevice> = RenderingServer::singleton()
            .create_local_rendering_device()
            .ok_or_else(|| "create_local_rendering_device returned null (headless)".to_string())?;

        let glsl_stripped: String = glsl.lines()
            .filter(|l| !l.trim_start().starts_with("#["))
            .collect::<Vec<_>>().join("\n");
        let mut src = RdShaderSource::new_gd();
        src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
        let spirv = rd.shader_compile_spirv_from_source(&src)
            .ok_or_else(|| "shader_compile_spirv_from_source returned null".to_string())?;
        let cerr = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
        if !cerr.is_empty() { return Err(format!("GLSL compile error: {cerr}")); }
        let shader = rd.shader_create_from_spirv(&spirv);
        if shader.is_invalid() { return Err("shader_create_from_spirv invalid".into()); }

        let field = recipe_load_field(dim, seed);
        let to_bytes = |v: &[f32]| -> Vec<u8> {
            let mut b = Vec::with_capacity(v.len() * 4);
            for &x in v { b.extend_from_slice(&x.to_le_bytes()); }
            b
        };
        let height_bytes = to_bytes(&field);
        let ones_bytes = to_bytes(&vec![1.0_f32; n]);
        let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size > u32") };
        let height_rid = rd.storage_buffer_create_ex(bsize(height_bytes.len()))
            .data(&PackedByteArray::from(height_bytes.as_slice())).done();
        let acc_a = rd.storage_buffer_create_ex(bsize(ones_bytes.len()))
            .data(&PackedByteArray::from(ones_bytes.as_slice())).done();
        let acc_b = rd.storage_buffer_create_ex(bsize(ones_bytes.len()))
            .data(&PackedByteArray::from(ones_bytes.as_slice())).done();

        let mut mk = |prev: Rid, next: Rid| -> Rid {
            let mut us: Array<Gd<RdUniform>> = Array::new();
            for (b, r) in [(0, height_rid), (1, prev), (2, next)] {
                let mut u = RdUniform::new_gd();
                u.set_uniform_type(UniformType::STORAGE_BUFFER);
                u.set_binding(b);
                u.add_id(r);
                us.push(&u);
            }
            rd.uniform_set_create(&us, shader, 0)
        };
        let set_ab = mk(acc_a, acc_b);
        let set_ba = mk(acc_b, acc_a);

        let mut push = Vec::with_capacity(16);
        push.extend_from_slice(&(dim as i32).to_le_bytes());
        push.extend_from_slice(&power.to_le_bytes());
        push.extend_from_slice(&0i32.to_le_bytes());
        push.extend_from_slice(&0i32.to_le_bytes());
        let push_pba = PackedByteArray::from(push.as_slice());
        let pipeline = rd.compute_pipeline_create(shader);
        let wg = ((dim as u32) + 15) / 16;

        let cl = rd.compute_list_begin();
        rd.compute_list_bind_compute_pipeline(cl, pipeline);
        for i in 0..iters {
            let set = if i % 2 == 0 { set_ab } else { set_ba };
            rd.compute_list_bind_uniform_set(cl, set, 0);
            rd.compute_list_set_push_constant(cl, &push_pba, push_pba.len() as u32);
            rd.compute_list_dispatch(cl, wg, wg, 1);
            rd.compute_list_add_barrier(cl);
        }
        rd.compute_list_end();

        let wall0 = std::time::Instant::now();
        rd.submit();
        rd.sync();
        let wall_us = wall0.elapsed().as_secs_f64() * 1.0e6;

        rd.free_rid(height_rid);
        rd.free_rid(acc_a);
        rd.free_rid(acc_b);
        rd.free_rid(pipeline);
        rd.free_rid(shader);
        rd.free();
        Ok(wall_us)
    }
}
```

Note: `flow_spike::make_ridged_field` is `pub` (`flow_spike.rs:30`), BUT the `flow_spike` MODULE is declared privately (`lib.rs:36` = `mod flow_spike;`), so `crate::flow_spike::make_ridged_field` will NOT resolve from `page_measure`. In the same lib.rs edit, change `mod flow_spike;` → `pub(crate) mod flow_spike;`. (This is a required prerequisite, not conditional.)

- [ ] **Step 4: Verify flow_spike visibility, then run tests to verify they pass**

Run: `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test -p wg10_terrain page_measure_tests 2>&1 | tail -20`
Expected: PASS (3 tests). If it fails on `make_ridged_field` private, make `flow_spike` mod `pub(crate)` in lib.rs and re-run.

- [ ] **Step 5: Commit**

```bash
git add wg-10/rust/src/page_measure.rs wg-10/rust/src/page_measure_tests.rs wg-10/rust/src/lib.rs
git commit -m "slice4a: per-page cost measurement spike (apron dims + recipe load)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 4a.2: Measurement gate (windowed) + record the §3.1 decision

**Files:**
- Create: `wg-10/worldgen_terrain/tests/page_measure_check.gd`
- Modify: `tools/gate.py` (add `page_measure` suite)
- Modify: `docs/superpowers/specs/2026-06-02-worldgen-slice4-gpu-page-integration-design.md` (record the number)

- [ ] **Step 1: Write the windowed measurement check**

Create `wg-10/worldgen_terrain/tests/page_measure_check.gd` (copy the skip/verdict shape of `flow_spike_check.gd`):

```gdscript
extends SceneTree

# WorldGen10 Slice-4a MEASUREMENT gate: real per-page GPU cost at APRON dimensions.
# Decides spec §3.1 (per-page-live vs coarse-drainage-fact). WINDOWED only (local RD
# null headless -> skip rc 2). Honest metric = wall-differential across dims (cancels
# fixed submit overhead), same model as flow_spike_check.gd.

const GLSL := "res://worldgen_terrain/shaders/flow_accum_spike.glsl"
const CORE_PX := 256
const APRON_PX := 160          # mountain MOUNTAIN_APRON_PX (recipes.rs::mountain::APRON_PX)
const APRON_DIM := CORE_PX + 2 * APRON_PX   # 576
const POWER := 1.45
const SEED := 1337
const STABLE_ITERS := 128      # flow-spike's converged iteration count
const REPEATS := 8
const BUDGET_MS := 6.0

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10PageMeasure"):
		push_error("[wg10-page-measure] Wg10PageMeasure not registered - run WINDOWED, rebuilt dll")
		return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-page-measure] status=skip reason=no-gpu")
		return 2
	probe.free()

	var m: Object = ClassDB.instantiate("Wg10PageMeasure")
	var err: String = str(m.call("load_shader", ProjectSettings.globalize_path(GLSL)))
	if err != "":
		push_error("[wg10-page-measure] shader load failed: %s" % err)
		return 1

	# warm-up (pays first-dispatch compile/alloc we don't want timed)
	if float(m.call("run", APRON_DIM, 8, POWER, SEED)) < 0.0:
		push_error("[wg10-page-measure] warm-up failed"); return 1

	# Best (min) wall ms at the apron dim, at the stable iter count and at a small iter count;
	# the differential isolates the marginal flow cost on the FULL apron grid.
	var best := func(dim: int, iters: int) -> float:
		var b := 1.0e30
		for r in range(REPEATS):
			var ms: float = m.call("run", dim, iters, POWER, SEED)
			if ms < 0.0: return -1.0
			b = minf(b, ms)
		return b

	var wall_hi: float = best.call(APRON_DIM, STABLE_ITERS)
	var wall_lo: float = best.call(APRON_DIM, 8)
	if wall_hi < 0.0 or wall_lo < 0.0:
		push_error("[wg10-page-measure] run failed"); return 1
	var per_iter_ms: float = (wall_hi - wall_lo) / float(STABLE_ITERS - 8)
	var flow_marginal_ms: float = per_iter_ms * float(STABLE_ITERS)

	# Decision: per-page-live fits if the flow pass leaves >= half the 6ms budget for the
	# recipe height work + the rest of the frame (same threshold as flow_spike_check.gd).
	var fits := flow_marginal_ms < (BUDGET_MS * 0.5)
	var pipeline := "per-page-live" if fits else "coarse-drainage-fact-fallback"
	print("[wg10-page-measure] apron_dim=%d stable_iters=%d per_iter_ms=%.5f flow_marginal_ms=%.4f half_budget_ms=%.2f PIPELINE=%s wall_hi=%.4f wall_lo=%.4f" % [
		APRON_DIM, STABLE_ITERS, per_iter_ms, flow_marginal_ms, BUDGET_MS * 0.5, pipeline, wall_hi, wall_lo])

	# This is a MEASUREMENT gate: it must SUCCEED at producing a number (non-degenerate),
	# not assert a particular verdict — both pipeline branches are valid spec outcomes.
	# Degenerate = non-positive marginal (timer broke) -> FAIL so the number is never trusted blind.
	if not (flow_marginal_ms > 0.0):
		push_error("[wg10-page-measure] FAIL: degenerate marginal (timer unreliable) flow_marginal_ms=%.4f" % flow_marginal_ms)
		return 1
	print("[wg10-page-measure] status=pass (measurement recorded; decision=%s)" % pipeline)
	return 0
```

- [ ] **Step 2: Add the `page_measure` suite to `tools/gate.py`**

In `tools/gate.py`, add a suite entry mirroring `gpu_flow` (windowed, allows skip rc 2). Locate the suite registry (the dict/table mapping suite name → list of check scripts) and add:

```python
"page_measure": ["worldgen_terrain/tests/page_measure_check.gd"],
```

Match the EXACT structure already used for `gpu_flow` — copy that entry's surrounding fields (timeout, windowed flag, skip-allowed). Run `git show 1d6a882:tools/gate.py` if needed to see the `gpu_flow` entry shape.

- [ ] **Step 3: Run the measurement gate WINDOWED**

This requires a real GPU device → the owner must run it (or run it as a separate windowed Godot instance; do NOT kill the owner's editor — memory `worldgen10-dont-kill-editor`). Command (windowed, from repo root, Godot 4.6.2):

Run: `env -u CARGO_TARGET_DIR GODOT_BIN=<godot-4.6.2> python tools/gate.py --suite page_measure`
Expected: `[wg10-page-measure] ... PIPELINE=<per-page-live|coarse-drainage-fact-fallback> ... status=pass`

- [ ] **Step 4: Record the decision in the spec §3.1**

Edit `docs/superpowers/specs/2026-06-02-worldgen-slice4-gpu-page-integration-design.md` §3.1: replace the "decided by the 4a measurement" framing with the actual measured `flow_marginal_ms` and the chosen pipeline (per-page-live OR coarse-fact). This is the documented measurement gate the spec §5 requires.

> **DECISION FORK — affects Tasks 4a.5/4b:** if PIPELINE=per-page-live, the page shader runs the flow relaxation inline per page (proceed as written below). If PIPELINE=coarse-drainage-fact-fallback, STOP and escalate to the human: the page-pipeline tasks below assume per-page-live, and the coarse-fact path needs its own bake/cache design (spec §3.1 FALLBACK) which is out of this plan's scope. Do not silently build the wrong pipeline.

- [ ] **Step 5: Commit**

```bash
git add wg-10/worldgen_terrain/tests/page_measure_check.gd tools/gate.py docs/superpowers/specs/2026-06-02-worldgen-slice4-gpu-page-integration-design.md
git commit -m "slice4a: per-page cost measurement gate + record §3.1 pipeline decision

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 4a.3: GLSL primitives + their parity gate (noise + gaussian)

**Files:**
- Create: `wg-10/worldgen_terrain/shaders/recipe_primitives.glsl`
- Create: `wg-10/worldgen_terrain/tests/primitive_parity_check.gd`
- Modify: `tools/gate.py` (add to `biome_page` suite)

The primitives must reproduce `recipe_noise.rs` in f32. CRITICAL hash detail (from `recipe_noise.rs:26-43`): the lattice hash uses **i64 wrapping arithmetic with a 31-bit mask** and an **arithmetic (sign-preserving) `>> 13`**. GLSL base profile has no 64-bit ints. The Rust f64 oracle runs i64 wrapping math; matching it in f32 GLSL is the parity risk this task de-risks. Approach: emulate the i64 hash with two u32 words (lo/hi) for the wrapping multiply-add, OR — if the parity gate shows u32-only is within Tier-2 — use a u32 hash and prove it stays within tolerance. The gate decides; do NOT assume.

- [ ] **Step 1: Write the primitives GLSL (include file)**

Create `wg-10/worldgen_terrain/shaders/recipe_primitives.glsl`. Mirror each `recipe_noise.rs` function. Start with the hash (the parity-critical one) emulating i64 wrapping with two u32 halves:

```glsl
// WorldGen10 GLSL mirror of recipe_noise.rs (the f64 parity ORACLE). f32 GLSL base profile
// (no 64-bit ints): the i64 wrapping lattice hash is emulated with two u32 words. Every
// function here is parity-gated against the committed fixture (primitive_parity_check.gd).
// EDIT-BOTH-SIDES: changes here must keep parity with recipe_noise.rs.

// --- i64 emulation: 64-bit value as (hi:u32, lo:u32) ---
// recipe_noise.rs hash2: h = ix*374761393 + iz*668265263 + seed*362437 (wrapping i64),
// then h = (h ^ (h>>13)) * 1274126177 (wrapping i64), h &= 0x7fffffff, h / 0x7fffffff.

uvec2 u64_add(uvec2 a, uvec2 b) {
    uint lo = a.y + b.y;
    uint carry = (lo < a.y) ? 1u : 0u;
    uint hi = a.x + b.x + carry;
    return uvec2(hi, lo);
}
// 64x64 -> low 64 bits multiply (wrapping), operands as (hi,lo).
uvec2 u64_mul(uvec2 a, uvec2 b) {
    // Schoolbook on 16-bit limbs would be exact but verbose; low-64 wrapping only needs:
    // lo = a.lo*b.lo (full 64), plus (a.lo*b.hi + a.hi*b.lo) shifted into hi.
    uint all = a.y;
    uint bll = b.y;
    // split into 16-bit halves for an exact 32x32->64 of the low words
    uint a0 = all & 0xffffu, a1 = all >> 16;
    uint b0 = bll & 0xffffu, b1 = bll >> 16;
    uint p00 = a0 * b0;
    uint p01 = a0 * b1;
    uint p10 = a1 * b0;
    uint p11 = a1 * b1;
    uint mid = p01 + p10;                 // may overflow 32 bits -> handle carry
    uint midc = (mid < p01) ? 0x10000u : 0u;
    uint lo = p00 + (mid << 16);
    uint loc = (lo < p00) ? 1u : 0u;
    uint hi = p11 + (mid >> 16) + midc + loc;
    // plus cross terms into hi: a.lo*b.hi + a.hi*b.lo (low 32 only)
    hi += a.y * b.x + a.x * b.y;
    return uvec2(hi, lo);
}
// arithmetic right shift by 13 of a signed 64-bit (hi,lo); sign = top bit of hi.
uvec2 i64_ashr13(uvec2 v) {
    uint sign = (v.x & 0x80000000u) != 0u ? 0xffffffffu : 0u;
    uint lo = (v.y >> 13) | (v.x << 19);
    uint hi = (v.x >> 13) | (sign << 19);  // fill from sign
    // correct sign fill for the high bits shifted in:
    hi = (uint(int(v.x) >> 13));           // GLSL int >> is arithmetic
    return uvec2(hi, lo);
}
uvec2 u64_xor(uvec2 a, uvec2 b) { return uvec2(a.x ^ b.x, a.y ^ b.y); }

uvec2 i64_from_int(int x) {
    uint sign = (x < 0) ? 0xffffffffu : 0u;
    return uvec2(sign, uint(x));
}

float hash2(int ix, int iz, int seed) {
    uvec2 h = u64_add(u64_mul(i64_from_int(ix), i64_from_int(374761393)),
                      u64_mul(i64_from_int(iz), i64_from_int(668265263)));
    h = u64_add(h, u64_mul(i64_from_int(seed), i64_from_int(362437)));
    h = u64_mul(u64_xor(h, i64_ashr13(h)), i64_from_int(1274126177));
    uint masked = h.y & 0x7fffffffu;       // 0x7fffffff fits in low 32 bits
    return float(masked) / float(0x7fffffffu);
}

float fade(float t) { return t*t*t*(t*(t*6.0-15.0)+10.0); }

float value_noise(float wx, float wz, int seed) {
    float fx = floor(wx); float fz = floor(wz);
    int x0 = int(fx); int z0 = int(fz);
    float tx = fade(wx - fx); float tz = fade(wz - fz);
    float c00 = hash2(x0, z0, seed);
    float c10 = hash2(x0+1, z0, seed);
    float c01 = hash2(x0, z0+1, seed);
    float c11 = hash2(x0+1, z0+1, seed);
    float top = c00 + (c10 - c00) * tx;
    float bot = c01 + (c11 - c01) * tx;
    return (top + (bot - top) * tz) * 2.0 - 1.0;
}

float fbm(float wx, float wz, float base_freq, int octaves, int seed, float gain, float lacunarity) {
    float h = 0.0; float amp = 1.0; float norm = 0.0; float freq = base_freq;
    for (int i = 0; i < octaves; ++i) {
        h += amp * value_noise(wx*freq, wz*freq, seed + i);
        norm += amp; amp *= gain; freq *= lacunarity;
    }
    return h / max(norm, 1e-9);
}

float ridged_multifractal(float wx, float wz, float base_freq, int octaves, int seed,
                          float gain, float lacunarity, float offset, float weight_gain) {
    float h = 0.0; float weight = 1.0; float amp = 1.0; float norm = 0.0; float freq = base_freq;
    for (int i = 0; i < octaves; ++i) {
        float signal = offset - abs(value_noise(wx*freq, wz*freq, seed + i));
        signal = max(signal, 0.0);
        signal = signal * signal * weight;
        h += amp * signal;
        norm += amp;
        weight = clamp(signal * weight_gain, 0.0, 1.0);
        amp *= gain; freq *= lacunarity;
    }
    return clamp(h / max(norm, 1e-9), 0.0, 1.0);
}

// recursive_domain_warp (steps=3 path), mirrors recipe_noise.rs recursive_domain_warp.
vec2 recursive_domain_warp(float wx, float wz, float warp_amount, float warp_freq, int seed,
                           int steps, float decay, float freq_mul) {
    if (warp_amount == 0.0 || steps == 0) return vec2(wx, wz);
    float ox = wx; float oz = wz; float amount = warp_amount; float freq = warp_freq;
    for (int i = 0; i < steps; ++i) {
        float dx = fbm(ox, oz, freq, 3, seed + 101 + i*37, 0.5, 2.0);
        float dz = fbm(ox, oz, freq, 3, seed + 151 + i*37, 0.5, 2.0);
        ox += amount * dx; oz += amount * dz;
        amount *= decay; freq *= freq_mul;
    }
    return vec2(ox, oz);
}
```

Note on `i64_ashr13`: GLSL `int >> n` IS arithmetic (sign-preserving). The simplest correct form is to operate on the full i64 via the two-word split; the line `hi = uint(int(v.x) >> 13)` recovers the arithmetic high word, and `lo = (v.y >> 13) | (v.x << 19)` brings the low 13 bits of `hi` into `lo`. The earlier `sign`/`hi` lines are superseded — KEEP ONLY the final two assignments (`lo`, then `hi = uint(int(v.x) >> 13)`); delete the dead `sign` line during implementation. The parity gate (Step 3) is the proof this is right.

- [ ] **Step 2: Write the primitive parity check (failing)**

The fixture-comparison decision applies here: export a small primitives fixture (Task 4a.4 covers fixture verification) OR inline a few known `recipe_noise.rs` outputs. Inline is simplest for the leaf primitives — compute reference values once from the Rust oracle and assert GLSL matches. Create `wg-10/worldgen_terrain/tests/primitive_parity_check.gd`:

```gdscript
extends SceneTree

# Slice-4a primitive parity: GLSL recipe_primitives.glsl vs recipe_noise.rs f64 oracle.
# Drives a tiny compute shader that evaluates hash2/value_noise/fbm/ridged_multifractal at
# fixed coords, reads back, compares to oracle values exported from Rust. WINDOWED only.
# Tier-2 epsilon (f32 vs f64): primitives are in [-1,1]/[0,1], so ABS_EPS small.

const ORACLE := "res://worldgen_terrain/fixtures/primitive_parity_fixture.json"
const ABS_EPS := 2.0e-4   # f32 noise vs f64 oracle (widen w/ recorded justification if hash needs it)

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10PrimitiveProbe"):
		push_error("[wg10-prim-parity] Wg10PrimitiveProbe not registered"); return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-prim-parity] status=skip reason=no-gpu"); return 2
	probe.free()
	var f := FileAccess.open(ORACLE, FileAccess.READ)
	if f == null:
		push_error("[wg10-prim-parity] missing oracle %s" % ORACLE); return 1
	var data: Dictionary = JSON.parse_string(f.get_as_text())
	# data = { "samples": [ {fn, args:[...], expected:float}, ... ] }
	var pr: Object = ClassDB.instantiate("Wg10PrimitiveProbe")
	pr.call("load_shader", ProjectSettings.globalize_path("res://worldgen_terrain/shaders/primitive_probe.glsl"))
	var max_d := 0.0; var fails := 0
	for s in data["samples"]:
		var got: float = pr.call("eval", str(s["fn"]), PackedFloat64Array(s["args"]))
		var d: float = absf(got - float(s["expected"]))
		max_d = maxf(max_d, d)
		if d > ABS_EPS:
			fails += 1
			if fails <= 5:
				push_error("[wg10-prim-parity] %s%s: got=%f exp=%f d=%g" % [s["fn"], str(s["args"]), got, s["expected"], d])
	if fails > 0:
		print("[wg10-prim-parity] status=fail samples=%d fails=%d maxd=%g" % [data["samples"].size(), fails, max_d]); return 1
	print("[wg10-prim-parity] status=pass samples=%d maxd=%g" % [data["samples"].size(), max_d]); return 0
```

This needs a tiny `Wg10PrimitiveProbe` `#[func]` class + a `primitive_probe.glsl` that includes `recipe_primitives.glsl` and dispatches one function per call writing one float. Add a sub-step:

- [ ] **Step 2b: Add the `Wg10PrimitiveProbe` Rust class + `primitive_probe.glsl`**

Create `wg-10/worldgen_terrain/shaders/primitive_probe.glsl` — a 1-invocation shader that `#include`-inlines the primitives (Godot's GLSL has no #include; PASTE the primitives source in, or have the Rust side concatenate `recipe_primitives.glsl + probe_main`). The Rust `Wg10PrimitiveProbe::eval(fn_name, args) -> f64` builds the right push constant (an int selecting the fn + the float args), dispatches 1×1×1, reads back one float. Model the dispatch on `flow_spike.rs` (local RD, single dispatch, `buffer_get_data`). Register the class in lib.rs.

The oracle JSON is generated by a tiny exporter:

- [ ] **Step 2c: Export the oracle from the Rust f64 primitives**

Create `tools/dem_pack/export_primitive_parity_fixture.py` (if the primitives are also in Python `worldgen_proto.py`, generate from there — they are the SAME math the Rust oracle mirrors per `recipe_noise.rs:1-5`). Emit `wg-10/worldgen_terrain/fixtures/primitive_parity_fixture.json` with ~30 samples spanning `hash2`, `value_noise`, `fbm`, `ridged_multifractal`, `recursive_domain_warp` (the warp returns 2 floats → two samples `warp_x`/`warp_z`) at varied coords/seeds INCLUDING negative coords (the arithmetic-shift sign path) and large coords (i64 wrapping path). Run it.

Run: `python tools/dem_pack/export_primitive_parity_fixture.py`
Expected: writes the JSON, prints sample count.

- [ ] **Step 3: Run the primitive parity gate WINDOWED**

Run (owner / separate windowed instance): `env -u CARGO_TARGET_DIR GODOT_BIN=<godot-4.6.2> python tools/gate.py --suite biome_page`
Expected: `[wg10-prim-parity] status=pass samples=~30 maxd=<small>`. If the i64-emulated hash is over `ABS_EPS`, the hash emulation has a bug — fix `u64_mul`/`i64_ashr13` until the negative-coord and large-coord samples pass (do NOT widen the epsilon to hide a hash bug; widen only for genuine f32-rounding spread with a recorded note).

- [ ] **Step 4: Commit**

```bash
git add wg-10/worldgen_terrain/shaders/recipe_primitives.glsl wg-10/worldgen_terrain/shaders/primitive_probe.glsl wg-10/rust/src/primitive_probe.rs wg-10/rust/src/lib.rs wg-10/worldgen_terrain/tests/primitive_parity_check.gd tools/dem_pack/export_primitive_parity_fixture.py wg-10/worldgen_terrain/fixtures/primitive_parity_fixture.json tools/gate.py
git commit -m "slice4a: GLSL noise/warp primitives + f32-vs-f64 parity gate

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 4a.4: GLSL gaussian-nearest + verify the mountain fixture stores apron params

**Files:**
- Modify: `wg-10/worldgen_terrain/shaders/recipe_primitives.glsl` (add `gaussian_filter_nearest` equivalent — see note)
- Read/verify: `tools/dem_pack/fixtures/recipe_mountain_fixture.json`
- Possibly modify: `tools/dem_pack/export_recipe_mountain_fixture.py` (if apron params/core height not stored)

The gaussian is a SEPARABLE whole-field operator (`array_ops.rs:41-67`: blur axis 0 then axis 1, scipy `mode='nearest'`, `truncate=4.0`, radius `int(truncate*sigma+0.5)`). On the GPU this is a multi-pass operation over the apron grid, NOT a per-point function. The mountain recipe uses gaussians at sigma 1.15/1.2/1.8/2.0/5.0/7.0 (see `recipes.rs`). DECISION for 4a: implement the gaussian as GPU passes inside the page pipeline (Task 4a.5), with the kernel built CPU-side and uploaded (the kernel only depends on sigma, not data). This task just verifies the fixture has what the parity gate needs.

- [ ] **Step 1: Verify the mountain fixture contents (DONE 2026-06-02 — schema confirmed below)**

VERIFIED SCHEMA (no exporter change needed). `recipe_mountain_fixture.json` top-level: `{generator_version:"recipe_fixtures/v1", source, note, records:[...]}`. Each entry in `records` (there are 2 cases, seed 0 + another) has EXACTLY these keys:
- `recipe` = `"mountain_seamsafe"`, `style_key` = `"alpine_branching"` (STYLES[0])
- `seed` (int), `feature_span_m` (float, e.g. 90000.0), `apron_px` (int, 160)
- `core_rows`, `core_cols` (24 each — the small parity core), `padded_rows`, `padded_cols` (344 = 24 + 2*160)
- `grid` = `{spacing, ox, oz}` (floats) — the meshgrid is rebuilt analytically: `xs[c]=(c-apron_px)*spacing+ox`, `zs[r]=(r-apron_px)*spacing+oz`, then numpy meshgrid (note field documents this)
- `height` = flat row-major f64 list of length `core_rows*core_cols` (576) — the CORE-cropped oracle output (NORMALIZED recipe units, pre-relief-multiply)

IMPORTANT dimension note for 4a.5: the FIXTURE uses a tiny 24-core / 344-padded grid (fast exact parity). The COST measurement spike (4a.1/4a.2) uses the real 256-core / 576-padded production page. The 4a.5 GPU parity gate must rebuild the grid at the FIXTURE's dims (read `padded_rows`/`padded_cols`/`apron_px`/`grid` per record), NOT the production 576.

Run: `python -c "import json; d=json.load(open('tools/dem_pack/fixtures/recipe_mountain_fixture.json')); r=d['records'][0]; print(sorted(r.keys()))"`
Expected: `['apron_px','core_cols','core_rows','feature_span_m','grid','height','padded_cols','padded_rows','recipe','seed','style_key']`. (Confirmed present — no Step 2 needed.)

- [ ] **Step 2: (NOT NEEDED — fixture already complete)** The verified schema above stores every apron-meshgrid param + padded dims + core height the GPU parity gate needs. No exporter change; the Rust `recipes_tests.rs` parity stays untouched.

- [ ] **Step 3: Document the gaussian-pass approach in the shader header (DONE 2026-06-02)**

Done in `recipe_primitives.glsl` header: gaussian-nearest is realized as separable GPU passes (axis0 then axis1, clamp-to-edge, CPU-built kernel uploaded per sigma) inside the page pipeline — NOT a per-point function — and the CPU kernel build must match `array_ops.rs::gaussian_kernel1d` (radius `int(truncate*sigma+0.5)`, `truncate=4.0`, normalized). Mountain sigmas documented: {1.15, 1.20, 1.80, 2.00, 5.00, 7.00, valley_width_px, floor_smooth_px}.

- [ ] **Step 4: Commit**

```bash
git add tools/dem_pack/fixtures/recipe_mountain_fixture.json tools/dem_pack/export_recipe_mountain_fixture.py wg-10/worldgen_terrain/shaders/recipe_primitives.glsl
git commit -m "slice4a: verify/extend mountain fixture apron params + document gaussian-pass plan

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 4a.5: Port the mountain recipe to the GLSL apron page pipeline

**Files:**
- Create: `wg-10/worldgen_terrain/shaders/biome_page_4a.glsl` (the recipe pipeline — multi-pass over the apron grid)
- Create: `wg-10/rust/src/biome_page_compute.rs` (`Wg10BiomePageCompute` orchestrating the apron buffers + passes; behind a flag, global RD)
- Modify: `wg-10/rust/src/lib.rs`

This is the heart of 4a. The mountain recipe (`recipes.rs::mountain::generate_seamsafe`) is a SEQUENCE of pointwise passes + whole-field gaussians + two flow-channel passes + crop. On the GPU it becomes a chain of dispatches over the apron grid, intermediate fields in storage buffers (or images), with the flow relaxation reused from 4a's measurement. The exact pass sequence (from `recipes.rs:287-451`):

1. pointwise: `recursive_domain_warp` → `regional`, `ranges` (via `oriented_ridges_point`), `ridge_detail`, `near_detail` (one dispatch, writes 4 fields)
2. gaussian(ranges, σ5) → `range_envelope` via smoothstep
3. `lowland_mask`: gaussian(range_field, σ7) then pointwise combine
4. gaussian(ranges, σ1.8); pointwise `massif`; gaussian(massif, σ2.0)
5. pointwise `base`
6. `flow_channels_seam_safe(base, σ1.15 pre-blur → flow relax → log1p-norm → σ=valley_width spread)` → `primary`; smoothstep → `primary_mask`
7. pointwise `rough_surface`; `flow_channels_seam_safe(...)` → `tributary`; smoothstep → `tributary_mask`
8. pointwise `high_mask`/`valley_mask`
9. pointwise assemble `height`
10. gaussian(valley_mask, σ1.2) → floor_mask; gaussian(height, σ=floor_smooth) → floor; pointwise blend
11. gaussian(height, σ1.2); pointwise final affine_remap
12. crop core (the output dispatch writes only the core region to the R32F image)

- [ ] **Step 1: Write the apron page pipeline shader (mountain)**

Create `wg-10/worldgen_terrain/shaders/biome_page_4a.glsl`. Because Godot GLSL has no `#include`, the Rust side concatenates `recipe_primitives.glsl` + this file before compile (document that in both headers). Structure it as MULTIPLE entry behaviors selected by a `pass` push-constant int (one compiled shader, dispatched once per pass with a different `pass` value), reading/writing the intermediate storage buffers bound per pass. Each pass writes its output field; the gaussian passes are the separable axis0/axis1 form using the uploaded kernel. The final pass crops: it maps the core output texel → apron-grid index `(r+apron, c+apron)` and `imageStore`s into the R32F page image, preserving the **texel-CORNER convention** (height_page.glsl:182-196 — texel 0 → core origin, N-1 → origin+span; the core meshgrid the fixture uses must match this).

Implement the pointwise math EXACTLY from `recipes.rs::mountain` (all the affine-remap constants `REGIONAL_CENTER=-0.50` etc. become GLSL consts; the LOOK gains; the assembly weights `0.08+0.58*hm` etc.). The flow relaxation pass reuses the `flow_accum_spike.glsl` body (pull relaxation) at the apron dim and the stable iter count.

This is a large shader — implement it pass-by-pass, validating each against the fixture incrementally (Step 3 runs the full-pipeline parity; for debugging, the probe class can dump an intermediate field). Keep the GLSL structured so each `pass` branch reads like the corresponding `recipes.rs` block.

- [ ] **Step 2: Write `Wg10BiomePageCompute` (orchestrator, behind a flag)**

Create `wg-10/rust/src/biome_page_compute.rs`. Model the resource lifecycle on `page_compute.rs` (cached context built once, per-page dispatch reuses it). It:
- builds the apron storage buffers (intermediate fields) once at configure,
- builds the CPU-side gaussian kernels (one per distinct sigma: 1.15, 1.2, 1.8, 2.0, 5.0, 7.0, valley_width, floor_smooth) via a Rust port of `gaussian_kernel1d` and uploads them,
- per page: sets the apron origin/span push constant, dispatches the pass chain (pointwise → gaussians → flow → assemble → crop) with barriers between dependent passes, writes the core into the target R32F image (caller-owned, like `compute_page_cached`),
- is behind a flag (`use_biome_path: bool`) — NOT the runtime default yet.

Expose a `#[func] generate_core_page(...) -> PackedFloat64Array` test entry that runs the pipeline for ONE page at the fixture's params and reads back the core (readback ONLY in this test entry, never the render path — spec §4). Register in lib.rs.

- [ ] **Step 3: Write the two-tier parity gate (GPU page vs fixture)**

Create `wg-10/worldgen_terrain/tests/biome_page_parity_check.gd`. Copy `gpu_parity_check.gd` two-tier shape but compare against the FIXTURE (the owner-approved oracle). NOTE the REAL fixture schema verified in 4a.4: top-level `{records:[...]}`, each record has `grid={spacing,ox,oz}`, `padded_rows`, `padded_cols`, `core_rows`, `core_cols`, `apron_px`, `seed`, `feature_span_m`, `style_key`, and a flat `height` list of `core_rows*core_cols` f64 in NORMALIZED recipe units (pre-relief, values ~[-0.5,0.5]). The gate loops over ALL records:

```gdscript
extends SceneTree

# Slice-4a two-tier parity: GPU mountain page vs the committed f64 fixture (the oracle the
# CPU port is proven against). Tier-1 (structural) = the GPU rebuilds the grid from the
# record's exact apron/grid params (a wrong dim/seed -> size mismatch or gross delta).
# Tier-2 = core height within a NORMALIZED-unit epsilon (the fixture is pre-relief units).
# WINDOWED only (local RD null headless -> skip rc 2). The flow contribution is the
# approximated part (spec 4 Tier-2); widen NORM_EPS only with a recorded justification.

const FIXTURE := "res://worldgen_terrain/fixtures/recipe_mountain_fixture.json"
# Normalized recipe units (NOT metres): height ~[-0.5,0.5]. The M2 metres budget 1e-2 over
# ~1000m relief maps to ~1e-5 normalized, but the flow-relaxation APPROXIMATION (spec 4) is
# coarser than the exact CPU sweep, so start at 1e-2 normalized and tighten/justify after the
# first real run measures the actual flow-driven delta. Record the achieved maxd in the spec.
const NORM_EPS := 1.0e-2

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10BiomePageCompute"):
		push_error("[wg10-biome-parity] Wg10BiomePageCompute not registered"); return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-biome-parity] status=skip reason=no-gpu"); return 2
	probe.free()
	var f := FileAccess.open(FIXTURE, FileAccess.READ)
	if f == null: push_error("[wg10-biome-parity] missing fixture"); return 1
	var fx: Dictionary = JSON.parse_string(f.get_as_text())
	var records: Array = fx.get("records", [])
	if records.is_empty(): push_error("[wg10-biome-parity] no records in fixture"); return 1

	var gpu: Object = ClassDB.instantiate("Wg10BiomePageCompute")
	gpu.call("load_shaders",
		ProjectSettings.globalize_path("res://worldgen_terrain/shaders/recipe_primitives.glsl"),
		ProjectSettings.globalize_path("res://worldgen_terrain/shaders/biome_page_4a.glsl"))

	var overall_max := 0.0
	var rec_i := 0
	for rec in records:
		var grid: Dictionary = rec["grid"]
		var prows := int(rec["padded_rows"]); var pcols := int(rec["padded_cols"])
		var apron := int(rec["apron_px"])
		var core_rows := int(rec["core_rows"]); var core_cols := int(rec["core_cols"])
		var expected: Array = rec["height"]
		# generate_core_page rebuilds the apron meshgrid from these PADDED dims (Tier-1 echo).
		var got: PackedFloat64Array = gpu.call("generate_core_page",
			float(grid["spacing"]), float(grid["ox"]), float(grid["oz"]),
			prows, pcols, apron, int(rec["seed"]), float(rec["feature_span_m"]))
		if got.size() != core_rows * core_cols:
			push_error("[wg10-biome-parity] rec=%d size got=%d exp=%d" % [rec_i, got.size(), core_rows*core_cols]); return 1
		var max_d := 0.0; var fails := 0
		for i in range(got.size()):
			var d: float = absf(got[i] - float(expected[i]))
			max_d = maxf(max_d, d)
			if d > NORM_EPS:
				fails += 1
				if fails <= 5: push_error("[wg10-biome-parity] rec=%d core[%d] gpu=%f exp=%f d=%g" % [rec_i, i, got[i], expected[i], d])
		if max_d != max_d:
			push_error("[wg10-biome-parity] rec=%d NaN delta (degenerate page)" % rec_i); return 1
		if fails > 0:
			print("[wg10-biome-parity] status=fail rec=%d core=%d fails=%d maxd=%g" % [rec_i, got.size(), fails, max_d]); return 1
		overall_max = maxf(overall_max, max_d)
		rec_i += 1
	print("[wg10-biome-parity] status=pass biome=mountain records=%d overall_maxd=%g eps=%g" % [records.size(), overall_max, NORM_EPS]); return 0
```

NOTE: `generate_core_page` signature is `(spacing, ox, oz, padded_rows, padded_cols, apron_px, seed, feature_span_m) -> PackedFloat64Array` and the Rust side uses `style = ALPINE_BRANCHING` (matches every record's `style_key="alpine_branching"`). The `load_shaders(primitives_path, page_path)` two-arg shape mirrors `Wg10PrimitiveProbe::load_shader` (the page shader concatenates `recipe_primitives.glsl` + `biome_page_4a.glsl` before compile, same as the probe). Units are NORMALIZED (pre-relief) — `NORM_EPS` is in those units; the spec §4 metres-relative tolerance is `NORM_EPS * relief_m`. After the first real run, record the achieved `overall_maxd` in the spec and tighten `NORM_EPS` toward it (the flow-approximation delta is the floor).

Add `biome_page_parity_check.gd` to the `biome_page` suite in `tools/gate.py`.

- [ ] **Step 4: Run the parity gate WINDOWED + iterate to green**

Run (owner / windowed): `env -u CARGO_TARGET_DIR GODOT_BIN=<godot-4.6.2> python tools/gate.py --suite biome_page`
Expected: `[wg10-biome-parity] status=pass biome=mountain ... maxd=<= eps>`.

The flow relaxation is an APPROXIMATION of the CPU sorted sweep (spec §4 Tier-2). If `max_d` exceeds the epsilon ONLY in the channel regions, that is the flow approximation, not a bug — raise the stable iter count or widen the epsilon with a recorded note quantifying the flow contribution. If `max_d` is large everywhere, it's a pointwise/gaussian bug — debug pass-by-pass (dump intermediates).

- [ ] **Step 5: Commit**

```bash
git add wg-10/worldgen_terrain/shaders/biome_page_4a.glsl wg-10/rust/src/biome_page_compute.rs wg-10/rust/src/lib.rs wg-10/worldgen_terrain/tests/biome_page_parity_check.gd wg-10/worldgen_terrain/fixtures/recipe_mountain_fixture.json tools/gate.py
git commit -m "slice4a: GLSL mountain recipe apron page pipeline + two-tier parity (GPU vs f64 fixture)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 4a.6: Slice-4a closeout — docs + full gate sweep

**Files:**
- Modify: `docs/plans/STATUS.md`, `docs/plans/HANDOFF.md`, `docs/plans/LOOSE_ENDS_LEDGER.md`

- [ ] **Step 1: Run the full gate sweep (verify no regression)**

Run: `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test -p wg10_terrain 2>&1 | tail -5` (cargo suite)
Then the windowed suites (owner): `python tools/gate.py --suite fast` / `--suite gpu` / `--suite m3` / `--suite biome_page` / `--suite page_measure`.
Expected: cargo all green (count grew by the new tests); windowed suites pass; `biome_page` + `page_measure` pass.

- [ ] **Step 2: Update STATUS/HANDOFF/LEDGER**

Record: 4a done (mountain end-to-end on GPU behind a flag, two-tier parity green, per-page cost measured = `<number>`, pipeline = `<decision>`). The legacy path is STILL the runtime default. Next = 4b (10 recipes + compose + grammar weights).

- [ ] **Step 3: Commit + push**

```bash
git add docs/plans/STATUS.md docs/plans/HANDOFF.md docs/plans/LOOSE_ENDS_LEDGER.md
git commit -m "slice4a: closeout — mountain GPU page proven behind flag, docs synced

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
GIT_TERMINAL_PROMPT=0 git push origin main
```

---

## SLICE 4b — generalize to 10 biomes + compose + grammar weights

> 4b mirrors the CPU recipe-port fan-out. Each biome reuses 4a's primitives + the apron pass pipeline; only the per-biome math + apron_px + constants differ. The 11 CPU recipes are `recipes_{volcanic,glacial,karst,grassland,desert,temperate,tundra,rainforest,coast,wetland}.rs` + mountain. Each is parity-gated against its own committed fixture, same shape as 4a.5 Step 3.

### Task 4b.1 … 4b.10: Port each remaining recipe to GLSL + parity-gate

For EACH biome in {volcanic, glacial, karst, grassland, desert, temperate, tundra, rainforest, coast, wetland} (one task each):

**Files (per biome `<b>`):**
- Modify: `wg-10/worldgen_terrain/shaders/biome_page.glsl` (add the `<b>` recipe pass branch; this file is the generalized successor to `biome_page_4a.glsl` — Task 4b.0 renames/promotes it)
- Modify: `wg-10/rust/src/biome_page_compute.rs` (dispatch the `<b>` recipe by id)
- Modify: `wg-10/worldgen_terrain/tests/biome_page_parity_check.gd` (loop over all biomes' fixtures, not just mountain)
- Read: `wg-10/rust/src/recipes_<b>.rs` (the exact math to mirror), `tools/dem_pack/fixtures/recipe_<b>_fixture.json`

**Steps (each biome):**
- [ ] Read `recipes_<b>.rs` — note its `APRON_PX`, affine-remap constants, style fields, and the assembly sequence (which passes it needs; some biomes skip flow channels).
- [ ] Add the `<b>` pass branch(es) to `biome_page.glsl`, mirroring the Rust math exactly (constants → GLSL consts).
- [ ] Add `<b>` to the parity check's biome loop (load `recipe_<b>_fixture.json`, generate, compare two-tier).
- [ ] Run `--suite biome_page` WINDOWED; iterate to green for `<b>` at `ABS_EPS` (flow-region tolerance note if needed).
- [ ] Commit: `slice4b: GLSL <b> recipe + parity (GPU vs f64 fixture)`.

> **Task 4b.0 (do first):** promote `biome_page_4a.glsl` → `biome_page.glsl`; make the recipe selection a push-constant `biome_id` int so one shader dispatches any recipe. Keep mountain parity green after the rename (re-run `--suite biome_page`). Commit separately.

### Task 4b.11: GLSL `compose_biomes` + grammar biome-weight field + compose parity

**Files:**
- Modify: `wg-10/worldgen_terrain/shaders/biome_page.glsl` (add compose passes: per-recipe height + per-recipe weight → `blend_height_favored` for N==2, `blend_field` fold for N>2 — EXACTLY `biome_compose.rs` fold semantics)
- Modify: `wg-10/rust/src/biome_page_compute.rs` (run the grammar weight field + compose the active recipes per page)
- Create: `wg-10/worldgen_terrain/tests/biome_compose_parity_check.gd` (GPU composed page vs `biome_compose_fixture.json`)
- Read: `wg-10/rust/src/biome_compose.rs`, `tools/dem_pack/fixtures/biome_compose_fixture.json`

**Steps:**
- [ ] Read `biome_compose.rs` — the fold: `use_favored = (mode=="height_favored") && (fields.len()==2)`; running accumulator `w_acc = acc_w/(acc_w+w+1e-12)`; `blend_height_favored` term order (relief_a/relief_b via gaussian σ=relief_sigma_px, favor, signal, band, w_adj clip). The relief proxy gaussian is the SAME nearest-mode gaussian (reuse the pass).
- [ ] Add the grammar biome-weight field: the per-pixel partition-of-unity weights come from the grammar (the same region/palette/family selection the legacy `height_page.glsl:113-119` computes — Tier-1 EXACT, integer hash identical both sides). Port that selection to pick which recipes are active + their weights at each pixel.
- [ ] Add compose passes to `biome_page.glsl` (gaussian-relief proxy + the favored/field blend fold).
- [ ] Write `biome_compose_parity_check.gd`: GPU composed page (2 biomes + a 3-biome triple-point case) vs `biome_compose_fixture.json`, two-tier. Add to `biome_page` suite.
- [ ] Run `--suite biome_page` WINDOWED; iterate to green. Tier-1 (which recipes active + weights) must match EXACTLY; Tier-2 composed height within epsilon.
- [ ] Commit: `slice4b: GLSL compose_biomes + grammar weights + compose parity`.

### Task 4b.12: Slice-4b closeout

- [ ] Full gate sweep (cargo + windowed suites). All green.
- [ ] Update STATUS/HANDOFF/LEDGER: 4b done (all 11 recipes + compose + grammar weights on GPU behind the flag, all parity-green). Legacy still default. Next = 4c flip.
- [ ] Commit + push.

---

## SLICE 4c — flip the runtime + remove the atlas

### Task 4c.1: Flip the page-pool producer to the biome path behind the flag

**Files:**
- Modify: `wg-10/rust/src/page_pool.rs` (configure the biome context as the producer when the flag is on)
- Modify: `wg-10/rust/src/page_compute.rs` (mark legacy kernel context; keep buildable but flag-gated)

- [ ] Read `page_pool.rs` (the `reset_configured_state` / configure path from commit ce61449) + how it builds `PageComputeContext` today.
- [ ] Add the producer selection: when `use_biome_path` is set, configure builds the `Wg10BiomePageCompute` context (apron buffers + kernels) instead of the kernel `PageComputeContext`, and per-page dispatch calls the biome pass chain. Default the flag ON for the runtime (this is the flip), but keep the legacy context constructible behind the flag OFF for the audit/rollback.
- [ ] Verify the M3 contracts survive: texel-CORNER seam convention, custom AABB, coarsest-hold-last-good (spec §6). The biome page must write the core with the identical pixel→world mapping as `height_page.glsl:182-196`.
- [ ] Run cargo isolated + the M3 windowed suite (`--suite m3`) — the moving-camera gate is what catches clipmap/seam correctness (memory `worldgen10-clipmap-rings`). Expected: m3 green with the biome path live.
- [ ] Commit: `slice4c: flip page-pool producer to GPU biome path (flag on)`.

### Task 4c.2: Remove the 25 MB kernel atlas + atlas-removal audit gate

**Files:**
- Modify: `wg-10/rust/src/page_compute.rs` / `gpu_compute.rs` (drop the kernel-atlas buffers from the biome path — krec/kparam/kdata bindings 6/7/8)
- Modify: `wg-10/rust/src/biome_page_compute.rs` (assert it never binds KData)
- Create: `wg-10/worldgen_terrain/tests/atlas_removal_audit_check.gd`

- [ ] Ensure the biome path creates NO atlas buffers (no `kdata` upload). The kernel atlas + `sample_kernel` are not referenced by any active render shader on the biome path.
- [ ] Write `atlas_removal_audit_check.gd`: a runtime+grep gate asserting (a) the active page shader source contains no `KData`/`sample_kernel` binding, (b) no atlas buffer is created on the biome path (probe the configured context's buffer set). Add to a `biome_page` (or new `atlas_audit`) suite.
- [ ] Run the audit WINDOWED. Expected: pass — no active shader samples KData, no atlas buffer on the new path.
- [ ] Commit: `slice4c: remove 25MB kernel atlas from the live path + audit gate`.

### Task 4c.3: Hardened perf gate (real GPU time, did-real-work)

**Files:**
- Create/Modify: `wg-10/worldgen_terrain/tests/biome_page_perf_check.gd` (model on `m5_perf_hardened_check.gd`)

- [ ] Read `m5_perf_hardened_check.gd` (the existing hardened perf pattern) + memory `worldgen10-real-gpu-time` (use `RenderingServer.viewport_get_measured_render_time_gpu`, NOT wall) + `worldgen10-profiling-must-be-real` (bake did-real-work assertions IN).
- [ ] Write the gate: fly the biome-path streaming scene at ~1000 m/s, assert real GPU-time **p99 < 6 ms** AND did-real-work (streamed pages > 0, non-black, biome recipe work contributed, NO atlas path used). The "no atlas path used" assertion is what proves the flip is real.
- [ ] Run WINDOWED. If p99 ≥ 6 ms: this is the spec §6 #1 risk materializing — escalate (the measurement in 4a.2 should have predicted it; if 4a said per-page-live fits but the real streamed p99 doesn't, the gap is the recipe work the spike approximated → either optimize the pass chain or invoke the coarse-fact fallback design).
- [ ] Commit: `slice4c: hardened GPU-time perf gate for the biome page path (p99<6ms, did-real-work)`.

### Task 4c.4: visible==collision no-regression + owner fly review

**Files:**
- Verify: `wg-10/worldgen_terrain/tests/facts_collision_parity_check.gd` still green

- [ ] Run `facts_collision_parity_check.gd` WINDOWED — the facts/collision path is unchanged legacy (spec §2/§7), so this gate must still pass: collision agrees with the rendered base within the accepted epsilon. (If it regressed, the render base diverged from facts — investigate; the facts path was NOT supposed to change.)
- [ ] **Owner fly review** (acceptance authority, spec §7.3 / §5): launch the biome-path streaming scene for the owner to fly the live biome-composed terrain. Do NOT self-approve the look. Present A/B (legacy flag-off vs biome flag-on) if useful. The owner judges look quality.
- [ ] On owner approval: update STATUS/HANDOFF/LEDGER — Slice 4 COMPLETE (biome composition live on GPU, atlas removed, perf green, owner-accepted). Mark the legacy `height_page.glsl`/`sample_kernel`/pack-assembler as dead (removable in a later cleanup slice). Commit + push.

```bash
git add docs/plans/STATUS.md docs/plans/HANDOFF.md docs/plans/LOOSE_ENDS_LEDGER.md
git commit -m "slice4: COMPLETE — biome composition live on GPU, atlas removed, owner-accepted

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
GIT_TERMINAL_PROMPT=0 git push origin main
```

---

## Notes for the implementer

- **Gate commands** (memory `worldgen10-gate-run-recipe`): all `<godot-4.6.2>` placeholders mean the Godot 4.6.2 binary (4.5 is too old for the extension). The verified invocation is `env -u CARGO_TARGET_DIR GODOT_BIN=<path-to-godot-4.6.2> python tools/gate.py --suite <name>`; the gpu/m3/biome_page/page_measure suites auto-window. A full fresh sweep example is in that memory.
- **Never kill the owner's Godot editor to rebuild** (memory `worldgen10-dont-kill-editor`). GDScript hot-reloads. For Rust rebuilds, ASK the owner to close the editor, or validate Rust edits without it via `env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo check/test` (memory `worldgen10-cargo-check-isolated`). Windowed gates (gpu/m3/biome_page/page_measure) need a real device and a rebuilt dll — they are owner-run or run as a separate windowed instance.
- **Commit only when the owner says**, stage by name, NEVER `git add -A` (owner convention). The commit steps above stage explicit paths; treat them as the staging list, but the owner triggers the actual commit cadence.
- **Verify artifacts, don't trust reports** (memory: a subagent once misreported writing a JSON that was stale) — after any fixture export, check mtime+size+the keys you expect.
- **No non-ASCII in `print()`** on Windows (a `→` crashed an exporter after a 7-min bake) — use `->`.
- **The measurement decides the pipeline** (Task 4a.2 Step 4 fork) — if it says coarse-fact, STOP and escalate; do not build the wrong pipeline.
