import numpy as np
import pytest

import glacial_synthesis as gs


def test_glacial_generate_is_deterministic_finite_and_nonflat():
    wx, wz = gs.grid(96, 90000.0)
    a = gs.generate(wx, wz, seed=31, style=gs.STYLES[0])
    b = gs.generate(wx, wz, seed=31, style=gs.STYLES[0])

    za = a["height"]
    assert za.shape == wx.shape
    assert np.all(np.isfinite(za))
    assert np.allclose(za, b["height"])
    assert float(np.ptp(za)) > 0.5


def test_glacial_styles_are_materially_different():
    wx, wz = gs.grid(80, 90000.0)
    heights = [gs.generate(wx, wz, seed=32, style=style)["height"] for style in gs.STYLES]
    deltas = []
    for i in range(len(heights) - 1):
        deltas.append(float(np.mean(np.abs(heights[i] - heights[i + 1]))))
    assert min(deltas) > 0.06


def test_glacial_troughs_are_carved_lower_than_background():
    wx, wz = gs.grid(96, 90000.0)
    result = gs.generate(wx, wz, seed=33, style=gs.STYLES[0])
    z = result["height"]
    troughs = np.maximum(result["primary_troughs"], result["tributaries"])
    carved = z[troughs > np.quantile(troughs, 0.82)]
    background = z[troughs <= np.quantile(troughs, 0.35)]
    assert carved.size > 0 and background.size > 0
    assert float(np.mean(carved)) < float(np.mean(background))


def test_glacial_trough_floors_are_smoother_than_ridge_walls():
    wx, wz = gs.grid(96, 90000.0)
    result = gs.generate(wx, wz, seed=34, style=gs.STYLES[2])
    z = result["height"]
    floor = result["trough_floor"]
    walls = result["relief_envelope"] * (1.0 - result["trough_floor"])

    gy, gx = np.gradient(z)
    slope = np.sqrt(gx * gx + gy * gy)
    floor_slope = slope[floor >= np.quantile(floor, 0.76)]
    wall_slope = slope[walls >= np.quantile(walls, 0.76)]
    assert floor_slope.size > 0 and wall_slope.size > 0
    assert float(np.mean(floor_slope)) < float(np.mean(wall_slope))


def test_glacial_icefields_are_broad_high_smooth_regions():
    wx, wz = gs.grid(96, 90000.0)
    result = gs.generate(wx, wz, seed=35, style=gs.STYLES[1])
    z = result["height"]
    ice = result["icefield"]
    high_ice = z[ice >= np.quantile(ice, 0.78)]
    low_nonice = z[ice <= np.quantile(ice, 0.35)]
    assert high_ice.size > 0 and low_nonice.size > 0
    assert float(np.mean(high_ice)) > float(np.mean(low_nonice))

    gy, gx = np.gradient(z)
    slope = np.sqrt(gx * gx + gy * gy)
    assert float(np.mean(slope[ice >= np.quantile(ice, 0.78)])) < float(np.quantile(slope, 0.80))


# ---------------------------------------------------------------------------
# Seam-exactness tests (apron_px > 0 path)
# ---------------------------------------------------------------------------

def test_glacial_apron_px_required_when_nonzero():
    """generate() must raise if apron_px > 0 but feature_span_m is not provided."""
    wx, wz = gs.grid(16, 60_000.0)
    with pytest.raises(ValueError, match="feature_span_m is required"):
        gs.generate(wx, wz, seed=0, apron_px=gs.GLACIAL_APRON_PX)


def test_glacial_apron_output_shape():
    """With apron_px > 0, output core is (N, N) not (N + 2*a, N + 2*a)."""
    N = 16
    S = 60_000.0
    APRON = gs.GLACIAL_APRON_PX
    cell = S / (N - 1)
    padded_n = N + 2 * APRON
    total_cols = padded_n
    x_1d = np.arange(total_cols, dtype=np.float64) * cell
    z_1d = np.arange(total_cols, dtype=np.float64) * cell
    wx, wz = np.meshgrid(x_1d, z_1d)
    result = gs.generate(wx, wz, seed=0, apron_px=APRON, feature_span_m=S)
    assert result["height"].shape == (N, N), (
        f"Expected core ({N}, {N}), got {result['height'].shape}"
    )


def test_glacial_generate_seam_exact():
    """Adjacent apron-padded windows must agree at their shared border to float32 epsilon.

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
      The seam-safe path carves troughs with REAL multiple-flow-direction (MFD) accumulation,
      a GLOBAL operation (a cell's discharge depends on all upstream cells). This CANNOT be
      made bit-exact across arbitrarily many windows: a border cell's drainage depends on
      upstream area that grows with world size, so no fixed apron captures all of it. The
      residual is SCALE-DEPENDENT -- a single 2-window seam (like this test) converges very
      small at apron 160, but a many-window world may reach ~1e-4 normalized (as measured for
      mountain in export_godot_mountain_seamsafe_chunks). The real bar (owner-accepted:
      "doesn't have to be exact if it's seamless and looks good") is delta SMALL RELATIVE TO
      RELIEF: with std ~1, a NORMALIZED delta < 1e-3 maps to < ~1.7 m at game scale --
      invisible/untrippable on glacier terrain. We assert < 1e-3 (visually seamless), which
      still catches REAL breakage (wrong feature_span, apron far too small, origin bug ->
      deltas 1e-2+). See mountain_synthesis docstring for the full derivation.
    """
    N = 64
    S = 60_000.0
    X0 = 10_000.0
    Z0 = 0.0
    APRON = gs.GLACIAL_APRON_PX  # canonical constant from the module
    SEED = 99
    STYLE = gs.STYLES[0]

    cell = S / (N - 1)   # world-space cell size in metres
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
    res_a = gs.generate(a_wx, a_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)
    res_b = gs.generate(b_wx, b_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)

    a_core = res_a["height"]  # already cropped to (N, N)
    b_core = res_b["height"]  # already cropped to (N, N)

    assert a_core.shape == (N, N), f"A core shape {a_core.shape} != ({N}, {N})"
    assert b_core.shape == (N, N), f"B core shape {b_core.shape} != ({N}, {N})"

    # The shared border: A's rightmost column vs B's leftmost column.
    # Bar = VISUALLY SEAMLESS (< 1e-3 normalized): global flow accumulation cannot be
    # bit-exact across windows (scale-dependent residual); the owner-accepted bar is
    # "seamless + looks good", not bit-identical. < 1e-3 still catches real breakage.
    SEAM_BAR = 1e-3
    border_delta = float(np.max(np.abs(a_core[:, -1] - b_core[:, 0])))
    # Report both normalized and approximate metres (at typical base_height_scale 1700).
    print(f"Seam border delta (max |A[:,-1] - B[:,0]|) = {border_delta:.6e}  "
          f"(~{border_delta * 1700.0:.3f} m at base_height_scale 1700)")
    assert border_delta < SEAM_BAR, (
        f"Seam NOT visually seamless: max border delta = {border_delta:.6e} (>= {SEAM_BAR:.0e}) "
        f"= ~{border_delta * 1700.0:.1f} m at game scale -- a visible seam (real breakage?)"
    )
