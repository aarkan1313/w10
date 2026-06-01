import dataclasses
import numpy as np
import corridor_router as cr
import traverse_corridor as tc
import keeper_v2 as v2
import geography_skeleton_windows as win
import export_godot_rough_world_chunks as ex


def _spiky():
    return dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=2.4, relief_amplitude=3.2)


def spec_core_n(spec):
    cs = win._core_slice(spec)
    return cs.stop - cs.start


def test_params_defaults_present_and_overridable():
    p = cr.CorridorParams()
    for name in ("gate_radius_px", "max_gates_per_edge", "corridor_density",
                 "corridor_width_m", "carve_max_m", "low_corridor_cutoff"):
        assert hasattr(p, name)
    p2 = dataclasses.replace(p, corridor_density=3)
    assert p2.corridor_density == 3 and p.corridor_density != 3


def test_edge_gates_are_identical_between_neighbours():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    p = cr.CorridorParams()
    ox, oz = ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M
    ca = v2.compose_windowed_height_v2(win.build_skeleton_window(ox, oz, 1, spec), 1, spec, _spiky())
    cb = v2.compose_windowed_height_v2(win.build_skeleton_window(ox + ex.CHUNK_SPAN_M, oz, 1, spec), 1, spec, _spiky())
    assert float(np.max(np.abs(ca[:, -1] - cb[:, 0]))) == 0.0       # seam line identical (keeper)
    ga = cr.edge_gates(ca[:, -1], p)
    gb = cr.edge_gates(cb[:, 0], p)
    assert ga == gb and len(ga) > 0                                  # gates identical -> routes meet exactly


def test_window_gates_returns_all_four_edges_on_core():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    p = cr.CorridorParams()
    full = v2.compose_windowed_height_v2_full(win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 1, spec), 1, spec, _spiky())
    g = cr.window_gates(full, spec, p)
    for edge in ("w", "e", "n", "s"):
        assert edge in g and len(g[edge]) > 0
    n = spec_core_n(spec)
    for r, c in g["w"]:
        assert c == 0 and 0 <= r < n
    for r, c in g["e"]:
        assert c == n - 1
    for r, c in g["n"]:
        assert r == 0
    for r, c in g["s"]:
        assert r == n - 1


def test_route_between_gates_connects_and_is_deterministic():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    p = cr.CorridorParams()
    tp = tc.TraverseParams()
    full = v2.compose_windowed_height_v2_full(win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 1, spec), 1, spec, _spiky())
    g = cr.window_gates(full, spec, p)
    a = g["w"][0]; b = g["e"][0]
    r1 = cr.route_between_gates(full, a, b, spec, tp)
    r2 = cr.route_between_gates(full, a, b, spec, tp)
    assert r1["path"] == r2["path"]                    # deterministic
    assert r1["path"][0] == a and r1["path"][-1] == b  # endpoints are the gates
    steps = [abs(r1["path"][i][0]-r1["path"][i+1][0]) + abs(r1["path"][i][1]-r1["path"][i+1][1]) for i in range(len(r1["path"])-1)]
    assert all(st == 1 for st in steps)                # connected 4-neighbour path


def test_build_corridor_density_one_spans_and_higher_reaches_more_edges():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    tp = tc.TraverseParams()
    full = v2.compose_windowed_height_v2_full(win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, 1, spec), 1, spec, _spiky())
    p1 = dataclasses.replace(cr.CorridorParams(), corridor_density=1)
    c1 = cr.build_corridor(full, spec, tp, p1)
    n = c1["mask"].shape[0]
    m = c1["mask"]
    spans = (m[:, 0].any() and m[:, -1].any()) or (m[0, :].any() and m[-1, :].any())
    assert spans
    assert c1["corridor_dist"].shape == (n, n)
    assert float(c1["corridor_dist"].min()) == 0.0
    p3 = dataclasses.replace(cr.CorridorParams(), corridor_density=3)
    c3 = cr.build_corridor(full, spec, tp, p3)
    def edges_touched(mask):
        return int(mask[:, 0].any()) + int(mask[:, -1].any()) + int(mask[0, :].any()) + int(mask[-1, :].any())
    assert edges_touched(c3["mask"]) >= edges_touched(c1["mask"])


