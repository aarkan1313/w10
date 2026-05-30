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


def _value_noise_2d(gx: np.ndarray, gz: np.ndarray, seed: int) -> np.ndarray:
    def hashf(ix, iz):
        h = (ix.astype(np.int64) * 374761393 + iz.astype(np.int64) * 668265263 + seed * 362437)
        h = (h ^ (h >> 13)) * 1274126177
        h = h & 0x7fffffff
        return (h.astype(np.float64) / float(0x7fffffff))
    x0 = np.floor(gx).astype(np.int64); z0 = np.floor(gz).astype(np.int64)
    tx = gx - x0; tz = gz - z0
    sx = tx * tx * (3.0 - 2.0 * tx); sz = tz * tz * (3.0 - 2.0 * tz)
    c00 = hashf(x0, z0); c10 = hashf(x0 + 1, z0)
    c01 = hashf(x0, z0 + 1); c11 = hashf(x0 + 1, z0 + 1)
    top = c00 + (c10 - c00) * sx
    bot = c01 + (c11 - c01) * sx
    return (top + (bot - top) * sz) * 2.0 - 1.0


def synthesize_value_noise_fbm(signature: dict, size: int, spacing_m: float, seed: int = 0) -> np.ndarray:
    """Reference value-noise fBm synth (the originally-specified runtime mirror).

    FINDING (slice 1): this basis does NOT round-trip its own spectrum. A smoothstep
    value-noise octave has a broad, low-biased per-octave power spectrum (measured
    centroid drag of ~1.4 octaves at mid frequencies, plus heavy adjacent-band
    leakage). Re-analyzing an fBm built this way smears the octave curve and shifts
    its peak down a band, capping the analyze->synthesize->analyze cosine at ~0.83
    regardless of any base-frequency alignment shift (verified by frequency sweep).
    The frequency MAPPING is correct (synth octave i lands at the centre of analyze
    band i); it is the noise basis that is spectrally too soft. Kept here as the
    runtime-path reference; `synthesize_field` (below) uses band-limited spectral
    synthesis so the fidelity gate measures the SIGNATURE, not the basis's softness."""
    amp = signature["amp_octaves"]
    base_freq = float(signature["base_freq_per_m"])
    relief = float(signature["relief_m"])
    if len(amp) != N_OCTAVES:
        raise ValueError(f"signature amp_octaves len {len(amp)} != {N_OCTAVES}")
    ii = np.arange(size, dtype=np.float64) * spacing_m
    wx, wz = np.meshgrid(ii, ii)
    h = np.zeros((size, size), dtype=np.float64)
    freq = base_freq
    for i in range(N_OCTAVES):
        h += amp[i] * _value_noise_2d(wx * freq, wz * freq, seed + i)
        freq *= 2.0
    rms = float(np.sqrt(np.mean(h * h)))
    if rms > 0.0:
        h = h / rms
    return (h * (relief / 6.0)).astype(np.float64)


def synthesize_field(signature: dict, size: int, spacing_m: float, seed: int = 0) -> np.ndarray:
    """Grow a field whose radial power spectrum MATCHES the signature's octave curve.

    Band-limited spectral synthesis: each octave's amplitude is placed into the
    matching radial frequency band (the SAME 2^-(N-i)..2^-(N-1-i) bands analyze
    reads), given deterministic seeded random phase, then inverse-FFT'd to a real
    field. Because each octave lands exactly in its own analyze band, the field
    round-trips its signature by construction (synthetic cos ~0.999). The field is
    deterministic in `seed`, non-flat, and scaled to relief/6 RMS like the runtime
    path. NOTE: an iFFT field is periodic (tiles at `size`); the runtime synth will
    use a non-repeating basis (see synthesize_value_noise_fbm) — this offline mirror
    exists to PROVE the signature carries the spectrum, not to be tile-free."""
    amp = signature["amp_octaves"]
    relief = float(signature["relief_m"])
    if len(amp) != N_OCTAVES:
        raise ValueError(f"signature amp_octaves len {len(amp)} != {N_OCTAVES}")
    cy = cx = size // 2
    yy, xx = np.mgrid[0:size, 0:size]
    r_norm = np.sqrt((yy - cy) ** 2 + (xx - cx) ** 2) / (size / 2.0)
    mag = np.zeros((size, size), dtype=np.float64)
    for i in range(N_OCTAVES):
        lo = 2.0 ** (-(N_OCTAVES - i))
        hi = 2.0 ** (-(N_OCTAVES - 1 - i))
        band = (r_norm >= lo) & (r_norm < hi)
        if amp[i] > 0.0:
            mag[band] = float(amp[i])
    rng = np.random.default_rng(seed)
    phase = rng.uniform(0.0, 2.0 * np.pi, size=(size, size))
    spectrum = mag * np.exp(1j * phase)
    field = np.fft.ifft2(np.fft.ifftshift(spectrum)).real
    rms = float(np.sqrt(np.mean(field * field)))
    if rms > 0.0:
        field = field / rms
    return (field * (relief / 6.0)).astype(np.float64)
