# WorldGen10 M3 Slice 6 — Fly Harness + p99 Acceptance Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the four §6.4 harness components (free-fly camera/movement, profiler, diagnostics overlay, review scene) and an automated `m3_accept_check` gate that drives the live `Wg10TerrainView.update` loop over a scripted ~1000 m/s flight path and asserts p99(total frame time) < 6 ms + no-black + never-stall — closing M3 pending the owner's manual fly.

**Architecture:** Four self-contained GDScript harness components under a `harness/` addon folder, each with a narrow interface and config (no terrain knowledge). The review scene assembles `{Wg10TerrainView + Wg10FlyCamera + Wg10Profiler + Wg10DiagnosticsOverlay}` and wires camera→view.update + profiler/view→overlay. The automated gate reuses the SAME view.update loop but drives it from a scripted path (no input), captures per-frame deltas via the profiler, disables vsync, and asserts the budget. No Rust changes — this is scene-side glue + a windowed gate.

**Tech Stack:** Godot 4.6 GDScript, the existing Rust classes (Wg10PagePool/Wg10Streamer/Wg10ClipmapRings/Wg10TerrainView), windowed gate via `tools/gate.py`.

---

## Conventions (read before Task 1)

- **No Rust changes.** All four classes this wires already exist:
  - `Wg10PagePool.configure(pack_dir, pack_file, glsl_path, capacity, page_px, world_span, seed) -> String`
  - `Wg10Streamer.configure(pool, num_levels, base_span, radius_pages, lead_frames, max_per_frame)`
  - `Wg10ClipmapRings.configure(num_levels, base_span, grid_res, shader_path)`
  - `Wg10TerrainView.configure(pool, streamer, rings, num_levels, base_span, height_scale, morph_region, relief_ref)`,
    `Wg10TerrainView.update(cam_x, cam_z, vel_x, vel_z)`, `Wg10TerrainView.stats() -> Dictionary` (created/reused/recomputed/full_events/resident).
