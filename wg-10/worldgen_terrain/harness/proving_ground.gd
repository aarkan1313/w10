extends Node3D

# PROVING GROUND — the reset scene. Activates ONE render component at a time so each is proven
# on-screen (owner-flown) before the next is switched on. Set STEP to choose how much is active.
# This is the "build a scene, prove one part at a time" plan (COMPONENT_INVENTORY.md). The proven
# CPU/GPU leaves (pool, policies, streamer, ring_geometry, page_compute) are reused untouched;
# only the presentation (this scene + ring_displace) is being rebuilt with proof at every step.
#
#   STEP 1: ONE level-0 page, ONE flat tile, morph OFF, NO streamer. Prove: a single continuous
#           tile with real relief, stable, no internal cracks. (page_compute + pool + shader.)
#
# LAUNCH windowed. Fly: WASD + Shift sprint, mouse look, Space/C up/down, ESC frees the mouse.
# HUD shows the step + camera pos. Start above the tile looking down-ish.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const FLY_CAMERA := "res://worldgen_terrain/harness/fly_camera.gd"

const STEP := 7
const PAGE_PX := 256
const SEED := 1337
const BASE_SPAN := 8192.0     # one level-0 page spans this many metres
const GRID_RES := 64          # mesh cells across the page (vertices = 65x65)
const CAPACITY := 96    # step 7: 3 levels x 9 = 27 + stream-ahead + parent-fetch headroom
const HEIGHT_SCALE := 0.35
const RELIEF_REF := 2000.0
# Streamer tunables. NUM_LEVELS grows with the step (4 = 1 level; 5+ = 2 levels for never-black).
const RADIUS_PAGES := 1
const LEAD_SECONDS := 0.5    # velocity lead in SECONDS (clamped by the policy to stay in-ring)
const MAX_PER_FRAME := 4

var _num_levels: int = 1       # set per step in _ready
var _morph_region: float = 0.0 # step 6: fine->coarse blend band width (0 = off)
var _pool: Object
var _streamer: Object          # step 4+: drives page residency from camera pos/vel
var _step4_tiles: Array = []   # step 4: 9 single-level tiles
# step 5+: per level, 9 persistent MeshInstance3D (level 0 = fine on top, level 1 = coarse under).
var _level_tiles: Array = []   # Array of Array[MeshInstance3D], indexed [level][slot]
# live artifact instrumentation (step 5): per [level][slot] remember last bound page + visibility,
# count flips so a transient pop shows up on the HUD with which level/slot/frame caused it.
var _last_key: Array = []      # [level][slot] -> Vector2 page origin (or (NAN,NAN) when hidden)
var _flip_count := 0
var _last_flip := ""
var _frame := 0
# step 7 perf: rolling frame-time window for a live p99 readout (the acceptance metric).
var _ft_window: Array[float] = []
const FT_WINDOW := 240
var _hud: Label
var _camera: Camera3D

