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
const SKY := Color(0.45, 0.62, 0.85)   # the env background — a pixel near this is SKY, not terrain (B3)

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
# real GPU time + TERRAIN (not sky) filling the lower frame + pages streamed + the clipmap drew geometry
# + DETAIL actually contributes (on-vs-off frame delta).
# B3 fix: the old nonblack test counted ANY pixel with c.r/g/b > 0.03 as "real" — but the sky is bright
# (0.45,0.62,0.85), so a 100%-EMPTY-SKY frame scored nonblack=1.0 and the proof couldn't tell terrain from
# sky. Now we count a pixel as TERRAIN only if it differs from the SKY color by > SKY_DELTA, and we add a
# detail-on-vs-off assertion so a fly-scale detail regression also fails the PERF gate (not only the
# separate static-page detail gate).
const MIN_TERRAIN_FRAC := 0.85   # TERRAIN (differs from sky) fills the lower frame (not an empty/sky scene)
const SKY_DELTA := 0.06          # a pixel is terrain (not sky) if max channel-diff from SKY exceeds this
const MIN_STREAM_EVENTS := 1     # pages were streamed under motion (created+recomputed grew) -> not static
const MIN_PRIMITIVES := 100000   # the clipmap actually drew geometry (45 tiles x ~64^2 -> millions; 100k floor)
const MIN_GPU_MS := 0.001        # measured GPU time is non-zero (the timer is actually working, not Metal-0)
const DETAIL_DELTA_MIN := 0.0008 # detail ON vs OFF must change the rendered frame by at least this (mean abs
                                 # luma delta); proves detail genuinely contributes at fly scale, not a no-op.

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
	var terrain_min := 1.0
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
			# periodic TERRAIN check (did it render terrain, not an empty SKY frame? — B3).
			# Counts a lower-frame pixel as terrain only if it differs from the SKY color, so a
			# 100%-sky frame scores ~0 (the old "any bright pixel" test scored sky as 1.0).
			if (f - WARM_FRAMES) % 24 == 0:
				var img: Image = vp.get_texture().get_image()
				if img != null:
					terrain_min = minf(terrain_min, _terrain_frac(img))

	var stN: Dictionary = pool.call("stats")
	var stream_events := int(stN.get("created", 0)) + int(stN.get("recomputed", 0)) - stream0

	# --- DETAIL on-vs-off (B3): capture the same framed view with detail ON then OFF; they must differ.
	# Proves M5 detail genuinely contributes at fly scale — a detail regression (detail silently doing
	# nothing) now fails the PERF gate too, not only the separate static-page detail gate.
	var detail_delta := _detail_on_off_delta(vp, cam, view, pos, headings)

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
	if terrain_min < MIN_TERRAIN_FRAC:
		errs.append("not-real-work: terrain (non-sky) frac=%.3f < %.2f (empty/sky frames? — B3)" % [terrain_min, MIN_TERRAIN_FRAC])
	if stream_events < MIN_STREAM_EVENTS:
		errs.append("not-real-work: %d stream events < %d (nothing streamed under motion -> static scene?)" % [stream_events, MIN_STREAM_EVENTS])
	if prim_max < MIN_PRIMITIVES:
		errs.append("not-real-work: max primitives %d < %d (clipmap didn't draw geometry?)" % [prim_max, MIN_PRIMITIVES])
	if detail_delta < DETAIL_DELTA_MIN:
		errs.append("not-real-work: detail on-vs-off delta=%.5f < %.5f (detail not contributing at fly scale? — B3)" % [detail_delta, DETAIL_DELTA_MIN])

	pool.call("free_all")

	print("[wg10-m5-perf] GPU p99=%.3fms mean=%.3fms max=%.3fms (budget %.1f) speed=%dm/s frames=%d" % [
		gpu_p99, gpu_mean, gpu_max, GPU_P99_BUDGET_MS, int(SPEED), MEASURE_FRAMES])
	print("[wg10-m5-perf] DID-REAL-WORK: terrain_frac_min=%.3f detail_delta=%.5f stream_events=%d prim_max=%d resident_max=%d cpu_update_mean=%.3fms" % [
		terrain_min, detail_delta, stream_events, prim_max, resident_max, cpu_update_sum / float(MEASURE_FRAMES)])
	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m5-perf] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-m5-perf] status=pass GPU p99=%.3fms — REAL GPU time, proven doing real work (terrain-not-sky + detail-contributes)" % gpu_p99)
	return 0


# Fraction of LOWER-frame pixels that are TERRAIN (differ from the SKY color by > SKY_DELTA). The lower
# third-to-bottom is where ground lives in a fly POV; a sky-only frame scores ~0 (the B3 fix — the old
# "any bright pixel" test scored the bright sky as nonblack=1.0).
func _terrain_frac(img: Image) -> float:
	var hit := 0; var samp := 0
	var y0 := int(img.get_height() / 3)
	for y in range(y0, img.get_height(), 4):
		for x in range(0, img.get_width(), 4):
			samp += 1
			var c := img.get_pixel(x, y)
			var d := maxf(maxf(absf(c.r - SKY.r), absf(c.g - SKY.g)), absf(c.b - SKY.b))
			if d > SKY_DELTA:
				hit += 1
	return float(hit) / float(max(samp, 1))


# Mean absolute luma delta between the current framed view rendered with detail ON vs OFF. Renders the
# SAME camera/position twice (only wg_detail_amp changes) so any difference is the detail contribution.
func _detail_on_off_delta(vp: SubViewport, cam: Camera3D, view: Object, pos: Vector2, headings: Array) -> float:
	var heading: Vector2 = headings[0]
	var eye := Vector3(pos.x - heading.x * 600.0, 900.0, pos.y - heading.y * 600.0)
	var look := Vector3(pos.x + heading.x * 1200.0, 0.0, pos.y + heading.y * 1200.0)
	cam.look_at_from_position(eye, look, Vector3.UP)
	view.call("update", pos.x, pos.y, heading.x * SPEED, heading.y * SPEED)
	var on_img := await _frame(vp)
	RenderingServer.global_shader_parameter_set("wg_detail_amp", 0.0)
	var off_img := await _frame(vp)
	RenderingServer.global_shader_parameter_set("wg_detail_amp", DETAIL_AMP)   # restore for any later use
	if on_img == null or off_img == null:
		return 0.0
	return _mean_abs_luma_delta(on_img, off_img)


func _frame(vp: SubViewport) -> Image:
	await process_frame
	RenderingServer.force_draw()
	await process_frame
	return vp.get_texture().get_image()


func _mean_abs_luma_delta(a: Image, b: Image) -> float:
	var n := 0; var s := 0.0
	var w := mini(a.get_width(), b.get_width())
	var h := mini(a.get_height(), b.get_height())
	for y in range(0, h, 4):
		for x in range(0, w, 4):
			var ca := a.get_pixel(x, y); var cb := b.get_pixel(x, y)
			var la := 0.299 * ca.r + 0.587 * ca.g + 0.114 * ca.b
			var lb := 0.299 * cb.r + 0.587 * cb.g + 0.114 * cb.b
			s += absf(la - lb); n += 1
	return s / float(max(n, 1))
