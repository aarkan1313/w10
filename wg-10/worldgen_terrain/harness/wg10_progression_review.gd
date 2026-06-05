extends Node3D

# Progression review harness for WG10 recovery work.
#
# This scene is intentionally narrower than mountain_fly_review.tscn: it exposes
# ordered, named validation steps so new terrain work can add one feature at a
# time and prove each layer before promotion.

const PRODUCERS := "res://worldgen_terrain/harness/mountain_fly_producers.gd"
const RUNTIME_CONFIG := "res://worldgen_terrain/harness/mountain_fly_runtime_config.gd"
const FLY_CAMERA := "res://worldgen_terrain/harness/fly_camera.gd"

const STEPS := [
	{
		"id": "reference_baseline",
		"label": "REFERENCE baseline",
		"mode": "REFERENCE",
		"preset": "network_ref",
		"status": "accepted",
		"expected_runtime": "static_reference",
		"expected_contract": "accepted_static_reference_visual_baseline",
		"adds": "accepted mountain-network height, material, pass-network, and conditioning facts",
		"blocks": "nothing; this is the visual baseline every candidate compares against",
	},
	{
		"id": "mountain_network_bridge",
		"label": "MOUNTAIN network bridge",
		"mode": "MOUNTAIN",
		"preset": "network_ref",
		"status": "bridge",
		"expected_runtime": "single",
		"expected_contract": "single_mountain_world_layer_reference_bridge",
		"adds": "single-producer runtime lane with accepted world-layer facts bound beside it",
		"blocks": "final procedural synthesis; height is still reference-backed",
	},
	{
		"id": "mountain_close_debug_candidate",
		"label": "MOUNTAIN close debug candidate",
		"mode": "MOUNTAIN",
		"preset": "close_debug",
		"status": "prototype",
		"expected_runtime": "single",
		"expected_contract": "single_seam_safe_mountain_page_recipe",
		"adds": "raw live seam-safe mountain page synthesis for measured comparison",
		"blocks": "owner acceptance until pass-network, conditioning, material facts, and visual gap close",
	},
	{
		"id": "world_reference_preview",
		"label": "WORLD reference preview",
		"mode": "WORLD",
		"preset": "network_ref",
		"status": "diagnostic",
		"expected_runtime": "world",
		"expected_contract": "world_route_reference_height_preview",
		"adds": "WORLD route/weight diagnostics over accepted reference height",
		"blocks": "full WORLD height composition until async/cache or cheaper preview is proven",
	},
]

const PROGRESSION_RULES := {
	"principle": "one shared renderer/streamer path, one feature added per step, gated before promotion",
	"gate_suite": "review_progression",
	"owner_gate_suite": "review_runtime_stress",
	"visual_gate_suite": "review_runtime_visual",
	"promotion_rule": "a step cannot become accepted terrain unless its contract facts, motion, visual repage, owner stress, and docs are green",
}

const FUTURE_STEPS := [
	{
		"id": "source_display_overlay",
		"label": "Source/display mapping overlay",
		"status": "next",
		"adds": "visible display-window and sampled-source-window facts for every current lane",
		"gate": "review_progression",
		"acceptance_rule": "mapping origin/span/scale is explicit and no scene-local scale constants are duplicated",
		"blocks": "material/pass-network work that depends on knowing which source window is sampled",
	},
	{
		"id": "material_fact_layers",
		"label": "Material fact layers",
		"status": "planned",
		"adds": "low-pass/corridor, floor, rock, and snow layers as separately gated facts",
		"gate": "review_progression + review_runtime_visual",
		"acceptance_rule": "each channel is non-vacuous, page-stable, and visually bounded against REFERENCE",
		"blocks": "procedural candidate promotion without accepted material readability",
	},
	{
		"id": "pass_network_facts",
		"label": "Pass-network facts",
		"status": "planned",
		"adds": "connected pass route and route-carving facts beside the live candidate",
		"gate": "review_progression",
		"acceptance_rule": "routes/carving are nonzero, connected at world-layer scale, and page-stable",
		"blocks": "raw live mountain terrain from being accepted as the mountain-network look",
	},
	{
		"id": "procedural_mountain_world_layer",
		"label": "Procedural mountain world layer",
		"status": "planned",
		"adds": "generated/cached world-layer height that consumes the accepted facts contract",
		"gate": "review_progression + review_runtime_visual + review_runtime_stress",
		"acceptance_rule": "numeric/visual gap to REFERENCE improves while strict streaming budgets stay green",
		"blocks": "replacing the reference-backed MOUNTAIN bridge",
	},
	{
		"id": "facts_collision_parity",
		"label": "Facts/collision parity",
		"status": "planned",
		"adds": "query/collision authority reading the same facts as the visual layer",
		"gate": "gpu + review_progression",
		"acceptance_rule": "visible terrain facts and queryable/collision facts agree over sampled pages",
		"blocks": "gameplay-facing terrain acceptance",
	},
]

