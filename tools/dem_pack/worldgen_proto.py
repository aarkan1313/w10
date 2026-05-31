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


def recursive_domain_warp(wx, wz, warp_amount, warp_freq, seed=0, steps=3, decay=0.55, freq_mul=1.9):
    """Recursive low-frequency coordinate bending for Slice 2A structure A/Bs.

    Plain single-pass warp bends roughness, but often still reads as same-noise texture. Re-warping the
    already-warped coordinates creates longer, more tangled corridors while staying local, deterministic,
    and parity-portable. This is still just an offline candidate until owner image review accepts it.
    """
    if warp_amount == 0.0 or steps <= 0:
        return wx, wz
    out_x = np.array(wx, dtype=np.float64, copy=True)
    out_z = np.array(wz, dtype=np.float64, copy=True)
    amount = float(warp_amount)
    freq = float(warp_freq)
    for i in range(int(steps)):
        dx = fbm(out_x, out_z, freq, 3, seed + 101 + i * 37)
        dz = fbm(out_x, out_z, freq, 3, seed + 151 + i * 37)
        out_x = out_x + amount * dx
        out_z = out_z + amount * dz
        amount *= float(decay)
        freq *= float(freq_mul)
    return out_x, out_z


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


def ridged_multifractal(wx, wz, base_freq, octaves, seed=0, gain=0.5, lacunarity=2.0,
                        offset=1.0, weight_gain=1.35):
    """Musgrave-style ridged multifractal candidate. Returns a normalized [0,1] ridge field.

    Unlike `ridged_fbm`, each octave is weighted by the previous octave's ridge signal, so crests tend to
    reinforce into connected ridge chains instead of independent roughness at every scale.
    """
    h = np.zeros_like(wx, dtype=np.float64)
    weight = np.ones_like(wx, dtype=np.float64)
    amp = 1.0
    norm = 0.0
    freq = base_freq
    for i in range(octaves):
        signal = float(offset) - np.abs(value_noise(wx * freq, wz * freq, seed + i))
        signal = np.clip(signal, 0.0, None)
        signal = signal * signal
        signal = signal * weight
        h += amp * signal
        norm += amp
        weight = np.clip(signal * float(weight_gain), 0.0, 1.0)
        amp *= gain
        freq *= lacunarity
    return np.clip(h / max(norm, 1e-9), 0.0, 1.0)


def cellular_edges(wx, wz, freq, seed=0, sharpness=2.0):
    """Cheap Worley/cellular edge network candidate. Returns [0,1], high near cell borders.

    This is an optional A/B for "large organizing lines" only. It is local and deterministic, but likely too
    cellular if overused; the render batch lets the owner reject it cheaply.
    """
    x = wx * freq
    z = wz * freq
    ix = np.floor(x).astype(np.int64)
    iz = np.floor(z).astype(np.int64)
    fx = x - ix
    fz = z - iz
    f1 = np.full_like(wx, np.inf, dtype=np.float64)
    f2 = np.full_like(wx, np.inf, dtype=np.float64)
    for dz in (-1, 0, 1):
        for dx in (-1, 0, 1):
            cx = ix + dx
            cz = iz + dz
            px = float(dx) + _hash2(cx, cz, seed + 11)
            pz = float(dz) + _hash2(cx, cz, seed + 29)
            d2 = (px - fx) * (px - fx) + (pz - fz) * (pz - fz)
            old_f1 = f1
            f1 = np.minimum(f1, d2)
            f2 = np.minimum(np.maximum(old_f1, d2), f2)
    gap = np.sqrt(f2) - np.sqrt(f1)
    return 1.0 - np.clip(gap * float(sharpness), 0.0, 1.0)


def range_spine_field(wx, wz, cell_size=65000.0, width=7000.0, seed=0, neighborhood=2):
    """World-anchored procedural range spines, high near long deterministic line segments.

    This is deliberately a different organizing primitive than fBm/ridged noise. It is still local-ish:
    each point only checks deterministic line segments in nearby coarse cells. Used for render-first
    experiments, not accepted runtime architecture yet.
    """
    gx = np.floor(wx / cell_size).astype(np.int64)
    gz = np.floor(wz / cell_size).astype(np.int64)
    out = np.zeros_like(wx, dtype=np.float64)
    for dz in range(-neighborhood, neighborhood + 1):
        for dx in range(-neighborhood, neighborhood + 1):
            cx = gx + dx
            cz = gz + dz
            jitter_x = (_hash2(cx, cz, seed + 1) - 0.5) * 0.65
            jitter_z = (_hash2(cx, cz, seed + 2) - 0.5) * 0.65
            center_x = (cx.astype(np.float64) + 0.5 + jitter_x) * cell_size
            center_z = (cz.astype(np.float64) + 0.5 + jitter_z) * cell_size
            angle = _hash2(cx, cz, seed + 3) * np.pi * 2.0
            length = cell_size * (1.15 + 0.75 * _hash2(cx, cz, seed + 4))
            vx = np.cos(angle) * length
            vz = np.sin(angle) * length
            x0 = center_x - vx * 0.5
            z0 = center_z - vz * 0.5
            denom = vx * vx + vz * vz + 1e-9
            t = np.clip(((wx - x0) * vx + (wz - z0) * vz) / denom, 0.0, 1.0)
            px = x0 + t * vx
            pz = z0 + t * vz
            d = np.sqrt((wx - px) * (wx - px) + (wz - pz) * (wz - pz))
            out = np.maximum(out, np.exp(-((d / float(width)) ** 2)))
    return np.clip(out, 0.0, 1.0)


