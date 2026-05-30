extends SceneTree

# M4 Facts API gate. Slice 1: Wg10Facts.get_height with NO edits must EXACTLY equal the
# authoritative base Wg10Height.height (the parity-gated formula) — Facts must not alter base
# terrain. (Slices 2-3 extend this file with stamp/bedrock/collision assertions.)

const PACK_RES_DIR := "res://worldgen_terrain/fixtures"
const PACK_FILE := "height_pack.json"
const SEED := 1337

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10Facts") or not ClassDB.class_exists("Wg10Height"):
		push_error("Wg10Facts / Wg10Height not registered"); return 1
	var os_dir: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var facts: Object = ClassDB.instantiate("Wg10Facts")
	var e1: String = str(facts.call("configure", os_dir, PACK_FILE, SEED))
	var h: Object = ClassDB.instantiate("Wg10Height")
	var e2: String = str(h.call("load_pack_dir", os_dir, PACK_FILE))
	if e1 != "" or e2 != "":
		push_error("pack load failed: facts=%s height=%s" % [e1, e2]); return 1

	var errors: Array[String] = []
	var coords := [Vector2(0, 0), Vector2(-1024.5, 2048.25), Vector2(40000.0, 9000.0), Vector2(1e5, -1e5)]
	for c in coords:
		var fv: float = facts.call("get_height", c.x, c.y)
		var bv: float = h.call("height", c.x, c.y, SEED)
		if fv != bv:
			errors.append("no-edit Facts height != base @ %s: %f vs %f" % [str(c), fv, bv])

	# relief_scale: a facts configured with relief_scale=R returns R× the unscaled base height
	var facts_scaled: Object = ClassDB.instantiate("Wg10Facts")
	var rs := 0.25
	if not facts_scaled.has_method("configure_scaled"):
		push_error("[facts] Wg10Facts has no configure_scaled method"); return 1
	var es: String = str(facts_scaled.call("configure_scaled", os_dir, PACK_FILE, SEED, rs))
	if es != "":
		push_error("[facts] configure_scaled failed: %s" % es); return 1
	var max_rel_err := 0.0
	for c in coords:   # reuse the same coords the no-edit parity loop uses
		var h_unscaled: float = facts.call("get_height", c.x, c.y)
		var h_scaled: float = facts_scaled.call("get_height", c.x, c.y)
		max_rel_err = max(max_rel_err, absf(h_scaled - h_unscaled * rs))
	if max_rel_err > 1e-6:
		errors.append("relief_scale mismatch: max|scaled - R*unscaled|=%.9f > 1e-6" % max_rel_err)
	else:
		print("[facts] relief_scale ok (max_err=%.9f at R=%.2f)" % [max_rel_err, rs])

	# stamp: digging a crater lowers get_height at the centre by ~depth; base elsewhere unchanged.
	var probe := Vector2(40000.0, 9000.0)
	var before: float = facts.call("get_height", probe.x, probe.y)
	facts.call("apply_edit", probe.x, probe.y, 200.0, -50.0, 1.0)
	var after: float = facts.call("get_height", probe.x, probe.y)
	if not (after < before - 40.0):
		errors.append("stamp did not dig: before=%f after=%f" % [before, after])
	# a point well outside the stamp is unchanged
	var far_before: float = h.call("height", probe.x + 5000.0, probe.y, SEED)
	var far_after: float = facts.call("get_height", probe.x + 5000.0, probe.y)
	if far_after != far_before:
		errors.append("stamp leaked outside radius: %f vs %f" % [far_after, far_before])
	# bedrock clamp: a huge dig is floored, not bottomless.
	facts.call("set_bedrock", before - 5.0, 1.0e9)
	facts.call("apply_edit", probe.x, probe.y, 200.0, -100000.0, 0.0)
	var clamped: float = facts.call("get_height", probe.x, probe.y)
	if absf(clamped - (before - 5.0)) > 0.5:
		errors.append("bedrock did not clamp: got %f expected ~%f" % [clamped, before - 5.0])
	# clear_edits + unbounded bedrock restores pure base.
	facts.call("clear_edits")
	facts.call("set_bedrock", -1.0e30, 1.0e30)
	var restored: float = facts.call("get_height", probe.x, probe.y)
	if absf(restored - before) > 1e-6:
		errors.append("clear_edits did not restore base: %f vs %f" % [restored, before])

	# collision_field cell (i,j) must equal get_height at that exact world point (no edits active).
	facts.call("clear_edits")
	var n := 5
	var cs := 400.0
	var ccx := 12345.0
	var ccz := -6789.0
	var field: PackedFloat32Array = facts.call("get_collision_field", ccx, ccz, cs, n)
	if field.size() != n * n:
		errors.append("collision_field size %d != %d" % [field.size(), n * n])
	else:
		var corner_x := ccx - cs * 0.5
		var corner_z := ccz - cs * 0.5
		var step := cs / float(n - 1)
		for j in range(n):
			for i in range(n):
				var wx := corner_x + i * step
				var wz := corner_z + j * step
				var pt: float = facts.call("get_height", wx, wz)
				if absf(field[j * n + i] - pt) > 1e-3:
					errors.append("collision cell (%d,%d) %f != point %f" % [i, j, field[j*n+i], pt])
	# bad args -> empty array, not a crash
	var bad: PackedFloat32Array = facts.call("get_collision_field", 0.0, 0.0, 0.0, 1)
	if bad.size() != 0:
		errors.append("bad-arg collision_field not empty: %d" % bad.size())

	if not errors.is_empty():
		for er in errors: push_error(er)
		print("[wg10-facts] status=fail errors=%d" % errors.size()); return 1
	print("[wg10-facts] status=pass coords=%d (no-edit base parity)" % coords.size())
	return 0
