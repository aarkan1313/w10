extends Node3D

# ============================================================================
# WG10 UNIFIED FEATURE REVIEW (§ owner sequential visual review)
# ----------------------------------------------------------------------------
# ONE scene to fly + profile EVERY shippable terrain feature, one at a time, in
# order, on the SAME clipmap pipeline. Each STEP reconfigures the SAME pool with a
# different producer (deterministic teardown between steps, B1), repositions the
# camera, and updates the HUD. Pure assembly: it reuses the existing harness
# components (Wg10FlyCamera, Wg10Profiler, the runtime-config helper, the producer
# helpers, Wg10TerrainView/ClipmapRings) — it does NOT duplicate them.
#
# Plus two steps for shipped subsystems that previously had NO visual surface:
#   - FACTS / COLLISION: samples the authoritative collision field around the camera
#     (Wg10Facts.get_collision_field) and draws it as a point cloud you can fly.
#   - TERRAIN EDITS: stamp a crater / mound (Wg10Facts.apply_edit) and watch the
#     collision overlay update live.
#
# LAUNCH: run this scene WINDOWED (RenderingDevice compute is windowed-only).
# CONTROLS:
#   ] / [        next / previous feature step
#   1..9 / 0     jump to step N (0 = step 10)
#   WASD         fly (camera-local), Shift = sprint (~1000s m/s)
#   Space / C    up / down,  mouse = look,  ESC = release mouse
#   M            morph-band heatmap toggle (clipmap steps)
#   N            M5 detail overlay toggle (clipmap steps)
#   R            reframe camera to this step's start pose
#   P            print the current step's profiling snapshot to the console
#   F            (facts/edit steps) stamp a CRATER at the camera ground point
#   G            (facts/edit steps) stamp a MOUND at the camera ground point
#   X            (facts/edit steps) clear all edits
# HUD (top-left): step name + role + acceptance; fps / frame p99 / real GPU p99 ms;
#   pool stats; region-fact bake progress (active step); facts/edit status.
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

# Facts node pack (Wg10Facts loads its OWN pack: a dir + file).
const FACTS_PACK_DIR := "res://worldgen_terrain/packs/dem_v1"
const FACTS_PACK_FILE := "terrain_pack.gate.json"
const FACTS_SEED := 1337
const FACTS_GRID_N := 33            # samples per side of the collision field
const FACTS_GRID_SPAN_M := 4096.0   # world size of the sampled collision patch
const EDIT_RADIUS_M := 400.0
const EDIT_DEPTH_M := 220.0         # crater digs -, mound raises +

