extends SceneTree

const RUNTIME_CONFIG := "res://worldgen_terrain/harness/mountain_fly_runtime_config.gd"

func _init() -> void:
	quit(_run())

func _run() -> int:
	var cfg: Object = load(RUNTIME_CONFIG).new()
	var errs: Array[String] = []

	_expect(int(cfg.num_levels()) == 5, "num_levels should be 5", errs)
	_expect(absf(float(cfg.base_span_m()) - 8192.0) < 0.001, "base_span_m should be 8192", errs)
	_expect(int(cfg.grid_res()) == 128, "grid_res should be 128", errs)
	_expect(absf(float(cfg.lead_seconds()) - 0.5) < 0.001, "lead_seconds should be 0.5", errs)
	_expect(absf(float(cfg.detail_amp_m()) - 350.0) < 0.001, "detail_amp_m should be 350", errs)
	_expect(absf(float(cfg.default_relief_scale()) - 0.25) < 0.001, "default relief scale should be 0.25", errs)
	_expect(not bool(cfg.default_morph_enabled()), "default morph should be off", errs)
	_expect(not bool(cfg.default_detail_enabled()), "default detail should be off", errs)
	_expect(absf(float(cfg.morph_region(false)) - 0.0) < 0.001, "morph off region should be 0", errs)
	_expect(absf(float(cfg.morph_region(true)) - 0.15) < 0.001, "morph on region should be 0.15", errs)
	_expect(absf(float(cfg.loaded_edge_m()) - 196608.0) < 0.001, "loaded edge should be 196608m", errs)
	_expect(cfg.sky_color() == Color(0.45, 0.62, 0.85), "sky color should match review scene", errs)

	if not errs.is_empty():
		for err in errs:
			push_error(err)
		print("[wg10-mountain-fly-runtime-config] status=fail errors=%d" % errs.size())
		return 1

	print("[wg10-mountain-fly-runtime-config] status=pass levels=%d loaded_edge=%.0f detail=%.0f" % [
		int(cfg.num_levels()),
		float(cfg.loaded_edge_m()),
		float(cfg.detail_amp_m()),
	])
	return 0

func _expect(condition: bool, message: String, errs: Array[String]) -> void:
	if not condition:
		errs.append(message)
