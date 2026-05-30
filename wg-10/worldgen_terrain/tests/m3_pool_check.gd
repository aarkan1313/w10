extends SceneTree

# M3 slice 2 gate: drive the page pool's acquire/release and assert the WG9-killer
# behaviors hold at the godot/RID layer (reuse on hit, budget respected, protected
# survives, Full is clean), AND a pooled page still renders. WINDOWED.
# The eviction LOGIC is proven exhaustively by the headless page_policy cargo tests;
# this proves the RID layer + render wiring.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const WORLD_SPAN := 8192.0
const PAGE_PX := 256
const GRID_RES := 128
const RELIEF_SCALE := 0.25
const SEED := 1337
const CAPACITY := 2
const VIEW_SIZE := Vector2i(640, 480)
const MIN_DISTINCT := 8

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10PagePool"):
		push_error("Wg10PagePool not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-m3-pool] status=skip reason=no-render-device"); return 2
	var os_dir: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var os_glsl: String = ProjectSettings.globalize_path(GLSL)
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var err: String = str(pool.call("configure", os_dir, PACK_FILE, os_glsl, CAPACITY, PAGE_PX, WORLD_SPAN, SEED))
	if err != "":
		push_error("pool configure failed: %s" % err); return 1

	var errs: Array[String] = []
	# distinct page origins, page-aligned (span apart)
	var a = pool.call("acquire_page", 0, 0.0, 0.0)         # slot 0 (created)
	var b = pool.call("acquire_page", 0, WORLD_SPAN, 0.0)  # slot 1 (created)
	if a == null or b == null: push_error("acquire returned null"); return 1
	var s1: Dictionary = pool.call("stats")
	if int(s1.get("created", 0)) != 2: errs.append("expected created=2 got %s" % str(s1))
	if int(s1.get("resident", 0)) != 2: errs.append("expected resident=2 got %s" % str(s1))

	# cache hit: re-acquire key A (still protected) -> reuse, no new create
	pool.call("acquire_page", 0, 0.0, 0.0)
	var s2: Dictionary = pool.call("stats")
	if int(s2.get("created", 0)) != 2: errs.append("hit should not create: %s" % str(s2))
	if int(s2.get("reused", 0)) < 1: errs.append("hit should increment reused: %s" % str(s2))

	# both A and B currently protected (never released). Acquire a 3rd key -> Full.
	var c = pool.call("acquire_page", 0, WORLD_SPAN * 2.0, 0.0)
	var s3: Dictionary = pool.call("stats")
	if int(s3.get("full_events", 0)) < 1: errs.append("all-protected acquire should be Full: %s" % str(s3))
	if int(s3.get("resident", 0)) != 2: errs.append("Full must not change residency: %s" % str(s3))
	if c != null: errs.append("Full acquire should return null")

	# release B, then acquire the 3rd key -> evicts B's slot (recompute, no create)
	pool.call("release_page", 0, WORLD_SPAN, 0.0)
	var d = pool.call("acquire_page", 0, WORLD_SPAN * 2.0, 0.0)
	if d == null: push_error("acquire after release returned null"); return 1
	var s4: Dictionary = pool.call("stats")
	if int(s4.get("created", 0)) != 2: errs.append("eviction must reuse a slot texture, not create: %s" % str(s4))
	if int(s4.get("recomputed", 0)) < 1: errs.append("eviction should increment recomputed: %s" % str(s4))
	if int(s4.get("resident", 0)) != 2: errs.append("budget exceeded: %s" % str(s4))

	# a pooled page still RENDERS: build the slice scene with page A's texture
	var tex = pool.call("acquire_page", 0, 0.0, 0.0)
	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	get_root().add_child(vp)

	var plane := PlaneMesh.new()
	plane.size = Vector2(WORLD_SPAN, WORLD_SPAN)
	plane.subdivide_width = GRID_RES
	plane.subdivide_depth = GRID_RES

	var mi := MeshInstance3D.new()
	mi.mesh = plane

	var mat := ShaderMaterial.new()
	mat.shader = load("res://worldgen_terrain/shaders/ring_displace.gdshader")
	mat.set_shader_parameter("height_tex", tex)
	mat.set_shader_parameter("world_span", WORLD_SPAN)
	mat.set_shader_parameter("relief_scale", RELIEF_SCALE)
	mi.material_override = mat

	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-45.0, 30.0, 0.0)
	light.light_energy = 1.2

	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.35, 0.55, 0.85)

	var cam := Camera3D.new()
	cam.far = WORLD_SPAN * 4.0
	cam.environment = env

	vp.add_child(mi)
	vp.add_child(light)
	vp.add_child(cam)

	var eye_y := WORLD_SPAN * 0.5
	var eye_z := WORLD_SPAN * 0.5
	var eye_pos := Vector3(0.0, eye_y, eye_z)
	cam.look_at_from_position(eye_pos, Vector3.ZERO, Vector3.UP)

	for i in range(8):
		await process_frame
	RenderingServer.force_draw()
	await process_frame

	var img := vp.get_texture().get_image()
	var distinct := {}
	for y in range(img.get_height()):
		for x in range(img.get_width()):
			var col := img.get_pixel(x, y)
			distinct[Vector3i(int(col.r * 16), int(col.g * 16), int(col.b * 16))] = true
	if distinct.size() < MIN_DISTINCT: errs.append("pooled page did not render relief: distinct=%d" % distinct.size())

	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m3-pool] status=fail errors=%d stats=%s" % [errs.size(), str(pool.call("stats"))])
		return 1
	print("[wg10-m3-pool] status=pass created=%d reused=%d recomputed=%d full=%d distinct=%d" % [
		int(s4.get("created", 0)), int(s4.get("reused", 0)), int(s4.get("recomputed", 0)), int(s4.get("full_events", 0)), distinct.size()])
	return 0
