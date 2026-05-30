extends Node3D

# M3 review scene (§7.4): the thin assembly point the OWNER flies for manual acceptance.
# Instantiates + configures pool/streamer/rings/Wg10TerrainView, adds a Wg10FlyCamera, a
# Wg10Profiler, and a Wg10DiagnosticsOverlay, and each frame feeds the camera's pos/vel to
# view.update. Pure assembly — no component logic here.
#
# LAUNCH: run this scene (windowed). Fly with WASD (+ Shift to sprint to ~1000s m/s), mouse to
# look, Space/C up/down, ESC to release the mouse. Watch the HUD: fps, frame p99 (target
# < 6 ms), resident pages. Confirm no stalls and no black/holes at speed (the M3 acceptance).

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const PROFILER := "res://worldgen_terrain/harness/profiler.gd"
const FLY_CAMERA := "res://worldgen_terrain/harness/fly_camera.gd"
const OVERLAY := "res://worldgen_terrain/harness/diagnostics_overlay.gd"
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
var _camera: Camera3D

func _ready() -> void:
	if RenderingServer.get_rendering_device() == null:
		push_error("m3_review: no RenderingDevice (run windowed)"); return
	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)

	var pool := ClassDB.instantiate("Wg10PagePool")
	var err := str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if err != "":
		push_error("m3_review: pool configure failed: %s" % err); return
	var streamer := ClassDB.instantiate("Wg10Streamer")
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_FRAMES, MAX_PER_FRAME)
	var rings := ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	add_child(rings)
	_view = ClassDB.instantiate("Wg10TerrainView")
	_view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF)

	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.45, 0.62, 0.85)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	add_child(light)

	_camera = load(FLY_CAMERA).new()
	_camera.environment = env
	_camera.far = BASE_SPAN * 32.0
	_camera.global_position = Vector3(0.0, 1200.0, 0.0)
	add_child(_camera)

	var profiler := load(PROFILER).new()
	add_child(profiler)
	var overlay := load(OVERLAY).new()
	add_child(overlay)
	overlay.call("bind_sources", profiler, _view)

func _process(_delta: float) -> void:
	if _view == null or _camera == null:
		return
	var p: Vector3 = _camera.global_position
	var v: Vector3 = _camera.call("get_velocity")
	_view.call("update", p.x, p.z, v.x, v.z)