var _runtime: Object
var _producer: Object
var _pool: Object
var _streamer: Object
var _rings: Object
var _view: Object
var _camera: Camera3D
var _label: Label
var _step_index := 0
var _last_config_error := ""
var _frame := 0
var _probe_mode := false

func _ready() -> void:
	if RenderingServer.get_rendering_device() == null:
		push_error("wg10_progression_review: no RenderingDevice (run windowed)")
		return

	_runtime = load(RUNTIME_CONFIG).new()
	_runtime.register_shader_globals(bool(_runtime.default_detail_enabled()))
	_producer = load(PRODUCERS).new()
	_pool = ClassDB.instantiate("Wg10PagePool")
	_last_config_error = _configure_current_step()
	if _last_config_error != "":
		push_error("wg10_progression_review: initial configure failed: %s" % _last_config_error)
		return

	_streamer = ClassDB.instantiate("Wg10Streamer")
	_runtime.configure_streamer(_streamer, _pool)
	_rings = ClassDB.instantiate("Wg10ClipmapRings")
	_runtime.configure_rings(_rings)
	add_child(_rings)

	_view = ClassDB.instantiate("Wg10TerrainView")
	_reconfigure_view()

	var env := Environment.new()
	_runtime.configure_review_environment(env)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	add_child(light)

	_camera = load(FLY_CAMERA).new()
	_camera.environment = env
	_camera.far = float(_runtime.review_visual_edge_m())
	add_child(_camera)
	_apply_review_camera_frame()

	var layer := CanvasLayer.new()
	add_child(layer)
	_label = Label.new()
	_label.position = Vector2(12, 12)
	_label.add_theme_font_size_override("font_size", 16)
	layer.add_child(_label)
	_refresh_label()

func _exit_tree() -> void:
	if _rings != null:
		_rings.call("unbind_all")
	if _pool != null:
		_pool.call("free_all")
		_pool = null

func _input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and event.keycode == KEY_BRACKETRIGHT:
		next_step()
	elif event is InputEventKey and event.pressed and event.keycode == KEY_BRACKETLEFT:
		previous_step()
	elif event is InputEventKey and event.pressed and event.keycode >= KEY_1 and event.keycode <= KEY_4:
		set_step_index(int(event.keycode - KEY_1))

func step_count() -> int:
	return STEPS.size()

func future_step_count() -> int:
	return FUTURE_STEPS.size()

func progression_manifest() -> Dictionary:
	return {
		"rules": PROGRESSION_RULES,
		"active_steps": STEPS,
		"future_steps": FUTURE_STEPS,
	}

func current_step_id() -> String:
	return str(_current_step().get("id", "missing"))

func set_step_index(index: int) -> bool:
	if index < 0 or index >= STEPS.size():
		return false
	_step_index = index
	if _rings != null:
		_rings.call("unbind_all")
	if _pool != null:
		_pool.call("free_all")
	_last_config_error = _configure_current_step()
	_runtime.set_debug_mode(0)
	_runtime.set_detail_enabled(bool(_runtime.default_detail_enabled()))
	_reconfigure_view()
	_apply_review_camera_frame()
	_refresh_label()
	if _last_config_error != "":
		push_error("wg10_progression_review: step %s configure failed: %s" % [
			current_step_id(),
			_last_config_error,
		])
		return false
	return true

func next_step() -> bool:
	return set_step_index((_step_index + 1) % STEPS.size())

func previous_step() -> bool:
	return set_step_index((_step_index - 1 + STEPS.size()) % STEPS.size())

