import numpy as np

import grassland_synthesis as gs
from grassland_synthesis import GRASSLAND_APRON_PX


def test_grassland_generate_is_deterministic_finite_and_nonflat():
    wx, wz = gs.grid(96, 90000.0)
    a = gs.generate(wx, wz, seed=71, style=gs.STYLES[0])
    b = gs.generate(wx, wz, seed=71, style=gs.STYLES[0])

    za = a["height"]
    assert za.shape == wx.shape
    assert np.all(np.isfinite(za))
    assert np.allclose(za, b["height"])
    assert float(np.ptp(za)) > 0.45


def test_grassland_styles_are_materially_different():
    wx, wz = gs.grid(80, 90000.0)
    heights = [gs.generate(wx, wz, seed=72, style=style)["height"] for style in gs.STYLES]
    deltas = []
    for i in range(len(heights) - 1):
        deltas.append(float(np.mean(np.abs(heights[i] - heights[i + 1]))))
    assert min(deltas) > 0.055


def test_grassland_sandhill_style_has_more_sandhills_than_prairie():
    wx, wz = gs.grid(96, 90000.0)
    prairie = gs.generate(wx, wz, seed=73, style=gs.STYLES[0])
    sandhills = gs.generate(wx, wz, seed=73, style=gs.STYLES[1])
    assert float(np.mean(sandhills["sandhills"])) > float(np.mean(prairie["sandhills"])) * 2.0


def test_grassland_dry_steppe_has_more_pans_than_rolling_prairie():
    wx, wz = gs.grid(96, 90000.0)
    prairie = gs.generate(wx, wz, seed=74, style=gs.STYLES[0])
    dry = gs.generate(wx, wz, seed=74, style=gs.STYLES[2])
    assert float(np.mean(dry["pans"])) > float(np.mean(prairie["pans"])) * 2.0


def test_grassland_chapada_has_stronger_escarpments_than_steppe():
    wx, wz = gs.grid(96, 90000.0)
    steppe = gs.generate(wx, wz, seed=75, style=gs.STYLES[2])
    chapada = gs.generate(wx, wz, seed=75, style=gs.STYLES[3])
    assert float(np.mean(chapada["escarpments"])) > float(np.mean(steppe["escarpments"])) * 2.0


def test_grassland_draws_and_pans_are_lower_than_swells():
    wx, wz = gs.grid(96, 90000.0)
    result = gs.generate(wx, wz, seed=76, style=gs.STYLES[0])
    z = result["height"]
    high_swells = z[result["swells"] > np.quantile(result["swells"], 0.78)]
    low_floor = z[np.maximum(result["draws"], result["pans"]) > np.quantile(np.maximum(result["draws"], result["pans"]), 0.78)]
    assert high_swells.size > 0 and low_floor.size > 0
    assert float(np.mean(high_swells)) > float(np.mean(low_floor))


# ---------------------------------------------------------------------------
# Seam-exactness test (apron_px > 0 path)
# ---------------------------------------------------------------------------

def test_grassland_generate_seam_exact():
    """Adjacent apron-padded windows must agree at their shared border (visually seamless).

    Setup:
      - Core span S = 60_000 m over N = 64 core cells; cell_size = S / (N-1)
      - Window A core covers world x in [X0, X0+S], window B core covers [X0+S, X0+2*S]
      - A MASTER 1-D array (arange * cell + origin) spans both windows so that both A and B
        slice the SAME float64 values at their shared border -- guaranteeing bit-identical
        border coordinates. (Independent linspace/arange calls on different spans produce
        different float64 representations of the same world position.)
      - After generate(apron_px=GRASSLAND_APRON_PX), height is cropped to the core;
        assert the shared border column matches within the VISUALLY-SEAMLESS bar (< 1e-3
        normalized ≈ < ~1.7 m at game scale using base_height_scale 1700).

    Tolerance note (the bar is VISUALLY SEAMLESS, not bit-exact):
      The seam-safe path carves draws with REAL multiple-flow-direction (MFD) accumulation,
      a GLOBAL operation. This CANNOT be made bit-exact across arbitrarily many windows:
      a border cell's drainage depends on upstream area that grows with world size. The
      residual is SCALE-DEPENDENT; the owner-accepted bar is 'seamless + looks good', not
      bit-identical. A normalized delta < 1e-3 is < ~1.7 m at game scale -- invisible.
      This threshold still catches REAL breakage (wrong feature_span, apron far too small,
      origin bug -> deltas 1e-2+).
    """
    N = 64
    S = 60_000.0
    X0 = 10_000.0
    Z0 = 0.0
    APRON = GRASSLAND_APRON_PX
    SEED = 99
    STYLE = gs.STYLES[0]

    cell = S / (N - 1)
    padded_n = N + 2 * APRON

    # Master 1-D arrays: A occupies master[0:padded_n], B occupies master[N-1:N-1+padded_n].
    # Shared border is master column (APRON + N - 1) -- same float64 in both slices.
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

    # feature_span_m must be the same fixed constant for all adjacent windows.
    res_a = gs.generate(a_wx, a_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)
    res_b = gs.generate(b_wx, b_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)

    a_core = res_a["height"]
    b_core = res_b["height"]

    assert a_core.shape == (N, N), f"A core shape {a_core.shape} != ({N}, {N})"
    assert b_core.shape == (N, N), f"B core shape {b_core.shape} != ({N}, {N})"

    SEAM_BAR = 1e-3
    border_delta = float(np.max(np.abs(a_core[:, -1] - b_core[:, 0])))
    print(f"Seam border delta (max |A[:,-1] - B[:,0]|) = {border_delta:.6e}  "
          f"(~{border_delta * 1700.0:.3f} m at base_height_scale 1700)")
    assert border_delta < SEAM_BAR, (
        f"Seam NOT visually seamless: max border delta = {border_delta:.6e} (>= {SEAM_BAR:.0e}) "
        f"= ~{border_delta * 1700.0:.1f} m at game scale -- a visible seam (real breakage?)"
    )
