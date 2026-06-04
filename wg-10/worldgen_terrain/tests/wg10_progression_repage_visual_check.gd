extends SceneTree

# Pixel-level repage-delta guard for the WG10 progression scene.
# The motion gate proves page-boundary transitions do not hide tiles or burst too many visible
# rebinds. This gate holds the render camera fixed while the logical clipmap centre crosses known
# page boundaries, then compares pre/post images. That isolates renderer re-page/LOD changes from
# normal camera motion.

const SCENE := "res://worldgen_terrain/harness/wg10_progression_review.tscn"

const VIEW_SIZE := Vector2i(640, 360)
const OUT_DIR := "D:/tmp/wg10_biome_compose"
const SPEED := 8000.0
const SETTLE_FRAMES := 160
const BOUNDARY_EPS_M := 32.0
const SAMPLE_STRIDE := 4
const SKY_DELTA := 0.06
const MAX_MEAN_RGB_DELTA := 0.018
const MAX_P95_RGB_DELTA := 0.095
const MAX_P99_RGB_DELTA := 0.160
const MIN_TERRAIN_SAMPLES := 2500
const MAX_REPAGE_AT_BOUNDARY := 9

const BOUNDARIES := [
	{"label": "l0", "x": 8192.0},
	{"label": "l1", "x": 16384.0},
	{"label": "l2", "x": 32768.0},
]

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-progression-repage-visual] status=skip reason=no-render-device")
		return 2

	DirAccess.make_dir_recursive_absolute(OUT_DIR)
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)

	var packed := load(SCENE)
	if packed == null:
		push_error("[wg10-progression-repage-visual] cannot load %s" % SCENE)
		return 1

	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	get_root().add_child(vp)

	var scene: Node = packed.instantiate()
	vp.add_child(scene)
	for _i in range(90):
		await process_frame

	var errs: Array[String] = []
	_expect(scene.has_method("set_probe_mode"), "scene missing set_probe_mode", errs)
	_expect(scene.has_method("set_probe_camera_frame"), "scene missing set_probe_camera_frame", errs)
	_expect(scene.has_method("update_for_probe"), "scene missing update_for_probe", errs)
	_expect(scene.has_method("debug_tile_states"), "scene missing debug_tile_states", errs)
	if not errs.is_empty():
		_teardown(scene, vp)
		for err in errs:
			push_error(err)
		print("[wg10-progression-repage-visual] status=fail errors=%d" % errs.size())
		return 1

	scene.call("set_probe_mode", true)
	var sky := Color(0.68, 0.76, 0.84)
	var steps := int(scene.call("step_count")) if scene.has_method("step_count") else 0
	var worst_mean := 0.0
	var worst_p95 := 0.0
	var worst_p99 := 0.0
	var checked := 0

	for step_index in range(steps):
		if not bool(scene.call("set_step_index", step_index)):
			errs.append("set_step_index(%d) failed" % step_index)
			continue
		scene.call("set_probe_mode", true)
		var snapshot: Dictionary = scene.call("debug_progression_snapshot")
		var step_id := str(snapshot.get("step_id", step_index))
		for boundary in BOUNDARIES:
			var result: Dictionary = await _check_boundary(vp, scene, step_id, boundary, sky)
			checked += 1
			worst_mean = maxf(worst_mean, float(result.get("mean", 0.0)))
			worst_p95 = maxf(worst_p95, float(result.get("p95", 0.0)))
			worst_p99 = maxf(worst_p99, float(result.get("p99", 0.0)))
			if int(result.get("rc", 1)) != 0:
				errs.append("%s/%s: %s" % [
					step_id,
					str(boundary.get("label", "")),
					str(result.get("error", "failed")),
				])

	_teardown(scene, vp)
	await process_frame
	await process_frame

	if not errs.is_empty():
		for err in errs:
			push_error(err)
		print("[wg10-progression-repage-visual] status=fail errors=%d checked=%d worst_mean=%.6f worst_p95=%.6f worst_p99=%.6f" % [
			errs.size(),
			checked,
			worst_mean,
			worst_p95,
			worst_p99,
		])
		return 1

	print("[wg10-progression-repage-visual] status=pass checks=%d worst_mean=%.6f worst_p95=%.6f worst_p99=%.6f size=%dx%d" % [
		checked,
		worst_mean,
		worst_p95,
		worst_p99,
		VIEW_SIZE.x,
		VIEW_SIZE.y,
	])
	return 0

