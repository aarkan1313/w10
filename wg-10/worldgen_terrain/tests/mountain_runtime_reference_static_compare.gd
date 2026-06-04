extends SceneTree

# Visual bridge guard: compare the owner-liked static mountain-network review
# scene against the runtime REFERENCE bridge under the same focus framing.
#
# The renderers intentionally differ (baked chunk vertex colors vs clipmap page
# textures/material pages), so this is a terrain-mask/silhouette comparison, not
# a byte-for-byte color diff.

const STATIC_SCENE := "res://worldgen_terrain/harness/mountain_network_chunks_review.tscn"
const PRODUCERS := "res://worldgen_terrain/harness/mountain_fly_producers.gd"
const RUNTIME_CONFIG := "res://worldgen_terrain/harness/mountain_fly_runtime_config.gd"

const VIEW_SIZE := Vector2i(1280, 720)
const OUT_STATIC := "D:/tmp/wg10_biome_compose/mountain_static_focus_compare.png"
const OUT_RUNTIME := "D:/tmp/wg10_biome_compose/mountain_runtime_reference_focus_compare.png"
const MASK_STRIDE := 4
const SKY_STATIC := Color(0.68, 0.76, 0.84)
const SKY_RUNTIME := Color(0.45, 0.62, 0.85)
const SKY_DELTA_STATIC := 0.045
const SKY_DELTA_RUNTIME := 0.060
const MIN_TERRAIN_FRAC := 0.34
const MIN_MASK_IOU := 0.55

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-runtime-static-visual] status=skip reason=no-render-device")
		return 2
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	DirAccess.make_dir_recursive_absolute("D:/tmp/wg10_biome_compose")

	var static_result := await _capture_static_focus()
	if int(static_result.get("rc", 1)) != 0:
		push_error("[wg10-runtime-static-visual] static capture failed: %s" % str(static_result.get("error", "failed")))
		return 1
	var runtime_result := await _capture_runtime_reference_focus()
	if int(runtime_result.get("rc", 1)) != 0:
		push_error("[wg10-runtime-static-visual] runtime capture failed: %s" % str(runtime_result.get("error", "failed")))
		return 1

	var static_img: Image = static_result["image"]
	var runtime_img: Image = runtime_result["image"]
	var stats := _mask_compare(static_img, runtime_img)
	var static_frac := float(stats["static_frac"])
	var runtime_frac := float(stats["runtime_frac"])
	var iou := float(stats["iou"])

	var errs: Array[String] = []
	if static_frac < MIN_TERRAIN_FRAC:
		errs.append("static terrain_frac %.3f < %.3f" % [static_frac, MIN_TERRAIN_FRAC])
	if runtime_frac < MIN_TERRAIN_FRAC:
		errs.append("runtime terrain_frac %.3f < %.3f" % [runtime_frac, MIN_TERRAIN_FRAC])
	if iou < MIN_MASK_IOU:
		errs.append("terrain mask IoU %.3f < %.3f" % [iou, MIN_MASK_IOU])

	if not errs.is_empty():
		for err in errs:
			push_error(err)
		print("[wg10-runtime-static-visual] status=fail static_frac=%.3f runtime_frac=%.3f iou=%.3f" % [
			static_frac,
			runtime_frac,
			iou,
		])
		return 1

	print("[wg10-runtime-static-visual] status=pass static_frac=%.3f runtime_frac=%.3f iou=%.3f wrote=%s,%s" % [
		static_frac,
		runtime_frac,
		iou,
		OUT_STATIC,
		OUT_RUNTIME,
	])
	return 0

func _capture_static_focus() -> Dictionary:
	var packed := load(STATIC_SCENE)
	if packed == null:
		return {"rc": 1, "error": "cannot load %s" % STATIC_SCENE}
	var vp := _make_viewport()
	var scene: Node = packed.instantiate()
	vp.add_child(scene)
	await process_frame
	await process_frame
	_hide_canvas_layers(scene)
	scene.call("_focus_camera")
	await _draw_frame()
	var img: Image = vp.get_texture().get_image()
	if img == null:
		scene.queue_free()
		vp.queue_free()
		return {"rc": 1, "error": "static image null"}
	var rc := img.save_png(OUT_STATIC)
	scene.queue_free()
	vp.queue_free()
	await process_frame
	if rc != OK:
		return {"rc": 1, "error": "static save rc=%d" % rc}
	return {"rc": 0, "image": img, "error": ""}

