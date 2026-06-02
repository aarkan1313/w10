import numpy as np

import rainforest_synthesis as rs


# ---------------------------------------------------------------------------
# Seam-exactness test (apron_px > 0 path)
# ---------------------------------------------------------------------------

def test_rainforest_generate_seam_exact():
    """Adjacent apron-padded windows must agree at their shared border (visually-seamless bar).

    Setup mirrors test_mountain_generate_seam_exact exactly:
      - Core span S = 60_000 m over N = 64 core cells; cell_size = S / (N-1)
      - Window A core covers world x in [X0, X0+S], window B core covers [X0+S, X0+2*S]
      - A MASTER 1-D arange spans both windows so that both A and B slice the SAME float64
        values at their shared border -- guaranteeing bit-identical border coordinates.
      - After generate(apron_px=APRON), height is cropped to the core; assert the shared
        border column matches to within the VISUALLY-SEAMLESS bar (< 1e-3 normalized).

    Tolerance note: global flow accumulation (MFD) cannot be made bit-exact across arbitrary
    windows (scale-dependent residual). The owner-accepted bar is 'seamless + looks good',
    not bit-identical. < 1e-3 normalized still catches real breakage (wrong feature_span,
    apron far too small, origin bug) which cause deltas 1e-2+.
    """
    N = 64
    S = 60_000.0
    X0 = 10_000.0
    Z0 = 0.0
    APRON = rs.RAINFOREST_APRON_PX
    SEED = 91
    STYLE = rs.STYLES[0]

    cell = S / (N - 1)
    padded_n = N + 2 * APRON

    # Master 1-D arrays so both windows share the SAME float64 values at the border.
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

    # Generate both windows with apron; feature_span_m MUST be the same fixed constant.
    res_a = rs.generate(a_wx, a_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)
    res_b = rs.generate(b_wx, b_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)

    a_core = res_a["height"]
    b_core = res_b["height"]

    assert a_core.shape == (N, N), f"A core shape {a_core.shape} != ({N}, {N})"
    assert b_core.shape == (N, N), f"B core shape {b_core.shape} != ({N}, {N})"

    # Visually-seamless bar: < 1e-3 normalized (same as mountain, grassland, desert).
    # At typical game height scale ~1700 m, 1e-3 normalized ~ 1.7 m -- invisible.
    SEAM_BAR = 1e-3
    border_delta = float(np.max(np.abs(a_core[:, -1] - b_core[:, 0])))
    field_std = float(np.std(a_core))
    delta_m = border_delta * 1700.0
    print(f"Seam border delta (max |A[:,-1] - B[:,0]|) = {border_delta:.6e}  "
          f"(~{delta_m:.3f} m at scale 1700; field std={field_std:.4f})")
    assert border_delta < SEAM_BAR, (
        f"Seam NOT visually seamless: max border delta = {border_delta:.6e} (>= {SEAM_BAR:.0e}) "
        f"= ~{delta_m:.1f} m at game scale -- a visible seam (real breakage?)"
    )


def test_rainforest_generate_is_deterministic_finite_and_nonflat():
    wx, wz = rs.grid(96, 90000.0)
    a = rs.generate(wx, wz, seed=91, style=rs.STYLES[0])
    b = rs.generate(wx, wz, seed=91, style=rs.STYLES[0])

    za = a["height"]
    assert za.shape == wx.shape
    assert np.all(np.isfinite(za))
    assert np.allclose(za, b["height"])
    assert float(np.ptp(za)) > 0.55


def test_rainforest_styles_are_materially_different():
    wx, wz = rs.grid(80, 90000.0)
    heights = [rs.generate(wx, wz, seed=92, style=style)["height"] for style in rs.STYLES]
    deltas = []
    for i in range(len(heights) - 1):
        deltas.append(float(np.mean(np.abs(heights[i] - heights[i + 1]))))
    assert min(deltas) > 0.055


def test_rainforest_shield_style_has_more_plateau_than_foothills():
    wx, wz = rs.grid(96, 90000.0)
    shield = rs.generate(wx, wz, seed=93, style=rs.STYLES[1])
    foothills = rs.generate(wx, wz, seed=93, style=rs.STYLES[2])
    assert float(np.mean(shield["plateau"])) > float(np.mean(foothills["plateau"])) * 1.5


def test_rainforest_foothills_have_more_ridges_than_lowland():
    wx, wz = rs.grid(96, 90000.0)
    foothills = rs.generate(wx, wz, seed=94, style=rs.STYLES[2])
    lowland = rs.generate(wx, wz, seed=94, style=rs.STYLES[3])
    assert float(np.mean(foothills["ridges"])) > float(np.mean(lowland["ridges"])) * 1.6


def test_rainforest_lowland_has_more_lowland_than_hills():
    wx, wz = rs.grid(96, 90000.0)
    hills = rs.generate(wx, wz, seed=95, style=rs.STYLES[0])
    lowland = rs.generate(wx, wz, seed=95, style=rs.STYLES[3])
    assert float(np.mean(lowland["lowland"])) > float(np.mean(hills["lowland"])) * 1.6


def test_rainforest_drainage_cuts_below_ridges():
    wx, wz = rs.grid(96, 90000.0)
    result = rs.generate(wx, wz, seed=96, style=rs.STYLES[0])
    z = result["height"]
    ridge_h = z[result["ridges"] > np.quantile(result["ridges"], 0.78)]
    drain_h = z[result["drainage"] > np.quantile(result["drainage"], 0.78)]
    assert ridge_h.size > 0 and drain_h.size > 0
    assert float(np.mean(ridge_h)) > float(np.mean(drain_h))
