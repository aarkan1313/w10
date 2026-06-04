extends Node3D

# Mountain/live-world fly review scene: the thin assembly point the OWNER flies to manually review
# runtime biome producers through the SAME proven M3 pipeline (pool -> streamer -> rings ->
# Wg10TerrainView -> fly camera -> profiler -> overlay). COPY of m3_review.gd, with producer modes
# for grammar-routed WORLD, single MOUNTAIN, accepted REFERENCE payload, and LEGACY kernel atlas.
#
# LAUNCH: run this scene (windowed). Fly with WASD (+ Shift to sprint to ~1000s m/s), mouse to
# look, Space/C up/down, ESC to release the mouse. Watch the HUD: fps, frame p99, resident pages.
#
# KEYS: K toggle cull-disable, M cycles normal/morph/route debug, O morph on/off, N detail on/off,
#       P toggles runtime scale preset, B cycles modes, and 1/2/3/4 jump directly to
#       REFERENCE/MOUNTAIN/WORLD/LEGACY. The streamer/view keep the same pool ref; on toggle we
#       free_all + reconfigure live. Starts in MOUNTAIN mode so the mountain review scene reviews
#       mountain content first; REFERENCE remains the accepted static-payload baseline.

# Producer modes, scale presets, relief, and pool configure calls live in a helper.
const PRODUCERS := "res://worldgen_terrain/harness/mountain_fly_producers.gd"
const RUNTIME_CONFIG := "res://worldgen_terrain/harness/mountain_fly_runtime_config.gd"

const PROFILER := "res://worldgen_terrain/harness/profiler.gd"
const FLY_CAMERA := "res://worldgen_terrain/harness/fly_camera.gd"
const OVERLAY := "res://worldgen_terrain/harness/diagnostics_overlay.gd"
# Runtime renderer constants live in mountain_fly_runtime_config.gd.

var _view: Object
var _pool: Object                # kept so _exit_tree can free its page-texture RIDs (B1)
var _streamer: Object
var _producer: Object
var _runtime: Object
var _camera: Camera3D
var _rings: Object               # debug: poll tile states for the flip log
var _dbg_label: Label
var _prev_states: PackedInt64Array = PackedInt64Array()
var _flip_log: Array[String] = []
var _cull_disabled := false
var _debug_mode := 0
var _morph_enabled := false
var _detail_on := false
var _frame := 0
var _last_config_error := ""

func _ready() -> void:
	if RenderingServer.get_rendering_device() == null:
		push_error("mountain_fly_review: no RenderingDevice (run windowed)"); return

	_runtime = load(RUNTIME_CONFIG).new()
	_morph_enabled = bool(_runtime.default_morph_enabled())
	_detail_on = bool(_runtime.default_detail_enabled())

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	_pool = pool   # keep a reference for deterministic teardown in _exit_tree (B1)
	_producer = load(PRODUCERS).new()
	var err: String = _configure_active_producer(pool)
	_last_config_error = err
	if err != "":
		push_error("mountain_fly_review: pool configure failed: %s" % err); return
	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	_streamer = streamer
	_runtime.configure_streamer(streamer, pool)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	_runtime.configure_rings(rings)
	add_child(rings)
	_rings = rings
	_view = ClassDB.instantiate("Wg10TerrainView")
	_reconfigure_view()

	# Match far plane and fog to the shared loaded extent.
	var loaded_edge := float(_runtime.loaded_edge_m())

	var env := Environment.new()
	_runtime.configure_review_environment(env)
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

	_runtime.register_shader_globals(_detail_on)

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

