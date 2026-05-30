extends SceneTree

# B2 capacity-pressure gate: prove the live terrain view never recycles a visible
# coarsest page under a tight pool budget. The first coarse-boundary frame should
# go Full and HOLD last-good displayed pages; subsequent frames should catch up
# once no-longer-displayed pages become evictable again. WINDOWED.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const PAGE_PX := 256
const SEED := 1337
const NUM_LEVELS := 3
const BASE_SPAN := 8192.0
const GRID_RES := 64
const RADIUS_PAGES := 1
const LEAD_SECONDS := 0.0
const MAX_PER_FRAME := 3
const CAPACITY := 9
const MORPH_REGION := 0.15
const RELIEF_SCALE := 0.25
const RELIEF_REF := 2000.0
const VIEW_SIZE := Vector2i(256, 256)
const WARM_FRAMES := 12
const CATCHUP_FRAMES := 8

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10TerrainView"):
		push_error("Wg10TerrainView not registered"); return 1
	if not ClassDB.class_exists("Wg10PagePool"):
		push_error("Wg10PagePool not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-m3-b2-capacity] status=skip reason=no-render-device"); return 2

	var pack_os: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os: String = ProjectSettings.globalize_path(GLSL)

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var cfg_err: String = str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if cfg_err != "":
		push_error("pool configure failed: %s" % cfg_err); return 1

	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_SECONDS, MAX_PER_FRAME)

	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	if int(rings.call("tile_count")) != NUM_LEVELS * 9:
		push_error("expected %d tiles, got %s" % [NUM_LEVELS * 9, str(rings.call("tile_count"))]); return 1

	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, RELIEF_SCALE, MORPH_REGION, RELIEF_REF, LEAD_SECONDS)

	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_DISABLED
	vp.own_world_3d = true
	vp.add_child(rings)
	get_root().add_child(vp)

	var errs: Array[String] = []
	var coarse_span := BASE_SPAN * pow(2.0, NUM_LEVELS - 1)
	var old_x := 0.0
	var new_x := coarse_span + 1.0

	for _i in range(WARM_FRAMES):
		view.call("update", old_x, 0.0, 0.0, 0.0)

	var warm := _coarsest_state(rings, pool, old_x, 0.0)
	if int(warm.get("visible", 0)) != 9:
		errs.append("warm-up: coarsest visible=%d, expected 9" % int(warm.get("visible", 0)))
	if int(warm.get("unpinned", 0)) != 0:
		errs.append("warm-up: visible coarsest pages not display-pinned=%d" % int(warm.get("unpinned", 0)))
	if int(warm.get("unbound", 0)) != 0:
		errs.append("warm-up: visible coarsest pages unbound=%d" % int(warm.get("unbound", 0)))
	if int(warm.get("held", 0)) != 0:
		errs.append("warm-up: coarsest held=%d at origin, expected 0" % int(warm.get("held", 0)))

	var before: Dictionary = pool.call("stats")
	var full_before := int(before.get("full_events", 0))

	view.call("update", new_x, 0.0, 0.0, 0.0)
	var pressure := _coarsest_state(rings, pool, new_x, 0.0)
	var after_pressure: Dictionary = pool.call("stats")
	var full_delta := int(after_pressure.get("full_events", 0)) - full_before

	if full_delta <= 0:
		errs.append("pressure frame was vacuous: full_events delta=%d" % full_delta)
	if int(pressure.get("visible", 0)) != 9:
		errs.append("pressure frame: coarsest visible=%d, expected 9" % int(pressure.get("visible", 0)))
	if int(pressure.get("unpinned", 0)) != 0:
		errs.append("pressure frame: visible coarsest pages not display-pinned=%d" % int(pressure.get("unpinned", 0)))
	if int(pressure.get("unbound", 0)) != 0:
		errs.append("pressure frame: visible coarsest pages unbound=%d" % int(pressure.get("unbound", 0)))
	if int(pressure.get("held", 0)) <= 0:
		errs.append("pressure frame did not exercise coarsest hold-last-good")

	var catchup := pressure
	for _f in range(CATCHUP_FRAMES):
		view.call("update", new_x, 0.0, 0.0, 0.0)
		catchup = _coarsest_state(rings, pool, new_x, 0.0)
		if int(catchup.get("visible", 0)) == 9 and int(catchup.get("unpinned", 0)) == 0 and int(catchup.get("held", 0)) == 0:
			break
	if int(catchup.get("held", 0)) != 0:
		errs.append("coarsest ring did not catch up after %d frames; held=%d" % [CATCHUP_FRAMES, int(catchup.get("held", 0))])
	if int(catchup.get("visible", 0)) != 9:
		errs.append("catch-up: coarsest visible=%d, expected 9" % int(catchup.get("visible", 0)))
	if int(catchup.get("unpinned", 0)) != 0:
		errs.append("catch-up: visible coarsest pages not display-pinned=%d" % int(catchup.get("unpinned", 0)))
	if int(catchup.get("unbound", 0)) != 0:
		errs.append("catch-up: visible coarsest pages unbound=%d" % int(catchup.get("unbound", 0)))

	var final_stats: Dictionary = pool.call("stats")
	if int(final_stats.get("resident", 0)) > CAPACITY:
		errs.append("budget exceeded: resident %d > %d" % [int(final_stats.get("resident", 0)), CAPACITY])

	pool.call("free_all")

	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m3-b2-capacity] status=fail errors=%d full_delta=%d pressure_held=%d catchup_held=%d stats=%s" % [
			errs.size(),
			full_delta,
			int(pressure.get("held", 0)),
			int(catchup.get("held", 0)),
			str(final_stats),
		])
		return 1

	print("[wg10-m3-b2-capacity] status=pass full_delta=%d pressure_held=%d resident=%d" % [
		full_delta,
		int(pressure.get("held", 0)),
		int(final_stats.get("resident", 0)),
	])
	return 0

func _coarsest_state(rings: Object, pool: Object, camera_x: float, camera_z: float) -> Dictionary:
	var level := NUM_LEVELS - 1
	var span: float = BASE_SPAN * pow(2.0, level)
	var center_x: float = floor(camera_x / span) * span
	var center_z: float = floor(camera_z / span) * span
	var states: PackedInt64Array = rings.call("debug_tile_states")
	var visible := 0
	var held := 0
	var unpinned := 0
	var unbound := 0

	for dz in range(-1, 2):
		for dx in range(-1, 2):
			var idx := _tile_state_index(level, dx, dz)
			var is_visible := int(states[idx]) == 1
			if not is_visible:
				continue
			visible += 1
			var ox := int(states[idx + 1])
			var oz := int(states[idx + 2])
			if ox == -9223372036854775808 or oz == -9223372036854775808:
				unbound += 1
				continue
			var expected_x := int(center_x + float(dx) * span)
			var expected_z := int(center_z + float(dz) * span)
			if ox != expected_x or oz != expected_z:
				held += 1
			if not bool(pool.call("is_displayed_pinned", level, float(ox), float(oz))):
				unpinned += 1

	return {
		"visible": visible,
		"held": held,
		"unpinned": unpinned,
		"unbound": unbound,
	}

func _tile_state_index(level: int, dx: int, dz: int) -> int:
	return (level * 9 + (dz + 1) * 3 + (dx + 1)) * 3
