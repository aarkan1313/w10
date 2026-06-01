"""Tests for the seam_safe shared utility module.

Run from repo root:
    python -m pytest tools/dem_pack/test_seam_safe.py -v
"""
import numpy as np
import pytest
import sys, os

# Allow import without installing (repo-root / tools/dem_pack on path already via conftest or direct run)
sys.path.insert(0, os.path.join(os.path.dirname(__file__)))
import seam_safe as ss


def test_affine_remap_is_data_independent():
    # same (center, scale) on two DIFFERENT arrays applies the SAME transform (no per-array stats)
    a = np.array([0.0, 1.0, 2.0])
    b = np.array([10.0, 20.0, 30.0])
    ra = ss.affine_remap(a, center=1.0, scale=2.0)
    rb = ss.affine_remap(b, center=1.0, scale=2.0)
    assert np.allclose(ra, (a - 1.0) * 2.0)
    assert np.allclose(rb, (b - 1.0) * 2.0)


def test_apron_blur_crop_returns_core_shape():
    field = np.random.default_rng(0).standard_normal((40, 40))
    apron = 8
    out = ss.apron_blur_crop(field, apron_px=apron, sigma=1.5)
    assert out.shape == (40 - 2 * apron, 40 - 2 * apron)


def test_apron_blur_crop_seam_exact_across_adjacent_windows():
    """Prove that apron_blur_crop produces bit-identical results at shared borders.

    Construction (unambiguous "reference blur" method):
    -------------------------------------------------------
    We blur one large `big` array once with the same gaussian_filter parameters
    (sigma, mode='nearest', truncate).  For a 2-D sub-window of `big` whose apron
    (on all four sides) is drawn from real `big` samples AND whose kernel support
    stays entirely inside `big` for the pixels we assert on, apron_blur_crop must
    return values identical to the corresponding region of the globally-blurred field
    — because both blurs see exactly the same input sample values.

    This directly implies seam-exactness: if window A's core-right column equals
    big_blurred at column c, and window B's core-left column also equals big_blurred
    at column c, then A-right == B-left bit-exactly.

    Boundary-handling note:
    The global blur uses `big`'s own edges for `mode='nearest'` clamping; the
    sub-window uses its own edges.  These agree only where the kernel support lies
    entirely within `big` (at least `reach` pixels from each big edge).  We
    therefore assert only on the "safe zone": pixels where both the big-row and
    big-col indices are in [reach, H-reach) × [reach, W-reach).

    Window geometry:
        window = big[y0 : y0+win_h, x0 : x0+win_w]
        win_h = core_h + 2*apron,  win_w = core_w + 2*apron
        core (after apron_blur_crop) maps to big[y0+apron : y0+apron+core_h,
                                                  x0+apron : x0+apron+core_w]
    We position the window so x0=apron, y0=apron, ensuring the apron pixels on
    every side are genuine `big` data (not invented padding).
    """
    from scipy.ndimage import gaussian_filter

    rng = np.random.default_rng(42)
    sigma = 1.5
    truncate = 4.0
    apron = 8  # must be >= int(truncate * sigma + 0.5) = int(4.0 * 1.5 + 0.5) = 6
    reach = int(np.floor(truncate * sigma + 0.5))  # = 6
    assert apron >= reach, "test setup: apron must cover kernel reach"

    H, W = 80, 80
    big = rng.standard_normal((H, W))

    # Reference: blur the whole field with the same parameters
    big_blurred = gaussian_filter(big, sigma=sigma, mode="nearest", truncate=truncate)

    # Window: 2-D sub-array inset by `apron` on every side of big.
    # After apron_blur_crop the core maps to big[apron:apron+core_h, apron:apron+core_w].
    core_h, core_w = 20, 20
    win_h = core_h + 2 * apron   # = 36
    win_w = core_w + 2 * apron   # = 36
    y0, x0 = apron, apron        # window starts at big row/col `apron`
    assert y0 + win_h <= H and x0 + win_w <= W, "window must fit in big"

    window = big[y0 : y0 + win_h, x0 : x0 + win_w]
    core = ss.apron_blur_crop(window, apron_px=apron, sigma=sigma, truncate=truncate)
    assert core.shape == (core_h, core_w)

    # Core pixel (r, c) maps to big pixel (y0+apron+r, x0+apron+c).
    # Assert only in the safe zone: big row/col both in [reach, dim-reach).
    safe_pixels = [
        (r, c)
        for r in range(core_h)
        for c in range(core_w)
        if reach <= (y0 + apron + r) < H - reach
        and reach <= (x0 + apron + c) < W - reach
    ]
    assert safe_pixels, "test setup: no safe pixels — widen big or reduce reach"

    # Collect into arrays for a single vectorised assertion
    rs = np.array([p[0] for p in safe_pixels])
    cs = np.array([p[1] for p in safe_pixels])
    big_rs = y0 + apron + rs
    big_cs = x0 + apron + cs

    np.testing.assert_array_equal(
        core[rs, cs],
        big_blurred[big_rs, big_cs],
        err_msg="apron_blur_crop core differs from reference blur in safe zone",
    )


def test_apron_blur_crop_rejects_kernel_larger_than_apron():
    field = np.zeros((20, 20))
    with pytest.raises(ValueError):
        ss.apron_blur_crop(field, apron_px=2, sigma=5.0)  # reach >> apron
