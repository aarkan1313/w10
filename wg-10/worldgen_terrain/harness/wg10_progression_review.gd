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
		"status": "implemented",
		"adds": "visible display-window and sampled-source-window facts for every current lane",
		"gate": "review_progression",
		"acceptance_rule": "mapping origin/span/scale is explicit and no scene-local scale constants are duplicated",
		"blocks": "material/pass-network work that depends on knowing which source window is sampled",
		"implemented_by": "source_display_report plus visible progression overlay",
	},
	{
		"id": "material_fact_layers",
		"label": "Material fact layers",
		"status": "implemented",
		"adds": "low-pass/corridor, floor, rock, and snow layers as separately gated facts",
		"gate": "review_progression + review_runtime_visual",
		"acceptance_rule": "each channel is non-vacuous, page-stable, and visually bounded against REFERENCE",
		"blocks": "procedural candidate promotion without accepted material readability",
		"implemented_by": "material_fact_report plus visible progression overlay",
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
var _source_overlay_layer: CanvasLayer
var _source_overlay_panel: Control
var _source_overlay_source_rect: ColorRect
var _source_overlay_display_rect: ColorRect
var _source_overlay_label: Label
var _source_overlay_state := {}
var _material_overlay_layer: CanvasLayer
var _material_overlay_panel: Control
var _material_overlay_label: Label
var _material_overlay_bars: Dictionary = {}
var _material_overlay_state := {}
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
	_create_source_display_overlay()
	_create_material_fact_overlay()
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
	if _pool != null:
		stats = _pool.call("stats")
		contract = _pool.call("mountain_world_layer_contract_report")
		static_reference = _pool.call("static_reference_report")
		mountain_reference = _pool.call("mountain_world_layer_reference_report")
		runtime_mode = str(_pool.call("biome_runtime_mode"))
		biome_path = bool(_pool.call("uses_biome_path"))
		source_transform = _pool.call("biome_source_transform")
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
		"source_display_report": _current_source_display_report(),
		"material_fact_report": _current_material_fact_report(),
		"static_material_bound_tiles": _static_material_bound_tiles(),
		"future_steps": FUTURE_STEPS,
		"progression_manifest": progression_manifest(),
	}

func set_probe_mode(enabled: bool) -> void:
	_probe_mode = enabled
	set_process(not enabled)
	if _label != null:
		_label.visible = not enabled
	if _source_overlay_layer != null:
		_source_overlay_layer.visible = not enabled
	if _material_overlay_layer != null:
		_material_overlay_layer.visible = not enabled

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

func debug_source_display_overlay_state() -> Dictionary:
	_refresh_source_display_overlay()
	return _source_overlay_state

func debug_material_fact_overlay_state() -> Dictionary:
	_refresh_material_fact_overlay()
	return _material_overlay_state

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

func _current_source_display_report() -> Dictionary:
	if _pool == null:
		return {}
	var contract: Dictionary = _pool.call("mountain_world_layer_contract_report")
	var static_reference: Dictionary = _pool.call("static_reference_report")
	var mountain_reference: Dictionary = _pool.call("mountain_world_layer_reference_report")
	var runtime_mode := str(_pool.call("biome_runtime_mode"))
	var source_transform: Dictionary = _pool.call("biome_source_transform")
	return _source_display_report(contract, static_reference, mountain_reference, runtime_mode, source_transform)

