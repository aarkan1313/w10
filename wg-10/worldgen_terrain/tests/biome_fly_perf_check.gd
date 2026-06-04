extends SceneTree

# Biome-fly DID-REAL-WORK perf gate (Task 6 §B) — the §5 drainage-priority MEASUREMENT.
# WINDOWED ONLY (global RenderingDevice null headless on this D3D12 box).
#
# WHAT this measures: the MOUNTAIN biome GPU producer (configure_biome -> build_biome_page_context
# with inline flow relaxation at FLOW_ITERS) streaming through the SAME M3 pipeline (pool -> streamer
# -> rings -> Wg10TerrainView) under a ~1000 m/s synthetic flight. It records the REAL GPU-time p99
# via RenderingServer.viewport_get_measured_render_time_gpu() (deferred, no-stall, vsync-immune;
# verified 8.97x load-responsive on this box).
#
# PER-SPEC VERDICT POLICY (the key difference from m5_perf_hardened): an OVER-budget p99 is a VALID
# RESULT, NOT a fail — this gate exists to MEASURE the inline-flow cost so §5 can decide drainage
# delivery (live-per-page flow vs coarse-drainage-fact). The gate FAILS only on DEGENERATE / NO-WORK:
#   * zero pages streamed (nothing produced under motion),
#   * the biome path is NOT actually active (uses_biome_path()==false -> silent legacy fallback),
#   * a black / empty-sky frame (nothing rendered),
#   * the GPU timer read ~0 (the measurement instrument isn't working -> the number is meaningless).
# A green p99 with no real work is FORBIDDEN: a degenerate frame can't false-pass this gate.

# --- BIOME (mountain GPU producer) config — matches mountain_fly_review.gd ---
const PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const MOUNTAIN := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const APRON_PX := 160
const FEATURE_SPAN_M := 90000.0
const FLOW_ITERS := 192          # measured production convergence count

const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const PAGE_PX := 256
const SEED := 1337
const NUM_LEVELS := 5
const BASE_SPAN := 8192.0
const GRID_RES := 128
const RADIUS_PAGES := 1
const LEAD_SECONDS := 0.5
const MAX_PER_FRAME := 4
const CAPACITY := 96
const MORPH_REGION := 0.15
const RELIEF_SCALE := 0.25
const RELIEF_REF := 2000.0
const DETAIL_AMP := 350.0        # detail ON (the shipped render path includes M5 detail cost)
const VIEW_SIZE := Vector2i(960, 540)
const SKY := Color(0.45, 0.62, 0.85)   # the env background — a pixel near this is SKY, not terrain (B3)

const SPEED := 1000.0            # ~1000 m/s acceptance speed
const WARM_FRAMES := 80          # settle streaming + let measured-render-time results fill (lag = frame_queue_size)
const MEASURE_FRAMES := 240
const GPU_P99_BUDGET_MS := 6.0   # the headline budget (real GPU ms). OVER is a valid result here, not a fail.

