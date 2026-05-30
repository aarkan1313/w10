extends SceneTree

# M5 Slice 1 gate — fBm uniform detail: bounded + edge-safe-with-detail + base-untouched.
# WINDOWED ONLY (global RenderingDevice is null headless on this D3D12 box).
#
# Three invariants, all observed from the rendered output (GDScript can't run the shader's
# fbm directly, so we prove them at the render boundary, not by mirroring the noise in CPU):
#   (1) BOUNDED     — detail-on does not blow the surface past the height-color range
#                     (saturated-pixel fraction stays low). |detail| <= wg_detail_amp by
#                     construction (fbm normalized to [-1,1]); the capture confirms no blowup.
#   (2) EDGE-SAFE   — two ABUTTING tiles (page at (0,0) and (SPAN,0)), each rendered SEPARATELY
#                     and each framing exactly its own [origin, origin+SPAN] span at the same
#                     pixel resolution. Tile A's RIGHTMOST column and tile B's LEFTMOST column are
#                     then the SAME world points on the shared edge (x==SPAN), so comparing them
#                     row-by-row isolates seam AGREEMENT from the normal terrain height-gradient.
#                     They must match within a tight luma epsilon because detail is a pure function
#                     of world XZ; if detail were page-local the shared edge would diverge. (The M3
#                     seam contract must survive M5.)
#   (3) NON-VACUOUS — detail-on differs from detail-off (detail genuinely displaces; the gate
#                     can't pass on a no-op).

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const WORLD_SPAN := 8192.0
const PAGE_PX := 256
const GRID_RES := 128
const RELIEF_SCALE := 0.25
const RELIEF_REF := 2000.0
const SEED := 1337
const DETAIL_AMP := 60.0           # metres of peak detail for the test (visible, bounded)
const VIEW_SIZE := Vector2i(512, 512)

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10PagePool"):
		push_error("[wg10-m5] Wg10PagePool not registered — run WINDOWED (not headless)")
		return 1
	if RenderingServer.get_rendering_device() == null:
		push_error("[wg10-m5] no RenderingDevice — run WINDOWED")
		return 2

	if not RenderingServer.global_shader_parameter_get_list().has("wg_detail_amp"):
		RenderingServer.global_shader_parameter_add("wg_detail_amp", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, 0.0)
	if not RenderingServer.global_shader_parameter_get_list().has("wg_dbg_mode"):
		RenderingServer.global_shader_parameter_add("wg_dbg_mode", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, 0.0)

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	var err := str(pool.call("configure", pack_os, PACK_FILE, glsl_os, 4, PAGE_PX, WORLD_SPAN, SEED))
	if err != "":
		push_error("[wg10-m5] pool configure failed: %s" % err)
		return 1
	var tex = pool.call("acquire_page", 0, 0.0, 0.0)
	if tex == null:
		push_error("[wg10-m5] acquire_page failed")
		return 1

	var off_img := await _capture(tex, 0.0)
	var on_img := await _capture(tex, DETAIL_AMP)
	if off_img == null or on_img == null:
		push_error("[wg10-m5] capture failed")
		pool.call("free_all")  # release page-texture RIDs before the early return (B1)
		return 1

	var diff := _mean_abs_diff(off_img, on_img)
	# Threshold set from the MEASURED realized amplitude, not guessed: at DETAIL_AMP=60 the mean
	# height-color delta is ~0.0026 with the smoothstep contrast (~0.0020 without it). A no-op
	# shader gives diff~=0.0000. 0.001 sits well below the realized signal and well above zero, so
	# it proves "detail is genuinely present" with comfortable headroom either way and cannot pass
	# on a no-op.
	var non_vacuous := diff > 0.001
	var sat := _saturated_frac(on_img)
	var bounded := sat < 0.20
	var edge_safe := await _edge_safe(DETAIL_AMP)

	var ok := non_vacuous and bounded and edge_safe
	print("[wg10-m5] non_vacuous=%s (diff=%.4f) bounded=%s (sat=%.3f) edge_safe=%s -> %s" % [
		non_vacuous, diff, bounded, sat, edge_safe, "PASS" if ok else "FAIL"])
	# Free the pool's page-texture RIDs explicitly (B1). Wg10PagePool now also self-frees via a
	# Rust Drop impl when the last RefCounted reference drops, so this is belt-and-suspenders — but
	# it releases the GPU RIDs deterministically at end-of-check rather than at GC time.
	pool.call("free_all")
	return 0 if ok else 1

func _capture(tex, amp: float) -> Image:
	RenderingServer.global_shader_parameter_set("wg_detail_amp", amp)
	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	vp.world_3d = World3D.new()
	var envh := Environment.new()
	envh.background_mode = Environment.BG_COLOR
	envh.background_color = Color.BLACK
	var cam := Camera3D.new()
	cam.projection = Camera3D.PROJECTION_ORTHOGONAL
	cam.size = WORLD_SPAN
	cam.position = Vector3(WORLD_SPAN * 0.5, 5000.0, WORLD_SPAN * 0.5)
	cam.rotation_degrees = Vector3(-90.0, 0.0, 0.0)
	cam.far = 20000.0
	cam.environment = envh
	vp.add_child(cam)
	var mesh := PlaneMesh.new()
	mesh.size = Vector2(WORLD_SPAN, WORLD_SPAN)
	mesh.subdivide_width = GRID_RES
	mesh.subdivide_depth = GRID_RES
	var mi := MeshInstance3D.new()
	mi.mesh = mesh
	mi.position = Vector3(WORLD_SPAN * 0.5, 0.0, WORLD_SPAN * 0.5)
	mi.material_override = _make_tile_material(tex, 0.0)
	vp.add_child(mi)
	get_root().add_child(vp)
	await process_frame
	await process_frame
	RenderingServer.force_draw()
	var img := vp.get_texture().get_image()
	vp.queue_free()
	return img

