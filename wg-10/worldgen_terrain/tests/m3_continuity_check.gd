extends SceneTree

# M3 visual-continuity gate (slice 8). Proves the two sampling fixes the owner's fly demanded —
# the timing/non-black gate (m3_accept_check) could not see either, because they are surface
# CONTINUITY defects in a perspective POV, not timing or coverage:
#   (1) HARD: abutting same-level fine pages share their boundary samples to EPS (the texel-corner
#       convention) -> no inter-tile seam. This is a DATA check on the page textures, read back
#       with RenderingDevice.texture_get_data (readback is a GATE-only op, NEVER on the render
#       path). Deterministic and not pixel/flaky — the precise proof the seam is gone.
#   (2) SOFT: under motion the rendered interior has no high-frequency morph banding. The per-tile
#       morph lattice produced many large frame-to-frame luminance jumps along an interior
#       scanline; the corrected neighborhood-center morph produces ~none. Saves a PNG to eyeball.
# WINDOWED (RenderingDevice compute + readback need a device). Prints metrics; returns 2 on skip.

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
const LEAD_SECONDS := 0.5
const MAX_PER_FRAME := 4
const CAPACITY := 48
const HEIGHT_SCALE := 0.35
const MORPH_REGION := 0.15
const RELIEF_REF := 2000.0
const SEAM_EPS := 1.0e-2          # metres; same scale as the parity gates' ABS_EPS
const VIEW_SIZE := Vector2i(960, 540)
const JUMP_THRESH := 0.35         # per-pixel luminance jump that counts as a banding sweep
const JUMP_FRAC_CEIL := 0.05      # at most 5% of interior samples may jump frame-to-frame
const RECOMPUTE_FRAC_CEIL := 0.35 # at most 35% of steady flying frames may recompute a page
                                  # (boundary crossings legitimately recompute; thrashing = ~100%)