func _configure_active_producer(pool: Object) -> String:
	if _producer == null:
		return "producer helper not loaded"
	return _producer.configure(pool)

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
		_debug_mode = (_debug_mode + 1) % 3
		_runtime.set_debug_mode(_debug_mode)
	elif event is InputEventKey and event.pressed and event.keycode == KEY_O:
		_morph_enabled = not _morph_enabled
		_reconfigure_view()
		print("[fly] morph_region=%s" % str(_current_morph_region()))
	elif event is InputEventKey and event.pressed and event.keycode == KEY_N:
		_detail_on = not _detail_on
		_runtime.set_detail_enabled(_detail_on)
	elif event is InputEventKey and event.pressed and event.keycode == KEY_P:
		_toggle_mountain_preset()
	elif event is InputEventKey and event.pressed and event.keycode == KEY_B:
		_cycle_producer_mode()
	elif event is InputEventKey and event.pressed and event.keycode == KEY_1:
		_set_producer_mode("REFERENCE")
	elif event is InputEventKey and event.pressed and event.keycode == KEY_2:
		_set_producer_mode("MOUNTAIN")
	elif event is InputEventKey and event.pressed and event.keycode == KEY_3:
		_set_producer_mode("WORLD")
	elif event is InputEventKey and event.pressed and event.keycode == KEY_4:
		_set_producer_mode("LEGACY")
	elif event is InputEventKey and event.pressed and event.keycode == KEY_R:
		_set_relief(_relief_m() * 1.25)   # taller mountains
	elif event is InputEventKey and event.pressed and event.keycode == KEY_F:
		_set_relief(_relief_m() * 0.8)    # flatter

# Live VERTICAL-SCALE knob (R/F): change the biome relief (metres) and reconfigure the pool so new
# pages bake at the new scale. Only meaningful in WORLD/MOUNTAIN mode (legacy ignores it).
func _set_relief(v: float) -> void:
	if _producer == null:
		return
	_producer.set_relief_m(v)
	print("[fly] relief_m=%.0f m" % _relief_m())
	_rebuild_runtime_pages("relief")

func _relief_m() -> float:
	if _producer == null:
		return 0.0
	return float(_producer.relief_m())

func _feature_span_m() -> float:
	if _producer == null:
		return 0.0
	return float(_producer.feature_span_m())

func _mountain_preset_label() -> String:
	if _producer == null:
		return "unknown"
	return str(_producer.preset_label())

func _toggle_mountain_preset() -> void:
	if _producer == null:
		return
	_producer.toggle_preset()
	_rebuild_runtime_pages("preset")
	_print_biome_state()

func _rebuild_runtime_pages(reason: String) -> void:
	if _pool == null or _producer == null or bool(_producer.is_legacy()):
		return
	# Detach ring materials from the page textures before free_all (else the next draws flood
	# "Texture binding 1 not valid" against the freed RIDs until pages re-stream).
	if _rings != null:
		_rings.call("unbind_all")
	_pool.call("free_all")
	var err := _configure_active_producer(_pool)
	_last_config_error = err
	if err != "":
		push_error("mountain_fly_review: %s reconfigure failed: %s" % [reason, err])
		return
	_prev_states = PackedInt64Array()

func _producer_label() -> String:
	if _producer == null:
		return "UNKNOWN"
	return str(_producer.mode_label())

func _print_biome_state() -> void:
	if _pool == null:
		return
	print("[fly] mode=%s runtime=%s biome_path=%s preset=%s feature_span_m=%.0f relief_m=%.0f" % [
		_producer_label(),
		str(_pool.call("biome_runtime_mode")),
		str(_pool.call("uses_biome_path")),
		_mountain_preset_label(),
		_feature_span_m(),
		_relief_m(),
	])

func _world_route_summary(p: Vector3, _v: Vector3) -> String:
	if _producer == null or not bool(_producer.is_world()) or _pool == null or _streamer == null:
		return ""
	var parts: Array[String] = []
	for level in range(int(_runtime.num_levels())):
		var span: float = float(_runtime.base_span_m()) * pow(2.0, level)
		var ox: float = floor(p.x / span) * span
		var oz: float = floor(p.z / span) * span
		var name := str(_pool.call("debug_world_biome_for_page", level, ox, oz))
		parts.append("L%d:%s" % [level, name])
	return "routes %s" % " ".join(parts)

func _current_morph_region() -> float:
	if _runtime == null:
		return 0.0
	return float(_runtime.morph_region(_morph_enabled))

func _debug_mode_label() -> String:
	if _debug_mode == 1:
		return "morph"
	if _debug_mode == 2:
		return "route"
	return "material"

func _reconfigure_view() -> void:
	if _view == null or _pool == null or _streamer == null or _rings == null or _runtime == null:
		return
	var relief_scale := float(_runtime.default_relief_scale())
	if _producer != null:
		relief_scale = float(_producer.view_relief_scale(relief_scale))
	_runtime.configure_view(_view, _pool, _streamer, _rings, _morph_enabled, relief_scale)

