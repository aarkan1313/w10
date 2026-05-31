extends Node3D

const DEFAULT_DATA_PATH := "res://worldgen_terrain/generated/review/rough_world_chunks_3x3.json"
const FLY_CAMERA := "res://worldgen_terrain/harness/fly_camera.gd"

const BASE_HEIGHT_SCALE := 260.0
const EASY_SLOPE := 0.12
const PASSABLE_SLOPE := 0.28
const STEEP_SLOPE := 0.45
const OVERLAY_TERRAIN := 0
const OVERLAY_SLOPE := 1
const OVERLAY_CORRIDOR := 2
const OVERLAY_COUNT := 3
const DEFAULT_REVIEW_VARIANTS := [
	{"id": "current_plain", "label": "current relief / plain", "relief": 1.0, "dressing": "plain"},
	{"id": "medium_dressed", "label": "medium relief / dressed", "relief": 1.25, "dressing": "review_biome"},
	{"id": "high_dressed", "label": "high relief / dressed", "relief": 1.50, "dressing": "review_biome"},
	{"id": "high_route_read", "label": "high relief / route-read", "relief": 1.65, "dressing": "review_route"},
]

@export var data_path := DEFAULT_DATA_PATH
@export var review_title := "WG10 rough-highlands chunk continuity review"

var _camera: Camera3D
var _hud: Label
var _chunks_root: Node3D
var _guides_root: Node3D
var _payload: Dictionary = {}
var _seed_worlds: Array = []
var _review_variants: Array = []
var _seed_index := 0
var _variant_index := 0
var _relief := 1.0
var _dressing_style := "plain"
var _overview := false
var _flat_lighting := false
var _show_seam_guides := false
var _seam_focus_index := -1
var _overlay_mode := OVERLAY_TERRAIN
var _seam_targets: Array[Dictionary] = []

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
	if not _load_payload():
		return
	_load_review_variants()
	_apply_variant(0, false)
	_build_chunks(true)

func _build_hud() -> void:
	var layer := CanvasLayer.new()
	add_child(layer)
	_hud = Label.new()
	_hud.position = Vector2(10, 8)
	_hud.add_theme_color_override("font_color", Color.WHITE)
	layer.add_child(_hud)

func _load_payload() -> bool:
	var path := ProjectSettings.globalize_path(data_path)
	if not FileAccess.file_exists(path):
		push_error("rough_world_chunks_review: missing generated data at %s. Run tools/dem_pack/export_godot_rough_world_chunks.py" % data_path)
		return false
	var text := FileAccess.get_file_as_string(path)
	var parsed: Variant = JSON.parse_string(text)
	if typeof(parsed) != TYPE_DICTIONARY or not parsed.has("seeds"):
		push_error("rough_world_chunks_review: invalid generated data")
		return false
	_payload = parsed
	_seed_worlds = _payload["seeds"]
	return not _seed_worlds.is_empty()

func _load_review_variants() -> void:
	_review_variants.clear()
	var payload_variants: Array = _payload.get("review_variants", [])
	var source: Array = payload_variants if not payload_variants.is_empty() else DEFAULT_REVIEW_VARIANTS
	for item in source:
		if typeof(item) == TYPE_DICTIONARY:
			_review_variants.append(item)
	if _review_variants.is_empty():
		_review_variants.append(DEFAULT_REVIEW_VARIANTS[0])

func _current_variant() -> Dictionary:
	if _review_variants.is_empty():
		return DEFAULT_REVIEW_VARIANTS[0]
	return _review_variants[clampi(_variant_index, 0, _review_variants.size() - 1)]

func _apply_variant(index: int, rebuild: bool = true) -> void:
	if _review_variants.is_empty():
		_load_review_variants()
	_variant_index = posmod(index, _review_variants.size())
	var variant := _current_variant()
	_relief = clampf(float(variant.get("relief", 1.0)), 0.35, 1.95)
	_dressing_style = str(variant.get("dressing", "plain"))
	if rebuild:
		_build_chunks()

func _cycle_variant() -> void:
	_apply_variant(_variant_index + 1)

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

func _height_scale() -> float:
	return BASE_HEIGHT_SCALE * _relief

