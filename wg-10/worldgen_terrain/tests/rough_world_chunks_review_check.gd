extends SceneTree

# Smoke-check the offline rough-world chunk review scene. This catches runtime
# failures that a bare import pass misses: JSON load, 3x3 mesh construction,
# seed payload, default-off seam guides, and next-seam camera focus.

const SCENE := "res://worldgen_terrain/harness/rough_world_chunks_review.tscn"
const EXPECTED_CHUNKS := 9
const EXPECTED_SEAM_GUIDES := 12
const EXPECTED_SEEDS := 2
const EXPECTED_FALLBACK_VARIANTS := 4

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
	var camera := scene.get_node_or_null("ReviewCamera")
	var payload: Dictionary = scene.get("_payload")
	var seeds: Array = scene.get("_seed_worlds")
	var variants: Array = scene.get("_review_variants")
	var seam_targets: Array = scene.get("_seam_targets")

	if payload.is_empty():
		errs.append("payload did not load")
	else:
		if str(payload.get("generator_version", "")) != "rough_world_chunks_v2_independent_windows":
			errs.append("unexpected generator_version=%s" % str(payload.get("generator_version", "")))
		if int(payload.get("chunk_count", 0)) != 3:
			errs.append("chunk_count=%d, expected 3" % int(payload.get("chunk_count", 0)))
		if float(payload.get("chunk_span_m", 0.0)) != 25600.0:
			errs.append("chunk_span_m=%s, expected 25600" % str(payload.get("chunk_span_m", 0.0)))
	if seeds.size() != EXPECTED_SEEDS:
		errs.append("seed worlds=%d, expected %d" % [seeds.size(), EXPECTED_SEEDS])
	if variants.size() != EXPECTED_FALLBACK_VARIANTS:
		errs.append("fallback variants=%d, expected %d" % [variants.size(), EXPECTED_FALLBACK_VARIANTS])
	if chunks == null:
		errs.append("Chunks node missing")
	elif chunks.get_child_count() != EXPECTED_CHUNKS:
		errs.append("chunk meshes=%d, expected %d" % [chunks.get_child_count(), EXPECTED_CHUNKS])
	else:
		for child in chunks.get_children():
			var mesh_instance := child as MeshInstance3D
			if mesh_instance == null:
				errs.append("non-MeshInstance child under Chunks: %s" % child.name)
				continue
			if mesh_instance.mesh == null:
				errs.append("%s has no mesh" % mesh_instance.name)
			elif mesh_instance.mesh.get_surface_count() != 1:
				errs.append("%s surface_count=%d" % [mesh_instance.name, mesh_instance.mesh.get_surface_count()])
	if guides == null:
		errs.append("SeamGuides node missing")
	else:
		if guides.visible:
			errs.append("seam guides should be default-off")
		if guides.get_child_count() != 0:
			errs.append("default seam guides=%d, expected lazy 0" % guides.get_child_count())
	if seam_targets.size() != 0:
		errs.append("default seam targets=%d, expected lazy 0" % seam_targets.size())
	if camera == null:
		errs.append("ReviewCamera missing")

	if errs.is_empty():
		scene.call("_focus_next_seam")
		await process_frame
		guides = scene.get_node_or_null("SeamGuides")
		if guides == null or not guides.visible:
			errs.append("next-seam focus did not enable seam guides")
		elif guides.get_child_count() != EXPECTED_SEAM_GUIDES:
			errs.append("lazy seam guides=%d, expected %d" % [guides.get_child_count(), EXPECTED_SEAM_GUIDES])
		seam_targets = scene.get("_seam_targets")
		if seam_targets.size() != EXPECTED_SEAM_GUIDES:
			errs.append("lazy seam targets=%d, expected %d" % [seam_targets.size(), EXPECTED_SEAM_GUIDES])
		if int(scene.get("_seam_focus_index")) != 0:
			errs.append("seam focus index=%d, expected 0" % int(scene.get("_seam_focus_index")))

	scene.queue_free()
	await process_frame

	if not errs.is_empty():
		for err in errs:
			push_error(err)
		print("[wg10-rough-chunks-review] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-rough-chunks-review] status=pass chunks=%d seam_guides=%d seeds=%d" % [
		EXPECTED_CHUNKS,
		EXPECTED_SEAM_GUIDES,
		EXPECTED_SEEDS,
	])
	return 0