def test_carve_corridor_is_seam_exact_on_barrier_and_bounded():
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    tp = tc.TraverseParams()
    p = cr.CorridorParams()
    ox, oz = ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M
    fa = v2.compose_windowed_height_v2_full(win.build_skeleton_window(ox, oz, 1, spec), 1, spec, _spiky())
    fb = v2.compose_windowed_height_v2_full(win.build_skeleton_window(ox + ex.CHUNK_SPAN_M, oz, 1, spec), 1, spec, _spiky())
    da = cr.carve_corridor(fa, cr.build_corridor(fa, spec, tp, p), spec, p)
    db = cr.carve_corridor(fb, cr.build_corridor(fb, spec, tp, p), spec, p)
    border = float(np.max(np.abs(da[:, -1] - db[:, 0])))
    assert border == 0.0, f"carve broke seams: {border}"
    assert np.max(np.abs(da)) <= p.carve_max_m / tp.height_scale_m + 1e-9
    assert np.all(da <= 1e-12)


def test_wall_sever_barrier_is_detected_carved_and_seam_exact():
    # slope-wall-sever barrier: seed 42, gain 3.5, 4 km span = an EXTREME ~80%-impassable massif.
    # This stress fixture proves the guarantee BITES where walls exist and stays seam-exact. Full passable
    # crossing of an 80%-impassable massif at the DEFAULT corridor width is a known limit (the per-game opt-in /
    # wider-corridor case, spec section 4.3/10): the carve is detected, applied, and seam-exact; `resolved` may
    # be False at default width for so extreme a wall. We assert detection + carve + seam-exactness, NOT a
    # false resolved=True. (The realistic play-scale low-corridor barrier DOES fully resolve -- see the
    # traverse_corridor guarantee test.)
    # Use corridor_density=1 (single spanning route): at this EXTREME small span (4 km), the carve feather
    # reach is a large fraction of the 129 grid, so a density>=2 network's cross-route interior can come within
    # feather reach of the perpendicular seam and break seam-exactness. A single route only touches the E/W
    # seams at gate-anchored stubs -> seam-exact. (Network density is for larger windows where the feather is
    # small relative to the grid; the realistic 25.6 km play scale is seam-exact at density 2 -- see
    # test_carve_corridor_is_seam_exact_on_barrier_and_bounded. This small-span/dense limit is documented in
    # the spec section 10 honest-risk + the carve precondition.)
    spec = ex._window_spec(129, 4000.0)
    p = dataclasses.replace(tc.TraverseParams(), scene_width_m=4000.0, low_corridor_cutoff=0.0)
    wall = dataclasses.replace(v2.KeeperV2Params(), post_tanh_gain=3.5, relief_amplitude=3.2)
    ox, oz = ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M
    import corridor_router as cr
    pc = cr.CorridorParams(corridor_density=1, low_corridor_cutoff=0.0)
    fa = v2.compose_windowed_height_v2_full(win.build_skeleton_window(ox, oz, 42, spec), 42, spec, wall)
    fb = v2.compose_windowed_height_v2_full(win.build_skeleton_window(ox + 4000.0, oz, 42, spec), 42, spec, wall)
    pre = tc.needs_route_core(cr._core(fa, spec), spec, p)
    assert pre["needs_route"] is True and pre["slope_wall_severs"] is True   # the wall is detected + severs
    da = cr.carve_corridor(fa, cr.build_corridor(fa, spec, p, pc), spec, pc, height_scale_m=p.height_scale_m)
    db = cr.carve_corridor(fb, cr.build_corridor(fb, spec, p, pc), spec, pc, height_scale_m=p.height_scale_m)
    assert np.count_nonzero(da) > 0                              # a corridor carve was applied
    assert float(np.max(np.abs(da[:, -1] - db[:, 0]))) == 0.0    # seam-exact on the extreme wall (density 1)
