extends SceneTree

const SCENE := "res://worldgen_terrain/harness/mountain_fly_review.tscn"
const ACCEPTED_RUNTIME_TILE_SOURCE_SCOPE := "generated_mountain_world_layer_tile_for_runtime_cache"

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
	_expect(bool(snapshot.get("camera_has_sync_mouse_from_rotation", false)), "camera missing sync_mouse_from_rotation", errs)
	_expect(str(snapshot.get("last_config_error", "")) == "", "configure error: %s" % str(snapshot.get("last_config_error", "")), errs)

	var runtime_mode := ""
	var biome_path := false
	var stats := {}
	runtime_mode = str(snapshot.get("runtime_mode", ""))
	biome_path = bool(snapshot.get("biome_path", false))
	stats = snapshot.get("stats", {})
	_expect(runtime_mode == "static_reference", "expected runtime=static_reference, got %s" % runtime_mode, errs)
	_expect(biome_path, "expected biome path enabled", errs)
	_expect(int(stats.get("created", 0)) > 0, "expected at least one created page", errs)
	_expect(int(stats.get("resident", 0)) > 0, "expected at least one resident page", errs)

	_expect(str(snapshot.get("mode", "")) == "REFERENCE", "expected default mode REFERENCE", errs)
	_expect(str(snapshot.get("preset", "")) == "network_ref", "expected default preset network_ref", errs)
	_expect_mode_taxonomy(snapshot, "default", errs)
	_expect(int(snapshot.get("seed", 0)) == 177, "expected seed=177", errs)
	_expect(absf(float(snapshot.get("feature_span_m", 0.0)) - 90000.0) < 0.001, "expected feature_span_m=90000", errs)
	_expect(absf(float(snapshot.get("relief_m", 0.0)) - 1700.0) < 0.001, "expected relief_m=1700", errs)
	_expect(absf(float(snapshot.get("view_relief_scale", 0.0)) - 1.0) < 0.001, "expected reference relief scale=1.0", errs)
	_expect(absf(float(snapshot.get("view_relief_ref", 0.0)) - 1700.0) < 0.001, "expected reference relief ref=1700", errs)
	_expect_view_config(snapshot, "default", 1.0, 1700.0, errs)
	var source_transform: Dictionary = snapshot.get("source_transform", {})
	_expect(absf(float(source_transform.get("source_scale", 0.0)) - 1.0) < 0.000001, "expected reference source scale=1", errs)
	_expect(absf(float(source_transform.get("source_offset_x_m", 0.0))) < 0.001, "expected reference source x offset=0", errs)
	_expect(absf(float(source_transform.get("source_offset_z_m", 0.0))) < 0.001, "expected reference source z offset=0", errs)
	_expect(not bool(snapshot.get("is_world", false)), "default producer should not be WORLD", errs)
	_expect(not bool(snapshot.get("is_legacy", false)), "default producer should not be LEGACY", errs)
	_expect(not bool(snapshot.get("morph_enabled", true)), "runtime default morph should be off", errs)
	_expect(not bool(snapshot.get("detail_on", true)), "runtime default detail should be off", errs)
	_expect_review_presentation_defaults(snapshot, "default", errs)
	_expect(absf(float(snapshot.get("loaded_edge_m", 0.0)) - 196608.0) < 0.001, "expected loaded_edge_m=196608", errs)
	_expect_reference_contract(snapshot, "default", errs)
	var reference_center_page: Dictionary = snapshot.get("static_reference_center_page", {})
	_expect_world_layer_contract_report(
		snapshot,
		"default",
		"accepted_static_reference_visual_baseline",
		true,
		false,
		true,
		true,
		true,
		errs
	)

	await _expect_mode_switch_resets_diagnostics(scene, errs)
	await _expect_mode_switch(scene, "MOUNTAIN", "single", true, false, false, 177, 1.0, 1700.0, reference_center_page, errs)
	await _expect_close_debug_candidate(scene, errs)
	await _expect_mode_switch(scene, "WORLD", "world", true, true, false, 1337, 1.0, 1700.0, {}, errs)
	await _expect_mode_switch(scene, "LEGACY", "legacy", false, false, true, 1337, 0.25, 1700.0, {}, errs)
	await _expect_mode_switch(scene, "REFERENCE", "static_reference", true, false, false, 177, 1.0, 1700.0, {}, errs)

	scene.queue_free()
	scene = null
	await process_frame
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

