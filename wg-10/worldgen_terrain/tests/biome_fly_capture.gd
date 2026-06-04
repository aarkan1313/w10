extends SceneTree

# One-shot CAPTURE: stream the live biome runtime through the same clipmap renderer as
# mountain_fly_review.tscn and save visual evidence. It captures:
# - MOUNTAIN/network_ref: the mountain review default
# - MOUNTAIN/close_debug: local-scale diagnostic
# - WORLD/network_ref: composed grammar-routed runtime plus route-color diagnostic
# WINDOWED only. Writes D:/tmp/wg10_biome_compose/biome_*_fly_capture*.png

const PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const MOUNTAIN := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const APRON_PX := 160
const FEATURE_SPAN_NETWORK_M := 90000.0
const FEATURE_SPAN_CLOSE_DEBUG_M := 3500.0
const FLOW_ITERS := 192
const PAGE_PX := 256
const SEED := 1337
const NUM_LEVELS := 5
const BASE_SPAN := 8192.0
const GRID_RES := 64
const RADIUS_PAGES := 1
const LEAD_SECONDS := 0.5
const MAX_PER_FRAME := 4
const CAPACITY := 96
const MORPH_REGION := 0.15
const RELIEF_SCALE := 0.25
const RELIEF_REF := 2000.0
const DETAIL_AMP := 350.0
const VIEW_SIZE := Vector2i(1280, 720)
const SKY := Color(0.45, 0.62, 0.85)
const OUT_MOUNTAIN_NETWORK := "D:/tmp/wg10_biome_compose/biome_mountain_network_fly_capture.png"
const OUT_MOUNTAIN_CLOSE := "D:/tmp/wg10_biome_compose/biome_mountain_close_fly_capture.png"
const OUT_WORLD := "D:/tmp/wg10_biome_compose/biome_world_fly_capture.png"
const OUT_ROUTE := "D:/tmp/wg10_biome_compose/biome_world_fly_capture_routes.png"

const MODE_MOUNTAIN := 0
const MODE_WORLD := 1

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-biome-capture] status=skip reason=no-render-device"); return 2
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	# Register BOTH global uniforms the render shader declares (ring_displace.gdshader: wg_dbg_mode +
	# wg_detail_amp). Missing wg_dbg_mode -> the render pipeline's global-uniform descriptor set is
	# incomplete -> "Uniforms were never supplied for set (3)" every draw (a HARNESS bug, not the producer).
	# Match the live review scene: add directly instead of calling global_shader_parameter_get_list(),
	# which Godot warns should not be used outside the editor.
	RenderingServer.global_shader_parameter_add("wg_dbg_mode", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, 0.0)
	RenderingServer.global_shader_parameter_add("wg_detail_amp", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, DETAIL_AMP)
	RenderingServer.global_shader_parameter_set("wg_detail_amp", DETAIL_AMP)

	var rc := await _capture_mode("mountain_network", MODE_MOUNTAIN, FEATURE_SPAN_NETWORK_M, OUT_MOUNTAIN_NETWORK, "")
	if rc != 0:
		return rc
	rc = await _capture_mode("mountain_close", MODE_MOUNTAIN, FEATURE_SPAN_CLOSE_DEBUG_M, OUT_MOUNTAIN_CLOSE, "")
	if rc != 0:
		return rc
	rc = await _capture_mode("world_network", MODE_WORLD, FEATURE_SPAN_NETWORK_M, OUT_WORLD, OUT_ROUTE)
	return rc