const HELD_CHANGE_CEIL := 0.01    # a camera-static frame must change <1% of pixels (else shimmer)

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10TerrainView"):
		push_error("Wg10TerrainView not registered"); return 1
	var rd := RenderingServer.get_rendering_device()
	if rd == null:
		print("[wg10-m3-continuity] status=skip reason=no-render-device"); return 2

	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var err: String = str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if err != "":
		push_error("pool configure failed: %s" % err); return 1
	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_SECONDS, MAX_PER_FRAME)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF, LEAD_SECONDS)

	var errs: Array[String] = []

	# --- Settle the streamer at a fixed position so a full 3x3 of level-0 pages is resident. ---
	var px := 40000.0
	var pz := -25000.0
	for f in range(60):
		view.call("update", px, pz, 0.0, 0.0)
		await process_frame

	# (1) HARD seam check on level 0 — reads back the REAL production page textures (the actual
	# output of height_page.glsl) and asserts that abutting pages' SHARED-EDGE texels are bit-equal.
	# Under the texel-corner convention, page A's last column (i=N-1, world ox+span0) IS page B's
	# first column (i=0, world ox+span0): the same world line -> the same height -> seam_diff=0.
	# Reverting the convention (e.g. back to texel-center) on the generation OR shader side would
	# move those texels off the shared line and the readback values would differ -> this gate fails.
	# This exercises height_page.glsl directly, so it is NOT tautological. Readback needs the page
	# textures' CAN_COPY_FROM bit (added in page_pool::create_page_texture; no render-path cost).
	# texture_get_data blocks until the page is readable — fine for a gate, banned on the hot path.
	var span0 := BASE_SPAN
	var ox: float = floor(px / span0) * span0
	var oz: float = floor(pz / span0) * span0
	var center_data := _read_page(rd, pool, 0, ox, oz)
	var east_data := _read_page(rd, pool, 0, ox + span0, oz)
	var north_data := _read_page(rd, pool, 0, ox, oz + span0)
	var max_east := -1.0
	var max_north := -1.0
	if center_data.is_empty():
		errs.append("seam: center page (%.0f,%.0f) not resident/readable — cannot test the seam" % [ox, oz])
	else:
		if east_data.is_empty():
			errs.append("seam: east page not resident/readable — cannot test EAST seam")
		else:
			# center's last column (x=N-1) vs east's first column (x=0), all rows z.
			max_east = _max_col_diff(center_data, PAGE_PX - 1, east_data, 0)
			if max_east > SEAM_EPS:
				errs.append("seam EAST: max shared-column readback diff %.6f m > %.4f" % [max_east, SEAM_EPS])
		if north_data.is_empty():
			errs.append("seam: north page not resident/readable — cannot test NORTH seam")
		else:
			# center's last row (z=N-1) vs north's first row (z=0), all cols x.
			max_north = _max_row_diff(center_data, PAGE_PX - 1, north_data, 0)
			if max_north > SEAM_EPS:
				errs.append("seam NORTH: max shared-row readback diff %.6f m > %.4f" % [max_north, SEAM_EPS])

	# Build a perspective flight rig (the POV the owner actually flies — the top-down/settled
	# checks missed the live defects).
	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.45, 0.62, 0.85)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	var cam := Camera3D.new()
	cam.far = BASE_SPAN * 32.0
	cam.environment = env
	vp.add_child(rings)
	vp.add_child(light)
	vp.add_child(cam)
	get_root().add_child(vp)

	var pos := Vector2(px, pz)
	var heading := Vector2(0.7, 0.7)
	var dt := 1.0 / 60.0
	var speed := 800.0

	# (2) CONTINUOUS-FLIGHT churn check (the owner's HUD showed `recomputed` climbing every
	# frame = the pool thrashing because the camera's own ring wasn't maintained). Fly steadily
	# for a long run; after warm-up, count how many of the flying frames triggered ANY page
	# recompute. A boundary-cross legitimately recomputes a few pages, but the steady state must
	# NOT recompute on most frames. We measure recomputes-per-measured-frame and cap the fraction
	# of frames that recompute. (Pre-fix: ~100%.)
	var prev_line := PackedFloat32Array()
	var big_jumps := 0
	var samples := 0
	var recompute_frames := 0
	var measured := 0
	for f in range(200):
		var vx := heading.x * speed
		var vz := heading.y * speed
		pos += Vector2(vx, vz) * dt
		var st_before: Dictionary = pool.call("stats")
		var rc_before := int(st_before.get("recomputed", 0)) + int(st_before.get("created", 0))
		view.call("update", pos.x, pos.y, vx, vz)
		var eye := Vector3(pos.x - heading.x * 600.0, 900.0, pos.y - heading.y * 600.0)
		var look := Vector3(pos.x + heading.x * 1200.0, 0.0, pos.y + heading.y * 1200.0)
		cam.look_at_from_position(eye, look, Vector3.UP)
		await process_frame
		RenderingServer.force_draw()
		await process_frame
		if f >= 60:
			measured += 1
			var st_after: Dictionary = pool.call("stats")
			var rc_after := int(st_after.get("recomputed", 0)) + int(st_after.get("created", 0))
			if rc_after > rc_before:
				recompute_frames += 1
			var img: Image = vp.get_texture().get_image()
			if img != null:
				if f == 60:
					img.save_png("user://m3_continuity.png")
				var y := int(img.get_height() * 0.7)   # interior terrain scanline (below horizon)
				var line := PackedFloat32Array()
				for x in range(0, img.get_width(), 4):
					var c := img.get_pixel(x, y)
					line.append(c.r * 0.3 + c.g * 0.6 + c.b * 0.1)
				if prev_line.size() == line.size():
					for i in range(line.size()):
						samples += 1
						if absf(line[i] - prev_line[i]) > JUMP_THRESH:
							big_jumps += 1
				prev_line = line

	var recompute_frac := float(recompute_frames) / float(max(measured, 1))
	if recompute_frac > RECOMPUTE_FRAC_CEIL:
		errs.append("page churn: %.3f of flying frames recomputed a page (>%.2f) — pool thrashing" % [recompute_frac, RECOMPUTE_FRAC_CEIL])
	var jump_frac := float(big_jumps) / float(max(samples, 1))
	if jump_frac > JUMP_FRAC_CEIL:
		errs.append("morph banding: %.3f of interior samples jumped >%.2f frame-to-frame (ceil %.2f)" % [jump_frac, JUMP_THRESH, JUMP_FRAC_CEIL])

	# (3) HOLD-STILL pixel stability (the owner: "changes while staying in the same chunk"). Stop
	# dead, let streaming settle, then hold for many frames and assert the rendered frame does NOT
	# change frame-to-frame. A flip-flop here = tiles oscillating between fine and a (now-correct)
	# fallback, or a page evicting/recomputing while stationary. Pre-fix this shimmered.
	for _w in range(40):
		view.call("update", pos.x, pos.y, 0.0, 0.0)
		await process_frame
		RenderingServer.force_draw()
		await process_frame
	# Lower, near-ground POV looking along the surface — frames inter-tile/level boundaries the
	# way the owner's in-game camera does (the high overview hid the "clear squares").
	cam.look_at_from_position(
		Vector3(pos.x - heading.x * 400.0, 350.0, pos.y - heading.y * 400.0),
		Vector3(pos.x + heading.x * 2000.0, 100.0, pos.y + heading.y * 2000.0), Vector3.UP)
	var held_prev: Image = null
	var held_changed := 0
	var held_frames := 0
	for _h in range(16):
		view.call("update", pos.x, pos.y, 0.0, 0.0)
		await process_frame
		RenderingServer.force_draw()
		await process_frame
		var himg: Image = vp.get_texture().get_image()
		if himg != null and _h == 15:
			himg.save_png("user://m3_continuity_held.png")
		if himg != null and held_prev != null:
			held_frames += 1
			# Count pixels that changed meaningfully between two held (camera-static) frames.
			var changed := 0
			var checked := 0
			var hy0 := int(himg.get_height() / 3)
			for hy in range(hy0, himg.get_height(), 6):
				for hx in range(0, himg.get_width(), 6):
					checked += 1
					var ca := himg.get_pixel(hx, hy)
					var cb := held_prev.get_pixel(hx, hy)
					if absf(ca.r - cb.r) + absf(ca.g - cb.g) + absf(ca.b - cb.b) > 0.04:
						changed += 1
			if float(changed) / float(max(checked, 1)) > HELD_CHANGE_CEIL:
				held_changed += 1
		held_prev = himg
	if held_changed > 0:
		errs.append("hold-still shimmer: %d/%d static frames changed >%.0f%% pixels — tiles oscillating while stationary" % [held_changed, held_frames, HELD_CHANGE_CEIL * 100.0])

	pool.call("free_all")

	print("[wg10-m3-continuity] seam_e=%.5f seam_n=%.5f recompute_frac=%.3f (ceil %.2f) morph_jump=%.3f held_changed=%d/%d" % [
		max_east, max_north, recompute_frac, RECOMPUTE_FRAC_CEIL, jump_frac, held_changed, held_frames])
	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m3-continuity] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-m3-continuity] status=pass")
	return 0

