extends SceneTree

# DESIGN §4: the same (x,z,seed) must return the same value regardless of caller
# and regardless of how many times it is sampled. Guards the determinism
# contract at the Godot/native boundary.

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10Hash"):
		push_error("Wg10Hash not registered")
		return 1
	var a: Object = ClassDB.instantiate("Wg10Hash")
	var b: Object = ClassDB.instantiate("Wg10Hash")  # different instance = different "caller"
	var errors: Array[String] = []
	var coords := [Vector2(0, 0), Vector2(-1024.5, 2048.25), Vector2(1e6, -1e6)]
	for c in coords:
		var va: float = a.call("fbm", c.x, c.y, 800.0, 1337, 4)
		var vb: float = b.call("fbm", c.x, c.y, 800.0, 1337, 4)
		if va != vb:
			errors.append("caller mismatch @ %s: %f vs %f" % [str(c), va, vb])
		var again: float = a.call("fbm", c.x, c.y, 800.0, 1337, 4)
		if va != again:
			errors.append("repeat mismatch @ %s: %f vs %f" % [str(c), va, again])
	if not errors.is_empty():
		for e in errors:
			push_error(e)
		print("[wg10-determinism] status=fail errors=%d" % errors.size())
		return 1
	print("[wg10-determinism] status=pass coords=%d" % coords.size())
	return 0