func _build_chunks(reset_camera: bool = false) -> void:
	if _chunks_root != null and is_instance_valid(_chunks_root):
		remove_child(_chunks_root)
		_chunks_root.free()
	if _guides_root != null and is_instance_valid(_guides_root):
		remove_child(_guides_root)
		_guides_root.free()
	_chunks_root = Node3D.new()
	_chunks_root.name = "Chunks"
	add_child(_chunks_root)
	_guides_root = Node3D.new()
	_guides_root.name = "SeamGuides"
	_guides_root.visible = _show_seam_guides
	add_child(_guides_root)
	_seam_targets.clear()

	var seed_world: Dictionary = _seed_worlds[_seed_index]
	var corridor_height := _height_percentile(seed_world["height"], 0.55)
	var chunk_grid := _chunk_grid(seed_world)
	for chunk_var in seed_world["chunks"]:
		var chunk: Dictionary = chunk_var
		var mesh_instance := MeshInstance3D.new()
		mesh_instance.name = "Chunk_%s_%s" % [chunk.get("chunk_x", "?"), chunk.get("chunk_z", "?")]
		mesh_instance.mesh = _make_mesh(chunk, corridor_height)
		mesh_instance.material_override = _make_material()
		_chunks_root.add_child(mesh_instance)
	_build_seam_guides(chunk_grid)

	if reset_camera:
		_focus_camera()

func _chunk_grid(seed_world: Dictionary) -> Array:
	var chunk_count := int(_payload.get("chunk_count", 3))
	var grid := []
	for z in range(chunk_count):
		var row := []
		for _x in range(chunk_count):
			row.append({})
		grid.append(row)
	for chunk_var in seed_world["chunks"]:
		var chunk: Dictionary = chunk_var
		var x := int(chunk.get("chunk_x", 0))
		var z := int(chunk.get("chunk_z", 0))
		if z >= 0 and z < grid.size() and x >= 0 and x < grid[z].size():
			grid[z][x] = chunk
	return grid

func _build_seam_guides(chunk_grid: Array) -> void:
	if _guides_root == null:
		return
	var chunk_count := int(_payload.get("chunk_count", 3))
	for z in range(chunk_count):
		for x in range(chunk_count - 1):
			var chunk: Dictionary = chunk_grid[z][x]
			if not chunk.is_empty():
				_add_seam_guide(chunk, true)
	for z in range(chunk_count - 1):
		for x in range(chunk_count):
			var chunk: Dictionary = chunk_grid[z][x]
			if not chunk.is_empty():
				_add_seam_guide(chunk, false)

func _make_guide_material() -> StandardMaterial3D:
	var mat := StandardMaterial3D.new()
	mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	mat.albedo_color = Color(0.05, 0.95, 1.0, 1.0)
	mat.disable_receive_shadows = true
	return mat

func _add_seam_guide(chunk: Dictionary, east_edge: bool) -> void:
	var n := int(chunk["n"])
	var heights: Array = chunk["height"]
	var span := float(chunk["span_m"])
	var origin_x := float(chunk["display_origin_x_m"])
	var origin_z := float(chunk["display_origin_z_m"])
	var verts := PackedVector3Array()
	var idx := PackedInt32Array()
	for i in range(n):
		var x := n - 1 if east_edge else i
		var z := i if east_edge else n - 1
		var h := float(heights[z * n + x])
		var px := origin_x + float(x) / float(n - 1) * span
		var pz := origin_z + float(z) / float(n - 1) * span
		verts.append(Vector3(px, h * _height_scale() + 9.0, pz))
	for i in range(n - 1):
		idx.append(i)
		idx.append(i + 1)
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = verts
	arrays[Mesh.ARRAY_INDEX] = idx
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_LINES, arrays)
	var instance := MeshInstance3D.new()
	instance.name = "SeamGuide"
	instance.mesh = mesh
	instance.material_override = _make_guide_material()
	_guides_root.add_child(instance)

	var mid := verts[int(n / 2)]
	_seam_targets.append({
		"axis": "x" if east_edge else "z",
		"position": mid,
		"span": span,
	})

func _make_mesh(chunk: Dictionary, corridor_height: float) -> ArrayMesh:
	var n := int(chunk["n"])
	var apron_n := int(chunk["apron_n"])
	var heights: Array = chunk["height"]
	var apron: Array = chunk["apron_height"]
	var corridors: Array = chunk.get("corridor", [])
	var span := float(chunk["span_m"])
	var origin_x := float(chunk["display_origin_x_m"])
	var origin_z := float(chunk["display_origin_z_m"])
	var height_scale := _height_scale()
	var cell := span / float(n - 1)
	var verts := PackedVector3Array()
	var normals := PackedVector3Array()
	var colors := PackedColorArray()
	var idx := PackedInt32Array()

	for z in range(n):
		for x in range(n):
			var i := z * n + x
			var h := float(heights[i])
			var structural_corridor := corridors.size() == heights.size() and int(corridors[i]) != 0
			var px := origin_x + float(x) / float(n - 1) * span
			var pz := origin_z + float(z) / float(n - 1) * span
			var slope := _slope_at(apron, apron_n, x, z, cell, height_scale)
			verts.append(Vector3(px, h * height_scale, pz))
			normals.append(_normal_at(apron, apron_n, x, z, cell, height_scale))
			colors.append(_terrain_color(h, slope, corridor_height, structural_corridor))

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

