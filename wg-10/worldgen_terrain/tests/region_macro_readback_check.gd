extends SceneTree

# Windowed gate: proves bake_region_macro_readback returns the same field as
# generate_runtime_page_flow for the identical padded grid parameters.
# Both functions call the same GPU path (compute_biome_page_cached), so the
# two readbacks must be bit-identical or within f32 rounding (maxd <= 1e-6).
# This gates the NEW plumbing, not the underlying GPU math (already covered by biome_macro_576).

const PRIM     := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE  := "res://worldgen_terrain/shaders/biome_page.glsl"
const FRAGMENT := "res://worldgen_terrain/shaders/biome_mountain.glsl"

# Small region: 129 core + 16 apron each side = 161 padded.
const CORE_PX       := 129
const APRON_PX      := 16
const SPACING       := 200.0
const OX            := 0.0
const OZ            := 0.0
const SEED          := 1
const FEATURE_SPAN  := 90000.0
const FLOW_ITERS    := 192
const FLOW_ON       := false
const MAX_EPS       := 1.0e-6

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10BiomePageCompute"):
		push_error("[wg10-region-macro] Wg10BiomePageCompute not registered")
		return 1

	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-region-macro] status=skip reason=no-gpu")
		return 2
	probe.free()

	var gpu: Object = ClassDB.instantiate("Wg10BiomePageCompute")
	var err: String = str(gpu.call("load_shaders",
		ProjectSettings.globalize_path(PRIM),
		ProjectSettings.globalize_path(MACHINE)))
	if err != "":
		push_error("[wg10-region-macro] shader load failed: %s" % err)
		return 1

	var frag_abs: String = ProjectSettings.globalize_path(FRAGMENT)
	var padded := CORE_PX + 2 * APRON_PX  # 161

	# --- reference: generate_runtime_page_flow (the proven path) ---
	var ref_arr: PackedFloat64Array = gpu.call("generate_runtime_page_flow",
		SPACING, OX, OZ,
		padded, padded, APRON_PX,
		SEED, FEATURE_SPAN, frag_abs,
		FLOW_ITERS, FLOW_ON)
	if ref_arr.size() != CORE_PX * CORE_PX:
		push_error("[wg10-region-macro] generate_runtime_page_flow returned size %d, expected %d" % [
			ref_arr.size(), CORE_PX * CORE_PX])
		return 1

	# --- new path: bake_region_macro_readback ---
	var got_arr: PackedFloat64Array = gpu.call("bake_region_macro_readback",
		SPACING, OX, OZ,
		CORE_PX, APRON_PX,
		SEED, FEATURE_SPAN, frag_abs,
		FLOW_ITERS, FLOW_ON)
	if got_arr.size() != CORE_PX * CORE_PX:
		push_error("[wg10-region-macro] bake_region_macro_readback returned size %d, expected %d" % [
			got_arr.size(), CORE_PX * CORE_PX])
		return 1

	# --- compare ---
	var maxd := 0.0
	var at := 0
	for i in range(got_arr.size()):
		var d: float = absf(got_arr[i] - ref_arr[i])
		if d > maxd:
			maxd = d
			at = i
	if maxd != maxd:
		push_error("[wg10-region-macro] status=fail NaN delta")
		return 1

	print("[wg10-region-macro] core_px=%d maxd=%s at=%d eps=%s" % [
		CORE_PX, str(maxd), at, str(MAX_EPS)])

	if maxd > MAX_EPS:
		print("[wg10-region-macro] status=fail core_px=%d maxd=%s eps=%s" % [
			CORE_PX, str(maxd), str(MAX_EPS)])
		return 1

	print("[wg10-region-macro] status=pass core_px=%d maxd=%s" % [CORE_PX, str(maxd)])
	return 0
