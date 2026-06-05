extends SceneTree

# Windowed gate: proves the ASYNC region-bake WORKER thread (its OWN per-thread RenderingDevice)
# round-trips a real GPU super-region bake. bake_super_region_via_worker spawns a BakeWorker, sends
# TWO small super-region requests (k=2, region_n=33) back-to-back (catches RD-context reuse issues),
# drains both results, and returns the first super-region's region-0 conditioned grid (in metres).
# The DEEP seam/parity checks are cargo-gated; this only proves the worker thread + own-RD GPU bake.

const PRIM     := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE  := "res://worldgen_terrain/shaders/biome_page.glsl"
const FRAGMENT := "res://worldgen_terrain/shaders/biome_mountain.glsl"

const REGION_N       := 33
const K              := 2
const APRON_PX       := 16
const REGION_SPAN_M  := 25600.0
const SPACING        := REGION_SPAN_M / (REGION_N - 1)  # = 800.0
const OX             := 0.0
const OZ             := 0.0
const SEED           := 1
const FEATURE_SPAN   := 90000.0
const HEIGHT_SCALE_M := 260.0
const FLOW_ITERS     := 192
const FLOW_ON        := false

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10BiomePageCompute"):
		push_error("[wg10-bake-worker] Wg10BiomePageCompute not registered")
		return 1

	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-bake-worker] status=skip reason=no-gpu")
		return 2
	probe.free()

	var gpu: Object = ClassDB.instantiate("Wg10BiomePageCompute")
	var err: String = str(gpu.call("load_shaders",
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE)))
	if err != "":
		push_error("[wg10-bake-worker] shader load failed: %s" % err)
		return 1

	var frag_abs: String = ProjectSettings.globalize_path(FRAGMENT)

	# bake_super_region_via_worker bakes 2 super-regions on the worker thread and returns the first
	# super-region's region-0 grid (region_n*region_n, metres).
	var grid: PackedFloat64Array = gpu.call("bake_super_region_via_worker",
		SPACING, REGION_SPAN_M, OX, OZ,
		REGION_N, K, APRON_PX,
		SEED, FEATURE_SPAN, HEIGHT_SCALE_M,
		frag_abs, FLOW_ITERS, FLOW_ON)

	var expect := REGION_N * REGION_N
	if grid.size() != expect:
		push_error("[wg10-bake-worker] status=fail returned size %d, expected %d" % [grid.size(), expect])
		return 1

	# Non-degenerate: finite + not all-zero.
	var all_zero := true
	var any_nan := false
	var vmin := grid[0]
	var vmax := grid[0]
	for i in range(grid.size()):
		var v: float = grid[i]
		if v != v or absf(v) == INF:
			any_nan = true
		if v != 0.0:
			all_zero = false
		if v < vmin:
			vmin = v
		if v > vmax:
			vmax = v

	if any_nan:
		push_error("[wg10-bake-worker] status=fail non-finite value in grid")
		return 1
	if all_zero:
		push_error("[wg10-bake-worker] status=fail grid is all-zero (degenerate)")
		return 1

	print("[wg10-bake-worker] status=pass k=%d regions=%d region_n=%d min=%s max=%s" % [
		K, K * K, REGION_N, str(vmin), str(vmax)])
	return 0
