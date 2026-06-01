"""seam_safe — shared seam-exactness helpers for WorldGen10 per-window terrain generation.

Seam-exactness contract
-----------------------
WorldGen10 streams infinite terrain as independent windows.  Adjacent windows MUST
agree bit-exactly at their shared border (a "seam").  Two operations break seams
when naively applied per-window:

1. Data-dependent normalization (zscore, norm01): each window computes statistics
   from its own samples, so identical border pixels get different normalizations.
   Fix: ``affine_remap`` — a data-INDEPENDENT affine transform using caller-supplied
   constants (center, scale), not per-array statistics.

2. Boundary-padding blurs (gaussian_filter with 'reflect'/'wrap'/'constant'): the
   padding invents samples that differ between adjacent windows.
   Fix: ``apron_blur_crop`` — blur an *apron-padded* window using ``mode='nearest'``
   (which clamps to real samples shared by both windows), then crop to the core.
   The cropped core is bit-identical to the corresponding region of any adjacent
   window's crop, as long as the kernel's half-width fits within the apron.

These two helpers are ported from ``keeper_v2.py`` (which pioneered the pattern)
and provided here as the single canonical import for all 11 biome synths and the
composition layer.  keeper_v2 retains its own private copies; do NOT modify it.

Usage::

    import seam_safe as ss
    core = ss.apron_blur_crop(padded_window, apron_px=16, sigma=2.0)
    field = ss.affine_remap(field, center=0.0, scale=1.0 / 280.0)
"""
from __future__ import annotations

import math

import numpy as np
from scipy.ndimage import gaussian_filter


def affine_remap(field: np.ndarray, center: float, scale: float) -> np.ndarray:
    """Data-independent affine remap: ``(field - center) * scale``.

    Replaces per-window z-score / norm01 normalization.  Because ``center`` and
    ``scale`` are caller-supplied constants (NOT derived from ``field``'s own
    statistics), the same transform is applied to every window, so shared border
    pixels remain bit-identical across adjacent windows.

    Parameters
    ----------
    field:
        Input array (any shape). Converted to float64 internally.
    center:
        Subtracted from every element (e.g. a known domain midpoint or mean).
    scale:
        Multiplied after centering (e.g. ``1/std`` of the domain distribution).

    Returns
    -------
    np.ndarray
        float64 array of same shape as ``field``.
    """
    return (np.asarray(field, dtype=np.float64) - float(center)) * float(scale)


def apron_blur_crop(
    field_with_apron: np.ndarray,
    apron_px: int,
    sigma: float,
    truncate: float = 4.0,
) -> np.ndarray:
    """Gaussian-blur an apron-padded window then crop to the authoritative core.

    The blur reads only real samples inside the apron-padded extent; ``mode='nearest'``
    clamps to existing border samples rather than inventing synthetic ones (unlike
    ``'wrap'``, ``'reflect'``, or ``'constant'``).  Because adjacent windows share the
    apron samples, the cropped core is bit-identical across both windows — guaranteed
    as long as the kernel's half-width (``int(truncate * sigma + 0.5)``) does not
    exceed ``apron_px``.

    Parameters
    ----------
    field_with_apron:
        2-D array whose outer ``apron_px`` pixels on every side are the apron
        (real data from neighbouring territory, not synthetic padding). Converted
        to float64 internally.
    apron_px:
        Number of apron pixels on each side. Must be >= kernel half-width.
    sigma:
        Gaussian standard deviation in pixels.
    truncate:
        Passed to ``scipy.ndimage.gaussian_filter``; controls kernel half-width as
        ``int(truncate * sigma + 0.5)``.  Defaults to 4.0 (scipy's default).

    Returns
    -------
    np.ndarray
        float64 array cropped to the core: shape ``(H - 2*a, W - 2*a)`` where
        ``H, W`` are the input dimensions and ``a = apron_px``.

    Raises
    ------
    ValueError
        If the kernel's half-width exceeds ``apron_px``, which would cause the blur
        to read beyond the apron and break seam-exactness.
    """
    a = int(apron_px)
    reach = int(math.floor(float(truncate) * float(sigma) + 0.5))
    if reach > a:
        raise ValueError(
            f"apron_blur_crop: kernel reach {reach}px "
            f"(truncate={truncate} * sigma={sigma}) exceeds apron {a}px "
            "(would break seam-exactness)"
        )
    blurred = gaussian_filter(
        np.asarray(field_with_apron, dtype=np.float64),
        sigma=float(sigma),
        mode="nearest",
        truncate=float(truncate),
    )
    return blurred[a:-a, a:-a] if a > 0 else blurred
