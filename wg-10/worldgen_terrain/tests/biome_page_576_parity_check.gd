extends SceneTree
# Production-scale (256-core/576-padded) cross-oracle parity: the RUNTIME mountain producer vs the
# independent Python f64 EXACT-sweep oracle. Closes audit gap #6 (the 344-padded fixture proved the
# recipe MATH only at fixture scale; cross-engine parity was never proven at production scale).
# Sweeps flow_iters [128,192,256] to separate UNDER-CONVERGENCE from a REAL divergence: the oracle is
# the fully-converged sweep, but the recipe-path STABLE_ITERS=128 under-converges the real 576 page in
# the channel/valley regions (~192 measured). PASS if ANY iter count reaches maxd <= NORM_EPS, and
# print the smallest (converged_at). If even 256 misses 1e-4 -> FAIL (a divergence beyond convergence,
# a bug to investigate). WINDOWED only (local RD null headless -> skip rc 2).
#
# Use str()/%d/%f in prints (this Godot 4.6.2 build does NOT substitute %e/%g). No non-ASCII.
const ORACLE := "res://worldgen_terrain/fixtures/mountain_576_oracle.json"
const PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const FRAGMENT := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const NORM_EPS := 1.0e-4
const ITER_SWEEP := [128, 192, 256]

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10BiomePageCompute"):
		push_error("[wg10-576-parity] Wg10BiomePageCompute not registered")
		return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-576-parity] status=skip reason=no-gpu")
		return 2
	probe.free()

	var f := FileAccess.open(ORACLE, FileAccess.READ)
	if f == null:
		push_error("[wg10-576-parity] missing oracle %s" % ORACLE)
		return 1
	var fx: Dictionary = JSON.parse_string(f.get_as_text())
	if fx == null:
		push_error("[wg10-576-parity] oracle parse failed %s" % ORACLE)
		return 1
	var records: Array = fx.get("records", [])
	if records.is_empty():
		push_error("[wg10-576-parity] no records in oracle")
		return 1

	var gpu: Object = ClassDB.instantiate("Wg10BiomePageCompute")
	var err: String = str(gpu.call("load_shaders",
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE)))
	if err != "":
		push_error("[wg10-576-parity] shader load failed: %s" % err)
		return 1

	var rec_i := 0
	for rec in records:
		var rc: int = _check_record(gpu, rec, rec_i)
		if rc != 0:
			return rc
		rec_i += 1
	print("[wg10-576-parity] status=pass records=%d eps=%s" % [records.size(), str(NORM_EPS)])
	return 0

func _check_record(gpu: Object, rec: Dictionary, rec_i: int) -> int:
	var grid: Dictionary = rec["grid"]
	var prows := int(rec["padded_rows"])
	var pcols := int(rec["padded_cols"])
	var apron := int(rec["apron_px"])
	var core_rows := int(rec["core_rows"])
	var core_cols := int(rec["core_cols"])
	var seed := int(rec["seed"])
	var feature_span_m := float(rec["feature_span_m"])
	var expected: Array = rec["height"]
	var core_n := core_rows * core_cols

	var maxd_at_last := -1.0
	for iters in ITER_SWEEP:
		var got: PackedFloat64Array = gpu.call("generate_runtime_page_576",
			float(grid["spacing"]), float(grid["ox"]), float(grid["oz"]),
			prows, pcols, apron, seed, feature_span_m,
			ProjectSettings.globalize_path(FRAGMENT), int(iters))
		if got.size() != core_n:
			push_error("[wg10-576-parity] rec=%d iters=%d size got=%d exp=%d" % [rec_i, int(iters), got.size(), core_n])
			return 1
		var maxd := 0.0
		for i in range(got.size()):
			var d: float = absf(got[i] - float(expected[i]))
			maxd = maxf(maxd, d)
		if maxd != maxd:
			push_error("[wg10-576-parity] rec=%d iters=%d NaN delta (degenerate page)" % [rec_i, int(iters)])
			return 1
		maxd_at_last = maxd
		print("[wg10-576-parity] rec=%d iters=%d maxd=%s" % [rec_i, int(iters), str(maxd)])
		if maxd <= NORM_EPS:
			print("[wg10-576-parity] rec=%d converged_at=%d maxd=%s" % [rec_i, int(iters), str(maxd)])
			return 0

	# No iter count in the sweep reached NORM_EPS -> a divergence beyond mere under-convergence.
	print("[wg10-576-parity] status=fail rec=%d maxd_at_256=%s eps=%s" % [rec_i, str(maxd_at_last), str(NORM_EPS)])
	return 1
