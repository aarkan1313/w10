extends Camera3D
class_name Wg10FlyCamera

# Free-fly camera+movement rig (§6.4): WASD horizontal, Space/C vertical, mouse look (while
# captured; ESC releases), Shift speed boost. Exposes position (the node's global_position) +
# get_velocity() each frame. Config-driven (no magic numbers). Knows nothing about terrain —
# the review scene feeds pos/vel to Wg10TerrainView.update.

@export var move_speed: float = 2000.0       # m/s base (x sprint reaches ~1000s of m/s)
@export var sprint_mult: float = 4.0
@export var vertical_speed: float = 1500.0    # m/s for Space/C
@export var mouse_sensitivity: float = 0.0025
@export var capture_mouse: bool = true

var _velocity: Vector3 = Vector3.ZERO
var _yaw: float = 0.0
var _pitch: float = 0.0

func _ready() -> void:
	if capture_mouse:
		Input.mouse_mode = Input.MOUSE_MODE_CAPTURED
	_yaw = rotation.y
	_pitch = rotation.x

func _input(event: InputEvent) -> void:
	if event is InputEventMouseMotion and Input.mouse_mode == Input.MOUSE_MODE_CAPTURED:
		_yaw -= event.relative.x * mouse_sensitivity
		_pitch = clamp(_pitch - event.relative.y * mouse_sensitivity, -1.5, 1.5)
		rotation = Vector3(_pitch, _yaw, 0.0)
	elif event is InputEventKey and event.pressed and event.keycode == KEY_ESCAPE:
		Input.mouse_mode = Input.MOUSE_MODE_VISIBLE

func _process(delta: float) -> void:
	var dir := Vector3.ZERO
	var b := global_transform.basis
	if Input.is_key_pressed(KEY_W):
		dir -= b.z
	if Input.is_key_pressed(KEY_S):
		dir += b.z
	if Input.is_key_pressed(KEY_A):
		dir -= b.x
	if Input.is_key_pressed(KEY_D):
		dir += b.x
	var up := 0.0
	if Input.is_key_pressed(KEY_SPACE):
		up += 1.0
	if Input.is_key_pressed(KEY_C):
		up -= 1.0
	var speed := move_speed * (sprint_mult if Input.is_key_pressed(KEY_SHIFT) else 1.0)
	var step := dir.normalized() * speed * delta + Vector3.UP * (up * vertical_speed * delta)
	if delta > 0.0:
		_velocity = step / delta
	else:
		_velocity = Vector3.ZERO
	global_position += step

## Current velocity (m/s, world space) — the review scene passes this to view.update.
func get_velocity() -> Vector3:
	return _velocity