func _expect_float_close(actual: float, expected: float, eps: float, message: String, errs: Array[String]) -> void:
	if absf(actual - expected) > eps:
		errs.append("%s: expected %.12f got %.12f" % [message, expected, actual])

func _expect_mode_taxonomy(snapshot: Dictionary, label: String, errs: Array[String]) -> void:
	var mode := str(snapshot.get("mode", ""))
	var preset := str(snapshot.get("preset", ""))
	var expected_role := ""
	var expected_acceptance := ""
	var expected_note_fragment := ""
	if mode == "REFERENCE":
		expected_role = "accepted_reference_baseline"
		expected_acceptance = "accepted_visual_baseline"
		expected_note_fragment = "static mountain-network"
	elif mode == "MOUNTAIN" and preset == "network_ref":
		expected_role = "reference_backed_mountain_bridge"
		expected_acceptance = "accepted_visual_bridge_not_final_procedural"
		expected_note_fragment = "reference-backed"
	elif mode == "MOUNTAIN":
		expected_role = "live_mountain_recipe_debug"
		expected_acceptance = "prototype_not_accepted"
		expected_note_fragment = "raw live"
	elif mode == "WORLD":
		expected_role = "world_composition_diagnostic"
		expected_acceptance = "diagnostic_not_owner_accepted"
		expected_note_fragment = "reference height"
		_expect(int(snapshot.get("world_active_biome_limit", -1)) == 1, "%s WORLD expected one active biome per page" % label, errs)
	elif mode == "LEGACY":
		expected_role = "legacy_atlas_regression"
		expected_acceptance = "legacy_regression_not_accepted"
		expected_note_fragment = "legacy atlas"
	else:
		errs.append("%s unknown mode for taxonomy: %s" % [label, mode])
		return
	_expect(str(snapshot.get("mode_role", "")) == expected_role, "%s expected role %s, got %s" % [label, expected_role, str(snapshot.get("mode_role", ""))], errs)
	_expect(str(snapshot.get("mode_acceptance", "")) == expected_acceptance, "%s expected acceptance %s, got %s" % [label, expected_acceptance, str(snapshot.get("mode_acceptance", ""))], errs)
	_expect(str(snapshot.get("mode_note", "")).contains(expected_note_fragment), "%s expected note to contain %s, got %s" % [label, expected_note_fragment, str(snapshot.get("mode_note", ""))], errs)

func _send_key(scene: Node, keycode: int) -> void:
	var event := InputEventKey.new()
	event.pressed = true
	event.keycode = keycode
	scene.call("_input", event)

func _expect_review_presentation_defaults(snapshot: Dictionary, label: String, errs: Array[String]) -> void:
	_expect(int(snapshot.get("debug_mode", -1)) == 0, "%s expected normal material debug mode" % label, errs)
	_expect(not bool(snapshot.get("cull_disabled", true)), "%s expected culling enabled" % label, errs)
	_expect(not bool(snapshot.get("detail_on", true)), "%s expected display detail disabled" % label, errs)

