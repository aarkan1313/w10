extends SceneTree

# Owner-flight stress probe for the manual "slow / laggy / popping / weird" report.
#
# The normal mode gates use a simple four-heading path. This probe is closer to a hand flight:
# speed pulses, stops, diagonal turns, and morph off/on variants while rendering the shared
# mountain_fly_review runtime path. It writes final evidence PNGs and fails on the owner-visible
# hard problems: visible tile hide/show churn, pool full events, degenerate/sky frames, or large
# synchronous update hitches.

const PRODUCERS := "res://worldgen_terrain/harness/mountain_fly_producers.gd"
const RUNTIME_CONFIG := "res://worldgen_terrain/harness/mountain_fly_runtime_config.gd"

const MODES := ["REFERENCE", "MOUNTAIN", "WORLD"]
const PRESET_NETWORK := "network_ref"
const VIEW_SIZE := Vector2i(1280, 720)
const OUT_DIR := "D:/tmp/wg10_biome_compose"
const DT := 1.0 / 60.0
const WARM_FRAMES := 120
const MEASURE_FRAMES := 360
const MAX_UPDATE_P99_MS := 33.4
const MAX_UPDATE_MAX_MS := 80.0
const MAX_GPU_P99_MS := 16.7
const MIN_TERRAIN_FRAC := 0.62
const SKY_DELTA := 0.06
const MIN_STREAM_EVENTS := 1
const MIN_REPAGES := 1
const BRIDGE_SAMPLE_STRIDE := 4
const BRIDGE_MEAN_RGB_DELTA_MAX := 0.0025
const BRIDGE_P95_RGB_DELTA_MAX := 0.0200

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10TerrainView"):
		push_error("[wg10-manual-stress] Wg10TerrainView not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-manual-stress] status=skip reason=no-render-device"); return 2
	if not RenderingServer.has_method("viewport_get_measured_render_time_gpu"):
		print("[wg10-manual-stress] status=skip reason=no-measured-render-time-api"); return 2

	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	DirAccess.make_dir_recursive_absolute(OUT_DIR)

	var runtime: Object = load(RUNTIME_CONFIG).new()
	runtime.register_shader_globals(bool(runtime.default_detail_enabled()))
	var errs: Array[String] = []
	var captures_by_morph := {}
	for morph_enabled in [false, true]:
		var morph_key := "morph_on" if bool(morph_enabled) else "morph_off"
		captures_by_morph[morph_key] = {}
		for mode in MODES:
			var result := await _run_case(runtime, str(mode), bool(morph_enabled))
			if int(result.get("rc", 1)) != 0:
				errs.append("%s morph=%s: %s" % [
					str(mode),
					"on" if bool(morph_enabled) else "off",
					str(result.get("error", "failed")),
				])
			else:
				var img: Image = result.get("image", null)
				if img == null:
					errs.append("%s morph=%s: missing final evidence image" % [
						str(mode),
						"on" if bool(morph_enabled) else "off",
					])
				else:
					var mode_captures: Dictionary = captures_by_morph[morph_key]
					mode_captures[str(mode)] = img
					captures_by_morph[morph_key] = mode_captures

	for morph_key in captures_by_morph.keys():
		var captures: Dictionary = captures_by_morph[morph_key]
		var reference: Image = captures.get("REFERENCE", null)
		if reference == null:
			errs.append("%s: missing REFERENCE image for bridge comparison" % str(morph_key))
			continue
		for mode in ["MOUNTAIN", "WORLD"]:
			var candidate: Image = captures.get(mode, null)
			if candidate == null:
				errs.append("%s: missing %s image for bridge comparison" % [str(morph_key), mode])
				continue
			var bridge_err := _bridge_image_error(reference, candidate, "%s_%s" % [str(morph_key), mode.to_lower()])
			if bridge_err != "":
				errs.append(bridge_err)

	if not errs.is_empty():
		for e in errs:
			push_error(e)
		print("[wg10-manual-stress] status=fail errors=%d" % errs.size())
		return 1

	print("[wg10-manual-stress] status=pass cases=%d frames=%d out=%s" % [
		MODES.size() * 2, MEASURE_FRAMES, OUT_DIR])
	return 0