def fault_block_field(wx, wz, cell_size=80000.0, width=9000.0, seed=0, neighborhood=2):
    """Broad signed fault bands. Produces blocky uplift/subsidence unlike isotropic fBm."""
    gx = np.floor(wx / cell_size).astype(np.int64)
    gz = np.floor(wz / cell_size).astype(np.int64)
    out = np.zeros_like(wx, dtype=np.float64)
    norm = 0.0
    for dz in range(-neighborhood, neighborhood + 1):
        for dx in range(-neighborhood, neighborhood + 1):
            cx = gx + dx
            cz = gz + dz
            center_x = (cx.astype(np.float64) + 0.5 + (_hash2(cx, cz, seed + 10) - 0.5) * 0.45) * cell_size
            center_z = (cz.astype(np.float64) + 0.5 + (_hash2(cx, cz, seed + 11) - 0.5) * 0.45) * cell_size
            angle = _hash2(cx, cz, seed + 12) * np.pi * 2.0
            nx = -np.sin(angle)
            nz = np.cos(angle)
            signed = (wx - center_x) * nx + (wz - center_z) * nz
            amp = (_hash2(cx, cz, seed + 13) * 2.0 - 1.0)
            influence = np.exp(-((signed / (cell_size * 0.55)) ** 2))
            out += amp * np.tanh(signed / float(width)) * influence
            norm += 1.0
    return np.clip(out / max(norm * 0.22, 1e-9), -1.0, 1.0)


