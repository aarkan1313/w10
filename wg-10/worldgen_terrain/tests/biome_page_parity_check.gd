extends SceneTree

# Slice-4b two-tier parity: each GPU biome page vs its committed f64 fixture (the oracle the
# CPU port is proven against). Tier-1 (structural) = the GPU rebuilds the grid from the
# record's exact apron/grid params (a wrong dim/seed -> size mismatch or gross delta).
# Tier-2 = core height within a NORMALIZED-unit epsilon (the fixture is pre-relief units).
# WINDOWED only (local RD null headless -> skip rc 2). The flow contribution is the
# approximated part (spec 4 Tier-2); widen NORM_EPS only with a recorded justification.
#
# Adding a biome is ONE entry in BIOMES below: {fixture, fragment, name}. The machine
# (biome_page.glsl) is loaded once with the primitives; the per-biome FRAGMENT is passed to
# generate_core_page, which concatenates primitives + machine + fragment and dispatches the
# matching schedule_<name>. Mountain stays in the list (it must keep its 1.89e-6 parity).

const PRIM_GLSL := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
# Slice-4b concat-selection: the GENERIC machine + the selected per-biome FRAGMENT. The machine
# is loaded once (with primitives) via load_shaders; the fragment is passed per generate_core_page.
const MACHINE_GLSL := "res://worldgen_terrain/shaders/biome_page.glsl"

# The biome list: each entry is {name, fixture, fragment}. To port a biome, copy its fixture into
# res://worldgen_terrain/fixtures/ and add one row here. KEEP mountain.
const BIOMES := [
	{
		"name": "mountain",
		"fixture": "res://worldgen_terrain/fixtures/recipe_mountain_fixture.json",
		"fragment": "res://worldgen_terrain/shaders/biome_mountain.glsl",
	},
	{
		"name": "grassland",
		"fixture": "res://worldgen_terrain/fixtures/recipe_grassland_fixture.json",
		"fragment": "res://worldgen_terrain/shaders/biome_grassland.glsl",
	},
	{
		"name": "desert",
		"fixture": "res://worldgen_terrain/fixtures/recipe_desert_fixture.json",
		"fragment": "res://worldgen_terrain/shaders/biome_desert.glsl",
	},
	{
		"name": "coast",
		"fixture": "res://worldgen_terrain/fixtures/recipe_coast_fixture.json",
		"fragment": "res://worldgen_terrain/shaders/biome_coast.glsl",
	},
]

# Normalized recipe units (NOT metres): height ~[-0.5,0.5]. MEASURED on RTX 5090/D3D12
# (2026-06-02): the GPU mountain page matches the f64 fixture to overall_maxd = 1.89e-6 over
# both fixture records -- the 128-iter flow relaxation converged essentially exactly to the CPU
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

	var gpu: Object = ClassDB.instantiate("Wg10BiomePageCompute")
	var err: String = str(gpu.call("load_shaders",
		ProjectSettings.globalize_path(PRIM_GLSL),
		ProjectSettings.globalize_path(MACHINE_GLSL)))
	if err != "":
		push_error("[wg10-biome-parity] shader load failed: %s" % err)
		return 1

	for biome in BIOMES:
		var rc: int = _check_biome(gpu, biome)
		if rc != 0:
			return rc
	print("[wg10-biome-parity] status=pass biomes=%d eps=%s" % [BIOMES.size(), str(NORM_EPS)])
	return 0

func _check_biome(gpu: Object, biome: Dictionary) -> int:
	var name: String = str(biome["name"])
	var fixture_path: String = str(biome["fixture"])
	var fragment_path: String = str(biome["fragment"])
	var f := FileAccess.open(fixture_path, FileAccess.READ)
	if f == null:
		push_error("[wg10-biome-parity] biome=%s missing fixture %s" % [name, fixture_path])
		return 1
	var fx: Dictionary = JSON.parse_string(f.get_as_text())
	var records: Array = fx.get("records", [])
	if records.is_empty():
		push_error("[wg10-biome-parity] biome=%s no records in fixture" % name)
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
		# and concatenates the selected biome FRAGMENT onto the machine per call.
		var got: PackedFloat64Array = gpu.call("generate_core_page",
			float(grid["spacing"]), float(grid["ox"]), float(grid["oz"]),
			prows, pcols, apron, int(rec["seed"]), float(rec["feature_span_m"]),
			ProjectSettings.globalize_path(fragment_path))
		if got.size() != core_rows * core_cols:
			push_error("[wg10-biome-parity] biome=%s rec=%d size got=%d exp=%d" % [name, rec_i, got.size(), core_rows * core_cols])
			return 1
		var max_d := 0.0
		var fails := 0
		for i in range(got.size()):
			var d: float = absf(got[i] - float(expected[i]))
			max_d = maxf(max_d, d)
			if d > NORM_EPS:
				fails += 1
				if fails <= 5:
					push_error("[wg10-biome-parity] biome=%s rec=%d core[%d] gpu=%f exp=%f d=%s" % [name, rec_i, i, got[i], expected[i], str(d)])
		if max_d != max_d:
			push_error("[wg10-biome-parity] biome=%s rec=%d NaN delta (degenerate page)" % [name, rec_i])
			return 1
		if fails > 0:
			print("[wg10-biome-parity] status=fail biome=%s rec=%d core=%d fails=%d maxd=%s" % [name, rec_i, got.size(), fails, str(max_d)])
			return 1
		overall_max = maxf(overall_max, max_d)
		rec_i += 1
	print("[wg10-biome-parity] biome=%s records=%d overall_maxd=%s" % [name, records.size(), str(overall_max)])
	return 0
