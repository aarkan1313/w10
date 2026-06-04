extends SceneTree

const SCENE := "res://worldgen_terrain/harness/mountain_fly_review.tscn"

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-mountain-fly-review-smoke] status=skip reason=no-render-device")
		return 2

	var packed := load(SCENE)
	if packed == null:
		push_error("[wg10-mountain-fly-review-smoke] cannot load %s" % SCENE)
		return 1

	var scene: Node = packed.instantiate()
	get_root().add_child(scene)
	for _i in range(45):
		await process_frame

	var errs: Array[String] = []
	var snapshot := {}
	if scene.has_method("debug_runtime_snapshot"):
		snapshot = scene.call("debug_runtime_snapshot")
	else:
		errs.append("scene missing debug_runtime_snapshot")

	_expect(bool(snapshot.get("has_pool", false)), "pool missing", errs)
	_expect(bool(snapshot.get("has_producer", false)), "producer helper missing", errs)
	_expect(bool(snapshot.get("has_runtime", false)), "runtime config missing", errs)
	_expect(bool(snapshot.get("has_view", false)), "view missing", errs)
	_expect(bool(snapshot.get("has_streamer", false)), "streamer missing", errs)
	_expect(bool(snapshot.get("has_rings", false)), "rings missing", errs)
	_expect(bool(snapshot.get("has_camera", false)), "camera missing", errs)
	_expect(str(snapshot.get("last_config_error", "")) == "", "configure error: %s" % str(snapshot.get("last_config_error", "")), errs)

	var runtime_mode := ""
	var biome_path := false
	var stats := {}
	runtime_mode = str(snapshot.get("runtime_mode", ""))
	biome_path = bool(snapshot.get("biome_path", false))
	stats = snapshot.get("stats", {})
	_expect(runtime_mode == "single", "expected runtime=single, got %s" % runtime_mode, errs)
	_expect(biome_path, "expected biome path enabled", errs)
	_expect(int(stats.get("created", 0)) > 0, "expected at least one created page", errs)
	_expect(int(stats.get("resident", 0)) > 0, "expected at least one resident page", errs)

	_expect(str(snapshot.get("mode", "")) == "MOUNTAIN", "expected default mode MOUNTAIN", errs)
	_expect(str(snapshot.get("preset", "")) == "network_ref", "expected default preset network_ref", errs)
	_expect(int(snapshot.get("seed", 0)) == 177, "expected seed=177", errs)
	_expect(absf(float(snapshot.get("feature_span_m", 0.0)) - 90000.0) < 0.001, "expected feature_span_m=90000", errs)
	_expect(absf(float(snapshot.get("relief_m", 0.0)) - 1700.0) < 0.001, "expected relief_m=1700", errs)
	_expect(absf(float(snapshot.get("view_relief_scale", 0.0)) - 0.5) < 0.001, "expected review relief scale=0.5", errs)
	var source_transform: Dictionary = snapshot.get("source_transform", {})
	_expect(absf(float(source_transform.get("source_scale", 0.0)) - 3.515625) < 0.000001, "expected source scale=3.515625", errs)
	_expect(absf(float(source_transform.get("source_offset_x_m", 0.0)) - 207000.0) < 0.001, "expected source x offset=207000", errs)
	_expect(absf(float(source_transform.get("source_offset_z_m", 0.0)) - 176000.0) < 0.001, "expected source z offset=176000", errs)
	_expect(not bool(snapshot.get("is_world", false)), "default producer should not be WORLD", errs)
	_expect(not bool(snapshot.get("is_legacy", false)), "default producer should not be LEGACY", errs)
	_expect(not bool(snapshot.get("morph_enabled", true)), "runtime default morph should be off", errs)
	_expect(not bool(snapshot.get("detail_on", true)), "runtime default detail should be off", errs)
	_expect(absf(float(snapshot.get("loaded_edge_m", 0.0)) - 196608.0) < 0.001, "expected loaded_edge_m=196608", errs)

	await _expect_mode_switch(scene, "REFERENCE", "static_reference", true, false, false, 177, 1.0, errs)
	await _expect_mode_switch(scene, "WORLD", "world", true, true, false, 1337, 0.25, errs)
	await _expect_mode_switch(scene, "LEGACY", "legacy", false, false, true, 1337, 0.25, errs)
	await _expect_mode_switch(scene, "MOUNTAIN", "single", true, false, false, 177, 0.5, errs)

	scene.queue_free()
	await process_frame

	if not errs.is_empty():
		for err in errs:
			push_error(err)
		print("[wg10-mountain-fly-review-smoke] status=fail errors=%d" % errs.size())
		return 1

	print("[wg10-mountain-fly-review-smoke] status=pass runtime=%s biome_path=%s created=%d resident=%d" % [
		runtime_mode,
		str(biome_path),
		int(stats.get("created", 0)),
		int(stats.get("resident", 0)),
	])
	return 0

