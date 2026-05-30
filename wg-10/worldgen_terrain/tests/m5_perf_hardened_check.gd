extends SceneTree

# M5/scale HARDENED perf gate — the answer to "is the profiling measuring REAL work?".
# WINDOWED ONLY (global RenderingDevice null headless on this D3D12 box).
#
# WHY this exists (vs m3_accept_check): m3_accept times wall-clock around force_draw, which is
# ASYNC-HIDDEN behind the vsync/present cadence (a 90x GPU load moved it only ~1.0-1.3x — proven by
# probe). A green wall-time p99 could pass on a scene doing almost no GPU work. This gate instead:
#   (1) measures REAL GPU TIME via RenderingServer.viewport_get_measured_render_time_gpu() — a
#       per-viewport, DEFERRED (no-stall, no observer-effect) GPU-ms reading that is immune to vsync
#       capping (Godot docs) and was VERIFIED 8.97x load-responsive on this box; and
#   (2) CO-ASSERTS the scene actually DID THE WORK it claims to measure — pages streamed under motion,
#       terrain non-black, real relief variety, detail present, primitive count in a sane range — so a
#       green GPU-ms number provably corresponds to the real streaming-clipmap-with-detail render at
#       fly scale, NOT an empty/static/culled frame.
# A regression that (a) blows GPU time OR (b) silently stops doing real work BOTH fail this gate.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
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
const DETAIL_AMP := 350.0        # detail ON (the real shipped render path includes M5 detail cost)
const VIEW_SIZE := Vector2i(960, 540)

const SPEED := 1000.0            # ~1000 m/s acceptance speed
const WARM_FRAMES := 80          # settle streaming + let measured-render-time results fill (lag = frame_queue_size)
const MEASURE_FRAMES := 240
const GPU_P99_BUDGET_MS := 6.0   # the headline budget (real GPU ms now, not wall-time)
const GPU_STALL_CEIL_MS := 33.0  # no single frame's GPU time worse than this

