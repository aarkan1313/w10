import numpy as np

import coast_synthesis as cs


def test_coast_generate_is_deterministic_finite_and_nonflat():
    wx, wz = cs.grid(96, 90000.0)
    a = cs.generate(wx, wz, seed=81, style=cs.STYLES[0])
    b = cs.generate(wx, wz, seed=81, style=cs.STYLES[0])

    za = a["height"]
    assert za.shape == wx.shape
    assert np.all(np.isfinite(za))
    assert np.allclose(za, b["height"])
    assert float(np.ptp(za)) > 0.55


def test_coast_styles_are_materially_different():
    wx, wz = cs.grid(80, 90000.0)
    heights = [cs.generate(wx, wz, seed=82, style=style)["height"] for style in cs.STYLES]
    deltas = []
    for i in range(len(heights) - 1):
        deltas.append(float(np.mean(np.abs(heights[i] - heights[i + 1]))))
    assert min(deltas) > 0.055


def test_coast_has_low_sea_and_higher_land():
    wx, wz = cs.grid(96, 90000.0)
    result = cs.generate(wx, wz, seed=83, style=cs.STYLES[0])
    z = result["height"]
    sea = z[result["sea"] > 0.72]
    land = z[result["sea"] < 0.28]
    assert sea.size > 0 and land.size > 0
    assert float(np.mean(land)) > float(np.mean(sea))


def test_fjord_style_has_more_channels_than_desert_scarp():
    wx, wz = cs.grid(96, 90000.0)
    fjord = cs.generate(wx, wz, seed=84, style=cs.STYLES[1])
    desert = cs.generate(wx, wz, seed=84, style=cs.STYLES[3])
    assert float(np.mean(fjord["channels"])) > float(np.mean(desert["channels"])) * 1.25


def test_ria_style_has_more_islands_than_cliffed_style():
    wx, wz = cs.grid(96, 90000.0)
    cliffed = cs.generate(wx, wz, seed=85, style=cs.STYLES[0])
    ria = cs.generate(wx, wz, seed=85, style=cs.STYLES[2])
    assert float(np.mean(ria["islands"])) > float(np.mean(cliffed["islands"])) * 1.45


def test_cliffed_style_has_coastal_scarp():
    wx, wz = cs.grid(96, 90000.0)
    result = cs.generate(wx, wz, seed=86, style=cs.STYLES[0])
    scarp = result["scarp"]
    assert float(np.quantile(scarp, 0.95)) > 0.35


# ---------------------------------------------------------------------------
# Seam-exactness test (apron_px > 0 path)
# ---------------------------------------------------------------------------

def test_coast_generate_seam_exact():
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
      The seam-safe path carves channels with REAL MFD flow accumulation, a GLOBAL
      operation that cannot be bit-exact across arbitrary windows (scale-dependent
      residual). The owner-accepted bar is "seamless + looks good", not bit-identical.
      < 1e-3 still catches real breakage (wrong feature_span / apron too small / origin bug).
      See mountain_synthesis docstring and test_mountain_generate_seam_exact for full rationale.
    """
    N = 64
    S = 60_000.0
    X0 = 10_000.0
    Z0 = 0.0
    APRON = cs.COAST_APRON_PX
    SEED = 99
    STYLE = cs.STYLES[0]

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

    # Verify grid setup: shared border column is bit-identical
    a_border_x = a_wx[0, APRON + N - 1]
    b_border_x = b_wx[0, APRON]
    print(f"\nA border wx = {a_border_x:.3f}, B border wx = {b_border_x:.3f}")
    assert a_border_x == b_border_x, (
        f"Grid setup error: A border x={a_border_x!r} != B border x={b_border_x!r}"
    )

    res_a = cs.generate(a_wx, a_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)
    res_b = cs.generate(b_wx, b_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)

    a_core = res_a["height"]
    b_core = res_b["height"]

    assert a_core.shape == (N, N), f"A core shape {a_core.shape} != ({N}, {N})"
    assert b_core.shape == (N, N), f"B core shape {b_core.shape} != ({N}, {N})"

    SEAM_BAR = 1e-3
    border_delta = float(np.max(np.abs(a_core[:, -1] - b_core[:, 0])))
    height_scale = 1700.0
    print(f"Coast seam border delta (max |A[:,-1] - B[:,0]|) = {border_delta:.6e}  "
          f"(~{border_delta * height_scale:.3f} m at height_scale {height_scale})")
    assert border_delta < SEAM_BAR, (
        f"Seam NOT visually seamless: max border delta = {border_delta:.6e} (>= {SEAM_BAR:.0e}) "
        f"= ~{border_delta * height_scale:.1f} m at game scale -- a visible seam"
    )
