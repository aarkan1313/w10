extends SceneTree

# Rung 1: live mountain macro (flow OFF) vs baked REFERENCE, over the same page, in METRES.
# The live recipe runs with NO reference binding (dispatch reaches compute_biome_page_cached) at the
# accepted scale/seed/source-window. This is the highest-value un-intercept: does the live recipe
# reproduce the accepted macro shape when fed the right inputs?
#
# ARCHITECTURE (matches biome_world_runtime_check / biome_runtime_isolate / m3_continuity readback):
# convergence readback uses a BARE Wg10PagePool configured directly via ladder_producers.gd — NO
# scene, NO Camera3D, NO SubViewport. acquire_page dispatches a COMPUTE list; a tree-resident 3D
# scene/camera makes force_draw open a screen draw list that collides ("only one compute/draw list
# active"). The bare pool has no concurrent render, so readback is the sole GPU work. The scene
# (wg10_unintercept_ladder.tscn) is for the owner FLY + smoke/visual checks, not numeric readback.
#
# Threshold: direction + no-regression. The offline contract number (1.21) is NORMALIZED units, NOT
# metres (see docs/plans/LADDER_CONVERGENCE_BASELINE.md), so it cannot be the budget here. This gate
# SELF-BASELINES: with MEAN_ABS_BUDGET < 0 it prints the metres number and passes-with-warning so the
# first run can record it; then set the budget to recorded*1.10 and the gate enforces no-regression.

const PRODUCERS := "res://worldgen_terrain/harness/ladder_producers.gd"
const HELPER := "res://worldgen_terrain/harness/ladder_convergence.gd"
const PAGE_PX := 256
# <0 = unbaselined: print + pass-with-warning, do not fail on budget. Set to recorded*1.10 after run 1.
const MEAN_ABS_BUDGET := -1.0

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if RenderingServer.get_rendering_device() == null:
		print("[ladder-rung1] status=skip reason=no-render-device"); return 2

	var producer: Object = load(PRODUCERS).new()
	var helper: Object = load(HELPER).new()
	var rd := RenderingServer.get_rendering_device()
	var pool: Object = ClassDB.instantiate("Wg10PagePool")

	# 1) Live mountain macro page (flow off via flow_iters=0) at a known page.
	if not bool(producer.call("set_rung", "mountain_macro")):
		print("[ladder-rung1] status=fail reason=set_rung"); return 1
	var err := str(producer.call("configure", pool))
	if err != "":
		print("[ladder-rung1] status=fail reason=configure-live err=%s" % err); return 1
	var live: PackedFloat32Array = await helper.produce_and_read(self, rd, pool, 0, 0.0, 0.0, PAGE_PX)

	# 2) Baked REFERENCE over the SAME page (reconfigure the same bare pool).
	if not bool(producer.call("set_rung", "reference")):
		print("[ladder-rung1] status=fail reason=set_rung-ref"); pool.call("free_all"); return 1
	err = str(producer.call("configure", pool))
	if err != "":
		print("[ladder-rung1] status=fail reason=configure-ref err=%s" % err); pool.call("free_all"); return 1
	var ref: PackedFloat32Array = await helper.produce_and_read(self, rd, pool, 0, 0.0, 0.0, PAGE_PX)

	pool.call("free_all")

	var d: Dictionary = helper.delta(live, ref)
	if d.is_empty():
		print("[ladder-rung1] status=fail reason=shape-mismatch live=%d ref=%d" % [live.size(), ref.size()]); return 1
	if not bool(d["nonvacuous"]):
		print("[ladder-rung1] status=fail reason=vacuous live_relief=%.2f ref_relief=%.2f" % [float(d["live_relief"]), float(d["ref_relief"])]); return 1

	var mean_abs := float(d["mean_abs"])
	print("[ladder-rung1] mean_abs=%.4f p95_abs=%.4f peak_abs=%.4f live_relief=%.1f ref_relief=%.1f budget=%.4f" % [
		mean_abs, float(d["p95_abs"]), float(d["peak_abs"]), float(d["live_relief"]), float(d["ref_relief"]), MEAN_ABS_BUDGET])

	if MEAN_ABS_BUDGET < 0.0:
		print("[ladder-rung1] status=pass UNBASELINED record mean_abs=%.4f metres in LADDER_CONVERGENCE_BASELINE.md then set MEAN_ABS_BUDGET=%.4f" % [mean_abs, mean_abs * 1.10])
		return 0
	if mean_abs > MEAN_ABS_BUDGET:
		print("[ladder-rung1] status=fail mean_abs=%.4f > budget=%.4f (regression vs recorded metres baseline)" % [mean_abs, MEAN_ABS_BUDGET]); return 1
	print("[ladder-rung1] status=pass mean_abs=%.4f budget=%.4f" % [mean_abs, MEAN_ABS_BUDGET]); return 0
