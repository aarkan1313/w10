extends SceneTree

# One-shot CAPTURE: stream the live biome runtime through the same clipmap renderer as
# mountain_fly_review.tscn and save visual evidence. It captures:
# - REFERENCE: the accepted static mountain-network payload through the runtime renderer
# - MOUNTAIN/network_ref: the reference-backed live bridge for the accepted mountain-network target
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
const BRIDGE_SAMPLE_STRIDE := 4
const BRIDGE_MEAN_RGB_DELTA_MAX := 0.0025
const BRIDGE_P95_RGB_DELTA_MAX := 0.02
const BRIDGE_PATH_SPEED := Vector2(8000.0, 0.0)
const BRIDGE_PATH_SAMPLE_FRAMES := [80, 160, 240]

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
	rc = _assert_reference_bridge_match()
	if rc != 0:
		return rc
	rc = await _assert_reference_bridge_path(runtime)
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
	var relief_ref := float(producer.view_relief_ref(float(runtime.default_relief_ref()), float(runtime.default_relief_scale())))
	runtime.configure_view(view, pool, streamer, rings, bool(runtime.default_morph_enabled()), relief_scale, relief_ref)

	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	var env := Environment.new()
	runtime.configure_review_environment(env)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	var cam := Camera3D.new()
	cam.far = float(runtime.review_visual_edge_m())
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

func _assert_reference_bridge_match() -> int:
	var reference := Image.new()
	var mountain := Image.new()
	var err := reference.load(OUT_REFERENCE)
	if err != OK:
		push_error("[wg10-biome-capture] reference image load failed rc=%d" % err)
		return 1
	err = mountain.load(OUT_MOUNTAIN_NETWORK)
	if err != OK:
		push_error("[wg10-biome-capture] mountain bridge image load failed rc=%d" % err)
		return 1
	if reference.get_size() != mountain.get_size():
		push_error("[wg10-biome-capture] reference/bridge size mismatch %s vs %s" % [str(reference.get_size()), str(mountain.get_size())])
		return 1
	return _assert_images_match(reference, mountain, "bridge_match")

func _assert_reference_bridge_path(runtime: Object) -> int:
	var reference := await _capture_bridge_path_images(runtime, "bridge_path_reference", MODE_REFERENCE)
	if int(reference.get("rc", 1)) != 0:
		push_error("[wg10-biome-capture] bridge path REFERENCE failed: %s" % str(reference.get("error", "failed")))
		return 1
	var mountain := await _capture_bridge_path_images(runtime, "bridge_path_mountain", MODE_MOUNTAIN)
	if int(mountain.get("rc", 1)) != 0:
		push_error("[wg10-biome-capture] bridge path MOUNTAIN failed: %s" % str(mountain.get("error", "failed")))
		return 1

	var ref_images: Array = reference.get("images", [])
	var mountain_images: Array = mountain.get("images", [])
	if ref_images.size() != mountain_images.size() or ref_images.size() != BRIDGE_PATH_SAMPLE_FRAMES.size():
		push_error("[wg10-biome-capture] bridge path image count mismatch ref=%d mountain=%d expected=%d" % [
			ref_images.size(), mountain_images.size(), BRIDGE_PATH_SAMPLE_FRAMES.size()])
		return 1
	for i in range(ref_images.size()):
		var rc := _assert_images_match(ref_images[i], mountain_images[i], "bridge_path_f%d" % int(BRIDGE_PATH_SAMPLE_FRAMES[i]))
		if rc != 0:
			return rc
	print("[wg10-biome-capture] bridge_path status=pass frames=%s speed=%d" % [
		str(BRIDGE_PATH_SAMPLE_FRAMES), int(BRIDGE_PATH_SPEED.length())])
	return 0

