from __future__ import annotations
import numpy as np
from scipy.ndimage import gaussian_filter

def apron_blur_crop(field_with_apron: np.ndarray, apron_px: int, sigma: float) -> np.ndarray:
    """Gaussian-blur an apron-padded window, then crop to the authoritative core.

    The blur reads only samples inside the apron-padded extent; because adjacent windows
    share those underlying world samples, the cropped CORE is identical across neighbors as
    long as sigma's reach (~3*sigma) <= apron_px. Use mode='nearest' so the result depends only
    on in-extent samples (deterministic), and assert the apron is wide enough.
    """
    a = int(apron_px)
    reach = int(np.ceil(3.0 * float(sigma)))
    if reach > a:
        raise ValueError(f"apron_blur_crop: sigma reach {reach}px exceeds apron {a}px (would break seams)")
    blurred = gaussian_filter(np.asarray(field_with_apron, dtype=np.float64), sigma=float(sigma), mode="nearest")
    return blurred[a:-a, a:-a] if a > 0 else blurred
