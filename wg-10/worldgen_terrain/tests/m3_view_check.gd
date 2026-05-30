extends SceneTree

# M3 slice 5b gate: drive Wg10TerrainView (3x3 tiling) over a scripted MOVING +x sweep across
# page boundaries; at each non-zero camera position render top-down ortho CENTERED on the
# camera and assert the 3x3 SURROUNDS the camera (full coverage) + seamless + never-black +
# the view triggers ZERO compute (read-only). Proves the slice-5a finding (one page doesn't
# surround) is fixed. WINDOWED. Saves PNGs.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE    := "terrain_pack.gate.json"
const GLSL         := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER       := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const PAGE_PX      := 256
const SEED         := 1337
const NUM_LEVELS   := 3
const BASE_SPAN    := 8192.0
const GRID_RES     := 64
const RADIUS_PAGES := 1
const LEAD_SECONDS := 0.5
const MAX_PER_FRAME := 4
const CAPACITY     := 48        # >= per-level coverage (3 levels x 9 = 27) + stream-ahead headroom
const MORPH_REGION := 0.15
const HEIGHT_SCALE := 0.35
const RELIEF_REF   := 2000.0
const VIEW_SIZE    := Vector2i(512, 512)
const MIN_DISTINCT := 8

# +x sweep incl. a level-0 boundary crossing and non-zero offsets.
const POSITIONS := [0.0, 2048.0, 4096.0, 8192.0, 20000.0]
const VEL_X := 6000.0
const WARM_FRAMES := 24         # let stream-ahead fill the 3x3 of every level before measuring

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10TerrainView"):
		push_error("Wg10TerrainView not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-m3-view] status=skip reason=no-render-device"); return 2

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
		push_error("expected %d tiles, got %s" % [NUM_LEVELS*9, str(rings.call("tile_count"))]); return 1

	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF, LEAD_SECONDS)

	var errs: Array[String] = []

	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.0, 0.0, 0.0)
	var cam := Camera3D.new()
	cam.projection = Camera3D.PROJECTION_ORTHOGONAL
	cam.size = BASE_SPAN * 1.5     # frame ~1.5 level-0 spans: the level-0 3x3 (3*span) more than fills it
	cam.far = BASE_SPAN * 16.0
	cam.environment = env
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-90.0, 0.0, 0.0)
	light.light_energy = 1.2
	vp.add_child(rings)
	vp.add_child(cam)
	vp.add_child(light)
	get_root().add_child(vp)

	var idx := 0
	for pos_x in POSITIONS:
		for _w in range(WARM_FRAMES):
			view.call("update", pos_x, 0.0, VEL_X, 0.0)

		# (zero-compute) Hold the camera static (zero velocity) and let the streamer reach
		# steady state — coverage fully resident, so the STREAMER stops computing. Then snapshot
		# and run more static frames: any further compute could ONLY come from the view (which is
		# read-only and must never compute — the anti-WG9 render-path rule). We detect steady
		# state by created+recomputed going flat across consecutive static frames, then assert it
		# STAYS flat. (Holding static at zero velocity, the streamer's bounded acquires fill the
		# fixed coverage in a few frames; the view, being read-only, adds nothing.)
		var settle := 0
		var prev := -1
		for _h in range(40):
			view.call("update", pos_x, 0.0, 0.0, 0.0)
			var s: Dictionary = pool.call("stats")
			var cc := int(s.get("created", 0)) + int(s.get("recomputed", 0))
			if cc == prev:
				settle += 1
				if settle >= 3:
					break   # steady: 3 consecutive static frames with no compute
			else:
				settle = 0
			prev = cc
		# now assert it stays flat over a few more static frames (the view never computes)
		var ps0: Dictionary = pool.call("stats")
		var c0 := int(ps0.get("created", 0)) + int(ps0.get("recomputed", 0))
		for _h2 in range(4):
			view.call("update", pos_x, 0.0, 0.0, 0.0)
		var ps1: Dictionary = pool.call("stats")
		var c1 := int(ps1.get("created", 0)) + int(ps1.get("recomputed", 0))
		if c1 != c0:
			errs.append("pos %.0f: compute while static after steady state (%d->%d) — view triggered render-path compute (WG9)" % [pos_x, c0, c1])

		cam.look_at_from_position(Vector3(pos_x, BASE_SPAN * 4.0, 0.0), Vector3(pos_x, 0.0, 0.0), Vector3(0.0, 0.0, -1.0))
		for _s in range(6):
			await process_frame
		RenderingServer.force_draw()
		await process_frame

		var img := vp.get_texture().get_image()
		if img == null:
			push_error("get_image null at pos %f" % pos_x); return 1
		img.save_png("user://m3_view_%d.png" % idx)

		# (1) full coverage — the headline fix: the 3x3 SURROUNDS the camera.
		var distinct := {}
		var nonblack := 0
		var total := img.get_width() * img.get_height()
		for y in range(img.get_height()):
			for x in range(img.get_width()):
				var c := img.get_pixel(x, y)
				if not (is_finite(c.r) and is_finite(c.g) and is_finite(c.b)):
					push_error("non-finite pixel @ %d,%d pos %f" % [x,y,pos_x]); return 1
				if c.r > 0.03 or c.g > 0.03 or c.b > 0.03:
					nonblack += 1
				distinct[Vector3i(int(c.r*16), int(c.g*16), int(c.b*16))] = true
		var frac := float(nonblack) / float(total)
		if frac < 0.98:
			errs.append("pos %.0f: NOT surrounded — nonblack=%.3f < 0.98 (3x3 should fill the frame)" % [pos_x, frac])
		if distinct.size() < MIN_DISTINCT:
			errs.append("pos %.0f: no relief distinct=%d" % [pos_x, distinct.size()])

		# (4) no z-fight in the overlap: two settled captures must be pixel-STABLE (flicker = z-fight).
		RenderingServer.force_draw()
		await process_frame
		var img2 := vp.get_texture().get_image()
		var diff := 0
		for y in range(img.get_height()):
			for x in range(img.get_width()):
				var a := img.get_pixel(x, y)
				var b := img2.get_pixel(x, y)
				if abs(a.r-b.r) + abs(a.g-b.g) + abs(a.b-b.b) > 0.05:
					diff += 1
		if diff > total / 50:    # >2% of pixels changed between two settled frames -> flicker
			errs.append("pos %.0f: overlap z-fight/flicker — %d px unstable between settled frames" % [pos_x, diff])

		# (5) never-black + budget
		var ps: Dictionary = pool.call("stats")
		if int(ps.get("resident", 0)) < 1:
			errs.append("pos %.0f: nothing resident" % pos_x)
		if int(ps.get("resident", 0)) > CAPACITY:
			errs.append("pos %.0f: budget exceeded resident %d > %d" % [pos_x, int(ps.get("resident",0)), CAPACITY])
		idx += 1

	# (7) tile<->page mapping (CPU): at cam=0, level 0, tile (1,0) should map to page origin
	# (BASE_SPAN, 0) (center page origin 0 + dx*span). Warm up AT cam=0 first so the streamer
	# makes that page resident (else the view falls back to coarser and the tile keeps a stale
	# key — which is the documented never-black fallback, not a mapping bug). With cam=0 held,
	# the level-0 (1,0) page enters the bounded stream-ahead within a few dozen frames.
	for _m in range(60):
		view.call("update", 0.0, 0.0, 0.0, 0.0)
	var key: Vector2i = rings.call("bound_page_key", 0, 1, 0)
	if key != Vector2i(int(BASE_SPAN), 0):
		errs.append("tile<->page mapping: level0 tile(1,0) -> %s, expected (%d,0) (page may not be resident — check warm-up)" % [str(key), int(BASE_SPAN)])

	pool.call("free_all")

	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m3-view] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-m3-view] status=pass positions=%d tiles=%d" % [POSITIONS.size(), NUM_LEVELS*9])
	return 0
