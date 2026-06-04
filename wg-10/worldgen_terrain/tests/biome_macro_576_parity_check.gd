extends SceneTree

# Diagnostic: runtime producer flow-off page vs the committed 576 macro oracle.
# This isolates origin/span + anchored gaussian parity from flow routing.

const ORACLE := "res://worldgen_terrain/fixtures/mountain_macro_oracle.json"
const PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const FRAGMENT := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const NORM_EPS := 5.0e-4
const FLOW_ITERS := 192

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10BiomePageCompute"):
		push_error("[wg10-macro-576] Wg10BiomePageCompute not registered")
		return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-macro-576] status=skip reason=no-gpu")
		return 2
	probe.free()

	var f := FileAccess.open(ORACLE, FileAccess.READ)
	if f == null:
		push_error("[wg10-macro-576] missing oracle %s" % ORACLE)
		return 1
	var fx: Dictionary = JSON.parse_string(f.get_as_text())
	if fx == null:
		push_error("[wg10-macro-576] oracle parse failed %s" % ORACLE)
		return 1
	var records: Array = fx.get("records", [])
	if records.is_empty():
		push_error("[wg10-macro-576] no records in oracle")
		return 1

	var gpu: Object = ClassDB.instantiate("Wg10BiomePageCompute")
	var err: String = str(gpu.call("load_shaders",
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE)))
	if err != "":
		push_error("[wg10-macro-576] shader load failed: %s" % err)
		return 1

	var rec_i := 0
	for rec in records:
		var rc := _check_record(gpu, rec, rec_i)
		if rc != 0:
			return rc
		rec_i += 1
	print("[wg10-macro-576] status=pass records=%d eps=%s" % [records.size(), str(NORM_EPS)])
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

	var got: PackedFloat64Array = gpu.call("generate_runtime_page_flow",
		float(grid["spacing"]), float(grid["ox"]), float(grid["oz"]),
		prows, pcols, apron, seed, feature_span_m,
		ProjectSettings.globalize_path(FRAGMENT), FLOW_ITERS, false)
	if got.size() != core_n:
		push_error("[wg10-macro-576] rec=%d size got=%d exp=%d" % [rec_i, got.size(), core_n])
		return 1
	var maxd := 0.0
	var at := 0
	for i in range(got.size()):
		var d: float = absf(got[i] - float(expected[i]))
		if d > maxd:
			maxd = d
			at = i
	if maxd != maxd:
		push_error("[wg10-macro-576] rec=%d NaN delta" % rec_i)
		return 1
	print("[wg10-macro-576] rec=%d maxd=%s at=%d eps=%s" % [rec_i, str(maxd), at, str(NORM_EPS)])
	if maxd > NORM_EPS:
		print("[wg10-macro-576] status=fail rec=%d maxd=%s eps=%s" % [rec_i, str(maxd), str(NORM_EPS)])
		return 1
	return 0
