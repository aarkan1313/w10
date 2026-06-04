extends SceneTree

# Owner fly-scene motion diagnostic for modes 1/2/3. This uses the shared producer/runtime helpers
# from mountain_fly_review.tscn, then records the two owner-visible failure classes:
# - synchronous update hitches while pages stream,
# - visible tile HIDE/SHOW churn while flying across page boundaries.

const PRODUCERS := "res://worldgen_terrain/harness/mountain_fly_producers.gd"
const RUNTIME_CONFIG := "res://worldgen_terrain/harness/mountain_fly_runtime_config.gd"

const MODES := ["REFERENCE", "MOUNTAIN", "WORLD"]
const PRESET_NETWORK := "network_ref"
const SPEED := 8000.0
const DT := 1.0 / 60.0
const WARM_FRAMES := 160
const MEASURE_FRAMES := 360
const MAX_UPDATE_P95_MS := 16.7
const MAX_UPDATE_MAX_MS := 50.0

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10TerrainView"):
		push_error("[wg10-modes-perf] Wg10TerrainView not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-modes-perf] status=skip reason=no-render-device"); return 2

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
		print("[wg10-modes-perf] status=fail errors=%d" % errs.size())
		return 1

	print("[wg10-modes-perf] status=pass modes=%d speed=%d frames=%d" % [
		MODES.size(), int(SPEED), MEASURE_FRAMES])
	return 0

func _run_mode(runtime: Object, mode: String) -> Dictionary:
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
	get_root().add_child(rings)
	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	var relief_scale := float(producer.view_relief_scale(float(runtime.default_relief_scale())))
	runtime.configure_view(view, pool, streamer, rings, bool(runtime.default_morph_enabled()), relief_scale)

	var heading := Vector2(1.0, 0.0)
	var pos := Vector2.ZERO
	for _w in range(WARM_FRAMES):
		view.call("update", pos.x, pos.y, heading.x * SPEED, heading.y * SPEED)

	var prev: PackedInt64Array = rings.call("debug_tile_states")
	var expected_tiles := int(runtime.num_levels()) * 9
	var visible0 := _visible_count(prev)

	var st0: Dictionary = pool.call("stats")
	var stream0 := int(st0.get("created", 0)) + int(st0.get("recomputed", 0))
	var streamer0: Dictionary = streamer.call("stats")
	var full0 := int(st0.get("full_events", 0)) + int(streamer0.get("full_events", 0))

	var update_samples: Array[float] = []
	var update_sum := 0.0
	var update_max := 0.0
	var acquired_max := 0
	var hide_total := 0
	var show_total := 0
	var repage_total := 0
	var coarsest_hide := 0
	var hidden_frames := 0
	var max_hidden := 0
	var levels := int(runtime.num_levels())
	var headings := [
		Vector2(1.0, 0.0),
		Vector2(0.70710678, 0.70710678),
		Vector2(0.0, 1.0),
		Vector2(-0.70710678, 0.70710678),
	]

	for f in range(MEASURE_FRAMES):
		heading = headings[int(f / 90) % headings.size()]
		var vel := heading * SPEED
		pos += vel * DT
		var tu0 := Time.get_ticks_usec()
		view.call("update", pos.x, pos.y, vel.x, vel.y)
		var update_ms := float(Time.get_ticks_usec() - tu0) / 1000.0
		update_samples.append(update_ms)
		update_sum += update_ms
		update_max = maxf(update_max, update_ms)

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

	update_samples.sort()
	var update_mean := update_sum / float(maxi(update_samples.size(), 1))
	var update_p95 := _percentile(update_samples, 0.95)
	var update_p99 := _percentile(update_samples, 0.99)

	var st1: Dictionary = pool.call("stats")
	var streamer1: Dictionary = streamer.call("stats")
	var stream_events := int(st1.get("created", 0)) + int(st1.get("recomputed", 0)) - stream0
	var full_events := int(st1.get("full_events", 0)) + int(streamer1.get("full_events", 0)) - full0
	var resident := int(st1.get("resident", 0))
	var runtime_mode := str(pool.call("biome_runtime_mode"))
	var biome_path := bool(pool.call("uses_biome_path"))

	rings.call("unbind_all")
	pool.call("free_all")
	rings.queue_free()

	print("[wg10-modes-perf] mode=%s runtime=%s biome_path=%s cpu_mean=%.3fms cpu_p95=%.3fms cpu_p99=%.3fms cpu_max=%.3fms acquired_max=%d stream_events=%d resident=%d repage=%d hide=%d show=%d hidden_frames=%d max_hidden=%d full_events=%d visible0=%d/%d" % [
		mode, runtime_mode, str(biome_path), update_mean, update_p95, update_p99, update_max,
		acquired_max, stream_events, resident, repage_total, hide_total, show_total,
		hidden_frames, max_hidden, full_events, visible0, expected_tiles])

	var errs: Array[String] = []
	if visible0 != expected_tiles:
		errs.append("warmup visible=%d, expected=%d" % [visible0, expected_tiles])
	if stream_events <= 0:
		errs.append("no stream events during motion")
	if repage_total <= 0:
		errs.append("no visible repages during motion")
	if hide_total > 0:
		errs.append("visible tiles hid during motion: hide=%d show=%d hidden_frames=%d max_hidden=%d" % [
			hide_total, show_total, hidden_frames, max_hidden])
	if coarsest_hide > 0:
		errs.append("coarsest tiles hid after warmup: %d" % coarsest_hide)
	if update_p95 > MAX_UPDATE_P95_MS:
		errs.append("cpu_update_p95 %.3fms > %.3fms" % [update_p95, MAX_UPDATE_P95_MS])
	if update_max > MAX_UPDATE_MAX_MS:
		errs.append("cpu_update_max %.3fms > %.3fms" % [update_max, MAX_UPDATE_MAX_MS])

	if not errs.is_empty():
		return {"rc": 1, "error": "; ".join(errs)}
	return {"rc": 0, "error": ""}

func _configure_producer(producer: Object, mode: String) -> String:
	if not bool(producer.set_mode_label(mode)):
		return "invalid mode %s" % mode
	if not bool(producer.set_preset_label(PRESET_NETWORK)):
		return "invalid preset %s" % PRESET_NETWORK
	return ""

func _visible_count(states: PackedInt64Array) -> int:
	var count := 0
	var t := 0
	while t * 3 < states.size():
		if int(states[t * 3]) == 1:
			count += 1
		t += 1
	return count

func _percentile(samples: Array[float], fraction: float) -> float:
	if samples.is_empty():
		return 0.0
	var idx := int(ceil(float(samples.size()) * fraction)) - 1
	idx = clampi(idx, 0, samples.size() - 1)
	return float(samples[idx])
