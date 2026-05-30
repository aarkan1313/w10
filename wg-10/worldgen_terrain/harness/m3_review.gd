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
# 5 levels: the clipmap reaches 1.5 * BASE_SPAN * 2^(NUM_LEVELS-1) from the camera. At 3 levels
# that was only ~49 km while the camera saw ~524 km -> ground loaded as you approached then
# unloaded behind ("loads then unloads"). 5 levels reaches ~197 km; the far plane is matched to it
# below so you never SEE the loaded edge. (Coarse distant rings are cheap: big pages, 9 each.)
const NUM_LEVELS := 5
const BASE_SPAN := 8192.0
const GRID_RES := 64
const RADIUS_PAGES := 1
const LEAD_SECONDS := 0.5
const MAX_PER_FRAME := 4
const CAPACITY := 96       # 5 levels x 9 = 45 + stream-ahead + parent-fetch headroom
const MORPH_REGION := 0.15
const HEIGHT_SCALE := 0.35
const RELIEF_REF := 2000.0

var _view: Object
var _camera: Camera3D
var _rings: Object               # debug: poll tile states for the flip log
var _dbg_label: Label
var _prev_states: PackedInt64Array = PackedInt64Array()
var _flip_log: Array[String] = []
var _cull_disabled := false
var _morph_view := false
var _frame := 0

func _ready() -> void:
	if RenderingServer.get_rendering_device() == null:
		push_error("m3_review: no RenderingDevice (run windowed)"); return
	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var err: String = str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if err != "":
		push_error("m3_review: pool configure failed: %s" % err); return
	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_SECONDS, MAX_PER_FRAME)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	add_child(rings)
	_rings = rings
	_view = ClassDB.instantiate("Wg10TerrainView")
	_view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF, LEAD_SECONDS)

	# Loaded extent = the coarsest 3x3's half-width from the camera. Match the far plane + fog to it
	# so the horizon fades to sky BEFORE the loaded edge — you never see ground load/unload at a
	# visible boundary. (RADIUS_PAGES + 0.5) * coarsest_span.
	var coarsest_span := BASE_SPAN * pow(2.0, NUM_LEVELS - 1)
	var loaded_edge := (RADIUS_PAGES + 0.5) * coarsest_span    # metres from camera to the outer edge

	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.45, 0.62, 0.85)
	# Distance fog fading to the sky color, ending just inside the loaded edge so the horizon is
	# sky, not a hard terrain cliff. depth_end ~ 85% of the loaded edge.
	env.fog_enabled = true
	env.fog_mode = Environment.FOG_MODE_DEPTH
	env.fog_depth_begin = loaded_edge * 0.45
	env.fog_depth_end = loaded_edge * 0.85
	env.fog_light_color = Color(0.45, 0.62, 0.85)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	add_child(light)

	_camera = load(FLY_CAMERA).new()
	_camera.environment = env
	# Far plane at the loaded edge: nothing renders past where terrain exists, and fog has already
	# faded the horizon to sky before that, so the edge is never visible.
	_camera.far = float(loaded_edge)
	add_child(_camera)
	# Set position AFTER the node is in the tree (global_position pre-tree warns + no-ops).
	_camera.global_position = Vector3(0.0, 1200.0, 0.0)

	var profiler: Node = load(PROFILER).new()
	add_child(profiler)
	var overlay: CanvasLayer = load(OVERLAY).new()
	add_child(overlay)
	overlay.call("bind_sources", profiler, _view)

	# DEBUG: register the global shader debug-mode param (used by ring_displace's wg_dbg_mode).
	# Press M to toggle the MORPH-BAND HEATMAP: blue=fine, green=blend, red=coarse. A hard LOD pop
	# shows as a blue->red edge with NO green; a proper geomorph shows a wide green gradient.
	RenderingServer.global_shader_parameter_add("wg_dbg_mode", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, 0.0)

	# DEBUG flip-log HUD (bottom-left). Press K to toggle cull-disable (A/B test the AABB-cull
	# theory): if a vanishing chunk STOPS with culling off, it's frustum culling; if it persists,
	# it's the bind/visibility path. The log names the last tiles that flipped HIDE/SHOW/REPAGE.
	var dbg_layer := CanvasLayer.new()
	add_child(dbg_layer)
	_dbg_label = Label.new()
	_dbg_label.position = Vector2(12, 360)
	_dbg_label.add_theme_color_override("font_color", Color.YELLOW)
	dbg_layer.add_child(_dbg_label)

func _input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and event.keycode == KEY_K:
		_cull_disabled = not _cull_disabled
		if _rings != null:
			_rings.call("debug_disable_culling", _cull_disabled)
	elif event is InputEventKey and event.pressed and event.keycode == KEY_M:
		_morph_view = not _morph_view
		RenderingServer.global_shader_parameter_set("wg_dbg_mode", 1.0 if _morph_view else 0.0)

func _process(_delta: float) -> void:
	if _view == null or _camera == null:
		return
	_frame += 1
	var p: Vector3 = _camera.global_position
	var v: Vector3 = _camera.call("get_velocity")
	_view.call("update", p.x, p.z, v.x, v.z)

	# poll tile states AFTER update; log any visibility/page flip so a vanish names its tile.
	if _rings != null:
		var states: PackedInt64Array = _rings.call("debug_tile_states")
		if _prev_states.size() == states.size():
			var t := 0
			while t * 3 + 2 < states.size():
				var vis := states[t * 3]
				var ox := states[t * 3 + 1]
				var oz := states[t * 3 + 2]
				var pv := _prev_states[t * 3]
				var pox := _prev_states[t * 3 + 1]
				var poz := _prev_states[t * 3 + 2]
				var level := t / 9
				var slot := t % 9
				if vis != pv:
					_flip_log.append("f%d L%d s%d %s" % [_frame, level, slot, "SHOW" if vis == 1 else "HIDE"])
				elif vis == 1 and (ox != pox or oz != poz):
					_flip_log.append("f%d L%d s%d REPAGE" % [_frame, level, slot])
				t += 1
		_prev_states = states
		while _flip_log.size() > 8:
			_flip_log.pop_front()
		_dbg_label.text = "cull %s (K toggles)\n%s" % [
			"DISABLED" if _cull_disabled else "on", "\n".join(_flip_log)]