- **Windowed gate** — the CONTROLLER runs it (`python tools/gate.py --suite m3`, GODOT_BIN set); a subagent without windowed Godot writes the files + reports DONE.
- **Commit trailer:** `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`. Stay on branch `main`.
- Harness GDScript uses TABS (match the existing `tests/*.gd`).
- The harness lives at `wg-10/worldgen_terrain/harness/` (a drop-in unit, separate from the addon's tests).
- gate constants reused from `m3_view_check.gd`: PACK_RES_DIR `res://worldgen_terrain/packs/dem_v1`, PACK_FILE `terrain_pack.gate.json`, GLSL `res://worldgen_terrain/shaders/height_page.glsl`, SHADER `res://worldgen_terrain/shaders/ring_displace.gdshader`, PAGE_PX 256, SEED 1337, NUM_LEVELS 3, BASE_SPAN 8192.0, GRID_RES 64, RADIUS_PAGES 1, LEAD_FRAMES 8.0, MAX_PER_FRAME 4, CAPACITY 48, MORPH_REGION 0.15, HEIGHT_SCALE 0.35, RELIEF_REF 2000.0.

---

## File Structure

**New (harness addon `wg-10/worldgen_terrain/harness/`):**
- `profiler.gd` — `Wg10Profiler` (Node): frame-delta ring buffer + p99/mean/max/fps/gpu_ms.
- `fly_camera.gd` — `Wg10FlyCamera` (Camera3D): free-fly input → position/velocity.
- `diagnostics_overlay.gd` — `Wg10DiagnosticsOverlay` (CanvasLayer): HUD reading profiler + view.stats().
- `m3_review.gd` (+ `m3_review.tscn`) — the thin assembly scene/script the owner flies.

**New (gate):**
- `wg-10/worldgen_terrain/tests/m3_accept_check.gd` — automated p99/no-black/never-stall gate.

**Modify:** `tools/gate.py` — add `m3_accept_check.gd` to the `m3` suite.

---

## Task 1: `Wg10Profiler` — frame-time capture

**Files:** Create `wg-10/worldgen_terrain/harness/profiler.gd`

The profiler is the foundation (the gate + overlay both read it). It's a generic Node: push
each frame's delta into a ring buffer, expose percentiles. No terrain knowledge.

- [ ] **Step 1: Write the component**

Create `wg-10/worldgen_terrain/harness/profiler.gd` (TABS):
```gdscript
extends Node
class_name Wg10Profiler

# Generic frame-time profiler: pushes each frame's delta into a fixed ring buffer and exposes
# p99/mean/max/fps over the captured window, plus a GPU-time cross-check. Attach to any scene;
# knows nothing about terrain. Config: ring size.

@export var ring_size: int = 512

var _ring: PackedFloat32Array = PackedFloat32Array()
var _idx: int = 0
var _count: int = 0

func _ready() -> void:
	_ring.resize(ring_size)

func _process(delta: float) -> void:
	_ring[_idx] = delta
	_idx = (_idx + 1) % ring_size
	_count = min(_count + 1, ring_size)

## Clear the captured window (call before a measured run so warm-up frames don't pollute p99).
func reset() -> void:
	_idx = 0
	_count = 0

## Manually push a frame delta (for the automated gate, which steps frames explicitly rather
## than relying on _process). Seconds.
func push(delta: float) -> void:
	_ring[_idx] = delta
	_idx = (_idx + 1) % ring_size
	_count = min(_count + 1, ring_size)

func _sorted_window() -> Array:
	var w := []
	for i in range(_count):
		w.append(_ring[i])
	w.sort()
	return w

## p99 frame time in MILLISECONDS over the captured window (0 if empty).
func p99_ms() -> float:
	if _count == 0: return 0.0
	var w := _sorted_window()
	var i := int(ceil(0.99 * w.size())) - 1
	i = clamp(i, 0, w.size() - 1)
	return w[i] * 1000.0

func mean_ms() -> float:
	if _count == 0: return 0.0
	var s := 0.0
	for i in range(_count): s += _ring[i]
	return (s / _count) * 1000.0

func max_ms() -> float:
	if _count == 0: return 0.0
	var m := 0.0
	for i in range(_count): m = max(m, _ring[i])
	return m * 1000.0

func fps() -> float:
	var mean := mean_ms()
	return 0.0 if mean <= 0.0 else 1000.0 / mean

## GPU frame time (ms) from Godot's monitor — the diagnostic CPU-vs-GPU split. Returns 0 if
## the monitor is unavailable on this platform/build.
func gpu_ms() -> float:
	return Performance.get_monitor(Performance.RENDER_VIDEO_MEM_USED) * 0.0 + _gpu_time_ms()

func _gpu_time_ms() -> float:
	# Godot 4.x exposes GPU frame time via the "RenderingServer" / Performance monitors. Use the
	# best available; fall back to 0.0 if absent.
	if Performance.has_method("get_monitor"):
		# TIME_* monitors are in seconds in some builds, ms in others — RENDER GPU time:
		var v := Performance.get_monitor(Performance.TIME_PROCESS)
		# TIME_PROCESS is CPU process time (s). For GPU, prefer the dedicated monitor if present.
		return v * 1000.0
	return 0.0
```

NOTE for the implementer (verify windowed): the exact GPU-time monitor in Godot 4.6 may be
`Performance.TIME_PROCESS` (CPU) vs a GPU-specific one. The HONEST budget number is the
**total frame delta** (p99_ms/mean_ms/max_ms from `_process`/`push`) — that's what the gate
asserts. `gpu_ms()` is a SECONDARY diagnostic; if the exact GPU monitor name is uncertain, make
`gpu_ms()` return Godot's `Performance.get_monitor(Performance.TIME_PROCESS) * 1000.0` (CPU
process time) clearly labeled, OR `RenderingServer.get_rendering_info(...)` if a GPU-time field
exists — whichever is real in 4.6. Do NOT block on gpu_ms; the gate uses total frame time.
Simplify `gpu_ms` to whatever compiles and returns a real number (the `* 0.0 +` above is a
placeholder hack — replace with the correct single monitor read).