func _capture_bridge_path_images(runtime: Object, label: String, mode: String) -> Dictionary:
	runtime.set_debug_mode(0)
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var producer: Object = load(PRODUCERS).new()
	var producer_err := _configure_producer(producer, mode, PRESET_NETWORK)
	if producer_err != "":
		return {"rc": 1, "error": producer_err}
	var err: String = producer.configure(pool)
	if err != "":
		return {"rc": 1, "error": "configure failed: %s" % err}

	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	runtime.configure_streamer(streamer, pool)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	runtime.configure_rings(rings)
	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	var relief_scale := float(producer.view_relief_scale(float(runtime.default_relief_scale())))
	var relief_ref := float(producer.view_relief_ref(float(runtime.default_relief_ref()), float(runtime.default_relief_scale())))
	runtime.configure_view(view, pool, streamer, rings, bool(runtime.default_morph_enabled()), relief_scale, relief_ref)

	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	var env := Environment.new()
	runtime.configure_review_environment(env)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	var cam := Camera3D.new()
	cam.far = float(runtime.review_visual_edge_m())
	cam.environment = env
	vp.add_child(rings)
	vp.add_child(light)
	vp.add_child(cam)
	get_root().add_child(vp)

	var images: Array[Image] = []
	var pos := Vector2.ZERO
	var dt := 1.0 / 60.0
	var last_frame := int(BRIDGE_PATH_SAMPLE_FRAMES[BRIDGE_PATH_SAMPLE_FRAMES.size() - 1])
	for f in range(last_frame + 1):
		pos += BRIDGE_PATH_SPEED * dt
		view.call("update", pos.x, pos.y, BRIDGE_PATH_SPEED.x, BRIDGE_PATH_SPEED.y)
		var cam_frame := _camera_frame(pos, float(producer.feature_span_m()))
		var eye: Vector3 = cam_frame["eye"]
		var look: Vector3 = cam_frame["look"]
		cam.look_at_from_position(eye, look, Vector3.UP)
		await process_frame
		RenderingServer.force_draw()
		await process_frame
		if BRIDGE_PATH_SAMPLE_FRAMES.has(f):
			var img: Image = vp.get_texture().get_image()
			if img == null:
				rings.call("unbind_all")
				pool.call("free_all")
				vp.queue_free()
				return {"rc": 1, "error": "null path image at frame %d" % f}
			images.append(img)

	var st: Dictionary = pool.call("stats")
	var pages := int(st.get("created", 0)) + int(st.get("recomputed", 0))
	rings.call("unbind_all")
	pool.call("free_all")
	vp.queue_free()
	print("[wg10-biome-capture] path_capture status=pass label=%s mode=%s frames=%s pages=%d" % [
		label, mode, str(BRIDGE_PATH_SAMPLE_FRAMES), pages])
	return {"rc": 0, "images": images, "error": ""}

func _assert_images_match(reference: Image, mountain: Image, label: String) -> int:
	if reference.get_size() != mountain.get_size():
		push_error("[wg10-biome-capture] %s size mismatch %s vs %s" % [label, str(reference.get_size()), str(mountain.get_size())])
		return 1
	var size := reference.get_size()
	var deltas: Array[float] = []
	var total := 0.0
	for y in range(0, size.y, BRIDGE_SAMPLE_STRIDE):
		for x in range(0, size.x, BRIDGE_SAMPLE_STRIDE):
			var a := reference.get_pixel(x, y)
			var b := mountain.get_pixel(x, y)
			var d := (absf(a.r - b.r) + absf(a.g - b.g) + absf(a.b - b.b)) / 3.0
			deltas.append(d)
			total += d
	deltas.sort()
	var mean := total / float(deltas.size())
	var p95 := deltas[int(floor(float(deltas.size() - 1) * 0.95))]
	if mean > BRIDGE_MEAN_RGB_DELTA_MAX or p95 > BRIDGE_P95_RGB_DELTA_MAX:
		push_error("[wg10-biome-capture] %s mismatch mean=%.6f p95=%.6f budgets %.6f/%.6f" % [
			label, mean, p95, BRIDGE_MEAN_RGB_DELTA_MAX, BRIDGE_P95_RGB_DELTA_MAX])
		return 1
	print("[wg10-biome-capture] %s status=pass samples=%d stride=%d mean=%.6f p95=%.6f budgets %.6f/%.6f" % [
		label, deltas.size(), BRIDGE_SAMPLE_STRIDE, mean, p95, BRIDGE_MEAN_RGB_DELTA_MAX, BRIDGE_P95_RGB_DELTA_MAX])
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
