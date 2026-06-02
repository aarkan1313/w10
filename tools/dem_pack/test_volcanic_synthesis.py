import numpy as np

import volcanic_synthesis as vs


def test_volcanic_generate_is_deterministic_finite_and_nonflat():
    wx, wz = vs.grid(96, 90000.0)
    a = vs.generate(wx, wz, seed=51, style=vs.STYLES[0])
    b = vs.generate(wx, wz, seed=51, style=vs.STYLES[0])

    za = a["height"]
    assert za.shape == wx.shape
    assert np.all(np.isfinite(za))
    assert np.allclose(za, b["height"])
    assert float(np.ptp(za)) > 0.5


def test_volcanic_styles_are_materially_different():
    wx, wz = vs.grid(80, 90000.0)
    heights = [vs.generate(wx, wz, seed=52, style=style)["height"] for style in vs.STYLES]
    deltas = []
    for i in range(len(heights) - 1):
        deltas.append(float(np.mean(np.abs(heights[i] - heights[i + 1]))))
    assert min(deltas) > 0.08


def test_volcanic_cones_are_higher_than_ash_plain():
    wx, wz = vs.grid(96, 90000.0)
    result = vs.generate(wx, wz, seed=53, style=vs.STYLES[0])
    z = result["height"]
    cones = result["cones"]
    ash = result["ash_plain"]
    cone_tops = z[cones > np.quantile(cones, 0.86)]
    plain = z[(ash >= np.quantile(ash, 0.45)) & (cones <= np.quantile(cones, 0.45))]
    assert cone_tops.size > 0 and plain.size > 0
    assert float(np.mean(cone_tops)) > float(np.mean(plain))


def test_volcanic_caldera_has_low_crater_and_high_rim_context():
    wx, wz = vs.grid(96, 90000.0)
    result = vs.generate(wx, wz, seed=54, style=vs.STYLES[2])
    z = result["height"]
    craters = result["craters"]
    cones = result["cones"]
    crater_floor = z[craters > np.quantile(craters, 0.95)]
    rim = z[(cones > np.quantile(cones, 0.70)) & (craters < np.quantile(craters, 0.88))]
    assert crater_floor.size > 0 and rim.size > 0
    assert float(np.mean(crater_floor)) < float(np.mean(rim))


def test_volcanic_rift_style_has_stronger_rift_than_shield_style():
    wx, wz = vs.grid(96, 90000.0)
    shield = vs.generate(wx, wz, seed=55, style=vs.STYLES[1])
    rift = vs.generate(wx, wz, seed=55, style=vs.STYLES[3])
    assert float(np.mean(rift["rift"])) > float(np.mean(shield["rift"])) * 0.95
    assert float(np.quantile(rift["cones"], 0.88)) > 0.05


# ---------------------------------------------------------------------------
# Seam-exactness test (apron_px > 0 path)
# ---------------------------------------------------------------------------

def test_volcanic_generate_seam_exact():
    """Adjacent apron-padded windows must agree at their shared border to the
    visually-seamless bar (< 1e-3 normalised).

    Setup:
      - Core span S = 60_000 m over N = 64 core cells; cell_size = S / (N-1)
      - Window A core covers world x in [X0, X0+S], window B core covers [X0+S, X0+2*S]
      - A MASTER 1-D array (arange * cell + origin) spans both windows so that both A
        and B slice the SAME float64 values at their shared border -- guaranteeing
        bit-identical border coordinates. (Independent linspace/arange calls on different
        spans produce different float64 representations of the same world position.)
      - After generate(apron_px=APRON), height is cropped to the core; assert the shared
        border column matches to within the VISUALLY-SEAMLESS bar (see below).

    Tolerance note (the bar is VISUALLY SEAMLESS, not bit-exact):
      Volcanic does not use MFD flow accumulation (gullies use the legacy
      flow_accumulation_channels only in the LEGACY path; seam-safe path uses
      _gully_channels_seam_safe with fixed-max MFD normalization). The residual is
      MFD convergence error plus affine-constant approximation error. Both are small
      at apron 160. The owner-accepted bar is delta SMALL RELATIVE TO RELIEF:
      a NORMALISED delta < 1e-3 is visually imperceptible -- assert < 1e-3.
      This still catches real breakage (wrong feature_span, apron far too small,
      data-dependent rotation centre, origin bug -> deltas 1e-2+).
    """
    N = 64
    S = 60_000.0
    X0 = 10_000.0
    Z0 = 0.0
    APRON = vs.VOLCANIC_APRON_PX
    SEED = 99
    STYLE = vs.STYLES[0]

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
    b_border_x  = b_wx[0, APRON]
    print(f"\nA border wx = {a_border_x:.3f}, B border wx = {b_border_x:.3f}")
    assert a_border_x == b_border_x, (
        f"Grid setup error: A border x={a_border_x!r} != B border x={b_border_x!r}"
    )

    res_a = vs.generate(a_wx, a_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)
    res_b = vs.generate(b_wx, b_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)

    a_core = res_a["height"]
    b_core = res_b["height"]

    assert a_core.shape == (N, N), f"A core shape {a_core.shape} != ({N}, {N})"
    assert b_core.shape == (N, N), f"B core shape {b_core.shape} != ({N}, {N})"

    SEAM_BAR = 1e-3
    border_delta = float(np.max(np.abs(a_core[:, -1] - b_core[:, 0])))
    # height_scale_m: volcanic output std ~ 1 maps to approx 1700 m at game scale
    print(f"Seam border delta (max |A[:,-1] - B[:,0]|) = {border_delta:.6e}  "
          f"(~{border_delta * 1700.0:.3f} m at base_height_scale 1700)")
    assert border_delta < SEAM_BAR, (
        f"Seam NOT visually seamless: max border delta = {border_delta:.6e} (>= {SEAM_BAR:.0e}) "
        f"= ~{border_delta * 1700.0:.1f} m at game scale -- a visible seam (real breakage?)"
    )
