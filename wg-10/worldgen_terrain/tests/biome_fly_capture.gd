extends SceneTree

# One-shot CAPTURE: stream the live biome runtime through the same clipmap renderer as
# mountain_fly_review.tscn and save visual evidence. It captures:
# - REFERENCE: the accepted static mountain-network payload through the runtime renderer
# - MOUNTAIN/network_ref: the explicit live candidate for the accepted mountain-network target
# - MOUNTAIN/close_debug: local-scale diagnostic
# - WORLD/network_ref: composed grammar-routed runtime plus route-color diagnostic
# WINDOWED only. Writes D:/tmp/wg10_biome_compose/biome_*_fly_capture*.png

const PRODUCERS := "res://worldgen_terrain/harness/mountain_fly_producers.gd"
const RUNTIME_CONFIG := "res://worldgen_terrain/harness/mountain_fly_runtime_config.gd"
const VIEW_SIZE := Vector2i(1280, 720)
const OUT_REFERENCE := "D:/tmp/wg10_biome_compose/biome_mountain_reference_fly_capture.png"
const OUT_MOUNTAIN_NETWORK := "D:/tmp/wg10_biome_compose/biome_mountain_network_fly_capture.png"
const OUT_MOUNTAIN_CLOSE := "D:/tmp/wg10_biome_compose/biome_mountain_close_fly_capture.png"
const OUT_WORLD := "D:/tmp/wg10_biome_compose/biome_world_fly_capture.png"
const OUT_ROUTE := "D:/tmp/wg10_biome_compose/biome_world_fly_capture_routes.png"

const MODE_REFERENCE := "REFERENCE"
const MODE_MOUNTAIN := "MOUNTAIN"
const MODE_WORLD := "WORLD"
const PRESET_NETWORK := "network_ref"
const PRESET_CLOSE_DEBUG := "close_debug"

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-biome-capture] status=skip reason=no-render-device"); return 2
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	var runtime: Object = load(RUNTIME_CONFIG).new()
	runtime.register_shader_globals(bool(runtime.default_detail_enabled()))

	var rc := await _capture_mode(runtime, "mountain_reference", MODE_REFERENCE, PRESET_NETWORK, OUT_REFERENCE, "")
	if rc != 0:
		return rc
	rc = await _capture_mode(runtime, "mountain_network", MODE_MOUNTAIN, PRESET_NETWORK, OUT_MOUNTAIN_NETWORK, "")
	if rc != 0:
		return rc
	rc = await _capture_mode(runtime, "mountain_close", MODE_MOUNTAIN, PRESET_CLOSE_DEBUG, OUT_MOUNTAIN_CLOSE, "")
	if rc != 0:
		return rc
	rc = await _capture_mode(runtime, "world_network", MODE_WORLD, PRESET_NETWORK, OUT_WORLD, OUT_ROUTE)
	return rc

func _capture_mode(runtime: Object, label: String, mode: String, preset: String, out_material: String, out_route: String) -> int:
	runtime.set_debug_mode(0)
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var producer: Object = load(PRODUCERS).new()
	var producer_err := _configure_producer(producer, mode, preset)
	if producer_err != "":
		push_error("[wg10-biome-capture] %s producer failed: %s" % [label, producer_err]); return 1
	var feature_span_m := float(producer.feature_span_m())
	var err: String = producer.configure(pool)
	if err != "":
		push_error("[wg10-biome-capture] %s configure failed: %s" % [label, err]); return 1
	var runtime_mode := str(pool.call("biome_runtime_mode"))
	var expected_runtime := "static_reference" if mode == MODE_REFERENCE else ("world" if mode == MODE_WORLD else "single")
	if runtime_mode != expected_runtime:
		push_error("[wg10-biome-capture] %s expected runtime=%s, got %s" % [label, expected_runtime, runtime_mode]); return 1
	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	runtime.configure_streamer(streamer, pool)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	runtime.configure_rings(rings)
	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	var relief_scale := float(producer.view_relief_scale(float(runtime.default_relief_scale())))
	runtime.configure_view(view, pool, streamer, rings, bool(runtime.default_morph_enabled()), relief_scale)

	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	var env := Environment.new()
	runtime.configure_review_environment(env)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	var cam := Camera3D.new()
	cam.far = float(runtime.loaded_edge_m())
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
		runtime.set_debug_mode(2)
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

func _configure_producer(producer: Object, mode: String, preset: String) -> String:
	if not bool(producer.set_mode_label(mode)):
		return "invalid mode %s" % mode
	if not bool(producer.set_preset_label(preset)):
		return "invalid preset %s" % preset
	return ""

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
