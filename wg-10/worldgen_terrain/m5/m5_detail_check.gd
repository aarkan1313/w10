extends SceneTree

# M5 Slice 1 gate — fBm uniform detail: bounded + edge-safe-with-detail + base-untouched.
# WINDOWED ONLY (global RenderingDevice is null headless on this D3D12 box).
#
# Three invariants, all observed from the rendered output (GDScript can't run the shader's
# fbm directly, so we prove them at the render boundary, not by mirroring the noise in CPU):
#   (1) BOUNDED     — detail-on does not blow the surface past the height-color range
#                     (saturated-pixel fraction stays low). |detail| <= wg_detail_amp by
#                     construction (fbm normalized to [-1,1]); the capture confirms no blowup.
#   (2) EDGE-SAFE   — two ABUTTING tiles (page at (0,0) and (SPAN,0)), each with its correct
#                     page_origin, agree along the shared world seam (x==SPAN) within a tight
#                     luma epsilon — because detail is a pure function of world XZ. If detail
#                     were page-local, the seam columns would diverge. (The M3 seam contract
#                     must survive M5.)
#   (3) NON-VACUOUS — detail-on differs from detail-off (detail genuinely displaces; the gate
#                     can't pass on a no-op).

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const WORLD_SPAN := 8192.0
const PAGE_PX := 256
const GRID_RES := 128
const HEIGHT_SCALE := 0.35
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
		return 1

	var diff := _mean_abs_diff(off_img, on_img)
	var non_vacuous := diff > 0.002
	var sat := _saturated_frac(on_img)
	var bounded := sat < 0.20
	var edge_safe := await _edge_safe(DETAIL_AMP)

	var ok := non_vacuous and bounded and edge_safe
	print("[wg10-m5] non_vacuous=%s (diff=%.4f) bounded=%s (sat=%.3f) edge_safe=%s -> %s" % [
		non_vacuous, diff, bounded, sat, edge_safe, "PASS" if ok else "FAIL"])
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
	var mat := ShaderMaterial.new()
	mat.shader = load(SHADER)
	mat.set_shader_parameter("height_tex", tex)
	mat.set_shader_parameter("coarse_height_tex", tex)
	mat.set_shader_parameter("world_span", WORLD_SPAN)
	mat.set_shader_parameter("coarse_span", WORLD_SPAN)
	mat.set_shader_parameter("height_scale", HEIGHT_SCALE)
	mat.set_shader_parameter("morph_region", 0.0)
	mat.set_shader_parameter("relief_ref", RELIEF_REF)
	mat.set_shader_parameter("page_origin", Vector2(0.0, 0.0))
	mat.set_shader_parameter("coarse_origin", Vector2(0.0, 0.0))
	mat.set_shader_parameter("level_center", Vector2(WORLD_SPAN * 0.5, WORLD_SPAN * 0.5))
	mat.set_shader_parameter("level_half_extent", WORLD_SPAN * 1.5)
	mi.material_override = mat
	vp.add_child(mi)
	get_root().add_child(vp)
	await process_frame
	await process_frame
	RenderingServer.force_draw()
	var img := vp.get_texture().get_image()
	vp.queue_free()
	return img

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
	var seam_max := await _capture_strip(amp)
	print("[wg10-m5]   edge seam_max_luma_delta=%.5f" % seam_max)
	return seam_max < 0.01

func _capture_strip(amp: float) -> float:
	RenderingServer.global_shader_parameter_set("wg_detail_amp", amp)
	var pool2: Object = ClassDB.instantiate("Wg10PagePool")
	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	pool2.call("configure", pack_os, PACK_FILE, glsl_os, 4, PAGE_PX, WORLD_SPAN, SEED)
	var ta = pool2.call("acquire_page", 0, 0.0, 0.0)
	var tb = pool2.call("acquire_page", 0, WORLD_SPAN, 0.0)
	if ta == null or tb == null:
		return 999.0
	var vp := SubViewport.new()
	vp.size = Vector2i(1024, 512)
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	vp.world_3d = World3D.new()
	var envh := Environment.new()
	envh.background_mode = Environment.BG_COLOR
	envh.background_color = Color.BLACK
	var cam := Camera3D.new()
	cam.projection = Camera3D.PROJECTION_ORTHOGONAL
	cam.size = WORLD_SPAN * 2.0
	cam.position = Vector3(WORLD_SPAN, 5000.0, WORLD_SPAN * 0.5)
	cam.rotation_degrees = Vector3(-90.0, 0.0, 0.0)
	cam.far = 20000.0
	cam.environment = envh
	vp.add_child(cam)
	_add_strip_tile(vp, ta, 0.0)
	_add_strip_tile(vp, tb, WORLD_SPAN)
	get_root().add_child(vp)
	await process_frame
	await process_frame
	RenderingServer.force_draw()
	var img := vp.get_texture().get_image()
	var col := img.get_width() / 2
	var m := 0.0
	var y := 0
	while y < img.get_height():
		var l := img.get_pixel(col - 1, y).v
		var r := img.get_pixel(col + 1, y).v
		m = maxf(m, absf(l - r))
		y += 1
	vp.queue_free()
	return m

func _add_strip_tile(vp: SubViewport, tex, origin_x: float) -> void:
	var mesh := PlaneMesh.new()
	mesh.size = Vector2(WORLD_SPAN, WORLD_SPAN)
	mesh.subdivide_width = GRID_RES
	mesh.subdivide_depth = GRID_RES
	var mi := MeshInstance3D.new()
	mi.mesh = mesh
	mi.position = Vector3(origin_x + WORLD_SPAN * 0.5, 0.0, WORLD_SPAN * 0.5)
	var mat := ShaderMaterial.new()
	mat.shader = load(SHADER)
	mat.set_shader_parameter("height_tex", tex)
	mat.set_shader_parameter("coarse_height_tex", tex)
	mat.set_shader_parameter("world_span", WORLD_SPAN)
	mat.set_shader_parameter("coarse_span", WORLD_SPAN)
	mat.set_shader_parameter("height_scale", HEIGHT_SCALE)
	mat.set_shader_parameter("morph_region", 0.0)
	mat.set_shader_parameter("relief_ref", RELIEF_REF)
	mat.set_shader_parameter("page_origin", Vector2(origin_x, 0.0))
	mat.set_shader_parameter("coarse_origin", Vector2(origin_x, 0.0))
	mat.set_shader_parameter("level_center", Vector2(origin_x + WORLD_SPAN * 0.5, WORLD_SPAN * 0.5))
	mat.set_shader_parameter("level_half_extent", WORLD_SPAN * 1.5)
	mi.material_override = mat
	vp.add_child(mi)
