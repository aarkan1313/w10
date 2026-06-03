extends SceneTree

# One-shot CAPTURE: set up the mountain biome streaming path (configure_biome), fly a few frames to
# stream real pages, render to an offscreen SubViewport, and SAVE A PNG. This is the unfakeable proof
# that the runtime producer renders REAL terrain (the perf gate's terrain_frac couldn't distinguish a
# failed-but-counted page from a real one). WINDOWED only. Writes D:/tmp/wg10_biome_compose/biome_fly_capture.png

const PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const MOUNTAIN := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const APRON_PX := 160
const FEATURE_SPAN_M := 3500.0  # scale-contract on-foot mountain (was 90000 = giant-massif sliver -> flat)
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
const OUT := "D:/tmp/wg10_biome_compose/biome_fly_capture.png"

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-biome-capture] status=skip reason=no-render-device"); return 2
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	# Register BOTH global uniforms the render shader declares (ring_displace.gdshader: wg_dbg_mode +
	# wg_detail_amp). Missing wg_dbg_mode -> the render pipeline's global-uniform descriptor set is
	# incomplete -> "Uniforms were never supplied for set (3)" every draw (a HARNESS bug, not the producer).
	if not RenderingServer.global_shader_parameter_get_list().has("wg_dbg_mode"):
		RenderingServer.global_shader_parameter_add("wg_dbg_mode", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, 0.0)
	if not RenderingServer.global_shader_parameter_get_list().has("wg_detail_amp"):
		RenderingServer.global_shader_parameter_add("wg_detail_amp", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, DETAIL_AMP)
	RenderingServer.global_shader_parameter_set("wg_detail_amp", DETAIL_AMP)

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var err: String = str(pool.call("configure_biome",
		ProjectSettings.globalize_path(PRIM), ProjectSettings.globalize_path(MACHINE),
		ProjectSettings.globalize_path(MOUNTAIN),
		CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, FEATURE_SPAN_M, FLOW_ITERS, 1000.0, SEED))
	if err != "":
		push_error("[wg10-biome-capture] configure_biome failed: %s" % err); return 1
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
		# oblique camera: above + behind, looking down-forward at the mountains
		var eye := Vector3(pos.x - 1400.0, 1600.0, pos.y - 1400.0)
		var look := Vector3(pos.x + 2400.0, 200.0, pos.y + 2400.0)
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
	var rc := img.save_png(OUT)
	# Detach the ring materials from the pool's page textures BEFORE freeing them, else the next
	# (teardown) draw rebuilds each tile material's uniform set against a freed page RID ->
	# "Texture (binding 1) is not a valid texture". Unbind, then free.
	rings.call("unbind_all")
	pool.call("free_all")
	if rc != OK:
		push_error("[wg10-biome-capture] save_png failed rc=%d" % rc); return 1
	print("[wg10-biome-capture] status=pass wrote %s pages=%d biome_path=%s size=%dx%d" % [
		OUT, pages, str(biome), VIEW_SIZE.x, VIEW_SIZE.y])
	return 0
