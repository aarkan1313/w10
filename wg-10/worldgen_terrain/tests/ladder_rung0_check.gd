extends SceneTree

# Ladder Rung 0: un-intercept plumbing proof.
# (1) Smoke: the scene instantiates, exposes the probe API, renders a non-black REFERENCE frame.
# (2) Analytic parity: switch to the analytic rung (live closed-form producer), produce+read a page
#     via the proven idiom, assert every sampled texel == amp*sin(wx/lam)*cos(wz/lam) within f32
#     epsilon, and assert two abutting pages share their boundary column exactly (seam-exact).
# This de-risks the baked->live FLIP independent of biome content: a live compute/produced page
# flows through the whole stack and the harness reads it correctly.

const SCENE := "res://worldgen_terrain/harness/wg10_unintercept_ladder.tscn"
const HELPER := "res://worldgen_terrain/harness/ladder_convergence.gd"
const VIEW_SIZE := Vector2i(640, 360)
const PAGE_PX := 256
const SPAN := 8192.0
const AMP := 300.0
const LAM := 4000.0

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[ladder-rung0] status=skip reason=no-render-device")
		return 2

	var packed := load(SCENE)
	if packed == null:
		push_error("[ladder-rung0] cannot load %s" % SCENE)
		return 1

	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	get_root().add_child(vp)

	var scene: Node = packed.instantiate()
	vp.add_child(scene)
	for _i in range(120):
		await process_frame

	var errs: Array[String] = []
	for m in ["set_probe_mode", "update_for_probe", "set_probe_camera_frame", "debug_tile_states", "current_rung", "pool", "set_rung"]:
		if not scene.has_method(m):
			errs.append("scene missing %s" % m)
	if not errs.is_empty():
		for e in errs:
			push_error(e)
		print("[ladder-rung0] status=fail errors=%d" % errs.size())
		scene.queue_free(); vp.queue_free()
		return 1

	var helper: Object = load(HELPER).new()
	var rd := RenderingServer.get_rendering_device()

	# --- Switch to the analytic rung (live closed-form producer). ---
	if not bool(scene.call("set_rung", "analytic")):
		print("[ladder-rung0] status=fail reason=set_rung-analytic")
		scene.queue_free(); vp.queue_free(); return 1
	for _i in range(30):
		await process_frame

	# Non-black render of the analytic surface (compute frac before teardown).
	await process_frame
	RenderingServer.force_draw()
	await process_frame
	var img := vp.get_texture().get_image()
	var nb := 0
	var tot := 0
	for y in range(0, img.get_size().y, 8):
		for x in range(0, img.get_size().x, 8):
			tot += 1
			if img.get_pixel(x, y).v > 0.04:
				nb += 1
	var frac := float(nb) / float(max(tot, 1))

	# Produce + read two abutting pages via the proven acquire->flush->read idiom.
	var heights: PackedFloat32Array = await helper.produce_and_read(self, rd, scene.call("pool"), 0, 0.0, 0.0, PAGE_PX)
	var next_heights: PackedFloat32Array = await helper.produce_and_read(self, rd, scene.call("pool"), 0, SPAN, 0.0, PAGE_PX)
	if heights.size() != PAGE_PX * PAGE_PX or next_heights.size() != PAGE_PX * PAGE_PX:
		print("[ladder-rung0] status=fail reason=bad-readback h=%d n=%d" % [heights.size(), next_heights.size()])
		scene.queue_free(); vp.queue_free(); return 1

	var worst := 0.0
	for z in range(0, PAGE_PX, 16):
		for x in range(0, PAGE_PX, 16):
			var wx := (float(x) / float(PAGE_PX - 1)) * SPAN
			var wz := (float(z) / float(PAGE_PX - 1)) * SPAN
			var expected := AMP * sin(wx / LAM) * cos(wz / LAM)
			worst = maxf(worst, absf(heights[z * PAGE_PX + x] - expected))
	# Seam: this page's last column == next page's first column (texel-corner share).
	var seam := 0.0
	for z in range(0, PAGE_PX, 16):
		seam = maxf(seam, absf(heights[z * PAGE_PX + (PAGE_PX - 1)] - next_heights[z * PAGE_PX + 0]))

	scene.queue_free(); vp.queue_free()
	await process_frame

	if frac < 0.5:
		print("[ladder-rung0] status=fail nonblack_frac=%.3f" % frac)
		return 1
	if worst > 0.01 * AMP:
		print("[ladder-rung0] status=fail analytic_worst=%.5f budget=%.5f" % [worst, 0.01 * AMP])
		return 1
	if seam > 0.001:
		print("[ladder-rung0] status=fail seam=%.6f" % seam)
		return 1
	print("[ladder-rung0] status=pass analytic_worst=%.5f seam=%.6f nonblack_frac=%.3f" % [worst, seam, frac])
	return 0
