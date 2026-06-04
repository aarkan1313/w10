extends SceneTree

const SCENE := "res://worldgen_terrain/harness/mountain_network_chunks_review.tscn"
const EXPECTED_CHUNKS := 81
const EXPECTED_SEAM_GUIDES := 144
const EXPECTED_STYLE_WORLDS := 4
const EXPECTED_VARIANTS := 3
const EXPECTED_BASE_HEIGHT_SCALE := 1700.0
const EXPECTED_FEATURE_SPAN_M := 90000.0
const EXPECTED_GENERATOR_VERSION := "mountain_synthesis_v0_9x9_original_scene_scale_review_pass_network"

func _init() -> void:
	quit(await _run())

func _run() -> int:
	var packed := load(SCENE)
	if packed == null:
		push_error("cannot load %s" % SCENE)
		return 1
	var scene: Node = packed.instantiate()
	get_root().add_child(scene)
	await process_frame
	await process_frame

	var errs: Array[String] = []
	var chunks := scene.get_node_or_null("Chunks")
	var guides := scene.get_node_or_null("SeamGuides")
	var camera := scene.get_node_or_null("ReviewCamera") as Camera3D
	var payload: Dictionary = scene.get("_payload")
	var style_worlds: Array = scene.get("_worlds")
	var base_height_scale := float(scene.get("base_height_scale"))
	var collision_body_count := int(scene.get("_collision_body_count"))

	if payload.is_empty():
		errs.append("payload did not load")
	else:
		if str(payload.get("generator_version", "")) != EXPECTED_GENERATOR_VERSION:
			errs.append("unexpected generator_version=%s" % str(payload.get("generator_version", "")))
		if int(payload.get("chunk_count", 0)) != 9:
			errs.append("chunk_count=%d, expected 9" % int(payload.get("chunk_count", 0)))
		if absf(float(payload.get("feature_span_m", 0.0)) - EXPECTED_FEATURE_SPAN_M) > 0.001:
			errs.append("feature_span_m=%s, expected %.1f" % [str(payload.get("feature_span_m", 0.0)), EXPECTED_FEATURE_SPAN_M])
		var variants: Array = payload.get("review_variants", [])
		if variants.size() != EXPECTED_VARIANTS:
			errs.append("review_variants=%d, expected %d" % [variants.size(), EXPECTED_VARIANTS])
	if absf(base_height_scale - EXPECTED_BASE_HEIGHT_SCALE) > 0.001:
		errs.append("base_height_scale=%.1f, expected %.1f" % [base_height_scale, EXPECTED_BASE_HEIGHT_SCALE])
	if style_worlds.size() != EXPECTED_STYLE_WORLDS:
		errs.append("style worlds=%d, expected %d" % [style_worlds.size(), EXPECTED_STYLE_WORLDS])
	if chunks == null:
		errs.append("Chunks node missing")
	elif chunks.get_child_count() != EXPECTED_CHUNKS:
		errs.append("chunk meshes=%d, expected %d" % [chunks.get_child_count(), EXPECTED_CHUNKS])
	if collision_body_count != EXPECTED_CHUNKS:
		errs.append("collision bodies=%d, expected %d" % [collision_body_count, EXPECTED_CHUNKS])
	if guides == null:
		errs.append("SeamGuides node missing")
	elif guides.visible:
		errs.append("seam guides should be default-off")
	if camera == null:
		errs.append("ReviewCamera missing")
	elif not camera.current:
		errs.append("ReviewCamera is not current")

	if errs.is_empty():
		scene.call("_focus_next_seam")
		await process_frame
		guides = scene.get_node_or_null("SeamGuides")
		var seam_targets: Array = scene.get("_seam_targets")
		if guides == null or not guides.visible:
			errs.append("next-seam focus did not enable seam guides")
		elif guides.get_child_count() != EXPECTED_SEAM_GUIDES:
			errs.append("lazy seam guides=%d, expected %d" % [guides.get_child_count(), EXPECTED_SEAM_GUIDES])
		if seam_targets.size() != EXPECTED_SEAM_GUIDES:
			errs.append("lazy seam targets=%d, expected %d" % [seam_targets.size(), EXPECTED_SEAM_GUIDES])

	scene.queue_free()
	await process_frame

	if not errs.is_empty():
		for err in errs:
			push_error(err)
		print("[wg10-mountain-network-chunks-review] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-mountain-network-chunks-review] status=pass chunks=%d seam_guides=%d style_worlds=%d variants=%d" % [
		EXPECTED_CHUNKS,
		EXPECTED_SEAM_GUIDES,
		EXPECTED_STYLE_WORLDS,
		EXPECTED_VARIANTS,
	])
	return 0
