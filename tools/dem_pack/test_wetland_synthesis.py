import numpy as np

import wetland_synthesis as ws


# ---------------------------------------------------------------------------
# Seam-exactness test (apron_px > 0 path)
# ---------------------------------------------------------------------------

def test_wetland_generate_seam_exact():
    """Adjacent apron-padded windows must agree at their shared border (visually seamless).

    Setup mirrors test_mountain_generate_seam_exact exactly:
      - Core span S = 60_000 m over N = 64 core cells; cell_size = S / (N-1)
      - Window A core covers world x in [X0, X0+S], window B core covers [X0+S, X0+2*S]
      - A MASTER 1-D array (arange * cell + origin) spans both windows so that both A
        and B slice the SAME float64 values at their shared border.
      - After generate(apron_px=APRON), height is cropped to the core; assert the shared
        border column matches to within the VISUALLY-SEAMLESS bar (< 1e-3 normalized).

    Tolerance note: wetland uses REAL MFD flow accumulation for channel routing -- a
    global operation that cannot be made bit-exact across windows on any finite apron.
    The residual is SCALE-DEPENDENT (same finding as mountain). The bar is VISUALLY
    SEAMLESS (< 1e-3 normalized), not bit-identical. This still catches real breakage
    (wrong feature_span, apron far too small, origin bug -> deltas 1e-2+).
    """
    N = 64
    S = 60_000.0
    X0 = 10_000.0
    Z0 = 0.0
    APRON = ws.WETLAND_APRON_PX
    SEED = 77
    STYLE = ws.STYLES[0]

    cell = S / (N - 1)
    padded_n = N + 2 * APRON

    # Master 1-D arrays: shared border is bit-identical by construction.
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

    res_a = ws.generate(a_wx, a_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)
    res_b = ws.generate(b_wx, b_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)

    a_core = res_a["height"]
    b_core = res_b["height"]

    assert a_core.shape == (N, N), f"A core shape {a_core.shape} != ({N}, {N})"
    assert b_core.shape == (N, N), f"B core shape {b_core.shape} != ({N}, {N})"

    SEAM_BAR = 1e-3
    border_delta = float(np.max(np.abs(a_core[:, -1] - b_core[:, 0])))
    # Height std ~ 1 -> ~1700 m at game scale; < 1e-3 normalized = < ~1.7 m (invisible).
    height_scale_m = 260.0  # wetland is low relief; use conservative estimate
    print(f"Seam border delta (max |A[:,-1] - B[:,0]|) = {border_delta:.6e}  "
          f"(~{border_delta * height_scale_m:.3f} m at {height_scale_m}m scale)")
    assert border_delta < SEAM_BAR, (
        f"Seam NOT visually seamless: max border delta = {border_delta:.6e} (>= {SEAM_BAR:.0e}) "
        f"= ~{border_delta * height_scale_m:.1f} m at game scale -- a visible seam"
    )


def test_wetland_generate_is_deterministic_finite_and_nonflat():
    wx, wz = ws.grid(96, 90000.0)
    a = ws.generate(wx, wz, seed=121, style=ws.STYLES[0])
    b = ws.generate(wx, wz, seed=121, style=ws.STYLES[0])

    za = a["height"]
    assert za.shape == wx.shape
    assert np.all(np.isfinite(za))
    assert np.allclose(za, b["height"])
    assert float(np.ptp(za)) > 0.45


def test_wetland_styles_are_materially_different():
    wx, wz = ws.grid(80, 90000.0)
    heights = [ws.generate(wx, wz, seed=122, style=style)["height"] for style in ws.STYLES]
    deltas = []
    for i in range(len(heights) - 1):
        deltas.append(float(np.mean(np.abs(heights[i] - heights[i + 1]))))
    assert min(deltas) > 0.045


def test_wetland_delta_has_more_channels_than_peat_bog():
    wx, wz = ws.grid(96, 90000.0)
    delta = ws.generate(wx, wz, seed=123, style=ws.STYLES[0])
    bog = ws.generate(wx, wz, seed=123, style=ws.STYLES[2])
    assert float(np.mean(delta["channels"])) > float(np.mean(bog["channels"])) * 1.5


def test_wetland_meander_has_more_levees_than_peat_bog():
    wx, wz = ws.grid(96, 90000.0)
    meander = ws.generate(wx, wz, seed=124, style=ws.STYLES[1])
    bog = ws.generate(wx, wz, seed=124, style=ws.STYLES[2])
    assert float(np.mean(meander["levees"])) > float(np.mean(bog["levees"])) * 2.0


def test_wetland_peat_bog_has_more_basin_than_delta():
    wx, wz = ws.grid(96, 90000.0)
    delta = ws.generate(wx, wz, seed=125, style=ws.STYLES[0])
    bog = ws.generate(wx, wz, seed=125, style=ws.STYLES[2])
    assert float(np.mean(bog["basin"])) > float(np.mean(delta["basin"])) * 1.4


def test_wetland_channels_are_lower_than_floodplain():
    wx, wz = ws.grid(96, 90000.0)
    result = ws.generate(wx, wz, seed=126, style=ws.STYLES[1])
    z = result["height"]
    channel_h = z[result["channels"] > np.quantile(result["channels"], 0.82)]
    non_channel_h = z[result["channels"] < np.quantile(result["channels"], 0.25)]
    assert channel_h.size > 0 and non_channel_h.size > 0
    assert float(np.mean(non_channel_h)) > float(np.mean(channel_h))