func _expect_mode_switch_resets_diagnostics(scene: Node, errs: Array[String]) -> void:
	_send_key(scene, KEY_M)
	_send_key(scene, KEY_K)
	for _i in range(2):
		await process_frame
	var dirty := {}
	if scene.has_method("debug_runtime_snapshot"):
		dirty = scene.call("debug_runtime_snapshot")
	else:
		errs.append("scene missing debug_runtime_snapshot after dirtying diagnostics")
		return
	_expect(int(dirty.get("debug_mode", -1)) == 1, "diagnostic setup expected morph heatmap mode", errs)
	_expect(bool(dirty.get("cull_disabled", false)), "diagnostic setup expected cull disabled", errs)
	await _expect_mode_switch(scene, "REFERENCE", "static_reference", true, false, false, 177, 1.0, 1700.0, {}, errs)

func _expect_reference_contract(snapshot: Dictionary, label: String, errs: Array[String]) -> void:
	var reference: Dictionary = snapshot.get("static_reference", {})
	_expect(str(reference.get("source_scope", "")) == ACCEPTED_RUNTIME_TILE_SOURCE_SCOPE, "%s REFERENCE missing runtime tile source scope" % label, errs)
	_expect(str(reference.get("generator_version", "")).contains("pass_network"), "%s REFERENCE missing pass-network generator version" % label, errs)
	_expect(absf(float(reference.get("height_scale_m", 0.0)) - 1700.0) < 0.001, "%s REFERENCE expected height_scale_m=1700" % label, errs)
	_expect(absf(float(reference.get("feature_span_m", 0.0)) - 90000.0) < 0.001, "%s REFERENCE expected feature_span_m=90000" % label, errs)
	_expect_runtime_tile_source_mapping(reference, "%s REFERENCE" % label, errs)
	_expect(bool(reference.get("has_corridor", false)), "%s REFERENCE expected corridor facts" % label, errs)
	_expect(float(reference.get("corridor_frac", 0.0)) > 0.0, "%s REFERENCE expected nonzero corridor coverage" % label, errs)
	_expect(bool(reference.get("has_material_hints", false)), "%s REFERENCE expected material hint facts" % label, errs)
	_expect(float(reference.get("floor_hint_frac", 0.0)) > 0.0, "%s REFERENCE expected floor hint coverage" % label, errs)
	_expect(float(reference.get("rock_hint_frac", 0.0)) > 0.0, "%s REFERENCE expected rock hint coverage" % label, errs)
	_expect(int(reference.get("pass_network_routes", 0)) > 0, "%s REFERENCE expected pass-network routes" % label, errs)
	_expect(float(reference.get("pass_network_carved_frac", 0.0)) > 0.0, "%s REFERENCE expected pass-network carve coverage" % label, errs)
	_expect(bool(reference.get("has_conditioning_stats", false)), "%s REFERENCE expected conditioning facts" % label, errs)
	_expect(float(reference.get("conditioning_source_ptp", 0.0)) > 0.0, "%s REFERENCE expected source conditioning span" % label, errs)
	_expect(float(reference.get("conditioning_p95", 0.0)) > float(reference.get("conditioning_p05", 0.0)), "%s REFERENCE expected ordered conditioning percentiles" % label, errs)
	_expect(float(reference.get("conditioning_ptp", 0.0)) > 0.0, "%s REFERENCE expected conditioned height span" % label, errs)
	var center_page: Dictionary = snapshot.get("static_reference_center_page", {})
	_expect(bool(center_page.get("has_corridor", false)), "%s REFERENCE center page expected corridor facts" % label, errs)
	_expect(int(center_page.get("samples_px", 0)) == 17, "%s REFERENCE center page expected 17 samples" % label, errs)
	_expect(float(center_page.get("corridor_frac", -1.0)) >= 0.0, "%s REFERENCE center page expected corridor fraction" % label, errs)
	_expect(bool(center_page.get("has_material_hints", false)), "%s REFERENCE center page expected material hint facts" % label, errs)
	_expect(float(center_page.get("floor_hint_mean", -1.0)) >= 0.0, "%s REFERENCE center page expected floor hint mean" % label, errs)
	_expect(float(center_page.get("rock_hint_mean", -1.0)) >= 0.0, "%s REFERENCE center page expected rock hint mean" % label, errs)
	_expect(int(snapshot.get("static_material_bound_tiles", 0)) > 0, "%s REFERENCE expected bound static material page textures" % label, errs)

