extends SceneTree

# Ladder scaffold smoke + Rung 0 analytic plumbing gate.
# Smoke phase: the scene instantiates, exposes the probe API, and renders a non-black
# REFERENCE frame through the proven render stack.
# Rung 0 phase (added in Task 0.5): switch to the analytic rung, produce+read a live page,
# and assert it matches the closed-form height (parity) + seam-exact across page boundaries.

const SCENE := "res://worldgen_terrain/harness/wg10_unintercept_ladder.tscn"
const HELPER := "res://worldgen_terrain/harness/ladder_convergence.gd"
const VIEW_SIZE := Vector2i(640, 360)

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

	# Smoke: render a REFERENCE frame and assert non-black.
	await process_frame
	RenderingServer.force_draw()
	await process_frame
	var img := vp.get_texture().get_image()
	var nonblack := 0
	var total := 0
	for y in range(0, img.get_size().y, 8):
		for x in range(0, img.get_size().x, 8):
			total += 1
			if img.get_pixel(x, y).v > 0.04:
				nonblack += 1
	var frac := float(nonblack) / float(max(total, 1))
	scene.queue_free()
	vp.queue_free()
	await process_frame

	if frac < 0.5:
		print("[ladder-rung0] status=fail nonblack_frac=%.3f rung=reference" % frac)
		return 1
	print("[ladder-rung0] status=pass nonblack_frac=%.3f rung=reference (smoke; analytic parity added in Task 0.5)" % frac)
	return 0
