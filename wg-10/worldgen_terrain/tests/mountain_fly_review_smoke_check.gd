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
	_expect(not bool(snapshot.get("is_world", false)), "default producer should not be WORLD", errs)
	_expect(not bool(snapshot.get("is_legacy", false)), "default producer should not be LEGACY", errs)
	_expect(not bool(snapshot.get("morph_enabled", true)), "runtime default morph should be off", errs)
	_expect(not bool(snapshot.get("detail_on", true)), "runtime default detail should be off", errs)
	_expect(absf(float(snapshot.get("loaded_edge_m", 0.0)) - 196608.0) < 0.001, "expected loaded_edge_m=196608", errs)

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
