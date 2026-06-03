extends Node3D

# Mountain live-fly review scene (Task 6): the thin assembly point the OWNER flies to manually
# accept the MOUNTAIN biome streaming through the SAME proven M3 pipeline (pool -> streamer ->
# rings -> Wg10TerrainView -> fly camera -> profiler -> overlay). COPY of m3_review.gd, with the
# pool configured via the GPU biome producer (configure_biome) instead of the legacy kernel atlas.
#
# LAUNCH: run this scene (windowed). Fly with WASD (+ Shift to sprint to ~1000s m/s), mouse to
# look, Space/C up/down, ESC to release the mouse. Watch the HUD: fps, frame p99, resident pages.
#
# KEYS: K toggle cull-disable, M morph-band heatmap, N detail on/off (as m3_review), and
#       B toggles A/B between the BIOME mountain producer and the LEGACY dem_v1 kernel atlas (the
#       streamer/view keep the same pool ref; on toggle we free_all + reconfigure live). Starts
#       in BIOME mode. Prints "[fly] biome_path=<true/false>" on each toggle.

# --- BIOME (mountain GPU producer) constants — replaces the legacy PACK/GLSL trio ---
const PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const MOUNTAIN := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const APRON_PX := 160
const FEATURE_SPAN_M := 3500.0  # scale-contract on-foot mountain. 90000 (a 90km massif) shows only a
								# gentle sliver per 8km page -> reads flat; 3500 = whole mountains per
								# few pages = a real range (capture-verified 2026-06-03).
const FLOW_ITERS := 192        # measured production convergence count (memory: 576^2 mountain ~192)
# VERTICAL SCALE KNOB (metres): the biome page multiplies its NORMALIZED recipe height (~[-3.2,2.2])
# by this before the texture write, so the render (VERTEX.y = h*relief_scale 0.25) sees metres. Default
# 1000 -> ~1350m effective range. Live-tunable: R raises, F lowers (x1.25 / x0.8), reconfigures the pool.
const RELIEF_M_DEFAULT := 1000.0
var _relief_m := RELIEF_M_DEFAULT

# --- LEGACY (dem_v1 kernel atlas) — for the B A/B toggle back to the old kernel path ---
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
const RELIEF_SCALE := 0.25
const RELIEF_REF := 2000.0
const DETAIL_AMP := 350.0    # M5 detail peak (metres). ×RELIEF_SCALE 0.25 = ~88 m effective — chosen
							 # to be clearly VISIBLE at fly scale (60 m → ~21 m was invisible; see STATUS
							 # M5 fly finding). STARTING value for live owner tuning, not a final look.

var _view: Object
var _pool: Object                # kept so _exit_tree can free its page-texture RIDs (B1)
var _camera: Camera3D
var _rings: Object               # debug: poll tile states for the flip log
var _dbg_label: Label
var _prev_states: PackedInt64Array = PackedInt64Array()
var _flip_log: Array[String] = []
var _cull_disabled := false
var _morph_view := false
var _detail_on := false   # start OFF so the FIRST N press turns detail ON (matches the m5 gate's 0.0
						  # baseline + the operator's "N enables detail" expectation; the scene used to
						  # start ON so N first turned it OFF — see STATUS M5 fly finding).
var _biome_mode := true   # start in BIOME (mountain GPU producer); B toggles to legacy and back.
var _frame := 0

func _ready() -> void:
	if RenderingServer.get_rendering_device() == null:
		push_error("mountain_fly_review: no RenderingDevice (run windowed)"); return

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	_pool = pool   # keep a reference for deterministic teardown in _exit_tree (B1)
	var err: String = _configure_biome(pool)
	if err != "":
		push_error("mountain_fly_review: pool configure_biome failed: %s" % err); return
	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_SECONDS, MAX_PER_FRAME)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	add_child(rings)
	_rings = rings
	_view = ClassDB.instantiate("Wg10TerrainView")
	_view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, RELIEF_SCALE, MORPH_REGION, RELIEF_REF, LEAD_SECONDS)

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
	# M5: global detail amplitude (read by ring_displace's wg_detail_amp). Register at 0.0 (detail OFF
	# at load, matching _detail_on=false + the m5 gate convention); N toggles to DETAIL_AMP and back.
	RenderingServer.global_shader_parameter_add("wg_detail_amp", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, 0.0)

	# DEBUG flip-log HUD (bottom-left). Press K to toggle cull-disable (A/B test the AABB-cull
	# theory): if a vanishing chunk STOPS with culling off, it's frustum culling; if it persists,
	# it's the bind/visibility path. The log names the last tiles that flipped HIDE/SHOW/REPAGE.
	var dbg_layer := CanvasLayer.new()
	add_child(dbg_layer)
	_dbg_label = Label.new()
	_dbg_label.position = Vector2(12, 360)
	_dbg_label.add_theme_color_override("font_color", Color.YELLOW)
	dbg_layer.add_child(_dbg_label)
	print("[fly] biome_path=%s" % str(_pool.call("uses_biome_path")))

