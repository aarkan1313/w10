# WG10 Un-Intercept Proving Ladder — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a new clean proving scene that flips WG10 terrain from baked→live-procedural one feature at a time, each rung gated by measured convergence toward the accepted baked REFERENCE, real-GPU-time perf, and non-vacuous "did it actually run" guards.

**Architecture:** A thin assembly scene (`wg10_unintercept_ladder.tscn` + `.gd`) reusing the proven `Wg10TerrainView`/`Wg10ClipmapRings`/`Wg10PagePool`/`Wg10Streamer` stack. It exposes a probe API mirroring the existing progression gates (`set_probe_mode`, `update_for_probe`, `set_probe_camera_frame`, `debug_tile_states`) so gate idioms are reused verbatim. A single reusable convergence helper reads back a live page and the baked reference page over the same world region — via the ALREADY-PROVEN readback idiom (`acquire_page`/`get_resident_page` → `get_texture_rd_rid` → `rd.texture_get_data` → `to_float32_array`, with a `force_draw()` flush first because the live compute path is fire-and-forget on the global RD) — and reports `mean_abs/p95_abs/peak_abs` height delta in metres. Each rung adds one producer configuration plus a serial gate suite.

**VERIFIED API FACTS (read from source 2026-06-04 — these are real, do not re-guess):**
- Page-height readback ALREADY WORKS, no new Rust needed: copy `_read_page(rd, pool, level, ox, oz)` from `m3_continuity_check.gd:245-255` and the "configure live biome → `acquire_page` → 4× `await process_frame`+`RenderingServer.force_draw()` → `texture_get_data` → `to_float32_array`" flush idiom from `biome_runtime_isolate.gd:39-65`. Pages carry `CAN_COPY_FROM`.
- View wiring is THREE calls via `mountain_fly_runtime_config.gd`: `configure_streamer(streamer, pool)`, `configure_rings(rings)`, `configure_view(view, pool, streamer, rings, morph_enabled, relief_scale, relief_ref)`. There is NO `apply_to_view`, NO `set_pool`/`set_streamer`.
- Per-frame drive: `view.call("update", camera_x, camera_z, vel_x, vel_z)`.
- `debug_tile_states()` is on `Wg10ClipmapRings` (returns `PackedInt64Array`, 3 ints/tile: `[visible, origin_x, origin_z]`), NOT on the view.
- Camera: `Wg10FlyCamera` (`fly_camera.gd`) EXTENDS `Camera3D` — instantiate it AS the camera (`load(FLY_CAMERA).new()` then `add_child`), it exposes `get_velocity() -> Vector3`. Do not build a separate Camera3D + child rig.
- Source transform for Rung 1 is EXACTLY (verified by `mountain_fly_review_smoke_check.gd:361-363`): `set_biome_source_transform(3.515625, 207000.0, 176000.0)` — applied as DIRECT offsets, NOT center-minus-halfspan.
- `set_biome_source_transform` requires the pool to already be configured for SingleBiome or World (it errors otherwise) — call it AFTER `configure_biome`.
- `view.configure(...)` signature (9 args): `(pool, streamer, rings, num_levels, base_span, relief_scale, morph_region, relief_ref, lead_seconds)` — but use the `configure_view` GDScript wrapper, don't call it raw.
- The ONLY new Rust in the whole plan is Rung 4's live-collision seam. Everything else is GDScript + gate.py.

**Tech Stack:** Godot 4.6.2 mono (GDScript scenes + `*_check.gd` SceneTree gates), Rust GDExtension (`wg10_terrain` crate), GLSL compute (RenderingDevice, windowed), `tools/gate.py` serial suite runner, pytest for the offline contract metric.

**Hard constraints (from the spec + handoff — do not violate):**
- NEVER mutate the accepted baked payload (`mountain_world_layer_tiles.json`) or `mountain_fly_review.tscn`. The baked REFERENCE is loaded read-only as the convergence target.
- NEVER run two Godot suites in parallel (GDExtension DLL import/copy races). All gate runs are serial.
- Do NOT clean/reset/broad-checkout the worktree (237 preexisting modified files; leave them).
- Do NOT force-kill the owner's Godot editor to rebuild. For Rust rebuilds, ASK the owner to close the editor; validate Rust edits in isolation with `CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo check/test` (does not need the editor closed). Only the real `tools/build_rust.ps1` build + windowed gates need the editor closed.
- Threshold policy is **direction + no-regression**, not absolute targets. Rung 1 must match-or-beat the offline-measured gap; Rungs 2–3 must each reduce the delta vs the prior rung. The owner's eye is the final arbiter on "close enough."
- GPU/RenderingDevice work is WINDOWED only on this hardware (D3D12/RTX 5090). New gate suites go in `WINDOWED_SUITES`.

**Environment / commands (verified working in this repo):**
- Godot: `$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'`
- Isolated Rust check (editor stays open): `$env:CARGO_TARGET_DIR='D:/tmp/wg10_check_target'; cargo check -p wg10_terrain` and `cargo test -p wg10_terrain --lib`
- Real Godot-facing build (editor MUST be closed): `powershell -ExecutionPolicy Bypass -File tools\build_rust.ps1` from repo root.
- Run a gate suite: `$env:GODOT_BIN=...; python tools\gate.py --suite <name>` from repo root (`D:\workflows\worldgen10`).
- Commit message footer (required): `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`. Commit only when the owner asks; on this branch (`slice4-gpu-page-integration`) we commit per-task as the plan says, but DO NOT push unless asked.

---

## File Structure

**New files:**
- `wg-10/worldgen_terrain/harness/wg10_unintercept_ladder.tscn` — the proving scene (Node3D root).
- `wg-10/worldgen_terrain/harness/wg10_unintercept_ladder.gd` — scene driver: assembles components, owns rung selection + probe API + HUD. Kept thin (assembly only; no metric logic).
- `wg-10/worldgen_terrain/harness/ladder_producers.gd` — RefCounted; owns the per-rung producer configuration (which `configure_*` call + scale/seed/source-window/flow per rung). Analogous to `mountain_fly_producers.gd` but for the live-procedural rungs.
- `wg-10/worldgen_terrain/harness/ladder_convergence.gd` — RefCounted; the reusable convergence helper. Pure math (`delta(live, ref) -> {mean_abs, p95_abs, peak_abs, samples, live_relief, ref_relief, nonvacuous}`) PLUS a readback wrapper that reuses the proven `_read_page` idiom (acquire/get_resident → `texture_get_data` → floats) with a `force_draw()` flush. No new Rust.
- `wg-10/worldgen_terrain/tests/ladder_rung0_check.gd` — Rung 0 plumbing gate (analytic height parity + seam + never-black).
- `wg-10/worldgen_terrain/tests/ladder_rung1_check.gd` — Rung 1 live-mountain-macro convergence gate.
- `wg-10/worldgen_terrain/tests/ladder_rung2_check.gd` — Rung 2 drainage convergence + flow perf gate.
- `wg-10/worldgen_terrain/tests/ladder_rung3_check.gd` — Rung 3 material convergence gate.
- `wg-10/worldgen_terrain/tests/ladder_rung4_check.gd` — Rung 4 live collision parity gate.
- `wg-10/worldgen_terrain/tests/ladder_rung5_check.gd` — Rung 5 multi-biome compose gate.
- `wg-10/worldgen_terrain/tests/ladder_convergence_selftest_check.gd` — proves the convergence helper itself is non-vacuous (catches a "always returns 0" helper bug).

**Modified files:**
- `tools/gate.py` — register `ladder_selftest` (headless) + `ladder_rung0`…`ladder_rung5` (windowed) suites in `CHECKS` and `WINDOWED_SUITES`.
- `wg-10/rust/src/page_pool/` — Rung 4 ONLY: add a debug seam that computes the CPU collision field for the live producer (the single runtime-Rust change in the whole plan). Phase 0 and Phase 1 add NO Rust — page readback already exists.
- `docs/plans/STATUS.md` — append a rung-status line as each rung lands (which rung is live + its measured convergence numbers).
- `wg-10/worldgen_terrain/harness/ladder_producers.gd` — grows one branch per rung as the ladder climbs.

**Reused as-is (do NOT modify — proven):** `Wg10TerrainView`, `Wg10ClipmapRings`, `Wg10PagePool` (all four producer paths), `Wg10Streamer`, `fly_camera.gd`, profiler/overlay, `mountain_fly_runtime_config.gd` (renderer constants).