func _current_material_fact_report() -> Dictionary:
	if _pool == null:
		return {}
	var contract: Dictionary = _pool.call("mountain_world_layer_contract_report")
	var runtime_mode := str(_pool.call("biome_runtime_mode"))
	var material_source := "missing"
	var global_report := {}
	var page_report := {}
	var channel_report_available := false
	var report_gap := ""

	if runtime_mode == "static_reference":
		material_source = "static_reference_payload"
		global_report = _pool.call("static_reference_report")
		page_report = _pool.call("static_reference_page_report", 0, 0.0, 0.0, 17)
		channel_report_available = true
	elif runtime_mode == "single":
		global_report = _pool.call("mountain_world_layer_reference_report")
		if not global_report.is_empty():
			material_source = "bound_mountain_world_layer_reference"
			page_report = _pool.call("mountain_world_layer_reference_page_report", 0, 0.0, 0.0, 17)
			channel_report_available = true
		else:
			material_source = "live_biome_recipe_missing_material_facts"
			report_gap = "raw live candidate has no accepted material fact layers"
	elif runtime_mode == "world":
		material_source = "world_preview_reference_material_pages"
		report_gap = "WORLD preview binds accepted material pages but has no separate page-report API"

	var has_material_hints := bool(contract.get("has_material_hints", false))
	if not global_report.is_empty():
		has_material_hints = bool(global_report.get("has_material_hints", false))

	var global_channels := {
		"low_pass": float(global_report.get("low_pass_hint_frac", 0.0)),
		"floor": float(global_report.get("floor_hint_frac", 0.0)),
		"rock": float(global_report.get("rock_hint_frac", 0.0)),
		"snow": float(global_report.get("snow_hint_frac", 0.0)),
	}
	var page_channels := {
		"low_pass": float(page_report.get("low_pass_hint_mean", 0.0)),
		"floor": float(page_report.get("floor_hint_mean", 0.0)),
		"rock": float(page_report.get("rock_hint_mean", 0.0)),
		"snow": float(page_report.get("snow_hint_mean", 0.0)),
	}
	var display_channels := page_channels if channel_report_available else global_channels
	return {
		"material_source": material_source,
		"has_material_hints": has_material_hints,
		"channel_report_available": channel_report_available,
		"expected_missing": material_source == "live_biome_recipe_missing_material_facts",
		"report_gap": report_gap,
		"static_material_bound_tiles": _static_material_bound_tiles(),
		"global_channels": global_channels,
		"page_channels": page_channels,
		"display_channels": display_channels,
		"global_total": _channel_total(global_channels),
		"page_total": _channel_total(page_channels),
		"display_total": _channel_total(display_channels),
		"nonzero_channel_count": _nonzero_channel_count(display_channels),
	}

func _channel_total(channels: Dictionary) -> float:
	var total := 0.0
	for key in ["low_pass", "floor", "rock", "snow"]:
		total += maxf(0.0, float(channels.get(key, 0.0)))
	return total

func _nonzero_channel_count(channels: Dictionary) -> int:
	var count := 0
	for key in ["low_pass", "floor", "rock", "snow"]:
		if float(channels.get(key, 0.0)) > 0.0001:
			count += 1
	return count

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

func _create_source_display_overlay() -> void:
	_source_overlay_layer = CanvasLayer.new()
	add_child(_source_overlay_layer)

	_source_overlay_panel = Control.new()
	_source_overlay_panel.anchor_left = 1.0
	_source_overlay_panel.anchor_right = 1.0
	_source_overlay_panel.offset_left = -370.0
	_source_overlay_panel.offset_right = -12.0
	_source_overlay_panel.offset_top = 12.0
	_source_overlay_panel.offset_bottom = 166.0
	_source_overlay_layer.add_child(_source_overlay_panel)

	var bg := ColorRect.new()
	bg.position = Vector2.ZERO
	bg.size = Vector2(358.0, 154.0)
	bg.color = Color(0.02, 0.025, 0.03, 0.72)
	_source_overlay_panel.add_child(bg)

	_source_overlay_source_rect = ColorRect.new()
	_source_overlay_source_rect.color = Color(0.12, 0.38, 0.95, 0.34)
	_source_overlay_panel.add_child(_source_overlay_source_rect)

	_source_overlay_display_rect = ColorRect.new()
	_source_overlay_display_rect.color = Color(1.0, 0.84, 0.16, 0.62)
	_source_overlay_panel.add_child(_source_overlay_display_rect)

	_source_overlay_label = Label.new()
	_source_overlay_label.position = Vector2(14.0, 12.0)
	_source_overlay_label.size = Vector2(330.0, 132.0)
	_source_overlay_label.add_theme_font_size_override("font_size", 13)
	_source_overlay_label.add_theme_color_override("font_color", Color(0.94, 0.96, 1.0))
	_source_overlay_panel.add_child(_source_overlay_label)

