from __future__ import annotations
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