- [ ] **Step 2: Sanity-check it parses**

If Godot is runnable: `& $GODOT_BIN --headless --check-only wg-10/worldgen_terrain/harness/profiler.gd` (or rely on the import pass). Expected: no parse error. (Controller verifies in the Task 5 gate run.)

- [ ] **Step 3: Commit**

```powershell
git add wg-10/worldgen_terrain/harness/profiler.gd
git commit -m "feat(m3): Wg10Profiler harness component (frame-time p99 capture)

Generic Node: pushes each frame delta into a fixed ring buffer, exposes p99_ms/mean_ms/max_ms/
fps over the window + reset()/push() (the gate steps frames explicitly) + a gpu_ms() diagnostic.
Total frame delta is the honest 6ms-budget number; gpu_ms is the CPU/GPU split. No terrain
knowledge — attach to any scene (§6.4).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `Wg10FlyCamera` — free-fly camera + movement

**Files:** Create `wg-10/worldgen_terrain/harness/fly_camera.gd`

Free-fly rig: WASD + Shift speed + mouse look + Space/C vertical; bindings/speeds in config;
exposes position + velocity. No terrain knowledge.

- [ ] **Step 1: Write the component**

Create `wg-10/worldgen_terrain/harness/fly_camera.gd` (TABS):
```gdscript
extends Camera3D
class_name Wg10FlyCamera

# Free-fly camera+movement rig (§6.4): WASD horizontal, Space/C vertical, mouse look (while
# captured), Shift speed boost. Exposes position + velocity each frame via get_velocity()/the
# node's global_position. Config-driven (no magic numbers). Knows nothing about terrain.

@export var move_speed: float = 2000.0       # m/s base
@export var sprint_mult: float = 4.0          # Shift multiplier (reach ~1000s of m/s)
@export var vertical_speed: float = 1500.0    # m/s for Space/C
@export var mouse_sensitivity: float = 0.0025
@export var capture_mouse: bool = true

var _velocity: Vector3 = Vector3.ZERO
var _yaw: float = 0.0
var _pitch: float = 0.0

func _ready() -> void:
	if capture_mouse:
		Input.mouse_mode = Input.MOUSE_MODE_CAPTURED
	_yaw = rotation.y
	_pitch = rotation.x

func _input(event: InputEvent) -> void:
	if event is InputEventMouseMotion and Input.mouse_mode == Input.MOUSE_MODE_CAPTURED:
		_yaw -= event.relative.x * mouse_sensitivity
		_pitch = clamp(_pitch - event.relative.y * mouse_sensitivity, -1.5, 1.5)
		rotation = Vector3(_pitch, _yaw, 0.0)
	elif event is InputEventKey and event.pressed and event.keycode == KEY_ESCAPE:
		Input.mouse_mode = Input.MOUSE_MODE_VISIBLE

func _process(delta: float) -> void:
	var dir := Vector3.ZERO
	var basis_ := global_transform.basis
	if Input.is_key_pressed(KEY_W): dir -= basis_.z
	if Input.is_key_pressed(KEY_S): dir += basis_.z
	if Input.is_key_pressed(KEY_A): dir -= basis_.x
	if Input.is_key_pressed(KEY_D): dir += basis_.x
	# horizontal movement on the XZ plane uses the look basis; vertical is world-up.
	var up := 0.0
	if Input.is_key_pressed(KEY_SPACE): up += 1.0
	if Input.is_key_pressed(KEY_C): up -= 1.0
	var speed := move_speed * (sprint_mult if Input.is_key_pressed(KEY_SHIFT) else 1.0)
	var step := dir.normalized() * speed * delta + Vector3.UP * (up * vertical_speed * delta)
	if delta > 0.0:
		_velocity = step / delta
	global_position += step

## Current velocity (m/s), world space.
func get_velocity() -> Vector3:
	return _velocity