func debug_progression_snapshot() -> Dictionary:
	var step := _current_step()
	var stats := {}
	var contract := {}
	var static_reference := {}
	var mountain_reference := {}
	var world_route_report := {}
	var world_weight_report := {}
	var runtime_mode := "missing"
	var biome_path := false
	var source_transform := {}
	var source_display_report := {}
	if _pool != null:
		stats = _pool.call("stats")
		contract = _pool.call("mountain_world_layer_contract_report")
		static_reference = _pool.call("static_reference_report")
		mountain_reference = _pool.call("mountain_world_layer_reference_report")
		runtime_mode = str(_pool.call("biome_runtime_mode"))
		biome_path = bool(_pool.call("uses_biome_path"))
		source_transform = _pool.call("biome_source_transform")
		source_display_report = _source_display_report(contract, static_reference, mountain_reference, runtime_mode, source_transform)
		if runtime_mode == "world":
			world_route_report = _pool.call("debug_world_biome_report_for_page", 0, 0.0, 0.0)
			world_weight_report = _pool.call("debug_world_biome_weight_field_report_for_page", 0, 0.0, 0.0, 17)

	return {
		"step_index": _step_index,
		"step_id": str(step.get("id", "")),
		"step_label": str(step.get("label", "")),
		"step_status": str(step.get("status", "")),
		"expected_runtime": str(step.get("expected_runtime", "")),
		"expected_contract": str(step.get("expected_contract", "")),
		"mode": str(_producer.mode_label()) if _producer != null else "missing",
		"preset": str(_producer.preset_label()) if _producer != null else "missing",
		"mode_role": str(_producer.mode_role()) if _producer != null else "missing",
		"mode_acceptance": str(_producer.mode_acceptance()) if _producer != null else "missing",
		"runtime_mode": runtime_mode,
		"biome_path": biome_path,
		"last_config_error": _last_config_error,
		"stats": stats,
		"contract": contract,
		"static_reference": static_reference,
		"mountain_world_layer_reference": mountain_reference,
		"world_route_report": world_route_report,
		"world_weight_report": world_weight_report,
		"source_transform": source_transform,
		"source_display_report": source_display_report,
		"static_material_bound_tiles": _static_material_bound_tiles(),
		"future_steps": FUTURE_STEPS,
		"progression_manifest": progression_manifest(),
	}

func set_probe_mode(enabled: bool) -> void:
	_probe_mode = enabled
	set_process(not enabled)
	if _label != null:
		_label.visible = not enabled

func update_for_probe(pos_x: float, pos_z: float, vel_x: float, vel_z: float) -> void:
	if _camera != null:
		_camera.global_position.x = pos_x
		_camera.global_position.z = pos_z
	if _view != null:
		_view.call("update", pos_x, pos_z, vel_x, vel_z)
	_refresh_label()

func set_probe_camera_frame(eye: Vector3, look: Vector3) -> void:
	if _camera == null:
		return
	_camera.look_at_from_position(eye, look, Vector3.UP)
	if _camera.has_method("sync_mouse_from_rotation"):
		_camera.call("sync_mouse_from_rotation")

func debug_tile_states() -> PackedInt64Array:
	if _rings == null:
		return PackedInt64Array()
	return _rings.call("debug_tile_states")

func debug_streamer_stats() -> Dictionary:
	if _streamer == null:
		return {}
	return _streamer.call("stats")

func _process(_delta: float) -> void:
	if _probe_mode:
		return
	if _view == null or _camera == null:
		return
	_frame += 1
	var p: Vector3 = _camera.global_position
	var v: Vector3 = _camera.call("get_velocity")
	_view.call("update", p.x, p.z, v.x, v.z)
	if _frame % 15 == 0:
		_refresh_label()

func _current_step() -> Dictionary:
	return STEPS[clampi(_step_index, 0, STEPS.size() - 1)]

