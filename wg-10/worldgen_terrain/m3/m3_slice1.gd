extends SceneTree

# M3 Slice 1 — first visual: compute one DEM height page, displace a ring mesh, capture PNG.
# WINDOWED ONLY (global RD is null headless).
# Probe-config baked in: UPDATE_ALWAYS + own_world_3d + Environment + settle+force_draw+frame.

# ---- config (pillar 1) ----
const PACK_RES_DIR  := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE     := "terrain_pack.gate.json"
const GLSL          := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER_SPATIAL := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const WORLD_SPAN    := 8192.0
const PAGE_PX       := 256
const GRID_RES      := 128
const RELIEF_SCALE  := 0.25
const RELIEF_REF    := 2000.0
const SEED          := 1337
const VIEW_SIZE     := Vector2i(640, 480)
const OUT_PNG       := "res://worldgen_terrain/tests/m3_slice1.png"

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10PagePool"):
		push_error("[wg10-m3-slice1] Wg10PagePool not registered — run WINDOWED (not headless)")
		return 1
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var pack_os: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os: String = ProjectSettings.globalize_path(GLSL)
	var cfg_err: String = str(pool.call("configure", pack_os, PACK_FILE, glsl_os, 4, PAGE_PX, WORLD_SPAN, SEED))
	if cfg_err != "":
		push_error("[wg10-m3-slice1] pool configure failed: %s" % cfg_err)
		return 1
	var tex = pool.call("acquire_page", 0, 0.0, 0.0)
	if tex == null:
		push_error("[wg10-m3-slice1] acquire_page returned null")
		return 1

	# --- build SubViewport (probe config: UPDATE_ALWAYS + own_world_3d) ---
	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true

	# --- plane mesh covering the page span ---
	var plane := PlaneMesh.new()
	plane.size = Vector2(WORLD_SPAN, WORLD_SPAN)
	plane.subdivide_width  = GRID_RES
	plane.subdivide_depth  = GRID_RES

	# --- ShaderMaterial using ring_displace.gdshader ---
	var mat := ShaderMaterial.new()
	var shader := load(SHADER_SPATIAL) as Shader
	if shader == null:
		push_error("[wg10-m3-slice1] failed to load shader: %s" % SHADER_SPATIAL)
		return 1
	mat.shader = shader
	mat.set_shader_parameter("height_tex",   tex)
	mat.set_shader_parameter("world_span",   WORLD_SPAN)
	mat.set_shader_parameter("relief_scale", RELIEF_SCALE)
	mat.set_shader_parameter("relief_ref",   RELIEF_REF)

	var mi := MeshInstance3D.new()
	mi.mesh = plane
	mi.material_override = mat

	# --- DirectionalLight3D ---
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-45.0, 30.0, 0.0)
	light.light_energy = 1.2

	# --- Camera3D with Environment (probe-required for nonblack output) ---
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.35, 0.55, 0.85)

	var cam := Camera3D.new()
	cam.far = WORLD_SPAN * 4.0
	cam.environment = env

	# --- assemble scene tree FIRST, then look_at (node must be in tree) ---
	vp.add_child(mi)
	vp.add_child(light)
	vp.add_child(cam)
	get_root().add_child(vp)

	# Camera vantage: steep oblique to fill most of the frame with terrain.
	# look_at_from_position works without requiring the node to be in the tree.
	var eye_y := WORLD_SPAN * 0.5
	var eye_z := WORLD_SPAN * 0.5
	var eye_pos := Vector3(0.0, eye_y, eye_z)
	# Aim at center of the page — steeper angle, horizon moves out of frame
	var look_target := Vector3(0.0, 0.0, 0.0)
	cam.look_at_from_position(eye_pos, look_target, Vector3.UP)

	# --- settle frames, force_draw, one more frame (probe sequence) ---
	for i in range(8):
		await process_frame
	RenderingServer.force_draw()
	await process_frame

	# --- capture ---
	var img := vp.get_texture().get_image()
	if img == null:
		push_error("[wg10-m3-slice1] get_image() returned null")
		return 1

	var save_err := img.save_png(OUT_PNG)
	if save_err != OK:
		push_error("[wg10-m3-slice1] save_png failed: %d" % save_err)
		return 1

	print("[wg10-m3-slice1] captured %s (%dx%d)" % [OUT_PNG, img.get_width(), img.get_height()])
	return 0
