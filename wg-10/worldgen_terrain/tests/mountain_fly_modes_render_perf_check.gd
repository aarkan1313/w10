extends SceneTree

# Render-path diagnostic for the owner fly scene's 1/2/3 modes. The existing
# mountain_fly_modes_perf_check.gd proves page update time and visible tile churn; this one proves
# the actual shared viewport render is non-degenerate and stays inside a 60 Hz frame budget.

const PRODUCERS := "res://worldgen_terrain/harness/mountain_fly_producers.gd"
const RUNTIME_CONFIG := "res://worldgen_terrain/harness/mountain_fly_runtime_config.gd"

const MODES := ["REFERENCE", "MOUNTAIN", "WORLD"]
const PRESET_NETWORK := "network_ref"
const VIEW_SIZE := Vector2i(1280, 720)
const SPEED := 2500.0
const DT := 1.0 / 60.0
const WARM_FRAMES := 90
const MEASURE_FRAMES := 180
const GPU_P99_BUDGET_MS := 16.7
const CPU_P99_BUDGET_MS := 16.7
const MIN_GPU_MS := 0.001
const MIN_STREAM_EVENTS := 1
const MIN_PRIMITIVES := 400000
const MIN_TERRAIN_FRAC := 0.80
const SKY_DELTA := 0.06

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10TerrainView"):
		push_error("[wg10-modes-render] Wg10TerrainView not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-modes-render] status=skip reason=no-render-device"); return 2
	if not RenderingServer.has_method("viewport_get_measured_render_time_gpu"):
		print("[wg10-modes-render] status=skip reason=no-measured-render-time-api"); return 2

	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)

	var runtime: Object = load(RUNTIME_CONFIG).new()
	runtime.register_shader_globals(bool(runtime.default_detail_enabled()))
	var errs: Array[String] = []

	for mode in MODES:
		var result: Dictionary = await _run_mode(runtime, str(mode))
		if int(result.get("rc", 1)) != 0:
			errs.append("%s: %s" % [str(mode), str(result.get("error", "failed"))])

	if not errs.is_empty():
		for e in errs:
			push_error(e)
		print("[wg10-modes-render] status=fail errors=%d" % errs.size())
		return 1

	print("[wg10-modes-render] status=pass modes=%d size=%dx%d grid=%d" % [
		MODES.size(), VIEW_SIZE.x, VIEW_SIZE.y, int(runtime.grid_res())])
	return 0