func _ready() -> void:
	if RenderingServer.get_rendering_device() == null:
		push_error("proving_ground: no RenderingDevice (run windowed)"); return

	# Sky + light so the unshaded terrain reads against a horizon.
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.45, 0.62, 0.85)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	add_child(light)

	# levels per step: steps 1-4 are single-level; step 5+ add a coarser level for never-black.
	_num_levels = 3 if STEP >= 7 else (2 if STEP >= 5 else 1)
	# morph region (fraction of the fine neighborhood half-extent over which fine fades to coarse).
	# 0 until step 6; step 6 turns it on to blend the LOD boundary.
	_morph_region = 0.35 if STEP >= 6 else 0.0

	# --- proven leaves: configure the pool, acquire ONE page ---
	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	_pool = ClassDB.instantiate("Wg10PagePool")
	var err: String = str(_pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if err != "":
		push_error("proving_ground: pool configure failed: %s" % err); return

	if STEP == 1:
		_build_step1()
	elif STEP == 2:
		_build_step2()
	elif STEP == 3:
		_build_step3()
	elif STEP == 4:
		_build_step4()
	elif STEP == 5 or STEP == 6 or STEP == 7:
		_build_step5()   # builds _num_levels x 9 tiles; the per-frame drive differs by step

	# --- fly camera ---
	_camera = load(FLY_CAMERA).new()
	_camera.environment = env
	_camera.far = BASE_SPAN * 16.0
	add_child(_camera)
	# Start above the area, looking down. Centre over whatever the step builds.
	match STEP:
		1:
			_camera.global_position = Vector3(BASE_SPAN * 0.5, 2500.0, BASE_SPAN * 0.5)
		2:
			_camera.global_position = Vector3(BASE_SPAN, 2500.0, BASE_SPAN * 0.5)
		3:
			# 3x3 spans [0, 3*BASE_SPAN]; centre over the middle page, higher to see all 9.
			_camera.global_position = Vector3(BASE_SPAN * 1.5, 6000.0, BASE_SPAN * 1.5)
		4, 5, 6, 7:
			# moving 3x3 follows the camera — start at the origin, a bit up, looking ahead.
			_camera.global_position = Vector3(0.0, 1800.0, 0.0)
		_:
			_camera.global_position = Vector3(BASE_SPAN, 2500.0, BASE_SPAN * 0.5)
	_camera.rotation_degrees = Vector3(-35.0, 0.0, 0.0)

	# --- HUD ---
	var layer := CanvasLayer.new()
	add_child(layer)
	_hud = Label.new()
	_hud.position = Vector2(8, 6)
	_hud.add_theme_color_override("font_color", Color.WHITE)
	layer.add_child(_hud)

# STEP 1: acquire the single level-0 page at world origin (0,0), one full-grid tile over world
# [0, BASE_SPAN], sampled by world UV, morph OFF.
func _build_step1() -> void:
	if _build_tile(0.0, 0.0) == null:
		push_error("proving_ground STEP1: acquire_page returned null")

# STEP 2: TWO adjacent level-0 pages — origin page (0,0) and its +X neighbor (BASE_SPAN,0). Each
# is its own page+tile sampled in its own world frame. The shared world edge at x=BASE_SPAN is
# the FIRST place a real seam can show: if texel-corner generation + world-UV sampling are right,
# the surface is continuous across it under a moving camera; if it cracks, the bug is isolated to
# two tiles. Morph OFF (no levels yet) so this is purely the fine seam.
func _build_step2() -> void:
	if _build_tile(0.0, 0.0) == null or _build_tile(BASE_SPAN, 0.0) == null:
		push_error("proving_ground STEP2: a page acquire returned null")

# STEP 3: a 3x3 of level-0 pages around the origin (page origins at (i,j)*BASE_SPAN for
# i,j in {0,1,2}), each its own tile sampled in its own frame, morph OFF. All edges are now
# the proven clamp-to-edge seam. Prove: nine pages read as ONE surface — no internal grid lines.
func _build_step3() -> void:
	for j in range(3):
		for i in range(3):
			if _build_tile(i * BASE_SPAN, j * BASE_SPAN) == null:
				push_error("proving_ground STEP3: page (%d,%d) acquire returned null" % [i, j])

# STEP 4: the STREAMER drives a MOVING 3x3 of level-0 tiles that follows the camera. 9 persistent
# tile meshes; each frame we call streamer.update(pos,vel) (which acquires/releases pages via the
# proven pool), then re-place + re-bind each tile to the resident page for its slot around the
# camera. A page not yet resident -> that tile is hidden (single level: no coarser fallback yet;
# never-black comes with level 2 at step 5). Prove: fly fast, the surface stays continuous, the
# HUD `recomputed` counter does NOT climb every frame (no churn), no flicker.
func _build_step4() -> void:
	_streamer = ClassDB.instantiate("Wg10Streamer")
	_streamer.call("configure", _pool, _num_levels, BASE_SPAN, RADIUS_PAGES, LEAD_SECONDS, MAX_PER_FRAME)
	# 9 persistent tiles, each a full level-0 grid mesh, re-placed + re-bound each frame.
	for _t in range(9):
		var mi := MeshInstance3D.new()
		mi.set_mesh(_make_grid_mesh(BASE_SPAN, GRID_RES))
		var mat := ShaderMaterial.new()
		mat.set_shader(load(SHADER))
		mat.set_shader_parameter("world_span", BASE_SPAN)
		mat.set_shader_parameter("coarse_span", BASE_SPAN)
		mat.set_shader_parameter("level_half_extent", BASE_SPAN)
		mat.set_shader_parameter("relief_scale", HEIGHT_SCALE)
		mat.set_shader_parameter("morph_region", 0.0)   # morph off (single level)
		mat.set_shader_parameter("relief_ref", RELIEF_REF)
		mi.set_material_override(mat)
		mi.visible = false
		add_child(mi)
		_step4_tiles.append(mi)

# Per-frame Step 4 drive: stream around the camera, then place + bind the 3x3 to resident pages.
func _drive_step4(cam_x: float, cam_z: float, vel_x: float, vel_z: float) -> void:
	# 1) let the proven streamer maintain residency (acquire/release via the pool).
	_streamer.call("update", cam_x, cam_z, vel_x, vel_z)
	# 2) place the 3x3 around the SAME clamped led centre the streamer covers (ask it — never
	#    recompute the lead here, or the view desyncs / flies off; the clamp keeps the camera
	#    inside the ring). Then bind each slot to its resident page; hide a not-yet-resident slot.
	var led: Vector2 = _streamer.call("coverage_center", cam_x, cam_z, vel_x, vel_z)
	var cox: float = floor(led.x / BASE_SPAN) * BASE_SPAN
	var coz: float = floor(led.y / BASE_SPAN) * BASE_SPAN
	var slot := 0
	for dz in range(-1, 2):
		for dx in range(-1, 2):
			var ox: float = cox + dx * BASE_SPAN
			var oz: float = coz + dz * BASE_SPAN
			var mi: MeshInstance3D = _step4_tiles[slot]
			var tex: Object = _pool.call("get_resident_page", 0, ox, oz)
			if tex == null:
				mi.visible = false
			else:
				mi.visible = true
				mi.position = Vector3(ox + BASE_SPAN * 0.5, 0.0, oz + BASE_SPAN * 0.5)
				var mat: ShaderMaterial = mi.get_material_override()
				mat.set_shader_parameter("height_tex", tex)
				mat.set_shader_parameter("coarse_height_tex", tex)
				mat.set_shader_parameter("page_origin", Vector2(ox, oz))
				mat.set_shader_parameter("coarse_origin", Vector2(ox, oz))
				mat.set_shader_parameter("level_center", Vector2(cox + BASE_SPAN * 0.5, coz + BASE_SPAN * 0.5))
			slot += 1

# STEP 5: TWO levels for NEVER-BLACK. Level 1 (coarse, span 2x) is a 3x3 ALWAYS drawn underneath;
# level 0 (fine) is a 3x3 drawn on top, each fine tile shown only when its page is resident. When
# a fine tile isn't ready yet, the coarse page shows through -> coarse-but-correct terrain, never
# a hole or wink (this is what kills the edge "switching" of step 4). Morph still OFF (step 6 adds
# the smooth fine<->coarse blend). render_priority: fine = 1 (on top), coarse = 0 (under).
func _build_step5() -> void:
	_streamer = ClassDB.instantiate("Wg10Streamer")
	_streamer.call("configure", _pool, _num_levels, BASE_SPAN, RADIUS_PAGES, LEAD_SECONDS, MAX_PER_FRAME)
	_level_tiles.clear()
	for level in range(_num_levels):
		var span_l: float = BASE_SPAN * pow(2.0, level)
		var prio := _num_levels - 1 - level   # finest highest -> drawn on top
		var tiles: Array = []
		for _t in range(9):
			var mi := MeshInstance3D.new()
			mi.set_mesh(_make_grid_mesh(span_l, GRID_RES))
			var mat := ShaderMaterial.new()
			mat.set_shader(load(SHADER))
			mat.set_shader_parameter("world_span", span_l)
			mat.set_shader_parameter("coarse_span", span_l)
			mat.set_shader_parameter("level_half_extent", span_l)
			mat.set_shader_parameter("relief_scale", HEIGHT_SCALE)
			# only the FINE level (0) morphs toward its coarse parent; the coarsest level has
			# nothing coarser to blend to, so it stays morph-off.
			var morph: float = _morph_region if level == 0 else 0.0
			mat.set_shader_parameter("morph_region", morph)
			mat.set_shader_parameter("relief_ref", RELIEF_REF)
			mat.set_render_priority(prio)
			mi.set_material_override(mat)
			mi.visible = false
			add_child(mi)
			tiles.append(mi)
		_level_tiles.append(tiles)
		# instrumentation: start every slot "hidden" (NAN sentinel)
		var keys: Array = []
		for _k in range(9):
			keys.append(Vector2(NAN, NAN))
		_last_key.append(keys)

# Per-frame Step 5 drive: stream both levels, then place each level's 3x3 on its own clamped led
# centre. Coarse (level 1) ALWAYS shows where resident (the blanket); fine (level 0) shows on top
# where resident, else the coarse beneath shows through. No fine tile is ever left visible at a
# stale position.
func _drive_step5(cam_x: float, cam_z: float, vel_x: float, vel_z: float) -> void:
	_frame += 1
	_streamer.call("update", cam_x, cam_z, vel_x, vel_z)
	var led: Vector2 = _streamer.call("coverage_center", cam_x, cam_z, vel_x, vel_z)
	for level in range(_num_levels):
		var span_l: float = BASE_SPAN * pow(2.0, level)
		var cox: float = floor(led.x / span_l) * span_l
		var coz: float = floor(led.y / span_l) * span_l
		var tiles: Array = _level_tiles[level]
		var slot := 0
		for dz in range(-1, 2):
			for dx in range(-1, 2):
				var ox: float = cox + dx * span_l
				var oz: float = coz + dz * span_l
				var mi: MeshInstance3D = tiles[slot]
				var tex: Object = _pool.call("get_resident_page", level, ox, oz)
				# instrumentation: a "flip" = this slot changing what it shows vs last frame
				# (hidden<->visible, or bound to a DIFFERENT page). That's the transient that can
				# read as a small square popping. We log level/slot/kind so the HUD names it.
				var new_key: Vector2 = Vector2(NAN, NAN) if tex == null else Vector2(ox, oz)
				var old_key: Vector2 = _last_key[level][slot]
				var was_hidden := is_nan(old_key.x)
				var now_hidden := tex == null
				if was_hidden != now_hidden:
					_flip_count += 1
					_last_flip = "f%d L%d slot%d %s" % [_frame, level, slot, "SHOW" if not now_hidden else "HIDE"]
				elif not now_hidden and (old_key.x != ox or old_key.y != oz):
					_flip_count += 1
					_last_flip = "f%d L%d slot%d REPAGE" % [_frame, level, slot]
				_last_key[level][slot] = new_key
				if tex == null:
					mi.visible = false
				else:
					mi.visible = true
					mi.position = Vector3(ox + span_l * 0.5, 0.0, oz + span_l * 0.5)
					var mat: ShaderMaterial = mi.get_material_override()
					mat.set_shader_parameter("world_span", span_l)
					mat.set_shader_parameter("height_tex", tex)
					mat.set_shader_parameter("coarse_height_tex", tex)
					mat.set_shader_parameter("page_origin", Vector2(ox, oz))
					mat.set_shader_parameter("coarse_origin", Vector2(ox, oz))
					mat.set_shader_parameter("level_center", Vector2(cox + span_l * 0.5, coz + span_l * 0.5))
				slot += 1

# STEP 6: like step 5, but the FINE level (0) GEOMORPHS toward the real coarse page beneath it,
# blending fine->coarse over the outer band of the fine 3x3 -> the hard LOD line (step-5 finding)
# becomes a smooth transition. Each fine tile binds: its fine page (height_tex) + the ACTUAL coarse
# page covering its footprint (coarse_height_tex/coarse_origin/coarse_span) + the FINE neighborhood
# centre/half-extent (so the blend rises to 1 exactly at the fine 3x3 outer edge, where coarse
# takes over). The coarse level (1) draws underneath unmorphed, as in step 5.
func _drive_step6(cam_x: float, cam_z: float, vel_x: float, vel_z: float) -> void:
	_frame += 1
	_streamer.call("update", cam_x, cam_z, vel_x, vel_z)
	var led: Vector2 = _streamer.call("coverage_center", cam_x, cam_z, vel_x, vel_z)
	for level in range(_num_levels):
		var span_l: float = BASE_SPAN * pow(2.0, level)
		var cox: float = floor(led.x / span_l) * span_l
		var coz: float = floor(led.y / span_l) * span_l
		var tiles: Array = _level_tiles[level]
		var slot := 0
		for dz in range(-1, 2):
			for dx in range(-1, 2):
				var ox: float = cox + dx * span_l
				var oz: float = coz + dz * span_l
				var mi: MeshInstance3D = tiles[slot]
				var tex: Object = _pool.call("get_resident_page", level, ox, oz)
				if tex == null:
					mi.visible = false
					_last_key[level][slot] = Vector2(NAN, NAN)
					slot += 1
					continue
				mi.visible = true
				mi.position = Vector3(ox + span_l * 0.5, 0.0, oz + span_l * 0.5)
				var mat: ShaderMaterial = mi.get_material_override()
				mat.set_shader_parameter("world_span", span_l)
				mat.set_shader_parameter("height_tex", tex)
				mat.set_shader_parameter("page_origin", Vector2(ox, oz))
				if level < _num_levels - 1:
					# any non-coarsest level morphs toward its PARENT (level+1) over its own 3x3
					# outer band: bind the real parent page covering this tile + this level's frame.
					var pspan: float = span_l * 2.0
					var tc_x: float = ox + span_l * 0.5
					var tc_z: float = oz + span_l * 0.5
					var p_ox: float = floor(tc_x / pspan) * pspan
					var p_oz: float = floor(tc_z / pspan) * pspan
					var ptex: Object = _pool.call("get_resident_page", level + 1, p_ox, p_oz)
					if ptex == null:
						# parent not resident -> no morph this frame (still covered: this level's
						# own page is resident here).
						mat.set_shader_parameter("coarse_height_tex", tex)
						mat.set_shader_parameter("coarse_span", span_l)
						mat.set_shader_parameter("coarse_origin", Vector2(ox, oz))
						mat.set_shader_parameter("morph_region", 0.0)
					else:
						mat.set_shader_parameter("coarse_height_tex", ptex)
						mat.set_shader_parameter("coarse_span", pspan)
						mat.set_shader_parameter("coarse_origin", Vector2(p_ox, p_oz))
						mat.set_shader_parameter("morph_region", _morph_region)
					# this level's 3x3 neighborhood centre (middle tile centre) + half-extent.
					mat.set_shader_parameter("level_center", Vector2(cox + span_l * 0.5, coz + span_l * 0.5))
					mat.set_shader_parameter("level_half_extent", 1.5 * span_l)
				else:
					# coarsest level: nothing coarser to blend to; its own page for both samplers.
					mat.set_shader_parameter("coarse_height_tex", tex)
					mat.set_shader_parameter("coarse_span", span_l)
					mat.set_shader_parameter("coarse_origin", Vector2(ox, oz))
					mat.set_shader_parameter("morph_region", 0.0)
					mat.set_shader_parameter("level_center", Vector2(cox + span_l * 0.5, coz + span_l * 0.5))
					mat.set_shader_parameter("level_half_extent", span_l)
				slot += 1

# Acquire one level-0 page at world (ox,oz) and build its full-grid tile over world
# [ox, ox+BASE_SPAN], sampling that page by world UV, morph OFF. Returns the MeshInstance or null.
func _build_tile(ox: float, oz: float) -> MeshInstance3D:
	var tex: Object = _pool.call("acquire_page", 0, ox, oz)
	if tex == null:
		return null
	var mesh := _make_grid_mesh(BASE_SPAN, GRID_RES)
	var mi := MeshInstance3D.new()
	mi.set_mesh(mesh)
	var mat := ShaderMaterial.new()
	mat.set_shader(load(SHADER))
	mat.set_shader_parameter("height_tex", tex)
	mat.set_shader_parameter("coarse_height_tex", tex)        # unused (morph off) but must be set
	mat.set_shader_parameter("world_span", BASE_SPAN)
	mat.set_shader_parameter("coarse_span", BASE_SPAN)
	mat.set_shader_parameter("page_origin", Vector2(ox, oz))
	mat.set_shader_parameter("coarse_origin", Vector2(ox, oz))
	mat.set_shader_parameter("level_center", Vector2(ox + BASE_SPAN * 0.5, oz + BASE_SPAN * 0.5))
	mat.set_shader_parameter("level_half_extent", BASE_SPAN)   # morph off, value irrelevant
	mat.set_shader_parameter("relief_scale", HEIGHT_SCALE)
	mat.set_shader_parameter("morph_region", 0.0)              # MORPH OFF — pure fine sample
	mat.set_shader_parameter("relief_ref", RELIEF_REF)
	mi.set_material_override(mat)
	# Mesh is centred at local origin spanning [-span/2, span/2]; place its centre at the page
	# centre so it covers world [ox, ox+BASE_SPAN] x [oz, oz+BASE_SPAN].
	mi.position = Vector3(ox + BASE_SPAN * 0.5, 0.0, oz + BASE_SPAN * 0.5)
	add_child(mi)
	return mi

# A flat XZ grid centred at local origin, side `span`, `res` cells per side. y filled by shader.
func _make_grid_mesh(span: float, res: int) -> ArrayMesh:
	var half := span * 0.5
	var cell := span / float(res)
	var n := res + 1
	var verts := PackedVector3Array()
	for iz in range(n):
		for ix in range(n):
			verts.append(Vector3(-half + ix * cell, 0.0, -half + iz * cell))
	var idx := PackedInt32Array()
	for cz in range(res):
		for cx in range(res):
			var v00 := cz * n + cx
			var v10 := cz * n + cx + 1
			var v01 := (cz + 1) * n + cx
			var v11 := (cz + 1) * n + cx + 1
			idx.append(v00); idx.append(v01); idx.append(v11)
			idx.append(v00); idx.append(v11); idx.append(v10)
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = verts
	arrays[Mesh.ARRAY_INDEX] = idx
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	return mesh

func _process(delta: float) -> void:
	if _hud == null or _camera == null:
		return
	var p: Vector3 = _camera.global_position

	# Step 4+: drive the streamer + moving 3x3 from the camera each frame.
	var pool_line := ""
	if _streamer != null:
		var v: Vector3 = _camera.call("get_velocity")
		if STEP == 4:
			_drive_step4(p.x, p.z, v.x, v.z)
		elif STEP == 5:
			_drive_step5(p.x, p.z, v.x, v.z)
		elif STEP == 6 or STEP == 7:
			_drive_step6(p.x, p.z, v.x, v.z)   # generic N-level morph drive
		var st: Dictionary = _pool.call("stats")
		pool_line = "\nresident %d   created %d   recomputed %d" % [
			int(st.get("resident", 0)), int(st.get("created", 0)), int(st.get("recomputed", 0))]
		if STEP == 5 or STEP == 6:
			pool_line += "\ntile flips %d   last: %s" % [_flip_count, _last_flip]

	# Step 7 = the perf acceptance: live frame-time p99 (target < 6 ms at ~1000 m/s).
	var perf_line := ""
	if STEP == 7:
		_ft_window.append(delta * 1000.0)
		if _ft_window.size() > FT_WINDOW:
			_ft_window.remove_at(0)
		if _ft_window.size() >= 30:
			var sorted := _ft_window.duplicate()
			sorted.sort()
			var p99: float = sorted[int(sorted.size() * 0.99) - 1]
			var mx: float = sorted[sorted.size() - 1]
			var ok := "OK" if p99 < 6.0 else "OVER"
			perf_line = "\nframe p99 %.2f ms  max %.2f ms  [%s, target <6ms @ ~1000 m/s]" % [p99, mx, ok]

	var desc := ""
	match STEP:
		1: desc = "STEP 1: ONE level-0 page, ONE flat tile, morph OFF, no streamer"
		2: desc = "STEP 2: TWO adjacent pages — fly the seam at x=%.0f (look for a crack)" % BASE_SPAN
		3: desc = "STEP 3: 3x3 of level-0 pages — one surface? any internal grid lines?"
		4: desc = "STEP 4: STREAMER drives a moving 3x3 — fly fast; watch for churn/flicker"
		5: desc = "STEP 5: 2 levels — coarse blanket UNDER fine. Fly fast: edge should NOT wink (coarse shows through)"
		6: desc = "STEP 6: geomorph ON — fine fades to coarse at the LOD boundary. The step-5 line should be GONE"
		7: desc = "STEP 7: 3 levels, full pipeline — SPRINT (Shift) to ~1000 m/s; p99 must stay <6ms, surface continuous"
		_: desc = "STEP %d" % STEP
	_hud.text = "PROVING GROUND  STEP %d\nfps %d   cam (%.0f, %.0f, %.0f)\n%s%s%s" % [
		STEP, Engine.get_frames_per_second(), p.x, p.y, p.z, desc, pool_line, perf_line]