func _capture_runtime_reference_focus() -> Dictionary:
	var runtime: Object = load(RUNTIME_CONFIG).new()
	runtime.register_shader_globals(bool(runtime.default_detail_enabled()))
	runtime.set_debug_mode(0)

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var producer: Object = load(PRODUCERS).new()
	if not bool(producer.set_mode_label("REFERENCE")):
		return {"rc": 1, "error": "cannot set REFERENCE mode"}
	if not bool(producer.set_preset_label("network_ref")):
		return {"rc": 1, "error": "cannot set network_ref preset"}
	var err := str(producer.configure(pool))
	if err != "":
		return {"rc": 1, "error": "producer configure failed: %s" % err}

	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	runtime.configure_streamer(streamer, pool)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	runtime.configure_rings(rings)
	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	var relief_scale := float(producer.view_relief_scale(float(runtime.default_relief_scale())))
	var relief_ref := float(producer.view_relief_ref(float(runtime.default_relief_ref()), float(runtime.default_relief_scale())))
	runtime.configure_view(view, pool, streamer, rings, bool(runtime.default_morph_enabled()), relief_scale, relief_ref)

	var vp := _make_viewport()
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

	for _f in range(180):
		view.call("update", 0.0, 0.0, 0.0, 0.0)
		_apply_runtime_focus_camera(cam)
		await _draw_frame()

	var img: Image = vp.get_texture().get_image()
	if img == null:
		rings.call("unbind_all")
		pool.call("free_all")
		vp.queue_free()
		return {"rc": 1, "error": "runtime image null"}
	var rc := img.save_png(OUT_RUNTIME)

	rings.call("unbind_all")
	pool.call("free_all")
	vp.queue_free()
	await process_frame
	if rc != OK:
		return {"rc": 1, "error": "runtime save rc=%d" % rc}
	return {"rc": 0, "image": img, "error": ""}

func _make_viewport() -> SubViewport:
	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	get_root().add_child(vp)
	return vp

func _apply_runtime_focus_camera(cam: Camera3D) -> void:
	var span := 76800.0
	var height_ref := 1700.0
	var eye := Vector3(0.0, maxf(220.0, span * 0.030 + height_ref * 0.80), span * 0.090)
	cam.look_at_from_position(eye, Vector3.ZERO, Vector3.UP)

func _draw_frame() -> void:
	await process_frame
	RenderingServer.force_draw()
	await process_frame

func _mask_compare(static_img: Image, runtime_img: Image) -> Dictionary:
	var size := static_img.get_size()
	if runtime_img.get_size() != size:
		return {"static_frac": 0.0, "runtime_frac": 0.0, "iou": 0.0}
	var static_hits := 0
	var runtime_hits := 0
	var intersection := 0
	var union := 0
	var samples := 0
	for y in range(0, size.y, MASK_STRIDE):
		for x in range(0, size.x, MASK_STRIDE):
			samples += 1
			var s_hit := _is_terrain(static_img.get_pixel(x, y), SKY_STATIC, SKY_DELTA_STATIC)
			var r_hit := _is_terrain(runtime_img.get_pixel(x, y), SKY_RUNTIME, SKY_DELTA_RUNTIME)
			if s_hit:
				static_hits += 1
			if r_hit:
				runtime_hits += 1
			if s_hit and r_hit:
				intersection += 1
			if s_hit or r_hit:
				union += 1
	return {
		"static_frac": float(static_hits) / float(maxi(samples, 1)),
		"runtime_frac": float(runtime_hits) / float(maxi(samples, 1)),
		"iou": float(intersection) / float(maxi(union, 1)),
	}

func _is_terrain(color: Color, sky: Color, delta: float) -> bool:
	var d := maxf(maxf(absf(color.r - sky.r), absf(color.g - sky.g)), absf(color.b - sky.b))
	return d > delta

func _hide_canvas_layers(node: Node) -> void:
	for child in node.get_children():
		if child is CanvasLayer:
			child.visible = false
		_hide_canvas_layers(child)
