extends SceneTree

# WorldGen10 Task 4a.3 parity gate: GLSL f32 noise/warp primitives vs the f64 oracle.
# WINDOWED ONLY: RenderingDevice compute is null headless on this D3D12 box, so a null
# device -> skip (rc 2), never a false pass.
#
# The whole point of this gate is to PROVE the i64-emulated GLSL lattice hash (uvec2(hi,lo)
# 64-bit wrapping math) reproduces the numpy int64 _hash2 closely enough that the f32
# noise built on it stays within tolerance of the f64 oracle -- including the adversarial
# samples: small NEGATIVE coords (arithmetic-shift sign path) and LARGE coords ~1e6 (int64
# wrapping-multiply path). The fixture (primitive_parity_fixture.json) carries f64 oracle
# `expected` values; we drive the GPU probe per sample and compare within ABS_EPS.
#
# ABS_EPS is the f32-vs-f64 budget for primitives in [-1,1]/[0,1]. It is NOT a knob to hide
# a hash bug: a wrong hash diverges by O(1), not O(1e-4). Warp outputs are world-coord
# offsets (can be ~1e4), so they get a RELATIVE tolerance instead.

const PRIM_GLSL := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const PROBE_GLSL := "res://worldgen_terrain/shaders/primitive_probe.glsl"
const FIXTURE := "res://worldgen_terrain/fixtures/primitive_parity_fixture.json"
const ABS_EPS := 2.0e-4        # f32 budget for primitives in [-1,1]/[0,1]
const WARP_REL_EPS := 1.0e-5   # warp outputs are large world coords -> relative tolerance

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10PrimitiveProbe"):
		push_error("[wg10-primitive-parity] Wg10PrimitiveProbe not registered - run WINDOWED, rebuilt dll")
		return 1
	var dev := RenderingServer.create_local_rendering_device()
	if dev == null:
		print("[wg10-primitive-parity] status=skip reason=no-gpu (headless or no device)")
		return 2  # distinct skip code: runner must NOT treat as pass
	dev.free()

	var fixture: Dictionary = _load_fixture()
	if fixture.is_empty():
		push_error("[wg10-primitive-parity] failed to load/parse fixture %s" % FIXTURE)
		return 1
	var samples: Array = fixture.get("samples", [])
	if samples.is_empty():
		push_error("[wg10-primitive-parity] fixture has no samples")
		return 1

	var probe: Object = ClassDB.instantiate("Wg10PrimitiveProbe")
	var os_prim: String = ProjectSettings.globalize_path(PRIM_GLSL)
	var os_probe: String = ProjectSettings.globalize_path(PROBE_GLSL)
	var err: String = str(probe.call("load_shader", os_prim, os_probe))
	if err != "":
		push_error("[wg10-primitive-parity] shader load failed: %s" % err)
		return 1

	var max_abs := 0.0
	var worst := ""
	var checked := 0
	var failed := 0
	for s in samples:
		var fn_name: String = str(s.get("fn", ""))
		var raw_args: Array = s.get("args", [])
		var expected: float = float(s.get("expected", 0.0))
		var args := PackedFloat64Array()
		for a in raw_args:
			args.push_back(float(a))
		var got: float = float(probe.call("eval", fn_name, args))
		if is_nan(got):
			push_error("[wg10-primitive-parity] eval returned NaN for fn=%s args=%s" % [fn_name, str(raw_args)])
			return 1
		var d: float = abs(got - expected)
		# Warp outputs are large world coords -> use a relative tolerance.
		var is_warp: bool = (fn_name == "warp_x" or fn_name == "warp_z")
		var tol: float = (WARP_REL_EPS * maxf(abs(expected), 1.0)) if is_warp else ABS_EPS
		if d > tol:
			failed += 1
			if d > max_abs:
				max_abs = d
				worst = "%s args=%s expected=%.10f got=%.10f d=%.3e tol=%.3e" % [fn_name, str(raw_args), expected, got, d, tol]
		else:
			if d > max_abs:
				max_abs = d
				worst = "%s d=%.3e (within tol)" % [fn_name, d]
		checked += 1

	if failed > 0:
		print("[wg10-primitive-parity] status=fail checked=%d failed=%d maxd=%.3e worst=[%s]" % [checked, failed, max_abs, worst])
		push_error("[wg10-primitive-parity] FAIL: %d/%d samples out of tolerance; worst=%s" % [failed, checked, worst])
		return 1
	print("[wg10-primitive-parity] status=pass checked=%d failed=0 maxd=%.3e worst=[%s]" % [checked, max_abs, worst])
	return 0

func _load_fixture() -> Dictionary:
	var f := FileAccess.open(FIXTURE, FileAccess.READ)
	if f == null:
		return {}
	var txt := f.get_as_text()
	f.close()
	var parsed = JSON.parse_string(txt)
	if typeof(parsed) != TYPE_DICTIONARY:
		return {}
	return parsed