---

## Phase 0 — Scaffold the scene + reusable convergence helper

This phase builds the vessel and the measuring tool, proven on the simplest possible content, before any real biome work.

### Task 0.1: Pin the baked-reference convergence baseline (the offline number)

The offline contract test already measures the live-seam-safe-recipe vs accepted-payload gap. We must record the exact current number so Rung 1's "no regression" gate has a concrete target.

**Files:**
- Test (read/run only): `tools/dem_pack/test_mountain_world_layer_contract.py`

- [ ] **Step 1: Run the existing offline contract test and capture the gap number**

Run from repo root:
```powershell
python -m pytest tools/dem_pack/test_mountain_world_layer_contract.py -q -s
```
Expected: PASS. In the captured output, find the measured gap line (the spec records it as approximately `mean_abs≈1.211743, p95_abs≈2.276974, peak_abs≈3.200543, corr≈-0.048456`). If the test does not print these, read the test to find which assertion carries them and add a `print(...)` of the measured dict (this is a test-local diagnostic print, allowed).

- [ ] **Step 2: Record the baseline note (DONE 2026-06-04 — unit caveat captured)**

`docs/plans/LADDER_CONVERGENCE_BASELINE.md` is written. KEY FINDING captured there:
the offline `mean_abs=1.211743` is in **normalized units, not metres**, and the
test's purpose is to prove the gap EXISTS (asserts `corr<0.80`), not to set a
convergence target. The windowed ladder reads metres, a different domain.
Therefore Rung 1 **self-baselines**: its first run prints the metres mean_abs and
passes-with-warning; that measured metres value is then recorded in the note and
becomes the no-regression budget (recorded × 1.10). The Rung 1 gate code
(Task 1.1) implements this `MEAN_ABS_BUDGET < 0 → pass-with-warning` path.

- [ ] **Step 3: Commit**

```powershell
git add docs/plans/LADDER_CONVERGENCE_BASELINE.md
git commit -m @'
docs(ladder): record offline convergence baseline for rung 1 gate

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 0.2: Create the ladder scene skeleton (assembly + REFERENCE rung only)

Build the scene wired to exactly one rung — the baked REFERENCE — so we prove the vessel renders the known-good baseline before adding live rungs. Mirror `wg10_progression_review.gd`'s component assembly and probe API.

**Files:**
- Create: `wg-10/worldgen_terrain/harness/ladder_producers.gd`
- Create: `wg-10/worldgen_terrain/harness/wg10_unintercept_ladder.gd`
- Create: `wg-10/worldgen_terrain/harness/wg10_unintercept_ladder.tscn`

- [ ] **Step 1: Write `ladder_producers.gd` with the REFERENCE rung**

```gdscript
extends RefCounted

# Per-rung producer configuration for the un-intercept proving ladder.
# Each rung selects ONE Wg10PagePool producer path + scale/seed/source-window/flow.
# Rung 0 (analytic) and rungs 1..5 (live procedural) are added in later tasks.
# Reference constants are lifted from mountain_fly_producers.gd to match the accepted baseline.

const PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const MOUNTAIN := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const ACCEPTED_WORLD_LAYER_PAYLOAD := "res://worldgen_terrain/generated/review/mountain_world_layer_tiles.json"

const PAGE_PX := 256
const APRON_PX := 160
const CAPACITY := 96
const BASE_SPAN := 8192.0
const MOUNTAIN_REVIEW_SEED := 177
const FLOW_ITERS := 192
const FLOW_MAX_LEVEL := 2
const FEATURE_SPAN_NETWORK_M := 90000.0
const RELIEF_M_DEFAULT := 1700.0
# Accepted source-window transform — VERIFIED exact (mountain_fly_review_smoke_check.gd:361-363).
# These are applied as DIRECT offsets: source = display * scale + offset.
const SOURCE_SCALE := 3.515625
const SOURCE_OFFSET_X_M := 207000.0
const SOURCE_OFFSET_Z_M := 176000.0

const RUNG_REFERENCE := "reference"

var _rung := RUNG_REFERENCE

func set_rung(rung: String) -> bool:
	if rung == RUNG_REFERENCE:
		_rung = rung
		return true
	return false

func rung() -> String:
	return _rung

func relief_m() -> float:
	return RELIEF_M_DEFAULT

func configure(pool: Object) -> String:
	if _rung == RUNG_REFERENCE:
		return _configure_reference(pool)
	return "ladder_producers: unknown rung %s" % _rung

func _configure_reference(pool: Object) -> String:
	return str(pool.call("configure_static_reference",
		ProjectSettings.globalize_path(ACCEPTED_WORLD_LAYER_PAYLOAD),
		CAPACITY, PAGE_PX, BASE_SPAN, MOUNTAIN_REVIEW_SEED))
```

- [ ] **Step 2: Write `wg10_unintercept_ladder.gd` (thin assembly + probe API)**

```gdscript
extends Node3D

# Un-intercept proving ladder scene.
# Thin assembly of the proven render stack (DESIGN 6.4: scenes assemble, they do not
# contain metric/report logic). Rung selection drives which producer the pool runs.
# Wiring sequence + probe API copied from wg10_progression_review.gd (verified template).

const PRODUCERS := "res://worldgen_terrain/harness/ladder_producers.gd"
const RUNTIME_CONFIG := "res://worldgen_terrain/harness/mountain_fly_runtime_config.gd"
const FLY_CAMERA := "res://worldgen_terrain/harness/fly_camera.gd"

var _producer: Object
var _runtime: Object   # mountain_fly_runtime_config.gd instance
var _pool: Object
var _streamer: Object
var _rings: Object
var _view: Object
var _camera: Camera3D   # this IS a Wg10FlyCamera (extends Camera3D)
var _label: Label
var _frame := 0
var _probe_mode := false

func _ready() -> void:
	_runtime = load(RUNTIME_CONFIG).new()
	_runtime.call("register_shader_globals", bool(_runtime.call("default_detail_enabled")))
	_producer = load(PRODUCERS).new()
	_build_runtime()
	_build_camera()
	_build_hud()

# Wiring sequence is the exact order from wg10_progression_review.gd:_ready (verified).
func _build_runtime() -> void:
	_pool = ClassDB.instantiate("Wg10PagePool")
	var err := str(_producer.call("configure", _pool))   # producer selects the rung's producer path
	if err != "":
		push_error("[ladder] producer.configure failed: %s" % err)
	_streamer = ClassDB.instantiate("Wg10Streamer")
	_runtime.call("configure_streamer", _streamer, _pool)
	_rings = ClassDB.instantiate("Wg10ClipmapRings")
	_runtime.call("configure_rings", _rings)
	add_child(_rings)
	_view = ClassDB.instantiate("Wg10TerrainView")
	add_child(_view)
	_reconfigure_view()

func _reconfigure_view() -> void:
	if _view == null or _pool == null or _streamer == null or _rings == null or _runtime == null:
		return
	# Ladder uses the live/reference relief directly (relief_scale 1.0); rungs render real metres.
	var relief_scale := 1.0
	var relief_ref := float(_producer.call("relief_m"))
	_runtime.call("configure_view", _view, _pool, _streamer, _rings, bool(_runtime.call("default_morph_enabled")), relief_scale, relief_ref)

func _build_camera() -> void:
	# Wg10FlyCamera EXTENDS Camera3D — instantiate it AS the camera (verified fly_camera.gd:1-2).
	_camera = load(FLY_CAMERA).new()
	_camera.far = 200000.0
	add_child(_camera)
	_camera.position = Vector3(-9000.0, 5200.0, -9000.0)
	_camera.look_at(Vector3(22000.0, 250.0, 22000.0))
	if _camera.has_method("sync_mouse_from_rotation"):
		_camera.call("sync_mouse_from_rotation")

func _build_hud() -> void:
	var layer := CanvasLayer.new()
	add_child(layer)
	_label = Label.new()
	_label.position = Vector2(12, 12)
	_label.text = "rung=%s" % str(_producer.call("rung"))
	layer.add_child(_label)

func _process(_delta: float) -> void:
	if _probe_mode or _view == null or _camera == null:
		return
	_frame += 1
	var p: Vector3 = _camera.global_position
	var v: Vector3 = _camera.call("get_velocity")
	_view.call("update", p.x, p.z, v.x, v.z)

# ---- probe API (mirrors wg10_progression_review.gd; gates depend on these names) ----

