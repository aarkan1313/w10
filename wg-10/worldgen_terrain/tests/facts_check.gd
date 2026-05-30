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

	if not errors.is_empty():
		for er in errors: push_error(er)
		print("[wg10-facts] status=fail errors=%d" % errors.size()); return 1
	print("[wg10-facts] status=pass coords=%d (no-edit base parity)" % coords.size())
	return 0
