"""WorldGen10 generator PROTOTYPE — pure numpy, offline, render-first (no engine/runtime).

The python MIRROR of the future Rust/GLSL `generate` (worldgen-core spec §3): domain warp ->
macro fBm landmass -> ridged ridgelines -> inverted-ridged valley carving, all driven by a
BiomeParams dict. This file exists to RENDER images the owner judges before any runtime rebuild
(the discipline that killed the spectral approach cheaply). NOTHING here runs at engine runtime."""

import numpy as np

N_OCTAVES = 6


def _hash2(ix, iz, seed):
    # integer lattice hash -> [0,1). Deterministic; wrapping int math.
    h = (ix.astype(np.int64) * 374761393 + iz.astype(np.int64) * 668265263 + int(seed) * 362437)
    h = (h ^ (h >> 13)) * 1274126177
    h = h & 0x7fffffff
    return h.astype(np.float64) / float(0x7fffffff)


def _fade(t):  # quintic smootherstep (C2) — smoother than cubic, fewer grid artifacts
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0)


def value_noise(wx, wz, seed=0):
    """Value noise on world-coord grids (cell size = 1 world unit at the given coords). [-1,1]."""
    x0 = np.floor(wx).astype(np.int64); z0 = np.floor(wz).astype(np.int64)
    tx = _fade(wx - x0); tz = _fade(wz - z0)
    c00 = _hash2(x0, z0, seed); c10 = _hash2(x0 + 1, z0, seed)
    c01 = _hash2(x0, z0 + 1, seed); c11 = _hash2(x0 + 1, z0 + 1, seed)
    top = c00 + (c10 - c00) * tx
    bot = c01 + (c11 - c01) * tx
    return (top + (bot - top) * tz) * 2.0 - 1.0


def fbm(wx, wz, base_freq, octaves, seed=0, gain=0.5, lacunarity=2.0):
    """Multi-octave value-noise fBm, normalized to ~[-1,1] (sum/Σgain^i). base_freq in 1/world-unit."""
    h = np.zeros_like(wx, dtype=np.float64)
    amp = 1.0; norm = 0.0; freq = base_freq
    for i in range(octaves):
        h += amp * value_noise(wx * freq, wz * freq, seed + i)
        norm += amp; amp *= gain; freq *= lacunarity
    return h / max(norm, 1e-9)


def ridged_fbm(wx, wz, base_freq, octaves, seed=0, gain=0.5, lacunarity=2.0):
    """Ridged fBm: each octave = 1 - |value_noise| (crest-biased -> linear ridges), summed+normalized -> [0,1]."""
    h = np.zeros_like(wx, dtype=np.float64)
    amp = 1.0; norm = 0.0; freq = base_freq
    for i in range(octaves):
        n = 1.0 - np.abs(value_noise(wx * freq, wz * freq, seed + i))
        h += amp * n
        norm += amp; amp *= gain; freq *= lacunarity
    return h / max(norm, 1e-9)


def domain_warp(wx, wz, warp_amount, warp_freq, seed=0):
    """Bend the coords by a low-freq fbm vector field. warp_amount in world units (0 = no warp).
    This is the anti-grid/anti-repeat spine: downstream sampling at warped coords never reads as a tile."""
    if warp_amount == 0.0:
        return wx, wz
    dx = fbm(wx, wz, warp_freq, 3, seed + 17)
    dz = fbm(wx, wz, warp_freq, 3, seed + 43)
    return wx + warp_amount * dx, wz + warp_amount * dz


def generate(wx, wz, params, seed=0):
    """The worldgen generator (spec §3): warp -> macro fBm landmass -> ridged ridgelines ->
    inverted-ridged valley carving -> ×relief. params is a BiomeParams dict. Pure function of
    world coords -> seamless/contiguous by construction; the python mirror of the runtime synth."""
    amps = params["octave_amps"]
    # 1. DOMAIN WARP — the anti-grid spine.
    w_x, w_z = domain_warp(wx, wz, params["warp_amount"], params["warp_freq"], seed)
    # 2. MACRO LANDMASS — multi-octave fBm at the biome's base freq + octave amplitudes.
    h = np.zeros_like(wx, dtype=np.float64)
    freq = params["base_freq"]
    for i in range(len(amps)):
        h += amps[i] * value_noise(w_x * freq, w_z * freq, seed + i)
        freq *= 2.0
    # 3. RIDGES — linear ridgelines, amplified in uplands (where h is already high).
    upland = np.clip((h - (-0.1)) / 0.6, 0.0, 1.0)    # smoothstep-ish 0..1 over the upper range
    upland = upland * upland * (3.0 - 2.0 * upland)
    h = h + params["ridge_strength"] * upland * ridged_fbm(w_x, w_z, params["ridge_freq"], 4, seed + 100)
    # 4. VALLEYS — inverted ridged noise carves connected drainage.
    h = h - params["valley_depth"] * ridged_fbm(w_x, w_z, params["valley_freq"], 4, seed + 200)
    # 5. relief
    return h * params["relief_m"]
