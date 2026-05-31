extends SceneTree

const SCENE := "res://worldgen_terrain/harness/rough_world_travel_review.tscn"
const EXPECTED_CHUNKS := 25
const EXPECTED_SEAM_GUIDES := 40
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
		if int(payload.get("chunk_count", 0)) != 5:
			errs.append("chunk_count=%d, expected 5" % int(payload.get("chunk_count", 0)))
		if int(payload.get("chunk_n", 0)) != 65:
			errs.append("chunk_n=%d, expected 65" % int(payload.get("chunk_n", 0)))
		if not payload.has("review_variants"):
			errs.append("payload missing review_variants")
		elif (payload["review_variants"] as Array).size() != EXPECTED_VARIANTS:
			errs.append("payload variants=%d, expected %d" % [(payload["review_variants"] as Array).size(), EXPECTED_VARIANTS])
	if seeds.size() != EXPECTED_SEEDS:
		errs.append("seed worlds=%d, expected %d" % [seeds.size(), EXPECTED_SEEDS])
	if variants.size() != EXPECTED_VARIANTS:
		errs.append("scene variants=%d, expected %d" % [variants.size(), EXPECTED_VARIANTS])
	if absf(float(scene.get("_relief")) - 1.0) > 0.001:
		errs.append("initial relief=%s, expected 1.0" % str(scene.get("_relief")))
	if str(scene.get("_dressing_style")) != "plain":
		errs.append("initial dressing=%s, expected plain" % str(scene.get("_dressing_style")))
	if chunks == null:
		errs.append("Chunks node missing")
	elif chunks.get_child_count() != EXPECTED_CHUNKS:
		errs.append("chunk meshes=%d, expected %d" % [chunks.get_child_count(), EXPECTED_CHUNKS])
	if guides == null:
		errs.append("SeamGuides node missing")
	else:
		if guides.visible:
			errs.append("seam guides should be default-off")
		if guides.get_child_count() != EXPECTED_SEAM_GUIDES:
			errs.append("seam guides=%d, expected %d" % [guides.get_child_count(), EXPECTED_SEAM_GUIDES])
	if seam_targets.size() != EXPECTED_SEAM_GUIDES:
		errs.append("seam targets=%d, expected %d" % [seam_targets.size(), EXPECTED_SEAM_GUIDES])

	if errs.is_empty():
		scene.call("_cycle_variant")
		await process_frame
		chunks = scene.get_node_or_null("Chunks")
		if int(scene.get("_variant_index")) != 1:
			errs.append("variant index=%d, expected 1" % int(scene.get("_variant_index")))
		if absf(float(scene.get("_relief")) - 1.25) > 0.001:
			errs.append("variant relief=%s, expected 1.25" % str(scene.get("_relief")))
		if str(scene.get("_dressing_style")) != "review_biome":
			errs.append("variant dressing=%s, expected review_biome" % str(scene.get("_dressing_style")))
		if chunks == null or chunks.get_child_count() != EXPECTED_CHUNKS:
			errs.append("variant rebuild chunk count invalid")

	scene.queue_free()
	await process_frame

	if not errs.is_empty():
		for err in errs:
			push_error(err)
		print("[wg10-rough-travel-review] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-rough-travel-review] status=pass chunks=%d seam_guides=%d seeds=%d" % [
		EXPECTED_CHUNKS,
		EXPECTED_SEAM_GUIDES,
		EXPECTED_SEEDS,
	])
	return 0