func _run_case(runtime: Object, mode: String, morph_enabled: bool) -> Dictionary:
	runtime.set_debug_mode(0)
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var producer: Object = load(PRODUCERS).new()
	var setup_err := _configure_producer(producer, mode)
	if setup_err != "":
		return {"rc": 1, "error": setup_err}
	var err: String = str(producer.configure(pool))
	if err != "":
		return {"rc": 1, "error": "configure failed: %s" % err}

	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	runtime.configure_streamer(streamer, pool)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	runtime.configure_rings(rings)
	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	var relief_scale := float(producer.view_relief_scale(float(runtime.default_relief_scale())))
	var relief_ref := float(producer.view_relief_ref(float(runtime.default_relief_ref()), float(runtime.default_relief_scale())))
	runtime.configure_view(view, pool, streamer, rings, morph_enabled, relief_scale, relief_ref)

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
	RenderingServer.viewport_set_measure_render_time(vp.get_viewport_rid(), true)

	var pos := Vector2.ZERO
	var last_dir := Vector2(1.0, 0.0)
	for f in range(WARM_FRAMES):
		var warm_vel := Vector2(6500.0, 1800.0)
		pos += warm_vel * DT
		last_dir = warm_vel.normalized()
		view.call("update", pos.x, pos.y, warm_vel.x, warm_vel.y)
		_apply_camera(cam, pos, last_dir, float(producer.feature_span_m()))
		await process_frame
		RenderingServer.force_draw()
		await process_frame

	var prev: PackedInt64Array = rings.call("debug_tile_states")
	var visible0 := _visible_count(prev)
	var expected_tiles := int(runtime.num_levels()) * 9
	var st0: Dictionary = pool.call("stats")
	var streamer0: Dictionary = streamer.call("stats")
	var stream0 := int(st0.get("created", 0)) + int(st0.get("recomputed", 0))
	var full0 := int(st0.get("full_events", 0)) + int(streamer0.get("full_events", 0))

	var cpu_samples: Array[float] = []
	var gpu_samples: Array[float] = []
	var cpu_sum := 0.0
	var gpu_sum := 0.0
	var cpu_max := 0.0
	var gpu_max := 0.0
	var cpu_max_frame := -1
	var gpu_max_frame := -1
	var acquired_max := 0
	var hide_total := 0
	var show_total := 0
	var repage_total := 0
	var coarsest_hide := 0
	var hidden_frames := 0
	var max_hidden := 0
	var terrain_min := 1.0
	var levels := int(runtime.num_levels())
	var out_tag := "%s_%s" % [mode.to_lower(), "morph_on" if morph_enabled else "morph_off"]
	var final_img: Image = null

	for f in range(MEASURE_FRAMES):
		var vel := _manual_velocity(f)
		pos += vel * DT
		if vel.length() > 1.0:
			last_dir = vel.normalized()

		var tu0 := Time.get_ticks_usec()
		view.call("update", pos.x, pos.y, vel.x, vel.y)
		var cpu_ms := float(Time.get_ticks_usec() - tu0) / 1000.0
		cpu_samples.append(cpu_ms)
		cpu_sum += cpu_ms
		if cpu_ms > cpu_max:
			cpu_max = cpu_ms
			cpu_max_frame = f

		_apply_camera(cam, pos, last_dir, float(producer.feature_span_m()))
		await process_frame
		RenderingServer.force_draw()
		await process_frame

		var gpu_ms := RenderingServer.viewport_get_measured_render_time_gpu(vp.get_viewport_rid())
		gpu_samples.append(gpu_ms)
		gpu_sum += gpu_ms
		if gpu_ms > gpu_max:
			gpu_max = gpu_ms
			gpu_max_frame = f

		var st_frame: Dictionary = streamer.call("stats")
		acquired_max = maxi(acquired_max, int(st_frame.get("acquired_this_frame", 0)))

		var states: PackedInt64Array = rings.call("debug_tile_states")
		var frame_hidden := 0
		var t := 0
		while t * 3 + 2 < states.size():
			var level := int(t / 9)
			var vis := int(states[t * 3])
			var ox := int(states[t * 3 + 1])
			var oz := int(states[t * 3 + 2])
			var pv := int(prev[t * 3])
			var pox := int(prev[t * 3 + 1])
			var poz := int(prev[t * 3 + 2])
			if vis == 0:
				frame_hidden += 1
			if vis != pv:
				if vis == 0:
					hide_total += 1
					if level == levels - 1:
						coarsest_hide += 1
				else:
					show_total += 1
			elif vis == 1 and (ox != pox or oz != poz):
				repage_total += 1
			t += 1
		if frame_hidden > 0:
			hidden_frames += 1
			max_hidden = maxi(max_hidden, frame_hidden)
		prev = states

		if f % 45 == 0 or f == MEASURE_FRAMES - 1:
			var img: Image = vp.get_texture().get_image()
			if img != null:
				final_img = img
				terrain_min = minf(terrain_min, _terrain_frac(img, runtime.sky_color()))

	var st1: Dictionary = pool.call("stats")
	var streamer1: Dictionary = streamer.call("stats")
	var stream_events := int(st1.get("created", 0)) + int(st1.get("recomputed", 0)) - stream0
	var full_events := int(st1.get("full_events", 0)) + int(streamer1.get("full_events", 0)) - full0
	var resident := int(st1.get("resident", 0))
	var runtime_mode := str(pool.call("biome_runtime_mode"))
	var cpu_p95 := _percentile(cpu_samples, 0.95)
	var cpu_p99 := _percentile(cpu_samples, 0.99)
	var gpu_p99 := _percentile(gpu_samples, 0.99)
	var cpu_mean := cpu_sum / float(maxi(cpu_samples.size(), 1))
	var gpu_mean := gpu_sum / float(maxi(gpu_samples.size(), 1))
	var out_path := "%s/manual_stress_%s.png" % [OUT_DIR, out_tag]
	var save_rc := OK
	if final_img != null:
		save_rc = final_img.save_png(out_path)
	else:
		save_rc = ERR_DOES_NOT_EXIST

	rings.call("unbind_all")
	pool.call("free_all")
	vp.queue_free()
	await process_frame

	print("[wg10-manual-stress] case=%s runtime=%s morph=%s cpu_mean=%.3fms cpu_p95=%.3fms cpu_p99=%.3fms cpu_max=%.3fms cpu_max_frame=%d gpu_mean=%.3fms gpu_p99=%.3fms gpu_max=%.3fms gpu_max_frame=%d acquired_max=%d stream_events=%d resident=%d repage=%d hide=%d show=%d hidden_frames=%d max_hidden=%d full_events=%d terrain_min=%.3f visible0=%d/%d wrote=%s" % [
		mode, runtime_mode, "on" if morph_enabled else "off", cpu_mean, cpu_p95, cpu_p99,
		cpu_max, cpu_max_frame, gpu_mean, gpu_p99, gpu_max, gpu_max_frame, acquired_max,
		stream_events, resident, repage_total, hide_total, show_total, hidden_frames,
		max_hidden, full_events, terrain_min, visible0, expected_tiles, out_path])

	var errs: Array[String] = []
	if visible0 != expected_tiles:
		errs.append("warmup visible=%d expected=%d" % [visible0, expected_tiles])
	if save_rc != OK:
		errs.append("save_png failed rc=%d" % save_rc)
	if stream_events < MIN_STREAM_EVENTS:
		errs.append("stream_events %d < %d" % [stream_events, MIN_STREAM_EVENTS])
	if repage_total < MIN_REPAGES:
		errs.append("repage %d < %d" % [repage_total, MIN_REPAGES])
	if hide_total > 0:
		errs.append("visible tiles hid: hide=%d show=%d hidden_frames=%d max_hidden=%d" % [
			hide_total, show_total, hidden_frames, max_hidden])
	if coarsest_hide > 0:
		errs.append("coarsest tiles hid: %d" % coarsest_hide)
	if full_events > 0:
		errs.append("full_events=%d" % full_events)
	if terrain_min < MIN_TERRAIN_FRAC:
		errs.append("terrain_min %.3f < %.3f" % [terrain_min, MIN_TERRAIN_FRAC])
	if cpu_p99 > MAX_UPDATE_P99_MS:
		errs.append("cpu_p99 %.3fms > %.3fms" % [cpu_p99, MAX_UPDATE_P99_MS])
	if cpu_max > MAX_UPDATE_MAX_MS:
		errs.append("cpu_max %.3fms > %.3fms" % [cpu_max, MAX_UPDATE_MAX_MS])
	if gpu_p99 > MAX_GPU_P99_MS:
		errs.append("gpu_p99 %.3fms > %.3fms" % [gpu_p99, MAX_GPU_P99_MS])

	if not errs.is_empty():
		return {"rc": 1, "error": "; ".join(errs)}
	return {"rc": 0, "error": "", "image": final_img}

