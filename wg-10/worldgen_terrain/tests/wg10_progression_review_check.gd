extends SceneTree

const SCENE := "res://worldgen_terrain/harness/wg10_progression_review.tscn"

const EXPECTED_STEPS := [
	{
		"id": "reference_baseline",
		"mode": "REFERENCE",
		"preset": "network_ref",
		"status": "accepted",
		"runtime": "static_reference",
		"contract": "accepted_static_reference_visual_baseline",
	},
	{
		"id": "mountain_network_bridge",
		"mode": "MOUNTAIN",
		"preset": "network_ref",
		"status": "bridge",
		"runtime": "single",
		"contract": "single_mountain_world_layer_reference_bridge",
	},
	{
		"id": "mountain_close_debug_candidate",
		"mode": "MOUNTAIN",
		"preset": "close_debug",
		"status": "prototype",
		"runtime": "single",
		"contract": "single_seam_safe_mountain_page_recipe",
	},
	{
		"id": "world_reference_preview",
		"mode": "WORLD",
		"preset": "network_ref",
		"status": "diagnostic",
		"runtime": "world",
		"contract": "world_route_reference_height_preview",
	},
]

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-progression-review] status=skip reason=no-render-device")
		return 2

	var packed := load(SCENE)
	if packed == null:
		push_error("[wg10-progression-review] cannot load %s" % SCENE)
		return 1

	var scene: Node = packed.instantiate()
	get_root().add_child(scene)
	for _i in range(60):
		await process_frame

	var errs: Array[String] = []
	_expect(scene.has_method("debug_progression_snapshot"), "scene missing debug_progression_snapshot", errs)
	_expect(scene.has_method("set_step_index"), "scene missing set_step_index", errs)
	_expect(scene.has_method("step_count"), "scene missing step_count", errs)
	if scene.has_method("step_count"):
		_expect(int(scene.call("step_count")) == EXPECTED_STEPS.size(), "unexpected step_count", errs)
	var future_count := int(scene.call("future_step_count")) if scene.has_method("future_step_count") else 0

	for i in range(EXPECTED_STEPS.size()):
		var ok := bool(scene.call("set_step_index", i))
		_expect(ok, "set_step_index(%d) failed" % i, errs)
		for _f in range(60):
			await process_frame
		if not scene.has_method("debug_progression_snapshot"):
			continue
		var snapshot: Dictionary = scene.call("debug_progression_snapshot")
		_expect_step(snapshot, EXPECTED_STEPS[i], i, errs)

	scene.queue_free()
	scene = null
	await process_frame
	await process_frame

	if not errs.is_empty():
		for err in errs:
			push_error(err)
		print("[wg10-progression-review] status=fail errors=%d" % errs.size())
		return 1

	print("[wg10-progression-review] status=pass steps=%d future=%d" % [
		EXPECTED_STEPS.size(),
		future_count,
	])
	return 0

