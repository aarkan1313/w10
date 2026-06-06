extends Node3D

# ============================================================================
# WG10 UNIFIED FEATURE REVIEW (§ owner sequential visual review)
# ----------------------------------------------------------------------------
# ONE scene to fly + profile EVERY shippable terrain feature, one at a time, in
# order. Each STEP reconfigures the SAME pool/streamer/rings/view with a different
# producer (deterministic teardown between steps, B1), repositions the camera, and
# updates the HUD. Pure assembly: it reuses the existing harness components
# (Wg10FlyCamera, Wg10Profiler, the config helper, Wg10TerrainView/ClipmapRings)
# and the producer-config helpers — it does NOT duplicate them.
#
# LAUNCH: run this scene WINDOWED (RenderingDevice compute is windowed-only).
# CONTROLS:
#   ] / [        next / previous feature step
#   1..9 / 0     jump to step N (0 = step 10)
#   WASD         fly (camera-local), Shift = sprint (~1000s m/s)
#   Space / C    up / down,  mouse = look,  ESC = release mouse
#   M            morph-band heatmap toggle (blue fine / green blend / red coarse)
#   N            M5 detail overlay toggle
#   R            reframe camera to this step's start pose
#   P            print the current step's profiling snapshot to the console
# HUD (top-left): step name + role + acceptance; fps / frame p99 / GPU p99 ms;
#   pool stats (resident/created/recomputed); region-fact stats when active.
# ============================================================================

const CONFIG := "res://worldgen_terrain/harness/mountain_fly_runtime_config.gd"
const PRODUCERS := "res://worldgen_terrain/harness/mountain_fly_producers.gd"
const PROFILER := "res://worldgen_terrain/harness/profiler.gd"
const FLY_CAMERA := "res://worldgen_terrain/harness/fly_camera.gd"

# Region-fact producer shaders/pack (the carved baked-look path; this session's feature).
const RF_PACK := "res://worldgen_terrain/packs/dem_v1/terrain_pack.gate.json"
const RF_PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const RF_MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const RF_FRAGMENT := "res://worldgen_terrain/shaders/biome_mountain.glsl"

# ---- the ordered feature list -------------------------------------------------
# Each step is a dictionary describing WHAT to review and how to set it up. The
# `setup` field names a method on this script (called via Callable) that configures
# the pool for that step and returns "" on success or an error string.
var _steps: Array = [
	{
		"name": "1. Accepted reference baseline",
		"role": "static mountain-network payload streamed through the live clipmap",
		"accept": "accepted visual baseline",
		"setup": "_setup_reference",
		"cam": Vector3(0.0, 1800.0, 0.0),
	},
	{
		"name": "2. Live mountain macro (no flow)",
		"role": "GPU seam-safe macro recipe, flow OFF — pure ridged/warped structure",
		"accept": "procedural macro, not final",
		"setup": "_setup_mountain_macro",
		"cam": Vector3(0.0, 1800.0, 0.0),
	},
	{
		"name": "3. Live mountain + flow (carved drainage)",
		"role": "GPU recipe with flow relaxation — drainage-carved valleys",
		"accept": "procedural with flow, not final",
		"setup": "_setup_mountain_flow",
		"cam": Vector3(0.0, 1800.0, 0.0),
	},
	{
		"name": "4. CARVED BAKED LOOK (region-fact producer)",
		"role": "off-frame super-region bake -> carve+condition -> sliced region facts on screen",
		"accept": "this session's feature: carved look on screen, internal-seam-exact",
		"setup": "_setup_region_fact",
		"cam": Vector3(0.0, 2200.0, 0.0),
	},
	{
		"name": "5. World composition (diagnostic)",
		"role": "grammar-routed route/weight overlay on accepted reference height",
		"accept": "diagnostic, not accepted (compose hitches)",
		"setup": "_setup_world",
		"cam": Vector3(0.0, 1800.0, 0.0),
	},
	{
		"name": "6. Legacy DEM atlas (regression baseline)",
		"role": "the original kernel-atlas renderer — regression reference only",
		"accept": "legacy regression, not accepted",
		"setup": "_setup_legacy",
		"cam": Vector3(0.0, 1800.0, 0.0),
	},
]

var _cfg: Object               # mountain_fly_runtime_config instance
var _pool: Object
var _streamer: Object
var _rings: Object
var _view: Object
var _camera: Camera3D
var _profiler: Node
var _hud: Label
var _hud_accum := 0.0
var _step_idx := 0
var _step_err := ""
var _morph_on := false
var _detail_on := false
# rolling GPU-time p99 (real GPU time, vsync-immune — memory: viewport_get_measured_render_time_gpu)
var _gpu_samples: Array[float] = []
const GPU_WINDOW := 240

