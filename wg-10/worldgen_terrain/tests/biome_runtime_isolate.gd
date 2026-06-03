extends SceneTree

# ISOLATION test: does the RUNTIME biome producer (multi-pass compute_list on the GLOBAL RD,
# fire-and-forget) actually write a VALID, NON-ZERO page texture? This strips away ALL streaming +
# render machinery: configure_biome -> acquire ONE page -> read the texture back -> check it's not
# all-zero. Decides whether the bug is in the PRODUCER (multi-pass global-RD compute broken) or in
# the downstream wrap/render binding. WINDOWED.
#
# The runtime producer writes via PASS_CROP_IMG -> imageStore on the global RD with no submit/sync
# (the engine submits at draw). To read it back HERE without a draw, we force a frame so the engine
# flushes the queued compute, then texture_get_data the page.

const PRIM := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE := "res://worldgen_terrain/shaders/biome_page.glsl"
const MOUNTAIN := "res://worldgen_terrain/shaders/biome_mountain.glsl"
const APRON_PX := 160
const FEATURE_SPAN_M := 90000.0
const FLOW_ITERS := 192
const PAGE_PX := 256
const BASE_SPAN := 8192.0
const CAPACITY := 16
const SEED := 1337

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-iso] status=skip reason=no-render-device"); return 2
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var err: String = str(pool.call("configure_biome",
		ProjectSettings.globalize_path(PRIM), ProjectSettings.globalize_path(MACHINE),
		ProjectSettings.globalize_path(MOUNTAIN),
		CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, FEATURE_SPAN_M, FLOW_ITERS, 1000.0, 2, SEED))
	if err != "":
		push_error("[wg10-iso] configure_biome failed: %s" % err); return 1
	print("[wg10-iso] configured biome_path=%s" % str(pool.call("uses_biome_path")))

	# acquire ONE page at the origin (level 0). This triggers compute_biome_page_cached on the global RD.
	var tex = pool.call("acquire_page", 0, 0.0, 0.0)
	if tex == null:
		push_error("[wg10-iso] acquire_page returned null (producer failed / pool Full)"); return 1
	print("[wg10-iso] acquire_page returned a texture: %s" % str(tex))

	# Force frames so the engine submits the queued compute work (no manual submit on the global RD).
	for i in range(4):
		await process_frame
		RenderingServer.force_draw()
		await process_frame

	# Read the page texture back off the global RD and check it's not all-zero (the producer wrote it).
	var rd := RenderingServer.get_rendering_device()
	var rid = tex.call("get_texture_rd_rid") if tex.has_method("get_texture_rd_rid") else null
	if rid == null:
		push_error("[wg10-iso] cannot get the texture's RD rid"); return 1
	var bytes: PackedByteArray = rd.texture_get_data(rid, 0)
	if bytes.is_empty():
		push_error("[wg10-iso] texture_get_data returned empty (texture invalid / CAN_COPY_FROM missing?)"); return 1
	# R32F: interpret as floats, find min/max/nonzero count.
	var floats := bytes.to_float32_array()
	var mn := 1.0e30; var mx := -1.0e30; var nz := 0
	for v in floats:
		mn = minf(mn, v); mx = maxf(mx, v)
		if absf(v) > 1.0e-9: nz += 1
	print("[wg10-iso] page R32F: count=%d nonzero=%d min=%f max=%f" % [floats.size(), nz, mn, mx])
	pool.call("free_all")
	if nz == 0:
		push_error("[wg10-iso] status=FAIL the page is ALL ZERO -> the runtime producer did NOT write it (multi-pass global-RD compute broken)")
		return 1
	if mn == mx:
		push_error("[wg10-iso] status=FAIL the page is CONSTANT (%f) -> no terrain structure written" % mn)
		return 1
	print("[wg10-iso] status=pass the runtime producer wrote a VALID non-constant page (min=%f max=%f) -> producer OK, bug is downstream (wrap/render binding)" % [mn, mx])
	return 0
