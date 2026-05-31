extends Node3D

const DATA_PATH := "res://worldgen_terrain/generated/review/rough_world_3d.json"
const FLY_CAMERA := "res://worldgen_terrain/harness/fly_camera.gd"

const WORLD_SIZE := 128.0
const HEIGHT_SCALE := 13.0

var _camera: Camera3D
var _hud: Label
var _terrain: MeshInstance3D
var _items: Array = []
var _selected := 4
var _relief := 1.0
var _overview := false
var _flat_lighting := false

func _ready() -> void:
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.68, 0.76, 0.84)
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color(0.94, 0.91, 0.82)
	env.ambient_light_energy = 1.65
	env.fog_enabled = true
	env.fog_light_color = Color(0.68, 0.76, 0.84)
	env.fog_density = 0.0018

	var sun := DirectionalLight3D.new()
	sun.name = "Sun"
	sun.light_energy = 0.82
	sun.shadow_enabled = false
	sun.rotation_degrees = Vector3(-68.0, -24.0, 0.0)
	add_child(sun)

	_camera = load(FLY_CAMERA).new()
	_camera.name = "ReviewCamera"
	_camera.environment = env
	_camera.move_speed = 18.0
	_camera.sprint_mult = 3.0
	_camera.vertical_speed = 15.0
	_camera.far = 360.0
	add_child(_camera)

	_build_hud()
	if not _load_items():
		return

	_terrain = MeshInstance3D.new()
	_terrain.name = "GeneratedWorld"
	_terrain.material_override = _make_material()
	add_child(_terrain)

	_select(4, true)

func _build_hud() -> void:
	var layer := CanvasLayer.new()
	add_child(layer)
	_hud = Label.new()
	_hud.position = Vector2(10, 8)
	_hud.add_theme_color_override("font_color", Color.WHITE)
	layer.add_child(_hud)

func _load_items() -> bool:
	var path := ProjectSettings.globalize_path(DATA_PATH)
	if not FileAccess.file_exists(path):
		push_error("rough_world_review: missing generated data at %s. Run tools/dem_pack/export_godot_rough_world_review.py" % DATA_PATH)
		return false
	var text := FileAccess.get_file_as_string(path)
	var parsed: Variant = JSON.parse_string(text)
	if typeof(parsed) != TYPE_DICTIONARY or not parsed.has("items"):
		push_error("rough_world_review: invalid generated data")
		return false
	_items = parsed["items"]
	return true

func _make_material() -> StandardMaterial3D:
	var mat := StandardMaterial3D.new()
	mat.vertex_color_use_as_albedo = true
	mat.roughness = 1.0
	mat.specular_mode = BaseMaterial3D.SPECULAR_DISABLED
	mat.cull_mode = BaseMaterial3D.CULL_DISABLED
	mat.disable_receive_shadows = true
	if _flat_lighting:
		mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	return mat

func _select(index: int, reset_camera: bool = false) -> void:
	if index < 0 or index >= _items.size():
		return
	_selected = index
	var item: Dictionary = _items[_selected]
	_terrain.mesh = _make_mesh(item)
	if reset_camera:
		_focus_camera()

func _make_mesh(item: Dictionary) -> ArrayMesh:
	var n := int(item["n"])
	var heights: Array = item["height"]
	var height_scale := HEIGHT_SCALE * _relief
	var verts := PackedVector3Array()
	var normals := PackedVector3Array()
	var colors := PackedColorArray()
	var idx := PackedInt32Array()
	var cell := WORLD_SIZE / float(n - 1)

	for z in range(n):
		for x in range(n):
			var i := z * n + x
			var h := float(heights[i])
			var px := (float(x) / float(n - 1) - 0.5) * WORLD_SIZE
			var pz := (float(z) / float(n - 1) - 0.5) * WORLD_SIZE
			verts.append(Vector3(px, h * height_scale, pz))
			normals.append(_normal_at(heights, n, x, z, cell, height_scale))
			colors.append(_terrain_color(h, str(item.get("kind", ""))))

	for z in range(n - 1):
		for x in range(n - 1):
			var a := z * n + x
			var b := a + 1
			var c := a + n
			var d := c + 1
			idx.append(a); idx.append(c); idx.append(b)
			idx.append(b); idx.append(c); idx.append(d)

	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = verts
	arrays[Mesh.ARRAY_NORMAL] = normals
	arrays[Mesh.ARRAY_COLOR] = colors
	arrays[Mesh.ARRAY_INDEX] = idx
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	return mesh