```

NOTE: `class_name` lets the scene reference it; if `class_name` collides with anything, drop it
and load by path. The Camera3D base means it IS the viewpoint. Config defaults give a base
2000 m/s, ×4 sprint = 8000 m/s top — comfortably spanning the ~1000 m/s acceptance speed.

- [ ] **Step 2: Commit**

```powershell
git add wg-10/worldgen_terrain/harness/fly_camera.gd
git commit -m "feat(m3): Wg10FlyCamera harness component (free-fly rig)

Camera3D free-fly: WASD + Shift sprint + mouse look (captured, ESC releases) + Space/C
vertical. Speeds/sensitivity in @export config (no magic numbers); base 2000 m/s, x4 sprint
spans the ~1000 m/s acceptance speed. Exposes get_velocity() + global_position; knows nothing
about terrain (§6.4) — the review scene feeds them to Wg10TerrainView.update.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: `Wg10DiagnosticsOverlay` — HUD

**Files:** Create `wg-10/worldgen_terrain/harness/diagnostics_overlay.gd`

A CanvasLayer HUD reading the profiler + view.stats() through narrow interfaces.

- [ ] **Step 1: Write the component**

Create `wg-10/worldgen_terrain/harness/diagnostics_overlay.gd` (TABS):
```gdscript
extends CanvasLayer
class_name Wg10DiagnosticsOverlay

# Live diagnostics HUD (§6.4): reads fps/p99/gpu from a Wg10Profiler and resident/created/
# full_events from a terrain view's stats() — both through narrow interfaces. Knows nothing
# about HOW those numbers are produced. Config: font size, update interval.

@export var update_interval: float = 0.25
@export var font_size: int = 16

var _profiler: Node = null      # Wg10Profiler
var _view: Object = null        # Wg10TerrainView (has stats())
var _label: Label
var _accum: float = 0.0

func _ready() -> void:
	_label = Label.new()
	_label.position = Vector2(12, 12)
	_label.add_theme_font_size_override("font_size", font_size)
	add_child(_label)

## Wire the data sources (called by the review scene).
func bind_sources(profiler: Node, view: Object) -> void:
	_profiler = profiler
	_view = view

func _process(delta: float) -> void:
	_accum += delta
	if _accum < update_interval:
		return
	_accum = 0.0
	var lines := []
	if _profiler != null:
		lines.append("fps %.0f   frame p99 %.2f ms   mean %.2f ms   max %.2f ms" % [
			_profiler.call("fps"), _profiler.call("p99_ms"), _profiler.call("mean_ms"), _profiler.call("max_ms")])
	if _view != null:
		var s: Dictionary = _view.call("stats")
		lines.append("resident %d   created %d   recomputed %d   full %d" % [
			int(s.get("resident", 0)), int(s.get("created", 0)), int(s.get("recomputed", 0)), int(s.get("full_events", 0))])
	_label.text = "\n".join(lines)
```

- [ ] **Step 2: Commit**

```powershell
git add wg-10/worldgen_terrain/harness/diagnostics_overlay.gd
git commit -m "feat(m3): Wg10DiagnosticsOverlay harness component (live HUD)

CanvasLayer HUD: reads fps/frame-p99/mean/max from a Wg10Profiler and resident/created/
recomputed/full from a view's stats() — both via narrow interfaces (bind_sources), knows
nothing about how they're produced (§6.4). Config: update interval, font size. The owner reads
this during the manual fly.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Review scene — `m3_review.gd` (+ .tscn) the assembly point

**Files:** Create `wg-10/worldgen_terrain/harness/m3_review.gd`, `wg-10/worldgen_terrain/harness/m3_review.tscn`

The thin scene the owner launches. Assembles pool/streamer/rings/view + camera + profiler +
overlay and wires them. No component logic.

- [ ] **Step 1: Write the assembly script**

Create `wg-10/worldgen_terrain/harness/m3_review.gd` (TABS):
```gdscript
extends Node3D

