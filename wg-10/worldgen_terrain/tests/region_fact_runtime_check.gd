extends SceneTree

# Windowed Rung-1 gate: proves the RegionFact PRODUCER path puts a carved, non-black baked-look page
# on screen via the off-frame super-region bake WORKER. It strips render/streaming machinery to the
# bone: a BARE Wg10PagePool (no camera/viewport), configure_region_fact -> acquire a page in region
# (0,0) and re-acquire in a loop while pumping process_frames (the pool drains finished bakes on each
# acquire tick) until the resident page upgrades from the flat fallback to the carved fact -> read it
# back off the global RD. Two checks: (1) the baked page is NON-DEGENERATE (finite, not all-zero,
# not constant); (2) the SEAM proof - two ADJACENT pages across an internal super-region region
# border AGREE at the shared world edge (the facts tile seam-exact).
#
# Deep carve/condition parity is already cargo-gated; this only proves the live producer wiring.

const PACK_JSON := "res://worldgen_terrain/packs/dem_v1/terrain_pack.gate.json"
const PRIM      := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE   := "res://worldgen_terrain/shaders/biome_page.glsl"
const FRAGMENT  := "res://worldgen_terrain/shaders/biome_mountain.glsl"

const REGION_N       := 33
const K              := 2
const APRON_PX       := 16
const SEED           := 1
const FEATURE_SPAN   := 90000.0
const HEIGHT_SCALE_M := 260.0
const FLOW_ITERS     := 192
const FLOW_ON        := false
const PAGE_PX        := 64
# A SMALL region span keeps the gate fast (the pack's region_size_m is overridden to match so the
# sliced region facts tile on the region grid).
const REGION_SPAN_M  := 1600.0
const MAX_FRAMES     := 90

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-region-rung1] status=skip reason=no-gpu"); return 2
	if not ClassDB.class_exists("Wg10PagePool"):
		push_error("[wg10-region-rung1] Wg10PagePool not registered"); return 1

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var err: String = str(pool.call("configure_region_fact",
		ProjectSettings.globalize_path(PACK_JSON),
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE),
		ProjectSettings.globalize_path(FRAGMENT),
		REGION_N, K, APRON_PX, SEED,
		FEATURE_SPAN, HEIGHT_SCALE_M, FLOW_ITERS, FLOW_ON,
		PAGE_PX, REGION_SPAN_M))
	if err != "":
		push_error("[wg10-region-rung1] configure_region_fact failed: %s" % err); return 1

	# Page (0,0): acquire + pump frames until the worker's bake lands (the pool drains on each
	# acquire). The first acquire writes the flat fallback; later acquires upgrade to the carved fact.
	var floats0 := await _acquire_until_carved(pool, 0.0, 0.0)
	if floats0.is_empty():
		push_error("[wg10-region-rung1] status=fail region (0,0) page never upgraded past flat fallback in %d frames" % MAX_FRAMES)
		pool.call("free_all"); return 1

	# Non-degenerate check.
	var mn := 1.0e30; var mx := -1.0e30; var nz := 0
	for v in floats0:
		if v != v or absf(v) == INF:
			push_error("[wg10-region-rung1] status=fail non-finite value in baked page"); pool.call("free_all"); return 1
		mn = minf(mn, v); mx = maxf(mx, v)
		if absf(v) > 1.0e-9: nz += 1
	if nz == 0:
		push_error("[wg10-region-rung1] status=fail baked page is all-zero (degenerate)"); pool.call("free_all"); return 1
	if mn == mx:
		push_error("[wg10-region-rung1] status=fail baked page is constant (%f)" % mn); pool.call("free_all"); return 1

	# SEAM proof: page at x=REGION_SPAN_M is the ADJACENT region across an internal super-region
	# border (region 0 vs region 1, both inside super (0,0) for K=2). The pages must AGREE along the
	# shared world edge x=REGION_SPAN_M: page0's rightmost column == page1's leftmost column.
	var floats1 := await _acquire_until_carved(pool, REGION_SPAN_M, 0.0)
	if floats1.is_empty():
		push_error("[wg10-region-rung1] status=fail adjacent region (1,0) page never baked"); pool.call("free_all"); return 1

	var seam_max := 0.0
	for row in range(PAGE_PX):
		var right := floats0[row * PAGE_PX + (PAGE_PX - 1)]  # page0 right edge (world x=REGION_SPAN_M)
		var left  := floats1[row * PAGE_PX + 0]              # page1 left edge  (world x=REGION_SPAN_M)
		seam_max = maxf(seam_max, absf(right - left))

	# The facts tile by construction (texel-corner slicing); the shared edge should match to f32.
	var SEAM_BAR := 1.0e-3
	if seam_max > SEAM_BAR:
		push_error("[wg10-region-rung1] status=fail seam mismatch %f > %f at internal super-region border" % [seam_max, SEAM_BAR])
		pool.call("free_all"); return 1

	print("[wg10-region-rung1] status=pass region_n=%d k=%d span=%s nonzero=%d min=%s max=%s seam_max=%s" % [
		REGION_N, K, str(REGION_SPAN_M), nz, str(mn), str(mx), str(seam_max)])
	pool.call("free_all")
	return 0

# Acquire (level 0) the page at world (ox,oz) repeatedly, pumping a process_frame between each so the
# worker thread can finish its bake and the pool can drain it. Returns the page floats once it is
# carved (non-flat); empty array if it never upgrades within MAX_FRAMES.
func _acquire_until_carved(pool: Object, ox: float, oz: float) -> PackedFloat32Array:
	for frame in range(MAX_FRAMES):
		var tex = pool.call("acquire_page", 0, ox, oz)
		if tex == null:
			push_error("[wg10-region-rung1] acquire_page returned null at (%s,%s)" % [str(ox), str(oz)])
			return PackedFloat32Array()
		# Let the engine submit queued GPU work and give the worker thread time to bake + the next
		# acquire a chance to drain it.
		await process_frame
		RenderingServer.force_draw()
		await process_frame

		var floats := _readback(tex)
		if not floats.is_empty() and not _is_flat(floats):
			return floats
		# release so the next acquire re-dispatches (now that the region may be cached).
		pool.call("release_page", 0, ox, oz)
	return PackedFloat32Array()

func _readback(tex) -> PackedFloat32Array:
	var rd := RenderingServer.get_rendering_device()
	var rid = tex.call("get_texture_rd_rid") if tex.has_method("get_texture_rd_rid") else null
	if rid == null:
		return PackedFloat32Array()
	var bytes: PackedByteArray = rd.texture_get_data(rid, 0)
	if bytes.is_empty():
		return PackedFloat32Array()
	return bytes.to_float32_array()

# Flat == all (near) zero: the not-yet-baked flat fallback.
func _is_flat(floats: PackedFloat32Array) -> bool:
	for v in floats:
		if absf(v) > 1.0e-9:
			return false
	return true
