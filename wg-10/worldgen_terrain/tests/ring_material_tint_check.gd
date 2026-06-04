extends SceneTree

const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10ClipmapRings"):
		push_error("[wg10-ring-material-tint] Wg10ClipmapRings not registered")
		return 1

	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", 1, 8192.0, 64, SHADER)
	get_root().add_child(rings)
	rings.call("set_tile_debug_color", 0, 0, 0, Color(0.10, 0.42, 0.24, 1.0), 0.34)

	var tile := (rings as Node).get_child(4) as MeshInstance3D
	var errs: Array[String] = []
	_expect(tile != null, "center tile missing", errs)
	var mat := tile.material_override as ShaderMaterial if tile != null else null
	_expect(mat != null, "center tile material missing", errs)
	if mat != null:
		var color: Color = mat.get_shader_parameter("biome_debug_color")
		var mix := float(mat.get_shader_parameter("biome_material_mix"))
		_expect(color.is_equal_approx(Color(0.10, 0.42, 0.24, 1.0)), "biome_debug_color not bound", errs)
		_expect(absf(mix - 0.34) < 0.0001, "biome_material_mix not bound", errs)

	rings.queue_free()
	if not errs.is_empty():
		for err in errs:
			push_error(err)
		print("[wg10-ring-material-tint] status=fail errors=%d" % errs.size())
		return 1

	print("[wg10-ring-material-tint] status=pass mix=0.34")
	return 0

func _expect(condition: bool, message: String, errs: Array[String]) -> void:
	if not condition:
		errs.append(message)
