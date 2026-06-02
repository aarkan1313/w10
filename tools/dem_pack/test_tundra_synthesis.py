import numpy as np

import tundra_synthesis as ts


def test_tundra_generate_is_deterministic_finite_and_nonflat():
    wx, wz = ts.grid(96, 90000.0)
    a = ts.generate(wx, wz, seed=111, style=ts.STYLES[0])
    b = ts.generate(wx, wz, seed=111, style=ts.STYLES[0])

    za = a["height"]
    assert za.shape == wx.shape
    assert np.all(np.isfinite(za))
    assert np.allclose(za, b["height"])
    assert float(np.ptp(za)) > 0.45


def test_tundra_styles_are_materially_different():
    wx, wz = ts.grid(80, 90000.0)
    heights = [ts.generate(wx, wz, seed=112, style=style)["height"] for style in ts.STYLES]
    deltas = []
    for i in range(len(heights) - 1):
        deltas.append(float(np.mean(np.abs(heights[i] - heights[i + 1]))))
    assert min(deltas) > 0.05


def test_tundra_patterned_ground_has_more_pattern_than_arctic_plain():
    wx, wz = ts.grid(96, 90000.0)
    plain = ts.generate(wx, wz, seed=113, style=ts.STYLES[0])
    patterned = ts.generate(wx, wz, seed=113, style=ts.STYLES[1])
    assert float(np.mean(patterned["pattern"])) > float(np.mean(plain["pattern"])) * 1.8


def test_tundra_glacial_fringe_has_more_fringe_than_plain():
    wx, wz = ts.grid(96, 90000.0)
    plain = ts.generate(wx, wz, seed=114, style=ts.STYLES[0])
    fringe = ts.generate(wx, wz, seed=114, style=ts.STYLES[2])
    assert float(np.mean(fringe["fringe"])) > float(np.mean(plain["fringe"])) * 2.0


def test_tundra_foothills_have_more_foothills_than_plain():
    wx, wz = ts.grid(96, 90000.0)
    plain = ts.generate(wx, wz, seed=115, style=ts.STYLES[0])
    foothills = ts.generate(wx, wz, seed=115, style=ts.STYLES[3])
    assert float(np.mean(foothills["foothills"])) > float(np.mean(plain["foothills"])) * 2.0


def test_tundra_drainage_is_lower_than_foothills():
    wx, wz = ts.grid(96, 90000.0)
    result = ts.generate(wx, wz, seed=116, style=ts.STYLES[3])
    z = result["height"]
    foot_h = z[result["foothills"] > np.quantile(result["foothills"], 0.78)]
    drain_h = z[result["drainage"] > np.quantile(result["drainage"], 0.78)]
    assert foot_h.size > 0 and drain_h.size > 0
    assert float(np.mean(foot_h)) > float(np.mean(drain_h))


# ---------------------------------------------------------------------------
# Seam-exactness test (apron_px > 0 path)
# ---------------------------------------------------------------------------

def test_tundra_generate_seam_exact():
    """Adjacent apron-padded windows must agree at their shared border to the visually-seamless bar.

    Setup:
      - Core span S = 60_000 m over N = 64 core cells; cell_size = S / (N-1)
      - Window A core covers world x in [X0, X0+S], window B core covers [X0+S, X0+2*S]
      - A MASTER 1-D array (arange * cell + origin) spans both windows so that both A and B
        slice the SAME float64 values at their shared border -- guaranteeing bit-identical
        border coordinates. (Independent linspace/arange calls on different spans produce
        different float64 representations of the same world position.)
      - After generate(apron_px=APRON), height is cropped to the core; assert the shared
        border column matches to within the VISUALLY-SEAMLESS bar (see below).

    Tolerance note (the bar is VISUALLY SEAMLESS, not bit-exact):
      The seam-safe path carves drainage with REAL multiple-flow-direction (MFD) accumulation,
      a GLOBAL operation (a cell's discharge depends on all upstream cells). This CANNOT be
      made bit-exact across arbitrarily many windows: a border cell's drainage depends on
      upstream area that grows with world size, so no fixed apron captures all of it. The
      residual is SCALE-DEPENDENT. The owner-accepted bar is "seamless + looks good", not
      bit-identical. A NORMALIZED delta < 1e-3 is invisible/untrippable at tundra scale --
      the test also catches REAL breakage (wrong feature_span, apron far too small, origin
      bug -> deltas 1e-2+). See mountain_synthesis docstring for the probe measurements.
    """
    N = 64
    S = 60_000.0
    X0 = 10_000.0
    Z0 = 0.0
    APRON = ts.TUNDRA_APRON_PX
    SEED = 99
    STYLE = ts.STYLES[0]

    cell = S / (N - 1)
    padded_n = N + 2 * APRON

    # Master 1-D arrays: A occupies master[0:padded_n], B occupies master[N-1:N-1+padded_n].
    # The shared border is master column (APRON + N - 1), which equals A[:, APRON+N-1] and
    # B[:, APRON] -- both are the SAME float64 value since they reference the same element.
    total_cols = 2 * N + 2 * APRON - 1
    master_x = np.arange(total_cols, dtype=np.float64) * cell + (X0 - APRON * cell)
    master_z = np.arange(total_cols, dtype=np.float64) * cell + (Z0 - APRON * cell)

    a_x_1d = master_x[0:padded_n]
    b_x_1d = master_x[N - 1:N - 1 + padded_n]
    z_1d   = master_z[0:padded_n]

    a_wx, a_wz = np.meshgrid(a_x_1d, z_1d)
    b_wx, b_wz = np.meshgrid(b_x_1d, z_1d)

    # Verify grid setup: shared border column is bit-identical
    a_border_x = a_wx[0, APRON + N - 1]
    b_border_x = b_wx[0, APRON]
    print(f"\nA border wx = {a_border_x:.3f}, B border wx = {b_border_x:.3f}")
    assert a_border_x == b_border_x, (
        f"Grid setup error: A border x={a_border_x!r} != B border x={b_border_x!r}"
    )

    # feature_span_m MUST be the same fixed constant for all adjacent windows.
    res_a = ts.generate(a_wx, a_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)
    res_b = ts.generate(b_wx, b_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)

    a_core = res_a["height"]
    b_core = res_b["height"]

    assert a_core.shape == (N, N), f"A core shape {a_core.shape} != ({N}, {N})"
    assert b_core.shape == (N, N), f"B core shape {b_core.shape} != ({N}, {N})"

    # The shared border: A's rightmost column vs B's leftmost column.
    # Bar = VISUALLY SEAMLESS (< 1e-3 normalized): global flow accumulation cannot be
    # bit-exact across windows (scale-dependent residual); < 1e-3 still catches real breakage.
    SEAM_BAR = 1e-3
    border_delta = float(np.max(np.abs(a_core[:, -1] - b_core[:, 0])))
    print(f"Seam border delta (max |A[:,-1] - B[:,0]|) = {border_delta:.6e}  "
          f"(~{border_delta * 1700.0:.3f} m at base_height_scale 1700)")
    assert border_delta < SEAM_BAR, (
        f"Seam NOT visually seamless: max border delta = {border_delta:.6e} (>= {SEAM_BAR:.0e}) "
        f"= ~{border_delta * 1700.0:.1f} m at game scale -- a visible seam (real breakage?)"
    )