# M3 review scene (§7.4): the thin assembly point the OWNER flies. Instantiates + configures
# pool/streamer/rings/Wg10TerrainView, adds a Wg10FlyCamera, a Wg10Profiler, and a
# Wg10DiagnosticsOverlay, and each frame feeds the camera's pos/vel to view.update. Pure
# assembly — no component logic here. Launch this scene and fly with WASD+Shift+mouse+Space/C.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const PAGE_PX := 256
const SEED := 1337
const NUM_LEVELS := 3
const BASE_SPAN := 8192.0
const GRID_RES := 64
const RADIUS_PAGES := 1
const LEAD_FRAMES := 8.0
const MAX_PER_FRAME := 4
const CAPACITY := 48
const MORPH_REGION := 0.15
const HEIGHT_SCALE := 0.35
const RELIEF_REF := 2000.0

var _view: Object
var _camera: Wg10FlyCamera

func _ready() -> void:
	if RenderingServer.get_rendering_device() == null:
		push_error("m3_review: no RenderingDevice (run windowed)"); return
	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)

	var pool := ClassDB.instantiate("Wg10PagePool")
	var err := str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if err != "":
		push_error("pool configure failed: %s" % err); return
	var streamer := ClassDB.instantiate("Wg10Streamer")
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_FRAMES, MAX_PER_FRAME)
	var rings := ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	add_child(rings)
	_view = ClassDB.instantiate("Wg10TerrainView")
	_view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF)

	# environment + light
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.45, 0.62, 0.85)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	add_child(light)

	# fly camera (start above the terrain looking down-forward)
	_camera = Wg10FlyCamera.new()
	_camera.environment = env
	_camera.far = BASE_SPAN * 32.0
	_camera.global_position = Vector3(0.0, 1200.0, 0.0)
	add_child(_camera)

	# profiler + overlay
	var profiler := Wg10Profiler.new()
	add_child(profiler)
	var overlay := Wg10DiagnosticsOverlay.new()
	add_child(overlay)
	overlay.bind_sources(profiler, _view)

func _process(_delta: float) -> void:
	if _view == null or _camera == null:
		return
	var p := _camera.global_position
	var v := _camera.get_velocity()
	_view.call("update", p.x, p.z, v.x, v.z)
