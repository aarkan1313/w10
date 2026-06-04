extends SceneTree

# Diagnostic for live biome fly morph pairs. The shipped scene uses flow_on for levels
# below FLOW_MAX_LEVEL and flow_off above it, so the L1->L2 morph can blend a carved
# surface into a macro surface. This reports that mismatch directly.

const PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const FRAGMENT := "res://worldgen_terrain/shaders/biome_mountain.glsl"

const PAGE_PX := 256
const APRON_PX := 160
const PADDED_PX := PAGE_PX + 2 * APRON_PX
const SEED := 1337
const FEATURE_SPAN_M := 3500.0
const FLOW_ITERS := 192
const BASE_SPAN := 8192.0
const RELIEF_M := 1000.0

func _init() -> void:
	quit(_run())

func _bake(gpu: Object, origin_x: float, origin_z: float, world_span: float, flow_on: bool) -> PackedFloat64Array:
	var spacing := world_span / float(PAGE_PX - 1)
	return gpu.call("generate_runtime_page_flow",
		spacing, origin_x, origin_z,
		PADDED_PX, PADDED_PX, APRON_PX,
		SEED, FEATURE_SPAN_M,
		ProjectSettings.globalize_path(FRAGMENT),
		FLOW_ITERS,
		flow_on)

func _sample(page: PackedFloat64Array, origin_x: float, origin_z: float, span: float, wx: float, wz: float) -> float:
	var u := (wx - origin_x) / span * float(PAGE_PX - 1)
	var v := (wz - origin_z) / span * float(PAGE_PX - 1)
	u = clampf(u, 0.0, float(PAGE_PX - 1) - 0.0001)
	v = clampf(v, 0.0, float(PAGE_PX - 1) - 0.0001)

	var x0 := int(floor(u))
	var z0 := int(floor(v))
	var x1 := mini(x0 + 1, PAGE_PX - 1)
	var z1 := mini(z0 + 1, PAGE_PX - 1)
	var fx := u - float(x0)
	var fz := v - float(z0)

	var h00: float = page[z0 * PAGE_PX + x0]
	var h10: float = page[z0 * PAGE_PX + x1]
	var h01: float = page[z1 * PAGE_PX + x0]
	var h11: float = page[z1 * PAGE_PX + x1]
	var a: float = lerp(h00, h10, fx)
	var b: float = lerp(h01, h11, fx)
	return lerp(a, b, fz)

func _relief(page: PackedFloat64Array) -> float:
	var hmin := 1.0e30
	var hmax := -1.0e30
	for i in range(page.size()):
		var h: float = page[i] * RELIEF_M
		hmin = minf(hmin, h)
		hmax = maxf(hmax, h)
	return maxf(hmax - hmin, 1.0e-6)

func _compare(gpu: Object, label: String, fine_span: float, fine_flow: bool, parent_flow: bool) -> void:
	var origin := Vector2(0.0, 0.0)
	var parent_span := fine_span * 2.0
	var p0 := _bake(gpu, origin.x, origin.y, fine_span, fine_flow)
	var p1 := _bake(gpu, origin.x, origin.y, parent_span, parent_flow)
	if p0.size() != PAGE_PX * PAGE_PX or p1.size() != PAGE_PX * PAGE_PX:
		print("[wg10-crosslevel-modes] %s status=bad-size fine=%d parent=%d" % [label, p0.size(), p1.size()])
		return

	var sample_n := 64
	var lo := 256.0
	var hi := fine_span - 256.0
	var total := 0.0
	var peak := 0.0
	var peak_x := 0.0
	var peak_z := 0.0
	var diffs: Array = []
	for iz in range(sample_n):
		for ix in range(sample_n):
			var wx: float = lerp(lo, hi, float(ix) / float(sample_n - 1))
			var wz: float = lerp(lo, hi, float(iz) / float(sample_n - 1))
			var h0 := _sample(p0, origin.x, origin.y, fine_span, wx, wz) * RELIEF_M
			var h1 := _sample(p1, origin.x, origin.y, parent_span, wx, wz) * RELIEF_M
			var d := absf(h0 - h1)
			total += d
			diffs.append(d)
			if d > peak:
				peak = d
				peak_x = wx
				peak_z = wz

	diffs.sort()
	var mean := total / float(diffs.size())
	var p95: float = float(diffs[int(floor(float(diffs.size() - 1) * 0.95))])
	var p99: float = float(diffs[int(floor(float(diffs.size() - 1) * 0.99))])
	var relief := _relief(p0)
	print("[wg10-crosslevel-modes] %s fine_span=%d parent_span=%d fine_flow=%s parent_flow=%s relief=%.2f mean=%.2f p95=%.2f p99=%.2f peak=%.2f ratio=%s peak_x=%.2f peak_z=%.2f" % [
		label, int(fine_span), int(parent_span), str(fine_flow), str(parent_flow), relief, mean, p95, p99, peak, str(peak / relief), peak_x, peak_z])

func _run() -> int:
	if not ClassDB.class_exists("Wg10BiomePageCompute"):
		push_error("[wg10-crosslevel-modes] Wg10BiomePageCompute not registered")
		return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-crosslevel-modes] status=skip reason=no-gpu")
		return 2
	probe.free()

	var gpu: Object = ClassDB.instantiate("Wg10BiomePageCompute")
	var err: String = str(gpu.call("load_shaders",
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE)))
	if err != "":
		push_error("[wg10-crosslevel-modes] shader load failed: %s" % err)
		return 1

	_compare(gpu, "L0on_L1on", BASE_SPAN, true, true)
	_compare(gpu, "L1on_L2off", BASE_SPAN * 2.0, true, false)
	_compare(gpu, "L1off_L2off", BASE_SPAN * 2.0, false, false)
	_compare(gpu, "L2off_L3off", BASE_SPAN * 4.0, false, false)
	print("[wg10-crosslevel-modes] status=pass")
	return 0