# ---- the ordered feature list -------------------------------------------------
# Each step describes WHAT to review. `kind` selects the runtime behaviour:
#   "producer" — reconfigure the page pool + clipmap with a producer (fly the terrain).
#   "facts"    — sample + draw the authoritative collision field (no page producer).
var _steps: Array = [
	{
		"name": "1. Accepted reference baseline",
		"role": "static mountain-network payload streamed through the live clipmap",
		"accept": "accepted visual baseline",
		"kind": "producer", "setup": "_setup_reference",
		"cam": Vector3(0.0, 1800.0, 0.0),
	},
	{
		"name": "2. Live procedural mountain (macro + flow)",
		"role": "GPU seam-safe mountain recipe, live (NOT reference-bound) — raw procedural look",
		"accept": "live procedural, not the accepted baseline (close-debug)",
		"kind": "producer", "setup": "_setup_mountain_live",
		"cam": Vector3(0.0, 1800.0, 0.0),
	},
	{
		"name": "3. Reference-backed mountain bridge",
		"role": "live mountain producer bound to the accepted payload (should MATCH step 1)",
		"accept": "bridge — matches reference by design, not final procedural",
		"kind": "producer", "setup": "_setup_mountain_bridge",
		"cam": Vector3(0.0, 1800.0, 0.0),
	},
	{
		"name": "4. CARVED BAKED LOOK (region-fact producer)",
		"role": "off-frame super-region bake -> carve+condition -> sliced region facts on screen",
		"accept": "this session's feature: carved look on screen, internal-seam-exact (owner A/B here)",
		"kind": "producer", "setup": "_setup_region_fact",
		"cam": Vector3(0.0, 2200.0, 0.0),
	},
	{
		"name": "5. World composition (diagnostic)",
		"role": "grammar-routed route/weight overlay on accepted reference height",
		"accept": "diagnostic, not accepted (compose hitches)",
		"kind": "producer", "setup": "_setup_world",
		"cam": Vector3(0.0, 1800.0, 0.0),
	},
	{
		"name": "6. Legacy DEM atlas (regression baseline)",
		"role": "the original kernel-atlas renderer — regression reference only",
		"accept": "legacy regression, not accepted",
		"kind": "producer", "setup": "_setup_legacy",
		"cam": Vector3(0.0, 1800.0, 0.0),
	},
	{
		"name": "7. FACTS / COLLISION field (authoritative)",
		"role": "Wg10Facts.get_collision_field sampled around the camera, drawn as a point cloud",
		"accept": "shipped facts/collision subsystem (parity-proven); first visual surface",
		"kind": "facts", "setup": "",
		"cam": Vector3(0.0, 1400.0, 0.0),
	},
	{
		"name": "8. TERRAIN EDITS (stamp crater / mound)",
		"role": "Wg10Facts.apply_edit — F=crater G=mound X=clear; collision overlay updates live",
		"accept": "shipped M4 edit API; first visual surface",
		"kind": "facts", "setup": "",
		"cam": Vector3(0.0, 1400.0, 0.0),
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

# facts/collision overlay state
var _facts: Object             # Wg10Facts
var _facts_points: MultiMeshInstance3D
var _facts_status := ""

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

	# Facts node (own pack) + a point-cloud overlay for the collision field.
	_facts = ClassDB.instantiate("Wg10Facts")
	var ferr := str(_facts.call("configure",
		ProjectSettings.globalize_path(FACTS_PACK_DIR), FACTS_PACK_FILE, FACTS_SEED))
	if ferr != "":
		push_error("feature_review: Wg10Facts configure failed: %s" % ferr)
	_build_facts_overlay()

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

# ---- facts/collision overlay --------------------------------------------------

func _build_facts_overlay() -> void:
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = true
	var cube := BoxMesh.new()
	cube.size = Vector3(60.0, 60.0, 60.0)
	mm.mesh = cube
	mm.instance_count = FACTS_GRID_N * FACTS_GRID_N
	_facts_points = MultiMeshInstance3D.new()
	_facts_points.multimesh = mm
	var mat := StandardMaterial3D.new()
	mat.vertex_color_use_as_albedo = true
	mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	_facts_points.material_override = mat
	_facts_points.visible = false
	add_child(_facts_points)

func _update_facts_overlay() -> void:
	if _facts == null or _facts_points == null:
		return
	var cx: float = _camera.global_position.x
	var cz: float = _camera.global_position.z
	var grid: PackedFloat32Array = _facts.call("get_collision_field", cx, cz, FACTS_GRID_SPAN_M, FACTS_GRID_N)
	if grid.size() != FACTS_GRID_N * FACTS_GRID_N:
		_facts_status = "get_collision_field returned %d (expected %d)" % [grid.size(), FACTS_GRID_N * FACTS_GRID_N]
		return
	var mm: MultiMesh = _facts_points.multimesh
	var corner_x := cx - FACTS_GRID_SPAN_M * 0.5
	var corner_z := cz - FACTS_GRID_SPAN_M * 0.5
	var step := FACTS_GRID_SPAN_M / float(FACTS_GRID_N - 1)
	var hmin := 1e30
	var hmax := -1e30
	for h in grid:
		hmin = minf(hmin, h); hmax = maxf(hmax, h)
	var span := maxf(1.0, hmax - hmin)
	var i := 0
	for j in range(FACTS_GRID_N):
		for k in range(FACTS_GRID_N):
			var h: float = grid[i]
			var wx := corner_x + float(k) * step
			var wz := corner_z + float(j) * step
			mm.set_instance_transform(i, Transform3D(Basis(), Vector3(wx, h, wz)))
			var t := (h - hmin) / span    # color by height: blue low -> red high
			mm.set_instance_color(i, Color(t, 0.25, 1.0 - t))
			i += 1
	_facts_status = "collision %dx%d  span %.0fm  height [%.0f, %.0f] m" % [
		FACTS_GRID_N, FACTS_GRID_N, FACTS_GRID_SPAN_M, hmin, hmax]

func _stamp_edit(depth: float) -> void:
	if _facts == null:
		return
	var cx: float = _camera.global_position.x
	var cz: float = _camera.global_position.z
	_facts.call("apply_edit", cx, cz, EDIT_RADIUS_M, depth, 1.0)
	_update_facts_overlay()

# ---- step lifecycle -----------------------------------------------------------

func _load_step(idx: int) -> void:
	_step_idx = clampi(idx, 0, _steps.size() - 1)
	var step: Dictionary = _steps[_step_idx]
	_step_err = ""

	var is_facts: bool = step["kind"] == "facts"
	# Clipmap rings render only on producer steps; the facts overlay only on facts steps.
	_rings.visible = not is_facts
	if _facts_points != null:
		_facts_points.visible = is_facts

	if is_facts:
		# No page producer; tear down any prior one so the streamer doesn't churn.
		if _pool != null:
			_pool.call("free_all")
			_pool = null
		_view = ClassDB.instantiate("Wg10TerrainView")   # detach view from the freed pool
		_profiler.call("reset")
		_gpu_samples.clear()
		_camera.global_position = step["cam"]
		_update_facts_overlay()
		return

	# Producer step: fresh pool, configure, (re)wire streamer + view.
	if _pool != null:
		_pool.call("free_all")
	_pool = ClassDB.instantiate("Wg10PagePool")
	var setup_name: String = step["setup"]
	_step_err = str(call(setup_name))
	if _step_err != "":
		push_error("feature_review: step '%s' setup failed: %s" % [step["name"], _step_err])
		return
	_cfg.call("configure_streamer", _streamer, _pool)
	_cfg.call("configure_view", _view, _pool, _streamer, _rings, _morph_on)
	_profiler.call("reset")
	_gpu_samples.clear()
	_camera.global_position = step["cam"]

# ---- per-step pool setup (one method per producer feature) --------------------

func _producer() -> Object:
	return load(PRODUCERS).new()

func _setup_reference() -> String:
	var p := _producer()
	p.set_mode_label("REFERENCE")
	return str(p.configure(_pool))

func _setup_mountain_live() -> String:
	# Raw live procedural mountain recipe (close_debug = NOT reference-bound) — the genuine
	# live look (macro + flow per flow_max_level), distinct from the accepted baseline.
	var p := _producer()
	p.set_mode_label("MOUNTAIN")
	p.set_preset_label("close_debug")
	return str(p.configure(_pool))

func _setup_mountain_bridge() -> String:
	# Reference-backed bridge: live mountain producer that binds the accepted payload for height.
	# Renders ~= step 1 BY DESIGN (it's a bridge, not a distinct procedural look) — kept so the
	# bridge contract is reviewable next to the raw live look (step 2) and the reference (step 1).
	var p := _producer()
	p.set_mode_label("MOUNTAIN")
	p.set_preset_label("network_ref")
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
			if _pool != null and _steps[_step_idx]["kind"] == "producer":
				_cfg.call("configure_view", _view, _pool, _streamer, _rings, _morph_on)
		KEY_N:
			_detail_on = not _detail_on
			_cfg.call("set_detail_enabled", _detail_on)
		KEY_R:
			_camera.global_position = _steps[_step_idx]["cam"]
		KEY_P:
			_print_snapshot()
		KEY_F:
			if _steps[_step_idx]["kind"] == "facts":
				_stamp_edit(-EDIT_DEPTH_M)   # crater (dig)
		KEY_G:
			if _steps[_step_idx]["kind"] == "facts":
				_stamp_edit(EDIT_DEPTH_M)    # mound (raise)
		KEY_X:
			if _steps[_step_idx]["kind"] == "facts" and _facts != null:
				_facts.call("clear_edits")
				_update_facts_overlay()
		_:
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
	if _camera == null:
		return
	var is_facts: bool = _steps[_step_idx]["kind"] == "facts"
	if not is_facts and _view != null:
		var p: Vector3 = _camera.global_position
		var v: Vector3 = _camera.call("get_velocity")
		_view.call("update", p.x, p.z, v.x, v.z)
	elif is_facts:
		# Re-sample the collision field as the camera moves (cheap; CPU sparse query).
		_update_facts_overlay()

	# Sample real GPU time (vsync-immune, no stall) for a rolling p99.
	var vp_rid := get_viewport().get_viewport_rid()
	var gpu_ms := RenderingServer.viewport_get_measured_render_time_gpu(vp_rid)
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
	if step["kind"] == "producer" and _view != null:
		var s: Dictionary = _view.call("stats")
		lines.append("pages: resident %d   created %d   recomputed %d   full %d" % [
			int(s.get("resident", 0)), int(s.get("created", 0)), int(s.get("recomputed", 0)), int(s.get("full_events", 0))])
		if _pool != null and _pool.has_method("region_fact_stats"):
			var rf: Dictionary = _pool.call("region_fact_stats")
			if bool(rf.get("active", false)):
				lines.append("region-fact: cached %d   baking %d   (off-frame super-region bake)" % [
					int(rf.get("cached_regions", 0)), int(rf.get("baking_in_flight", 0))])
	elif step["kind"] == "facts":
		lines.append("facts: %s" % _facts_status)
		lines.append("   F = crater   G = mound   X = clear edits  (at camera ground point)")
	lines.append("")
	lines.append("] next  [ prev  1-0 jump  R reframe  M morph  N detail  P snapshot  WASD+Shift fly")
	_hud.text = "\n".join(lines)

func _print_snapshot() -> void:
	var step: Dictionary = _steps[_step_idx]
	var rf := {}
	var pool_stats := {}
	if step["kind"] == "producer":
		if _view != null:
			pool_stats = _view.call("stats")
		if _pool != null and _pool.has_method("region_fact_stats"):
			rf = _pool.call("region_fact_stats")
	print("[feature-review] step='%s' fps=%.0f frame_p99_ms=%.3f gpu_p99_ms=%.3f pool=%s region_fact=%s facts='%s'" % [
		step["name"], _profiler.call("fps"), _profiler.call("p99_ms"), _gpu_p99_ms(),
		str(pool_stats), str(rf), _facts_status])
