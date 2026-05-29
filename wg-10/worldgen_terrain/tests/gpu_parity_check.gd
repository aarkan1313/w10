extends SceneTree

# CPU/GPU parity for the full formula. Tier 1: family-selection signatures must
# match EXACTLY (integer hash identical both sides). Tier 2: height within a
# documented f32 epsilon. Runs WINDOWED (RenderingDevice compute needs a device).

const PACK_RES_DIR := "res://worldgen_terrain/fixtures"
const PACK_FILE := "height_pack.json"
const SHADER_RES := "res://worldgen_terrain/shaders/height_field.glsl"
const ABS_EPS := 1.0e-2   # metres; f32 vs f64 over heights up to ~1000 m
const REL_EPS := 1.0e-5   # f32 ~7 sig digits

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10Height") or not ClassDB.class_exists("Wg10GpuCompute"):
		push_error("native classes not registered")
		return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-gpu-parity] status=skip reason=no-gpu (headless or no device)")
		return 2  # distinct skip code — runner must NOT treat as pass
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

	# coordinate grid — within i32 region-index range (shader sign-extension assumption).
	var xs := PackedFloat64Array(); var zs := PackedFloat64Array()
	for ix in range(-12, 12):
		for iz in range(-12, 12):
			xs.append(float(ix) * 12345.0 + 17.0)
			zs.append(float(iz) * 9876.0 - 31.0)
	var n := xs.size()
	var gpu_h: PackedFloat64Array = gpu.call("heights", xs, zs, 1337)
	var gpu_s: PackedInt64Array = gpu.call("signatures", xs, zs, 1337)
	if gpu_h.size() != n or gpu_s.size() != n:
		push_error("gpu output size mismatch: h=%d s=%d n=%d" % [gpu_h.size(), gpu_s.size(), n])
		return 1

	var errors := 0
	var max_dh := 0.0
	var sig_mismatch := 0
	for i in range(n):
		var x := xs[i]; var z := zs[i]
		var ch: float = cpu.call("height", x, z, 1337)
		var cs: int = cpu.call("family_signature", x, z, 1337)
		if cs != gpu_s[i]:
			sig_mismatch += 1
			if sig_mismatch <= 3:
				push_error("Tier1 signature mismatch @ (%f,%f): cpu=%d gpu=%d" % [x, z, cs, gpu_s[i]])
		var dh: float = absf(ch - float(gpu_h[i]))
		if dh > max_dh: max_dh = dh
		var tol := maxf(ABS_EPS, REL_EPS * 1000.0)
		if dh > tol:
			errors += 1
			if errors <= 3:
				push_error("Tier2 height delta @ (%f,%f): cpu=%f gpu=%f d=%f" % [x, z, ch, gpu_h[i], dh])

	if sig_mismatch > 0 or errors > 0:
		print("[wg10-gpu-parity] status=fail coords=%d sig_mismatch=%d height_fail=%d maxd=%s" % [n, sig_mismatch, errors, str(max_dh)])
		return 1
	print("[wg10-gpu-parity] status=pass coords=%d families_exact=true maxd=%s" % [n, str(max_dh)])
	return 0
