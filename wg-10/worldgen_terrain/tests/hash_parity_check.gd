extends SceneTree

# Cross-boundary parity gate: proves the NATIVE lib as loaded by Godot
# reproduces WG9's reference fixture (not just the Rust unit test).
#
# The binding's `stable_hash_ints(prefix, PackedInt64Array)` represents only
# cases shaped "string prefix + all-int tail". The fixture also contains cases
# with a string in the middle (e.g. "kernel|mountain|..."); those are skipped
# here and logged. That is sufficient parity for this layer — the Rust unit
# tests already lock the full hash contract, and the int-tail cases prove the
# native boundary round-trips correctly. (Plan Task 6, flagged risk #3.)

const FIXTURE := "res://worldgen_terrain/fixtures/hash_reference.json"

func _init() -> void:
	quit(_run())

func _run() -> int:
	var f := FileAccess.open(FIXTURE, FileAccess.READ)
	if f == null:
		push_error("missing fixture: %s" % FIXTURE)
		return 1
	var data: Variant = JSON.parse_string(f.get_as_text())
	if typeof(data) != TYPE_DICTIONARY:
		push_error("fixture not an object")
		return 1
	if not ClassDB.class_exists("Wg10Hash"):
		push_error("Wg10Hash native class not registered")
		return 1
	var hasher: Object = ClassDB.instantiate("Wg10Hash")
	var errors: Array[String] = []
	var checked := 0
	var skipped := 0
	for case_value in (data as Dictionary).get("stable_hash_cases", []) as Array:
		var case: Dictionary = case_value as Dictionary
		var values: Array = case["values"] as Array
		# Representable only if values[0] is a string and values[1..] are all ints.
		var representable := values.size() >= 1 and typeof(values[0]) == TYPE_STRING
		if representable:
			for i in range(1, values.size()):
				if typeof(values[i]) != TYPE_INT and typeof(values[i]) != TYPE_FLOAT:
					representable = false
					break
		if not representable:
			skipped += 1
			continue
		var prefix: String = str(values[0])
		var ints := PackedInt64Array()
		for i in range(1, values.size()):
			ints.append(int(values[i]))
		var got: int = int(hasher.call("stable_hash_ints", prefix, ints))
		var want: int = int(case["hash_u32"])
		checked += 1
		if got != want:
			errors.append("%s got=%d want=%d" % [str(case.get("joined_text", "")), got, want])
	if not errors.is_empty():
		for e in errors:
			push_error(e)
		print("[wg10-hash-parity] status=fail errors=%d checked=%d skipped=%d" % [errors.size(), checked, skipped])
		return 1
	if checked == 0:
		push_error("no representable stable_hash cases were checked")
		print("[wg10-hash-parity] status=fail checked=0 skipped=%d" % skipped)
		return 1
	print("[wg10-hash-parity] status=pass checked=%d skipped=%d (skipped = string-in-tail cases)" % [checked, skipped])
	return 0
