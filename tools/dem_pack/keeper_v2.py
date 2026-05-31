from __future__ import annotations
from dataclasses import dataclass
import numpy as np
from scipy.ndimage import gaussian_filter

def apron_blur_crop(field_with_apron: np.ndarray, apron_px: int, sigma: float, truncate: float = 4.0) -> np.ndarray:
    """Gaussian-blur an apron-padded window, then crop to the authoritative core (all axes).

    The blur reads only samples inside the apron-padded extent, and `mode='nearest'` invents no
    out-of-window samples (unlike 'wrap'/'reflect'/'constant'), so the cropped CORE is bit-identical
    across adjacent windows that share those samples — as long as the kernel's half-width fits in the
    apron. scipy's kernel half-width is int(truncate*sigma + 0.5); we pass the SAME truncate to the
    filter and the guard so they cannot disagree.
    """
    a = int(apron_px)
    reach = int(np.floor(float(truncate) * float(sigma) + 0.5))  # matches gaussian_filter's kernel half-width
    if reach > a:
        raise ValueError(f"apron_blur_crop: kernel reach {reach}px (truncate {truncate}*sigma {sigma}) exceeds apron {a}px (would break seams)")
    blurred = gaussian_filter(np.asarray(field_with_apron, dtype=np.float64), sigma=float(sigma),
                              mode="nearest", truncate=float(truncate))
    return blurred[a:-a, a:-a] if a > 0 else blurred

def affine_remap(field: np.ndarray, center: float, scale: float) -> np.ndarray:
    """Data-independent remap (replaces znorm). Same (center,scale) every window => shared
    borders stay bit-identical. center/scale are tunable constants, NOT per-array statistics."""
    return (np.asarray(field, dtype=np.float64) - float(center)) * float(scale)

@dataclass(frozen=True)
class KeeperV2Params:
    softmax_temp: float = 0.36          # A's regime softmax temperature
    relief_amplitude: float = 1.0       # overall vertical gain (default ~A relative; tune by eye)
    incision_gain: float = 1.0
    range_texture_gain: float = 0.32
    badland_gain: float = 0.28
    fine_gain: float = 0.10
    blur_radius_m: float = 950.0        # final shaping blur
    weight_blur_m: float = 1700.0       # smooth_weights blur radius
    remap_center: float = 0.0           # affine remap (replaces znorm); tune to match A's tone
    remap_scale: float = 1.0
