extends Node3D

# Un-intercept proving ladder scene.
# Thin assembly of the proven render stack (DESIGN 6.4: scenes assemble, they do not
# contain metric/report logic). Rung selection drives which producer the pool runs.
# Wiring sequence + probe API copied from wg10_progression_review.gd (verified template).

const PRODUCERS := "res://worldgen_terrain/harness/ladder_producers.gd"
const RUNTIME_CONFIG := "res://worldgen_terrain/harness/mountain_fly_runtime_config.gd"
const FLY_CAMERA := "res://worldgen_terrain/harness/fly_camera.gd"

var _producer: Object
var _runtime: Object   # mountain_fly_runtime_config.gd instance
var _pool: Object
var _streamer: Object
var _rings: Object
var _view: Object
var _camera: Camera3D   # this IS a Wg10FlyCamera (extends Camera3D)
var _label: Label
var _frame := 0
var _probe_mode := false

func _ready() -> void:
	_runtime = load(RUNTIME_CONFIG).new()
	_runtime.call("register_shader_globals", bool(_runtime.call("default_detail_enabled")))
	_producer = load(PRODUCERS).new()
	_build_runtime()
	_build_camera()
	_build_hud()

# Wiring sequence is the exact order from wg10_progression_review.gd:_ready (verified).
func _build_runtime() -> void:
	_pool = ClassDB.instantiate("Wg10PagePool")
	var err := str(_producer.call("configure", _pool))   # producer selects the rung's producer path
	if err != "":
		push_error("[ladder] producer.configure failed: %s" % err)
	_streamer = ClassDB.instantiate("Wg10Streamer")
	_runtime.call("configure_streamer", _streamer, _pool)
	_rings = ClassDB.instantiate("Wg10ClipmapRings")
	_runtime.call("configure_rings", _rings)
	add_child(_rings)
	_view = ClassDB.instantiate("Wg10TerrainView")
	add_child(_view)
	_reconfigure_view()

func _reconfigure_view() -> void:
	if _view == null or _pool == null or _streamer == null or _rings == null or _runtime == null:
		return
	# Ladder renders real metres directly (relief_scale 1.0); relief_ref follows the rung.
	var relief_scale := 1.0
	var relief_ref := float(_producer.call("relief_m"))
	_runtime.call("configure_view", _view, _pool, _streamer, _rings, bool(_runtime.call("default_morph_enabled")), relief_scale, relief_ref)

func _build_camera() -> void:
	# Wg10FlyCamera EXTENDS Camera3D — instantiate it AS the camera (verified fly_camera.gd:1-2).
	_camera = load(FLY_CAMERA).new()
	_camera.far = 200000.0
	add_child(_camera)
	_camera.position = Vector3(-9000.0, 5200.0, -9000.0)
	_camera.look_at(Vector3(22000.0, 250.0, 22000.0))
	if _camera.has_method("sync_mouse_from_rotation"):
		_camera.call("sync_mouse_from_rotation")

func _build_hud() -> void:
	var layer := CanvasLayer.new()
	add_child(layer)
	_label = Label.new()
	_label.position = Vector2(12, 12)
	_label.text = "rung=%s" % str(_producer.call("rung"))
	layer.add_child(_label)

func _process(_delta: float) -> void:
	if _probe_mode or _view == null or _camera == null:
		return
	_frame += 1
	var p: Vector3 = _camera.global_position
	var v: Vector3 = _camera.call("get_velocity")
	_view.call("update", p.x, p.z, v.x, v.z)

# ---- probe API (mirrors wg10_progression_review.gd; gates depend on these names) ----

func set_probe_mode(enabled: bool) -> void:
	_probe_mode = enabled
	if _label != null:
		_label.visible = not enabled

func set_rung(rung: String) -> bool:
	if not bool(_producer.call("set_rung", rung)):
		return false
	# Re-acquire a clean pool for the new producer path, then re-wire streamer + view.
	if _pool != null and _pool.has_method("free_all"):
		_pool.call("free_all")
	_pool = ClassDB.instantiate("Wg10PagePool")
	var err := str(_producer.call("configure", _pool))
	if err != "":
		push_error("[ladder] reconfigure failed: %s" % err)
		return false
	_runtime.call("configure_streamer", _streamer, _pool)
	_reconfigure_view()
	if _label != null:
		_label.text = "rung=%s" % rung
	return true

func current_rung() -> String:
	return str(_producer.call("rung"))

func pool() -> Object:
	return _pool

# Probe-mode per-frame drive (gates call this instead of letting _process run).
func update_for_probe(px: float, pz: float, vx: float, vz: float) -> void:
	if _view != null:
		_view.call("update", px, pz, vx, vz)

func set_probe_camera_frame(eye: Vector3, look: Vector3) -> void:
	if _camera == null:
		return
	_camera.position = eye
	_camera.look_at(look)
	if _camera.has_method("sync_mouse_from_rotation"):
		_camera.call("sync_mouse_from_rotation")

# debug_tile_states lives on ClipmapRings (verified clipmap_rings.rs:392); proxy to it.
func debug_tile_states() -> PackedInt64Array:
	if _rings != null:
		return _rings.call("debug_tile_states")
	return PackedInt64Array()