func _configure_producer(producer: Object, mode: String) -> String:
	if not bool(producer.set_mode_label(mode)):
		return "invalid mode %s" % mode
	if not bool(producer.set_preset_label(PRESET_NETWORK)):
		return "invalid preset %s" % PRESET_NETWORK
	return ""

func _manual_velocity(frame: int) -> Vector2:
	if frame < 45:
		return Vector2(2200.0, 0.0)
	if frame < 105:
		return Vector2(10000.0, 0.0)
	if frame < 165:
		return Vector2(7200.0, 7200.0)
	if frame < 205:
		return Vector2.ZERO
	if frame < 275:
		return Vector2(-9500.0, 3800.0)
	if frame < 320:
		return Vector2(0.0, -12000.0)
	return Vector2(5200.0, -5200.0)

func _apply_camera(cam: Camera3D, pos: Vector2, dir: Vector2, feature_span_m: float) -> void:
	var side := Vector2(-dir.y, dir.x)
	if feature_span_m > 10000.0:
		var eye2 := pos - dir * 9200.0 + side * 2500.0
		var look2 := pos + dir * 22000.0
		cam.look_at_from_position(Vector3(eye2.x, 5200.0, eye2.y), Vector3(look2.x, 250.0, look2.y), Vector3.UP)
	else:
		var eye_close := pos - dir * 950.0 + side * 220.0
		var look_close := pos + dir * 1900.0
		cam.look_at_from_position(Vector3(eye_close.x, 760.0, eye_close.y), Vector3(look_close.x, 80.0, look_close.y), Vector3.UP)

