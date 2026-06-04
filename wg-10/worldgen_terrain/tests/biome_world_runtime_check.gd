extends SceneTree

# Smoke gate for the grammar-routed WORLD producer. It proves the pool can configure
# all runtime biome contexts, route through biome_runtime_mode=world, acquire a page,
# and write a non-constant texture on the global RenderingDevice.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
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
		print("[wg10-biome-world] status=skip reason=no-render-device")
		return 2

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	print("[wg10-biome-world] step=configure")
	var err: String = str(pool.call("configure_biome_world",
		ProjectSettings.globalize_path(PACK_RES_DIR),
		PACK_FILE,
		CAPACITY, PAGE_PX, APRON_PX, BASE_SPAN, FEATURE_SPAN_M, FLOW_ITERS, 1000.0, 2, SEED))
	if err != "":
		push_error("[wg10-biome-world] configure_biome_world failed: %s" % err)
		return 1

	var runtime_mode := str(pool.call("biome_runtime_mode"))
	var biome_path := bool(pool.call("uses_biome_path"))
	if runtime_mode != "world":
		push_error("[wg10-biome-world] expected runtime=world, got %s" % runtime_mode)
		return 1
	if not biome_path:
		push_error("[wg10-biome-world] uses_biome_path=false after world configure")
		return 1

	var route_counts := {}
	var route_radius := 64
	var route_step := 8
	for ix in range(-route_radius, route_radius + 1, route_step):
		for iz in range(-route_radius, route_radius + 1, route_step):
			var name := str(pool.call(
				"debug_world_biome_for_page",
				0,
				float(ix) * BASE_SPAN,
				float(iz) * BASE_SPAN))
			if name == "":
				push_error("[wg10-biome-world] debug_world_biome_for_page returned empty")
				return 1
			route_counts[name] = int(route_counts.get(name, 0)) + 1
	if route_counts.size() < 2:
		push_error("[wg10-biome-world] world routing collapsed to one biome: %s" % str(route_counts))
		return 1
	print("[wg10-biome-world] routes=%s" % str(route_counts))

	print("[wg10-biome-world] step=acquire")
	var tex = pool.call("acquire_page", 0, 0.0, 0.0)
	if tex == null:
		push_error("[wg10-biome-world] acquire_page returned null")
		return 1
	for i in range(4):
		await process_frame
		RenderingServer.force_draw()
		await process_frame

	print("[wg10-biome-world] step=readback")
	var rd := RenderingServer.get_rendering_device()
	var rid = tex.call("get_texture_rd_rid") if tex.has_method("get_texture_rd_rid") else null
	if rid == null:
		push_error("[wg10-biome-world] cannot get texture RD RID")
		return 1
	var bytes: PackedByteArray = rd.texture_get_data(rid, 0)
	if bytes.is_empty():
		push_error("[wg10-biome-world] texture_get_data returned empty")
		return 1
	var floats := bytes.to_float32_array()
	var mn := 1.0e30
	var mx := -1.0e30
	var nz := 0
	for v in floats:
		mn = minf(mn, v)
		mx = maxf(mx, v)
		if absf(v) > 1.0e-9:
			nz += 1
	pool.call("free_all")
	if nz == 0 or mn == mx:
		push_error("[wg10-biome-world] page is degenerate nonzero=%d min=%f max=%f" % [nz, mn, mx])
		return 1
	print("[wg10-biome-world] status=pass runtime=%s biome_path=%s nonzero=%d min=%f max=%f" % [
		runtime_mode, str(biome_path), nz, mn, mx])
	return 0