func _expect_world_layer_contract_report(
	snapshot: Dictionary,
	label: String,
	expected_kind: String,
	expected_accepted_baseline: bool,
	expected_live_candidate: bool,
	expected_pass_network: bool,
	expected_conditioning: bool,
	expected_material_hints: bool,
	errs: Array[String],
) -> void:
	var report: Dictionary = snapshot.get("mountain_world_layer_contract", {})
	_expect(str(report.get("runtime_mode", "")) == str(snapshot.get("runtime_mode", "")), "%s contract report runtime mismatch" % label, errs)
	_expect(str(report.get("contract_kind", "")) == expected_kind, "%s expected contract kind %s, got %s" % [label, expected_kind, str(report.get("contract_kind", ""))], errs)
	_expect(bool(report.get("accepted_visual_baseline", false)) == expected_accepted_baseline, "%s accepted baseline flag mismatch" % label, errs)
	_expect(bool(report.get("explicit_live_candidate", false)) == expected_live_candidate, "%s live candidate flag mismatch" % label, errs)
	_expect(bool(report.get("has_pass_network_routes", false)) == expected_pass_network, "%s pass-network fact flag mismatch" % label, errs)
	_expect(bool(report.get("has_route_carving", false)) == expected_pass_network, "%s route-carving fact flag mismatch" % label, errs)
	_expect(bool(report.get("has_page_stable_conditioning", false)) == expected_conditioning, "%s conditioning fact flag mismatch" % label, errs)
	_expect(bool(report.get("has_material_hints", false)) == expected_material_hints, "%s material hint flag mismatch" % label, errs)
	_expect(not bool(report.get("satisfies_mountain_world_layer_contract", true)), "%s should not claim full live mountain contract yet" % label, errs)
	_expect(str(report.get("blocking_gap", "")) != "", "%s expected an explicit blocking gap string" % label, errs)
	if expected_kind != "legacy_dem_kernel_atlas":
		_expect(bool(report.get("has_source_display_mapping", false)), "%s expected loaded source/display mapping facts" % label, errs)
	if expected_kind == "single_seam_safe_mountain_page_recipe":
		_expect(str(report.get("blocking_gap", "")).contains("pass-network"), "%s live MOUNTAIN gap should name pass-network" % label, errs)
	if expected_kind == "single_mountain_world_layer_reference_bridge":
		_expect(str(report.get("blocking_gap", "")).contains("reference-backed"), "%s live MOUNTAIN bridge gap should name reference-backed height" % label, errs)
		_expect(str(report.get("height_source", "")) == "bound_world_layer_reference_payload", "%s live MOUNTAIN expected reference-backed height source" % label, errs)
		_expect(not bool(report.get("procedural_world_layer_height", true)), "%s live MOUNTAIN bridge should not claim procedural height" % label, errs)
		if expected_pass_network:
			_expect(bool(report.get("has_bound_world_layer_reference", false)), "%s live MOUNTAIN expected bound world-layer reference facts" % label, errs)
			_expect(bool(report.get("height_consumes_world_layer_facts", false)), "%s live MOUNTAIN bridge should consume bound height facts" % label, errs)
			_expect(str(report.get("reference_source_scope", "")) == ACCEPTED_RUNTIME_TILE_SOURCE_SCOPE, "%s live MOUNTAIN reference source scope mismatch" % label, errs)
	if expected_kind == "world_route_reference_height_preview":
		_expect(str(report.get("height_source", "")) == "accepted_reference_payload_for_preview", "%s WORLD preview expected accepted reference height source" % label, errs)
		_expect(not bool(report.get("procedural_world_layer_height", true)), "%s WORLD preview should not claim procedural height" % label, errs)
		_expect(bool(report.get("has_world_preview_reference", false)), "%s WORLD preview expected bound reference height" % label, errs)
		_expect(str(report.get("reference_source_scope", "")) == ACCEPTED_RUNTIME_TILE_SOURCE_SCOPE, "%s WORLD preview reference source scope mismatch" % label, errs)
	if expected_kind == "accepted_static_reference_visual_baseline":
		_expect(str(report.get("source_scope", "")) == ACCEPTED_RUNTIME_TILE_SOURCE_SCOPE, "%s REFERENCE contract report source scope mismatch" % label, errs)