func set_probe_mode(enabled: bool) -> void:
	_probe_mode = enabled
	if _label != null:
		_label.visible = not enabled

func set_rung(rung: String) -> bool:
	if not bool(_producer.call("set_rung", rung)):
		return false
	# Re-acquire a clean pool for the new producer path, then re-wire the view.
	if _pool != null and _pool.has_method("free_all"):
		_pool.call("free_all")
	_pool = ClassDB.instantiate("Wg10PagePool")
	var err := str(_producer.call("configure", _pool))
	if err != "":
		push_error("[ladder] reconfigure failed: %s" % err)
		return false
	_runtime.call("configure_streamer", _streamer, _pool)
	_reconfigure_view()
	if _label != null:
		_label.text = "rung=%s" % rung
	return true

func current_rung() -> String:
	return str(_producer.call("rung"))

func pool() -> Object:
	return _pool

# Probe-mode per-frame drive (gates call this instead of letting _process run).
func update_for_probe(px: float, pz: float, vx: float, vz: float) -> void:
	if _view != null:
		_view.call("update", px, pz, vx, vz)

func set_probe_camera_frame(eye: Vector3, look: Vector3) -> void:
	if _camera == null:
		return
	_camera.position = eye
	_camera.look_at(look)
	if _camera.has_method("sync_mouse_from_rotation"):
		_camera.call("sync_mouse_from_rotation")

# debug_tile_states lives on ClipmapRings (verified clipmap_rings.rs:392); proxy to it.
func debug_tile_states() -> PackedInt64Array:
	if _rings != null:
		return _rings.call("debug_tile_states")
	return PackedInt64Array()
```

> Wiring + method names above are VERIFIED against `wg10_progression_review.gd`,
> `mountain_fly_runtime_config.gd`, `terrain_view.rs`, `clipmap_rings.rs`, and
> `fly_camera.gd` (read 2026-06-04). The `relief_m()` accessor must exist on
> `ladder_producers.gd` (Task 0.2 Step 1 defines it). The smoke gate (Task 0.3)
> catches any residual mismatch.

- [ ] **Step 3: Create the `.tscn`**

Create `wg-10/worldgen_terrain/harness/wg10_unintercept_ladder.tscn` as a minimal scene whose root is a Node3D with the script attached. Use the existing `wg10_progression_review.tscn` as the structural template (same external resource wiring for the script). Minimal text form:
```
[gd_scene load_steps=2 format=3]
[ext_resource type="Script" path="res://worldgen_terrain/harness/wg10_unintercept_ladder.gd" id="1"]
[node name="Wg10UninterceptLadder" type="Node3D"]
script = ExtResource("1")
```

- [ ] **Step 4: Commit**

```powershell
git add wg-10/worldgen_terrain/harness/ladder_producers.gd wg-10/worldgen_terrain/harness/wg10_unintercept_ladder.gd wg-10/worldgen_terrain/harness/wg10_unintercept_ladder.tscn
git commit -m @'
feat(ladder): scaffold un-intercept ladder scene with REFERENCE rung

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 0.3: Smoke gate — the ladder scene instantiates and renders the REFERENCE rung

**Files:**
- Create: `wg-10/worldgen_terrain/tests/ladder_rung0_check.gd` (starts as a SMOKE check; expands to the analytic-parity gate in Task 0.5)
- Modify: `tools/gate.py`

- [ ] **Step 1: Write the smoke gate**

```gdscript
extends SceneTree

# Ladder scaffold smoke + (later) Rung 0 analytic plumbing gate.
# This first version proves the scene instantiates, exposes the probe API, and
# renders a non-black REFERENCE frame through the proven render stack.

const SCENE := "res://worldgen_terrain/harness/wg10_unintercept_ladder.tscn"
const VIEW_SIZE := Vector2i(640, 360)

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[ladder-rung0] status=skip reason=no-render-device")
		return 2

	var packed := load(SCENE)
	if packed == null:
		push_error("[ladder-rung0] cannot load %s" % SCENE)
		return 1

	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	get_root().add_child(vp)

	var scene: Node = packed.instantiate()
	vp.add_child(scene)
	for _i in range(120):
		await process_frame

	var errs: Array[String] = []
	for m in ["set_probe_mode", "update_for_probe", "set_probe_camera_frame", "debug_tile_states", "current_rung", "pool"]:
		if not scene.has_method(m):
			errs.append("scene missing %s" % m)
	if not errs.is_empty():
		for e in errs:
			push_error(e)
		print("[ladder-rung0] status=fail errors=%d" % errs.size())
		scene.queue_free(); vp.queue_free()
		return 1

	# Render a frame and assert non-black.
	await process_frame
	RenderingServer.force_draw()
	await process_frame
	var img := vp.get_texture().get_image()
	var nonblack := 0
	var total := 0
	for y in range(0, img.get_size().y, 8):
		for x in range(0, img.get_size().x, 8):
			total += 1
			if img.get_pixel(x, y).v > 0.04:
				nonblack += 1
	var frac := float(nonblack) / float(max(total, 1))
	scene.queue_free()
	vp.queue_free()
	await process_frame

	if frac < 0.5:
		print("[ladder-rung0] status=fail nonblack_frac=%.3f rung=reference" % frac)
		return 1
	print("[ladder-rung0] status=pass nonblack_frac=%.3f rung=reference" % frac)
	return 0
```

- [ ] **Step 2: Register the suite in `tools/gate.py`**

In `CHECKS`, add after the `review_progression` entry:
```python
    # Un-intercept proving ladder. Each rung flips one baked crutch to a live procedural path
    # and gates convergence toward the accepted baked REFERENCE. Windowed (RenderingDevice).
    "ladder_rung0": [
        "worldgen_terrain/tests/ladder_rung0_check.gd",
    ],
```
In `WINDOWED_SUITES`, add `"ladder_rung0",`.

- [ ] **Step 3: Run the smoke gate**

Ensure the owner's editor is closed, then build + run:
```powershell
powershell -ExecutionPolicy Bypass -File tools\build_rust.ps1
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'; python tools\gate.py --suite ladder_rung0
```
Expected: `[ladder-rung0] status=pass nonblack_frac=...` and `[gate] suite=ladder_rung0 ... fail=0`.
If it fails on a missing method name, fix the name in `wg10_unintercept_ladder.gd` to match the real component API (see the NOTE in Task 0.2) and re-run.

- [ ] **Step 4: Commit**

```powershell
git add wg-10/worldgen_terrain/tests/ladder_rung0_check.gd tools/gate.py
git commit -m @'
test(ladder): smoke-gate ladder scaffold rendering REFERENCE rung

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 0.4: Build the reusable convergence helper + its self-test

The helper is the heart of the ladder. It (a) reads back a live page's height via the PROVEN idiom (no new Rust), (b) the caller also reads the reference page the same way, (c) it reports `mean_abs/p95_abs/peak_abs` in metres, and (d) carries non-vacuous guards. We prove the helper's math before trusting any rung verdict.

**Files:**
- Create: `wg-10/worldgen_terrain/harness/ladder_convergence.gd`
- Create: `wg-10/worldgen_terrain/tests/ladder_convergence_selftest_check.gd`
- Modify: `tools/gate.py`

> NO RUST CHANGE in this task. Page readback already exists and is proven —
> `m3_continuity_check.gd:245-255` (`_read_page`) and `biome_runtime_isolate.gd:39-65`
> (configure→acquire→flush→`texture_get_data`). The helper packages those.

- [ ] **Step 1: Write the convergence helper (pure math + proven readback)**

```gdscript
extends RefCounted

# Reusable convergence measurement for the un-intercept ladder.
# Reads back a page's R32F height via the PROVEN idiom (acquire/get_resident -> get_texture_rd_rid
# -> rd.texture_get_data -> to_float32_array), with a force_draw() flush because the live biome
# compute is fire-and-forget on the global RD (see biome_runtime_isolate.gd). The caller reads BOTH
# the live rung's page and the reference rung's page over the same (level, origin) and passes both
# arrays to delta(). delta() reports the same shaped metric the offline contract test uses.