func _expect_step(snapshot: Dictionary, expected: Dictionary, index: int, errs: Array[String]) -> void:
	var label := "%d:%s" % [index, str(expected.get("id", ""))]
	_expect(str(snapshot.get("last_config_error", "")) == "", "%s configure error: %s" % [label, str(snapshot.get("last_config_error", ""))], errs)
	_expect(int(snapshot.get("step_index", -1)) == index, "%s step_index mismatch" % label, errs)
	_expect(str(snapshot.get("step_id", "")) == str(expected.get("id", "")), "%s step id mismatch" % label, errs)
	_expect(str(snapshot.get("step_status", "")) == str(expected.get("status", "")), "%s step status mismatch" % label, errs)
	_expect(str(snapshot.get("mode", "")) == str(expected.get("mode", "")), "%s mode mismatch" % label, errs)
	_expect(str(snapshot.get("preset", "")) == str(expected.get("preset", "")), "%s preset mismatch" % label, errs)
	_expect(str(snapshot.get("runtime_mode", "")) == str(expected.get("runtime", "")), "%s runtime mismatch" % label, errs)
	_expect(str(snapshot.get("expected_contract", "")) == str(expected.get("contract", "")), "%s expected contract mismatch" % label, errs)

	var stats: Dictionary = snapshot.get("stats", {})
	_expect(int(stats.get("resident", 0)) > 0, "%s expected resident pages" % label, errs)
	_expect(int(stats.get("created", 0)) > 0, "%s expected created pages" % label, errs)
	_expect(int(stats.get("full_events", 0)) == 0, "%s expected no pool full events" % label, errs)

	var contract: Dictionary = snapshot.get("contract", {})
	_expect(str(contract.get("contract_kind", "")) == str(expected.get("contract", "")), "%s contract kind mismatch" % label, errs)
	_expect(not bool(contract.get("satisfies_mountain_world_layer_contract", true)), "%s should not claim final live contract" % label, errs)
	_expect(str(contract.get("blocking_gap", "")) != "", "%s expected explicit blocking gap" % label, errs)

	if str(expected.get("id", "")) == "reference_baseline":
		_expect(bool(contract.get("accepted_visual_baseline", false)), "%s expected accepted baseline flag" % label, errs)
		_expect(bool(contract.get("has_pass_network_routes", false)), "%s expected pass-network facts" % label, errs)
		_expect(bool(contract.get("has_page_stable_conditioning", false)), "%s expected conditioning facts" % label, errs)
		_expect(int(snapshot.get("static_material_bound_tiles", 0)) > 0, "%s expected material pages" % label, errs)
	elif str(expected.get("id", "")) == "mountain_network_bridge":
		_expect(str(contract.get("height_source", "")) == "bound_world_layer_reference_payload", "%s expected reference-backed height" % label, errs)
		_expect(bool(contract.get("has_bound_world_layer_reference", false)), "%s expected bound reference" % label, errs)
		_expect(bool(contract.get("has_pass_network_routes", false)), "%s expected pass-network facts" % label, errs)
		_expect(int(snapshot.get("static_material_bound_tiles", 0)) > 0, "%s expected material pages" % label, errs)
	elif str(expected.get("id", "")) == "mountain_close_debug_candidate":
		_expect(not bool(contract.get("has_bound_world_layer_reference", true)), "%s should not bind accepted reference" % label, errs)
		_expect(not bool(contract.get("has_pass_network_routes", true)), "%s should expose missing pass-network gap" % label, errs)
		_expect(int(snapshot.get("static_material_bound_tiles", -1)) == 0, "%s should not bind material pages" % label, errs)
	elif str(expected.get("id", "")) == "world_reference_preview":
		_expect(str(contract.get("height_source", "")) == "accepted_reference_payload_for_preview", "%s expected accepted preview height" % label, errs)
		_expect(bool(contract.get("has_world_preview_reference", false)), "%s expected world preview reference" % label, errs)
		_expect(int(snapshot.get("static_material_bound_tiles", 0)) > 0, "%s expected material pages" % label, errs)
		var weight_report: Dictionary = snapshot.get("world_weight_report", {})
		_expect(int(weight_report.get("rows", 0)) == 17, "%s expected 17x17 WORLD weight sample" % label, errs)
		_expect(int(weight_report.get("active_biomes", 0)) == 1, "%s expected bounded WORLD preview" % label, errs)

	var future: Array = snapshot.get("future_steps", [])
	_expect(future.size() >= 5, "%s expected future progression steps" % label, errs)
	_expect(_future_contains(future, "procedural_mountain_world_layer"), "%s missing procedural future step" % label, errs)

func _future_contains(future: Array, id: String) -> bool:
	for item in future:
		if item is Dictionary and str(item.get("id", "")) == id:
			return true
	return false

func _expect(condition: bool, message: String, errs: Array[String]) -> void:
	if not condition:
		errs.append(message)
