extends RefCounted

# Builds the owner-fly runtime report consumed by smoke/visual gates.
# The scene owns input and assembly; this helper owns diagnostic shape.

func build(
	pool: Object,
	producer: Object,
	runtime: Object,
	view: Object,
	streamer: Object,
	rings: Object,
	camera: Object,
	morph_enabled: bool,
	detail_on: bool,
	debug_mode: int,
	cull_disabled: bool,
	last_config_error: String
) -> Dictionary:
	var stats := {}
	var source_transform := {}
	var mountain_world_layer_contract := {}
	var mountain_world_layer_reference := {}
	var mountain_world_layer_reference_center_page := {}
	var static_reference := {}
	var static_reference_center_page := {}
	var world_biome_report_center_page := {}
	var world_biome_weight_field_center_page := {}
	var view_config := {}
	var runtime_mode := "missing"
	var biome_path := false
	if pool != null:
		stats = pool.call("stats")
		source_transform = pool.call("biome_source_transform")
		mountain_world_layer_contract = pool.call("mountain_world_layer_contract_report")
		mountain_world_layer_reference = pool.call("mountain_world_layer_reference_report")
		mountain_world_layer_reference_center_page = pool.call("mountain_world_layer_reference_page_report", 0, 0.0, 0.0, 17)
		static_reference = pool.call("static_reference_report")
		static_reference_center_page = pool.call("static_reference_page_report", 0, 0.0, 0.0, 17)
		runtime_mode = str(pool.call("biome_runtime_mode"))
		biome_path = bool(pool.call("uses_biome_path"))
		if runtime_mode == "world":
			world_biome_report_center_page = pool.call("debug_world_biome_report_for_page", 0, 0.0, 0.0)
			world_biome_weight_field_center_page = pool.call("debug_world_biome_weight_field_report_for_page", 0, 0.0, 0.0, 17)
	if view != null and view.has_method("config_report"):
		view_config = view.call("config_report")

	var mode := "missing"
	var preset := "missing"
	var mode_role := "missing"
	var mode_acceptance := "missing"
	var mode_note := "missing"
	var seed := -1
	var feature_span_m := 0.0
	var relief_m := 0.0
	var view_relief_scale := 0.0
	var view_relief_ref := 0.0
	var loaded_edge_m := 0.0
	var world_active_biome_limit := -1
	var is_world := false
	var is_legacy := false
	if runtime != null:
		loaded_edge_m = float(runtime.loaded_edge_m())
	if producer != null:
		mode = str(producer.mode_label())
		preset = str(producer.preset_label())
		mode_role = str(producer.mode_role())
		mode_acceptance = str(producer.mode_acceptance())
		mode_note = str(producer.mode_note())
		seed = int(producer.runtime_seed())
		feature_span_m = float(producer.feature_span_m())
		relief_m = float(producer.relief_m())
		var default_relief_scale := float(runtime.default_relief_scale()) if runtime != null else 0.25
		var default_relief_ref := float(runtime.default_relief_ref()) if runtime != null else 1700.0
		view_relief_scale = float(producer.view_relief_scale(default_relief_scale))
		view_relief_ref = float(producer.view_relief_ref(default_relief_ref, default_relief_scale))
		world_active_biome_limit = int(producer.world_active_biome_limit())
		is_world = bool(producer.is_world())
		is_legacy = bool(producer.is_legacy())

	return {
		"last_config_error": last_config_error,
		"has_pool": pool != null,
		"has_producer": producer != null,
		"has_runtime": runtime != null,
		"has_view": view != null,
		"has_streamer": streamer != null,
		"has_rings": rings != null,
		"has_camera": camera != null,
		"runtime_mode": runtime_mode,
		"biome_path": biome_path,
		"stats": stats,
		"source_transform": source_transform,
		"mountain_world_layer_contract": mountain_world_layer_contract,
		"mountain_world_layer_reference": mountain_world_layer_reference,
		"mountain_world_layer_reference_center_page": mountain_world_layer_reference_center_page,
		"static_reference": static_reference,
		"static_reference_center_page": static_reference_center_page,
		"world_biome_report_center_page": world_biome_report_center_page,
		"world_biome_weight_field_center_page": world_biome_weight_field_center_page,
		"view_config": view_config,
		"mode": mode,
		"preset": preset,
		"mode_role": mode_role,
		"mode_acceptance": mode_acceptance,
		"mode_note": mode_note,
		"seed": seed,
		"feature_span_m": feature_span_m,
		"relief_m": relief_m,
		"view_relief_scale": view_relief_scale,
		"view_relief_ref": view_relief_ref,
		"loaded_edge_m": loaded_edge_m,
		"world_active_biome_limit": world_active_biome_limit,
		"is_world": is_world,
		"is_legacy": is_legacy,
		"morph_enabled": morph_enabled,
		"detail_on": detail_on,
		"debug_mode": debug_mode,
		"cull_disabled": cull_disabled,
		"static_material_bound_tiles": _static_material_bound_tiles(rings),
	}

func _static_material_bound_tiles(rings: Object) -> int:
	if rings == null:
		return 0
	var count := 0
	for child in rings.get_children():
		if child is MeshInstance3D:
			var mat: Material = child.get_material_override()
			if mat is ShaderMaterial:
				var mix_variant: Variant = mat.get_shader_parameter("static_material_mix")
				var mix_value := float(mix_variant) if typeof(mix_variant) in [TYPE_FLOAT, TYPE_INT] else 0.0
				if mix_value > 0.5:
					count += 1
	return count
