"""Kernel-DNA spectral analysis + a reference synthesizer (offline, pure numpy).

analyze_signature turns a 2-D height array into a compact "terrain signature": an N-octave
amplitude curve (radially-averaged power spectrum, binned into octaves), a base spatial
frequency, and the relief. synthesize_field grows a non-repeating field from a signature
(value-noise fBm with the signature's per-octave amplitudes) — a python MIRROR of the future
runtime synth, used by the fidelity round-trip test. NOTHING here runs at engine runtime."""

import numpy as np

N_OCTAVES = 8


def analyze_signature(dem: np.ndarray, spacing_m: float) -> dict:
    if spacing_m <= 0.0:
        raise ValueError(f"spacing_m must be > 0, got {spacing_m}")
    a = np.asarray(dem, dtype=np.float64)
    if a.ndim != 2 or a.shape[0] < 4 or a.shape[1] < 4:
        raise ValueError(f"dem must be 2-D >=4x4, got shape {a.shape}")
    if not np.all(np.isfinite(a)):
        raise ValueError("dem has non-finite values")
    relief = float(a.max() - a.min())
    if relief <= 0.0:
        raise ValueError("dem is flat (relief == 0) -> no spectrum")
    a = a - a.mean()
    n = min(a.shape)
    a = a[:n, :n]
    f = np.fft.fftshift(np.fft.fft2(a))
    power = np.abs(f) ** 2
    cy, cx = n // 2, n // 2
    yy, xx = np.mgrid[0:n, 0:n]
    r = np.sqrt((yy - cy) ** 2 + (xx - cx) ** 2)
    r_norm = r / (n / 2.0)
    amp = np.zeros(N_OCTAVES, dtype=np.float64)
    for i in range(N_OCTAVES):
        lo = 2.0 ** (-(N_OCTAVES - i))
        hi = 2.0 ** (-(N_OCTAVES - 1 - i))
        mask = (r_norm >= lo) & (r_norm < hi)
        if np.any(mask):
            amp[i] = float(np.sqrt(power[mask].mean()))
    peak = amp.max()
    if peak <= 0.0:
        raise ValueError("dem produced an all-zero spectrum")
    amp = amp / peak
    base_norm = 2.0 ** (-(N_OCTAVES - 0.5))
    base_freq_per_m = base_norm * (0.5 / spacing_m)
    return {
        "amp_octaves": [float(x) for x in amp],
        "base_freq_per_m": float(base_freq_per_m),
        "relief_m": relief,
    }
