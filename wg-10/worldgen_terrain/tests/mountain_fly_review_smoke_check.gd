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
	var pool: Object = scene.get("_pool")
	var producer: Object = scene.get("_producer")
	var runtime: Object = scene.get("_runtime")
	var view: Object = scene.get("_view")
	var streamer: Object = scene.get("_streamer")
	var rings: Object = scene.get("_rings")
	var camera := scene.get("_camera") as Camera3D

	_expect(pool != null, "pool missing", errs)
	_expect(producer != null, "producer helper missing", errs)
	_expect(runtime != null, "runtime config missing", errs)
	_expect(view != null, "view missing", errs)
	_expect(streamer != null, "streamer missing", errs)
	_expect(rings != null, "rings missing", errs)
	_expect(camera != null, "camera missing", errs)

	var runtime_mode := ""
	var biome_path := false
	var stats := {}
	if pool != null:
		runtime_mode = str(pool.call("biome_runtime_mode"))
		biome_path = bool(pool.call("uses_biome_path"))
		stats = pool.call("stats")
		_expect(runtime_mode == "single", "expected runtime=single, got %s" % runtime_mode, errs)
		_expect(biome_path, "expected biome path enabled", errs)
		_expect(int(stats.get("created", 0)) > 0, "expected at least one created page", errs)
		_expect(int(stats.get("resident", 0)) > 0, "expected at least one resident page", errs)

	if producer != null:
		_expect(str(producer.mode_label()) == "MOUNTAIN", "expected default mode MOUNTAIN", errs)
		_expect(str(producer.preset_label()) == "network_ref", "expected default preset network_ref", errs)
		_expect(absf(float(producer.feature_span_m()) - 90000.0) < 0.001, "expected feature_span_m=90000", errs)
		_expect(absf(float(producer.relief_m()) - 1000.0) < 0.001, "expected relief_m=1000", errs)
		_expect(not bool(producer.is_world()), "default producer should not be WORLD", errs)
		_expect(not bool(producer.is_legacy()), "default producer should not be LEGACY", errs)
	if runtime != null:
		_expect(not bool(runtime.default_morph_enabled()), "runtime default morph should be off", errs)
		_expect(not bool(runtime.default_detail_enabled()), "runtime default detail should be off", errs)
		_expect(absf(float(runtime.loaded_edge_m()) - 196608.0) < 0.001, "expected loaded_edge_m=196608", errs)

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
