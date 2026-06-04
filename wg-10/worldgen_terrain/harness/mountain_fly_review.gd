extends Node3D

# Mountain/live-world fly review scene: the thin assembly point the OWNER flies to manually review
# runtime biome producers through the SAME proven M3 pipeline (pool -> streamer -> rings ->
# Wg10TerrainView -> fly camera -> profiler -> overlay). COPY of m3_review.gd, with producer modes
# for grammar-routed WORLD, single MOUNTAIN, and LEGACY kernel atlas.
#
# LAUNCH: run this scene (windowed). Fly with WASD (+ Shift to sprint to ~1000s m/s), mouse to
# look, Space/C up/down, ESC to release the mouse. Watch the HUD: fps, frame p99, resident pages.
#
# KEYS: K toggle cull-disable, M morph-band heatmap, O morph on/off, N detail on/off, P toggles
#       runtime scale preset, and B cycles WORLD -> MOUNTAIN -> LEGACY. The streamer/view keep the
#       same pool ref; on toggle we free_all + reconfigure live. Starts in WORLD mode.

# --- BIOME (mountain GPU producer) constants — replaces the legacy PACK/GLSL trio ---
const PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const MOUNTAIN := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const APRON_PX := 160
const MOUNTAIN_PRESET_NETWORK := 0
const MOUNTAIN_PRESET_CLOSE_DEBUG := 1
const FEATURE_SPAN_NETWORK_M := 90000.0     # accepted static network-chunk baseline scale
const FEATURE_SPAN_CLOSE_DEBUG_M := 3500.0  # close-up page/debug scale; not the accepted baseline
const FLOW_ITERS := 192        # measured production convergence count (memory: 576^2 mountain ~192)
# SCALE-INVARIANCE: first clipmap level (0=finest) baked WITHOUT the drainage carve. A page at
# `level` runs flow_on = level < FLOW_MAX_LEVEL. 2 -> carve on levels 0,1 (near camera), off 2.. .
const FLOW_MAX_LEVEL := 2
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
const MORPH_REGION_ON := 0.15
const MORPH_REGION_OFF := 0.0
const RELIEF_SCALE := 0.25
const RELIEF_REF := 2000.0
const DETAIL_AMP := 350.0    # M5 detail peak (metres). ×RELIEF_SCALE 0.25 = ~88 m effective — chosen
							 # to be clearly VISIBLE at fly scale (60 m → ~21 m was invisible; see STATUS
							 # M5 fly finding). STARTING value for live owner tuning, not a final look.

const MODE_WORLD := 0
const MODE_MOUNTAIN := 1
const MODE_LEGACY := 2

var _view: Object
var _pool: Object                # kept so _exit_tree can free its page-texture RIDs (B1)
var _streamer: Object
var _camera: Camera3D
var _rings: Object               # debug: poll tile states for the flip log
var _dbg_label: Label
var _prev_states: PackedInt64Array = PackedInt64Array()
var _flip_log: Array[String] = []
var _cull_disabled := false
var _morph_view := false
var _morph_enabled := false # Runtime biome modes start with morph OFF: fine/coarse surfaces still differ visibly.
var _detail_on := false   # start OFF so the FIRST N press turns detail ON (matches the m5 gate's 0.0
						  # baseline + the operator's "N enables detail" expectation; the scene used to
						  # start ON so N first turned it OFF — see STATUS M5 fly finding).
var _producer_mode := MODE_WORLD   # B cycles WORLD -> MOUNTAIN -> LEGACY.
var _mountain_preset := MOUNTAIN_PRESET_NETWORK
var _frame := 0

func _ready() -> void:
	if RenderingServer.get_rendering_device() == null:
		push_error("mountain_fly_review: no RenderingDevice (run windowed)"); return

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	_pool = pool   # keep a reference for deterministic teardown in _exit_tree (B1)
	var err: String = _configure_active_producer(pool)
	if err != "":
		push_error("mountain_fly_review: pool configure failed: %s" % err); return
	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	_streamer = streamer
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_SECONDS, MAX_PER_FRAME)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	add_child(rings)
	_rings = rings
	_view = ClassDB.instantiate("Wg10TerrainView")
	_reconfigure_view()

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
	_print_biome_state()

# Configure the pool for the grammar-routed WORLD producer path.
func _configure_world(pool: Object) -> String:
	return str(pool.call("configure_biome_world",
		ProjectSettings.globalize_path(PACK_RES_DIR),
		PACK_FILE,
		CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, _feature_span_m(), FLOW_ITERS, _relief_m, FLOW_MAX_LEVEL, SEED))

# Configure the pool for the single MOUNTAIN GPU producer path.
func _configure_biome(pool: Object) -> String:
	return str(pool.call("configure_biome",
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE),
		ProjectSettings.globalize_path(MOUNTAIN),
		CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, _feature_span_m(), FLOW_ITERS, _relief_m, FLOW_MAX_LEVEL, SEED))

# Configure the pool for the LEGACY (dem_v1 kernel atlas) path — the A/B comparison.
func _configure_legacy(pool: Object) -> String:
	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	return str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))