func _apply_mode_reconfigure(reason: String) -> void:
	if _pool == null or _producer == null:
		return
	# Detach ring materials from the page textures before free_all (else the next draws flood
	# "Texture binding 1 not valid" against the freed RIDs until pages re-stream after reconfigure).
	if _rings != null:
		_rings.call("unbind_all")
	_pool.call("free_all")
	var err := _configure_active_producer(_pool)
	_last_config_error = err
	if bool(_producer.is_legacy()):
		_morph_enabled = true
	else:
		_morph_enabled = false
	if err != "":
		push_error("mountain_fly_review: %s reconfigure failed: %s" % [reason, err])
		return
	_reconfigure_view()
	# Reset the flip-log baseline so the post-reconfigure repage churn doesn't spam the HUD.
	_prev_states = PackedInt64Array()
	_print_biome_state()

# Live producer toggle (B): free_all + reconfigure the SAME pool object between single MOUNTAIN,
# accepted REFERENCE payload, LEGACY dem_v1 kernel atlas, and grammar-routed WORLD. The streamer/view hold the same pool ref and keep
# working — next update re-acquires pages from the freshly-configured pool. Prints the new state.
func _cycle_producer_mode() -> void:
	if _pool == null or _producer == null:
		return
	_producer.cycle_mode()
	_apply_mode_reconfigure("mode-cycle")

func _set_producer_mode(label: String) -> void:
	if _pool == null or _producer == null:
		return
	if not bool(_producer.set_mode_label(label)):
		push_error("mountain_fly_review: invalid producer mode %s" % label)
		return
	_apply_mode_reconfigure("mode-%s" % label.to_lower())

func debug_runtime_snapshot() -> Dictionary:
	var stats := {}
	var source_transform := {}
	var runtime_mode := "missing"
	var biome_path := false
	if _pool != null:
		stats = _pool.call("stats")
		source_transform = _pool.call("biome_source_transform")
		runtime_mode = str(_pool.call("biome_runtime_mode"))
		biome_path = bool(_pool.call("uses_biome_path"))

	var mode := "missing"
	var preset := "missing"
	var seed := -1
	var feature_span_m := 0.0
	var relief_m := 0.0
	var view_relief_scale := 0.0
	var loaded_edge_m := 0.0
	var is_world := false
	var is_legacy := false
	if _runtime != null:
		loaded_edge_m = float(_runtime.loaded_edge_m())
	if _producer != null:
		mode = str(_producer.mode_label())
		preset = str(_producer.preset_label())
		seed = int(_producer.runtime_seed())
		feature_span_m = float(_producer.feature_span_m())
		relief_m = float(_producer.relief_m())
		var default_relief_scale := float(_runtime.default_relief_scale()) if _runtime != null else 0.25
		view_relief_scale = float(_producer.view_relief_scale(default_relief_scale))
		is_world = bool(_producer.is_world())
		is_legacy = bool(_producer.is_legacy())

	return {
		"last_config_error": _last_config_error,
		"has_pool": _pool != null,
		"has_producer": _producer != null,
		"has_runtime": _runtime != null,
		"has_view": _view != null,
		"has_streamer": _streamer != null,
		"has_rings": _rings != null,
		"has_camera": _camera != null,
		"runtime_mode": runtime_mode,
		"biome_path": biome_path,
		"stats": stats,
		"source_transform": source_transform,
		"mode": mode,
		"preset": preset,
		"seed": seed,
		"feature_span_m": feature_span_m,
		"relief_m": relief_m,
		"view_relief_scale": view_relief_scale,
		"loaded_edge_m": loaded_edge_m,
		"is_world": is_world,
		"is_legacy": is_legacy,
		"morph_enabled": _morph_enabled,
		"detail_on": _detail_on,
	}

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
		var route_summary := _world_route_summary(p, v)
		var route_line := "%s\n" % route_summary if route_summary != "" else ""
		_dbg_label.text = "mode %s (1 ref 2 mountain 3 world 4 legacy | B cycles) | preset %s %.0fkm (P toggles) | debug %s (M cycles) | cull %s (K toggles) | morph %s (O toggles)\n%s%s" % [
			_producer_label(),
			_mountain_preset_label(),
			_feature_span_m() / 1000.0,
			_debug_mode_label(),
			"DISABLED" if _cull_disabled else "on",
			"on" if _morph_enabled else "off",
			route_line,
			"\n".join(_flip_log)]