func _expect_view_config(snapshot: Dictionary, label: String, expected_relief_scale: float, expected_relief_ref: float, errs: Array[String]) -> void:
	var view_config: Dictionary = snapshot.get("view_config", {})
	_expect(bool(view_config.get("configured", false)), "%s view should report configured", errs)
	_expect(absf(float(view_config.get("relief_scale", 0.0)) - expected_relief_scale) < 0.001, "%s view actual relief scale mismatch" % label, errs)
	_expect(absf(float(view_config.get("relief_ref", 0.0)) - expected_relief_ref) < 0.001, "%s view actual relief ref mismatch" % label, errs)
	var expected_morph := 0.15 if bool(snapshot.get("morph_enabled", false)) else 0.0
	_expect(absf(float(view_config.get("morph_region", -1.0)) - expected_morph) < 0.001, "%s view actual morph region mismatch" % label, errs)
	_expect(int(view_config.get("num_levels", 0)) == 5, "%s view expected 5 levels" % label, errs)
	_expect(absf(float(view_config.get("base_span_m", 0.0)) - 8192.0) < 0.001, "%s view expected base span 8192" % label, errs)

func _expect_mountain_layer_reference_contract(snapshot: Dictionary, label: String, errs: Array[String]) -> void:
	var reference: Dictionary = snapshot.get("mountain_world_layer_reference", {})
	_expect(str(reference.get("source_scope", "")) == ACCEPTED_RUNTIME_TILE_SOURCE_SCOPE, "%s bound mountain layer missing source scope" % label, errs)
	_expect(str(reference.get("generator_version", "")).contains("pass_network"), "%s bound mountain layer missing pass-network generator version" % label, errs)
	_expect_runtime_tile_source_mapping(reference, "%s bound mountain layer" % label, errs)
	_expect(bool(reference.get("has_corridor", false)), "%s bound mountain layer expected corridor facts" % label, errs)
	_expect(bool(reference.get("has_material_hints", false)), "%s bound mountain layer expected material hints" % label, errs)
	_expect(int(reference.get("pass_network_routes", 0)) > 0, "%s bound mountain layer expected pass-network routes" % label, errs)
	_expect(float(reference.get("pass_network_carved_frac", 0.0)) > 0.0, "%s bound mountain layer expected carved fraction" % label, errs)
	_expect(bool(reference.get("has_conditioning_stats", false)), "%s bound mountain layer expected conditioning stats" % label, errs)
	var center_page: Dictionary = snapshot.get("mountain_world_layer_reference_center_page", {})
	_expect(bool(center_page.get("has_corridor", false)), "%s bound mountain layer center page expected corridor facts" % label, errs)
	_expect(bool(center_page.get("has_material_hints", false)), "%s bound mountain layer center page expected material hints" % label, errs)
	_expect(float(center_page.get("floor_hint_mean", -1.0)) >= 0.0, "%s bound mountain layer expected floor hint mean" % label, errs)
	_expect(float(center_page.get("rock_hint_mean", -1.0)) >= 0.0, "%s bound mountain layer expected rock hint mean" % label, errs)
	_expect(int(snapshot.get("static_material_bound_tiles", 0)) > 0, "%s live MOUNTAIN expected bound material fact pages" % label, errs)

