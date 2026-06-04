extends SceneTree

# Scene-level motion hardening for the WG10 progression harness.
# This drives the actual progression scene across page boundaries and records
# the owner-visible failure classes that a static step smoke cannot catch:
# update hitches, hidden visible tiles, pool full events, and abrupt repage bursts.

const SCENE := "res://worldgen_terrain/harness/wg10_progression_review.tscn"

const SPEED := 8000.0
const DT := 1.0 / 60.0
const WARM_FRAMES := 160
const MEASURE_FRAMES := 360
const MAX_UPDATE_P95_MS := 16.7
const MAX_UPDATE_P99_MS := 16.7
const MAX_UPDATE_MAX_MS := 50.0
const MAX_REPAGE_FRAME := 9

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-progression-motion] status=skip reason=no-render-device")
		return 2

	var packed := load(SCENE)
	if packed == null:
		push_error("[wg10-progression-motion] cannot load %s" % SCENE)
		return 1

	var scene: Node = packed.instantiate()
	get_root().add_child(scene)
	for _i in range(80):
		await process_frame

	var errs: Array[String] = []
	_expect(scene.has_method("set_probe_mode"), "scene missing set_probe_mode", errs)
	_expect(scene.has_method("update_for_probe"), "scene missing update_for_probe", errs)
	_expect(scene.has_method("debug_tile_states"), "scene missing debug_tile_states", errs)
	_expect(scene.has_method("debug_streamer_stats"), "scene missing debug_streamer_stats", errs)
	if not errs.is_empty():
		scene.queue_free()
		for err in errs:
			push_error(err)
		print("[wg10-progression-motion] status=fail errors=%d" % errs.size())
		return 1

	scene.call("set_probe_mode", true)
	var steps := int(scene.call("step_count")) if scene.has_method("step_count") else 0
	for i in range(steps):
		var result: Dictionary = _run_step(scene, i)
		if int(result.get("rc", 1)) != 0:
			errs.append("%s: %s" % [str(result.get("step_id", i)), str(result.get("error", "failed"))])

	scene.queue_free()
	scene = null
	await process_frame
	await process_frame

	if not errs.is_empty():
		for err in errs:
			push_error(err)
		print("[wg10-progression-motion] status=fail errors=%d" % errs.size())
		return 1

	print("[wg10-progression-motion] status=pass steps=%d speed=%d frames=%d" % [
		steps,
		int(SPEED),
		MEASURE_FRAMES,
	])
	return 0

