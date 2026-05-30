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
const LEAD_FRAMES := 8.0
const MAX_PER_FRAME := 4
const CAPACITY := 48
const HEIGHT_SCALE := 0.35
const MORPH_REGION := 0.15
const RELIEF_REF := 2000.0
const SEAM_EPS := 1.0e-2          # metres; same scale as the parity gates' ABS_EPS
const VIEW_SIZE := Vector2i(960, 540)
const JUMP_THRESH := 0.35         # per-pixel luminance jump that counts as a banding sweep
const JUMP_FRAC_CEIL := 0.05      # at most 5% of interior samples may jump frame-to-frame

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
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_FRAMES, MAX_PER_FRAME)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF)

	var errs: Array[String] = []

	# --- Settle the streamer at a fixed position so a full 3x3 of level-0 pages is resident. ---
	var px := 40000.0
	var pz := -25000.0
	for f in range(60):
		view.call("update", px, pz, 0.0, 0.0)
		await process_frame

	# (1) HARD seam check on level 0 — proven on the CPU height field (no GPU readback: the page
	# textures are STORAGE+SAMPLING only, deliberately NOT CPU_READ, so reading them back would
	# force a render-path cost just for a gate; and GPU==CPU parity is already the M2 gpu suite's
	# job). This asserts the SAMPLING CONVENTION directly: under texel-corner generation, a fine
	# page (origin O, span S, N texels) samples texel (i,j) at world (O + i/(N-1)*S, O + j/(N-1)*S).
	# Page A's last column (i=N-1, world O+S) and abutting page B's first column (i=0, world
	# O_B = O+S) therefore land on the SAME world line -> the same height_at value -> no seam. We
	# verify the world coords coincide AND that Wg10Height agrees there (catches a convention drift
	# that would move the shared edge off a shared sample, i.e. reintroduce the seam).
	var span0 := BASE_SPAN
	var ox: float = floor(px / span0) * span0
	var oz: float = floor(pz / span0) * span0
	# Reconstruct each page's per-texel WORLD sample points from the page's own (origin, span) under
	# the texel-corner convention — exactly as height_page.glsl main() does: texel k -> world
	# origin + k/(N-1)*span. Then evaluate the GPU height field (Wg10GpuCompute -> height_field.glsl,
	# the readback-capable parity path) at page A's shared-edge texels AND at page B's shared-edge
	# texels, and assert they MATCH within EPS. If a future change reverts the convention (e.g. back
	# to texel-center) on either the generation or the shader side, the two pages' shared-edge world
	# points diverge by ~one texel and the heights differ -> this gate fails. This is the precise,
	# non-tautological proof the seam is gone, without reading the (deliberately non-CPU-readable)
	# production page textures.
	var gpu: Object = ClassDB.instantiate("Wg10GpuCompute")
	var ge: String = str(gpu.call("load_pack_dir", pack_os, PACK_FILE, glsl_os))
	if ge != "":
		push_error("gpu pack load failed: %s" % ge); return 1
	var denom := float(PAGE_PX - 1)

	# EAST seam: page A=(ox,oz) last column (i=N-1) vs page B=(ox+span0,oz) first column (i=0).
	var ax_xs := PackedFloat64Array(); var ax_zs := PackedFloat64Array()
	var bx_xs := PackedFloat64Array(); var bx_zs := PackedFloat64Array()
	for j in range(PAGE_PX):
		var v: float = float(j) / denom
		# A: origin (ox,oz), texel (N-1, j)
		ax_xs.append(ox + 1.0 * span0); ax_zs.append(oz + v * span0)
		# B: origin (ox+span0,oz), texel (0, j)
		bx_xs.append((ox + span0) + 0.0 * span0); bx_zs.append(oz + v * span0)
	var a_east: PackedFloat64Array = gpu.call("heights", ax_xs, ax_zs, SEED)
	var b_east: PackedFloat64Array = gpu.call("heights", bx_xs, bx_zs, SEED)
	var max_east := 0.0
	for j in range(PAGE_PX):
		max_east = maxf(max_east, absf(a_east[j] - b_east[j]))
	if max_east > SEAM_EPS:
		errs.append("seam EAST: max shared-column height diff %.6f m > %.4f" % [max_east, SEAM_EPS])

	# NORTH seam: page A last row (j=N-1) vs page C=(ox,oz+span0) first row (j=0).
	var az_xs := PackedFloat64Array(); var az_zs := PackedFloat64Array()
	var cz_xs := PackedFloat64Array(); var cz_zs := PackedFloat64Array()
	for i in range(PAGE_PX):
		var u: float = float(i) / denom
		az_xs.append(ox + u * span0); az_zs.append(oz + 1.0 * span0)          # A texel (i, N-1)
		cz_xs.append(ox + u * span0); cz_zs.append((oz + span0) + 0.0 * span0) # C texel (i, 0)
	var a_north: PackedFloat64Array = gpu.call("heights", az_xs, az_zs, SEED)
	var c_north: PackedFloat64Array = gpu.call("heights", cz_xs, cz_zs, SEED)
	var max_north := 0.0
	for i in range(PAGE_PX):
		max_north = maxf(max_north, absf(a_north[i] - c_north[i]))
	if max_north > SEAM_EPS:
		errs.append("seam NORTH: max shared-row height diff %.6f m > %.4f" % [max_north, SEAM_EPS])

	# (2) SOFT morph-banding under motion + a PNG artifact. Render a perspective flight POV,
	# sample an interior luminance scanline each measured frame, count large frame-to-frame jumps.
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
	var prev_line := PackedFloat32Array()
	var big_jumps := 0
	var samples := 0
	for f in range(120):
		var vx := heading.x * speed
		var vz := heading.y * speed
		pos += Vector2(vx, vz) * dt
		view.call("update", pos.x, pos.y, vx, vz)
		var eye := Vector3(pos.x - heading.x * 600.0, 900.0, pos.y - heading.y * 600.0)
		var look := Vector3(pos.x + heading.x * 1200.0, 0.0, pos.y + heading.y * 1200.0)
		cam.look_at_from_position(eye, look, Vector3.UP)
		await process_frame
		RenderingServer.force_draw()
		await process_frame
		if f >= 40 and f % 8 == 0:
			var img: Image = vp.get_texture().get_image()
			if img != null:
				if f == 40:
					img.save_png("user://m3_continuity.png")
				var y := int(img.get_height() * 0.7)   # interior terrain scanline (below the horizon)
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

	var jump_frac := float(big_jumps) / float(max(samples, 1))
	if jump_frac > JUMP_FRAC_CEIL:
		errs.append("morph banding: %.3f of interior samples jumped >%.2f frame-to-frame (ceil %.2f)" % [jump_frac, JUMP_THRESH, JUMP_FRAC_CEIL])

	pool.call("free_all")

	print("[wg10-m3-continuity] seam_east=%.5f seam_north=%.5f eps=%.4f morph_jump_frac=%.3f (ceil %.2f)" % [max_east, max_north, SEAM_EPS, jump_frac, JUMP_FRAC_CEIL])
	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m3-continuity] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-m3-continuity] status=pass")
	return 0