func _visible_count(states: PackedInt64Array) -> int:
	var count := 0
	var t := 0
	while t * 3 < states.size():
		if int(states[t * 3]) == 1:
			count += 1
		t += 1
	return count

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

func _bridge_image_error(reference: Image, candidate: Image, label: String) -> String:
	if reference.get_size() != candidate.get_size():
		return "%s bridge size mismatch %s vs %s" % [label, str(reference.get_size()), str(candidate.get_size())]
	var size := reference.get_size()
	var deltas: Array[float] = []
	var total := 0.0
	for y in range(0, size.y, BRIDGE_SAMPLE_STRIDE):
		for x in range(0, size.x, BRIDGE_SAMPLE_STRIDE):
			var a := reference.get_pixel(x, y)
			var b := candidate.get_pixel(x, y)
			var d := (absf(a.r - b.r) + absf(a.g - b.g) + absf(a.b - b.b)) / 3.0
			deltas.append(d)
			total += d
	deltas.sort()
	var mean := total / float(maxi(deltas.size(), 1))
	var p95 := deltas[int(floor(float(deltas.size() - 1) * 0.95))]
	if mean > BRIDGE_MEAN_RGB_DELTA_MAX or p95 > BRIDGE_P95_RGB_DELTA_MAX:
		return "%s bridge mismatch mean=%.6f p95=%.6f budgets %.6f/%.6f" % [
			label, mean, p95, BRIDGE_MEAN_RGB_DELTA_MAX, BRIDGE_P95_RGB_DELTA_MAX]
	print("[wg10-manual-stress] bridge_match label=%s samples=%d stride=%d mean=%.6f p95=%.6f budgets %.6f/%.6f" % [
		label, deltas.size(), BRIDGE_SAMPLE_STRIDE, mean, p95, BRIDGE_MEAN_RGB_DELTA_MAX, BRIDGE_P95_RGB_DELTA_MAX])
	return ""

func _percentile(samples: Array[float], fraction: float) -> float:
	if samples.is_empty():
		return 0.0
	samples.sort()
	var idx := int(ceil(float(samples.size()) * fraction)) - 1
	idx = clampi(idx, 0, samples.size() - 1)
	return float(samples[idx])
