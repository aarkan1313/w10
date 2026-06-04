extends SceneTree

# One-shot CAPTURE for the accepted static mountain-network baseline.
# This is not a runtime producer proof; it records the owner-liked offline artifact
# (`mountain_network_chunks_review.tscn`) so live runtime captures can be compared
# against the same evidence folder.

const SCENE := "res://worldgen_terrain/harness/mountain_network_chunks_review.tscn"
const VIEW_SIZE := Vector2i(1280, 720)
const OUT_FOCUS := "D:/tmp/wg10_biome_compose/mountain_network_static_focus_capture.png"
const OUT_OVERVIEW := "D:/tmp/wg10_biome_compose/mountain_network_static_overview_capture.png"

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-mountain-network-capture] status=skip reason=no-render-device"); return 2

	var packed := load(SCENE)
	if packed == null:
		push_error("[wg10-mountain-network-capture] cannot load %s" % SCENE); return 1

	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	get_root().add_child(vp)

	var scene: Node = packed.instantiate()
	vp.add_child(scene)
	await process_frame
	await process_frame
	_hide_canvas_layers(scene)

	DirAccess.make_dir_recursive_absolute("D:/tmp/wg10_biome_compose")
	var focus_rc := await _capture(scene, vp, "_focus_camera", OUT_FOCUS)
	var overview_rc := await _capture(scene, vp, "_overview_camera", OUT_OVERVIEW)
	var payload: Dictionary = scene.get("_payload")
	scene.queue_free()
	vp.queue_free()
	await process_frame

	if focus_rc != OK:
		push_error("[wg10-mountain-network-capture] focus save failed rc=%d" % focus_rc); return 1
	if overview_rc != OK:
		push_error("[wg10-mountain-network-capture] overview save failed rc=%d" % overview_rc); return 1

	print("[wg10-mountain-network-capture] status=pass wrote %s and %s chunks=%d feature_span_m=%.0f size=%dx%d" % [
		OUT_FOCUS,
		OUT_OVERVIEW,
		int(payload.get("chunk_count", 0)),
		float(payload.get("feature_span_m", 0.0)),
		VIEW_SIZE.x,
		VIEW_SIZE.y,
	])
	return 0

func _capture(scene: Node, vp: SubViewport, camera_method: String, out_path: String) -> int:
	scene.call(camera_method)
	await process_frame
	RenderingServer.force_draw()
	await process_frame
	var img: Image = vp.get_texture().get_image()
	if img == null:
		return ERR_DOES_NOT_EXIST
	return img.save_png(out_path)

func _hide_canvas_layers(node: Node) -> void:
	for child in node.get_children():
		if child is CanvasLayer:
			child.visible = false
		_hide_canvas_layers(child)
