import numpy as np
import geography_engine as geo

import mountain_synthesis as ms


def test_mountain_generate_is_deterministic_finite_and_nonflat():
    wx, wz = ms.grid(96, 90000.0)
    a = ms.generate(wx, wz, seed=12, style=ms.STYLES[0])
    b = ms.generate(wx, wz, seed=12, style=ms.STYLES[0])

    za = a["height"]
    assert za.shape == wx.shape
    assert np.all(np.isfinite(za))
    assert np.allclose(za, b["height"])
    assert float(np.ptp(za)) > 0.5


def test_mountain_styles_are_materially_different():
    wx, wz = ms.grid(80, 90000.0)
    heights = [ms.generate(wx, wz, seed=13, style=style)["height"] for style in ms.STYLES]
    deltas = []
    for i in range(len(heights) - 1):
        deltas.append(float(np.mean(np.abs(heights[i] - heights[i + 1]))))
    assert min(deltas) > 0.08


def test_mountain_channels_are_carved_lower_than_nonchannels():
    wx, wz = ms.grid(96, 90000.0)
    result = ms.generate(wx, wz, seed=14, style=ms.STYLES[3])
    z = result["height"]
    channels = np.maximum(result["primary_channels"], result["tributaries"])
    carved = z[channels > np.quantile(channels, 0.80)]
    background = z[channels <= np.quantile(channels, 0.35)]
    assert carved.size > 0 and background.size > 0
    assert float(np.mean(carved)) < float(np.mean(background))


def test_mountain_range_field_controls_high_ground():
    wx, wz = ms.grid(96, 90000.0)
    result = ms.generate(wx, wz, seed=15, style=ms.STYLES[2])
    corr = np.corrcoef(result["height"].ravel(), result["range_envelope"].ravel())[0, 1]
    assert corr > 0.12


def test_mountain_lowlands_are_lower_and_smoother():
    wx, wz = ms.grid(96, 90000.0)
    result = ms.generate(wx, wz, seed=16, style=ms.STYLES[0])
    z = result["height"]
    lowland = result["lowland"]
    high = result["range_envelope"]
    low_z = z[lowland >= np.quantile(lowland, 0.75)]
    range_z = z[high >= np.quantile(high, 0.75)]
    assert low_z.size > 0 and range_z.size > 0
    assert float(np.mean(low_z)) < float(np.mean(range_z))

    gy, gx = np.gradient(z)
    slope = np.sqrt(gx * gx + gy * gy)
    assert float(np.mean(slope[lowland >= np.quantile(lowland, 0.75)])) < float(np.mean(slope[high >= np.quantile(high, 0.75)]))


# ---------------------------------------------------------------------------
# Seam-exactness test (apron_px > 0 path)
# ---------------------------------------------------------------------------

def test_mountain_generate_seam_exact():
    """Adjacent apron-padded windows must agree at their shared border to float32 epsilon.

    Setup:
      - Core span S = 60_000 m over N = 64 core cells; cell_size = S / (N-1)
      - Window A core covers world x in [X0, X0+S], window B core covers [X0+S, X0+2*S]
      - A MASTER 1-D array (arange * cell + origin) spans both windows so that both A and B
        slice the SAME float64 values at their shared border — guaranteeing bit-identical
        border coordinates. (Independent linspace/arange calls on different spans produce
        different float64 representations of the same world position.)
      - After generate(apron_px=APRON), height is cropped to the core; assert the shared
        border column matches to within float32 epsilon (< 1e-7).

    Tolerance note (why < 1e-7, not == 0.0):
      The seam-safe path now carves valleys with REAL multiple-flow-direction (MFD)
      accumulation, which is a GLOBAL operation (a cell's discharge depends on all
      upstream cells).  On a finite apron the border is not bit-exact, but it CONVERGES
      to bit-exact as the apron grows — the probe (probe_flow_seam_real.py) measured the
      final-height border delta at 1.7e-10 (apron 80), 5.6e-17 (128), 0.0 (200).  1.7e-10
      is far below float32 epsilon (~1.19e-7), so on the GPU (float32) the seam is
      bit-identical at apron 80.  We assert < 1e-7 rather than inflate the apron to 200
      for a literal 0.0 (that doubles per-window compute for a sub-epsilon difference).
      The earlier local-DoG proxy WAS literally 0.0 but produced disconnected/soft
      valleys; real flow accumulation gives connected drainage, which the owner requires.
    """
    N = 64
    S = 60_000.0
    X0 = 10_000.0
    Z0 = 0.0
    APRON = ms.MOUNTAIN_APRON_PX  # canonical constant from the module
    SEED = 99
    STYLE = ms.STYLES[0]

    cell = S / (N - 1)  # world-space cell size in metres
    padded_n = N + 2 * APRON  # grid size including apron on both sides

    # Master 1-D arrays: A occupies master[0:padded_n], B occupies master[N-1:N-1+padded_n].
    # The shared border is master column (APRON + N - 1), which equals A[:, APRON+N-1] and
    # B[:, APRON] — both are the SAME float64 value since they reference the same element.
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
    res_a = ms.generate(a_wx, a_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)
    res_b = ms.generate(b_wx, b_wz, seed=SEED, style=STYLE, apron_px=APRON, feature_span_m=S)

    a_core = res_a["height"]  # already cropped to (N, N)
    b_core = res_b["height"]  # already cropped to (N, N)

    assert a_core.shape == (N, N), f"A core shape {a_core.shape} != ({N}, {N})"
    assert b_core.shape == (N, N), f"B core shape {b_core.shape} != ({N}, {N})"

    # The shared border: A's rightmost column vs B's leftmost column.
    # Asserts < 1e-7 (float32 epsilon): global flow accumulation converges to bit-exact
    # as the apron grows; at apron 80 the residual (~1.7e-10) is sub-float32-epsilon, so
    # the GPU (float32) sees a bit-identical seam. See docstring for the convergence probe.
    border_delta = float(np.max(np.abs(a_core[:, -1] - b_core[:, 0])))
    print(f"Seam border delta (max |A[:,-1] - B[:,0]|) = {border_delta:.6e}")
    assert border_delta < 1e-7, (
        f"Seam NOT exact to float32 epsilon: max border delta = {border_delta:.6e} (>= 1e-7)"
    )
