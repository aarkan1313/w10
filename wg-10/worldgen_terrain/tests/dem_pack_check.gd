extends SceneTree

# Real DEM pack property check (gate subset) through the native Wg10Height lib.
# finite, bounded, deterministic, varied — NOT visual. Same family of properties
# as height_check.gd, but on real 512x512 DEM kernels.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"

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
		push_error("dem pack load failed: %s" % err)
		return 1
	# max relief across the pack's families (bound for height) — read the json.
	var f := FileAccess.open(PACK_RES_DIR + "/" + PACK_FILE, FileAccess.READ)
	if f == null:
		push_error("cannot read pack json")
		return 1
	var pack: Dictionary = JSON.parse_string(f.get_as_text())
	var max_relief := 0.0
	for fid in pack["families"]:
		max_relief = maxf(max_relief, float(pack["families"][fid]["relief_m"]))

	var errors: Array[String] = []
	var coords := [Vector2(0, 0), Vector2(-1024.5, 2048.25), Vector2(1e6, -1e6), Vector2(40000.0, 9000.0)]
	for c in coords:
		var v: float = h.call("height", c.x, c.y, 1337)
		if not is_finite(v):
			errors.append("non-finite @ %s: %s" % [str(c), str(v)])
		if v < -1.0 or v > max_relief + 1.0:
			errors.append("out of bounds @ %s: %s (max_relief %s)" % [str(c), str(v), str(max_relief)])
		var v2: float = h.call("height", c.x, c.y, 1337)
		if v != v2:
			errors.append("non-deterministic @ %s" % str(c))

	var seen := {}
	for ix in range(-8, 8):
		for iz in range(-8, 8):
			var hv: float = h.call("height", float(ix) * 40000.0, float(iz) * 40000.0, 1337)
			seen[snappedf(hv, 0.01)] = true
	if seen.size() < 2:
		errors.append("height variety collapsed: %d" % seen.size())

	if not errors.is_empty():
		for e in errors: push_error(e)
		print("[wg10-dem-pack] status=fail errors=%d" % errors.size())
		return 1
	print("[wg10-dem-pack] status=pass coords=%d variety=%d max_relief=%s" % [coords.size(), seen.size(), str(max_relief)])
	return 0
