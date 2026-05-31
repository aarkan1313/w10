extends SceneTree

const SCENE := "res://worldgen_terrain/harness/rough_world_infinite_review.tscn"
const EXPECTED_VISIBLE_CHUNKS := 900
const EXPECTED_SEAM_GUIDES := 1740
const EXPECTED_SEEDS := 2
const EXPECTED_VARIANTS := 4

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
	var payload: Dictionary = scene.get("_payload")
	var seeds: Array = scene.get("_seed_worlds")
	var variants: Array = scene.get("_review_variants")
	var seam_targets: Array = scene.get("_seam_targets")

	if payload.is_empty():
		errs.append("payload did not load")
	else:
		if str(payload.get("generator_version", "")) != "rough_world_chunks_v2_independent_windows":
			errs.append("unexpected generator_version=%s" % str(payload.get("generator_version", "")))
		if int(payload.get("chunk_count", 0)) != 30:
			errs.append("chunk_count=%d, expected 30" % int(payload.get("chunk_count", 0)))
		if int(payload.get("active_window_count", 0)) != 30:
			errs.append("active_window_count=%d, expected 30" % int(payload.get("active_window_count", 0)))
		if int(payload.get("chunk_n", 0)) != 41:
			errs.append("chunk_n=%d, expected 41" % int(payload.get("chunk_n", 0)))
	if seeds.size() != EXPECTED_SEEDS:
		errs.append("seed worlds=%d, expected %d" % [seeds.size(), EXPECTED_SEEDS])
	if variants.size() != EXPECTED_VARIANTS:
		errs.append("scene variants=%d, expected %d" % [variants.size(), EXPECTED_VARIANTS])
	if int(scene.get("_window_x")) != 0 or int(scene.get("_window_z")) != 0:
		errs.append("initial window=%d,%d expected 0,0" % [int(scene.get("_window_x")), int(scene.get("_window_z"))])
	if chunks == null:
		errs.append("Chunks node missing")
	elif chunks.get_child_count() != EXPECTED_VISIBLE_CHUNKS:
		errs.append("visible chunk meshes=%d, expected %d" % [chunks.get_child_count(), EXPECTED_VISIBLE_CHUNKS])
	if guides == null:
		errs.append("SeamGuides node missing")
	else:
		if guides.visible:
			errs.append("seam guides should be default-off")
		if guides.get_child_count() != 0:
			errs.append("default seam guides=%d, expected lazy 0" % guides.get_child_count())
	if seam_targets.size() != 0:
		errs.append("default seam targets=%d, expected lazy 0" % seam_targets.size())

	if errs.is_empty():
		scene.call("_focus_next_seam")
		await process_frame
		chunks = scene.get_node_or_null("Chunks")
		guides = scene.get_node_or_null("SeamGuides")
		seam_targets = scene.get("_seam_targets")
		if chunks == null or chunks.get_child_count() != EXPECTED_VISIBLE_CHUNKS:
			errs.append("moved visible chunk count invalid")
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
		print("[wg10-rough-infinite-review] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-rough-infinite-review] status=pass visible_chunks=%d seam_guides=%d seeds=%d" % [
		EXPECTED_VISIBLE_CHUNKS,
		EXPECTED_SEAM_GUIDES,
		EXPECTED_SEEDS,
	])
	return 0