# Read a resident R32F page back as a PackedFloat32Array (row-major, PAGE_PX*PAGE_PX). Empty if
# the page is not resident or unreadable. GATE-only readback (texture_get_data blocks the GPU;
# never on the render path). Needs the page texture's CAN_COPY_FROM bit.
func _read_page(rd: RenderingDevice, pool: Object, level: int, ox: float, oz: float) -> PackedFloat32Array:
	var tex: Object = pool.call("get_resident_page", level, ox, oz)
	if tex == null:
		return PackedFloat32Array()
	var rid: RID = tex.call("get_texture_rd_rid")
	if not rid.is_valid():
		return PackedFloat32Array()
	var bytes: PackedByteArray = rd.texture_get_data(rid, 0)
	if bytes.size() < PAGE_PX * PAGE_PX * 4:
		return PackedFloat32Array()
	return bytes.to_float32_array()

# Max |diff| between column `ax` of page `a` and column `bx` of page `b`, over all rows (z).
func _max_col_diff(a: PackedFloat32Array, ax: int, b: PackedFloat32Array, bx: int) -> float:
	var m := 0.0
	for row in range(PAGE_PX):
		m = maxf(m, absf(a[row * PAGE_PX + ax] - b[row * PAGE_PX + bx]))
	return m

# Max |diff| between row `az` of page `a` and row `bz` of page `b`, over all columns (x).
func _max_row_diff(a: PackedFloat32Array, az: int, b: PackedFloat32Array, bz: int) -> float:
	var m := 0.0
	for col in range(PAGE_PX):
		m = maxf(m, absf(a[az * PAGE_PX + col] - b[bz * PAGE_PX + col]))
	return m