func _refresh_source_display_overlay() -> void:
	if _source_overlay_layer == null:
		return
	var report := _current_source_display_report()
	if report.is_empty():
		_source_overlay_layer.visible = false
		_source_overlay_state = {"visible": false}
		return
	if not _probe_mode:
		_source_overlay_layer.visible = true

	var source_span := float(report.get("source_span_x_m", 0.0))
	var display_span := float(report.get("display_span_x_m", 0.0))
	if source_span <= 0.0:
		source_span = maxf(1.0, float(report.get("source_scale", 1.0)))
	if display_span <= 0.0:
		display_span = 1.0
	var ratio := clampf(display_span / maxf(source_span, 1.0), 0.08, 1.0)

	var source_rect := Rect2(18.0, 74.0, 216.0, 58.0)
	var display_w := source_rect.size.x * ratio
	var display_rect := Rect2(
		source_rect.position.x + (source_rect.size.x - display_w) * 0.5,
		source_rect.position.y + 18.0,
		display_w,
		22.0
	)
	_source_overlay_source_rect.position = source_rect.position
	_source_overlay_source_rect.size = source_rect.size
	_source_overlay_display_rect.position = display_rect.position
	_source_overlay_display_rect.size = display_rect.size

	var kind := str(report.get("mapping_kind", "missing"))
	var step := _current_step()
	var ratio_text := "%.3f" % float(report.get("source_scene_ratio", float(report.get("source_scale", 1.0))))
	_source_overlay_label.text = "source/display | %s\nstep %s\nsource %.0fm  display %.0fm  x%s\nsource = display * %.3f + (%.0f, %.0f)" % [
		kind,
		str(step.get("id", "")),
		source_span,
		display_span,
		ratio_text,
		float(report.get("source_scale", 1.0)),
		float(report.get("source_offset_x_m", 0.0)),
		float(report.get("source_offset_z_m", 0.0)),
	]

	_source_overlay_state = {
		"visible": _source_overlay_layer.visible,
		"mapping_kind": kind,
		"step_id": str(step.get("id", "")),
		"source_span_x_m": source_span,
		"display_span_x_m": display_span,
		"ratio": ratio,
		"source_rect": _rect_report(source_rect),
		"display_rect": _rect_report(display_rect),
		"label": _source_overlay_label.text,
	}

func _rect_report(rect: Rect2) -> Dictionary:
	return {
		"x": rect.position.x,
		"y": rect.position.y,
		"w": rect.size.x,
		"h": rect.size.y,
	}