func _configure_active_producer(pool: Object) -> String:
	if _producer_mode == MODE_WORLD:
		return _configure_world(pool)
	if _producer_mode == MODE_MOUNTAIN:
		return _configure_biome(pool)
	return _configure_legacy(pool)

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
	elif event is InputEventKey and event.pressed and event.keycode == KEY_O:
		_morph_enabled = not _morph_enabled
		_reconfigure_view()
		print("[fly] morph_region=%s" % str(_current_morph_region()))
	elif event is InputEventKey and event.pressed and event.keycode == KEY_N:
		_detail_on = not _detail_on
		RenderingServer.global_shader_parameter_set("wg_detail_amp", DETAIL_AMP if _detail_on else 0.0)
	elif event is InputEventKey and event.pressed and event.keycode == KEY_P:
		_toggle_mountain_preset()
	elif event is InputEventKey and event.pressed and event.keycode == KEY_B:
		_cycle_producer_mode()
	elif event is InputEventKey and event.pressed and event.keycode == KEY_R:
		_set_relief(_relief_m * 1.25)   # taller mountains
	elif event is InputEventKey and event.pressed and event.keycode == KEY_F:
		_set_relief(_relief_m * 0.8)    # flatter

# Live VERTICAL-SCALE knob (R/F): change the biome relief (metres) and reconfigure the pool so new
# pages bake at the new scale. Only meaningful in WORLD/MOUNTAIN mode (legacy ignores it).
func _set_relief(v: float) -> void:
	_relief_m = clampf(v, 50.0, 20000.0)
	print("[fly] relief_m=%.0f m" % _relief_m)
	_rebuild_runtime_pages("relief")

func _feature_span_m() -> float:
	if _mountain_preset == MOUNTAIN_PRESET_CLOSE_DEBUG:
		return FEATURE_SPAN_CLOSE_DEBUG_M
	return FEATURE_SPAN_NETWORK_M

func _mountain_preset_label() -> String:
	if _mountain_preset == MOUNTAIN_PRESET_CLOSE_DEBUG:
		return "close_debug"
	return "network_ref"

func _toggle_mountain_preset() -> void:
	if _mountain_preset == MOUNTAIN_PRESET_NETWORK:
		_mountain_preset = MOUNTAIN_PRESET_CLOSE_DEBUG
	else:
		_mountain_preset = MOUNTAIN_PRESET_NETWORK
	_rebuild_runtime_pages("preset")
	_print_biome_state()

func _rebuild_runtime_pages(reason: String) -> void:
	if _pool == null or _producer_mode == MODE_LEGACY:
		return
	# Detach ring materials from the page textures before free_all (else the next draws flood
	# "Texture binding 1 not valid" against the freed RIDs until pages re-stream).
	if _rings != null:
		_rings.call("unbind_all")
	_pool.call("free_all")
	var err := _configure_active_producer(_pool)
	if err != "":
		push_error("mountain_fly_review: %s reconfigure failed: %s" % [reason, err])
		return
	_prev_states = PackedInt64Array()

func _producer_label() -> String:
	if _producer_mode == MODE_WORLD:
		return "WORLD"
	if _producer_mode == MODE_MOUNTAIN:
		return "MOUNTAIN"
	return "LEGACY"

func _print_biome_state() -> void:
	if _pool == null:
		return
	print("[fly] mode=%s runtime=%s biome_path=%s preset=%s feature_span_m=%.0f relief_m=%.0f" % [
		_producer_label(),
		str(_pool.call("biome_runtime_mode")),
		str(_pool.call("uses_biome_path")),
		_mountain_preset_label(),
		_feature_span_m(),
		_relief_m,
	])

func _current_morph_region() -> float:
	return MORPH_REGION_ON if _morph_enabled else MORPH_REGION_OFF

func _reconfigure_view() -> void:
	if _view == null or _pool == null or _streamer == null or _rings == null:
		return
	_view.call("configure", _pool, _streamer, _rings, NUM_LEVELS, BASE_SPAN, RELIEF_SCALE, _current_morph_region(), RELIEF_REF, LEAD_SECONDS)

# Live producer toggle (B): free_all + reconfigure the SAME pool object between grammar-routed
# WORLD, single MOUNTAIN, and LEGACY dem_v1 kernel atlas. The streamer/view hold the same pool ref and keep
# working — next update re-acquires pages from the freshly-configured pool. Prints the new state.
func _cycle_producer_mode() -> void:
	if _pool == null:
		return
	if _producer_mode == MODE_WORLD:
		_producer_mode = MODE_MOUNTAIN
	elif _producer_mode == MODE_MOUNTAIN:
		_producer_mode = MODE_LEGACY
	else:
		_producer_mode = MODE_WORLD
	# Detach ring materials from the page textures before free_all (else the next draws flood
	# "Texture binding 1 not valid" against the freed RIDs until pages re-stream after reconfigure).
	if _rings != null:
		_rings.call("unbind_all")
	_pool.call("free_all")
	var err := _configure_active_producer(_pool)
	if _producer_mode == MODE_LEGACY:
		_morph_enabled = true
	else:
		_morph_enabled = false
	if err != "":
		push_error("mountain_fly_review: reconfigure failed: %s" % err)
		return
	_reconfigure_view()
	# Reset the flip-log baseline so the post-reconfigure repage churn doesn't spam the HUD.
	_prev_states = PackedInt64Array()
	_print_biome_state()

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
		_dbg_label.text = "mode %s (B cycles) | preset %s %.0fkm (P toggles) | cull %s (K toggles) | morph %s (O toggles)\n%s" % [
			_producer_label(),
			_mountain_preset_label(),
			_feature_span_m() / 1000.0,
			"DISABLED" if _cull_disabled else "on",
			"on" if _morph_enabled else "off",
			"\n".join(_flip_log)]
