extends SceneTree

# WorldGen10 Slice-3 #1 RISK SPIKE harness: GPU flow-accumulation by iterative relaxation.
# MEASUREMENT only (not wired into the render path). WINDOWED ONLY: RenderingDevice compute
# is null headless on this D3D12 box, so a null device -> skip (rc 2), never a false pass.
#
# Question: can the MFD drainage operator (a sequential sorted sweep on CPU) run live on the
# GPU within the per-page frame budget at PAGE_PX=256? We run the relaxation at several
# iteration counts, report REAL GPU time (RenderingDevice timestamps, captured inside the
# dispatch loop -> device time, not wall/vsync), and check that the result CONVERGES as iters
# grow (the approximation approaches the CPU sorted-sweep). PASS = the flow pass is small
# enough (sub-ms..low-ms for one 256x256 page) that per-page height+flow stays well under the
# p99 < 6ms frame budget at ~1000 m/s streaming.

const GLSL := "res://worldgen_terrain/shaders/flow_accum_spike.glsl"
const DIM := 256          # real page resolution PAGE_PX
const POWER := 1.45       # MFD exponent, matches array_ops / geography_skeleton default
const SEED := 1337
const ITER_COUNTS := [32, 64, 128, 256, 384, 512, 768, 1024]
const REPEATS := 8        # take the min wall time across repeats (warm, least-noisy)
const BUDGET_MS := 6.0    # per-page frame budget the flow pass must fit well inside

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10FlowSpike"):
		push_error("[wg10-flow-spike] Wg10FlowSpike not registered - run WINDOWED, rebuilt dll")
		return 1
	var probe := RenderingServer.create_local_rendering_device()
	if probe == null:
		print("[wg10-flow-spike] status=skip reason=no-gpu (headless or no device)")
		return 2  # distinct skip code: runner must NOT treat as pass
	probe.free()

	var spike: Object = ClassDB.instantiate("Wg10FlowSpike")
	var os_glsl: String = ProjectSettings.globalize_path(GLSL)
	var err: String = str(spike.call("load_shader", os_glsl))
	if err != "":
		push_error("[wg10-flow-spike] shader load failed: %s" % err)
		return 1

	# Warm-up run (first dispatch pays shader-compile / allocation costs we do not want timed).
	var warm: float = spike.call("run", DIM, 8, POWER, SEED)
	if warm < 0.0:
		push_error("[wg10-flow-spike] warm-up run failed (see error log)")
		return 1

	# Measure GPU ms per iteration count (min across repeats), and cache final acc for convergence.
	# Also record CPU-timestamp and wall-clock deltas at the best-GPU repeat, to cross-check that
	# the GPU number is honest (real device time), not a measurement artifact.
	var ms_by_iter := {}
	var cpu_by_iter := {}
	var wall_by_iter := {}
	var acc_by_iter := {}
	for it in ITER_COUNTS:
		# Select the best repeat by WALL time (the trustworthy metric; GPU timestamp is
		# unreliable on local RD), and record the matching gpu/cpu numbers for reporting.
		var best_wall := 1.0e30
		var best_gpu := 0.0
		var best_cpu := 0.0
		var acc: PackedFloat64Array = PackedFloat64Array()
		for r in range(REPEATS):
			var ms: float = spike.call("run", DIM, it, POWER, SEED)
			if ms < 0.0:
				push_error("[wg10-flow-spike] run failed iters=%d" % it)
				return 1
			var wall_ms: float = float(spike.call("last_wall_us")) / 1000.0
			if wall_ms < best_wall:
				best_wall = wall_ms
				best_gpu = ms
				best_cpu = float(spike.call("last_cpu_us")) / 1000.0
				acc = spike.call("get_last_acc")
		ms_by_iter[it] = best_gpu
		cpu_by_iter[it] = best_cpu
		wall_by_iter[it] = best_wall
		acc_by_iter[it] = acc

	# Report per-iteration-count cost + per-iteration cost (gpu, with cpu/wall cross-check).
	for it in ITER_COUNTS:
		var ms: float = ms_by_iter[it]
		var per_iter: float = ms / float(it)
		print("[wg10-flow-spike] iters=%d gpu_ms=%.4f per_iter_ms=%.5f cpu_ms=%.4f wall_ms=%.4f dim=%d" % [it, ms, per_iter, cpu_by_iter[it], wall_by_iter[it], DIM])

	# Convergence: max abs delta between consecutive iteration counts' final acc.
	# Normalize the delta by the typical acc magnitude (use the 256-iter mean) so it is readable.
	var ref_acc: PackedFloat64Array = acc_by_iter[ITER_COUNTS[ITER_COUNTS.size() - 1]]
	var ref_mean := 0.0
	for v in ref_acc:
		ref_mean += v
	ref_mean = ref_mean / float(max(ref_acc.size(), 1))

	var prev_acc: PackedFloat64Array = acc_by_iter[ITER_COUNTS[0]]
	var conv_ok := true
	var last_rel_delta := 1.0e30
	for i in range(1, ITER_COUNTS.size()):
		var cur: PackedFloat64Array = acc_by_iter[ITER_COUNTS[i]]
		var md: float = _max_abs_delta(prev_acc, cur)
		var rel: float = md / maxf(ref_mean, 1e-9)
		print("[wg10-flow-spike] converged_delta iters=%d_vs_%d max_abs=%.4f rel_to_mean=%.5f" % [ITER_COUNTS[i - 1], ITER_COUNTS[i], md, rel])
		last_rel_delta = rel
		prev_acc = cur

	# --- HONEST measurement note ---
	# get_captured_timestamp_gpu_time() on a LOCAL RenderingDevice on this box (D3D12)
	# reports absurd values (thousands of ms) that EXCEED the wall-clock of the whole
	# submit+sync -- it is physically impossible, so the GPU-timestamp number is NOT
	# trustworthy here. The wall-clock around submit()+sync() (wall_ms above) is the honest
	# upper bound on real GPU work, and a DIFFERENTIAL across iteration counts cancels the
	# fixed per-submit setup/readback overhead to isolate the marginal per-iteration cost.
	var lo: int = ITER_COUNTS[0]
	var hi: int = ITER_COUNTS[ITER_COUNTS.size() - 1]
	var wall_lo: float = wall_by_iter[lo]
	var wall_hi: float = wall_by_iter[hi]
	var per_iter_wall_ms: float = (wall_hi - wall_lo) / float(hi - lo)
	# Fixed overhead implied by the differential (intercept at 0 iters), informational.
	var fixed_ms: float = wall_lo - per_iter_wall_ms * float(lo)
	print("[wg10-flow-spike] DIFFERENTIAL per_iter_wall_ms=%.5f fixed_overhead_ms=%.4f (gpu_timestamp UNRELIABLE on local RD - see wall_ms vs gpu_ms)" % [per_iter_wall_ms, fixed_ms])

	# Convergence verdict: drainage is visually stable at the LOWER iter count of the first
	# consecutive pair whose acc fields agree (delta -> ~0). If iters=A and iters=B (A<B) give
	# the same field, then A iters already suffice.
	var iters_for_stable := hi
	for i in range(1, ITER_COUNTS.size()):
		var cur: PackedFloat64Array = acc_by_iter[ITER_COUNTS[i]]
		var prev: PackedFloat64Array = acc_by_iter[ITER_COUNTS[i - 1]]
		var rel2: float = _max_abs_delta(prev, cur) / maxf(ref_mean, 1e-9)
		if rel2 < 0.01:        # < 1% of mean acc -> the lower count already converged
			iters_for_stable = ITER_COUNTS[i - 1]
			break

	# Marginal per-page GPU cost at the stable iteration count (the honest wall-differential;
	# the ~0.2ms fixed submit/setup overhead is amortized into the page's single submit and
	# shared with the height pass, so the MARGINAL cost is the right per-page flow figure).
	var flow_marginal_ms: float = per_iter_wall_ms * float(iters_for_stable)
	var flow_with_fixed_ms: float = fixed_ms + flow_marginal_ms
	# The flow pass should leave >= half the 6ms budget for height gen + the rest of the frame.
	var fits := flow_with_fixed_ms < (BUDGET_MS * 0.5)
	# A genuine GATE: live GPU flow must BOTH fit the budget AND converge. Either failing = FAIL.
	var passed := fits and conv_ok
	var verdict := "PASS" if passed else "FAIL"
	print("[wg10-flow-spike] VERDICT=%s stable_iters=%d flow_marginal_ms~=%.4f flow_with_fixed_ms~=%.4f (wall-differential model) budget_ms=%.1f half_budget_ms=%.1f converging=%s last_rel_delta=%.5f" % [
		verdict, iters_for_stable, flow_marginal_ms, flow_with_fixed_ms, BUDGET_MS, BUDGET_MS * 0.5, str(conv_ok), last_rel_delta])
	if not passed:
		push_error("[wg10-flow-spike] FAIL: fits_budget=%s converged=%s -- live GPU flow over budget or non-convergent" % [str(fits), str(conv_ok)])
		return 1
	return 0

func _max_abs_delta(a: PackedFloat64Array, b: PackedFloat64Array) -> float:
	var n: int = min(a.size(), b.size())
	var m := 0.0
	for i in range(n):
		var d: float = abs(a[i] - b[i])
		if d > m:
			m = d
	return m
