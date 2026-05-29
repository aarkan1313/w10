extends SceneTree

# Cross-boundary grammar properties through the native Wg10Grammar lib.
# Property-based (DESIGN §4 + design spec §5) — NOT parity against WG9 values.

const PACK := "res://worldgen_terrain/fixtures/golden_pack.json"

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10Grammar"):
		push_error("Wg10Grammar native class not registered")
		return 1
	var f := FileAccess.open(PACK, FileAccess.READ)
	if f == null:
		push_error("missing pack: %s" % PACK)
		return 1
	var g: Object = ClassDB.instantiate("Wg10Grammar")
	var err: String = str(g.call("load_pack_json", f.get_as_text()))
	if err != "":
		push_error("pack load failed: %s" % err)
		return 1

	var errors: Array[String] = []
	var coords := [Vector2(0, 0), Vector2(-1024.5, 2048.25), Vector2(1e6, -1e6), Vector2(40000.0, 9000.0)]
	for c in coords:
		var ids: PackedInt64Array = g.call("family_ids", c.x, c.y, 1337)
		var weights: PackedFloat64Array = g.call("weight_values", c.x, c.y, 1337)
		if ids.size() != weights.size():
			errors.append("id/weight length mismatch @ %s" % str(c))
		var sum := 0.0
		for wv in weights:
			sum += float(wv)
		if absf(sum - 1.0) > 1e-9:
			errors.append("weights !=1 @ %s: %f" % [str(c), sum])
		# determinism: same query twice -> identical
		var ids2: PackedInt64Array = g.call("family_ids", c.x, c.y, 1337)
		var weights2: PackedFloat64Array = g.call("weight_values", c.x, c.y, 1337)
		if ids != ids2 or weights != weights2:
			errors.append("non-deterministic @ %s" % str(c))

	# variety: across a region grid, more than one family set appears.
	var signatures := {}
	for rx in range(-8, 8):
		for rz in range(-8, 8):
			var wx: float = float(rx) * 40000.0
			var wz: float = float(rz) * 40000.0
			var ids: PackedInt64Array = g.call("family_ids", wx, wz, 1337)
			signatures[str(ids)] = true
	if signatures.size() < 2:
		errors.append("family variety collapsed: %d unique signatures" % signatures.size())

	if not errors.is_empty():
		for e in errors:
			push_error(e)
		print("[wg10-grammar] status=fail errors=%d" % errors.size())
		return 1
	print("[wg10-grammar] status=pass coords=%d variety=%d" % [coords.size(), signatures.size()])
	return 0