# --- did-real-work floors (a green number must clear ALL of these or it isn't measuring the real render) ---
const MIN_TERRAIN_FRAC := 0.85   # TERRAIN (differs from sky) fills the lower frame (not an empty/sky scene)
const SKY_DELTA := 0.06          # a pixel is terrain (not sky) if max channel-diff from SKY exceeds this
const MIN_STREAM_EVENTS := 1     # pages were streamed under motion (created+recomputed grew) -> not static
const MIN_PRIMITIVES := 100000   # the clipmap actually drew geometry (45 tiles x ~64^2 -> millions; 100k floor)
const MIN_GPU_MS := 0.001        # measured GPU time is non-zero (the timer is actually working, not Metal-0)

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10TerrainView"):
		push_error("[wg10-biome-fly] Wg10TerrainView not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-biome-fly] status=skip reason=no-render-device"); return 2
	if not RenderingServer.has_method("viewport_get_measured_render_time_gpu"):
		print("[wg10-biome-fly] status=skip reason=no-measured-render-time-api"); return 2

	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)

	# detail ON for the measured render (the shipped path includes M5 detail).
	if not RenderingServer.global_shader_parameter_get_list().has("wg_detail_amp"):
		RenderingServer.global_shader_parameter_add("wg_detail_amp", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, DETAIL_AMP)
	RenderingServer.global_shader_parameter_set("wg_detail_amp", DETAIL_AMP)

	# --- BIOME PATH setup: configure_biome (NOT the legacy kernel atlas configure) ---
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var err: String = str(pool.call("configure_biome",
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE),
		ProjectSettings.globalize_path(MOUNTAIN),
		CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, FEATURE_SPAN_M, FLOW_ITERS, 1000.0, 2, SEED))
	if err != "":
		push_error("[wg10-biome-fly] pool configure_biome failed: %s" % err); return 1
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
	env.background_color = SKY
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
			if (f - WARM_FRAMES) % 24 == 0:
				var img: Image = vp.get_texture().get_image()
				if img != null:
					terrain_min = minf(terrain_min, _terrain_frac(img))

	var stN: Dictionary = pool.call("stats")
	var stream_events := int(stN.get("created", 0)) + int(stN.get("recomputed", 0)) - stream0
	var biome_active: bool = bool(pool.call("uses_biome_path"))

	# --- GPU p99 ---
	gpu_samples.sort()
	var gpu_p99 := 0.0
	var gpu_mean := 0.0
	if gpu_samples.size() > 0:
		gpu_p99 = gpu_samples[int(floor(gpu_samples.size() * 0.99)) if gpu_samples.size() > 1 else 0]
		var s := 0.0
		for v in gpu_samples: s += v
		gpu_mean = s / float(gpu_samples.size())

	var verdict := "under" if gpu_p99 <= GPU_P99_BUDGET_MS else "over"

	# --- DID-REAL-WORK assertions (a green perf number must be measuring the REAL render). Per §B
	# an over-budget p99 is NOT here — over-budget is a valid measurement result, not a fail. The
	# gate fails ONLY on degenerate / no-work / wrong-path. ---
	if not biome_active:
		errs.append("not-real-work: uses_biome_path()=false -> the biome producer is NOT active (silent legacy fallback?)")
	if gpu_max < MIN_GPU_MS:
		errs.append("GPU timer read ~0 (max %.5f ms) -> measurement not working; perf number is meaningless" % gpu_max)
	if terrain_min < MIN_TERRAIN_FRAC:
		errs.append("not-real-work: terrain (non-sky) frac=%.3f < %.2f (empty/black/sky frames?)" % [terrain_min, MIN_TERRAIN_FRAC])
	if stream_events < MIN_STREAM_EVENTS:
		errs.append("not-real-work: %d stream events < %d (nothing streamed under motion -> static scene?)" % [stream_events, MIN_STREAM_EVENTS])
	if prim_max < MIN_PRIMITIVES:
		errs.append("not-real-work: max primitives %d < %d (clipmap didn't draw geometry?)" % [prim_max, MIN_PRIMITIVES])

	pool.call("free_all")

	print("[wg10-biome-fly] GPU p99=%.3fms mean=%.3fms max=%.3fms (budget %.1f) speed=%dm/s frames=%d flow_iters=%d" % [
		gpu_p99, gpu_mean, gpu_max, GPU_P99_BUDGET_MS, int(SPEED), MEASURE_FRAMES, FLOW_ITERS])
	print("[wg10-biome-fly] DID-REAL-WORK: biome_path=%s terrain_frac_min=%.3f stream_events=%d prim_max=%d resident_max=%d cpu_update_mean=%.3fms" % [
		str(biome_active), terrain_min, stream_events, prim_max, resident_max, cpu_update_sum / float(MEASURE_FRAMES)])
	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-biome-fly] status=fail errors=%d" % errs.size())
		return 1
	# Headline result line — pages, biome_path, p99, verdict (under|over budget), status=pass.
	print("[wg10-biome-fly] pages=%d biome_path=%s p99=%.3fms verdict=%s-budget status=pass" % [
		stream_events, str(biome_active), gpu_p99, verdict])
	return 0


# Fraction of LOWER-frame pixels that are TERRAIN (differ from the SKY color by > SKY_DELTA). The lower
# third-to-bottom is where ground lives in a fly POV; a sky-only / black frame scores ~0.
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