func _ready() -> void:
	if RenderingServer.get_rendering_device() == null:
		push_error("feature_review: no RenderingDevice (run WINDOWED, not headless)")
		return
	_cfg = load(CONFIG).new()

	# Renderer scaffold (built ONCE; producers are swapped per step via pool reconfigure).
	_rings = ClassDB.instantiate("Wg10ClipmapRings")
	_cfg.call("configure_rings", _rings)
	add_child(_rings)
	_streamer = ClassDB.instantiate("Wg10Streamer")
	_view = ClassDB.instantiate("Wg10TerrainView")

	# Camera + environment + light.
	var env := Environment.new()
	_cfg.call("configure_review_environment", env)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	add_child(light)
	_camera = load(FLY_CAMERA).new()
	_camera.environment = env
	_camera.far = float(_cfg.call("loaded_edge_m"))
	add_child(_camera)
	_camera.global_position = Vector3(0.0, 1800.0, 0.0)

	_profiler = load(PROFILER).new()
	add_child(_profiler)
	_cfg.call("register_shader_globals", false)

	# HUD.
	var layer := CanvasLayer.new()
	add_child(layer)
	_hud = Label.new()
	_hud.position = Vector2(12, 12)
	_hud.add_theme_font_size_override("font_size", 16)
	layer.add_child(_hud)

	_load_step(0)

func _exit_tree() -> void:
	# Deterministic teardown (B1): free the pool's page-texture RIDs + join the bake worker.
	if _pool != null:
		_pool.call("free_all")
		_pool = null

# ---- step lifecycle -----------------------------------------------------------

func _load_step(idx: int) -> void:
	_step_idx = clampi(idx, 0, _steps.size() - 1)
	var step: Dictionary = _steps[_step_idx]

	# Fresh pool per step (deterministic; a producer's configure_* calls free_before_reconfigure,
	# but a brand-new pool avoids any cross-producer state leak between steps).
	if _pool != null:
		_pool.call("free_all")
	_pool = ClassDB.instantiate("Wg10PagePool")

	var setup_name: String = step["setup"]
	_step_err = str(call(setup_name))
	if _step_err != "":
		push_error("feature_review: step '%s' setup failed: %s" % [step["name"], _step_err])
		# Leave the renderer pointed at the (failed) pool; HUD will show the error.
		return

	# (Re)wire the streamer + view to the new pool.
	_cfg.call("configure_streamer", _streamer, _pool)
	_cfg.call("configure_view", _view, _pool, _streamer, _rings, _morph_on)

	_profiler.call("reset")
	_gpu_samples.clear()
	_camera.global_position = step["cam"]

# ---- per-step pool setup (one method per feature) -----------------------------

func _producer() -> Object:
	return load(PRODUCERS).new()

func _setup_reference() -> String:
	var p := _producer()
	p.set_mode_label("REFERENCE")
	return str(p.configure(_pool))

func _setup_mountain_macro() -> String:
	var p := _producer()
	p.set_mode_label("MOUNTAIN")
	p.set_preset_label("close_debug")   # raw live recipe (no reference bind)
	# flow OFF for "macro" is handled by the producer's flow_max_level; close_debug shows live macro.
	return str(p.configure(_pool))

func _setup_mountain_flow() -> String:
	var p := _producer()
	p.set_mode_label("MOUNTAIN")
	p.set_preset_label("network_ref")   # reference-backed bridge (flow-on network look)
	return str(p.configure(_pool))

func _setup_world() -> String:
	var p := _producer()
	p.set_mode_label("WORLD")
	return str(p.configure(_pool))

func _setup_legacy() -> String:
	var p := _producer()
	p.set_mode_label("LEGACY")
	return str(p.configure(_pool))

func _setup_region_fact() -> String:
	# The carved baked-look producer (this session). region_span_m == BASE_SPAN so each region fact
	# is one base page tiling cleanly with the clipmap; region_n == page_px for full-res sampling;
	# k=2 super-region (carve-big-then-slice = internal-seam-exact). flow_on for the carved drainage.
	var base_span: float = _cfg.call("base_span_m")
	return str(_pool.call("configure_region_fact",
		ProjectSettings.globalize_path(RF_PACK),
		ProjectSettings.globalize_path(RF_PRIM),
		ProjectSettings.globalize_path(RF_MACHINE),
		ProjectSettings.globalize_path(RF_FRAGMENT),
		256,            # region_n (grid side == page_px)
		2,              # k (2x2 super-region)
		160,            # apron_px (matches the mountain producer apron)
		177,            # seed (mountain review seed)
		90000.0,        # feature_span_m (network scale)
		1700.0,         # height_scale_m
		192,            # flow_iters
		true,           # flow_on (carved drainage)
		256,            # page_px
		base_span))     # region_span_m == BASE_SPAN -> one fact per base page