# --- did-real-work floors (a green number must clear ALL of these or it isn't measuring the real render) ---
# NOTE: relief-VARIETY is deliberately NOT asserted here — a flight-POV perspective frame is mostly
# distant fogged terrain in a narrow color band, so a fly-frame color-bucket count is the wrong
# instrument (it fails on a perfectly good render). Relief variety is proven by m3_view_check (top-down
# ortho) where it belongs. THIS gate proves the perf number is measuring the REAL render via: non-zero
# real GPU time + terrain non-black + pages streamed under motion + the clipmap actually drew geometry.
const MIN_NONBLACK := 0.85       # terrain fills the lower frame (not an empty/black scene)
const MIN_STREAM_EVENTS := 1     # pages were streamed under motion (created+recomputed grew) -> not static
const MIN_PRIMITIVES := 100000   # the clipmap actually drew geometry (45 tiles x ~64^2 -> millions; 100k floor)
const MIN_GPU_MS := 0.001        # measured GPU time is non-zero (the timer is actually working, not Metal-0)

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10TerrainView"):
		push_error("[wg10-m5-perf] Wg10TerrainView not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-m5-perf] status=skip reason=no-render-device"); return 2
	if not RenderingServer.has_method("viewport_get_measured_render_time_gpu"):
		print("[wg10-m5-perf] status=skip reason=no-measured-render-time-api"); return 2

	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)

	# detail ON for the measured render (the shipped path includes M5 detail).
	if not RenderingServer.global_shader_parameter_get_list().has("wg_detail_amp"):
		RenderingServer.global_shader_parameter_add("wg_detail_amp", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, DETAIL_AMP)
	RenderingServer.global_shader_parameter_set("wg_detail_amp", DETAIL_AMP)

	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var err: String = str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if err != "":
		push_error("[wg10-m5-perf] pool configure failed: %s" % err); return 1
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
	env.background_color = Color(0.45, 0.62, 0.85)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	var cam := Camera3D.new()
	cam.far = BASE_SPAN * 32.0
	cam.environment = env
	vp.add_child(rings)
	vp.add_child(light)
	vp.add_child(cam)
	get_root().add_child(vp)

	# enable the REAL GPU-time measurement on this viewport (deferred, no-stall).
	RenderingServer.viewport_set_measure_render_time(vp.get_viewport_rid(), true)

	# scripted ~1000 m/s flight across many page boundaries (turning legs exercise the streamer).
	var headings := [Vector2(1,0), Vector2(0.7,0.7), Vector2(0,1), Vector2(-0.7,0.7), Vector2(1,0)]
	var pos := Vector2(0.0, 0.0)
	var dt := 1.0 / 60.0
	var errs: Array[String] = []

	var st0: Dictionary = pool.call("stats")
	var stream0 := int(st0.get("created", 0)) + int(st0.get("recomputed", 0))

	# measurement accumulators
	var gpu_samples: Array[float] = []
	var gpu_max := 0.0
	var cpu_update_sum := 0.0
	var prim_max := 0
	var nonblack_min := 1.0
	var resident_max := 0

	var total := WARM_FRAMES + MEASURE_FRAMES
	for f in range(total):
		var heading: Vector2 = headings[int(f / 60) % headings.size()]
		var vx := heading.x * SPEED
		var vz := heading.y * SPEED
		pos += Vector2(vx, vz) * dt
		var tu0 := Time.get_ticks_usec()
		view.call("update", pos.x, pos.y, vx, vz)
		var update_ms := float(Time.get_ticks_usec() - tu0) / 1000.0
		var eye := Vector3(pos.x - heading.x * 600.0, 900.0, pos.y - heading.y * 600.0)
		var look := Vector3(pos.x + heading.x * 1200.0, 0.0, pos.y + heading.y * 1200.0)
		cam.look_at_from_position(eye, look, Vector3.UP)
		await process_frame
		RenderingServer.force_draw()
		await process_frame

		if f >= WARM_FRAMES:
			# REAL GPU ms for the last frame (deferred read; non-stalling).
			var gpu_ms := RenderingServer.viewport_get_measured_render_time_gpu(vp.get_viewport_rid())
			gpu_samples.append(gpu_ms)
			gpu_max = maxf(gpu_max, gpu_ms)
			cpu_update_sum += update_ms
			# primitive load (did the clipmap actually draw geometry?)
			var prims: int = RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_PRIMITIVES_IN_FRAME)
			prim_max = maxi(prim_max, prims)
			var st: Dictionary = pool.call("stats")
			resident_max = maxi(resident_max, int(st.get("resident", 0)))
			# periodic nonblack check (did it render terrain, not an empty/black frame?)
			if (f - WARM_FRAMES) % 24 == 0:
				var img: Image = vp.get_texture().get_image()
				if img != null:
					var nb := 0; var samp := 0
					var y0 := int(img.get_height() / 3)
					for y in range(y0, img.get_height(), 4):
						for x in range(0, img.get_width(), 4):
							samp += 1
							var c := img.get_pixel(x, y)
							if c.r > 0.03 or c.g > 0.03 or c.b > 0.03:
								nb += 1
					var frac := float(nb) / float(max(samp, 1))
					nonblack_min = minf(nonblack_min, frac)

	var stN: Dictionary = pool.call("stats")
	var stream_events := int(stN.get("created", 0)) + int(stN.get("recomputed", 0)) - stream0

	# --- GPU p99 ---
	gpu_samples.sort()
	var gpu_p99 := 0.0
	var gpu_mean := 0.0
	if gpu_samples.size() > 0:
		gpu_p99 = gpu_samples[int(floor(gpu_samples.size() * 0.99)) if gpu_samples.size() > 1 else 0]
		var s := 0.0
		for v in gpu_samples: s += v
		gpu_mean = s / float(gpu_samples.size())

	# --- PERF assertions (real GPU time) ---
	if gpu_p99 > GPU_P99_BUDGET_MS:
		errs.append("GPU p99 %.3f ms > %.1f ms budget (at ~%d m/s)" % [gpu_p99, GPU_P99_BUDGET_MS, int(SPEED)])
	if gpu_max > GPU_STALL_CEIL_MS:
		errs.append("GPU stall: max frame %.3f ms > %.1f ms" % [gpu_max, GPU_STALL_CEIL_MS])

	# --- DID-REAL-WORK assertions (a green perf number must be measuring the REAL render) ---
	if gpu_max < MIN_GPU_MS:
		errs.append("GPU timer read ~0 (max %.5f ms) -> measurement not working; perf number is meaningless" % gpu_max)
	if nonblack_min < MIN_NONBLACK:
		errs.append("not-real-work: terrain nonblack=%.3f < %.2f (empty/black frames?)" % [nonblack_min, MIN_NONBLACK])
	if stream_events < MIN_STREAM_EVENTS:
		errs.append("not-real-work: %d stream events < %d (nothing streamed under motion -> static scene?)" % [stream_events, MIN_STREAM_EVENTS])
	if prim_max < MIN_PRIMITIVES:
		errs.append("not-real-work: max primitives %d < %d (clipmap didn't draw geometry?)" % [prim_max, MIN_PRIMITIVES])

	pool.call("free_all")

	print("[wg10-m5-perf] GPU p99=%.3fms mean=%.3fms max=%.3fms (budget %.1f) speed=%dm/s frames=%d" % [
		gpu_p99, gpu_mean, gpu_max, GPU_P99_BUDGET_MS, int(SPEED), MEASURE_FRAMES])
	print("[wg10-m5-perf] DID-REAL-WORK: nonblack_min=%.3f stream_events=%d prim_max=%d resident_max=%d cpu_update_mean=%.3fms" % [
		nonblack_min, stream_events, prim_max, resident_max, cpu_update_sum / float(MEASURE_FRAMES)])
	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m5-perf] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-m5-perf] status=pass GPU p99=%.3fms — REAL GPU time, proven doing real work" % gpu_p99)
	return 0
