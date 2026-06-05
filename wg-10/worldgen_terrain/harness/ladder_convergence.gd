extends RefCounted

# Reusable convergence measurement for the un-intercept ladder.
# Reads back a page's R32F height via the PROVEN idiom (acquire/get_resident -> get_texture_rd_rid
# -> rd.texture_get_data -> to_float32_array), with a force_draw() flush because the live biome
# compute is fire-and-forget on the global RD (see biome_runtime_isolate.gd). The caller reads BOTH
# the live rung's page and the reference rung's page over the same (level, origin) and passes both
# arrays to delta(). delta() reports the same shaped metric the offline contract test uses.

# Pure math: compare two flat row-major height arrays (metres). Returns {} on shape mismatch.
func delta(live: PackedFloat32Array, ref: PackedFloat32Array) -> Dictionary:
	if live.size() == 0 or live.size() != ref.size():
		return {}
	var deltas: Array[float] = []
	var total := 0.0
	var live_min := INF
	var live_max := -INF
	var ref_min := INF
	var ref_max := -INF
	for i in range(live.size()):
		var lv := live[i]
		var rv := ref[i]
		var d := absf(lv - rv)
		deltas.append(d)
		total += d
		live_min = minf(live_min, lv); live_max = maxf(live_max, lv)
		ref_min = minf(ref_min, rv); ref_max = maxf(ref_max, rv)
	deltas.sort()
	var n := deltas.size()
	# Non-vacuous: BOTH fields must have real relief (>1 m), else a flat-vs-flat bug could fake a pass.
	var live_relief := live_max - live_min
	var ref_relief := ref_max - ref_min
	return {
		"mean_abs": total / float(n),
		"p95_abs": deltas[clampi(int(floor(float(n - 1) * 0.95)), 0, n - 1)],
		"peak_abs": deltas[n - 1],
		"samples": n,
		"live_relief": live_relief,
		"ref_relief": ref_relief,
		"nonvacuous": live_relief > 1.0 and ref_relief > 1.0,
	}

# Flush the global RD so a fire-and-forget compute page becomes readable (biome_runtime_isolate idiom).
func flush_gpu(tree: SceneTree) -> void:
	for _i in range(4):
		await tree.process_frame
		RenderingServer.force_draw()
		await tree.process_frame

# Read a resident page back as floats (m3_continuity_check._read_page idiom). Empty on miss.
func read_resident_page(rd: RenderingDevice, pool: Object, level: int, ox: float, oz: float, page_px: int) -> PackedFloat32Array:
	var tex: Object = pool.call("get_resident_page", level, ox, oz)
	if tex == null:
		return PackedFloat32Array()
	var rid: RID = tex.call("get_texture_rd_rid")
	if not rid.is_valid():
		return PackedFloat32Array()
	var bytes: PackedByteArray = rd.texture_get_data(rid, 0)
	if bytes.size() < page_px * page_px * 4:
		return PackedFloat32Array()
	return bytes.to_float32_array()

# Acquire (produce) + flush + read one page for the CURRENTLY configured producer.
# This is the one-call "produce a live page and read its heights" the rungs use.
func produce_and_read(tree: SceneTree, rd: RenderingDevice, pool: Object, level: int, ox: float, oz: float, page_px: int) -> PackedFloat32Array:
	var tex = pool.call("acquire_page", level, ox, oz)
	if tex == null:
		return PackedFloat32Array()
	await flush_gpu(tree)
	return await read_resident_page(rd, pool, level, ox, oz, page_px)
