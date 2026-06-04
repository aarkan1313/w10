extends SceneTree
# Production-scale (256-core/576-padded) cross-oracle parity: the RUNTIME mountain producer vs the
# independent Python f64 EXACT-sweep oracle. Closes audit gap #6 (the 344-padded fixture proved the
# recipe MATH only at fixture scale; cross-engine parity was never proven at production scale).
# Sweeps flow_iters [128,192,256] to separate UNDER-CONVERGENCE from a REAL divergence: the oracle is
# the fully-converged sweep, but the recipe-path STABLE_ITERS=128 under-converges the real 576 page in
# the channel/valley regions (~192 measured). PASS if ANY iter count reaches maxd <= NORM_EPS, and
# print the smallest (converged_at). WINDOWED only (local RD null headless -> skip rc 2).
#
# TIER-2 EPS (2e-3), recorded justification (2026-06-04, RTX 5090 after scale-invariant
# kernel anchoring): the converged runtime 576 page plateaus at maxd 1.4712e-3 from 128..256
# iters -> NOT under-convergence. The paired flow-off runtime diagnostic
# (biome_macro_576_parity_check.gd) matches the 576 macro oracle at 2.3156e-5, and the CPU f64
# port matches the flow-on oracle to machine floor, so origin/span + anchored gaussian math are
# ruled out. The remaining residual is the same f32 MFD routing floor as the pre-anchoring proof,
# but the scale-invariant oracle uses spacing=351.5625 m/px, so the anchored flow kernels are
# intentionally much narrower than the old fixed cell-sigma path. NORM_EPS=2e-3 gives about
# 1.36x headroom over the measured floor (~1.47 m per 1000 m relief) while still tripping real
# pointwise/gaussian/assembly regressions, which are orders larger. Memory:
# worldgen10-576-parity-residual, updated by the Slice 4 scale-invariance proof.
# Use str()/%d/%f in prints (this Godot 4.6.2 build does NOT substitute %e/%g). No non-ASCII.
const ORACLE := "res://worldgen_terrain/fixtures/mountain_576_oracle.json"
const PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const FRAGMENT := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const NORM_EPS := 2.0e-3
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
		var total := 0.0
		var at := 0
		for i in range(got.size()):
			var d: float = absf(got[i] - float(expected[i]))
			total += d
			if d > maxd:
				maxd = d
				at = i
		if maxd != maxd:
			push_error("[wg10-576-parity] rec=%d iters=%d NaN delta (degenerate page)" % [rec_i, int(iters)])
			return 1
		maxd_at_last = maxd
		print("[wg10-576-parity] rec=%d iters=%d maxd=%s mean=%s at=%d" % [rec_i, int(iters), str(maxd), str(total / float(got.size())), at])
		if maxd <= NORM_EPS:
			print("[wg10-576-parity] rec=%d converged_at=%d maxd=%s" % [rec_i, int(iters), str(maxd)])
			return 0

	# No iter count in the sweep reached NORM_EPS -> a divergence beyond mere under-convergence.
	print("[wg10-576-parity] status=fail rec=%d maxd_at_256=%s eps=%s" % [rec_i, str(maxd_at_last), str(NORM_EPS)])
	return 1