```

- [ ] **Step 2: Create the scene file**

Create `wg-10/worldgen_terrain/harness/m3_review.tscn` — a minimal scene with a single Node3D
root that has `m3_review.gd` attached:
```
[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://worldgen_terrain/harness/m3_review.gd" id="1"]

[node name="M3Review" type="Node3D"]
script = ExtResource("1")
```

- [ ] **Step 3: Commit**

```powershell
git add wg-10/worldgen_terrain/harness/m3_review.gd wg-10/worldgen_terrain/harness/m3_review.tscn
git commit -m "feat(m3): m3_review scene — the thin fly-test assembly (§7.4)

Assembles pool+streamer+rings+Wg10TerrainView + Wg10FlyCamera + Wg10Profiler +
Wg10DiagnosticsOverlay and feeds the camera's pos/vel to view.update each frame. Pure assembly,
no component logic. The OWNER launches this scene and flies (WASD+Shift+mouse+Space/C) for the
M3 manual acceptance.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: `m3_accept_check.gd` — automated p99/no-black/never-stall gate (CONTROLLER runs windowed)

**Files:** Create `wg-10/worldgen_terrain/tests/m3_accept_check.gd`; Modify `tools/gate.py`

Drives the SAME view.update loop over a scripted ~1000 m/s flight path, captures frame deltas,
asserts the budget. CONTROLLER runs windowed + reads the printed numbers + tunes.

- [ ] **Step 1: gate.py — add to the m3 suite**

In `tools/gate.py`, append `"worldgen_terrain/tests/m3_accept_check.gd"` to the `"m3"` list.

- [ ] **Step 2: Write the gate** (TABS)

Create `wg-10/worldgen_terrain/tests/m3_accept_check.gd`:
```gdscript
extends SceneTree

# M3 acceptance gate (§7.3 — the REGRESSION CATCHER; the owner's manual fly is the final
# authority). Drives Wg10TerrainView.update over a scripted ~1000 m/s flight path (straight
# runs + turns across many page boundaries) in a SubViewport with a flight-POV camera, captures
# total per-frame time, and asserts p99 < 6 ms + no-black + never-stall over the measured run.
# vsync disabled so frame time is real. WINDOWED. Prints p99/mean/max/gpu; saves a PNG.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const PROFILER := "res://worldgen_terrain/harness/profiler.gd"
const PAGE_PX := 256
const SEED := 1337
const NUM_LEVELS := 3
const BASE_SPAN := 8192.0
const GRID_RES := 64
const RADIUS_PAGES := 1
const LEAD_FRAMES := 8.0
const MAX_PER_FRAME := 4
const CAPACITY := 48
const MORPH_REGION := 0.15
const HEIGHT_SCALE := 0.35
const RELIEF_REF := 2000.0
const VIEW_SIZE := Vector2i(960, 540)

const SPEED := 1000.0          # ~1000 m/s acceptance speed
const WARM_FRAMES := 60        # let streaming + frame times settle (excluded from p99)
const MEASURE_FRAMES := 240    # measured window
const P99_BUDGET_MS := 6.0
const STALL_CEIL_MS := 33.0    # no single frame worse than this (a visible hitch)
const MIN_NONBLACK := 0.90     # flight POV has sky; terrain must dominate the lower frame

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10TerrainView"):
		push_error("Wg10TerrainView not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-m3-accept] status=skip reason=no-render-device"); return 2

	# vsync off so frame time reflects real render cost, not the monitor cap.
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)

	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	var pool := ClassDB.instantiate("Wg10PagePool")
	var err := str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if err != "":
		push_error("pool configure failed: %s" % err); return 1
	var streamer := ClassDB.instantiate("Wg10Streamer")
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_FRAMES, MAX_PER_FRAME)
	var rings := ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	var view := ClassDB.instantiate("Wg10TerrainView")
	view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF)

	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.45, 0.62, 0.85)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	var cam := Camera3D.new()
	cam.far = BASE_SPAN * 32.0
	cam.environment = env
	vp.add_child(rings)
	vp.add_child(light)
	vp.add_child(cam)
	get_root().add_child(vp)

	var profiler = load(PROFILER).new()
	vp.add_child(profiler)

	# Scripted flight path: straight + turning legs at SPEED across many page boundaries.
	# heading changes each leg so the streamer's velocity-lead is exercised in multiple dirs.
	var headings := [Vector2(1,0), Vector2(0.7,0.7), Vector2(0,1), Vector2(-0.7,0.7), Vector2(1,0)]
	var pos := Vector2(0.0, 0.0)
	var errs: Array[String] = []
	var black_frames := 0
	var frame := 0
	var dt := 1.0 / 60.0   # fixed step for a deterministic path (real frame time is measured separately)

	var total := WARM_FRAMES + MEASURE_FRAMES
	for f in range(total):
		var heading: Vector2 = headings[(f / 60) % headings.size()]
		var vx := heading.x * SPEED
		var vz := heading.y * SPEED
		pos += Vector2(vx, vz) * dt
		var t0 := Time.get_ticks_usec()
		view.call("update", pos.x, pos.y, vx, vz)
		# position the flight-POV camera: behind+above the point, looking forward along heading.
		var eye := Vector3(pos.x - heading.x * 600.0, 900.0, pos.y - heading.y * 600.0)
		var look := Vector3(pos.x + heading.x * 1200.0, 0.0, pos.y + heading.y * 1200.0)
		cam.look_at_from_position(eye, look, Vector3.UP)
		await process_frame
		RenderingServer.force_draw()
		await process_frame
		var ft := (Time.get_ticks_usec() - t0) / 1000.0   # ms wall time for this frame's work
		if f >= WARM_FRAMES:
			profiler.call("push", ft / 1000.0)   # push expects seconds
			# no-black sample (every 12th measured frame to keep it cheap)
			if (f - WARM_FRAMES) % 12 == 0:
				var img := vp.get_texture().get_image()
				if img != null:
					var nb := 0
					var tot := img.get_width() * img.get_height()
					for y in range(0, img.get_height(), 4):
						for x in range(0, img.get_width(), 4):
							var c := img.get_pixel(x, y)
							if c.r > 0.03 or c.g > 0.03 or c.b > 0.03:
								nb += 1
					var frac := float(nb) / float(tot / 16)
					if frac < MIN_NONBLACK:
						black_frames += 1
						errs.append("frame %d: black/holes nonblack=%.3f < %.2f" % [f, frac, MIN_NONBLACK])
				if f == WARM_FRAMES:
					img.save_png("user://m3_accept.png")
		frame = f

	var p99 := float(profiler.call("p99_ms"))
	var mean := float(profiler.call("mean_ms"))
	var mx := float(profiler.call("max_ms"))
	if p99 > P99_BUDGET_MS:
		errs.append("p99 %.2f ms > %.1f ms budget (at ~1000 m/s)" % [p99, P99_BUDGET_MS])
	if mx > STALL_CEIL_MS:
		errs.append("stall: max frame %.2f ms > %.1f ms ceiling" % [mx, STALL_CEIL_MS])

	pool.call("free_all")

	print("[wg10-m3-accept] p99=%.2fms mean=%.2fms max=%.2fms speed=%dm/s frames=%d" % [p99, mean, mx, int(SPEED), MEASURE_FRAMES])
	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m3-accept] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-m3-accept] status=pass p99=%.2fms (budget %.1fms)" % [p99, P99_BUDGET_MS])
	return 0
```

NOTE for the CONTROLLER (you run this windowed):
- The per-frame wall-time uses `Time.get_ticks_usec()` around `update + force_draw + process_frame`.
  Because the SubViewport renders on UPDATE_ALWAYS and `force_draw()` flushes, this captures the
  render. If the measured frame time looks dominated by the `await process_frame` scheduling
  rather than render, switch to reading Godot's frame time monitor (`Performance.get_monitor(
  Performance.TIME_PROCESS)`) accumulated, or measure across N frames and divide. VALIDATE the
  number is sane (a flat plane should be << 6ms; the 3×3 terrain some ms) before trusting pass/fail.
- If p99 FAILS: real finding. Print the gpu/cpu split, inspect — the 3×3 overlap overdraw is the
  prime suspect. Do NOT raise P99_BUDGET_MS to pass. Surface it with the candidate levers
  (toroidal rebind / hollow-coarse / fewer tiles).
- Tune WARM_FRAMES/MEASURE_FRAMES/camera framing windowed; MIN_NONBLACK may need adjusting for
  the sky fraction in a flight POV (the budget is no-black TERRAIN, not no-sky). Adjust the
  nonblack sample region to the lower ~⅔ of the frame if sky dilutes it — but do NOT weaken it
  into vacuity.

- [ ] **Step 3 (CONTROLLER): build, run m3, read the numbers, inspect the PNG**
```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools/gate.py --suite m3
```
Expected: `[wg10-m3-accept] p99=..ms ... status=pass` and `[gate] suite=m3 checks=5 fail=0`.
Inspect `user://m3_accept.png` (flight POV of the terrain at speed). slice-1/2/3/view still pass.

