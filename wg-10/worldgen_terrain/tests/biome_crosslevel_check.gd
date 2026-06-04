extends SceneTree

# Scale-invariance gate: bake level 0 and level 1 mountain MACRO pages over the
# same world region with flow_on=false, sample both at identical world XZ points,
# and assert the parent page is a low-frequency version of the child rather than
# a different surface. This is the "did we kill the 73% morph warp?" proof.

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

# Hard ceiling for peak cross-level macro mismatch. The failing pre-fix warp was ~0.73.
# The post scale-invariant runtime measures ~0.0667 peak / ~0.0033 mean; 0.08 keeps a
# tight regression tripwire while allowing the parent bilinear sample to miss one fine
# local peak on an arbitrary 64x64 probe grid.
const CROSS_EPS := 0.08

func _init() -> void:
	quit(_run())

func _bake_macro(gpu: Object, origin_x: float, origin_z: float, world_span: float) -> PackedFloat64Array:
	var spacing := world_span / float(PAGE_PX - 1)
	return gpu.call("generate_runtime_page_flow",
		spacing, origin_x, origin_z,
		PADDED_PX, PADDED_PX, APRON_PX,
		SEED, FEATURE_SPAN_M,
		ProjectSettings.globalize_path(FRAGMENT),
		FLOW_ITERS,
		false)

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

func _run() -> int:
	if not ClassDB.class_exists("Wg10BiomePageCompute"):
		push_error("[wg10-crosslevel] Wg10BiomePageCompute not registered")
		return 1

	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-crosslevel] status=skip reason=no-gpu")
		return 2
	probe.free()

	var gpu: Object = ClassDB.instantiate("Wg10BiomePageCompute")
	var err: String = str(gpu.call("load_shaders",
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE)))
	if err != "":
		push_error("[wg10-crosslevel] shader load failed: %s" % err)
		return 1

	var l0_span := BASE_SPAN
	var l1_span := BASE_SPAN * 2.0
	var origin := Vector2(0.0, 0.0)

	var p0 := _bake_macro(gpu, origin.x, origin.y, l0_span)
	var p1 := _bake_macro(gpu, origin.x, origin.y, l1_span)
	if p0.size() != PAGE_PX * PAGE_PX or p1.size() != PAGE_PX * PAGE_PX:
		push_error("[wg10-crosslevel] bake returned wrong size l0=%d l1=%d" % [p0.size(), p1.size()])
		return 1

	var hmin := 1.0e30
	var hmax := -1.0e30
	for i in range(p0.size()):
		var hm: float = p0[i] * RELIEF_M
		hmin = minf(hmin, hm)
		hmax = maxf(hmax, hm)
	var relief := maxf(hmax - hmin, 1.0e-6)

	var sample_n := 64
	var lo := 256.0
	var hi := l0_span - 256.0
	var total := 0.0
	var peak := 0.0
	var peak_x := 0.0
	var peak_z := 0.0
	var cnt := 0
	var diffs: Array = []
	for iz in range(sample_n):
		for ix in range(sample_n):
			var wx: float = lerp(lo, hi, float(ix) / float(sample_n - 1))
			var wz: float = lerp(lo, hi, float(iz) / float(sample_n - 1))
			var h0 := _sample(p0, origin.x, origin.y, l0_span, wx, wz) * RELIEF_M
			var h1 := _sample(p1, origin.x, origin.y, l1_span, wx, wz) * RELIEF_M
			var d := absf(h0 - h1)
			total += d
			diffs.append(d)
			if d > peak:
				peak = d
				peak_x = wx
				peak_z = wz
			cnt += 1

	var mean := total / float(cnt)
	var ratio := peak / relief
	diffs.sort()
	var p95: float = float(diffs[int(floor(float(diffs.size() - 1) * 0.95))])
	var p99: float = float(diffs[int(floor(float(diffs.size() - 1) * 0.99))])
	print("[wg10-crosslevel] l0_span=%d l1_span=%d flow_on=false samples=%d" % [int(l0_span), int(l1_span), cnt])
	print("[wg10-crosslevel] relief_m=%.2f mean_abs_m=%.2f p95_abs_m=%.2f p99_abs_m=%.2f peak_abs_m=%.2f peak_x=%.2f peak_z=%.2f ratio=%s eps=%s" % [relief, mean, p95, p99, peak, peak_x, peak_z, str(ratio), str(CROSS_EPS)])
	if ratio > CROSS_EPS:
		print("[wg10-crosslevel] status=fail macro agreement ratio=%s > eps=%s" % [str(ratio), str(CROSS_EPS)])
		return 1
	print("[wg10-crosslevel] status=pass macro agreement ratio=%s <= eps=%s" % [str(ratio), str(CROSS_EPS)])
	return 0
