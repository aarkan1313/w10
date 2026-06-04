extends SceneTree

const PRODUCERS := "res://worldgen_terrain/harness/mountain_fly_producers.gd"

func _init() -> void:
	quit(await _run())

func _run() -> int:
	var script := load(PRODUCERS)
	if script == null:
		push_error("[wg10-mountain-fly-producers] cannot load %s" % PRODUCERS)
		return 1

	var producers: Object = script.new()
	var errs: Array[String] = []

	_expect(str(producers.mode_label()) == "REFERENCE", "default mode should be REFERENCE", errs)
	_expect(str(producers.preset_label()) == "network_ref", "default preset should be network_ref", errs)
	_expect(int(producers.runtime_seed()) == 177, "default reference seed should match accepted mountain-network seed", errs)
	_expect(absf(float(producers.feature_span_m()) - 90000.0) < 0.001, "network span should be 90000m", errs)
	_expect(absf(float(producers.relief_m()) - 1700.0) < 0.001, "default relief should be 1700m", errs)
	_expect(absf(float(producers.view_relief_scale(0.25)) - 1.0) < 0.001, "reference view relief scale should be 1", errs)
	_expect(absf(float(producers.view_relief_ref(1700.0, 0.25)) - 1700.0) < 0.001, "reference relief ref should match displayed relief", errs)
	_expect(absf(float(producers.source_scale()) - 1.0) < 0.001, "reference source scale should be identity", errs)
	_expect(not bool(producers.is_world()), "default should not be WORLD", errs)
	_expect(not bool(producers.is_legacy()), "default should not be LEGACY", errs)
	_expect(bool(producers.is_reference()), "default should be REFERENCE", errs)
	_expect_taxonomy(producers, "accepted_reference_baseline", "accepted_visual_baseline", "static mountain-network", errs)

	_expect(bool(producers.set_mode_label("MOUNTAIN")), "set_mode_label MOUNTAIN should succeed", errs)
	_expect(str(producers.mode_label()) == "MOUNTAIN", "set_mode_label should enter MOUNTAIN", errs)
	_expect(absf(float(producers.view_relief_scale(0.25)) - 1.0) < 0.001, "mountain network view relief scale should match reference-backed height bridge", errs)
	_expect(absf(float(producers.view_relief_ref(1700.0, 0.25)) - 1700.0) < 0.001, "mountain network relief ref should follow reference-backed height", errs)
	_expect(absf(float(producers.source_scale()) - 3.515625) < 0.000001, "mountain network source scale should match accepted source/display ratio", errs)
	_expect(absf(float(producers.source_offset_x_m()) - 207000.0) < 0.001, "mountain network source x offset should match accepted source center", errs)
	_expect(absf(float(producers.source_offset_z_m()) - 176000.0) < 0.001, "mountain network source z offset should match accepted source center", errs)
	_expect(not bool(producers.is_reference()), "MOUNTAIN should not report is_reference", errs)
	_expect_taxonomy(producers, "reference_backed_mountain_bridge", "accepted_visual_bridge_not_final_procedural", "reference-backed", errs)

	producers.toggle_preset()
	_expect(str(producers.preset_label()) == "close_debug", "toggle preset should enter close_debug", errs)
	_expect(absf(float(producers.feature_span_m()) - 3500.0) < 0.001, "close_debug span should be 3500m", errs)
	_expect(absf(float(producers.view_relief_scale(0.25)) - 0.25) < 0.001, "close_debug view relief scale should pass through", errs)
	_expect(absf(float(producers.view_relief_ref(1700.0, 0.25)) - 425.0) < 0.001, "close_debug relief ref should follow displayed relief", errs)
	_expect(absf(float(producers.source_scale()) - 1.0) < 0.001, "close_debug source scale should be identity", errs)
	_expect_taxonomy(producers, "live_mountain_recipe_debug", "prototype_not_accepted", "raw live", errs)
	producers.toggle_preset()
	_expect(str(producers.preset_label()) == "network_ref", "second toggle should return to network_ref", errs)
	_expect(bool(producers.set_preset_label("close_debug")), "set_preset_label close_debug should succeed", errs)
	_expect(str(producers.preset_label()) == "close_debug", "set_preset_label should enter close_debug", errs)
	_expect(bool(producers.set_preset_label("network_ref")), "set_preset_label network_ref should succeed", errs)
	_expect(str(producers.preset_label()) == "network_ref", "set_preset_label should enter network_ref", errs)
	_expect(not bool(producers.set_preset_label("bad")), "invalid preset label should fail", errs)
	_expect(str(producers.preset_label()) == "network_ref", "invalid preset label should leave preset unchanged", errs)

	producers.set_relief_m(1.0)
	_expect(absf(float(producers.relief_m()) - 50.0) < 0.001, "relief lower clamp should be 50m", errs)
	producers.set_relief_m(30000.0)
	_expect(absf(float(producers.relief_m()) - 20000.0) < 0.001, "relief upper clamp should be 20000m", errs)

	producers.cycle_mode()
	_expect(str(producers.mode_label()) == "REFERENCE", "MOUNTAIN should cycle to REFERENCE", errs)
	_expect(bool(producers.is_reference()), "REFERENCE mode should report is_reference", errs)
	_expect(int(producers.runtime_seed()) == 177, "REFERENCE seed should match accepted mountain-network seed", errs)
	_expect(absf(float(producers.view_relief_scale(0.25)) - 1.0) < 0.001, "REFERENCE view relief scale should be 1", errs)
	_expect(absf(float(producers.view_relief_ref(1700.0, 0.25)) - 20000.0) < 0.001, "REFERENCE relief ref should follow clamped relief", errs)
	_expect(absf(float(producers.source_scale()) - 1.0) < 0.001, "REFERENCE source scale should be identity", errs)
	_expect_taxonomy(producers, "accepted_reference_baseline", "accepted_visual_baseline", "static mountain-network", errs)
	producers.cycle_mode()
	_expect(str(producers.mode_label()) == "MOUNTAIN", "REFERENCE should cycle to MOUNTAIN", errs)
	_expect(not bool(producers.is_reference()), "MOUNTAIN should not report is_reference after owner cycle", errs)
	_expect(int(producers.runtime_seed()) == 177, "MOUNTAIN should stay on accepted mountain-network seed", errs)
	_expect(absf(float(producers.view_relief_ref(1700.0, 0.25)) - 20000.0) < 0.001, "MOUNTAIN relief ref should follow clamped relief", errs)
	_expect(absf(float(producers.source_scale()) - 3.515625) < 0.000001, "MOUNTAIN source scale should stay on network reference scale", errs)
	_expect_taxonomy(producers, "reference_backed_mountain_bridge", "accepted_visual_bridge_not_final_procedural", "reference-backed", errs)
	_expect(bool(producers.set_mode_label("WORLD")), "set_mode_label WORLD should succeed", errs)
	_expect(str(producers.mode_label()) == "WORLD", "set_mode_label should enter WORLD", errs)
	_expect(bool(producers.is_world()), "WORLD mode should report is_world", errs)
	_expect(int(producers.runtime_seed()) == 1337, "WORLD seed should use world seed", errs)
	_expect(absf(float(producers.view_relief_ref(1700.0, 0.25)) - 5000.0) < 0.001, "WORLD relief ref should follow clamped relief and default scale", errs)
	_expect(int(producers.world_active_biome_limit()) == 1, "WORLD review should stay capped until compose is off the fly stream", errs)
	_expect_taxonomy(producers, "world_composition_diagnostic", "diagnostic_not_owner_accepted", "one-biome-per-page", errs)
	producers.cycle_mode()
	_expect(str(producers.mode_label()) == "REFERENCE", "WORLD diagnostic should return to REFERENCE on owner cycle", errs)
	producers.cycle_mode()
	_expect(str(producers.mode_label()) == "MOUNTAIN", "REFERENCE should cycle back to MOUNTAIN after leaving WORLD", errs)
	_expect(bool(producers.set_mode_label("LEGACY")), "set_mode_label LEGACY should succeed", errs)
	_expect(str(producers.mode_label()) == "LEGACY", "set_mode_label should enter LEGACY", errs)
	_expect(bool(producers.is_legacy()), "LEGACY mode should report is_legacy", errs)
	_expect(int(producers.runtime_seed()) == 1337, "LEGACY seed should use world seed", errs)
	_expect_taxonomy(producers, "legacy_atlas_regression", "legacy_regression_not_accepted", "legacy atlas", errs)
	producers.cycle_mode()
	_expect(str(producers.mode_label()) == "REFERENCE", "LEGACY diagnostic should return to REFERENCE on owner cycle", errs)
	_expect(bool(producers.set_mode_label("WORLD")), "set_mode_label WORLD should still succeed", errs)
	_expect(str(producers.mode_label()) == "WORLD", "set_mode_label should enter WORLD directly", errs)
	_expect(bool(producers.set_mode_label("REFERENCE")), "set_mode_label REFERENCE should succeed", errs)
	_expect(str(producers.mode_label()) == "REFERENCE", "set_mode_label should enter REFERENCE", errs)
	_expect(bool(producers.set_mode_label("MOUNTAIN")), "set_mode_label MOUNTAIN should succeed", errs)
	_expect(str(producers.mode_label()) == "MOUNTAIN", "set_mode_label should enter MOUNTAIN", errs)
	_expect(not bool(producers.set_mode_label("BAD")), "invalid mode label should fail", errs)
	_expect(str(producers.mode_label()) == "MOUNTAIN", "invalid mode label should leave mode unchanged", errs)

	if not errs.is_empty():
		for err in errs:
			push_error(err)
		print("[wg10-mountain-fly-producers] status=fail errors=%d" % errs.size())
		return 1

	print("[wg10-mountain-fly-producers] status=pass")
	return 0

func _expect(condition: bool, message: String, errs: Array[String]) -> void:
	if not condition:
		errs.append(message)

func _expect_taxonomy(
	producers: Object,
	expected_role: String,
	expected_acceptance: String,
	expected_note_fragment: String,
	errs: Array[String],
) -> void:
	_expect(str(producers.mode_role()) == expected_role, "expected mode role %s, got %s" % [expected_role, str(producers.mode_role())], errs)
	_expect(str(producers.mode_acceptance()) == expected_acceptance, "expected mode acceptance %s, got %s" % [expected_acceptance, str(producers.mode_acceptance())], errs)
	_expect(str(producers.mode_note()).contains(expected_note_fragment), "expected mode note to contain %s, got %s" % [expected_note_fragment, str(producers.mode_note())], errs)
