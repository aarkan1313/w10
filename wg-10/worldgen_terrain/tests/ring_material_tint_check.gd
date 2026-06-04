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

	var shader_source := FileAccess.get_file_as_string(ProjectSettings.globalize_path(SHADER))
	_expect(shader_source.contains("static_material_tex : filter_linear"), "static material texture should be linearly filtered for soft presentation", errs)
	_expect(shader_source.contains("float material_fade = clamp(page_fade, 0.0, 1.0);"), "shader should derive material_fade from page_fade", errs)
	_expect(shader_source.contains("float accepted_material_mix = static_material_mix * material_fade * 0.62;"), "static material mix should fade and stay presentation-softened", errs)
	_expect(shader_source.contains("biome_material_mix * material_fade"), "WORLD route tint should fade with page_fade", errs)
	_expect(shader_source.contains("float corridor_w = clamp(1.0 - abs(code - 1.0), 0.0, 1.0);"), "static material codes should blend corridor weight softly", errs)
	_expect(shader_source.contains("float total_w = corridor_w + rock_w + snow_w;"), "static material blend should combine soft class weights", errs)
	_expect(shader_source.contains("vec3(0.18, 0.39, 0.33)"), "static corridor material should match accepted corridor blend target", errs)
	_expect(shader_source.contains("mix(base, vec3(0.46, 0.45, 0.41), 0.70)"), "static rock material should blend instead of replacing terrain color", errs)
	_expect(shader_source.contains("mix(base, vec3(0.82, 0.84, 0.78), 0.64)"), "static snow material should blend instead of replacing terrain color", errs)
	_expect(not shader_source.contains("vec3(0.74, 0.78, 0.72)"), "static snow material should not use a flat replacement tint", errs)
	_expect(not shader_source.contains("vec3(0.90, 0.88, 0.76)"), "static snow material should not use chalk-white override", errs)
	_expect(shader_source.contains("float lit = 0.64 + 0.36 * ndl;"), "terrain lighting should keep softened directional contrast", errs)
	_expect(shader_source.contains("0.12);"), "terrain lighting should keep softened slope shadow", errs)

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