# Pure math: compare two flat row-major height arrays (metres). Returns {} on shape mismatch.
func delta(live: PackedFloat32Array, ref: PackedFloat32Array) -> Dictionary:
	if live.size() == 0 or live.size() != ref.size():
		return {}
	var deltas: Array[float] = []
	var total := 0.0
	var live_min := INF
	var live_max := -INF
	var ref_min := INF
	var ref_max := -INF
	for i in range(live.size()):
		var lv := live[i]
		var rv := ref[i]
		var d := absf(lv - rv)
		deltas.append(d)
		total += d
		live_min = minf(live_min, lv); live_max = maxf(live_max, lv)
		ref_min = minf(ref_min, rv); ref_max = maxf(ref_max, rv)
	deltas.sort()
	var n := deltas.size()
	# Non-vacuous: BOTH fields must have real relief (>1 m), else a flat-vs-flat bug could fake a pass.
	var live_relief := live_max - live_min
	var ref_relief := ref_max - ref_min
	return {
		"mean_abs": total / float(n),
		"p95_abs": deltas[clampi(int(floor(float(n - 1) * 0.95)), 0, n - 1)],
		"peak_abs": deltas[n - 1],
		"samples": n,
		"live_relief": live_relief,
		"ref_relief": ref_relief,
		"nonvacuous": live_relief > 1.0 and ref_relief > 1.0,
	}

# Flush the global RD so a fire-and-forget compute page becomes readable (biome_runtime_isolate idiom).
func flush_gpu(tree: SceneTree) -> void:
	for _i in range(4):
		await tree.process_frame
		RenderingServer.force_draw()
		await tree.process_frame

# Read a resident page back as floats (m3_continuity_check._read_page idiom). Empty on miss.
func read_resident_page(rd: RenderingDevice, pool: Object, level: int, ox: float, oz: float, page_px: int) -> PackedFloat32Array:
	var tex: Object = pool.call("get_resident_page", level, ox, oz)
	if tex == null:
		return PackedFloat32Array()
	var rid: RID = tex.call("get_texture_rd_rid")
	if not rid.is_valid():
		return PackedFloat32Array()
	var bytes: PackedByteArray = rd.texture_get_data(rid, 0)
	if bytes.size() < page_px * page_px * 4:
		return PackedFloat32Array()
	return bytes.to_float32_array()

# Acquire (produce) + flush + read one page for the CURRENTLY configured producer.
# This is the one-call "produce a live page and read its heights" the rungs use.
func produce_and_read(tree: SceneTree, rd: RenderingDevice, pool: Object, level: int, ox: float, oz: float, page_px: int) -> PackedFloat32Array:
	var tex = pool.call("acquire_page", level, ox, oz)
	if tex == null:
		return PackedFloat32Array()
	await flush_gpu(tree)
	return await read_resident_page(rd, pool, level, ox, oz, page_px)
```

> NOTE on `flow_on`: there is no per-call `flow_on` arg on `acquire_page`. Flow is
> a producer-config property (`flow_iters` passed to `configure_biome`, gated by
> `flow_max_level`). Rung 1 (flow off) vs Rung 2 (flow on) is therefore expressed
> as TWO producer configs in `ladder_producers.gd` (flow_iters=0 vs flow_iters=192),
> NOT a readback flag. The plan's Rung 1/2 tasks reflect this.

- [ ] **Step 2: (removed — no Rust readback needed)**

Page readback already exists and is proven. This step intentionally does nothing;
the helper above uses `acquire_page` + `get_resident_page` + `texture_get_data`,
all already registered. Proceed to Step 3.

- [ ] **Step 3: Write the helper self-test gate**

```gdscript
extends SceneTree

# Proves ladder_convergence.gd is non-vacuous: identical fields -> ~0 delta, and a known
# offset -> exactly that delta. Catches a "helper always returns 0" bug before any rung trusts it.

const HELPER := "res://worldgen_terrain/harness/ladder_convergence.gd"

func _init() -> void:
	quit(_run())

func _run() -> int:
	var h := load(HELPER).new()

	# Case 1: identical fields with real relief -> mean_abs == 0, nonvacuous true.
	var a := PackedFloat32Array()
	for i in range(64):
		a.append(float(i) * 10.0)  # 0..630 m, real relief
	var same := h.delta(a, a)
	if same.is_empty():
		print("[ladder-selftest] status=fail reason=empty-on-identical"); return 1
	if absf(float(same["mean_abs"])) > 1e-6:
		print("[ladder-selftest] status=fail mean_abs=%.8f expected 0" % float(same["mean_abs"])); return 1
	if not bool(same["nonvacuous"]):
		print("[ladder-selftest] status=fail reason=identical-marked-vacuous"); return 1

	# Case 2: constant +5 m offset -> mean_abs == 5, peak_abs == 5.
	var b := PackedFloat32Array()
	for i in range(64):
		b.append(a[i] + 5.0)
	var off := h.delta(a, b)
	if absf(float(off["mean_abs"]) - 5.0) > 1e-5 or absf(float(off["peak_abs"]) - 5.0) > 1e-5:
		print("[ladder-selftest] status=fail mean=%.6f peak=%.6f expected 5/5" % [float(off["mean_abs"]), float(off["peak_abs"])]); return 1

	# Case 3: flat fields -> vacuous flagged.
	var flat := PackedFloat32Array()
	for i in range(64):
		flat.append(3.0)
	var vac := h.delta(flat, flat)
	if bool(vac["nonvacuous"]):
		print("[ladder-selftest] status=fail reason=flat-marked-nonvacuous"); return 1

	# Case 4: shape mismatch -> empty.
	var short := PackedFloat32Array([1.0, 2.0])
	if not h.delta(a, short).is_empty():
		print("[ladder-selftest] status=fail reason=mismatch-not-empty"); return 1

	print("[ladder-selftest] status=pass cases=4")
	return 0
```

- [ ] **Step 4: Register `ladder_selftest` (headless — pure GDScript, no render device)**

In `tools/gate.py` `CHECKS`, add:
```python
    "ladder_selftest": [
        "worldgen_terrain/tests/ladder_convergence_selftest_check.gd",
    ],
```
Do NOT add this one to `WINDOWED_SUITES` (it needs no GPU).

- [ ] **Step 5: Run the self-test**

```powershell
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'; python tools\gate.py --suite ladder_selftest
```
Expected: `[ladder-selftest] status=pass cases=4` and `fail=0`.

- [ ] **Step 6: Commit** (no Rust — readback reuses existing methods)

```powershell
git add wg-10/worldgen_terrain/harness/ladder_convergence.gd wg-10/worldgen_terrain/tests/ladder_convergence_selftest_check.gd tools/gate.py
git commit -m @'
feat(ladder): reusable convergence helper + self-test (reuses proven page readback)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 0.5: Rung 0 — analytic known-height producer + parity gate

Now prove the *un-intercept mechanism* on dead-simple content: a known analytic height, generated live, read back, matched to the closed-form formula. This de-risks the flip from baked→live separately from biome content.

**Files:**
- Modify: `wg-10/worldgen_terrain/harness/ladder_producers.gd` (add the analytic rung)
- Modify: `wg-10/rust/src/page_pool/` (add a minimal analytic producer path OR reuse Legacy with a known pack — see Step 1)
- Modify: `wg-10/worldgen_terrain/tests/ladder_rung0_check.gd` (expand smoke → analytic parity)

- [ ] **Step 1: Add an `AnalyticHeight` producer arm (the only Rust in Phase 0)**