func _run_mode(runtime: Object, mode: String) -> Dictionary:
	runtime.set_debug_mode(0)
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var producer: Object = load(PRODUCERS).new()
	var setup_err := _configure_producer(producer, mode)
	if setup_err != "":
		return {"rc": 1, "error": setup_err}
	var err: String = str(producer.configure(pool))
	if err != "":
		return {"rc": 1, "error": "configure failed: %s" % err}
	var expected_runtime := "static_reference" if mode == "REFERENCE" else ("world" if mode == "WORLD" else "single")
	var runtime_mode := str(pool.call("biome_runtime_mode"))
	if runtime_mode != expected_runtime:
		return {"rc": 1, "error": "expected runtime=%s got=%s" % [expected_runtime, runtime_mode]}

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
	RenderingServer.viewport_set_measure_render_time(vp.get_viewport_rid(), true)

	var pos := Vector2.ZERO
	var headings := [
		Vector2(1.0, 0.0),
		Vector2(0.70710678, 0.70710678),
		Vector2(0.0, 1.0),
		Vector2(-0.70710678, 0.70710678),
	]
	var st0: Dictionary = pool.call("stats")
	var stream0 := int(st0.get("created", 0)) + int(st0.get("recomputed", 0))
	var gpu_samples: Array[float] = []
	var cpu_samples: Array[float] = []
	var gpu_max := 0.0
	var prim_max := 0
	var terrain_min := 1.0
	var resident_max := 0
	var total := WARM_FRAMES + MEASURE_FRAMES

	for f in range(total):
		var heading: Vector2 = headings[int(f / 60) % headings.size()]
		var vel := heading * SPEED
		pos += vel * DT
		var tu0 := Time.get_ticks_usec()
		view.call("update", pos.x, pos.y, vel.x, vel.y)
		var cpu_ms := float(Time.get_ticks_usec() - tu0) / 1000.0

		var cam_frame := _camera_frame(pos, float(producer.feature_span_m()))
		var eye: Vector3 = cam_frame["eye"]
		var look: Vector3 = cam_frame["look"]
		cam.look_at_from_position(eye, look, Vector3.UP)
		await process_frame
		RenderingServer.force_draw()
		await process_frame

		if f >= WARM_FRAMES:
			var gpu_ms := RenderingServer.viewport_get_measured_render_time_gpu(vp.get_viewport_rid())
			gpu_samples.append(gpu_ms)
			cpu_samples.append(cpu_ms)
			gpu_max = maxf(gpu_max, gpu_ms)
			prim_max = maxi(prim_max, RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_PRIMITIVES_IN_FRAME))
			var st: Dictionary = pool.call("stats")
			resident_max = maxi(resident_max, int(st.get("resident", 0)))
			if (f - WARM_FRAMES) % 30 == 0:
				var img: Image = vp.get_texture().get_image()
				if img != null:
					terrain_min = minf(terrain_min, _terrain_frac(img, runtime.sky_color()))

	var st1: Dictionary = pool.call("stats")
	var stream_events := int(st1.get("created", 0)) + int(st1.get("recomputed", 0)) - stream0
	var biome_active := bool(pool.call("uses_biome_path"))
	var vertex_count := int(rings.call("total_vertex_count"))
	var gpu_p99 := _percentile(gpu_samples, 0.99)
	var gpu_mean := _mean(gpu_samples)
	var cpu_p99 := _percentile(cpu_samples, 0.99)
	var cpu_mean := _mean(cpu_samples)

	rings.call("unbind_all")
	pool.call("free_all")
	vp.queue_free()

	print("[wg10-modes-render] mode=%s runtime=%s gpu_mean=%.3fms gpu_p99=%.3fms gpu_max=%.3fms cpu_mean=%.3fms cpu_p99=%.3fms terrain_min=%.3f stream_events=%d resident_max=%d prim_max=%d vertices=%d biome_path=%s" % [
		mode, runtime_mode, gpu_mean, gpu_p99, gpu_max, cpu_mean, cpu_p99, terrain_min,
		stream_events, resident_max, prim_max, vertex_count, str(biome_active)])

	var errs: Array[String] = []
	if gpu_max < MIN_GPU_MS:
		errs.append("GPU timer read ~0 (max %.5f ms)" % gpu_max)
	if gpu_p99 > GPU_P99_BUDGET_MS:
		errs.append("gpu_p99 %.3fms > %.3fms" % [gpu_p99, GPU_P99_BUDGET_MS])
	if cpu_p99 > CPU_P99_BUDGET_MS:
		errs.append("cpu_p99 %.3fms > %.3fms" % [cpu_p99, CPU_P99_BUDGET_MS])
	if terrain_min < MIN_TERRAIN_FRAC:
		errs.append("terrain_frac %.3f < %.2f" % [terrain_min, MIN_TERRAIN_FRAC])
	if stream_events < MIN_STREAM_EVENTS:
		errs.append("stream_events %d < %d" % [stream_events, MIN_STREAM_EVENTS])
	if prim_max < MIN_PRIMITIVES:
		errs.append("prim_max %d < %d" % [prim_max, MIN_PRIMITIVES])

	if not errs.is_empty():
		return {"rc": 1, "error": "; ".join(errs)}
	return {"rc": 0, "error": ""}

func _configure_producer(producer: Object, mode: String) -> String:
	if not bool(producer.set_mode_label(mode)):
		return "invalid mode %s" % mode
	if not bool(producer.set_preset_label(PRESET_NETWORK)):
		return "invalid preset %s" % PRESET_NETWORK
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

func _terrain_frac(img: Image, sky: Color) -> float:
	var hit := 0
	var samp := 0
	var y0 := int(img.get_height() / 3)
	for y in range(y0, img.get_height(), 4):
		for x in range(0, img.get_width(), 4):
			samp += 1
			var c := img.get_pixel(x, y)
			var d := maxf(maxf(absf(c.r - sky.r), absf(c.g - sky.g)), absf(c.b - sky.b))
			if d > SKY_DELTA:
				hit += 1
	return float(hit) / float(maxi(samp, 1))

func _percentile(samples: Array[float], fraction: float) -> float:
	if samples.is_empty():
		return 0.0
	samples.sort()
	var idx := int(ceil(float(samples.size()) * fraction)) - 1
	idx = clampi(idx, 0, samples.size() - 1)
	return float(samples[idx])

func _mean(samples: Array[float]) -> float:
	if samples.is_empty():
		return 0.0
	var sum := 0.0
	for v in samples:
		sum += float(v)
	return sum / float(samples.size())
