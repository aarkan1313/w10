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
const ROUTE_LOD_LEVELS := 4
const PARENT_SET_RADIUS := 4
const MATERIAL_RUNNER_UP := 0.15

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

	var weight_samples := 0
	var multi_active_pages := 0
	var ambiguous_pages := 0
	var max_active := 0
	var max_runner_up := 0.0
	var mean_runner_up := 0.0
	var mean_top := 0.0
	var weakest_top := 1.0
	var corner_mixed_pages := 0
	var max_corner_mismatches := 0
	var max_probe_active := 0
	var weakest_probe_top := 1.0
	var max_probe_runner_up := 0.0
	for ix in range(-route_radius, route_radius + 1, route_step):
		for iz in range(-route_radius, route_radius + 1, route_step):
			var report = pool.call(
				"debug_world_biome_report_for_page",
				0,
				float(ix) * BASE_SPAN,
				float(iz) * BASE_SPAN)
			if typeof(report) != TYPE_DICTIONARY or report.is_empty():
				push_error("[wg10-biome-world] debug_world_biome_report_for_page returned empty")
				return 1
			var active := int(report.get("active_count", 0))
			var top := float(report.get("selected_weight", 0.0))
			var runner := float(report.get("runner_up_weight", 0.0))
			var corner_mismatches := int(report.get("corner_route_mismatches", 0))
			var probe_active := int(report.get("max_probe_active_count", active))
			var probe_top := float(report.get("min_probe_top_weight", top))
			var probe_runner := float(report.get("max_probe_runner_up_weight", runner))
			weight_samples += 1
			max_active = max(max_active, active)
			mean_top += top
			mean_runner_up += runner
			weakest_top = minf(weakest_top, top)
			max_runner_up = maxf(max_runner_up, runner)
			if active > 1:
				multi_active_pages += 1
			if runner >= MATERIAL_RUNNER_UP:
				ambiguous_pages += 1
			if corner_mismatches > 0:
				corner_mixed_pages += 1
			max_corner_mismatches = max(max_corner_mismatches, corner_mismatches)
			max_probe_active = max(max_probe_active, probe_active)
			weakest_probe_top = minf(weakest_probe_top, probe_top)
			max_probe_runner_up = maxf(max_probe_runner_up, probe_runner)
	if weight_samples == 0:
		push_error("[wg10-biome-world] no route weight samples")
		return 1
	mean_top /= float(weight_samples)
	mean_runner_up /= float(weight_samples)
	print("[wg10-biome-world] route_weights samples=%d multi_active=%d ambiguous=%d max_active=%d mean_top=%f weakest_top=%f mean_runner_up=%f max_runner_up=%f material_runner_up=%f" % [
		weight_samples,
		multi_active_pages,
		ambiguous_pages,
		max_active,
		mean_top,
		weakest_top,
		mean_runner_up,
		max_runner_up,
		MATERIAL_RUNNER_UP,
	])
	if multi_active_pages == 0:
		push_error("[wg10-biome-world] route weights collapsed; no active compose weights visible")
		return 1
	print("[wg10-biome-world] route_inpage corner_mixed=%d max_corner_mismatches=%d max_probe_active=%d weakest_probe_top=%f max_probe_runner_up=%f" % [
		corner_mixed_pages,
		max_corner_mismatches,
		max_probe_active,
		weakest_probe_top,
		max_probe_runner_up,
	])

	var field_report = pool.call(
		"debug_world_biome_weight_field_report_for_page",
		0,
		0.0,
		0.0,
		17)
	if typeof(field_report) != TYPE_DICTIONARY or field_report.is_empty():
		push_error("[wg10-biome-world] debug_world_biome_weight_field_report_for_page returned empty")
		return 1
	var field_delta := float(field_report.get("max_sum_delta", 1.0))
	var field_active := int(field_report.get("active_biomes", 0))
	if field_delta > 1.0e-5 or field_active == 0:
		push_error("[wg10-biome-world] invalid weight field active=%d max_sum_delta=%f" % [
			field_active,
			field_delta,
		])
		return 1
	print("[wg10-biome-world] route_weight_field samples=%d active_biomes=%d max_texel_active=%d min_sum=%f max_sum=%f max_sum_delta=%f" % [
		int(field_report.get("sample_count", 0)),
		field_active,
		int(field_report.get("max_texel_active_count", 0)),
		float(field_report.get("min_sum", 0.0)),
		float(field_report.get("max_sum", 0.0)),
		field_delta,
	])

	var lod_route_samples := 0
	var lod_route_mismatches := 0
	var stable_child_mismatches := 0
	var lod_samples_by_level := []
	var lod_mismatches_by_level := []
	for _level in range(ROUTE_LOD_LEVELS):
		lod_samples_by_level.append(0)
		lod_mismatches_by_level.append(0)
	for ix in range(-route_radius, route_radius + 1, route_step):
		for iz in range(-route_radius, route_radius + 1, route_step):
			var child_ox: float = float(ix) * BASE_SPAN
			var child_oz: float = float(iz) * BASE_SPAN
			var child_name := str(pool.call("debug_world_biome_for_page", 0, child_ox, child_oz))
			var child_report = pool.call("debug_world_biome_report_for_page", 0, child_ox, child_oz)
			var child_corner_mismatches := int(child_report.get("corner_route_mismatches", 0))
			var child_cx: float = child_ox + BASE_SPAN * 0.5
			var child_cz: float = child_oz + BASE_SPAN * 0.5
			for parent_level in range(1, ROUTE_LOD_LEVELS):
				var parent_span: float = BASE_SPAN * pow(2.0, parent_level)
				var parent_ox: float = floor(child_cx / parent_span) * parent_span
				var parent_oz: float = floor(child_cz / parent_span) * parent_span
				var parent_name := str(pool.call("debug_world_biome_for_page", parent_level, parent_ox, parent_oz))
				lod_route_samples += 1
				lod_samples_by_level[parent_level] = int(lod_samples_by_level[parent_level]) + 1
				if parent_name != child_name:
					lod_route_mismatches += 1
					lod_mismatches_by_level[parent_level] = int(lod_mismatches_by_level[parent_level]) + 1
					if child_corner_mismatches == 0:
						stable_child_mismatches += 1
	var lod_route_ratio := float(lod_route_mismatches) / maxf(float(lod_route_samples), 1.0)
	print("[wg10-biome-world] lod_route_mismatch=%d/%d ratio=%f" % [
		lod_route_mismatches,
		lod_route_samples,
		lod_route_ratio,
	])
	var lod_parts := []
	for parent_level in range(1, ROUTE_LOD_LEVELS):
		var level_samples := int(lod_samples_by_level[parent_level])
		var level_mismatches := int(lod_mismatches_by_level[parent_level])
		var level_ratio := float(level_mismatches) / maxf(float(level_samples), 1.0)
		lod_parts.append("L%d=%d/%d(%.6f)" % [
			parent_level,
			level_mismatches,
			level_samples,
			level_ratio,
		])
	print("[wg10-biome-world] lod_route_by_parent %s stable_child_mismatch=%d" % [
		" ".join(lod_parts),
		stable_child_mismatches,
	])

	var parent_child_parents := 0
	var parent_child_mixed := 0
	var parent_child_mismatched_children := 0
	var parent_child_samples := 0
	var parent_route_absent := 0
	var max_child_routes := 0
	var parent_child_parts := []
	for parent_level in range(1, ROUTE_LOD_LEVELS):
		var level_parents := 0
		var level_mixed := 0
		var level_mismatched_children := 0
		var level_child_samples := 0
		var level_parent_absent := 0
		var level_max_child_routes := 0
		var parent_span: float = BASE_SPAN * pow(2.0, parent_level)
		var children_per_axis := int(pow(2.0, parent_level))
		for pix in range(-PARENT_SET_RADIUS, PARENT_SET_RADIUS + 1):
			for piz in range(-PARENT_SET_RADIUS, PARENT_SET_RADIUS + 1):
				var parent_ox: float = float(pix) * parent_span
				var parent_oz: float = float(piz) * parent_span
				var parent_name := str(pool.call("debug_world_biome_for_page", parent_level, parent_ox, parent_oz))
				var child_routes := {}
				var mismatched_children := 0
				for cx in range(children_per_axis):
					for cz in range(children_per_axis):
						var child_ox: float = parent_ox + float(cx) * BASE_SPAN
						var child_oz: float = parent_oz + float(cz) * BASE_SPAN
						var child_name := str(pool.call("debug_world_biome_for_page", 0, child_ox, child_oz))
						child_routes[child_name] = int(child_routes.get(child_name, 0)) + 1
						if child_name != parent_name:
							mismatched_children += 1
				level_parents += 1
				level_child_samples += children_per_axis * children_per_axis
				level_mismatched_children += mismatched_children
				level_max_child_routes = max(level_max_child_routes, child_routes.size())
				if child_routes.size() > 1:
					level_mixed += 1
				if not child_routes.has(parent_name):
					level_parent_absent += 1
		parent_child_parents += level_parents
		parent_child_mixed += level_mixed
		parent_child_mismatched_children += level_mismatched_children
		parent_child_samples += level_child_samples
		parent_route_absent += level_parent_absent
		max_child_routes = max(max_child_routes, level_max_child_routes)
		parent_child_parts.append("L%d=mixed:%d/%d child_mismatch:%d/%d parent_absent:%d max_routes:%d" % [
			parent_level,
			level_mixed,
			level_parents,
			level_mismatched_children,
			level_child_samples,
			level_parent_absent,
			level_max_child_routes,
		])
	print("[wg10-biome-world] route_parent_child parents=%d mixed=%d child_mismatch=%d/%d parent_absent=%d max_child_routes=%d %s" % [
		parent_child_parents,
		parent_child_mixed,
		parent_child_mismatched_children,
		parent_child_samples,
		parent_route_absent,
		max_child_routes,
		" | ".join(parent_child_parts),
	])

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