# Configure the pool for the BIOME (mountain GPU producer) path.
func _configure_biome(pool: Object) -> String:
	return str(pool.call("configure_biome",
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE),
		ProjectSettings.globalize_path(MOUNTAIN),
		CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, FEATURE_SPAN_M, FLOW_ITERS, _relief_m, SEED))

# Configure the pool for the LEGACY (dem_v1 kernel atlas) path — the A/B comparison.
func _configure_legacy(pool: Object) -> String:
	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	return str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))

func _exit_tree() -> void:
	# Free the pool's page-texture RIDs on scene teardown (B1). Wg10PagePool also self-frees via a
	# Rust Drop impl when its last reference drops, so this is deterministic belt-and-suspenders —
	# it releases the GPU RIDs at scene-exit rather than at GC time.
	# Detach the ring materials from those page textures FIRST so the post-free draw doesn't rebuild
	# a tile material's uniform set against a freed page RID ("Texture binding 1 not valid" flood).
	if _rings != null:
		_rings.call("unbind_all")
	if _pool != null:
		_pool.call("free_all")
		_pool = null

func _input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and event.keycode == KEY_K:
		_cull_disabled = not _cull_disabled
		if _rings != null:
			_rings.call("debug_disable_culling", _cull_disabled)
	elif event is InputEventKey and event.pressed and event.keycode == KEY_M:
		_morph_view = not _morph_view
		RenderingServer.global_shader_parameter_set("wg_dbg_mode", 1.0 if _morph_view else 0.0)
	elif event is InputEventKey and event.pressed and event.keycode == KEY_N:
		_detail_on = not _detail_on
		RenderingServer.global_shader_parameter_set("wg_detail_amp", DETAIL_AMP if _detail_on else 0.0)
	elif event is InputEventKey and event.pressed and event.keycode == KEY_B:
		_toggle_biome()
	elif event is InputEventKey and event.pressed and event.keycode == KEY_R:
		_set_relief(_relief_m * 1.25)   # taller mountains
	elif event is InputEventKey and event.pressed and event.keycode == KEY_F:
		_set_relief(_relief_m * 0.8)    # flatter

# Live VERTICAL-SCALE knob (R/F): change the biome relief (metres) and reconfigure the pool so new
# pages bake at the new scale. Only meaningful in BIOME mode (legacy ignores it). Prints the new value.
func _set_relief(v: float) -> void:
	_relief_m = clampf(v, 50.0, 20000.0)
	print("[fly] relief_m=%.0f m" % _relief_m)
	if _pool != null and _biome_mode:
		# Detach ring materials from the page textures before free_all (else the next draws flood
		# "Texture binding 1 not valid" against the freed RIDs until pages re-stream).
		if _rings != null:
			_rings.call("unbind_all")
		_pool.call("free_all")
		var err := _configure_biome(_pool)
		if err != "":
			push_error("mountain_fly_review: relief reconfigure failed: %s" % err)
		_prev_states = PackedInt64Array()

# A/B live toggle (B): free_all + reconfigure the SAME pool object between the biome mountain
# producer and the legacy dem_v1 kernel atlas. The streamer/view hold the same pool ref and keep
# working — next update re-acquires pages from the freshly-configured pool. Prints the new state.
func _toggle_biome() -> void:
	if _pool == null:
		return
	_biome_mode = not _biome_mode
	# Detach ring materials from the page textures before free_all (else the next draws flood
	# "Texture binding 1 not valid" against the freed RIDs until pages re-stream after reconfigure).
	if _rings != null:
		_rings.call("unbind_all")
	_pool.call("free_all")
	var err: String
	if _biome_mode:
		err = _configure_biome(_pool)
	else:
		err = _configure_legacy(_pool)
	if err != "":
		push_error("mountain_fly_review: reconfigure failed: %s" % err)
		return
	# Reset the flip-log baseline so the post-reconfigure repage churn doesn't spam the HUD.
	_prev_states = PackedInt64Array()
	print("[fly] biome_path=%s" % str(_pool.call("uses_biome_path")))

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
		_dbg_label.text = "mode %s (B toggles) | cull %s (K toggles)\n%s" % [
			"BIOME" if _biome_mode else "LEGACY",
			"DISABLED" if _cull_disabled else "on", "\n".join(_flip_log)]
