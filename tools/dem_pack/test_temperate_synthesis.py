import numpy as np

import temperate_synthesis as ts


# ---------------------------------------------------------------------------
# Seam-exactness test (apron_px > 0 path)
# ---------------------------------------------------------------------------

def test_temperate_generate_seam_exact():
    """Adjacent apron-padded windows must agree at their shared border (visually seamless).

    Setup:
      - Core span S = 60_000 m over N = 64 core cells; cell_size = S / (N-1)
      - Window A core covers world x in [X0, X0+S], window B core covers [X0+S, X0+2*S]
      - A MASTER 1-D array (arange * cell + origin) spans both windows so that both A and B
        slice the SAME float64 values at their shared border -- guaranteeing bit-identical
        border coordinates. (Independent linspace/arange calls on different spans produce
        different float64 representations of the same world position.)
      - After generate(apron_px=APRON), height is cropped to the core; assert the shared
        border column matches to within the VISUALLY-SEAMLESS bar (< 1e-3 normalized).

    Tolerance note (the bar is VISUALLY SEAMLESS, not bit-exact):
      The seam-safe path carves valleys with REAL multiple-flow-direction (MFD) accumulation,
      a GLOBAL operation. This CANNOT be made bit-exact across arbitrarily many windows: a
      border cell's drainage depends on upstream area that grows with world size. The
      residual is SCALE-DEPENDENT. The owner-accepted bar is 'seamless + looks good',
      not bit-identical. < 1e-3 still catches real breakage (wrong feature_span / apron
      too small / origin bug). See mountain_synthesis docstring for full discussion.
    """
    N = 64
    S = 60_000.0
    X0 = 10_000.0
    Z0 = 0.0
    APRON = ts.TEMPERATE_APRON_PX
    SEED = 99
    STYLE = ts.STYLES[0]

    cell = S / (N - 1)
    padded_n = N + 2 * APRON

    # Master 1-D arrays: A occupies master[0:padded_n], B occupies master[N-1:N-1+padded_n].
    total_cols = 2 * N + 2 * APRON - 1
    master_x = np.arange(total_cols, dtype=np.float64) * cell + (X0 - APRON * cell)
    master_z = np.arange(total_cols, dtype=np.float64) * cell + (Z0 - APRON * cell)

    a_x_1d = master_x[0:padded_n]
    b_x_1d = master_x[N - 1:N - 1 + padded_n]
    z_1d   = master_z[0:padded_n]

    a_wx, a_wz = np.meshgrid(a_x_1d, z_1d)
    b_wx, b_wz = np.meshgrid(b_x_1d, z_1d)

    # Verify grid setup: shared border column is bit-identical.
    a_border_x = a_wx[0, APRON + N - 1]
    b_border_x = b_wx[0, APRON]
    print(f"\nA border wx = {a_border_x:.3f}, B border wx = {b_border_x:.3f}")
    assert a_border_x == b_border_x, (
        f"Grid setup error: A border x={a_border_x!r} != B border x={b_border_x!r}"
    )

    res_a = ts.generate(a_wx, a_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)
    res_b = ts.generate(b_wx, b_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)

    a_core = res_a["height"]
    b_core = res_b["height"]

    assert a_core.shape == (N, N), f"A core shape {a_core.shape} != ({N}, {N})"
    assert b_core.shape == (N, N), f"B core shape {b_core.shape} != ({N}, {N})"

    SEAM_BAR = 1e-3
    border_delta = float(np.max(np.abs(a_core[:, -1] - b_core[:, 0])))
    # Approximate game-scale metres: final field std ~1 maps to ~700m for temperate terrain.
    print(f"Seam border delta (max |A[:,-1] - B[:,0]|) = {border_delta:.6e}  "
          f"(~{border_delta * 700.0:.3f} m at game scale)")
    assert border_delta < SEAM_BAR, (
        f"Seam NOT visually seamless: max border delta = {border_delta:.6e} (>= {SEAM_BAR:.0e}) "
        f"= ~{border_delta * 700.0:.1f} m at game scale -- a visible seam (real breakage?)"
    )


def test_temperate_generate_is_deterministic_finite_and_nonflat():
    wx, wz = ts.grid(96, 90000.0)
    a = ts.generate(wx, wz, seed=101, style=ts.STYLES[0])
    b = ts.generate(wx, wz, seed=101, style=ts.STYLES[0])

    za = a["height"]
    assert za.shape == wx.shape
    assert np.all(np.isfinite(za))
    assert np.allclose(za, b["height"])
    assert float(np.ptp(za)) > 0.50


def test_temperate_styles_are_materially_different():
    wx, wz = ts.grid(80, 90000.0)
    heights = [ts.generate(wx, wz, seed=102, style=style)["height"] for style in ts.STYLES]
    deltas = []
    for i in range(len(heights) - 1):
        deltas.append(float(np.mean(np.abs(heights[i] - heights[i + 1]))))
    assert min(deltas) > 0.055


def test_temperate_appalachian_has_more_ridges_than_rounded_hills():
    wx, wz = ts.grid(96, 90000.0)
    ridges = ts.generate(wx, wz, seed=103, style=ts.STYLES[0])
    hills = ts.generate(wx, wz, seed=103, style=ts.STYLES[1])
    assert float(np.mean(ridges["ridges"])) > float(np.mean(hills["ridges"])) * 1.8


def test_temperate_rounded_hills_have_more_hills_than_appalachian():
    wx, wz = ts.grid(96, 90000.0)
    ridges = ts.generate(wx, wz, seed=104, style=ts.STYLES[0])
    hills = ts.generate(wx, wz, seed=104, style=ts.STYLES[1])
    assert float(np.mean(hills["hills"])) > float(np.mean(ridges["hills"])) * 1.25


def test_temperate_glaciated_upland_has_more_upland_than_rounded_hills():
    wx, wz = ts.grid(96, 90000.0)
    rounded = ts.generate(wx, wz, seed=105, style=ts.STYLES[1])
    upland = ts.generate(wx, wz, seed=105, style=ts.STYLES[2])
    assert float(np.mean(upland["upland"])) > float(np.mean(rounded["upland"])) * 1.4


def test_temperate_valleys_are_lower_than_ridges():
    wx, wz = ts.grid(96, 90000.0)
    result = ts.generate(wx, wz, seed=106, style=ts.STYLES[0])
    z = result["height"]
    ridge_h = z[result["ridges"] > np.quantile(result["ridges"], 0.78)]
    valley_h = z[result["valleys"] > np.quantile(result["valleys"], 0.78)]
    assert ridge_h.size > 0 and valley_h.size > 0
    assert float(np.mean(ridge_h)) > float(np.mean(valley_h))