def flow_accumulation_channels(z, power=0.45):
    """Offline flow accumulation on a rendered grid. Returns [0,1] branch/channel mask.

    This is intentionally not a cheap local per-page operator. It is here to answer a research question:
    if connected drainage is what the owner misses, does even a crude flow pass immediately read more real?
    If yes, true world-anchored coarse flow belongs in the roadmap instead of more local noise tuning.
    """
    h = np.asarray(z, dtype=np.float64)
    rows, cols = h.shape
    acc = np.ones_like(h, dtype=np.float64)
    order = np.argsort(-h.ravel())
    for idx in order:
        y = int(idx // cols)
        x = int(idx - y * cols)
        best_y = y
        best_x = x
        best_h = h[y, x]
        for oy in (-1, 0, 1):
            for ox in (-1, 0, 1):
                if ox == 0 and oy == 0:
                    continue
                ny = y + oy
                nx = x + ox
                if ny < 0 or ny >= rows or nx < 0 or nx >= cols:
                    continue
                if h[ny, nx] < best_h:
                    best_h = h[ny, nx]
                    best_y = ny
                    best_x = nx
        if best_y != y or best_x != x:
            acc[best_y, best_x] += acc[y, x]
    ch = np.log1p(acc)
    ch = ch / (float(ch.max()) + 1e-9)
    return np.power(ch, float(power))


STRUCTURE_VARIANTS = (
    "baseline",
    "recursive_warp",
    "multifractal_ridges",
    "ridge_valley_coupled",
    "cellular_valleys",
    "range_spines",
    "fault_blocks",
    "flow_carved_ranges",
)


def _macro_height(w_x, w_z, params, seed):
    h = np.zeros_like(w_x, dtype=np.float64)
    freq = params["base_freq"]
    for i, amp in enumerate(params["octave_amps"]):
        h += amp * value_noise(w_x * freq, w_z * freq, seed + i)
        freq *= 2.0
    return h


def _upland_mask(h):
    upland = np.clip((h - (-0.1)) / 0.6, 0.0, 1.0)
    return upland * upland * (3.0 - 2.0 * upland)


def generate_variant(wx, wz, params, seed=0, variant="baseline"):
    """Generate one Slice 2A structure candidate.

    `baseline` is exactly `generate()`. Other variants are render-first experiments, not accepted runtime
    architecture. They keep the same params so image differences come from the structure basis.
    """
    if variant == "baseline":
        return generate(wx, wz, params, seed)
    if variant not in STRUCTURE_VARIANTS:
        raise ValueError(f"unknown structure variant {variant!r}; expected one of {STRUCTURE_VARIANTS}")

    warp_amount = params["warp_amount"]
    warp_freq = params["warp_freq"]
    if variant == "recursive_warp":
        w_x, w_z = recursive_domain_warp(wx, wz, warp_amount * 1.8, warp_freq * 0.75, seed, steps=3)
    else:
        w_x, w_z = recursive_domain_warp(wx, wz, warp_amount * 1.25, warp_freq, seed, steps=2)

    h = _macro_height(w_x, w_z, params, seed)
    upland = _upland_mask(h)

    if variant == "recursive_warp":
        ridges = ridged_fbm(w_x, w_z, params["ridge_freq"] * 0.85, 4, seed + 100, gain=0.55)
        valleys = ridged_fbm(w_x, w_z, params["valley_freq"] * 0.80, 4, seed + 200, gain=0.55)
        h = h + params["ridge_strength"] * upland * ridges
        h = h - params["valley_depth"] * (0.35 + 0.65 * upland) * valleys
    elif variant == "multifractal_ridges":
        ridges = ridged_multifractal(w_x, w_z, params["ridge_freq"] * 0.75, 5, seed + 100, gain=0.58)
        valleys = ridged_multifractal(w_x, w_z, params["valley_freq"] * 0.65, 5, seed + 200, gain=0.54)
        h = h + params["ridge_strength"] * (0.25 + 0.75 * upland) * ridges
        h = h - params["valley_depth"] * (0.25 + 0.75 * upland) * np.power(valleys, 1.25)
    elif variant == "ridge_valley_coupled":
        ridges = ridged_multifractal(w_x, w_z, params["ridge_freq"] * 0.70, 5, seed + 100, gain=0.60)
        valley_net = ridged_multifractal(
            w_x + ridges * warp_amount * 0.18,
            w_z - ridges * warp_amount * 0.18,
            params["valley_freq"] * 0.58,
            5,
            seed + 200,
            gain=0.56,
        )
        h = h + params["ridge_strength"] * upland * ridges
        h = h - params["valley_depth"] * (0.20 + 0.95 * upland) * (0.65 * valley_net + 0.35 * valley_net * ridges)
    elif variant == "cellular_valleys":
        ridges = ridged_multifractal(w_x, w_z, params["ridge_freq"] * 0.70, 5, seed + 100, gain=0.58)
        valley_net = ridged_multifractal(w_x, w_z, params["valley_freq"] * 0.55, 5, seed + 200, gain=0.55)
        cells = cellular_edges(w_x, w_z, params["valley_freq"] * 0.28, seed + 300, sharpness=2.6)
        channels = np.maximum(valley_net, cells * 0.85)
        h = h + params["ridge_strength"] * upland * ridges
        h = h - params["valley_depth"] * (0.20 + 0.90 * upland) * channels
    elif variant == "range_spines":
        broad = range_spine_field(wx, wz, cell_size=76000.0, width=21000.0, seed=seed + 400)
        sharp = range_spine_field(wx, wz, cell_size=76000.0, width=6500.0, seed=seed + 400)
        ridges = ridged_multifractal(w_x, w_z, params["ridge_freq"] * 0.60, 4, seed + 100, gain=0.58)
        h = h * 0.45 + params["ridge_strength"] * (0.95 * broad + 0.55 * sharp + 0.25 * ridges)
        h = h - params["valley_depth"] * 0.35 * ridged_multifractal(w_x, w_z, params["valley_freq"] * 0.45, 4, seed + 200)
    elif variant == "fault_blocks":
        faults = fault_block_field(wx, wz, cell_size=90000.0, width=9000.0, seed=seed + 500)
        broad = range_spine_field(wx, wz, cell_size=85000.0, width=24000.0, seed=seed + 510)
        sharp = range_spine_field(wx, wz, cell_size=85000.0, width=7500.0, seed=seed + 510)
        h = h * 0.40 + 0.55 * faults + params["ridge_strength"] * (0.65 * broad + 0.35 * sharp)
        h = h - params["valley_depth"] * 0.45 * ridged_multifractal(w_x, w_z, params["valley_freq"] * 0.50, 4, seed + 200)
    elif variant == "flow_carved_ranges":
        broad = range_spine_field(wx, wz, cell_size=80000.0, width=23000.0, seed=seed + 600)
        sharp = range_spine_field(wx, wz, cell_size=80000.0, width=7500.0, seed=seed + 600)
        base = h * 0.38 + params["ridge_strength"] * (0.80 * broad + 0.45 * sharp)
        channels = flow_accumulation_channels(base)
        h = base - (0.25 + params["valley_depth"] * 0.95) * channels

    return h * params["relief_m"]