- [ ] **Step 4 (CONTROLLER): fast + gpu unchanged**
```powershell
python tools/gate.py --suite fast
python tools/gate.py --suite gpu
```

- [ ] **Step 5: Commit**
```powershell
git add wg-10/worldgen_terrain/tests/m3_accept_check.gd tools/gate.py
git commit -m "test(m3): m3_accept_check — automated p99<6ms + no-black acceptance gate

Drives Wg10TerrainView.update over a scripted ~1000 m/s flight path (straight + turning legs
across many page boundaries) in a SubViewport flight POV, vsync disabled, captures total
per-frame time, asserts p99 < 6 ms + no-black + never-stall (max < 33ms) over a 240-frame
measured window after warm-up. Prints p99/mean/max; saves m3_accept.png. The REGRESSION CATCHER
(§7.3); the owner's manual fly of m3_review.tscn is the final authority. m3 suite now 5 checks.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: STATUS + ROADMAP — harness + gate done; M3 milestone OPEN for the manual fly

**Files:** Modify `docs/plans/STATUS.md`, `docs/plans/ROADMAP.md`

- [ ] **Step 1: STATUS.md** — `Last updated:` + add current-state bullets for the four harness
  components + `m3_accept_check` with the MEASURED p99 number (copy the actual printed value).
  "What's next": the M3 milestone has ONE remaining box — the owner's manual fly of
  `m3_review.tscn`. Update the gate-runner line (m3 = 5 checks). Record the p99 number honestly
  (with the windowed-measurement caveat from spec §6). State plainly: automated gate green; M3
  OPEN pending the owner's manual fly.

- [ ] **Step 2: ROADMAP.md** — `Last updated:`; flip the harness + acceptance-gate items to
  DONE; the **MANUAL ACCEPTANCE** box stays `[ ]` (the owner's fly). Note the measured p99.

- [ ] **Step 3: fresh evidence** — copy ACTUAL numbers:
```powershell
$env:CARGO_TARGET_DIR=$null; Push-Location wg-10/rust; cargo test 2>&1 | Select-String "test result"; Pop-Location
python tools/gate.py --suite m3
```

- [ ] **Step 4: Commit**
```powershell
git add docs/plans/STATUS.md docs/plans/ROADMAP.md
git commit -m "docs(m3): harness + p99 acceptance gate done; M3 open for owner manual fly

