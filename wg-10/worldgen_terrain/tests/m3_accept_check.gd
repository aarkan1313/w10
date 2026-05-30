extends SceneTree

# M3 acceptance gate (§7.3 — the REGRESSION CATCHER; the owner's manual fly of m3_review.tscn
# is the final authority). Drives Wg10TerrainView.update over a scripted ~1000 m/s flight path
# (straight runs + turns across many page boundaries) in a SubViewport with a flight-POV camera,
# captures total per-frame work time, and asserts p99 < 6 ms + no-black + never-stall over the
# measured run. vsync disabled so frame time is real. WINDOWED. Prints p99/mean/max; saves a PNG.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const PROFILER := "res://worldgen_terrain/harness/profiler.gd"
const PAGE_PX := 256
const SEED := 1337
const NUM_LEVELS := 3
const BASE_SPAN := 8192.0
const GRID_RES := 64
const RADIUS_PAGES := 1
const LEAD_FRAMES := 8.0
const MAX_PER_FRAME := 4
const CAPACITY := 48
const MORPH_REGION := 0.15
const HEIGHT_SCALE := 0.35
const RELIEF_REF := 2000.0
const VIEW_SIZE := Vector2i(960, 540)

const SPEED := 1000.0          # ~1000 m/s acceptance speed
const WARM_FRAMES := 60        # let streaming + frame times settle (excluded from p99)
const MEASURE_FRAMES := 240    # measured window
const P99_BUDGET_MS := 6.0
const STALL_CEIL_MS := 33.0    # no single frame worse than this (a visible hitch)
const MIN_NONBLACK := 0.85     # flight POV has sky; terrain must dominate the lower frame

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10TerrainView"):
		push_error("Wg10TerrainView not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-m3-accept] status=skip reason=no-render-device"); return 2

	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)

	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var err: String = str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if err != "":
		push_error("pool configure failed: %s" % err); return 1
	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_FRAMES, MAX_PER_FRAME)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF)

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

	var profiler: Node = load(PROFILER).new()
	vp.add_child(profiler)

	# Scripted flight path: straight + turning legs at SPEED across many page boundaries; heading
	# changes each ~60-frame leg so the streamer's velocity-lead is exercised in multiple dirs.
	var headings := [Vector2(1,0), Vector2(0.7,0.7), Vector2(0,1), Vector2(-0.7,0.7), Vector2(1,0)]
	var pos := Vector2(0.0, 0.0)
	var errs: Array[String] = []
	var dt := 1.0 / 60.0   # fixed step for a deterministic path (real frame time measured separately)

	var total := WARM_FRAMES + MEASURE_FRAMES
	# DIAGNOSTIC accumulators: split frames into those where the streamer computed pages
	# (created+recomputed grew) vs render-only frames, to see what dominates the p99 tail.
	var update_ms_sum := 0.0
	var compute_frames := 0
	var compute_ms_max := 0.0
	var renderonly_ms_max := 0.0
	for f in range(total):
		var heading: Vector2 = headings[int(f / 60) % headings.size()]
		var vx := heading.x * SPEED
		var vz := heading.y * SPEED
		pos += Vector2(vx, vz) * dt
		var st_before: Dictionary = pool.call("stats")
		var cc_before := int(st_before.get("created", 0)) + int(st_before.get("recomputed", 0))
		var tu0 := Time.get_ticks_usec()
		view.call("update", pos.x, pos.y, vx, vz)
		var update_ms := float(Time.get_ticks_usec() - tu0) / 1000.0
		var eye := Vector3(pos.x - heading.x * 600.0, 900.0, pos.y - heading.y * 600.0)
		var look := Vector3(pos.x + heading.x * 1200.0, 0.0, pos.y + heading.y * 1200.0)
		cam.look_at_from_position(eye, look, Vector3.UP)
		await process_frame
		RenderingServer.force_draw()
		await process_frame
		var ft_ms := float(Time.get_ticks_usec() - tu0) / 1000.0   # ms wall time for this frame's work
		var st_after: Dictionary = pool.call("stats")
		var cc_after := int(st_after.get("created", 0)) + int(st_after.get("recomputed", 0))
		var did_compute := cc_after > cc_before
		if f >= WARM_FRAMES:
			update_ms_sum += update_ms
			if did_compute:
				compute_frames += 1
				compute_ms_max = max(compute_ms_max, ft_ms)
			else:
				renderonly_ms_max = max(renderonly_ms_max, ft_ms)
			profiler.call("push", ft_ms / 1000.0)   # push expects seconds
			if (f - WARM_FRAMES) % 12 == 0:
				var img: Image = vp.get_texture().get_image()
				if img != null:
					if f == WARM_FRAMES:
						img.save_png("user://m3_accept.png")
					# no-black: sample the LOWER ~2/3 of the frame (terrain region; the upper third
					# is mostly sky in a flight POV, which we don't penalize).
					var nb := 0
					var samp := 0
					var y0 := int(img.get_height() / 3)
					for y in range(y0, img.get_height(), 4):
						for x in range(0, img.get_width(), 4):
							samp += 1
							var c := img.get_pixel(x, y)
							if c.r > 0.03 or c.g > 0.03 or c.b > 0.03:
								nb += 1
					var frac := float(nb) / float(max(samp, 1))
					if frac < MIN_NONBLACK:
						errs.append("frame %d: black/holes in terrain region nonblack=%.3f < %.2f" % [f, frac, MIN_NONBLACK])

	var p99 := float(profiler.call("p99_ms"))
	var mean := float(profiler.call("mean_ms"))
	var mx := float(profiler.call("max_ms"))
	if p99 > P99_BUDGET_MS:
		errs.append("p99 %.2f ms > %.1f ms budget (at ~%d m/s)" % [p99, P99_BUDGET_MS, int(SPEED)])
	if mx > STALL_CEIL_MS:
		errs.append("stall: max frame %.2f ms > %.1f ms ceiling" % [mx, STALL_CEIL_MS])

	pool.call("free_all")

	print("[wg10-m3-accept] p99=%.2fms mean=%.2fms max=%.2fms speed=%dm/s frames=%d" % [p99, mean, mx, int(SPEED), MEASURE_FRAMES])
	print("[wg10-m3-accept] DIAG: compute_frames=%d/%d compute_ms_max=%.2f renderonly_ms_max=%.2f update_ms_mean=%.3f" % [
		compute_frames, MEASURE_FRAMES, compute_ms_max, renderonly_ms_max, update_ms_sum / float(MEASURE_FRAMES)])
	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m3-accept] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-m3-accept] status=pass p99=%.2fms (budget %.1fms)" % [p99, P99_BUDGET_MS])
	return 0
