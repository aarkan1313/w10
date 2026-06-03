extends SceneTree

# Slice-4 DRAINAGE MEASUREMENT: how many flow PULL-relaxation iters does the REAL 576 production
# page need to CONVERGE? This decides whether live-per-page flow fits the frame budget (~0.0336
# ms/iter at 576 -> the 3ms half-budget crosses at ~89 iters) or whether the coarse-drainage-fact
# subsystem is actually required. The 344 parity FIXTURE converges by ~32-64 iters, but the 576
# production page (256 core cells vs the fixture's 24) has LONGER flow paths -> likely needs MORE
# iters. Measure SELF-convergence: generate the page at increasing iter counts, find where the
# output stops changing (delta-to-the-256-iter reference < the parity epsilon 1e-4).
#
# WINDOWED only (local RD null headless -> skip rc 2). MEASUREMENT gate: rc 0 if it produces a
# number, rc 1 only on degenerate/error. No oracle needed -- pure self-convergence at production dims.

const PRIM_GLSL := "res://worldgen_terrain/shaders/recipe_primitives.glsl"
const MACHINE_GLSL := "res://worldgen_terrain/shaders/biome_page.glsl"
# Glacial converges SLOWEST of the 11 (its aggressive trough carving was the only biome to drift at
# 32 iters on the fixture); mountain is the reference structure. Measure both -> worst case bounds it.
const FRAGMENTS := {
	"mountain": "res://worldgen_terrain/shaders/biome_mountain.glsl",
	"glacial": "res://worldgen_terrain/shaders/biome_glacial.glsl",
}

const CORE_PX := 256
const APRON_PX := 160
const PADDED := CORE_PX + 2 * APRON_PX   # 576 -- the REAL production apron
const FEATURE_SPAN_M := 90000.0          # the mountain/glacial fixture feature span
const SEED := 0
const ITER_SWEEP := [16, 32, 48, 64, 96, 128, 192, 256]   # 256 is the converged reference
const CONV_EPS := 1.0e-4                  # the biome parity epsilon: "converged" = delta below this
const PER_ITER_MS := 0.0336               # measured at 576 (page_measure); half-budget (3ms) ~ 89 iters

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10BiomePageCompute"):
		push_error("[wg10-flow-converge] Wg10BiomePageCompute not registered")
		return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-flow-converge] status=skip reason=no-gpu")
		return 2
	probe.free()

	var gpu: Object = ClassDB.instantiate("Wg10BiomePageCompute")
	var err: String = str(gpu.call("load_shaders",
		ProjectSettings.globalize_path(PRIM_GLSL),
		ProjectSettings.globalize_path(MACHINE_GLSL)))
	if err != "":
		push_error("[wg10-flow-converge] shader load failed: %s" % err)
		return 1

	# A representative grid spacing: the fixture's per-px metre spacing scaled so the 256-core
	# spans the same feature extent the fixture's 24-core did (so flow-path LENGTHS in cells reflect
	# the real production density). fixture: 24 core over feature_span -> spacing ~ feature/24-ish;
	# production: 256 core over the same feature -> spacing = feature_span / CORE_PX gives the real
	# per-px density. (Absolute origin is irrelevant to convergence; path lengths in CELLS matter.)
	var spacing: float = FEATURE_SPAN_M / float(CORE_PX)

	var overall_converged_at := 0
	var any_degenerate := false
	for biome in FRAGMENTS:
		var frag: String = ProjectSettings.globalize_path(FRAGMENTS[biome])
		# Generate the page at each iter count; cache cores.
		var cores := {}
		for it_g in ITER_SWEEP:
			var it: int = int(it_g)
			var core: PackedFloat64Array = gpu.call("generate_core_page_iters",
				spacing, 0.0, 0.0, PADDED, PADDED, APRON_PX, SEED, FEATURE_SPAN_M, frag, it)
			if core.size() != CORE_PX * CORE_PX:
				push_error("[wg10-flow-converge] %s iters=%d bad size %d" % [biome, it, core.size()])
				return 1
			cores[it] = core
		# Reference = the highest iter count (most converged).
		var ref_core: PackedFloat64Array = cores[ITER_SWEEP[ITER_SWEEP.size() - 1]]
		var ref_absmax := 0.0
		for v in ref_core:
			ref_absmax = maxf(ref_absmax, absf(v))
		if ref_absmax <= 0.0:
			push_error("[wg10-flow-converge] %s reference page is all-zero (degenerate)" % biome)
			any_degenerate = true
			continue
		# For each iter count: max-abs delta vs the 256 reference (raw + normalized by ref_absmax).
		var converged_at: int = int(ITER_SWEEP[ITER_SWEEP.size() - 1])
		var found := false
		for it_v in ITER_SWEEP:
			var it: int = int(it_v)
			var d: float = _max_abs_delta(cores[it], ref_core)
			var dn: float = d / ref_absmax
			print("[wg10-flow-converge] biome=%s iters=%d delta_to_ref=%s norm=%s est_ms=%s" % [
				biome, it, str(d), str(dn), str(PER_ITER_MS * float(it))])
			if not found and dn < CONV_EPS:
				converged_at = it
				found = true
		var fits := (PER_ITER_MS * float(converged_at)) < 3.0   # half of the 6ms frame budget
		print("[wg10-flow-converge] biome=%s CONVERGED_AT=%d est_ms=%s fits_half_budget=%s" % [
			biome, converged_at, str(PER_ITER_MS * float(converged_at)), str(fits)])
		overall_converged_at = max(overall_converged_at, converged_at)

	if any_degenerate:
		push_error("[wg10-flow-converge] a reference page was degenerate")
		return 1
	var overall_ms: float = PER_ITER_MS * float(overall_converged_at)
	var live_fits: bool = overall_ms < 3.0
	print("[wg10-flow-converge] VERDICT overall_converged_at=%d est_ms=%s live_per_page_fits_half_budget=%s (if true, NO coarse-fact subsystem needed)" % [
		overall_converged_at, str(overall_ms), str(live_fits)])
	# MEASUREMENT gate: success = produced a non-degenerate convergence number for every biome.
	if overall_converged_at < 1:
		push_error("[wg10-flow-converge] degenerate: no convergence count")
		return 1
	print("[wg10-flow-converge] status=pass (measurement recorded)")
	return 0

func _max_abs_delta(a: PackedFloat64Array, b: PackedFloat64Array) -> float:
	var nn: int = min(a.size(), b.size())
	var m := 0.0
	for i in range(nn):
		var d: float = absf(a[i] - b[i])
		if d > m:
			m = d
	return m
