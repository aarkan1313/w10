extends SceneTree

# Live mountain runtime motion gate. This is the automated version of the fly-scene HUD's
# HIDE/SHOW/REPAGE log: warm the owner MOUNTAIN/network runtime to full visible residency, then fly
# at sprint speed across display page boundaries. After warmup, visible tiles may REPAGE at
# boundaries, but they must not HIDE; hidden fine tiles are the visible pop-in the owner reported.

const PRODUCERS := "res://worldgen_terrain/harness/mountain_fly_producers.gd"
const RUNTIME_CONFIG := "res://worldgen_terrain/harness/mountain_fly_runtime_config.gd"

const MODE_MOUNTAIN := "MOUNTAIN"
const PRESET_NETWORK := "network_ref"
const SPEED := 8000.0
const DT := 1.0 / 60.0
const WARM_FRAMES := 140
const MEASURE_FRAMES := 360

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10TerrainView"):
		push_error("[wg10-mountain-churn] Wg10TerrainView not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-mountain-churn] status=skip reason=no-render-device"); return 2

	var runtime: Object = load(RUNTIME_CONFIG).new()
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var producer: Object = load(PRODUCERS).new()
	if not bool(producer.set_mode_label(MODE_MOUNTAIN)):
		push_error("[wg10-mountain-churn] invalid mode"); return 1
	if not bool(producer.set_preset_label(PRESET_NETWORK)):
		push_error("[wg10-mountain-churn] invalid preset"); return 1
	var err: String = str(producer.configure(pool))
	if err != "":
		push_error("[wg10-mountain-churn] configure failed: %s" % err); return 1

	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	runtime.configure_streamer(streamer, pool)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	runtime.configure_rings(rings)
	get_root().add_child(rings)
	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	var relief_scale := float(producer.view_relief_scale(float(runtime.default_relief_scale())))
	var relief_ref := float(producer.view_relief_ref(float(runtime.default_relief_ref()), float(runtime.default_relief_scale())))
	runtime.configure_view(view, pool, streamer, rings, bool(runtime.default_morph_enabled()), relief_scale, relief_ref)

	var heading := Vector2(1.0, 0.0)
	var pos := Vector2.ZERO
	for _w in range(WARM_FRAMES):
		view.call("update", pos.x, pos.y, heading.x * SPEED, heading.y * SPEED)

	var prev: PackedInt64Array = rings.call("debug_tile_states")
	var visible0 := _visible_count(prev)
	var expected_tiles := int(runtime.num_levels()) * 9
	var errs: Array[String] = []
	if visible0 != expected_tiles:
		errs.append("warmup visible=%d, expected=%d" % [visible0, expected_tiles])

	var st0: Dictionary = pool.call("stats")
	var stream0 := int(st0.get("created", 0)) + int(st0.get("recomputed", 0))

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
		view.call("update", pos.x, pos.y, vel.x, vel.y)
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

	var st1: Dictionary = pool.call("stats")
	var stream_events := int(st1.get("created", 0)) + int(st1.get("recomputed", 0)) - stream0
	var resident := int(st1.get("resident", 0))

	rings.call("unbind_all")
	pool.call("free_all")
	rings.queue_free()
	await process_frame

	if stream_events <= 0:
		errs.append("no stream events during motion; path did not exercise page boundaries")
	if repage_total <= 0:
		errs.append("no repages during motion; path did not exercise visible boundary transitions")
	if hide_total > 0:
		errs.append("visible tiles hid during motion: hide=%d show=%d hidden_frames=%d max_hidden=%d" % [
			hide_total, show_total, hidden_frames, max_hidden])
	if coarsest_hide > 0:
		errs.append("coarsest tiles hid after warmup: %d" % coarsest_hide)

	print("[wg10-mountain-churn] motion frames=%d speed=%d stream_events=%d resident=%d repage=%d hide=%d show=%d hidden_frames=%d max_hidden=%d" % [
		MEASURE_FRAMES, int(SPEED), stream_events, resident, repage_total, hide_total, show_total, hidden_frames, max_hidden])
	if not errs.is_empty():
		for e in errs:
			push_error(e)
		print("[wg10-mountain-churn] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-mountain-churn] status=pass")
	return 0

func _visible_count(states: PackedInt64Array) -> int:
	var count := 0
	var t := 0
	while t * 3 < states.size():
		if int(states[t * 3]) == 1:
			count += 1
		t += 1
	return count