func _create_material_fact_overlay() -> void:
	_material_overlay_layer = CanvasLayer.new()
	add_child(_material_overlay_layer)

	_material_overlay_panel = Control.new()
	_material_overlay_panel.anchor_left = 1.0
	_material_overlay_panel.anchor_right = 1.0
	_material_overlay_panel.offset_left = -370.0
	_material_overlay_panel.offset_right = -12.0
	_material_overlay_panel.offset_top = 178.0
	_material_overlay_panel.offset_bottom = 352.0
	_material_overlay_layer.add_child(_material_overlay_panel)

	var bg := ColorRect.new()
	bg.position = Vector2.ZERO
	bg.size = Vector2(358.0, 174.0)
	bg.color = Color(0.02, 0.025, 0.03, 0.72)
	_material_overlay_panel.add_child(bg)

	_material_overlay_label = Label.new()
	_material_overlay_label.position = Vector2(14.0, 10.0)
	_material_overlay_label.size = Vector2(330.0, 52.0)
	_material_overlay_label.add_theme_font_size_override("font_size", 13)
	_material_overlay_label.add_theme_color_override("font_color", Color(0.94, 0.96, 1.0))
	_material_overlay_panel.add_child(_material_overlay_label)

	var specs := [
		{"id": "low_pass", "label": "low/corr", "color": Color(0.12, 0.55, 0.48, 0.82), "y": 70.0},
		{"id": "floor", "label": "floor", "color": Color(0.38, 0.70, 0.30, 0.82), "y": 94.0},
		{"id": "rock", "label": "rock", "color": Color(0.58, 0.55, 0.50, 0.82), "y": 118.0},
		{"id": "snow", "label": "snow", "color": Color(0.86, 0.88, 0.82, 0.86), "y": 142.0},
	]
	for spec in specs:
		var row_label := Label.new()
		row_label.position = Vector2(14.0, float(spec["y"]) - 4.0)
		row_label.size = Vector2(72.0, 18.0)
		row_label.text = str(spec["label"])
		row_label.add_theme_font_size_override("font_size", 12)
		row_label.add_theme_color_override("font_color", Color(0.92, 0.94, 0.96))
		_material_overlay_panel.add_child(row_label)

		var track := ColorRect.new()
		track.position = Vector2(88.0, float(spec["y"]))
		track.size = Vector2(224.0, 12.0)
		track.color = Color(1.0, 1.0, 1.0, 0.12)
		_material_overlay_panel.add_child(track)

		var bar := ColorRect.new()
		bar.position = track.position
		bar.size = Vector2(1.0, 12.0)
		bar.color = spec["color"]
		_material_overlay_panel.add_child(bar)
		_material_overlay_bars[str(spec["id"])] = bar

func _refresh_material_fact_overlay() -> void:
	if _material_overlay_layer == null:
		return
	var report := _current_material_fact_report()
	if report.is_empty():
		_material_overlay_layer.visible = false
		_material_overlay_state = {"visible": false}
		return
	if not _probe_mode:
		_material_overlay_layer.visible = true

	var channels: Dictionary = report.get("display_channels", {})
	var total := maxf(float(report.get("display_total", 0.0)), 1.0)
	for key in ["low_pass", "floor", "rock", "snow"]:
		var bar := _material_overlay_bars.get(key, null) as ColorRect
		if bar == null:
			continue
		var value := clampf(float(channels.get(key, 0.0)) / total, 0.0, 1.0)
		bar.size.x = maxf(1.0, 224.0 * value)

	var source := str(report.get("material_source", "missing"))
	var step := _current_step()
	var gap := str(report.get("report_gap", ""))
	var hint_state := "facts yes" if bool(report.get("has_material_hints", false)) else "facts missing"
	_material_overlay_label.text = "material facts | %s\nstep %s | %s%s" % [
		source,
		str(step.get("id", "")),
		hint_state,
		" | " + gap if gap != "" else "",
	]
	_material_overlay_state = {
		"visible": _material_overlay_layer.visible,
		"step_id": str(step.get("id", "")),
		"material_source": source,
		"has_material_hints": bool(report.get("has_material_hints", false)),
		"channel_report_available": bool(report.get("channel_report_available", false)),
		"expected_missing": bool(report.get("expected_missing", false)),
		"report_gap": gap,
		"nonzero_channel_count": int(report.get("nonzero_channel_count", 0)),
		"display_total": float(report.get("display_total", 0.0)),
		"static_material_bound_tiles": int(report.get("static_material_bound_tiles", 0)),
		"bars": _material_bar_report(),
		"label": _material_overlay_label.text,
	}

func _material_bar_report() -> Dictionary:
	var out := {}
	for key in ["low_pass", "floor", "rock", "snow"]:
		var bar := _material_overlay_bars.get(key, null) as ColorRect
		if bar != null:
			out[key] = _rect_report(Rect2(bar.position, bar.size))
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
	_refresh_source_display_overlay()
	_refresh_material_fact_overlay()

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
