import dataclasses
import numpy as np
import traverse_corridor as tc
import keeper_v2 as v2
import geography_skeleton_windows as win
import export_godot_rough_world_chunks as ex


def test_params_defaults_present_and_overridable():
    p = tc.TraverseParams()
    for name in ("slope_budget", "low_corridor_cutoff", "min_barrier_component_frac",
                 "slope_penalty", "drainage_bias", "corridor_width_m", "carve_max_m",
                 "row_tolerance_px", "band_px", "scene_width_m", "height_scale_m"):
        assert hasattr(p, name)
    p2 = dataclasses.replace(p, slope_budget=0.40)
    assert p2.slope_budget == 0.40 and p.slope_budget != 0.40


def _window():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    w = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 133, spec)
    return w, spec


def p_v2():
    return v2.KeeperV2Params()


def test_padded_slope_matches_core_cell_size():
    w, spec = _window()
    p = tc.TraverseParams()
    full = v2.compose_windowed_height_v2_full(w, 133, spec, p_v2())
    slopes = tc.padded_slope(full, spec, p)
    assert slopes.shape == full.shape
    assert np.all(np.isfinite(slopes))


def test_default_config_is_crossable_spiky_needs_route():
    w, spec = _window()
    p = tc.TraverseParams()
    full_default = v2.compose_windowed_height_v2_full(w, 133, spec, v2.KeeperV2Params())
    nd = tc.needs_route(full_default, spec, p)
    assert nd["slope_wall_severs"] is False          # measured: gentle default has no slope-wall

    # seed=1 chosen by measurement: spiky config breaks the low corridor there (low_crosses=False).
    # seed=133 is too low-dominant (mean~-1.0) for any config to lose the low corridor crossing.
    spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    w_spiky = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 1, spec)
    full_spiky = v2.compose_windowed_height_v2_full(w_spiky, 1, spec, spiky)
    ns = tc.needs_route(full_spiky, spec, p)
    assert ns["slope_wall_frac"] > 0.0               # spiky has real impassable terrain
    assert ns["needs_route"] is True


def test_least_cost_path_is_deterministic_and_edge_to_edge():
    w, spec = _window()
    p = tc.TraverseParams()
    spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    full = v2.compose_windowed_height_v2_full(w, 133, spec, spiky)
    slopes = tc.padded_slope(full, spec, p)
    channel = np.asarray(w["channel_axis"], dtype=np.float64)
    r1 = tc.least_cost_crossing(slopes, full, channel, spec, p, axis="x")
    r2 = tc.least_cost_crossing(slopes, full, channel, spec, p, axis="x")
    assert r1["path"] == r2["path"]                      # determinism
    rows = [pt[0] for pt in r1["path"]]
    cols = [pt[1] for pt in r1["path"]]
    assert min(cols) == 0 and max(cols) == full.shape[1] - 1   # spans west->east on the padded grid
    assert min(rows) >= 0 and max(rows) < full.shape[0]
    assert isinstance(r1["max_step_slope"], float)


def test_least_cost_path_z_axis_spans_ns():
    w, spec = _window()
    p = tc.TraverseParams()
    full = v2.compose_windowed_height_v2_full(w, 133, spec, v2.KeeperV2Params())
    slopes = tc.padded_slope(full, spec, p)
    channel = np.asarray(w["channel_axis"], dtype=np.float64)
    r = tc.least_cost_crossing(slopes, full, channel, spec, p, axis="z")
    rows = [pt[0] for pt in r["path"]]
    assert min(rows) == 0 and max(rows) == full.shape[0] - 1


# --- Tier-3 corridor: verify-first no-op works + seam-safe; real-barrier carve is honestly pending ---
# (See spec §1.2 BUILD FINDING / memory worldgen10-tier3-seam-exact-carve: the seam-exact CONNECTED carve is
#  blocked on a cross-seam-stitched connected-corridor fact. The module must NOT emit a seam-breaking carve nor
#  falsely claim a route. These tests lock that honest contract.)

def test_verify_first_noop_is_zero_seam_safe_and_resolved():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    p = tc.TraverseParams()
    ox, oz = ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M
    # gentle default (seed 133) is crossable -> no-op
    ra = tc.build_traverse_corridor(win.build_skeleton_window(ox, oz, 133, spec), 133, spec, p, v2.KeeperV2Params())
    rb = tc.build_traverse_corridor(win.build_skeleton_window(ox + ex.CHUNK_SPAN_M, oz, 133, spec), 133, spec, p, v2.KeeperV2Params())
    assert ra["needs_route"] is False
    assert ra["carved"] is False and ra["resolved"] is True and ra["carve_pending"] is False
    assert np.count_nonzero(ra["carve_delta"]) == 0
    # seam-safe: zero deltas trivially agree at the border (and stay zero across neighbours)
    assert float(np.max(np.abs(ra["carve_delta"][:, -1] - rb["carve_delta"][:, 0]))) == 0.0


def test_real_barrier_is_resolved_seam_exact_and_still_rugged():
    # seed 1, spiky, 25.6 km is a measured low-corridor barrier (memory worldgen10-tier3-barrier-measurements)
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    p = tc.TraverseParams()
    spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    ox, oz = ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M
    wa = win.build_skeleton_window(ox, oz, 1, spec)
    wb = win.build_skeleton_window(ox + ex.CHUNK_SPAN_M, oz, 1, spec)
    ra = tc.build_traverse_corridor(wa, 1, spec, p, spiky)
    rb = tc.build_traverse_corridor(wb, 1, spec, p, spiky)
    assert ra["needs_route"] is True                       # a real barrier exists
    assert ra["resolved"] is True and ra["carved"] is True  # ... and the corridor carve RESOLVED it
    keeper_core = v2.compose_windowed_height_v2(wa, 1, spec, spiky)
    final_core = keeper_core + ra["carve_delta"]
    assert tc.needs_route_core(final_core, spec, p)["needs_route"] is False   # guarantee holds
    border = float(np.max(np.abs(ra["carve_delta"][:, -1] - rb["carve_delta"][:, 0])))
    assert border == 0.0, f"carve broke seams: {border}"                      # seam-exact
    slopes = tc.trav.slope_grid(final_core, scene_width_m=p.scene_width_m, height_scale_m=p.height_scale_m)
    assert float(np.percentile(slopes, 90.0)) >= tc.trav.MIN_STRUCTURAL_SLOPE_P90   # still rugged


def test_crossing_holds_matches_needs_route_core():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    p = tc.TraverseParams()
    spiky = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)
    # gentle default core: crossable -> crossing_holds True
    wg = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 133, spec)
    gentle_core = v2.compose_windowed_height_v2(wg, 133, spec, v2.KeeperV2Params())
    assert tc.crossing_holds(gentle_core, spec, p) is True
    # unresolved barrier core (seed 1 spiky, no carve): crossing_holds False (NOT vacuously True)
    wb = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 1, spec)
    barrier_core = v2.compose_windowed_height_v2(wb, 1, spec, spiky)
    assert tc.crossing_holds(barrier_core, spec, p) is False


def test_compose_with_corridor_parity_and_deterministic():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    p = tc.TraverseParams()
    kp = v2.KeeperV2Params()
    w = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 133, spec)
    final1, res1 = tc.compose_with_corridor(w, 133, spec, p, kp)
    final2, res2 = tc.compose_with_corridor(w, 133, spec, p, kp)
    keeper = v2.compose_windowed_height_v2(w, 133, spec, kp)
    assert np.allclose(final1, keeper + res1["carve_delta"])   # final == keeper + carve (parity by construction)
    assert np.array_equal(final1, final2)                       # deterministic