func _height_at(heights: Array, n: int, x: int, z: int) -> float:
	var cx: int = clampi(x, 0, n - 1)
	var cz: int = clampi(z, 0, n - 1)
	return float(heights[cz * n + cx])

func _normal_at(heights: Array, n: int, x: int, z: int, cell: float, height_scale: float) -> Vector3:
	var hl := _height_at(heights, n, x - 1, z)
	var hr := _height_at(heights, n, x + 1, z)
	var hd := _height_at(heights, n, x, z - 1)
	var hu := _height_at(heights, n, x, z + 1)
	var x_vec := Vector3(cell * 2.0, (hr - hl) * height_scale, 0.0)
	var z_vec := Vector3(0.0, (hu - hd) * height_scale, cell * 2.0)
	return z_vec.cross(x_vec).normalized()

func _terrain_color(h: float, kind: String) -> Color:
	var t: float = clampf((h + 1.0) * 0.5, 0.0, 1.0)
	var low := Color(0.40, 0.48, 0.38)
	var mid := Color(0.62, 0.56, 0.40)
	var high := Color(0.74, 0.70, 0.58)
	var crest := Color(0.82, 0.79, 0.68)
	var c: Color
	if t < 0.58:
		c = low.lerp(mid, t / 0.58)
	elif t < 0.90:
		c = mid.lerp(high, (t - 0.58) / 0.32)
	else:
		c = high.lerp(crest, (t - 0.90) / 0.10)
	if kind == "ref":
		c = c.lerp(Color(0.78, 0.78, 0.72), 0.18)
	return c

func _input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		match event.keycode:
			KEY_1, KEY_2, KEY_3, KEY_4, KEY_5, KEY_6, KEY_7, KEY_8, KEY_9:
				_select(event.keycode - KEY_1)
			KEY_0:
				_select(9)
			KEY_BRACKETLEFT:
				_select((_selected - 1 + _items.size()) % _items.size())
			KEY_BRACKETRIGHT:
				_select((_selected + 1) % _items.size())
			KEY_EQUAL, KEY_PLUS:
				_set_relief(_relief + 0.10)
			KEY_MINUS:
				_set_relief(_relief - 0.10)
			KEY_R:
				_set_relief(1.0)
			KEY_F:
				_focus_camera()
			KEY_G:
				_overview = not _overview
				if _overview:
					_overview_camera()
				else:
					_focus_camera()
			KEY_L:
				_flat_lighting = not _flat_lighting
				_terrain.material_override = _make_material()

func _set_relief(value: float) -> void:
	_relief = clampf(value, 0.35, 1.75)
	if _items.size() > 0:
		_terrain.mesh = _make_mesh(_items[_selected])

func _focus_camera() -> void:
	_overview = false
	_camera.global_position = Vector3(0.0, 18.0, 52.0)
	_camera.rotation_degrees = Vector3(-23.0, 0.0, 0.0)

func _overview_camera() -> void:
	_camera.global_position = Vector3(0.0, 94.0, 92.0)
	_camera.rotation_degrees = Vector3(-50.0, 0.0, 0.0)

func _process(_delta: float) -> void:
	if _hud == null or _items.is_empty():
		return
	var item: Dictionary = _items[_selected]
	_hud.text = "WG10 rough-highlands generated-world review\n" \
		+ "1-4 refs | 5-0 synth | [/] prev/next | F focus | G overview | +/- relief | L flat | WASD/Space/C fly | Esc mouse\n" \
		+ "Selected: %s | %s | %.1f km source | relief %.2fx | lighting %s" % [
			item.get("label", "?"),
			item.get("kind", "?"),
			float(item.get("span_km", 0.0)),
			_relief,
			"flat" if _flat_lighting else "lit/no-shadow",
		]