func _apron_height_at(apron: Array, apron_n: int, x: int, z: int) -> float:
	var ax: int = clampi(x + 1, 0, apron_n - 1)
	var az: int = clampi(z + 1, 0, apron_n - 1)
	return float(apron[az * apron_n + ax])

func _normal_at(apron: Array, apron_n: int, x: int, z: int, cell: float, height_scale: float) -> Vector3:
	var hl := _apron_height_at(apron, apron_n, x - 1, z)
	var hr := _apron_height_at(apron, apron_n, x + 1, z)
	var hd := _apron_height_at(apron, apron_n, x, z - 1)
	var hu := _apron_height_at(apron, apron_n, x, z + 1)
	var x_vec := Vector3(cell * 2.0, (hr - hl) * height_scale, 0.0)
	var z_vec := Vector3(0.0, (hu - hd) * height_scale, cell * 2.0)
	return z_vec.cross(x_vec).normalized()

func _slope_at(apron: Array, apron_n: int, x: int, z: int, cell: float, height_scale: float) -> float:
	var hl := _apron_height_at(apron, apron_n, x - 1, z)
	var hr := _apron_height_at(apron, apron_n, x + 1, z)
	var hd := _apron_height_at(apron, apron_n, x, z - 1)
	var hu := _apron_height_at(apron, apron_n, x, z + 1)
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

func _terrain_color(h: float, slope: float, corridor_height: float, structural_corridor: bool) -> Color:
	if _overlay_mode == OVERLAY_SLOPE:
		if slope < EASY_SLOPE:
			return Color(0.18, 0.62, 0.28)
		if slope < PASSABLE_SLOPE:
			return Color(0.86, 0.72, 0.22)
		if slope < STEEP_SLOPE:
			return Color(0.92, 0.40, 0.18)
		return Color(0.70, 0.13, 0.12)
	if _overlay_mode == OVERLAY_CORRIDOR:
		var is_low_corridor := structural_corridor or (h <= corridor_height and slope <= PASSABLE_SLOPE)
		if is_low_corridor:
			return Color(0.08, 0.72, 0.88)
		if slope <= EASY_SLOPE:
			return Color(0.24, 0.70, 0.32)
		if slope <= PASSABLE_SLOPE:
			return Color(0.80, 0.78, 0.24)
		if slope <= STEEP_SLOPE:
			return Color(0.88, 0.46, 0.16)
		return Color(0.52, 0.10, 0.10)
	if _dressing_style == "review_biome" or _dressing_style == "review_route":
		return _review_dressed_color(h, slope, structural_corridor)
	return _plain_terrain_color(h)

func _plain_terrain_color(h: float) -> Color:
	var t: float = clampf((h + 1.0) * 0.5, 0.0, 1.0)
	var low := Color(0.40, 0.48, 0.38)
	var mid := Color(0.62, 0.56, 0.40)
	var high := Color(0.74, 0.70, 0.58)
	var crest := Color(0.82, 0.79, 0.68)
	if t < 0.58:
		return low.lerp(mid, t / 0.58)
	if t < 0.90:
		return mid.lerp(high, (t - 0.58) / 0.32)
	return high.lerp(crest, (t - 0.90) / 0.10)

func _review_dressed_color(h: float, slope: float, structural_corridor: bool) -> Color:
	var t: float = clampf((h + 1.0) * 0.5, 0.0, 1.0)
	var valley := Color(0.16, 0.34, 0.27)
	var grass := Color(0.30, 0.46, 0.26)
	var dry := Color(0.55, 0.48, 0.33)
	var rock := Color(0.46, 0.45, 0.41)
	var snow := Color(0.82, 0.84, 0.78)
	var base := valley
	if t < 0.35:
		base = valley.lerp(grass, t / 0.35)
	elif t < 0.62:
		base = grass.lerp(dry, (t - 0.35) / 0.27)
	elif t < 0.82:
		base = dry.lerp(rock, (t - 0.62) / 0.20)
	elif t < 0.94:
		base = rock.lerp(snow, (t - 0.82) / 0.12)
	else:
		base = snow
	if slope > PASSABLE_SLOPE:
		base = base.lerp(rock, clampf((slope - PASSABLE_SLOPE) / maxf(STEEP_SLOPE - PASSABLE_SLOPE, 0.001), 0.0, 0.72))
	if slope > STEEP_SLOPE:
		base = base.lerp(Color(0.32, 0.32, 0.30), 0.45)
	if structural_corridor:
		var route := Color(0.12, 0.42, 0.46) if _dressing_style == "review_route" else Color(0.18, 0.39, 0.33)
		base = base.lerp(route, 0.58)
	return base

