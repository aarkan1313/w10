extends SceneTree

# M3 slice 3 gate: drive Wg10Streamer over a synthetic high-speed straight-line
# sweep and assert the stream-ahead invariants. WINDOWED (needs the global
# RenderingDevice via the pool). The scheduling LOGIC is proven exhaustively by the
# headless schedule_policy cargo tests; this proves the godot driver + pool wiring
# and the never-black invariant under motion.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const PAGE_PX := 256
const SEED := 1337

# Scheduler config. base_span == pool world_span (level-0 page size). 3 levels.
const NUM_LEVELS := 3
const BASE_SPAN := 8192.0
const RADIUS_PAGES := 1          # 3x3 ring per level -> 27 covered keys/frame
const LEAD_FRAMES := 4.0
const MAX_PER_FRAME := 2
# Capacity must hold the coarsest ring (9) + headroom so the coarse blanket stays
# resident and finer detail can stream. 9 (coarsest) + 9 (mid) + slack.
const CAPACITY := 24

const FRAMES := 60
# Fast enough that missing >> max_per_frame every frame (exercises the bound and
# the fallback). One level-0 page per ~2 frames of travel at this speed.
const VEL_X := 6000.0
const VEL_Z := 0.0

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10Streamer"):
		push_error("Wg10Streamer not registered"); return 1
	if not ClassDB.class_exists("Wg10PagePool"):
		push_error("Wg10PagePool not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-m3-stream] status=skip reason=no-render-device"); return 2

	var os_dir: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var os_glsl: String = ProjectSettings.globalize_path(GLSL)

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var err: String = str(pool.call("configure", os_dir, PACK_FILE, os_glsl, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if err != "":
		push_error("pool configure failed: %s" % err); return 1

	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_FRAMES, MAX_PER_FRAME)

	var errs: Array[String] = []
	var any_missing_served_by_fallback := false

	# Warm-up: prime the coarsest blanket first so frame-0 never-black holds. Acquire
	# the full coarsest ring at the start position before the sweep.
	_prime_coarsest(pool, NUM_LEVELS - 1)

	for f in range(FRAMES):
		var cam_x := VEL_X * float(f)   # straight line along +x
		var cam_z := 0.0
		streamer.call("update", cam_x, cam_z, VEL_X, VEL_Z)
		var st: Dictionary = streamer.call("stats")

		# (1) bounded work
		if int(st.get("acquired_this_frame", 0)) > MAX_PER_FRAME:
			errs.append("frame %d: acquired %d > max %d" % [f, int(st.get("acquired_this_frame",0)), MAX_PER_FRAME])

		# (2) budget never exceeded
		var ps: Dictionary = pool.call("stats")
		if int(ps.get("resident", 0)) > CAPACITY:
			errs.append("frame %d: resident %d > capacity %d" % [f, int(ps.get("resident",0)), CAPACITY])

		# (3) never black: every covered page is resident OR has a coarser fallback
		var resident := _key_set(pool.call("resident_keys"))
		var coverage := _key_list(streamer.call("coverage_keys", cam_x, cam_z, VEL_X, VEL_Z))
		for k in coverage:
			if resident.has(k):
				continue
			if _coarser_fallback_resident(k, resident, NUM_LEVELS, BASE_SPAN):
				any_missing_served_by_fallback = true
			else:
				errs.append("frame %d: covered page %s had NO resident fallback (BLACK)" % [f, str(k)])

	# (5) non-vacuity: at this speed, missing >> max, so fallback must have fired
	if not any_missing_served_by_fallback:
		errs.append("vacuous pass: no missing page was ever served by fallback (speed too low?)")

	# (4) determinism: same sweep -> identical per-frame acquire/release counts.
	var seq_a := _run_sweep_counts(os_dir, os_glsl)
	var seq_b := _run_sweep_counts(os_dir, os_glsl)
	if seq_a != seq_b:
		errs.append("non-deterministic: sweep count sequences differ")

	pool.call("free_all")

	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m3-stream] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-m3-stream] status=pass frames=%d fallback_fired=%s" % [FRAMES, str(any_missing_served_by_fallback)])
	return 0

# --- helpers ---

func _prime_coarsest(pool: Object, coarsest: int) -> void:
	var span := BASE_SPAN * pow(2.0, coarsest)
	for dz in range(-RADIUS_PAGES, RADIUS_PAGES + 1):
		for dx in range(-RADIUS_PAGES, RADIUS_PAGES + 1):
			var ox := dx * int(span)
			var oz := dz * int(span)
			var tex = pool.call("acquire_page", coarsest, float(ox), float(oz))
			pool.call("release_page", coarsest, float(ox), float(oz))

func _key_list(flat: PackedInt64Array) -> Array:
	var out := []
	var i := 0
	while i + 2 < flat.size():
		out.append(Vector3i(flat[i], flat[i+1], flat[i+2]))
		i += 3
	return out

func _key_set(flat: PackedInt64Array) -> Dictionary:
	var d := {}
	for k in _key_list(flat):
		d[k] = true
	return d

# Mirrors SchedulePolicy::coarser_fallback (schedule_policy.rs) — keep in sync.
func _coarser_fallback_resident(k: Vector3i, resident: Dictionary, num_levels: int, base_span: float) -> bool:
	var level := k.x
	var span := base_span * pow(2.0, level)
	var cx := float(k.y) + span * 0.5
	var cz := float(k.z) + span * 0.5
	for l in range(level + 1, num_levels):
		var s := base_span * pow(2.0, l)
		var ox := int(floor(cx / s)) * int(s)
		var oz := int(floor(cz / s)) * int(s)
		if resident.has(Vector3i(l, ox, oz)):
			return true
	return false

# Re-run a short sweep on a fresh pool/streamer and return the per-frame
# (acquired, released) count sequence for the determinism check.
func _run_sweep_counts(os_dir: String, os_glsl: String) -> Array:
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	pool.call("configure", os_dir, PACK_FILE, os_glsl, CAPACITY, PAGE_PX, BASE_SPAN, SEED)
	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_FRAMES, MAX_PER_FRAME)
	_prime_coarsest(pool, NUM_LEVELS - 1)
	var seq := []
	for f in range(FRAMES):
		streamer.call("update", VEL_X * float(f), 0.0, VEL_X, VEL_Z)
		var st: Dictionary = streamer.call("stats")
		seq.append([int(st.get("acquired_this_frame",0)), int(st.get("released_this_frame",0))])
	pool.call("free_all")
	return seq
