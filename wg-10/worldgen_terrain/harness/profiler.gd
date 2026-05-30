extends Node
class_name Wg10Profiler

# Generic frame-time profiler (§6.4): pushes each frame's delta into a fixed ring buffer and
# exposes p99/mean/max/fps over the captured window, plus a CPU process-time cross-check.
# Attach to any scene; knows nothing about terrain. Config: ring size.
#
# The TOTAL frame delta (p99_ms/mean_ms/max_ms) is the honest 6ms-budget number (CPU+GPU+
# present). cpu_ms() is a secondary diagnostic (Godot's process-time monitor).

@export var ring_size: int = 512

var _ring: PackedFloat32Array = PackedFloat32Array()
var _idx: int = 0
var _count: int = 0

func _ready() -> void:
	_ring.resize(ring_size)

func _process(delta: float) -> void:
	push(delta)

## Clear the captured window (call before a measured run so warm-up frames don't pollute p99).
func reset() -> void:
	_idx = 0
	_count = 0

## Push a frame delta (seconds). The automated gate steps frames explicitly and calls this
## directly; _process also calls it for the interactive scene.
func push(delta: float) -> void:
	if _ring.size() != ring_size:
		_ring.resize(ring_size)
	_ring[_idx] = delta
	_idx = (_idx + 1) % ring_size
	_count = min(_count + 1, ring_size)

func _sorted_window() -> Array:
	var w := []
	for i in range(_count):
		w.append(_ring[i])
	w.sort()
	return w

## p99 frame time in MILLISECONDS over the captured window (0 if empty).
func p99_ms() -> float:
	if _count == 0:
		return 0.0
	var w := _sorted_window()
	var i := int(ceil(0.99 * w.size())) - 1
	i = clamp(i, 0, w.size() - 1)
	return w[i] * 1000.0

func mean_ms() -> float:
	if _count == 0:
		return 0.0
	var s := 0.0
	for i in range(_count):
		s += _ring[i]
	return (s / _count) * 1000.0

func max_ms() -> float:
	if _count == 0:
		return 0.0
	var m := 0.0
	for i in range(_count):
		m = max(m, _ring[i])
	return m * 1000.0

func fps() -> float:
	var mean := mean_ms()
	return 0.0 if mean <= 0.0 else 1000.0 / mean

## CPU process time (ms) from Godot's monitor — a diagnostic split alongside the total frame
## delta. (The total frame delta is the budget number; this just shows the CPU portion.)
func cpu_ms() -> float:
	return Performance.get_monitor(Performance.TIME_PROCESS) * 1000.0
