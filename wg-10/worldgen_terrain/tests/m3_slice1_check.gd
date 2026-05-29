extends SceneTree
# M3 slice 1 gate: render one DEM page on a ring, assert the frame shows real
# relief (distinct colors present, non-black, finite). Value-correctness is proven
# by the M2 gpu parity gate (same formula); this proves the render path. WINDOWED.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE    := "terrain_pack.gate.json"
const GLSL         := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER_SPATIAL := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const WORLD_SPAN   := 8192.0
const PAGE_PX      := 256
const GRID_RES     := 128
const HEIGHT_SCALE := 0.35
const RELIEF_REF   := 2000.0
const SEED         := 1337
const VIEW_SIZE    := Vector2i(640, 480)
const MIN_DISTINCT     := 8
const MIN_NONBLACK_FRAC := 0.10

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10PagePool"):
		push_error("Wg10PagePool not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-m3-slice1] status=skip reason=no-render-device"); return 2

	# --- acquire the height page from the pool ---
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var pack_os: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os: String = ProjectSettings.globalize_path(GLSL)
	var cfg_err: String = str(pool.call("configure", pack_os, PACK_FILE, glsl_os, 4, PAGE_PX, WORLD_SPAN, SEED))
	if cfg_err != "":
		push_error("[wg10-m3-slice1] pool configure failed: %s" % cfg_err); return 1
	var tex = pool.call("acquire_page", 0, 0.0, 0.0)
	if tex == null:
		push_error("[wg10-m3-slice1] acquire_page returned null"); return 1

	# --- build SubViewport (UPDATE_ALWAYS + own_world_3d) ---
	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true

	# --- plane mesh covering the page span ---
	var plane := PlaneMesh.new()
	plane.size = Vector2(WORLD_SPAN, WORLD_SPAN)
	plane.subdivide_width = GRID_RES
	plane.subdivide_depth = GRID_RES

	# --- ShaderMaterial using ring_displace.gdshader ---
	var mat := ShaderMaterial.new()
	var shader := load(SHADER_SPATIAL) as Shader
	if shader == null:
		push_error("[wg10-m3-slice1] failed to load shader: %s" % SHADER_SPATIAL); return 1
	mat.shader = shader
	mat.set_shader_parameter("height_tex",   tex)
	mat.set_shader_parameter("world_span",   WORLD_SPAN)
	mat.set_shader_parameter("height_scale", HEIGHT_SCALE)
	mat.set_shader_parameter("relief_ref",   RELIEF_REF)

	var mi := MeshInstance3D.new()
	mi.mesh = plane
	mi.material_override = mat

	# --- DirectionalLight3D ---
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-45.0, 30.0, 0.0)
	light.light_energy = 1.2

	# --- Camera3D with Environment ---
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.35, 0.55, 0.85)

	var cam := Camera3D.new()
	cam.far = WORLD_SPAN * 4.0
	cam.environment = env

	# --- assemble scene tree FIRST, then position camera ---
	vp.add_child(mi)
	vp.add_child(light)
	vp.add_child(cam)
	get_root().add_child(vp)

	# Camera vantage: steep oblique to fill most of the frame with terrain.
	var eye_y := WORLD_SPAN * 0.5
	var eye_z := WORLD_SPAN * 0.5
	var eye_pos := Vector3(0.0, eye_y, eye_z)
	var look_target := Vector3(0.0, 0.0, 0.0)
	cam.look_at_from_position(eye_pos, look_target, Vector3.UP)

	# --- settle 8 frames + force_draw + 1 frame (probe sequence) ---
	for i in range(8):
		await process_frame
	RenderingServer.force_draw()
	await process_frame

	# --- capture and validate ---
	var img := vp.get_texture().get_image()
	if img == null:
		push_error("[wg10-m3-slice1] get_image() returned null"); return 1

	var distinct := {}
	var nonblack := 0
	var total := img.get_width() * img.get_height()
	for y in range(img.get_height()):
		for x in range(img.get_width()):
			var c := img.get_pixel(x, y)
			if not (is_finite(c.r) and is_finite(c.g) and is_finite(c.b)):
				push_error("non-finite pixel @ %d,%d" % [x, y]); return 1
			if c.r > 0.03 or c.g > 0.03 or c.b > 0.03:
				nonblack += 1
			distinct[Vector3i(int(c.r * 16), int(c.g * 16), int(c.b * 16))] = true
	var frac := float(nonblack) / float(total)
	# NOTE: with a colored sky background, nonblack_frac ~1.0 (sky everywhere) — it only
	# rules out a fully-black frame. The REAL relief signal is `distinct`: a flat/failed
	# surface yields few distinct colors; real DEM relief yields many height-gradient bands.
	if distinct.size() < MIN_DISTINCT or frac < MIN_NONBLACK_FRAC:
		push_error("frame lacks relief: distinct=%d nonblack_frac=%.3f" % [distinct.size(), frac])
		print("[wg10-m3-slice1] status=fail distinct=%d nonblack_frac=%.3f" % [distinct.size(), frac])
		return 1
	print("[wg10-m3-slice1] status=pass distinct=%d nonblack_frac=%.3f" % [distinct.size(), frac])
	return 0