func _check_boundary(vp: SubViewport, scene: Node, step_id: String, boundary: Dictionary, sky: Color) -> Dictionary:
	var boundary_label := str(boundary.get("label", "boundary"))
	var boundary_x := float(boundary.get("x", 0.0))
	var pre_pos := Vector2(boundary_x - BOUNDARY_EPS_M, 0.0)
	var post_pos := Vector2(boundary_x + BOUNDARY_EPS_M, 0.0)
	var vel := Vector2(SPEED, 0.0)

	for _i in range(SETTLE_FRAMES):
		scene.call("update_for_probe", pre_pos.x, pre_pos.y, vel.x, vel.y)

	var eye_look := _camera_frame(step_id, pre_pos)
	scene.call("set_probe_camera_frame", eye_look["eye"], eye_look["look"])
	var pre_img := await _capture_image(vp)
	if pre_img == null:
		return {"rc": 1, "error": "null pre image", "mean": 1.0, "p95": 1.0, "p99": 1.0}

	var prev_states: PackedInt64Array = scene.call("debug_tile_states")
	scene.call("update_for_probe", post_pos.x, post_pos.y, vel.x, vel.y)
	var states: PackedInt64Array = scene.call("debug_tile_states")
	var repage_count := _repage_count(prev_states, states)
	scene.call("set_probe_camera_frame", eye_look["eye"], eye_look["look"])
	var post_img := await _capture_image(vp)
	if post_img == null:
		return {"rc": 1, "error": "null post image", "mean": 1.0, "p95": 1.0, "p99": 1.0}

	var diff := _terrain_delta(pre_img, post_img, sky)
	var mean := float(diff.get("mean", 0.0))
	var p95 := float(diff.get("p95", 0.0))
	var p99 := float(diff.get("p99", 0.0))
	var samples := int(diff.get("samples", 0))
	var out_base := "%s/progression_repage_%s_%s" % [OUT_DIR, step_id, boundary_label]
	var pre_path := "%s_pre.png" % out_base
	var post_path := "%s_post.png" % out_base
	if mean > MAX_MEAN_RGB_DELTA or p95 > MAX_P95_RGB_DELTA or p99 > MAX_P99_RGB_DELTA or repage_count > MAX_REPAGE_AT_BOUNDARY:
		pre_img.save_png(pre_path)
		post_img.save_png(post_path)

	print("[wg10-progression-repage-visual] step=%s boundary=%s repage=%d mean=%.6f p95=%.6f p99=%.6f samples=%d" % [
		step_id,
		boundary_label,
		repage_count,
		mean,
		p95,
		p99,
		samples,
	])

	var errs: Array[String] = []
	if samples < MIN_TERRAIN_SAMPLES:
		errs.append("terrain_samples %d < %d" % [samples, MIN_TERRAIN_SAMPLES])
	if repage_count <= 0:
		errs.append("boundary did not cause a visible repage")
	if repage_count > MAX_REPAGE_AT_BOUNDARY:
		errs.append("repage_count %d > %d" % [repage_count, MAX_REPAGE_AT_BOUNDARY])
	if mean > MAX_MEAN_RGB_DELTA:
		errs.append("mean %.6f > %.6f" % [mean, MAX_MEAN_RGB_DELTA])
	if p95 > MAX_P95_RGB_DELTA:
		errs.append("p95 %.6f > %.6f" % [p95, MAX_P95_RGB_DELTA])
	if p99 > MAX_P99_RGB_DELTA:
		errs.append("p99 %.6f > %.6f" % [p99, MAX_P99_RGB_DELTA])
	if not errs.is_empty():
		var evidence := " evidence=%s,%s" % [pre_path, post_path]
		return {"rc": 1, "error": "; ".join(errs) + evidence, "mean": mean, "p95": p95, "p99": p99}
	return {"rc": 0, "error": "", "mean": mean, "p95": p95, "p99": p99}

func _capture_image(vp: SubViewport) -> Image:
	await process_frame
	RenderingServer.force_draw()
	await process_frame
	return vp.get_texture().get_image()

func _camera_frame(step_id: String, pos: Vector2) -> Dictionary:
	if step_id == "mountain_close_debug_candidate":
		return {
			"eye": Vector3(pos.x - 900.0, 720.0, pos.y - 900.0),
			"look": Vector3(pos.x + 1800.0, 60.0, pos.y + 1800.0),
		}
	return {
		"eye": Vector3(pos.x - 9000.0, 5200.0, pos.y - 9000.0),
		"look": Vector3(pos.x + 22000.0, 250.0, pos.y + 22000.0),
	}

func _terrain_delta(a: Image, b: Image, sky: Color) -> Dictionary:
	var size := a.get_size()
	var deltas: Array[float] = []
	var total := 0.0
	for y in range(0, size.y, SAMPLE_STRIDE):
		for x in range(0, size.x, SAMPLE_STRIDE):
			var ca := a.get_pixel(x, y)
			var cb := b.get_pixel(x, y)
			if not _is_terrain(ca, sky) and not _is_terrain(cb, sky):
				continue
			var d := (absf(ca.r - cb.r) + absf(ca.g - cb.g) + absf(ca.b - cb.b)) / 3.0
			deltas.append(d)
			total += d
	if deltas.is_empty():
		return {"samples": 0, "mean": 1.0, "p95": 1.0, "p99": 1.0}
	deltas.sort()
	return {
		"samples": deltas.size(),
		"mean": total / float(deltas.size()),
		"p95": deltas[clampi(int(floor(float(deltas.size() - 1) * 0.95)), 0, deltas.size() - 1)],
		"p99": deltas[clampi(int(floor(float(deltas.size() - 1) * 0.99)), 0, deltas.size() - 1)],
	}

func _is_terrain(color: Color, sky: Color) -> bool:
	var d := maxf(maxf(absf(color.r - sky.r), absf(color.g - sky.g)), absf(color.b - sky.b))
	return d > SKY_DELTA

func _repage_count(prev: PackedInt64Array, states: PackedInt64Array) -> int:
	var count := 0
	var t := 0
	while t * 3 + 2 < states.size() and t * 3 + 2 < prev.size():
		var vis := int(states[t * 3])
		var ox := int(states[t * 3 + 1])
		var oz := int(states[t * 3 + 2])
		var pv := int(prev[t * 3])
		var pox := int(prev[t * 3 + 1])
		var poz := int(prev[t * 3 + 2])
		if vis == 1 and pv == 1 and (ox != pox or oz != poz):
			count += 1
		t += 1
	return count

func _teardown(scene: Node, vp: SubViewport) -> void:
	if scene != null:
		scene.queue_free()
	if vp != null:
		vp.queue_free()

func _expect(condition: bool, message: String, errs: Array[String]) -> void:
	if not condition:
		errs.append(message)