func _expect_runtime_tile_source_mapping(reference: Dictionary, label: String, errs: Array[String]) -> void:
	_expect(bool(reference.get("has_source_display_mapping", false)), "%s expected explicit source/display mapping" % label, errs)
	_expect_float_close(float(reference.get("display_origin_x_m", 0.0)), -38400.0, 0.001, "%s display origin x" % label, errs)
	_expect_float_close(float(reference.get("display_origin_z_m", 0.0)), -38400.0, 0.001, "%s display origin z" % label, errs)
	_expect_float_close(float(reference.get("display_span_x_m", 0.0)), 76800.0, 0.001, "%s display span x" % label, errs)
	_expect_float_close(float(reference.get("display_span_z_m", 0.0)), 76800.0, 0.001, "%s display span z" % label, errs)
	_expect_float_close(float(reference.get("source_origin_x_m", 0.0)), 72000.0, 0.001, "%s source origin x" % label, errs)
	_expect_float_close(float(reference.get("source_origin_z_m", 0.0)), 41000.0, 0.001, "%s source origin z" % label, errs)
	_expect_float_close(float(reference.get("source_span_x_m", 0.0)), 270000.0, 0.001, "%s source span x" % label, errs)
	_expect_float_close(float(reference.get("source_span_z_m", 0.0)), 270000.0, 0.001, "%s source span z" % label, errs)
	_expect_float_close(float(reference.get("source_scene_ratio", 0.0)), 3.515625, 0.000001, "%s source scene ratio" % label, errs)

func _expect_bound_page_matches_reference(expected: Dictionary, actual: Dictionary, errs: Array[String]) -> void:
	_expect(not expected.is_empty(), "MOUNTAIN missing default REFERENCE center-page baseline", errs)
	_expect(not actual.is_empty(), "MOUNTAIN missing bound center-page report", errs)
	if expected.is_empty() or actual.is_empty():
		return
	for key in ["has_corridor", "has_material_hints"]:
		_expect(
			bool(actual.get(key, false)) == bool(expected.get(key, false)),
			"MOUNTAIN bound center-page %s mismatch" % key,
			errs
		)
	for key in ["level", "samples_px"]:
		_expect(
			int(actual.get(key, -1)) == int(expected.get(key, -1)),
			"MOUNTAIN bound center-page %s mismatch" % key,
			errs
		)
	for key in [
		"origin_x",
		"origin_z",
		"world_span_m",
		"corridor_frac",
		"low_pass_hint_mean",
		"floor_hint_mean",
		"rock_hint_mean",
		"snow_hint_mean",
	]:
		_expect_float_close(
			float(actual.get(key, -999999.0)),
			float(expected.get(key, -999999.0)),
			0.000000001,
			"MOUNTAIN bound center-page %s mismatch" % key,
			errs
		)