func _run_step(scene: Node, index: int) -> Dictionary:
	var ok := bool(scene.call("set_step_index", index))
	if not ok:
		return {"rc": 1, "step_id": index, "error": "set_step_index failed"}
	scene.call("set_probe_mode", true)

	var heading := Vector2(1.0, 0.0)
	var pos := Vector2.ZERO
	for _w in range(WARM_FRAMES):
		scene.call("update_for_probe", pos.x, pos.y, heading.x * SPEED, heading.y * SPEED)

	var prev: PackedInt64Array = scene.call("debug_tile_states")
	var expected_tiles := int(prev.size() / 3)
	var visible0 := _visible_count(prev)
	var snapshot0: Dictionary = scene.call("debug_progression_snapshot")
	var stats0: Dictionary = snapshot0.get("stats", {})
	var streamer0: Dictionary = scene.call("debug_streamer_stats")
	var stream0 := int(stats0.get("created", 0)) + int(stats0.get("recomputed", 0))
	var full0 := int(stats0.get("full_events", 0)) + int(streamer0.get("full_events", 0))

	var update_samples: Array[float] = []
	var update_sum := 0.0
	var update_max := 0.0
	var acquired_max := 0
	var hide_total := 0
	var show_total := 0
	var repage_total := 0
	var repage_frame_max := 0
	var hidden_frames := 0
	var max_hidden := 0
	var coarsest_hide := 0
	var levels := int(expected_tiles / 9)
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
		scene.call("update_for_probe", pos.x, pos.y, vel.x, vel.y)
		var update_ms := float(Time.get_ticks_usec() - tu0) / 1000.0
		update_samples.append(update_ms)
		update_sum += update_ms
		update_max = maxf(update_max, update_ms)

		var streamer_frame: Dictionary = scene.call("debug_streamer_stats")
		acquired_max = maxi(acquired_max, int(streamer_frame.get("acquired_this_frame", 0)))

		var states: PackedInt64Array = scene.call("debug_tile_states")
		var frame_hidden := 0
		var frame_repage := 0
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
				frame_repage += 1
			t += 1
		repage_frame_max = maxi(repage_frame_max, frame_repage)
		if frame_hidden > 0:
			hidden_frames += 1
			max_hidden = maxi(max_hidden, frame_hidden)
		prev = states

	update_samples.sort()
	var update_mean := update_sum / float(maxi(update_samples.size(), 1))
	var update_p95 := _percentile(update_samples, 0.95)
	var update_p99 := _percentile(update_samples, 0.99)

	var snapshot1: Dictionary = scene.call("debug_progression_snapshot")
	var stats1: Dictionary = snapshot1.get("stats", {})
	var streamer1: Dictionary = scene.call("debug_streamer_stats")
	var stream_events := int(stats1.get("created", 0)) + int(stats1.get("recomputed", 0)) - stream0
	var full_events := int(stats1.get("full_events", 0)) + int(streamer1.get("full_events", 0)) - full0
	var step_id := str(snapshot1.get("step_id", index))
	var runtime_mode := str(snapshot1.get("runtime_mode", "missing"))
	var status := str(snapshot1.get("step_status", "missing"))
	var resident := int(stats1.get("resident", 0))

	print("[wg10-progression-motion] step=%d id=%s status=%s runtime=%s cpu_mean=%.3fms cpu_p95=%.3fms cpu_p99=%.3fms cpu_max=%.3fms acquired_max=%d stream_events=%d resident=%d repage=%d repage_frame_max=%d hide=%d show=%d hidden_frames=%d max_hidden=%d full_events=%d visible0=%d/%d" % [
		index,
		step_id,
		status,
		runtime_mode,
		update_mean,
		update_p95,
		update_p99,
		update_max,
		acquired_max,
		stream_events,
		resident,
		repage_total,
		repage_frame_max,
		hide_total,
		show_total,
		hidden_frames,
		max_hidden,
		full_events,
		visible0,
		expected_tiles,
	])

	var errs: Array[String] = []
	if str(snapshot1.get("last_config_error", "")) != "":
		errs.append("configure error: %s" % str(snapshot1.get("last_config_error", "")))
	if expected_tiles <= 0:
		errs.append("no tile state rows")
	if visible0 != expected_tiles:
		errs.append("warmup visible=%d, expected=%d" % [visible0, expected_tiles])
	if stream_events <= 0:
		errs.append("no stream events during motion")
	if repage_total <= 0:
		errs.append("no visible repages during motion")
	if repage_frame_max > MAX_REPAGE_FRAME:
		errs.append("repage_frame_max %d > %d" % [repage_frame_max, MAX_REPAGE_FRAME])
	if hide_total > 0:
		errs.append("visible tiles hid during motion: hide=%d show=%d hidden_frames=%d max_hidden=%d" % [
			hide_total,
			show_total,
			hidden_frames,
			max_hidden,
		])
	if coarsest_hide > 0:
		errs.append("coarsest tiles hid after warmup: %d" % coarsest_hide)
	if full_events > 0:
		errs.append("pool full events during motion: %d" % full_events)
	if update_p95 > MAX_UPDATE_P95_MS:
		errs.append("cpu_update_p95 %.3fms > %.3fms" % [update_p95, MAX_UPDATE_P95_MS])
	if update_p99 > MAX_UPDATE_P99_MS:
		errs.append("cpu_update_p99 %.3fms > %.3fms" % [update_p99, MAX_UPDATE_P99_MS])
	if update_max > MAX_UPDATE_MAX_MS:
		errs.append("cpu_update_max %.3fms > %.3fms" % [update_max, MAX_UPDATE_MAX_MS])

	if not errs.is_empty():
		return {"rc": 1, "step_id": step_id, "error": "; ".join(errs)}
	return {"rc": 0, "step_id": step_id, "error": ""}

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

func _expect(condition: bool, message: String, errs: Array[String]) -> void:
	if not condition:
		errs.append(message)
