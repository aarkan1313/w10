extends SceneTree

# M3 slice 4 gate: assemble Wg10ClipmapRings, bind real DEM pages per level, render
# TOP-DOWN ORTHO, and assert: (1) no holes (nonblack ~1 over terrain), (2) real relief
# (distinct colors), (3) seam continuity across the level-0/level-1 boundary (no crack),
# (4) morph continuity (no hard color jump across the seam), (5) recenter doesn't rebuild
# (vertex count unchanged after a camera move). Saves m3_rings.png. WINDOWED.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE    := "terrain_pack.gate.json"
const GLSL         := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER       := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const PAGE_PX      := 256
const SEED         := 1337
const NUM_LEVELS   := 2
const BASE_SPAN    := 8192.0
const GRID_RES     := 64
const MORPH_REGION := 0.15
const HEIGHT_SCALE := 0.35
const RELIEF_REF   := 2000.0
const CAPACITY     := 8
const VIEW_SIZE    := Vector2i(512, 512)
const MIN_DISTINCT := 8

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10ClipmapRings"):
		push_error("Wg10ClipmapRings not registered"); return 1
	if not ClassDB.class_exists("Wg10PagePool"):
		push_error("Wg10PagePool not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-m3-rings] status=skip reason=no-render-device"); return 2

	var pack_os: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os: String = ProjectSettings.globalize_path(GLSL)

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var cfg_err: String = str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if cfg_err != "":
		push_error("pool configure failed: %s" % cfg_err); return 1

	# One page per level at the camera origin. Level L span = BASE_SPAN*2^L; the page for
	# level L at origin has origin (0,0) in that level's grid.
	var tex0 = pool.call("acquire_page", 0, 0.0, 0.0)
	var tex1 = pool.call("acquire_page", 1, 0.0, 0.0)
	if tex0 == null or tex1 == null:
		push_error("acquire_page returned null"); return 1

	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	if int(rings.call("level_count")) != NUM_LEVELS:
		push_error("expected %d levels, got %s" % [NUM_LEVELS, str(rings.call("level_count"))]); return 1

	# level 0 morphs toward level 1 (coarse); level 1 coarsest -> no morph (coarse=self, region 0).
	rings.call("bind_page", 0, tex0, tex1, BASE_SPAN, BASE_SPAN * 2.0, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF)
	rings.call("bind_page", 1, tex1, tex1, BASE_SPAN * 2.0, BASE_SPAN * 2.0, HEIGHT_SCALE, 0.0, RELIEF_REF)

	var verts_before := int(rings.call("total_vertex_count"))

	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true

	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.0, 0.0, 0.0)   # BLACK bg so holes read as black

	var cam := Camera3D.new()
	cam.projection = Camera3D.PROJECTION_ORTHOGONAL
	cam.size = BASE_SPAN * 2.0            # ortho height = coarsest span (frame fills with terrain)
	cam.far = BASE_SPAN * 8.0
	cam.environment = env

	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-90.0, 0.0, 0.0)  # straight down
	light.light_energy = 1.2

	vp.add_child(rings)
	vp.add_child(cam)
	vp.add_child(light)
	get_root().add_child(vp)

	# top-down: eye above origin looking straight down (-Y); up = -Z so +X is right.
	cam.look_at_from_position(Vector3(0.0, BASE_SPAN * 2.0, 0.0), Vector3.ZERO, Vector3(0.0, 0.0, -1.0))

	rings.call("recenter", 0.0, 0.0)

	for i in range(8):
		await process_frame
	RenderingServer.force_draw()
	await process_frame

	var img := vp.get_texture().get_image()
	if img == null:
		push_error("get_image() returned null"); return 1
	img.save_png("user://m3_rings.png")

	var errs: Array[String] = []

	# (1) no holes + (2) real relief
	var distinct := {}
	var nonblack := 0
	var total := img.get_width() * img.get_height()
	for y in range(img.get_height()):
		for x in range(img.get_width()):
			var c := img.get_pixel(x, y)
			if not (is_finite(c.r) and is_finite(c.g) and is_finite(c.b)):
				push_error("non-finite pixel @ %d,%d" % [x,y]); return 1
			if c.r > 0.03 or c.g > 0.03 or c.b > 0.03:
				nonblack += 1
			distinct[Vector3i(int(c.r*16), int(c.g*16), int(c.b*16))] = true
	var frac := float(nonblack) / float(total)
	if frac < 0.95:
		errs.append("holes: nonblack_frac=%.3f < 0.95 (gap/crack shows as black)" % frac)
	if distinct.size() < MIN_DISTINCT:
		errs.append("no relief: distinct=%d < %d" % [distinct.size(), MIN_DISTINCT])

	# (3) seam continuity + (4) morph continuity: the level-0/level-1 boundary is at
	# |world x| = BASE_SPAN/2. Frame spans the coarsest span (2*BASE_SPAN) so the boundary
	# lands at 1/4 and 3/4 across the frame. Assert no black gap and no hard color jump.
	var midy := img.get_height() / 2
	var boundary_cols := [img.get_width()/4, (img.get_width()*3)/4]
	for bx in boundary_cols:
		var black_run := 0
		for dx in range(-2, 3):
			var c := img.get_pixel(int(clamp(bx + dx, 0, img.get_width()-1)), midy)
			if c.r <= 0.03 and c.g <= 0.03 and c.b <= 0.03:
				black_run += 1
		if black_run > 0:
			errs.append("seam crack: %d black px at boundary col %d (level-0/1 seam)" % [black_run, bx])
		var c_in := img.get_pixel(int(clamp(bx - 2, 0, img.get_width()-1)), midy)
		var c_out := img.get_pixel(int(clamp(bx + 2, 0, img.get_width()-1)), midy)
		var dr: float = abs(c_in.r - c_out.r) + abs(c_in.g - c_out.g) + abs(c_in.b - c_out.b)
		if dr > 0.5:
			errs.append("morph discontinuity: color jump %.2f across boundary col %d" % [dr, bx])

	# (5) recenter doesn't rebuild
	rings.call("recenter", 3000.0, -1500.0)
	var verts_after := int(rings.call("total_vertex_count"))
	if verts_after != verts_before:
		errs.append("recenter rebuilt mesh: verts %d -> %d" % [verts_before, verts_after])

	pool.call("free_all")

	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m3-rings] status=fail errors=%d nonblack=%.3f distinct=%d" % [errs.size(), frac, distinct.size()])
		return 1
	print("[wg10-m3-rings] status=pass nonblack=%.3f distinct=%d verts=%d" % [frac, distinct.size(), verts_before])
	return 0