func _capture_mode(label: String, mode: int, feature_span_m: float, out_material: String, out_route: String) -> int:
	RenderingServer.global_shader_parameter_set("wg_dbg_mode", 0.0)
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var err := _configure_pool(pool, mode, feature_span_m)
	if err != "":
		push_error("[wg10-biome-capture] %s configure failed: %s" % [label, err]); return 1
	var runtime_mode := str(pool.call("biome_runtime_mode"))
	var expected_runtime := "world" if mode == MODE_WORLD else "single"
	if runtime_mode != expected_runtime:
		push_error("[wg10-biome-capture] %s expected runtime=%s, got %s" % [label, expected_runtime, runtime_mode]); return 1
	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_SECONDS, MAX_PER_FRAME)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, RELIEF_SCALE, MORPH_REGION, RELIEF_REF, LEAD_SECONDS)

	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = SKY
	env.fog_enabled = true
	env.fog_mode = Environment.FOG_MODE_DEPTH
	env.fog_depth_begin = BASE_SPAN * 8.0
	env.fog_depth_end = BASE_SPAN * 20.0
	env.fog_light_color = SKY
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	var cam := Camera3D.new()
	cam.far = BASE_SPAN * 32.0
	cam.environment = env
	vp.add_child(rings)
	vp.add_child(light)
	vp.add_child(cam)
	get_root().add_child(vp)

	# Fly a short path to stream pages, then frame a 3/4 oblique view of the terrain ahead.
	var pos := Vector2(0.0, 0.0)
	var dt := 1.0 / 60.0
	var v := Vector2(700.0, 700.0)
	for f in range(140):
		pos += v * dt
		view.call("update", pos.x, pos.y, v.x, v.y)
		var cam_frame := _camera_frame(pos, feature_span_m)
		var eye: Vector3 = cam_frame["eye"]
		var look: Vector3 = cam_frame["look"]
		cam.look_at_from_position(eye, look, Vector3.UP)
		await process_frame
		RenderingServer.force_draw()
		await process_frame

	var st: Dictionary = pool.call("stats")
	var pages := int(st.get("created", 0)) + int(st.get("recomputed", 0))
	var biome := bool(pool.call("uses_biome_path"))

	var img: Image = vp.get_texture().get_image()
	if img == null:
		push_error("[wg10-biome-capture] null image"); return 1
	DirAccess.make_dir_recursive_absolute("D:/tmp/wg10_biome_compose")
	var rc := img.save_png(out_material)

	var route_rc := OK
	if out_route != "":
		RenderingServer.global_shader_parameter_set("wg_dbg_mode", 2.0)
		RenderingServer.force_draw()
		await process_frame
		var route_img: Image = vp.get_texture().get_image()
		route_rc = route_img.save_png(out_route) if route_img != null else ERR_DOES_NOT_EXIST

	# Detach the ring materials from the pool's page textures BEFORE freeing them, else the next
	# (teardown) draw rebuilds each tile material's uniform set against a freed page RID ->
	# "Texture (binding 1) is not a valid texture". Unbind, then free.
	rings.call("unbind_all")
	pool.call("free_all")
	if rc != OK:
		push_error("[wg10-biome-capture] save_png failed rc=%d" % rc); return 1
	if route_rc != OK:
		push_error("[wg10-biome-capture] save_route_png failed rc=%d" % route_rc); return 1
	var route_suffix := " route=%s" % out_route if out_route != "" else ""
	print("[wg10-biome-capture] status=pass label=%s runtime=%s feature_span_m=%.0f wrote=%s%s pages=%d biome_path=%s size=%dx%d" % [
		label, runtime_mode, feature_span_m, out_material, route_suffix, pages, str(biome), VIEW_SIZE.x, VIEW_SIZE.y])
	return 0

func _configure_pool(pool: Object, mode: int, feature_span_m: float) -> String:
	if mode == MODE_WORLD:
		return str(pool.call("configure_biome_world",
			ProjectSettings.globalize_path(PACK_RES_DIR),
			PACK_FILE,
			CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, feature_span_m, FLOW_ITERS, 1000.0, 2, SEED))
	return str(pool.call("configure_biome",
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE),
		ProjectSettings.globalize_path(MOUNTAIN),
		CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, feature_span_m, FLOW_ITERS, 1000.0, 2, SEED))

func _camera_frame(pos: Vector2, feature_span_m: float) -> Dictionary:
	if feature_span_m > 10000.0:
		return {
			"eye": Vector3(pos.x - 9000.0, 5200.0, pos.y - 9000.0),
			"look": Vector3(pos.x + 22000.0, 250.0, pos.y + 22000.0),
		}
	return {
		"eye": Vector3(pos.x - 900.0, 720.0, pos.y - 900.0),
		"look": Vector3(pos.x + 1800.0, 60.0, pos.y + 1800.0),
	}