func _expect_mode_switch(
	scene: Node,
	mode: String,
	expected_runtime: String,
	expected_biome_path: bool,
	expected_world: bool,
	expected_legacy: bool,
	expected_seed: int,
	expected_view_relief_scale: float,
	expected_view_relief_ref: float,
	expected_reference_center_page: Dictionary,
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
	_expect_mode_taxonomy(snapshot, mode, errs)
	_expect(str(snapshot.get("runtime_mode", "")) == expected_runtime, "%s expected runtime=%s, got %s" % [mode, expected_runtime, str(snapshot.get("runtime_mode", ""))], errs)
	_expect(bool(snapshot.get("biome_path", false)) == expected_biome_path, "%s biome_path mismatch" % mode, errs)
	_expect(bool(snapshot.get("is_world", false)) == expected_world, "%s is_world mismatch" % mode, errs)
	_expect(bool(snapshot.get("is_legacy", false)) == expected_legacy, "%s is_legacy mismatch" % mode, errs)
	_expect(int(snapshot.get("seed", 0)) == expected_seed, "%s expected seed=%d, got %d" % [mode, expected_seed, int(snapshot.get("seed", 0))], errs)
	_expect(absf(float(snapshot.get("view_relief_scale", 0.0)) - expected_view_relief_scale) < 0.001, "%s expected view relief scale %.3f, got %.3f" % [mode, expected_view_relief_scale, float(snapshot.get("view_relief_scale", 0.0))], errs)
	_expect(absf(float(snapshot.get("view_relief_ref", 0.0)) - expected_view_relief_ref) < 0.001, "%s expected view relief ref %.3f, got %.3f" % [mode, expected_view_relief_ref, float(snapshot.get("view_relief_ref", 0.0))], errs)
	_expect_review_presentation_defaults(snapshot, mode, errs)
	if mode == "MOUNTAIN":
		var source_transform: Dictionary = snapshot.get("source_transform", {})
		_expect(absf(float(source_transform.get("source_scale", 0.0)) - 3.515625) < 0.000001, "MOUNTAIN expected source scale=3.515625", errs)
		_expect(absf(float(source_transform.get("source_offset_x_m", 0.0)) - 207000.0) < 0.001, "MOUNTAIN expected source x offset=207000", errs)
		_expect(absf(float(source_transform.get("source_offset_z_m", 0.0)) - 176000.0) < 0.001, "MOUNTAIN expected source z offset=176000", errs)
		_expect_world_layer_contract_report(snapshot, mode, "single_mountain_world_layer_reference_bridge", false, true, true, true, true, errs)
		_expect_mountain_layer_reference_contract(snapshot, mode, errs)
		_expect_bound_page_matches_reference(expected_reference_center_page, snapshot.get("mountain_world_layer_reference_center_page", {}), errs)
	_expect_view_config(snapshot, mode, expected_view_relief_scale, expected_view_relief_ref, errs)
	if mode == "WORLD":
		_expect_world_layer_contract_report(snapshot, mode, "world_route_reference_height_preview", false, false, true, true, true, errs)
		_expect_world_preview_reports(snapshot, mode, errs)
		_expect(int(snapshot.get("static_material_bound_tiles", 0)) > 0, "%s WORLD preview expected bound reference material pages" % mode, errs)
	if mode == "LEGACY":
		_expect_world_layer_contract_report(snapshot, mode, "legacy_dem_kernel_atlas", false, false, false, false, false, errs)
	if mode == "REFERENCE":
		_expect_reference_contract(snapshot, mode, errs)
		_expect_world_layer_contract_report(snapshot, mode, "accepted_static_reference_visual_baseline", true, false, true, true, true, errs)
	var switched_stats: Dictionary = snapshot.get("stats", {})
	_expect(int(switched_stats.get("resident", 0)) > 0, "%s expected resident pages after switch" % mode, errs)

func _expect_close_debug_candidate(scene: Node, errs: Array[String]) -> void:
	if not scene.has_method("_toggle_mountain_preset"):
		errs.append("scene missing _toggle_mountain_preset")
		return
	scene.call("_toggle_mountain_preset")
	for _i in range(30):
		await process_frame
	var snapshot := {}
	if scene.has_method("debug_runtime_snapshot"):
		snapshot = scene.call("debug_runtime_snapshot")
	else:
		errs.append("scene missing debug_runtime_snapshot after close-debug toggle")
		return

	_expect(str(snapshot.get("last_config_error", "")) == "", "close-debug configure error: %s" % str(snapshot.get("last_config_error", "")), errs)
	_expect(str(snapshot.get("mode", "")) == "MOUNTAIN", "close-debug expected MOUNTAIN mode", errs)
	_expect(str(snapshot.get("preset", "")) == "close_debug", "close-debug expected close_debug preset", errs)
	_expect_mode_taxonomy(snapshot, "MOUNTAIN/close_debug", errs)
	_expect(str(snapshot.get("runtime_mode", "")) == "single", "close-debug expected runtime=single", errs)
	_expect(bool(snapshot.get("biome_path", false)), "close-debug expected biome path", errs)
	_expect(absf(float(snapshot.get("feature_span_m", 0.0)) - 3500.0) < 0.001, "close-debug expected feature_span_m=3500", errs)
	_expect(absf(float(snapshot.get("view_relief_scale", 0.0)) - 0.25) < 0.001, "close-debug expected relief scale=0.25", errs)
	_expect(absf(float(snapshot.get("view_relief_ref", 0.0)) - 425.0) < 0.001, "close-debug expected relief ref=425", errs)
	_expect_review_presentation_defaults(snapshot, "MOUNTAIN/close_debug", errs)
	_expect_view_config(snapshot, "MOUNTAIN/close_debug", 0.25, 425.0, errs)
	var source_transform: Dictionary = snapshot.get("source_transform", {})
	_expect(absf(float(source_transform.get("source_scale", 0.0)) - 1.0) < 0.000001, "close-debug expected source scale=1", errs)
	_expect(absf(float(source_transform.get("source_offset_x_m", 0.0))) < 0.001, "close-debug expected source x offset=0", errs)
	_expect(absf(float(source_transform.get("source_offset_z_m", 0.0))) < 0.001, "close-debug expected source z offset=0", errs)
	_expect_world_layer_contract_report(snapshot, "MOUNTAIN/close_debug", "single_seam_safe_mountain_page_recipe", false, true, false, false, false, errs)
	var report: Dictionary = snapshot.get("mountain_world_layer_contract", {})
	_expect(not bool(report.get("has_bound_world_layer_reference", true)), "close-debug should not have a bound accepted reference", errs)
	_expect(str(report.get("height_source", "")) == "", "close-debug should not report reference-backed height", errs)
	_expect(snapshot.get("mountain_world_layer_reference", {}).is_empty(), "close-debug should not expose mountain reference facts", errs)

	# Restore the owner-accepted network lane so later mode checks keep their expected preset.
	scene.call("_toggle_mountain_preset")
	for _i in range(30):
		await process_frame
	if scene.has_method("debug_runtime_snapshot"):
		var restored: Dictionary = scene.call("debug_runtime_snapshot")
		_expect(str(restored.get("preset", "")) == "network_ref", "MOUNTAIN preset should restore to network_ref", errs)
		_expect_review_presentation_defaults(restored, "MOUNTAIN/restored_network", errs)
		_expect_view_config(restored, "MOUNTAIN/restored_network", 1.0, 1700.0, errs)

func _expect_world_preview_reports(snapshot: Dictionary, label: String, errs: Array[String]) -> void:
	var route_report: Dictionary = snapshot.get("world_biome_report_center_page", {})
	var weight_report: Dictionary = snapshot.get("world_biome_weight_field_center_page", {})
	_expect(not route_report.is_empty(), "%s expected WORLD route diagnostic report" % label, errs)
	_expect(not weight_report.is_empty(), "%s expected WORLD weight-field diagnostic report" % label, errs)
	if route_report.is_empty() or weight_report.is_empty():
		return
	_expect(int(weight_report.get("rows", 0)) == 17, "%s expected WORLD weight-field rows=17" % label, errs)
	_expect(int(weight_report.get("cols", 0)) == 17, "%s expected WORLD weight-field cols=17" % label, errs)
	_expect(int(weight_report.get("sample_count", 0)) == 289, "%s expected WORLD weight-field sample count=289" % label, errs)
	_expect(int(weight_report.get("active_biomes", 0)) == 1, "%s owner WORLD route preview should expose one active biome field" % label, errs)
	_expect(int(weight_report.get("max_texel_active_count", 0)) == 1, "%s owner WORLD preview should have one active biome per texel" % label, errs)
	_expect(absf(float(weight_report.get("max_sum_delta", 1.0))) < 0.00001, "%s WORLD preview weights should remain normalized" % label, errs)
	_expect(int(route_report.get("active_count", 0)) >= int(weight_report.get("active_biomes", 0)), "%s route report should explain at least the composed biome" % label, errs)
