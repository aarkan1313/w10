extends SceneTree

# M4: visible(GPU page) vs collision(CPU Wg10Facts.get_height) parity on BASE terrain (DESIGN §4 —
# entities don't float/sink). Computes a level-0 page via Wg10PagePool, reads it back (gate-only),
# and compares each sampled texel's height to Wg10Facts.get_height at that texel's world point. No
# edits (edited cells are an intentional collidable-not-visible exception, out of scope here).
# WINDOWED (RenderingDevice compute + readback need a device).

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const PAGE_PX := 256
const SEED := 1337
const BASE_SPAN := 8192.0
const ABS_EPS := 1.0e-2   # metres; same scale as the M2 parity gates

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10Facts") or not ClassDB.class_exists("Wg10PagePool"):
		push_error("Wg10Facts / Wg10PagePool not registered"); return 1
	var rd := RenderingServer.get_rendering_device()
	if rd == null:
		print("[wg10-facts-parity] status=skip reason=no-render-device"); return 2

	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var e1: String = str(pool.call("configure", pack_os, PACK_FILE, glsl_os, 8, PAGE_PX, BASE_SPAN, SEED))
	var facts: Object = ClassDB.instantiate("Wg10Facts")
	var e2: String = str(facts.call("configure", pack_os, PACK_FILE, SEED))
	if e1 != "" or e2 != "":
		push_error("load failed: pool=%s facts=%s" % [e1, e2]); return 1

	# compute + read back one level-0 page at the origin
	var ox := 0.0
	var oz := 0.0
	var tex: Object = pool.call("acquire_page", 0, ox, oz)
	if tex == null:
		push_error("acquire_page returned null"); return 1
	var rid: RID = tex.call("get_texture_rd_rid")
	var bytes: PackedByteArray = rd.texture_get_data(rid, 0)
	var data: PackedFloat32Array = bytes.to_float32_array()
	if data.size() < PAGE_PX * PAGE_PX:
		push_error("page readback too small: %d" % data.size()); return 1

	# texel (i,j) world point under the page's texel-corner convention: ox + i/(N-1)*span.
	var n := PAGE_PX
	var denom := float(n - 1)
	var max_d := 0.0
	var fails := 0
	var checked := 0
	for j in range(0, n, 16):        # sparse subgrid; parity is per-point, 16x16 samples is plenty
		for i in range(0, n, 16):
			var wx := ox + float(i) / denom * BASE_SPAN
			var wz := oz + float(j) / denom * BASE_SPAN
			var gpu_h: float = data[j * n + i]
			var cpu_h: float = facts.call("get_height", wx, wz)
			var d: float = absf(gpu_h - cpu_h)
			checked += 1
			if d > max_d: max_d = d
			if d > ABS_EPS:
				fails += 1
				if fails <= 3:
					push_error("visible/collision mismatch @ (%f,%f): gpu=%f cpu=%f d=%f" % [wx, wz, gpu_h, cpu_h, d])

	pool.call("free_all")
	if fails > 0:
		print("[wg10-facts-parity] status=fail mismatches=%d/%d maxd=%s" % [fails, checked, str(max_d)]); return 1
	print("[wg10-facts-parity] status=pass checked=%d maxd=%s (visible==collision on base terrain)" % [checked, str(max_d)])
	return 0