func _input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		match event.keycode:
			KEY_T:
				_seed_index = (_seed_index + 1) % _seed_worlds.size()
				_build_chunks()
			KEY_EQUAL, KEY_PLUS:
				_set_relief(_relief + 0.10)
			KEY_MINUS:
				_set_relief(_relief - 0.10)
			KEY_R:
				_apply_variant(_variant_index)
			KEY_V:
				_cycle_variant()
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
				_build_chunks()
			KEY_B:
				_show_seam_guides = not _show_seam_guides
				if _guides_root != null:
					_guides_root.visible = _show_seam_guides
			KEY_N:
				_focus_next_seam()
			KEY_P:
				_overlay_mode = (_overlay_mode + 1) % OVERLAY_COUNT
				_build_chunks()

func _set_relief(value: float) -> void:
	_relief = clampf(value, 0.35, 1.75)
	_build_chunks()

func _apply_camera_limits() -> void:
	var span := float(_payload.get("world_span_m", 76800.0))
	_camera.move_speed = maxf(90.0, span * 0.040)
	_camera.vertical_speed = maxf(70.0, span * 0.026)
	_camera.far = maxf(2400.0, span * 2.3)

func _focus_camera() -> void:
	_overview = false
	_apply_camera_limits()
	var span := float(_payload.get("world_span_m", 76800.0))
	_camera.global_position = Vector3(0.0, maxf(220.0, span * 0.052 + _height_scale() * 0.30), span * 0.185)
	_camera.rotation_degrees = Vector3(-24.0, 0.0, 0.0)

func _overview_camera() -> void:
	_apply_camera_limits()
	var span := float(_payload.get("world_span_m", 76800.0))
	_camera.global_position = Vector3(0.0, span * 0.54, span * 0.58)
	_camera.rotation_degrees = Vector3(-48.0, 0.0, 0.0)

func _focus_next_seam() -> void:
	if _seam_targets.is_empty():
		return
	_show_seam_guides = true
	if _guides_root != null:
		_guides_root.visible = true
	_seam_focus_index = (_seam_focus_index + 1) % _seam_targets.size()
	var target: Dictionary = _seam_targets[_seam_focus_index]
	var pos: Vector3 = target["position"]
	var span := float(target["span"])
	_overview = false
	_apply_camera_limits()
	_camera.global_position = pos + Vector3(span * 0.10, maxf(260.0, span * 0.030), span * 0.12)
	_camera.look_at(pos, Vector3.UP)

func _overlay_label() -> String:
	match _overlay_mode:
		OVERLAY_SLOPE:
			return "slope"
		OVERLAY_CORRIDOR:
			return "corridor"
		_:
			return "terrain"

func _variant_label() -> String:
	var variant := _current_variant()
	return "%d/%d %s" % [
		_variant_index + 1,
		_review_variants.size(),
		str(variant.get("label", variant.get("id", "variant"))),
	]

func _process(_delta: float) -> void:
	if _hud == null or _seed_worlds.is_empty():
		return
	var seed_world: Dictionary = _seed_worlds[_seed_index]
	var chunk_km := float(_payload.get("chunk_span_m", 0.0)) / 1000.0
	var world_km := float(_payload.get("world_span_m", 0.0)) / 1000.0
	var chunk_count := int(_payload.get("chunk_count", 3))
	_hud.text = "%s\n" % review_title \
		+ "T seed | V variant | P overlay | B seam guides | N next seam | F focus | G overview | +/- relief | R reset variant | L flat | WASD/Space/C fly | Esc mouse\n" \
		+ "Seed: %s | chunks %dx%d @ %.1f km | world %.1f km | variant %s | relief %.2fx | height %.0fm | %s | dressing %s | %s | seam guides %s | prototype, not runtime streaming" % [
			seed_world.get("seed", "?"),
			chunk_count,
			chunk_count,
			chunk_km,
			world_km,
			_variant_label(),
			_relief,
			_height_scale(),
			_overlay_label(),
			_dressing_style,
			"flat" if _flat_lighting else "lit/no-shadow/no-fog",
			"on" if _show_seam_guides else "off",
		]
