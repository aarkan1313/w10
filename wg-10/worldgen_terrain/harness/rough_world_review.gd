extends Node3D

# Default payload (the FOCUS-variant single-window review). Overridable per-scene via @export
# so the same switcher can drive other item payloads (e.g. the A|B|v2 keeper comparison).
@export var data_path := "res://worldgen_terrain/generated/review/rough_world_3d.json"
const FLY_CAMERA := "res://worldgen_terrain/harness/fly_camera.gd"

const BASE_WORLD_SIZE := 128.0
const BASE_HEIGHT_SCALE := 260.0
const REFERENCE_WORLD_SIZE := 25600.0
const SCALE_PRESETS := [10.0, 25.0, 50.0, 100.0, 150.0, 200.0]
const RELIEF_POLICY_PRESETS := [0.0, 0.5, 1.0]
const EASY_SLOPE := 0.12
const PASSABLE_SLOPE := 0.28
const STEEP_SLOPE := 0.45
const OVERLAY_TERRAIN := 0
const OVERLAY_SLOPE := 1
const OVERLAY_CORRIDOR := 2
const OVERLAY_COUNT := 3

var _camera: Camera3D
var _hud: Label
var _terrain: MeshInstance3D
var _items: Array = []
var _selected := 4
var _relief := 1.0
var _scale_index := 5
var _relief_policy_index := 0
var _overview := false
var _flat_lighting := false
var _overlay_mode := OVERLAY_TERRAIN

func _ready() -> void:
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.68, 0.76, 0.84)
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color(0.94, 0.91, 0.82)
	env.ambient_light_energy = 1.65
	env.fog_enabled = false

	var sun := DirectionalLight3D.new()
	sun.name = "Sun"
	sun.light_energy = 0.82
	sun.shadow_enabled = false
	sun.rotation_degrees = Vector3(-68.0, -24.0, 0.0)
	add_child(sun)

	_camera = load(FLY_CAMERA).new()
	_camera.name = "ReviewCamera"
	_camera.environment = env
	_camera.sprint_mult = 4.0
	add_child(_camera)

	_build_hud()
	if not _load_items():
		return

	_terrain = MeshInstance3D.new()
	_terrain.name = "GeneratedWorld"
	_terrain.material_override = _make_material()
	add_child(_terrain)

	# Default to item 4 (the owner-preferred FOCUS variant in the 6-item payload) but clamp to the
	# payload size so smaller payloads (e.g. the 3-item A|B|v2 switcher) don't index out of bounds.
	_select(clampi(4, 0, _items.size() - 1), true)

func _build_hud() -> void:
	var layer := CanvasLayer.new()
	add_child(layer)
	_hud = Label.new()
	_hud.position = Vector2(10, 8)
	_hud.add_theme_color_override("font_color", Color.WHITE)
	layer.add_child(_hud)

func _load_items() -> bool:
	var path := ProjectSettings.globalize_path(data_path)
	if not FileAccess.file_exists(path):
		push_error("rough_world_review: missing generated data at %s. Run the matching export_godot_rough_world_*.py" % data_path)
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

func _world_scale() -> float:
	return float(SCALE_PRESETS[_scale_index])

func _world_size() -> float:
	return BASE_WORLD_SIZE * _world_scale()

func _relief_exponent() -> float:
	return float(RELIEF_POLICY_PRESETS[_relief_policy_index])

func _height_scale() -> float:
	return BASE_HEIGHT_SCALE * _relief * pow(_world_size() / REFERENCE_WORLD_SIZE, _relief_exponent())

func _apply_camera_limits() -> void:
	var span := _world_size()
	_camera.move_speed = maxf(55.0, span * 0.035)
	_camera.vertical_speed = maxf(45.0, span * 0.025)
	_camera.far = maxf(1200.0, span * 2.2)

func _make_mesh(item: Dictionary) -> ArrayMesh:
	var n := int(item["n"])
	var heights: Array = item["height"]
	var world_size := _world_size()
	var height_scale := _height_scale()
	var verts := PackedVector3Array()
	var normals := PackedVector3Array()
	var colors := PackedColorArray()
	var idx := PackedInt32Array()
	var cell := world_size / float(n - 1)
	var corridor_height := _height_percentile(heights, 0.55)

	for z in range(n):
		for x in range(n):
			var i := z * n + x
			var h := float(heights[i])
			var px := (float(x) / float(n - 1) - 0.5) * world_size
			var pz := (float(z) / float(n - 1) - 0.5) * world_size
			verts.append(Vector3(px, h * height_scale, pz))
			normals.append(_normal_at(heights, n, x, z, cell, height_scale))
			colors.append(_terrain_color(h, str(item.get("kind", "")), _slope_at(heights, n, x, z, cell, height_scale), corridor_height))

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

