extends SceneTree

# WorldGen10 Slice-4a MEASUREMENT gate: real per-page GPU cost at APRON dimensions.
# Decides spec 3.1 (per-page-live vs coarse-drainage-fact). WINDOWED only (local RD
# null headless -> skip rc 2). Honest metric = wall-differential across iters (cancels
# fixed submit overhead), same model as flow_spike_check.gd.

const GLSL := "res://worldgen_terrain/shaders/flow_accum_spike.glsl"
const CORE_PX := 256
const APRON_PX := 160          # mountain MOUNTAIN_APRON_PX (recipes.rs::mountain::APRON_PX)
const APRON_DIM := CORE_PX + 2 * APRON_PX   # 576
const POWER := 1.45
const SEED := 1337
const STABLE_ITERS := 128      # flow-spike converged iteration count
const LOW_ITERS := 8
const REPEATS := 8
const BUDGET_MS := 6.0

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10PageMeasure"):
		push_error("[wg10-page-measure] Wg10PageMeasure not registered - run WINDOWED, rebuilt dll")
		return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-page-measure] status=skip reason=no-gpu (headless or no device)")
		return 2
	probe.free()

	var m: Object = ClassDB.instantiate("Wg10PageMeasure")
	var err: String = str(m.call("load_shader", ProjectSettings.globalize_path(GLSL)))
	if err != "":
		push_error("[wg10-page-measure] shader load failed: %s" % err)
		return 1

	# warm-up (pays first-dispatch compile/alloc we do not want timed)
	if float(m.call("run", APRON_DIM, LOW_ITERS, POWER, SEED)) < 0.0:
		push_error("[wg10-page-measure] warm-up failed")
		return 1

	# best (min) wall ms at the apron dim, at STABLE_ITERS and at LOW_ITERS.
	var best_hi := _best_wall(m, APRON_DIM, STABLE_ITERS)
	var best_lo := _best_wall(m, APRON_DIM, LOW_ITERS)
	if best_hi < 0.0 or best_lo < 0.0:
		push_error("[wg10-page-measure] run failed")
		return 1

	var per_iter_ms: float = (best_hi - best_lo) / float(STABLE_ITERS - LOW_ITERS)
	var flow_marginal_ms: float = per_iter_ms * float(STABLE_ITERS)

	# per-page-live fits if the flow pass leaves >= half the 6ms budget for the recipe
	# height work + the rest of the frame (same threshold as flow_spike_check.gd).
	var fits := flow_marginal_ms < (BUDGET_MS * 0.5)
	var pipeline := "per-page-live" if fits else "coarse-drainage-fact-fallback"
	print("[wg10-page-measure] apron_dim=%d stable_iters=%d per_iter_ms=%.5f flow_marginal_ms=%.4f half_budget_ms=%.2f PIPELINE=%s wall_hi=%.4f wall_lo=%.4f" % [
		APRON_DIM, STABLE_ITERS, per_iter_ms, flow_marginal_ms, BUDGET_MS * 0.5, pipeline, best_hi, best_lo])

	# MEASUREMENT gate: must SUCCEED at producing a non-degenerate number, not assert a
	# particular verdict (both pipeline branches are valid spec outcomes). Degenerate =
	# non-positive marginal (timer broke) -> FAIL so the number is never trusted blind.
	if not (flow_marginal_ms > 0.0):
		push_error("[wg10-page-measure] FAIL: degenerate marginal (timer unreliable) flow_marginal_ms=%.4f" % flow_marginal_ms)
		return 1
	print("[wg10-page-measure] status=pass (measurement recorded; decision=%s)" % pipeline)
	return 0

func _best_wall(m: Object, dim: int, iters: int) -> float:
	var b := 1.0e30
	for r in range(REPEATS):
		var ms: float = m.call("run", dim, iters, POWER, SEED)
		if ms < 0.0:
			return -1.0
		b = minf(b, ms)
	return b
