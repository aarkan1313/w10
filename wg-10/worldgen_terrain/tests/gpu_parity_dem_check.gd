extends SceneTree

# CPU/GPU parity on the REAL DEM pack (gate subset) — real-scale validation of
# the M2 kernel atlas. Tier 1 family signatures EXACT, Tier 2 height within f32
# epsilon. Windowed (RenderingDevice compute needs a device).

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const SHADER_RES := "res://worldgen_terrain/shaders/height_field.glsl"
const ABS_EPS := 1.0e-2
const REL_EPS := 1.0e-5

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10Height") or not ClassDB.class_exists("Wg10GpuCompute"):
		push_error("native classes not registered")
		return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-gpu-parity-dem] status=skip reason=no-gpu")
		return 2
	probe.free()
	var os_dir: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var os_glsl: String = ProjectSettings.globalize_path(SHADER_RES)
	var cpu: Object = ClassDB.instantiate("Wg10Height")
	var gpu: Object = ClassDB.instantiate("Wg10GpuCompute")
	var e1: String = str(cpu.call("load_pack_dir", os_dir, PACK_FILE))
	var e2: String = str(gpu.call("load_pack_dir", os_dir, PACK_FILE, os_glsl))
	if e1 != "" or e2 != "":
		push_error("pack load failed: cpu=%s gpu=%s" % [e1, e2])
		return 1

	var xs := PackedFloat64Array(); var zs := PackedFloat64Array()
	for ix in range(-12, 12):
		for iz in range(-12, 12):
			xs.append(float(ix) * 12345.0 + 17.0)
			zs.append(float(iz) * 9876.0 - 31.0)
	var n := xs.size()
	var gpu_h: PackedFloat64Array = gpu.call("heights", xs, zs, 1337)
	var gpu_s: PackedInt64Array = gpu.call("signatures", xs, zs, 1337)
	if gpu_h.size() != n or gpu_s.size() != n:
		push_error("gpu output size mismatch")
		return 1

	# max relief for the relative tolerance term
	var f := FileAccess.open(PACK_RES_DIR + "/" + PACK_FILE, FileAccess.READ)
	var pack: Dictionary = JSON.parse_string(f.get_as_text())
	var max_relief := 1.0
	for fid in pack["families"]:
		max_relief = maxf(max_relief, float(pack["families"][fid]["relief_m"]))

	var sig_mismatch := 0; var height_fail := 0; var max_dh := 0.0
	for i in range(n):
		var cs: int = cpu.call("family_signature", xs[i], zs[i], 1337)
		if cs != gpu_s[i]:
			sig_mismatch += 1
			if sig_mismatch <= 3:
				push_error("Tier1 sig mismatch @ (%s,%s): cpu=%d gpu=%d" % [str(xs[i]), str(zs[i]), cs, gpu_s[i]])
		var ch: float = cpu.call("height", xs[i], zs[i], 1337)
		var dh: float = absf(ch - float(gpu_h[i]))
		if dh > max_dh: max_dh = dh
		var tol := maxf(ABS_EPS, REL_EPS * max_relief)
		if dh > tol:
			height_fail += 1
			if height_fail <= 3:
				push_error("Tier2 height delta @ (%s,%s): d=%s tol=%s" % [str(xs[i]), str(zs[i]), str(dh), str(tol)])

	if sig_mismatch > 0 or height_fail > 0:
		print("[wg10-gpu-parity-dem] status=fail sig_mismatch=%d height_fail=%d maxd=%s" % [sig_mismatch, height_fail, str(max_dh)])
		return 1
	print("[wg10-gpu-parity-dem] status=pass coords=%d families_exact=true maxd=%s" % [n, str(max_dh)])
	return 0
