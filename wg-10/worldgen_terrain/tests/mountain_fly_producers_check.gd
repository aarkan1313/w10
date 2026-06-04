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

	_expect(str(producers.mode_label()) == "MOUNTAIN", "default mode should be MOUNTAIN", errs)
	_expect(str(producers.preset_label()) == "network_ref", "default preset should be network_ref", errs)
	_expect(absf(float(producers.feature_span_m()) - 90000.0) < 0.001, "network span should be 90000m", errs)
	_expect(absf(float(producers.relief_m()) - 1000.0) < 0.001, "default relief should be 1000m", errs)
	_expect(not bool(producers.is_world()), "default should not be WORLD", errs)
	_expect(not bool(producers.is_legacy()), "default should not be LEGACY", errs)

	producers.toggle_preset()
	_expect(str(producers.preset_label()) == "close_debug", "toggle preset should enter close_debug", errs)
	_expect(absf(float(producers.feature_span_m()) - 3500.0) < 0.001, "close_debug span should be 3500m", errs)
	producers.toggle_preset()
	_expect(str(producers.preset_label()) == "network_ref", "second toggle should return to network_ref", errs)

	producers.set_relief_m(1.0)
	_expect(absf(float(producers.relief_m()) - 50.0) < 0.001, "relief lower clamp should be 50m", errs)
	producers.set_relief_m(30000.0)
	_expect(absf(float(producers.relief_m()) - 20000.0) < 0.001, "relief upper clamp should be 20000m", errs)

	producers.cycle_mode()
	_expect(str(producers.mode_label()) == "LEGACY", "MOUNTAIN should cycle to LEGACY", errs)
	_expect(bool(producers.is_legacy()), "LEGACY mode should report is_legacy", errs)
	producers.cycle_mode()
	_expect(str(producers.mode_label()) == "WORLD", "LEGACY should cycle to WORLD", errs)
	_expect(bool(producers.is_world()), "WORLD mode should report is_world", errs)
	producers.cycle_mode()
	_expect(str(producers.mode_label()) == "MOUNTAIN", "WORLD should cycle to MOUNTAIN", errs)

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