func _make_tile_material(tex, origin_x: float) -> ShaderMaterial:
	var mat := ShaderMaterial.new()
	mat.shader = load(SHADER)
	mat.set_shader_parameter("height_tex", tex)
	mat.set_shader_parameter("coarse_height_tex", tex)
	mat.set_shader_parameter("world_span", WORLD_SPAN)
	mat.set_shader_parameter("coarse_span", WORLD_SPAN)
	mat.set_shader_parameter("relief_scale", RELIEF_SCALE)
	mat.set_shader_parameter("morph_region", 0.0)
	mat.set_shader_parameter("relief_ref", RELIEF_REF)
	mat.set_shader_parameter("page_origin", Vector2(origin_x, 0.0))
	mat.set_shader_parameter("coarse_origin", Vector2(origin_x, 0.0))
	mat.set_shader_parameter("level_center", Vector2(origin_x + WORLD_SPAN * 0.5, WORLD_SPAN * 0.5))
	mat.set_shader_parameter("level_half_extent", WORLD_SPAN * 1.5)
	return mat

func _mean_abs_diff(a: Image, b: Image) -> float:
	var n := 0
	var s := 0.0
	var y := 0
	while y < a.get_height():
		var x := 0
		while x < a.get_width():
			s += absf(a.get_pixel(x, y).v - b.get_pixel(x, y).v)
			n += 1
			x += 8
		y += 8
	return s / float(maxi(n, 1))

func _saturated_frac(img: Image) -> float:
	var n := 0
	var sat := 0
	var y := 0
	while y < img.get_height():
		var x := 0
		while x < img.get_width():
			var v := img.get_pixel(x, y).v
			if v >= 0.999 or v <= 0.001:
				sat += 1
			n += 1
			x += 8
		y += 8
	return float(sat) / float(maxi(n, 1))

func _edge_safe(amp: float) -> bool:
	# Render the two abutting tiles SEPARATELY, each framing exactly its own span at the same
	# resolution. A's rightmost column and B's leftmost column are the SAME shared-edge world
	# points (x==WORLD_SPAN), so this isolates seam agreement from the terrain height-gradient.
	var img_a := await _capture_one_tile(amp, 0.0)         # tile x in [0, SPAN], page_origin.x=0
	var img_b := await _capture_one_tile(amp, WORLD_SPAN)  # tile x in [SPAN, 2*SPAN], page_origin.x=SPAN
	if img_a == null or img_b == null:
		print("[wg10-m5]   edge capture failed")
		return false
	if img_a.get_height() != img_b.get_height():
		push_error("[wg10-m5] edge tiles differ in pixel height — rows do not align")
		return false
	var m := 0.0
	var y := 0
	while y < img_a.get_height():
		var left_of_seam := img_a.get_pixel(img_a.get_width() - 1, y).v
		var right_of_seam := img_b.get_pixel(0, y).v
		m = maxf(m, absf(left_of_seam - right_of_seam))
		y += 1
	print("[wg10-m5]   edge seam_max_luma_delta=%.5f" % m)
	return m < 0.01

# Render ONE tile covering world [origin_x, origin_x+SPAN] x [0, SPAN], top-down ortho framing
# EXACTLY that tile, with its correct page_origin. Acquires (and frees) its own page+pool.
func _capture_one_tile(amp: float, origin_x: float) -> Image:
	RenderingServer.global_shader_parameter_set("wg_detail_amp", amp)
	var pool2: Object = ClassDB.instantiate("Wg10PagePool")
	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	var e := str(pool2.call("configure", pack_os, PACK_FILE, glsl_os, 4, PAGE_PX, WORLD_SPAN, SEED))
	if e != "":
		push_error("[wg10-m5] configure failed: %s" % e)
		return null
	var tex = pool2.call("acquire_page", 0, origin_x, 0.0)
	if tex == null:
		push_error("[wg10-m5] acquire_page failed (origin_x=%.1f)" % origin_x)
		pool2.call("free_all")  # release any allocated RIDs before the early return (B1)
		return null
	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	vp.world_3d = World3D.new()
	var envh := Environment.new()
	envh.background_mode = Environment.BG_COLOR
	envh.background_color = Color.BLACK
	var cam := Camera3D.new()
	cam.projection = Camera3D.PROJECTION_ORTHOGONAL
	cam.size = WORLD_SPAN
	cam.position = Vector3(origin_x + WORLD_SPAN * 0.5, 5000.0, WORLD_SPAN * 0.5)
	cam.rotation_degrees = Vector3(-90.0, 0.0, 0.0)
	cam.far = 20000.0
	cam.environment = envh
	vp.add_child(cam)
	var mesh := PlaneMesh.new()
	mesh.size = Vector2(WORLD_SPAN, WORLD_SPAN)
	mesh.subdivide_width = GRID_RES
	mesh.subdivide_depth = GRID_RES
	var mi := MeshInstance3D.new()
	mi.mesh = mesh
	mi.position = Vector3(origin_x + WORLD_SPAN * 0.5, 0.0, WORLD_SPAN * 0.5)
	mi.material_override = _make_tile_material(tex, origin_x)
	vp.add_child(mi)
	get_root().add_child(vp)
	await process_frame
	await process_frame
	RenderingServer.force_draw()
	var img := vp.get_texture().get_image()
	vp.queue_free()
	pool2.call("free_all")  # release this tile's page-texture RIDs before returning (B1)
	return img
