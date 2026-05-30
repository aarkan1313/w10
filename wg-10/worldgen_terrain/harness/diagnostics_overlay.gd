extends CanvasLayer
class_name Wg10DiagnosticsOverlay

# Live diagnostics HUD (§6.4): reads fps/frame-p99/mean/max from a Wg10Profiler and resident/
# created/recomputed/full from a terrain view's stats() — both through narrow interfaces
# (bind_sources). Knows nothing about HOW those numbers are produced. Config: update interval,
# font size.

@export var update_interval: float = 0.25
@export var font_size: int = 16

var _profiler: Node = null      # Wg10Profiler
var _view: Object = null        # Wg10TerrainView (has stats())
var _label: Label
var _accum: float = 0.0

func _ready() -> void:
	_label = Label.new()
	_label.position = Vector2(12, 12)
	_label.add_theme_font_size_override("font_size", font_size)
	add_child(_label)

## Wire the data sources (called by the review scene). Narrow interface — no internals.
func bind_sources(profiler: Node, view: Object) -> void:
	_profiler = profiler
	_view = view

func _process(delta: float) -> void:
	_accum += delta
	if _accum < update_interval:
		return
	_accum = 0.0
	var lines := []
	if _profiler != null:
		lines.append("fps %.0f   frame p99 %.2f ms   mean %.2f ms   max %.2f ms" % [
			_profiler.call("fps"), _profiler.call("p99_ms"), _profiler.call("mean_ms"), _profiler.call("max_ms")])
	if _view != null:
		var s: Dictionary = _view.call("stats")
		lines.append("resident %d   created %d   recomputed %d   full %d" % [
			int(s.get("resident", 0)), int(s.get("created", 0)), int(s.get("recomputed", 0)), int(s.get("full_events", 0))])
	_label.text = "\n".join(lines)