This is a tiny, self-contained producer that writes a CLOSED-FORM height so the gate can predict every texel. It de-risks the un-intercept plumbing independent of biome content. Read `wg-10/rust/src/page_pool/producer.rs` (the `ProducerKind` enum + `dispatch_page_compute` match) and `mod.rs` (the pool's `Option` fields + an existing simple `configure_*`).

In `producer.rs`: add `Analytic` to `ProducerKind` (`runtime_mode` → `"analytic"`, `uses_biome_path` → `false`). Add a pool field `analytic: Option<AnalyticParams>` where `struct AnalyticParams { amp: f64, lambda: f64 }`, set `active_producer_kind` to return `Analytic` when `self.analytic.is_some()` (place the check FIRST so it wins). Add a `dispatch_page_compute` arm:
```rust
Some(ProducerKind::Analytic) => {
    let a = self.analytic.as_ref().ok_or("analytic producer missing params")?;
    let n = page_px as usize;
    let mut data: Vec<f32> = Vec::with_capacity(n * n);
    for j in 0..n {
        // texel-CORNER convention (u = px/(N-1)) so abutting pages SHARE boundary samples (seam-exact).
        let wz = origin_z + (j as f64 / (n as f64 - 1.0)) * world_span;
        for i in 0..n {
            let wx = origin_x + (i as f64 / (n as f64 - 1.0)) * world_span;
            let h = a.amp * (wx / a.lambda).sin() * (wz / a.lambda).cos();
            data.push(h as f32);
        }
    }
    let bytes = PackedByteArray::from_iter(data.iter().flat_map(|f| f.to_le_bytes()));
    let layers = PackedByteArray::from(bytes);
    rd.texture_update(tex_rid, 0, layers);
    Ok(())
}
```
> Verify `texture_update`'s exact arg shape against how `static_reference` writes
> its page (`write_page_texture` in `static_reference/presentation.rs` or
> `sampling.rs` — it already does a CPU→GPU `texture_update` of R32F page data).
> COPY that exact call (data packing + args) rather than the sketch above; the
> sketch shows intent, the real `texture_update` signature/format must match the
> proven static path.

Add a `#[func] configure_analytic(&mut self, capacity: i64, page_px: i64, world_span: f64, amp: f64, lambda: f64) -> GString` next to `configure_static_reference` in `config_api.rs` that sets up the pool slots (copy `configure_static_reference`'s slot/policy setup) and sets `self.analytic = Some(AnalyticParams { amp, lambda })`. Validate in isolation:
```powershell
$env:CARGO_TARGET_DIR='D:/tmp/wg10_check_target'; cargo test -p wg10_terrain --lib
```
Expected: existing tests pass (count unchanged). AMP=300.0, LAMBDA=4000.0 (~600 m ptp; non-vacuous).

- [ ] **Step 2: Add the analytic rung to `ladder_producers.gd`**

```gdscript
const RUNG_ANALYTIC := "analytic"
const ANALYTIC_AMP := 300.0
const ANALYTIC_LAMBDA := 4000.0
```
Extend `set_rung` to accept `RUNG_ANALYTIC`. Add the branch in `configure`:
```gdscript
func _configure_analytic(pool: Object) -> String:
	return str(pool.call("configure_analytic", CAPACITY, PAGE_PX, BASE_SPAN, ANALYTIC_AMP, ANALYTIC_LAMBDA))
```
(`relief_m()` should return a sane value for the analytic rung too, e.g. `ANALYTIC_AMP`, so view config gets a non-zero relief_ref.)

- [ ] **Step 3: Expand `ladder_rung0_check.gd` to an analytic parity gate**

Use the convergence helper's `produce_and_read` (acquire→flush→read). Compute `frac` (non-black) BEFORE teardown. After the method-presence check:

```gdscript
	var helper := load("res://worldgen_terrain/harness/ladder_convergence.gd").new()
	var rd := RenderingServer.get_rendering_device()
	var pool: Object = scene.call("pool")
	var page_px := 256
	var span := 8192.0
	var amp := 300.0
	var lam := 4000.0

	if not bool(scene.call("set_rung", "analytic")):
		print("[ladder-rung0] status=fail reason=set_rung-analytic"); scene.queue_free(); vp.queue_free(); return 1
	for _i in range(30):
		await process_frame

	# Non-black render of the analytic surface (compute frac here, before teardown).
	await process_frame
	RenderingServer.force_draw()
	await process_frame
	var img := vp.get_texture().get_image()
	var nb := 0; var tot := 0
	for y in range(0, img.get_size().y, 8):
		for x in range(0, img.get_size().x, 8):
			tot += 1
			if img.get_pixel(x, y).v > 0.04: nb += 1
	var frac := float(nb) / float(max(tot, 1))

	# Produce + read two abutting pages via the proven idiom.
	var heights: PackedFloat32Array = await helper.produce_and_read(self, rd, pool, 0, 0.0, 0.0, page_px)
	var next_heights: PackedFloat32Array = await helper.produce_and_read(self, rd, pool, 0, span, 0.0, page_px)
	if heights.size() != page_px * page_px or next_heights.size() != page_px * page_px:
		print("[ladder-rung0] status=fail reason=bad-readback h=%d n=%d" % [heights.size(), next_heights.size()]); scene.queue_free(); vp.queue_free(); return 1

	var worst := 0.0
	for z in range(0, page_px, 16):
		for x in range(0, page_px, 16):
			var wx := (float(x) / float(page_px - 1)) * span
			var wz := (float(z) / float(page_px - 1)) * span
			var expected := amp * sin(wx / lam) * cos(wz / lam)
			worst = maxf(worst, absf(heights[z * page_px + x] - expected))
	# Seam: this page's last column == next page's first column (texel-corner share).
	var seam := 0.0
	for z in range(0, page_px, 16):
		seam = maxf(seam, absf(heights[z * page_px + (page_px - 1)] - next_heights[z * page_px + 0]))

	scene.queue_free(); vp.queue_free()
	await process_frame

	if frac < 0.5:
		print("[ladder-rung0] status=fail nonblack_frac=%.3f" % frac); return 1
	if worst > 0.01 * amp:
		print("[ladder-rung0] status=fail analytic_worst=%.5f budget=%.5f" % [worst, 0.01 * amp]); return 1
	if seam > 0.001:
		print("[ladder-rung0] status=fail seam=%.6f" % seam); return 1
	print("[ladder-rung0] status=pass analytic_worst=%.5f seam=%.6f nonblack_frac=%.3f" % [worst, seam, frac])
	return 0
```

- [ ] **Step 4: Build (editor closed) + run the gate**

```powershell
powershell -ExecutionPolicy Bypass -File tools\build_rust.ps1
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'; python tools\gate.py --suite ladder_rung0
```
Expected: `[ladder-rung0] status=pass analytic_worst=<small> seam=0.000000 nonblack_frac=...`.

- [ ] **Step 5: Owner fly + accept**

Ask the owner to open the editor and fly `res://worldgen_terrain/harness/wg10_unintercept_ladder.tscn` (Rung 0 = analytic). Confirm: a smooth sine-egg-carton surface, no holes, no pop. Record acceptance.

- [ ] **Step 6: Commit + record status**

Append to `docs/plans/STATUS.md` (top, one line): `Ladder Rung 0 (analytic plumbing) GREEN: analytic_worst=<v>, seam=0.000000; owner-flown <date>.` Then:
```powershell
git add -A
git commit -m @'
feat(ladder): rung 0 analytic un-intercept plumbing proven (parity + seam + non-black)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

## Phase 1 — The spine (Rungs 1–2): live mountain + drainage

### Task 1.1: Rung 1 — live mountain macro (flow OFF) at the reference scale

Run the REAL mountain GPU recipe (no reference binding) at the accepted scale/seed/source-window, flow off, and gate convergence vs the baked REFERENCE macro — must not regress from the offline-measured gap.

**Files:**
- Modify: `wg-10/worldgen_terrain/harness/ladder_producers.gd` (add `RUNG_MOUNTAIN_MACRO`)
- Create: `wg-10/worldgen_terrain/tests/ladder_rung1_check.gd`
- Modify: `tools/gate.py`

- [ ] **Step 1: Add the live-mountain rung to `ladder_producers.gd`**

Flow on/off is a PRODUCER CONFIG (`flow_iters`), not a readback flag. Rung 1 = flow off = `flow_iters=0`. The source transform values are VERIFIED exact (from `mountain_fly_review_smoke_check.gd:361-363`): scale `3.515625`, offsets `207000, 176000` applied directly.
```gdscript
const RUNG_MOUNTAIN_MACRO := "mountain_macro"  # live recipe, flow OFF, reference scale
const FLOW_ITERS_OFF := 0
```
In `set_rung`, accept it. Branch in `configure` — calls `configure_biome` with the reference-matching constants and `flow_iters=0`, applies the verified source transform, and binds NO reference (so dispatch reaches `compute_biome_page_cached`):
```gdscript
func _configure_mountain_macro(pool: Object) -> String:
	var err := str(pool.call("configure_biome",
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE),
		ProjectSettings.globalize_path(MOUNTAIN),
		CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, FEATURE_SPAN_NETWORK_M, FLOW_ITERS_OFF, RELIEF_M_DEFAULT, FLOW_MAX_LEVEL, MOUNTAIN_REVIEW_SEED))
	if err != "":
		return err
	# Verified accepted source-window transform (mountain_fly_review_smoke_check.gd:361-363):
	# applied as DIRECT offsets. set_biome_source_transform requires SingleBiome/World already configured.
	return str(pool.call("set_biome_source_transform", SOURCE_SCALE, SOURCE_OFFSET_X_M, SOURCE_OFFSET_Z_M))
```
(`SOURCE_SCALE`, `SOURCE_OFFSET_X_M`, `SOURCE_OFFSET_Z_M` are defined in the Task 0.2 constants block.)

- [ ] **Step 2: Write the Rung 1 convergence gate (flow OFF)**

```gdscript
extends SceneTree

# Rung 1: live mountain macro (flow OFF) vs baked REFERENCE macro, over the shared region.
# No-regression gate: live mean_abs must be <= offline baseline * 1.15 (see LADDER_CONVERGENCE_BASELINE.md).

const SCENE := "res://worldgen_terrain/harness/wg10_unintercept_ladder.tscn"
const HELPER := "res://worldgen_terrain/harness/ladder_convergence.gd"
# METRES-domain budget. The offline contract number (mean_abs=1.211743) is in NORMALIZED units,
# NOT metres (see docs/plans/LADDER_CONVERGENCE_BASELINE.md), so it CANNOT be the budget here.
# This gate self-baselines: on the FIRST run set MEAN_ABS_BUDGET to a large sentinel, read the
# printed metres mean_abs, record it in LADDER_CONVERGENCE_BASELINE.md, then set the budget to
# recorded*1.10 and commit. Until recorded, the gate PASSES-with-warning (prints the number, does
# not fail) so the first run can capture the baseline.
const MEAN_ABS_BUDGET := -1.0   # <0 = unbaselined: print + pass-with-warning, do not fail on budget

const PAGE_PX := 256

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[ladder-rung1] status=skip reason=no-render-device"); return 2

	var packed := load(SCENE)
	var vp := SubViewport.new()
	vp.size = Vector2i(640, 360); vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS; vp.own_world_3d = true
	get_root().add_child(vp)
	var scene: Node = packed.instantiate()
	vp.add_child(scene)
	for _i in range(120):
		await process_frame

	var helper := load(HELPER).new()
	var rd := RenderingServer.get_rendering_device()

	# 1) Read the LIVE mountain macro page (flow off via flow_iters=0) at a known page.
	if not bool(scene.call("set_rung", "mountain_macro")):
		print("[ladder-rung1] status=fail reason=set_rung"); _td(scene, vp); return 1
	for _i in range(30):
		await process_frame
	var live: PackedFloat32Array = await helper.produce_and_read(self, rd, scene.call("pool"), 0, 0.0, 0.0, PAGE_PX)

	# 2) Read the baked REFERENCE over the SAME page. set_rung rebuilds the pool, so re-fetch it.
	if not bool(scene.call("set_rung", "reference")):
		print("[ladder-rung1] status=fail reason=set_rung-ref"); _td(scene, vp); return 1
	for _i in range(30):
		await process_frame
	var ref: PackedFloat32Array = await helper.produce_and_read(self, rd, scene.call("pool"), 0, 0.0, 0.0, PAGE_PX)

	_td(scene, vp)
	await process_frame

	var d := helper.delta(live, ref)
	if d.is_empty():
		print("[ladder-rung1] status=fail reason=shape-mismatch live=%d ref=%d" % [live.size(), ref.size()]); return 1
	if not bool(d["nonvacuous"]):
		print("[ladder-rung1] status=fail reason=vacuous live_relief=%.2f ref_relief=%.2f" % [float(d["live_relief"]), float(d["ref_relief"])]); return 1

	var mean_abs := float(d["mean_abs"])
	print("[ladder-rung1] mean_abs=%.4f p95_abs=%.4f peak_abs=%.4f live_relief=%.1f ref_relief=%.1f budget=%.4f" % [
		mean_abs, float(d["p95_abs"]), float(d["peak_abs"]), float(d["live_relief"]), float(d["ref_relief"]), MEAN_ABS_BUDGET])
	if MEAN_ABS_BUDGET < 0.0:
		# Unbaselined first run: capture the metres number, pass-with-warning so it can be recorded.
		print("[ladder-rung1] status=pass UNBASELINED — record mean_abs=%.4f (metres) in LADDER_CONVERGENCE_BASELINE.md, then set MEAN_ABS_BUDGET=%.4f and re-commit" % [mean_abs, mean_abs * 1.10])
		return 0
	if mean_abs > MEAN_ABS_BUDGET:
		print("[ladder-rung1] status=fail mean_abs=%.4f > budget=%.4f (regression vs recorded metres baseline)" % [mean_abs, MEAN_ABS_BUDGET])
		return 1
	print("[ladder-rung1] status=pass mean_abs=%.4f budget=%.4f" % [mean_abs, MEAN_ABS_BUDGET])
	return 0

func _td(scene: Node, vp: SubViewport) -> void:
	if scene != null: scene.queue_free()
	if vp != null: vp.queue_free()
```

> IMPORTANT honest-result note: if Rung 1 FAILS the budget, that is a **real
> finding**, not necessarily a bug to tune away. Per the spec's Rung 1 plateau
> risk: the live seam-safe recipe intentionally lacks the baked field's
> full-field conditioning + pass-network carving, so it may plateau above the
> offline gap. If it fails: (a) confirm the source transform + scale match the
> reference exactly (the #1 cause), (b) confirm flow is actually OFF on both
> reads, (c) if still above budget after those, STOP and report to the owner that
> the live recipe cannot reach the baked macro without conditioning/pass-network
> as a live fact — that decision is the roadmap's next real fork, not a tuning
> task. Record the measured plateau number.

- [ ] **Step 3: Register `ladder_rung1` (windowed) + run**

In `tools/gate.py`: add `"ladder_rung1": ["worldgen_terrain/tests/ladder_rung1_check.gd"],` to `CHECKS` and `"ladder_rung1",` to `WINDOWED_SUITES`. Then:
```powershell
powershell -ExecutionPolicy Bypass -File tools\build_rust.ps1
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'; python tools\gate.py --suite ladder_rung1
```
Expected (success path): `[ladder-rung1] status=pass mean_abs=<= budget>`. (If it fails, follow the honest-result note.)

- [ ] **Step 4: Owner fly + accept**

Owner opens editor, flies the scene at Rung 1 (live mountain, flow off), A/B against REFERENCE. The macro silhouette should read as the same mountain structure (valleys will look unfinished — that's Rung 2's job). Record verdict.

- [ ] **Step 5: Commit + record status**

Append to STATUS.md: `Ladder Rung 1 (live mountain macro, flow off) <GREEN|PLATEAU>: mean_abs=<v> vs budget <b>; owner verdict <...>.` Then commit `-A` with message `feat(ladder): rung 1 live mountain macro convergence gated vs reference`.

---

### Task 1.2: Rung 2 — drainage ON (convergence improves + flow perf gate)

**Files:**
- Modify: `wg-10/worldgen_terrain/harness/ladder_producers.gd` (add `RUNG_MOUNTAIN_FLOW`)
- Create: `wg-10/worldgen_terrain/tests/ladder_rung2_check.gd`
- Modify: `tools/gate.py`

- [ ] **Step 1: Add the flow-on rung**

Flow is a producer config (`flow_iters`), so Rung 2 is a SECOND live config identical to `mountain_macro` except `flow_iters=192`:
```gdscript
const RUNG_MOUNTAIN_FLOW := "mountain_flow"  # live recipe, flow ON, reference scale
```
Add to `set_rung`. Its `configure` is `_configure_mountain_macro`'s body but passing `FLOW_ITERS` (=192) instead of `FLOW_ITERS_OFF`. Extract the shared body into a helper taking the iter count to stay DRY:
```gdscript
func _configure_live_mountain(pool: Object, flow_iters: int) -> String:
	var err := str(pool.call("configure_biome",
		ProjectSettings.globalize_path(PRIM), ProjectSettings.globalize_path(MACHINE), ProjectSettings.globalize_path(MOUNTAIN),
		CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, FEATURE_SPAN_NETWORK_M, flow_iters, RELIEF_M_DEFAULT, FLOW_MAX_LEVEL, MOUNTAIN_REVIEW_SEED))
	if err != "":
		return err
	return str(pool.call("set_biome_source_transform", SOURCE_SCALE, SOURCE_OFFSET_X_M, SOURCE_OFFSET_Z_M))
```
`mountain_macro` → `_configure_live_mountain(pool, FLOW_ITERS_OFF)`; `mountain_flow` → `_configure_live_mountain(pool, FLOW_ITERS)`. The gate reads the macro rung (flow off) and the flow rung (flow on) and compares both to REFERENCE.

- [ ] **Step 2: Write the Rung 2 gate (convergence improves vs Rung 1 + real GPU-time flow budget)**

The gate must prove TWO things:
1. **Convergence improves:** read the live page with `flow_on=true` and with `flow_on=false` over the same region vs the baked REFERENCE; assert `mean_abs(flow_on) < mean_abs(flow_off)` (drainage measurably reduces the gap). Reuse the helper.
2. **Flow perf fits budget:** measure real GPU time for a flow-on page production at production page size using `RenderingServer.viewport_get_measured_render_time_gpu` (the memory's hard rule — NOT wall-time), and assert it is under the one-frame budget (`16.7 ms`). Mirror the timing pattern in `biome_fly_perf_check.gd` (read it first and copy its GPU-timer read + warm-up frames exactly).

```gdscript
extends SceneTree

# Rung 2: drainage ON. (1) flow-on converges closer to REFERENCE than flow-off.
# (2) the live flow path fits the one-frame real-GPU-time budget in motion.
# Flow on/off are two producer configs (flow_iters 192 vs 0), not a readback flag.

const SCENE := "res://worldgen_terrain/harness/wg10_unintercept_ladder.tscn"
const HELPER := "res://worldgen_terrain/harness/ladder_convergence.gd"
const PAGE_PX := 256
const GPU_BUDGET_MS := 16.7
const WARM_FRAMES := 80      # copied from biome_fly_perf_check.gd (settle + GPU timer fill)
const MEASURE_FRAMES := 240

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[ladder-rung2] status=skip reason=no-render-device"); return 2
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	var packed := load(SCENE)
	var vp := SubViewport.new()
	vp.size = Vector2i(640, 360); vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS; vp.own_world_3d = true
	get_root().add_child(vp)
	RenderingServer.viewport_set_measure_render_time(vp.get_viewport_rid(), true)
	var scene: Node = packed.instantiate()
	vp.add_child(scene)
	for _i in range(120):
		await process_frame
	var helper := load(HELPER).new()
	var rd := RenderingServer.get_rendering_device()

	# --- Convergence: flow-off rung, flow-on rung, reference; all read at the same page. ---
	if not bool(scene.call("set_rung", "mountain_macro")):
		print("[ladder-rung2] status=fail reason=set_rung-off"); _td(scene, vp); return 1
	for _i in range(30): await process_frame
	var live_off: PackedFloat32Array = await helper.produce_and_read(self, rd, scene.call("pool"), 0, 0.0, 0.0, PAGE_PX)

	if not bool(scene.call("set_rung", "mountain_flow")):
		print("[ladder-rung2] status=fail reason=set_rung-on"); _td(scene, vp); return 1
	for _i in range(30): await process_frame
	var live_on: PackedFloat32Array = await helper.produce_and_read(self, rd, scene.call("pool"), 0, 0.0, 0.0, PAGE_PX)

	if not bool(scene.call("set_rung", "reference")):
		print("[ladder-rung2] status=fail reason=set_rung-ref"); _td(scene, vp); return 1
	for _i in range(30): await process_frame
	var ref: PackedFloat32Array = await helper.produce_and_read(self, rd, scene.call("pool"), 0, 0.0, 0.0, PAGE_PX)

	var d_off := helper.delta(live_off, ref)
	var d_on := helper.delta(live_on, ref)
	var flow_changed := helper.delta(live_off, live_on)
	if d_off.is_empty() or d_on.is_empty() or flow_changed.is_empty():
		print("[ladder-rung2] status=fail reason=shape-mismatch"); _td(scene, vp); return 1
	var flow_effect := float(flow_changed["mean_abs"])
	var mean_on := float(d_on["mean_abs"])
	var mean_off := float(d_off["mean_abs"])

	# --- Perf: fly the flow-on rung and measure real GPU-time p99 (biome_fly_perf_check idiom). ---
	if not bool(scene.call("set_rung", "mountain_flow")):
		print("[ladder-rung2] status=fail reason=set_rung-perf"); _td(scene, vp); return 1
	scene.call("set_probe_mode", true)
	var gpu_samples: Array[float] = []
	var pos := Vector2(0.0, 0.0)
	var vel := Vector2(2000.0, 0.0)   # ~2000 m/s forces fresh flow-on page production
	for f in range(WARM_FRAMES + MEASURE_FRAMES):
		pos += vel * 0.016
		scene.call("update_for_probe", pos.x, pos.y, vel.x, vel.y)
		await process_frame
		RenderingServer.force_draw()
		await process_frame
		if f >= WARM_FRAMES:
			gpu_samples.append(RenderingServer.viewport_get_measured_render_time_gpu(vp.get_viewport_rid()))
	_td(scene, vp)
	await process_frame

	var gpu_p99 := -1.0
	if gpu_samples.size() > 0:
		gpu_samples.sort()
		gpu_p99 = gpu_samples[clampi(int(floor(float(gpu_samples.size()) * 0.99)), 0, gpu_samples.size() - 1)]

	print("[ladder-rung2] mean_off=%.4f mean_on=%.4f flow_effect=%.4f gpu_p99=%.3f budget=%.1f samples=%d" % [
		mean_off, mean_on, flow_effect, gpu_p99, GPU_BUDGET_MS, gpu_samples.size()])

	if flow_effect < 1.0:
		print("[ladder-rung2] status=fail reason=flow-no-op flow_effect=%.4f" % flow_effect); return 1
	if mean_on >= mean_off:
		print("[ladder-rung2] status=fail reason=drainage-did-not-help mean_on=%.4f >= mean_off=%.4f" % [mean_on, mean_off]); return 1
	if gpu_p99 < 0.0:
		print("[ladder-rung2] status=fail reason=no-gpu-measurement (perf assertion would be vacuous)"); return 1
	if gpu_p99 > GPU_BUDGET_MS:
		print("[ladder-rung2] status=fail reason=flow-over-budget gpu_p99=%.3f > %.1f" % [gpu_p99, GPU_BUDGET_MS]); return 1
	print("[ladder-rung2] status=pass mean_on=%.4f < mean_off=%.4f flow_effect=%.4f gpu_p99=%.3f" % [mean_on, mean_off, flow_effect, gpu_p99])
	return 0

func _td(scene: Node, vp: SubViewport) -> void:
	if scene != null: scene.queue_free()
	if vp != null: vp.queue_free()
```
> `gpu_p99 < 0` (no samples) is a FAIL: we must not pass a perf gate we didn't
> measure (the "profiling must be real" memory). The GPU-time idiom
> (`viewport_set_measure_render_time` + `viewport_get_measured_render_time_gpu`,
> 80 warm frames) is copied verbatim from `biome_fly_perf_check.gd:109-148`.
> HONEST-RESULT note: if `gpu_p99 > 16.7`, that is the real "live flow doesn't fit
> the synchronous frame budget" finding the memories already flagged as likely
> (576² needs ~192 iters ≈ 6.45 ms, grid-size dependent) — record it and report
> to the owner; the resolution is the drainage-fact/async fork, NOT tuning the gate.

- [ ] **Step 3: Register `ladder_rung2` (windowed) + run**

Add to `CHECKS` + `WINDOWED_SUITES`, then build (editor closed) + run as in prior tasks. Expected: `[ladder-rung2] status=pass mean_on < mean_off ... gpu_ms <= 16.7`.

- [ ] **Step 4: Owner fly + accept**

Owner flies Rung 2; valleys/channels should now read as drainage-shaped vs Rung 1. A/B against REFERENCE. Record verdict.

- [ ] **Step 5: Commit + record status**

Append STATUS.md line with mean_on/mean_off/gpu_ms; commit `-A` with `feat(ladder): rung 2 drainage on — converges closer + fits gpu budget`.

> **MILESTONE — the spine is complete.** With Rungs 0–2 green and flown, the
> project has crossed from "flies baked terrain" to "flies live procedural
> mountains with drainage, gated against the accepted bar." Pause here for an
> owner go/no-go before Rungs 3–5 (they are completion, not the core proof).

---

## Phase 2 — Completion (Rungs 3–5): material, collision, multi-biome

> These rungs are gated on the spine being accepted. They follow the identical
> shape (add rung to `ladder_producers.gd` → write `ladder_rungN_check.gd` →
> register windowed suite → build → gate → owner fly → commit + STATUS line).
> Detailed per-rung below.

### Task 2.1: Rung 3 — material derived from the live field

**Files:** modify `ladder_producers.gd`; create `ladder_rung3_check.gd`; modify `tools/gate.py`.

- [ ] **Step 1: Add `RUNG_MOUNTAIN_MATERIAL`** — same live config as Rung 2 but the gate compares material channels instead of (or in addition to) height. The producer already computes material; the point is to compare the LIVE-derived material masks against the baked REFERENCE material masks rather than binding the baked ones.

- [ ] **Step 2: Write `ladder_rung3_check.gd`** — read back the live page's RGBA material fact texture (the same RGBA32F page the renderer samples: R=low-pass/corridor, G=floor, B=rock, A=snow) via a pool debug readback (mirror `debug_produce_and_read` but for the material texture; add `debug_produce_and_read_material` if absent). Read the baked REFERENCE material over the same region. Use the convergence helper per-channel (4 deltas). Gate: each channel non-vacuous AND the combined material delta is bounded (direction: live material should be a recognizable relative of the reference masks — set the budget from the first measured run, recorded like Rung 1's baseline, with no-regression thereafter). Keep a non-vacuous guard that the live material actually varies (not a constant mask).

- [ ] **Step 3: Register `ladder_rung3` windowed + run + owner fly + commit + STATUS line.** Message: `feat(ladder): rung 3 live-derived material gated vs reference masks`.

### Task 2.2: Rung 4 — collision parity on live pages (the one runtime-Rust change)

**Files:** modify `wg-10/rust/src/page_pool/producer.rs` (+ possibly `facts.rs` wiring); create `ladder_rung4_check.gd`; modify `tools/gate.py`.

- [ ] **Step 1: Wire `collision_field()` into the live producer path.** Read `wg-10/rust/src/page_pool/facts.rs` and `facts_api.rs`. The audit found `collision_field()` exists but is never called by live dispatch. Add a debug/test seam on the pool — `debug_collision_field(origin_x, origin_z, span, samples_per_side, seed) -> PackedFloat32Array` — that computes the CPU collision field for the CURRENT live producer over the region (compose base + edit delta, clamp), mirroring the M4 `get_collision_field` contract but routed through the live producer's height. Validate in isolation: `$env:CARGO_TARGET_DIR='D:/tmp/wg10_check_target'; cargo test -p wg10_terrain --lib` (existing tests unchanged).

- [ ] **Step 2: Write `ladder_rung4_check.gd`** — for the live mountain rung (flow on), read back the GPU page heights (`debug_produce_and_read`) and the CPU collision field (`debug_collision_field`) over the same region at matched sample positions, and assert `visible(GPU) == collision(CPU)` within the M4 parity epsilon (the existing `facts_collision_parity_check.gd` uses ~`0.0009 m` on base terrain — read it and reuse its epsilon + sampling). Non-vacuous: both fields must have real relief.

- [ ] **Step 3: Register `ladder_rung4` windowed + run + owner fly (confirm an entity dropped onto live terrain rests on the visible surface, if a quick test body is available) + commit + STATUS line.** Message: `feat(ladder): rung 4 live collision parity (visible==collision on procedural pages)`.

### Task 2.3: Rung 5 — multi-biome compose

**Files:** modify `ladder_producers.gd`; create `ladder_rung5_check.gd`; modify `tools/gate.py`.

- [ ] **Step 1: Add `RUNG_WORLD_COMPOSE`** — calls `configure_biome_world` WITHOUT binding the world preview reference (so `compute_biome_world_page_composed` actually runs), and lifts the active-biome cap for THIS scene only (call `set_biome_world_active_limit` with a value > 1, e.g. 4). Do not touch `mountain_fly_producers.gd`'s cap.

- [ ] **Step 2: Write `ladder_rung5_check.gd`** — produce a composed page in a region the grammar routes to multiple biomes; assert (a) genuinely multi-biome: query the pool's weight-field report (`debug_world_biome_weight_field_report_for_page` exists) and assert `active_biomes > 1` with normalized weights; (b) seam-exact: two abutting composed pages share their boundary column within epsilon (reuse the seam check from Rung 0); (c) perf: real GPU-time per composed page under budget — and explicitly catch the known ~1.9 s synchronous-compose hitch (fail with `reason=compose-over-budget` so the honest outcome "compose needs async/cache" is recorded, not worked around). Convergence vs REFERENCE is checked only where the mountain biome overlaps the reference region (optional/secondary).

- [ ] **Step 3: Register `ladder_rung5` windowed + run + owner fly + commit + STATUS line.** Message: `feat(ladder): rung 5 live multi-biome compose (multi-biome + seam + budget gated)`.

---

## Final Task: Roll up the ladder into a single suite + docs

- [ ] **Step 1: Add a `ladder_all` convenience suite** in `tools/gate.py` `CHECKS` listing the selftest + all green rung checks in order (selftest, rung0, rung1, rung2, and 3–5 as they land), and add to `WINDOWED_SUITES`. This runs the whole ladder serially for a single "is the live path still proven end-to-end?" check.

- [ ] **Step 2: Run the full ladder** (editor closed): `python tools\gate.py --suite ladder_all`. Expected: all rungs pass. Record the final convergence numbers per rung.

- [ ] **Step 3: Update the roadmap + memory.** Append to `docs/plans/ROADMAP.md` a short "Un-intercept ladder" status block (which rungs are live, their convergence numbers, that the live procedural path is now proven end-to-end). Update STATUS.md top. Do NOT rewrite history; append.

- [ ] **Step 4: Final commit.** `docs(ladder): record end-to-end live procedural path proven via un-intercept ladder`.

---

## Self-Review notes (author)

- **Spec coverage:** Rung 0 (plumbing) → Task 0.5; Rung 1 (live mountain) → 1.1; Rung 2 (drainage) → 1.2; Rung 3 (material) → 2.1; Rung 4 (collision) → 2.2; Rung 5 (multi-biome) → 2.3. Gate strategy (convergence + perf + non-vacuous + owner fly) → embedded in every rung gate. Threshold policy (direction + no-regression) → Task 0.1 baseline + Rung 1 budget + Rung 2 "must improve". Hard constraints (no baked mutation, serial gates, no worktree clean, isolated cargo check, real GPU time) → header + per-task commands. Clean new scene (not progression retrofit) → Phase 0.
- **API names VERIFIED against source (2026-06-04)** and the plan corrected accordingly. The header's "VERIFIED API FACTS" block is authoritative. Key corrections from the first draft: (1) page readback already exists — `produce_and_read`/`_read_page` reuse `acquire_page`+`get_resident_page`+`texture_get_data`, NO new Rust in Phase 0/1; (2) wiring is `configure_streamer`/`configure_rings`/`configure_view`, not `apply_to_view`/`set_pool`; (3) per-frame is `view.update(...)`; (4) `debug_tile_states` is on ClipmapRings; (5) camera is `Wg10FlyCamera extends Camera3D`; (6) source transform is `set_biome_source_transform(3.515625, 207000, 176000)` as DIRECT offsets (verified by `mountain_fly_review_smoke_check.gd:361-363`); (7) flow on/off is a `flow_iters` producer config (192 vs 0), not a readback flag. The smoke gate (Task 0.3) catches any residual mismatch first.
- **The ONLY Rust additions** are: Rung 0's tiny `AnalyticHeight` producer arm (Task 0.5 Step 1) and Rung 4's live-collision seam (Task 2.2 Step 1). Both validated in isolation with `CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo test` before the editor-closed build.
- **Honest-failure paths** are first-class: Rung 1 plateau (recipe may not reach baked conditioning/pass-network quality), Rung 2 flow-over-budget (live flow may not fit synchronous frame — the memories predict this), and Rung 5 compose-hitch all have "STOP and report, don't tune around it" instructions, per the spec's anti-parity-theater stance.
- **Remaining intentional non-literal steps:** Rungs 3–5 (Phase 2) are specified at task+gate-shape granularity rather than full literal code, because they are gated on the spine being accepted and their exact gate code depends on numbers measured in Rungs 0–2 (e.g. Rung 3's material budget is set from its first measured run, like Rung 1's baseline). This is a deliberate, flagged deferral — not a placeholder — consistent with the spec's "rungs 0–2 are the spine; 3–5 are completion" scoping. When the spine lands, Phase 2 tasks get the same full-literal treatment Phase 0/1 received.