Four §6.4 harness components (Wg10FlyCamera/Wg10Profiler/Wg10DiagnosticsOverlay + m3_review
scene) + m3_accept_check (p99=<measured>ms over a scripted ~1000 m/s flight, no-black,
never-stall) landed. m3 suite 5 checks fail=0; fast/gpu unchanged; cargo green. Per §7.3 the
automated gate is the regression catcher — M3 milestone stays OPEN with one box: the owner's
manual fly of m3_review.tscn (the final authority).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage:** §1 scope (4 components + review scene + accept gate; YAGNI deferrals) → Tasks 1-5. §2 components (each §6.4 self-contained/narrow/config) → Tasks 1-4. §3 accept gate (scripted ~1000 m/s, same update loop, vsync off, p99<6ms + no-black + never-stall, prints numbers) → Task 5. §5 done + the acceptance split (slice done on green; milestone open for manual fly) → Task 6. ✓

**2. Placeholder scan:** The profiler `gpu_ms()` has an explicit "replace the `*0.0+` placeholder hack with the real monitor read" instruction — that's a flagged-for-implementer real-API-verify, not a silent placeholder; the gate uses total frame time, not gpu_ms, so it's non-blocking. The gate's framing/threshold constants carry explicit "validate windowed, don't weaken to vacuity, a fail is a real finding" instructions. No TBD/"handle edge cases". ✓ (Fix the gpu_ms placeholder during Task 1.)

**3. Type consistency:** `Wg10Profiler` methods `p99_ms/mean_ms/max_ms/fps/gpu_ms/reset/push` consistent across Task 1 (def), Task 3 (overlay reads `fps`/`p99_ms`/`mean_ms`/`max_ms`), Task 5 (gate reads `p99_ms`/`mean_ms`/`max_ms`, calls `push`). `Wg10FlyCamera.get_velocity()` + `global_position` consistent Task 2 ↔ Task 4 (review reads them). `Wg10DiagnosticsOverlay.bind_sources(profiler, view)` consistent Task 3 ↔ Task 4. `Wg10TerrainView.configure(...)`/`update(...)`/`stats()` match the real Rust signatures. All harness constants match `m3_view_check`. ✓

**Refinement note:** the gate's per-frame wall-time measurement (Time.get_ticks_usec around update+force_draw) is the pragmatic windowed approach; the spec's "total frame delta" ideal is `_process` delta, but the gate steps frames manually (await process_frame) so it times the step explicitly. Both measure the render-bearing work; the controller validates the number is sane (spec §6 caveat). This is the honest windowed-measurement reality, recorded.
