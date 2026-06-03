extends SceneTree

# Slice-4b.11 COMPOSE parity: the GPU compose layer (blend_field / blend_height_favored /
# compose_biomes fold) vs the committed f64 fixture (biome_compose_fixture.json), the oracle the
# CPU port (wg-10/rust/src/biome_compose.rs) is itself proven against. Input + weight fields are
# stored DIRECTLY in the fixture (NO recipe noise / grammar), so this gate proves the COMPOSE MATH
# in isolation -- independent of every biome recipe and the grammar weight field.
#
# Record kinds -> GPU entry:
#   blend_field          -> blend_pair(a, b, w_a, mode_is_field=true)   (_blend_field oracle)
#   blend_height_favored -> blend_pair(a, b, w_a, mode_is_field=false)  (_blend_height_favored)
#   compose              -> compose_fields(fields, weights, n, mode_is_field=(cfg.mode=="field"))
#
# The compose passes are ADDITIVE generic passes in the machine (biome_page.glsl, codes 60..66),
# handled inline in main() so they never reach a biome fragment. The compose entry concatenates the
# MOUNTAIN fragment purely to satisfy biome_pass()'s declaration (it is never executed).
#
# WINDOWED only (local RD null headless -> skip rc 2). The blend is f32 (GPU) vs f64 (oracle) over
# fields ~[-1,1]; ABS_EPS mirrors the biome NORM_EPS (1e-4). Widen only with a recorded justification.

const PRIM_GLSL := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE_GLSL := "res://worldgen_terrain/shaders/biome_page.glsl"
# Any biome fragment satisfies the machine's biome_pass() declaration during compose (compose passes
# are inline in main() and never reach the fragment). Mountain by convention.
const COMPOSE_FRAGMENT := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const FIXTURE := "res://worldgen_terrain/fixtures/biome_compose_fixture.json"

# f32(GPU) vs f64(oracle) over fields ~[-1,1]. Same budget as the biome NORM_EPS.
const ABS_EPS := 1.0e-4

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10BiomePageCompute"):
		push_error("[wg10-compose-parity] Wg10BiomePageCompute not registered")
		return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-compose-parity] status=skip reason=no-gpu")
		return 2
	probe.free()

	var gpu: Object = ClassDB.instantiate("Wg10BiomePageCompute")
	var err: String = str(gpu.call("load_shaders",
		ProjectSettings.globalize_path(PRIM_GLSL),
		ProjectSettings.globalize_path(MACHINE_GLSL)))
	if err != "":
		push_error("[wg10-compose-parity] shader load failed: %s" % err)
		return 1
	var ferr: String = str(gpu.call("load_compose_fragment",
		ProjectSettings.globalize_path(COMPOSE_FRAGMENT)))
	if ferr != "":
		push_error("[wg10-compose-parity] compose fragment load failed: %s" % ferr)
		return 1

	var f := FileAccess.open(FIXTURE, FileAccess.READ)
	if f == null:
		push_error("[wg10-compose-parity] missing fixture %s" % FIXTURE)
		return 1
	var fx: Dictionary = JSON.parse_string(f.get_as_text())
	var records: Array = fx.get("records", [])
	if records.is_empty():
		push_error("[wg10-compose-parity] no records in fixture")
		return 1

	var overall_max := 0.0
	var rec_i := 0
	for rec in records:
		var rc: int = _check_record(gpu, rec, rec_i)
		if rc != 0:
			return rc
		rec_i += 1
	print("[wg10-compose-parity] status=pass records=%d eps=%s" % [records.size(), str(ABS_EPS)])
	return 0

func _check_record(gpu: Object, rec: Dictionary, rec_i: int) -> int:
	var kind: String = str(rec["kind"])
	var case: String = str(rec.get("case", "?"))
	var rows := int(rec["rows"])
	var cols := int(rec["cols"])
	var n := rows * cols
	var cfg: Dictionary = rec["cfg"]
	var mode_is_field: bool = (str(cfg["mode"]) == "field")
	var favor: float = float(cfg["favor_strength"])
	var floor_v: float = float(cfg["relief_confidence_floor"])
	var expected: Array = rec["expected"]
	if expected.size() != n:
		push_error("[wg10-compose-parity] rec=%d case=%s expected size %d != %d" % [rec_i, case, expected.size(), n])
		return 1

	var got: PackedFloat64Array
	if kind == "blend_field" or kind == "blend_height_favored":
		# blend_pair: w_a used DIRECTLY (NOT the accumulator fold). mode_is_field selects the path.
		var a := _to_f64(rec["a"])
		var b := _to_f64(rec["b"])
		var w_a := _to_f64(rec["w_a"])
		got = gpu.call("blend_pair", a, b, w_a, rows, cols, mode_is_field, favor, floor_v)
	elif kind == "compose":
		# compose_fields: flatten the N fields + weights row-major-concatenated.
		var fields_arr: Array = rec["fields"]
		var weights_arr: Array = rec["weights"]
		var nf := fields_arr.size()
		if weights_arr.size() != nf:
			push_error("[wg10-compose-parity] rec=%d case=%s fields/weights count mismatch %d vs %d" % [rec_i, case, nf, weights_arr.size()])
			return 1
		var fields_flat := PackedFloat64Array()
		var weights_flat := PackedFloat64Array()
		fields_flat.resize(nf * n)
		weights_flat.resize(nf * n)
		var fs := fields_flat
		var ws := weights_flat
		for k in range(nf):
			var fk: Array = fields_arr[k]
			var wk: Array = weights_arr[k]
			if fk.size() != n or wk.size() != n:
				push_error("[wg10-compose-parity] rec=%d case=%s field/weight %d size != %d" % [rec_i, case, k, n])
				return 1
			for i in range(n):
				fs[k * n + i] = float(fk[i])
				ws[k * n + i] = float(wk[i])
		got = gpu.call("compose_fields", fs, ws, nf, rows, cols, mode_is_field, favor, floor_v)
	else:
		push_error("[wg10-compose-parity] rec=%d case=%s unknown kind '%s'" % [rec_i, case, kind])
		return 1

	if got.size() != n:
		push_error("[wg10-compose-parity] rec=%d case=%s size got=%d exp=%d" % [rec_i, case, got.size(), n])
		return 1

	var max_d := 0.0
	var fails := 0
	for i in range(n):
		var d: float = absf(got[i] - float(expected[i]))
		max_d = maxf(max_d, d)
		if d > ABS_EPS:
			fails += 1
			if fails <= 5:
				push_error("[wg10-compose-parity] rec=%d case=%s out[%d] gpu=%f exp=%f d=%s" % [rec_i, case, i, got[i], expected[i], str(d)])
	if max_d != max_d:
		push_error("[wg10-compose-parity] rec=%d case=%s NaN delta" % [rec_i, case])
		return 1
	if fails > 0:
		print("[wg10-compose-parity] status=fail rec=%d case=%s kind=%s n=%d fails=%d maxd=%s" % [rec_i, case, kind, n, fails, str(max_d)])
		return 1
	print("[wg10-compose-parity] rec=%d case=%s kind=%s n=%d maxd=%s" % [rec_i, case, kind, n, str(max_d)])
	return 0

func _to_f64(arr: Array) -> PackedFloat64Array:
	var out := PackedFloat64Array()
	out.resize(arr.size())
	for i in range(arr.size()):
		out[i] = float(arr[i])
	return out
