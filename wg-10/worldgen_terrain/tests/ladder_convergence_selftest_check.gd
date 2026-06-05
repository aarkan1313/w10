extends SceneTree

# Proves ladder_convergence.gd is non-vacuous: identical fields -> ~0 delta, a known offset -> that
# delta exactly, flat fields -> vacuous flagged, shape mismatch -> empty. Catches a "helper always
# returns 0" bug before any rung trusts it. Pure GDScript math; headless (no render device).

const HELPER := "res://worldgen_terrain/harness/ladder_convergence.gd"

func _init() -> void:
	quit(_run())

func _run() -> int:
	var h: Object = load(HELPER).new()

	# Case 1: identical fields with real relief -> mean_abs == 0, nonvacuous true.
	var a := PackedFloat32Array()
	for i in range(64):
		a.append(float(i) * 10.0)  # 0..630 m, real relief
	var same: Dictionary = h.delta(a, a)
	if same.is_empty():
		print("[ladder-selftest] status=fail reason=empty-on-identical"); return 1
	if absf(float(same["mean_abs"])) > 1e-6:
		print("[ladder-selftest] status=fail mean_abs=%.8f expected 0" % float(same["mean_abs"])); return 1
	if not bool(same["nonvacuous"]):
		print("[ladder-selftest] status=fail reason=identical-marked-vacuous"); return 1

	# Case 2: constant +5 m offset -> mean_abs == 5, peak_abs == 5.
	var b := PackedFloat32Array()
	for i in range(64):
		b.append(a[i] + 5.0)
	var off: Dictionary = h.delta(a, b)
	if absf(float(off["mean_abs"]) - 5.0) > 1e-5 or absf(float(off["peak_abs"]) - 5.0) > 1e-5:
		print("[ladder-selftest] status=fail mean=%.6f peak=%.6f expected 5/5" % [float(off["mean_abs"]), float(off["peak_abs"])]); return 1

	# Case 3: flat fields -> vacuous flagged.
	var flat := PackedFloat32Array()
	for i in range(64):
		flat.append(3.0)
	var vac: Dictionary = h.delta(flat, flat)
	if bool(vac["nonvacuous"]):
		print("[ladder-selftest] status=fail reason=flat-marked-nonvacuous"); return 1

	# Case 4: shape mismatch -> empty.
	var short := PackedFloat32Array([1.0, 2.0])
	var mismatch: Dictionary = h.delta(a, short)
	if not mismatch.is_empty():
		print("[ladder-selftest] status=fail reason=mismatch-not-empty"); return 1

	print("[ladder-selftest] status=pass cases=4")
	return 0
