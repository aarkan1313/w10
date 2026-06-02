extends SceneTree

# Slice-4a two-tier parity: GPU mountain page vs the committed f64 fixture (the oracle the
# CPU port is proven against). Tier-1 (structural) = the GPU rebuilds the grid from the
# record's exact apron/grid params (a wrong dim/seed -> size mismatch or gross delta).
# Tier-2 = core height within a NORMALIZED-unit epsilon (the fixture is pre-relief units).
# WINDOWED only (local RD null headless -> skip rc 2). The flow contribution is the
# approximated part (spec 4 Tier-2); widen NORM_EPS only with a recorded justification.

const FIXTURE := "res://worldgen_terrain/fixtures/recipe_mountain_fixture.json"
const PRIM_GLSL := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
# Slice-4b concat-selection: the GENERIC machine + the selected per-biome FRAGMENT. The machine
# is loaded once (with primitives) via load_shaders; the fragment is passed per generate_core_page.
const MACHINE_GLSL := "res://worldgen_terrain/shaders/biome_page.glsl"
const MOUNTAIN_GLSL := "res://worldgen_terrain/shaders/biome_mountain.glsl"
# Normalized recipe units (NOT metres): height ~[-0.5,0.5]. MEASURED on RTX 5090/D3D12
# (2026-06-02): the GPU mountain page matches the f64 fixture to overall_maxd = 1.89e-6 over
# both fixture records — the 128-iter flow relaxation converged essentially exactly to the CPU
# sweep and the f32 drift across all 25 passes is negligible (FAR below the feared flow-approx
# floor). Tightened from the initial 1e-2 placeholder to 1e-4 (~50x the achieved drift): tight
# enough that a real regression trips it, with headroom for the other 10 biomes' flow patterns
# in 4b (some carve more aggressively). Widen ONLY with a recorded justification.
const NORM_EPS := 1.0e-4

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10BiomePageCompute"):
		push_error("[wg10-biome-parity] Wg10BiomePageCompute not registered")
		return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-biome-parity] status=skip reason=no-gpu")
		return 2
	probe.free()
	var f := FileAccess.open(FIXTURE, FileAccess.READ)
	if f == null:
		push_error("[wg10-biome-parity] missing fixture %s" % FIXTURE)
		return 1
	var fx: Dictionary = JSON.parse_string(f.get_as_text())
	var records: Array = fx.get("records", [])
	if records.is_empty():
		push_error("[wg10-biome-parity] no records in fixture")
		return 1

	var gpu: Object = ClassDB.instantiate("Wg10BiomePageCompute")
	var err: String = str(gpu.call("load_shaders",
		ProjectSettings.globalize_path(PRIM_GLSL),
		ProjectSettings.globalize_path(MACHINE_GLSL)))
	if err != "":
		push_error("[wg10-biome-parity] shader load failed: %s" % err)
		return 1

	var overall_max := 0.0
	var rec_i := 0
	for rec in records:
		var grid: Dictionary = rec["grid"]
		var prows := int(rec["padded_rows"])
		var pcols := int(rec["padded_cols"])
		var apron := int(rec["apron_px"])
		var core_rows := int(rec["core_rows"])
		var core_cols := int(rec["core_cols"])
		var expected: Array = rec["height"]
		# generate_core_page rebuilds the apron meshgrid from these PADDED dims (Tier-1 echo)
		# and concatenates the selected biome FRAGMENT (mountain) onto the machine per call.
		var got: PackedFloat64Array = gpu.call("generate_core_page",
			float(grid["spacing"]), float(grid["ox"]), float(grid["oz"]),
			prows, pcols, apron, int(rec["seed"]), float(rec["feature_span_m"]),
			ProjectSettings.globalize_path(MOUNTAIN_GLSL))
		if got.size() != core_rows * core_cols:
			push_error("[wg10-biome-parity] rec=%d size got=%d exp=%d" % [rec_i, got.size(), core_rows * core_cols])
			return 1
		var max_d := 0.0
		var fails := 0
		for i in range(got.size()):
			var d: float = absf(got[i] - float(expected[i]))
			max_d = maxf(max_d, d)
			if d > NORM_EPS:
				fails += 1
				if fails <= 5:
					push_error("[wg10-biome-parity] rec=%d core[%d] gpu=%f exp=%f d=%s" % [rec_i, i, got[i], expected[i], str(d)])
		if max_d != max_d:
			push_error("[wg10-biome-parity] rec=%d NaN delta (degenerate page)" % rec_i)
			return 1
		if fails > 0:
			print("[wg10-biome-parity] status=fail rec=%d core=%d fails=%d maxd=%s" % [rec_i, got.size(), fails, str(max_d)])
			return 1
		overall_max = maxf(overall_max, max_d)
		rec_i += 1
	print("[wg10-biome-parity] status=pass biome=mountain records=%d overall_maxd=%s eps=%s" % [records.size(), str(overall_max), str(NORM_EPS)])
	return 0