func _source_display_report(
	contract: Dictionary,
	static_reference: Dictionary,
	mountain_reference: Dictionary,
	runtime_mode: String,
	source_transform: Dictionary
) -> Dictionary:
	var out := {
		"has_contract_mapping": bool(contract.get("has_source_display_mapping", false)),
		"runtime_mode": runtime_mode,
		"sample_rule": "source = display * source_scale + source_offset",
		"source_scale": float(source_transform.get("source_scale", 1.0)),
		"source_offset_x_m": float(source_transform.get("source_offset_x_m", 0.0)),
		"source_offset_z_m": float(source_transform.get("source_offset_z_m", 0.0)),
		"reference_mapping": false,
		"mapping_kind": "none",
		"promotion_gate": str(PROGRESSION_RULES.get("gate_suite", "")),
	}

	var reference := {}
	if not static_reference.is_empty():
		reference = static_reference
		out["mapping_kind"] = "static_reference_payload"
	elif not mountain_reference.is_empty():
		reference = mountain_reference
		out["mapping_kind"] = "bound_mountain_world_layer_reference"
	elif runtime_mode == "single":
		out["mapping_kind"] = "live_biome_source_transform"
	elif runtime_mode == "world":
		out["mapping_kind"] = "world_preview_reference_contract"

	if not reference.is_empty():
		out["reference_mapping"] = bool(reference.get("has_source_display_mapping", false))
		out["display_origin_x_m"] = float(reference.get("display_origin_x_m", 0.0))
		out["display_origin_z_m"] = float(reference.get("display_origin_z_m", 0.0))
		out["display_span_x_m"] = float(reference.get("display_span_x_m", 0.0))
		out["display_span_z_m"] = float(reference.get("display_span_z_m", 0.0))
		out["source_origin_x_m"] = float(reference.get("source_origin_x_m", 0.0))
		out["source_origin_z_m"] = float(reference.get("source_origin_z_m", 0.0))
		out["source_span_x_m"] = float(reference.get("source_span_x_m", 0.0))
		out["source_span_z_m"] = float(reference.get("source_span_z_m", 0.0))
		out["source_scene_ratio"] = float(reference.get("source_scene_ratio", 0.0))

	return out

func _configure_current_step() -> String:
	if _producer == null or _pool == null:
		return "missing producer or pool"
	var step := _current_step()
	if not bool(_producer.set_mode_label(str(step.get("mode", "")))):
		return "invalid mode %s" % str(step.get("mode", ""))
	if not bool(_producer.set_preset_label(str(step.get("preset", "")))):
		return "invalid preset %s" % str(step.get("preset", ""))
	return str(_producer.configure(_pool))

func _reconfigure_view() -> void:
	if _view == null or _pool == null or _streamer == null or _rings == null or _runtime == null:
		return
	var relief_scale := float(_runtime.default_relief_scale())
	var relief_ref := float(_runtime.default_relief_ref())
	if _producer != null:
		relief_scale = float(_producer.view_relief_scale(relief_scale))
		relief_ref = float(_producer.view_relief_ref(relief_ref, float(_runtime.default_relief_scale())))
	_runtime.configure_view(_view, _pool, _streamer, _rings, bool(_runtime.default_morph_enabled()), relief_scale, relief_ref)

func _apply_review_camera_frame() -> void:
	if _camera == null:
		return
	var span := 76800.0
	var height_ref := 1700.0
	if _producer != null and _runtime != null:
		height_ref = float(_producer.view_relief_ref(1700.0, float(_runtime.default_relief_scale())))
	var eye := Vector3(0.0, maxf(220.0, span * 0.030 + height_ref * 0.80), span * 0.090)
	_camera.global_position = eye
	_camera.look_at(Vector3.ZERO, Vector3.UP)
	if _camera.has_method("sync_mouse_from_rotation"):
		_camera.call("sync_mouse_from_rotation")

func _refresh_label() -> void:
	if _label == null:
		return
	var step := _current_step()
	var stats := {}
	if _pool != null:
		stats = _pool.call("stats")
	_label.text = "step %d/%d %s | %s\nmode %s preset %s | runtime %s\nresident %d created %d full %d" % [
		_step_index + 1,
		STEPS.size(),
		str(step.get("id", "")),
		str(step.get("status", "")),
		str(_producer.mode_label()) if _producer != null else "missing",
		str(_producer.preset_label()) if _producer != null else "missing",
		str(_pool.call("biome_runtime_mode")) if _pool != null else "missing",
		int(stats.get("resident", 0)),
		int(stats.get("created", 0)),
		int(stats.get("full_events", 0)),
	]

func _static_material_bound_tiles() -> int:
	if _rings == null:
		return 0
	var count := 0
	for child in _rings.get_children():
		if child is MeshInstance3D:
			var mat: Material = child.get_material_override()
			if mat is ShaderMaterial:
				var mix_variant: Variant = mat.get_shader_parameter("static_material_mix")
				var mix_value := float(mix_variant) if typeof(mix_variant) in [TYPE_FLOAT, TYPE_INT] else 0.0
				if mix_value > 0.5:
					count += 1
	return count
