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

const STEP := 3
const PAGE_PX := 256
const SEED := 1337
const BASE_SPAN := 8192.0     # one level-0 page spans this many metres
const GRID_RES := 64          # mesh cells across the page (vertices = 65x65)
const CAPACITY := 16    # step 3 needs 9 resident pages; margin to spare
const HEIGHT_SCALE := 0.35
const RELIEF_REF := 2000.0

var _pool: Object
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
		_:
			_camera.global_position = Vector3(BASE_SPAN, 2500.0, BASE_SPAN * 0.5)
	_camera.rotation_degrees = Vector3(-50.0, 0.0, 0.0)

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
	mat.set_shader_parameter("height_scale", HEIGHT_SCALE)
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

func _process(_delta: float) -> void:
	if _hud == null or _camera == null:
		return
	var p: Vector3 = _camera.global_position
	var desc := ""
	match STEP:
		1: desc = "STEP 1: ONE level-0 page, ONE flat tile, morph OFF, no streamer"
		2: desc = "STEP 2: TWO adjacent pages — fly the seam at x=%.0f (look for a crack)" % BASE_SPAN
		3: desc = "STEP 3: 3x3 of level-0 pages — one surface? any internal grid lines?"
		_: desc = "STEP %d" % STEP
	_hud.text = "PROVING GROUND  STEP %d\nfps %d   cam (%.0f, %.0f, %.0f)\n%s" % [
		STEP, Engine.get_frames_per_second(), p.x, p.y, p.z, desc]
