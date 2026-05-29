extends SceneTree

# Cross-boundary height properties through the native Wg10Height lib.
# Property-based (design spec §5) — finite, deterministic, bounded. NOT visual.

const PACK_RES_DIR := "res://worldgen_terrain/fixtures"
const PACK_FILE := "height_pack.json"
const MAX_RELIEF := 1000.0  # max relief_m across families in height_pack.json

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10Height"):
		push_error("Wg10Height native class not registered")
		return 1
	var os_dir: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var h: Object = ClassDB.instantiate("Wg10Height")
	var err: String = str(h.call("load_pack_dir", os_dir, PACK_FILE))
	if err != "":
		push_error("pack load failed: %s" % err)
		return 1

	var errors: Array[String] = []
	var coords := [Vector2(0, 0), Vector2(-1024.5, 2048.25), Vector2(1e6, -1e6), Vector2(40000.0, 9000.0)]
	for c in coords:
		var v: float = h.call("height", c.x, c.y, 1337)
		if not is_finite(v):
			errors.append("non-finite height @ %s: %f" % [str(c), v])
		if v < -1.0 or v > MAX_RELIEF + 1.0:
			errors.append("height out of bounds @ %s: %f" % [str(c), v])
		var v2: float = h.call("height", c.x, c.y, 1337)
		if v != v2:
			errors.append("non-deterministic @ %s: %f vs %f" % [str(c), v, v2])

	# variety: across a grid, more than one distinct height appears (not collapsed).
	var seen := {}
	for ix in range(-8, 8):
		for iz in range(-8, 8):
			var wx: float = float(ix) * 40000.0
			var wz: float = float(iz) * 40000.0
			var v: float = h.call("height", wx, wz, 1337)
			seen[snappedf(v, 0.01)] = true
	if seen.size() < 2:
		errors.append("height variety collapsed: %d distinct values" % seen.size())

	if not errors.is_empty():
		for e in errors:
			push_error(e)
		print("[wg10-height] status=fail errors=%d" % errors.size())
		return 1
	print("[wg10-height] status=pass coords=%d variety=%d" % [coords.size(), seen.size()])
	return 0
