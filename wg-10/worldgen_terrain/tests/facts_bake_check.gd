extends SceneTree

# M4 slice 4: Wg10Facts.bake_collision_region (GPU bulk path) must agree with get_collision_field
# (the CPU sparse path) over the same region, within the M2 parity epsilon — proving the off-frame
# GPU bake returns the SAME authoritative heights, just computed in bulk. Also checks bad-arg
# rejection. WINDOWED (GPU compute + readback need a device). The bake is an OFF-FRAME op (readback
# stall); this gate calls it once, as intended.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_field.glsl"   # the point-sampler shader gpu.heights uses
const SEED := 1337
const ABS_EPS := 1.0e-2   # metres; same scale as the M2 parity gates

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10Facts") or not ClassDB.class_exists("Wg10GpuCompute"):
		push_error("Wg10Facts / Wg10GpuCompute not registered"); return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-facts-bake] status=skip reason=no-gpu"); return 2
	probe.free()

	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	var facts: Object = ClassDB.instantiate("Wg10Facts")
	var e1: String = str(facts.call("configure", pack_os, PACK_FILE, SEED))
	var gpu: Object = ClassDB.instantiate("Wg10GpuCompute")
	var e2: String = str(gpu.call("load_pack_dir", pack_os, PACK_FILE, glsl_os))
	if e1 != "" or e2 != "":
		push_error("load failed: facts=%s gpu=%s" % [e1, e2]); return 1

	var ccx := 23456.0
	var ccz := -12345.0
	var size := 4000.0
	var n := 33   # Jolt-friendly (2^5+1); a real bulk patch

	var cpu: PackedFloat32Array = facts.call("get_collision_field", ccx, ccz, size, n)
	var bake: PackedFloat32Array = facts.call("bake_collision_region", gpu, ccx, ccz, size, n)

	var errors: Array[String] = []
	if cpu.size() != n * n or bake.size() != n * n:
		errors.append("size mismatch: cpu=%d bake=%d expected=%d" % [cpu.size(), bake.size(), n * n])
	else:
		var max_d := 0.0
		var fails := 0
		for k in range(n * n):
			var d: float = absf(cpu[k] - bake[k])
			if d > max_d: max_d = d
			if d > ABS_EPS:
				fails += 1
		if fails > 0:
			errors.append("GPU bake vs CPU collision mismatch: %d/%d cells > %.4f, maxd=%f" % [fails, n*n, ABS_EPS, max_d])
		else:
			print("[wg10-facts-bake] cells=%d maxd=%s (GPU bulk == CPU collision)" % [n*n, str(max_d)])

	# bad args -> empty
	var bad: PackedFloat32Array = facts.call("bake_collision_region", gpu, 0.0, 0.0, 0.0, 1)
	if bad.size() != 0:
		errors.append("bad-arg bake not empty: %d" % bad.size())

	if not errors.is_empty():
		for er in errors: push_error(er)
		print("[wg10-facts-bake] status=fail errors=%d" % errors.size()); return 1
	print("[wg10-facts-bake] status=pass")
	return 0
