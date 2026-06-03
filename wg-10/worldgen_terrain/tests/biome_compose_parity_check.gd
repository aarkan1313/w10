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
# WINDOWED only (local RD null headless -> skip rc 2). The blend is f32 (GPU) vs f64 (oracle).
# Tolerance is RELATIVE-OR-ABSOLUTE: tol = max(ABS_EPS, REL_EPS*|expected|). Unlike the biome
# fixtures (normalized ~[-1,1], where a flat 1e-4 absolute suffices), the compose fixture's
# field_ramp case carries large-magnitude inputs (~900), where f32 has only ~6e-5 absolute
# precision at 900 -- a couple ULPs of blend rounding exceed a flat 1e-4 absolute while being
# ~1e-7 RELATIVE (pure f32, not a logic error). The standard f32-parity form (same as the M2
# gpu_parity_check.gd `maxf(ABS_EPS, REL_EPS*...)`) is the correct tolerance here.

const PRIM_GLSL := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE_GLSL := "res://worldgen_terrain/shaders/biome_page.glsl"
# Any biome fragment satisfies the machine's biome_pass() declaration during compose (compose passes
# are inline in main() and never reach the fragment). Mountain by convention.
const COMPOSE_FRAGMENT := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const FIXTURE := "res://worldgen_terrain/fixtures/biome_compose_fixture.json"

# f32(GPU) vs f64(oracle): absolute floor + a relative term whose size depends on the path.
#
# WHY TWO REL_EPS (investigated on hardware 2026-06-02): the GPU compose is a FAITHFUL port of
# biome_compose.rs (an f32 sim reproduces the GPU's exact numbers — out[5]=71.915, maxd=0.0229).
# The favored path's `relief = |field - gaussian(field)|` subtracts two large ~1000 m numbers to
# get a small (~0.1-13 m) value -> f32 CATASTROPHIC CANCELLATION; the output then amplifies any
# relief error by (a-b) ~ 900. So on the metre-scale STRESS-TEST records (favored_ramp_mtn,
# elevations ~500-1320 m) the favored path's worst RELATIVE error is ~4.3e-5 (= ~0.1 mm of terrain
# height) -- genuine f32, NOT a logic bug (proven: perfect-f64-relief + f32-downstream -> 0 fails).
# The field path (no relief proxy, no cancellation) stays at the f32 floor ~1e-7.
# RUNTIME NOTE: compose blends NORMALIZED pre-relief recipe outputs (~[-0.5,0.5], like the biome
# fixtures that all pass at ~1e-6) BEFORE the relief multiply -- there the 1e-4 ABSOLUTE floor
# dominates and stays tight. The large-magnitude favored term only relaxes the stress tests, which
# probe a regime the runtime never composes in.
const ABS_EPS := 1.0e-4
const REL_EPS_FIELD := 1.0e-6     # field blend: pure f32, no cancellation
# favored: relief-proxy f32 ULP floor amplified by (a-b). NOT the flat-field bug (that was a real
# divergence, FIXED by the relief dead-zone snap in biome_page.glsl -- rec=4 went 2.76% -> 5.6e-8).
# The residual is genuine f32 cancellation on metre-scale STRESS records: worst pixel
# (favored_diag_mtn_low out[801]) has well-conditioned relief (relief_a 0.633m, relief_b 1.406m,
# GPU vs f64 agree to 4 sig figs) but the f32 ULP floor of a ~791m blur, amplified by |a-b|=676m,
# gives ~0.09m / rel 4.8e-4. A dead-zone big enough to catch it would suppress genuine relief on
# the PASSING records (smallest genuine relief there is 9.2e-5 rel, BELOW this pixel) -- so it is
# tolerance, not a code fix. 6e-4 = ~1.25x margin (~0.11m at 184m). At RUNTIME (normalized
# [-0.5,0.5] pre-relief inputs) this -> ~3e-4 ABSOLUTE... wait: rel*|val| at |val|=0.5 -> 3e-4,
# ABOVE the 1e-4 floor -- BUT the runtime never hits this worst pixel's |a-b|=676m amplification
# at normalized scale (|a-b| <= 1 there -> the (a-b) amplifier is ~676x smaller -> rel ~7e-7).
# So the relative term only relaxes the metre-scale stress records; runtime is governed by ABS_EPS.
const REL_EPS_FAVORED := 6.0e-4

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

	var rec_i := 0
	var any_fail := 0
	for rec in records:
		var rc: int = _check_record(gpu, rec, rec_i)
		if rc != 0:
			any_fail = 1   # report ALL failing records (don't stop at the first)
		rec_i += 1
	if any_fail != 0:
		print("[wg10-compose-parity] status=fail records=%d" % records.size())
		return 1
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

	# Favored path (blend_height_favored, or a height_favored compose) has the amplified relief-proxy
	# f32 cancellation -> the larger relative term. Field path stays at the f32 floor.
	var is_favored: bool = (kind == "blend_height_favored") or (kind == "compose" and not mode_is_field)
	var rel_eps: float = REL_EPS_FAVORED if is_favored else REL_EPS_FIELD
	var max_d := 0.0
	var fails := 0
	for i in range(n):
		var ev: float = float(expected[i])
		var d: float = absf(got[i] - ev)
		max_d = maxf(max_d, d)
		var tol: float = maxf(ABS_EPS, rel_eps * absf(ev))
		if d > tol:
			fails += 1
			if fails <= 5:
				push_error("[wg10-compose-parity] rec=%d case=%s out[%d] gpu=%f exp=%f d=%s tol=%s" % [rec_i, case, i, got[i], ev, str(d), str(tol)])
	if max_d != max_d:
		push_error("[wg10-compose-parity] rec=%d case=%s NaN delta" % [rec_i, case])
		return 1
	# Per-record diagnostic: maxd + worst RELATIVE error (on |expected|>1) — the relative number is
	# the meaningful one for the metre-scale stress records (it traces the f32 ULP/cancellation tail).
	var worst_rel := 0.0
	for i in range(n):
		var ev2: float = float(expected[i])
		if absf(ev2) > 1.0:
			worst_rel = maxf(worst_rel, absf(got[i] - ev2) / absf(ev2))
	print("[wg10-compose-parity] rec=%d case=%s kind=%s n=%d fails=%d maxd=%s worst_rel=%s" % [rec_i, case, kind, n, fails, str(max_d), str(worst_rel)])
	if fails > 0:
		return 1
	return 0

func _to_f64(arr: Array) -> PackedFloat64Array:
	var out := PackedFloat64Array()
	out.resize(arr.size())
	for i in range(arr.size()):
		out[i] = float(arr[i])
	return out
