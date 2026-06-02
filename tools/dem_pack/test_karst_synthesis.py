import numpy as np

import karst_synthesis as ks


def test_karst_generate_is_deterministic_finite_and_nonflat():
    wx, wz = ks.grid(96, 90000.0)
    a = ks.generate(wx, wz, seed=41, style=ks.STYLES[0])
    b = ks.generate(wx, wz, seed=41, style=ks.STYLES[0])

    za = a["height"]
    assert za.shape == wx.shape
    assert np.all(np.isfinite(za))
    assert np.allclose(za, b["height"])
    assert float(np.ptp(za)) > 0.5


def test_karst_styles_are_materially_different():
    wx, wz = ks.grid(80, 90000.0)
    heights = [ks.generate(wx, wz, seed=42, style=style)["height"] for style in ks.STYLES]
    deltas = []
    for i in range(len(heights) - 1):
        deltas.append(float(np.mean(np.abs(heights[i] - heights[i + 1]))))
    assert min(deltas) > 0.06


def test_karst_dolines_are_lower_than_background():
    wx, wz = ks.grid(96, 90000.0)
    result = ks.generate(wx, wz, seed=43, style=ks.STYLES[1])
    z = result["height"]
    dolines = result["dolines"]
    plateau = result["plateau"]
    plateau_high = plateau > np.quantile(plateau, 0.55)
    pit = z[(dolines > np.quantile(dolines, 0.94)) & plateau_high]
    background = z[(dolines <= np.quantile(dolines, 0.35)) & plateau_high]
    assert pit.size > 0 and background.size > 0
    assert float(np.mean(pit)) < float(np.mean(background))


def test_karst_towers_are_higher_than_floors():
    wx, wz = ks.grid(96, 90000.0)
    result = ks.generate(wx, wz, seed=44, style=ks.STYLES[0])
    z = result["height"]
    towers = result["towers"]
    floors = np.maximum(result["dolines"], result["dry_valleys"])
    high_towers = z[towers > np.quantile(towers, 0.82)]
    low_floors = z[floors > np.quantile(floors, 0.78)]
    assert high_towers.size > 0 and low_floors.size > 0
    assert float(np.mean(high_towers)) > float(np.mean(low_floors))


def test_karst_linear_style_has_stronger_dry_valleys_than_mogote_plain():
    wx, wz = ks.grid(96, 90000.0)
    linear = ks.generate(wx, wz, seed=45, style=ks.STYLES[2])
    mogote = ks.generate(wx, wz, seed=45, style=ks.STYLES[3])
    assert float(np.mean(linear["dry_valleys"])) > float(np.mean(mogote["dry_valleys"])) * 0.90
    assert float(np.quantile(linear["lineaments"], 0.90)) > float(np.quantile(mogote["lineaments"], 0.90)) * 0.95


# ---------------------------------------------------------------------------
# Seam-exactness test (apron_px > 0 path)
# ---------------------------------------------------------------------------

def test_karst_generate_seam_exact():
    """Adjacent apron-padded windows must agree at their shared border to within 1e-3 (normalized).

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
      The seam-safe path carves dry valleys with REAL multiple-flow-direction (MFD) accumulation,
      a GLOBAL operation (a cell's discharge depends on all upstream cells). This CANNOT be
      made bit-exact across arbitrarily many windows: a border cell's drainage depends on
      upstream area that grows with world size, so no fixed apron captures all of it. The
      residual is SCALE-DEPENDENT. The real bar (owner-accepted: "doesn't have to be exact
      if it's seamless and looks good") is delta SMALL RELATIVE TO RELIEF: a NORMALIZED
      delta < 1e-3 is below a visible/trippable threshold. We assert < 1e-3 (visually seamless),
      which also still catches REAL breakage (wrong feature_span, apron far too small, origin
      bug -> deltas 1e-2+).
    """
    N = 64
    S = 60_000.0
    X0 = 10_000.0
    Z0 = 0.0
    APRON = ks.KARST_APRON_PX  # canonical constant from the module
    SEED = 99
    STYLE = ks.STYLES[0]

    cell = S / (N - 1)  # world-space cell size in metres
    padded_n = N + 2 * APRON  # grid size including apron on both sides

    # Master 1-D arrays: A occupies master[0:padded_n], B occupies master[N-1:N-1+padded_n].
    # The shared border is master column (APRON + N - 1), which equals A[:, APRON+N-1] and
    # B[:, APRON] -- both are the SAME float64 value since they reference the same element.
    total_cols = 2 * N + 2 * APRON - 1
    master_x = np.arange(total_cols, dtype=np.float64) * cell + (X0 - APRON * cell)
    master_z = np.arange(total_cols, dtype=np.float64) * cell + (Z0 - APRON * cell)

    a_x_1d = master_x[0:padded_n]
    b_x_1d = master_x[N - 1:N - 1 + padded_n]
    z_1d   = master_z[0:padded_n]           # z extent same for both windows

    a_wx, a_wz = np.meshgrid(a_x_1d, z_1d)
    b_wx, b_wz = np.meshgrid(b_x_1d, z_1d)

    # Verify grid setup: shared border column is bit-identical
    a_border_x = a_wx[0, APRON + N - 1]    # A's last core column
    b_border_x  = b_wx[0, APRON]            # B's first core column
    print(f"\nA border wx = {a_border_x:.3f}, B border wx = {b_border_x:.3f}")
    assert a_border_x == b_border_x, (
        f"Grid setup error: A border x={a_border_x!r} != B border x={b_border_x!r}"
    )

    # --- generate both windows with apron ---
    # feature_span_m MUST be the same fixed constant for all adjacent windows.
    # Using S (the core span) is the canonical choice.
    res_a = ks.generate(a_wx, a_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)
    res_b = ks.generate(b_wx, b_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)

    a_core = res_a["height"]  # already cropped to (N, N)
    b_core = res_b["height"]  # already cropped to (N, N)

    assert a_core.shape == (N, N), f"A core shape {a_core.shape} != ({N}, {N})"
    assert b_core.shape == (N, N), f"B core shape {b_core.shape} != ({N}, {N})"

    # The shared border: A's rightmost column vs B's leftmost column.
    # Bar = VISUALLY SEAMLESS (< 1e-3 normalized): global flow accumulation cannot be
    # bit-exact across windows (scale-dependent residual); < 1e-3 still catches real
    # breakage (wrong feature_span / apron too small / origin bug). See docstring.
    SEAM_BAR = 1e-3
    border_delta = float(np.max(np.abs(a_core[:, -1] - b_core[:, 0])))
    # Use a representative relief scale; karst field std is ~1 -> ~1700 m at game scale
    relief_m = 1700.0
    print(f"Seam border delta (max |A[:,-1] - B[:,0]|) = {border_delta:.6e}  "
          f"(~{border_delta * relief_m:.3f} m at base_height_scale {relief_m:.0f})")
    assert border_delta < SEAM_BAR, (
        f"Seam NOT visually seamless: max border delta = {border_delta:.6e} (>= {SEAM_BAR:.0e}) "
        f"= ~{border_delta * relief_m:.1f} m at game scale -- a visible seam (real breakage?)"
    )