func _expect(condition: bool, message: String, errs: Array[String]) -> void:
	if not condition:
		errs.append(message)

func _expect_mode_switch(
	scene: Node,
	mode: String,
	expected_runtime: String,
	expected_biome_path: bool,
	expected_world: bool,
	expected_legacy: bool,
	expected_seed: int,
	expected_view_relief_scale: float,
	errs: Array[String],
) -> void:
	if not scene.has_method("_set_producer_mode"):
		errs.append("scene missing _set_producer_mode")
		return
	scene.call("_set_producer_mode", mode)
	for _i in range(30):
		await process_frame
	var snapshot := {}
	if scene.has_method("debug_runtime_snapshot"):
		snapshot = scene.call("debug_runtime_snapshot")
	else:
		errs.append("scene missing debug_runtime_snapshot after %s switch" % mode)
		return

	_expect(str(snapshot.get("last_config_error", "")) == "", "%s configure error: %s" % [mode, str(snapshot.get("last_config_error", ""))], errs)
	_expect(str(snapshot.get("mode", "")) == mode, "expected mode %s, got %s" % [mode, str(snapshot.get("mode", ""))], errs)
	_expect(str(snapshot.get("runtime_mode", "")) == expected_runtime, "%s expected runtime=%s, got %s" % [mode, expected_runtime, str(snapshot.get("runtime_mode", ""))], errs)
	_expect(bool(snapshot.get("biome_path", false)) == expected_biome_path, "%s biome_path mismatch" % mode, errs)
	_expect(bool(snapshot.get("is_world", false)) == expected_world, "%s is_world mismatch" % mode, errs)
	_expect(bool(snapshot.get("is_legacy", false)) == expected_legacy, "%s is_legacy mismatch" % mode, errs)
	_expect(int(snapshot.get("seed", 0)) == expected_seed, "%s expected seed=%d, got %d" % [mode, expected_seed, int(snapshot.get("seed", 0))], errs)
	_expect(absf(float(snapshot.get("view_relief_scale", 0.0)) - expected_view_relief_scale) < 0.001, "%s expected view relief scale %.3f, got %.3f" % [mode, expected_view_relief_scale, float(snapshot.get("view_relief_scale", 0.0))], errs)
	if mode == "REFERENCE":
		var reference: Dictionary = snapshot.get("static_reference", {})
		_expect(str(reference.get("source_scope", "")) == "coherent_full_field_carved_with_pass_network_sliced_for_review", "REFERENCE missing mountain-network source scope", errs)
		_expect(str(reference.get("generator_version", "")).contains("pass_network"), "REFERENCE missing pass-network generator version", errs)
		_expect(absf(float(reference.get("height_scale_m", 0.0)) - 1700.0) < 0.001, "REFERENCE expected height_scale_m=1700", errs)
		_expect(absf(float(reference.get("feature_span_m", 0.0)) - 90000.0) < 0.001, "REFERENCE expected feature_span_m=90000", errs)
		_expect(bool(reference.get("has_corridor", false)), "REFERENCE expected corridor facts", errs)
		_expect(float(reference.get("corridor_frac", 0.0)) > 0.0, "REFERENCE expected nonzero corridor coverage", errs)
		_expect(int(reference.get("pass_network_routes", 0)) > 0, "REFERENCE expected pass-network routes", errs)
		_expect(float(reference.get("pass_network_carved_frac", 0.0)) > 0.0, "REFERENCE expected pass-network carve coverage", errs)
	var switched_stats: Dictionary = snapshot.get("stats", {})
	_expect(int(switched_stats.get("resident", 0)) > 0, "%s expected resident pages after switch" % mode, errs)