func _slope_at(heights: Array, n: int, x: int, z: int, cell: float, height_scale: float) -> float:
	var hl := _height_at(heights, n, x - 1, z)
	var hr := _height_at(heights, n, x + 1, z)
	var hd := _height_at(heights, n, x, z - 1)
	var hu := _height_at(heights, n, x, z + 1)
	var dx := ((hr - hl) * height_scale) / maxf(cell * 2.0, 0.001)
	var dz := ((hu - hd) * height_scale) / maxf(cell * 2.0, 0.001)
	return sqrt(dx * dx + dz * dz)

func _height_percentile(heights: Array, pct: float) -> float:
	if heights.is_empty():
		return 0.0
	var sorted := heights.duplicate()
	sorted.sort()
	var idx := clampi(int(round((float(sorted.size()) - 1.0) * pct)), 0, sorted.size() - 1)
	return float(sorted[idx])

func _terrain_color(h: float, kind: String, slope: float, corridor_height: float) -> Color:
	if _overlay_mode == OVERLAY_SLOPE:
		if slope < EASY_SLOPE:
			return Color(0.18, 0.62, 0.28)
		if slope < PASSABLE_SLOPE:
			return Color(0.86, 0.72, 0.22)
		if slope < STEEP_SLOPE:
			return Color(0.92, 0.40, 0.18)
		return Color(0.70, 0.13, 0.12)
	if _overlay_mode == OVERLAY_CORRIDOR:
		var is_low_corridor := h <= corridor_height and slope <= PASSABLE_SLOPE
		if is_low_corridor:
			return Color(0.08, 0.72, 0.88)
		if slope <= EASY_SLOPE:
			return Color(0.24, 0.70, 0.32)
		if slope <= PASSABLE_SLOPE:
			return Color(0.80, 0.78, 0.24)
		if slope <= STEEP_SLOPE:
			return Color(0.88, 0.46, 0.16)
		return Color(0.52, 0.10, 0.10)
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
			KEY_P:
				_overlay_mode = (_overlay_mode + 1) % OVERLAY_COUNT
				_rebuild_current_mesh()
			KEY_K:
				_set_relief_policy_index(_relief_policy_index + 1)
			KEY_COMMA:
				_set_scale_index(_scale_index - 1)
			KEY_PERIOD:
				_set_scale_index(_scale_index + 1)

func _set_relief(value: float) -> void:
	_relief = clampf(value, 0.35, 1.75)
	_rebuild_current_mesh()

func _set_scale_index(value: int) -> void:
	_scale_index = clampi(value, 0, SCALE_PRESETS.size() - 1)
	_rebuild_current_mesh()
	if _overview:
		_overview_camera()
	else:
		_focus_camera()

func _set_relief_policy_index(value: int) -> void:
	_relief_policy_index = value % RELIEF_POLICY_PRESETS.size()
	_rebuild_current_mesh()
	if _overview:
		_overview_camera()
	else:
		_focus_camera()

func _rebuild_current_mesh() -> void:
	if _items.size() > 0:
		_terrain.mesh = _make_mesh(_items[_selected])

func _focus_camera() -> void:
	_overview = false
	_apply_camera_limits()
	var span := _world_size()
	_camera.global_position = Vector3(0.0, maxf(110.0, span * 0.040 + _height_scale() * 0.30), span * 0.115)
	_camera.rotation_degrees = Vector3(-22.0, 0.0, 0.0)

func _overview_camera() -> void:
	_apply_camera_limits()
	var span := _world_size()
	_camera.global_position = Vector3(0.0, span * 0.48, span * 0.55)
	_camera.rotation_degrees = Vector3(-47.0, 0.0, 0.0)

func _overlay_label() -> String:
	match _overlay_mode:
		OVERLAY_SLOPE:
			return "slope"
		OVERLAY_CORRIDOR:
			return "corridor"
		_:
			return "terrain"

func _process(_delta: float) -> void:
	if _hud == null or _items.is_empty():
		return
	var item: Dictionary = _items[_selected]
	var scene_km := _world_size() / 1000.0
	var source_km := float(item.get("span_km", 0.0))
	var source_scene_ratio := source_km / maxf(scene_km, 0.001)
	_hud.text = "WG10 rough-highlands generated-world review\n" \
		+ "1-4 refs | 5-0 synth | [/] prev/next | F focus | G overview | +/- relief | ,/. scale | K policy | P overlay | L flat | WASD/Space/C fly | Esc mouse\n" \
		+ "Selected: %s | %s | %.1f km source -> %.1f km scene | source/scene %.2fx | relief %.2fx | k %.1f | height %.0fm | scale %.0fx | %s | lighting %s" % [
			item.get("label", "?"),
			item.get("kind", "?"),
			source_km,
			scene_km,
			source_scene_ratio,
			_relief,
			_relief_exponent(),
			_height_scale(),
			_world_scale(),
			_overlay_label(),
			"flat" if _flat_lighting else "lit/no-shadow/no-fog",
		]