# ---- input --------------------------------------------------------------------

func _input(event: InputEvent) -> void:
	if not (event is InputEventKey and event.pressed):
		return
	match event.keycode:
		KEY_BRACKETRIGHT:
			_load_step(_step_idx + 1)
		KEY_BRACKETLEFT:
			_load_step(_step_idx - 1)
		KEY_M:
			_morph_on = not _morph_on
			RenderingServer.global_shader_parameter_set("wg_dbg_mode", 1.0 if _morph_on else 0.0)
			# re-apply morph to the view
			_cfg.call("configure_view", _view, _pool, _streamer, _rings, _morph_on)
		KEY_N:
			_detail_on = not _detail_on
			_cfg.call("set_detail_enabled", _detail_on)
		KEY_R:
			_camera.global_position = _steps[_step_idx]["cam"]
		KEY_P:
			_print_snapshot()
		_:
			# number keys 1..9,0 jump to a step
			var n := _digit_for(event.keycode)
			if n >= 0:
				_load_step(n)

func _digit_for(keycode: int) -> int:
	if keycode >= KEY_1 and keycode <= KEY_9:
		return keycode - KEY_1
	if keycode == KEY_0:
		return 9
	return -1

# ---- per-frame update + HUD ---------------------------------------------------

func _process(delta: float) -> void:
	if _view == null or _camera == null:
		return
	var p: Vector3 = _camera.global_position
	var v: Vector3 = _camera.call("get_velocity")
	_view.call("update", p.x, p.z, v.x, v.z)

	# Sample real GPU time (vsync-immune, no stall) for a rolling p99.
	var gpu_ms := RenderingServer.viewport_get_measured_render_time_gpu(get_viewport().get_viewport_rid())
	if gpu_ms > 0.0:
		_gpu_samples.append(gpu_ms)
		if _gpu_samples.size() > GPU_WINDOW:
			_gpu_samples.pop_front()

	_hud_accum += delta
	if _hud_accum >= 0.25:
		_hud_accum = 0.0
		_refresh_hud()

func _gpu_p99_ms() -> float:
	if _gpu_samples.is_empty():
		return 0.0
	var s := _gpu_samples.duplicate()
	s.sort()
	var i := int(float(s.size()) * 0.99)
	i = clampi(i, 0, s.size() - 1)
	return s[i]

func _refresh_hud() -> void:
	var step: Dictionary = _steps[_step_idx]
	var lines := []
	lines.append("[ %d/%d ]  %s" % [_step_idx + 1, _steps.size(), step["name"]])
	lines.append("   role: %s" % step["role"])
	lines.append("   accept: %s" % step["accept"])
	if _step_err != "":
		lines.append("   !! SETUP ERROR: %s" % _step_err)
	lines.append("")
	lines.append("fps %.0f   frame p99 %.2f ms   mean %.2f ms   GPU p99 %.2f ms" % [
		_profiler.call("fps"), _profiler.call("p99_ms"), _profiler.call("mean_ms"), _gpu_p99_ms()])
	if _view != null:
		var s: Dictionary = _view.call("stats")
		lines.append("pages: resident %d   created %d   recomputed %d   full %d" % [
			int(s.get("resident", 0)), int(s.get("created", 0)), int(s.get("recomputed", 0)), int(s.get("full_events", 0))])
	# Region-fact stats when that producer is active.
	if _pool != null and _pool.has_method("region_fact_stats"):
		var rf: Dictionary = _pool.call("region_fact_stats")
		if bool(rf.get("active", false)):
			lines.append("region-fact: cached %d   baking %d   (off-frame super-region bake)" % [
				int(rf.get("cached_regions", 0)), int(rf.get("baking_in_flight", 0))])
	lines.append("")
	lines.append("] next  [ prev  1-0 jump  R reframe  M morph  N detail  P snapshot  WASD+Shift fly")
	_hud.text = "\n".join(lines)

func _print_snapshot() -> void:
	var step: Dictionary = _steps[_step_idx]
	var rf := {}
	if _pool != null and _pool.has_method("region_fact_stats"):
		rf = _pool.call("region_fact_stats")
	print("[feature-review] step='%s' fps=%.0f frame_p99_ms=%.3f gpu_p99_ms=%.3f pool=%s region_fact=%s" % [
		step["name"], _profiler.call("fps"), _profiler.call("p99_ms"), _gpu_p99_ms(),
		str(_view.call("stats")), str(rf)])
