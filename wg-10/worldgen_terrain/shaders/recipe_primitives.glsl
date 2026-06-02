// WorldGen10 GLSL mirror of the offline noise/warp primitives in
// tools/dem_pack/worldgen_proto.py (the AUTHORITATIVE f64 oracle).
//
// This file is the GPU side of Task 4a.3. It is NOT a standalone shader: it has no
// `#version`, no `layout`, no `main`. The Rust probe (primitive_probe.rs) CONCATENATES
// this file with primitive_probe.glsl (which carries #version + main) before compiling,
// because Godot GLSL has no #include. Keep this file as plain helper functions only.
//
// THE HARD PART (de-risked first, in isolation, against an f64 oracle): the lattice hash
// `_hash2` runs int64 WRAPPING arithmetic in numpy. GLSL base profile (#version 450) has
// NO 64-bit integers, so int64 is emulated as uvec2(hi, lo):
//   * u64_add / u64_mul  : low-64 WRAPPING add / multiply (16-bit-limb schoolbook so the
//                          low-word product carry into the high word is EXACT).
//   * i64_ashr13         : ARITHMETIC right shift by 13 of a SIGNED 64-bit value. The
//                          numpy `>>` is arithmetic; GLSL `int >> n` is arithmetic too, so
//                          the high word uses signed-int shift, the low word ORs in the
//                          bottom 13 bits of the high word (shifted up by 32-13 = 19).
//   * u64_xor            : componentwise.
//   * i64_from_int       : sign-extend an int into uvec2 (hi = v>>31 arithmetic).
// Only the FINAL `& 0x7fffffff` collapses to 32 bits, but the `>>13` happens on the FULL
// signed 64-bit intermediate (the ix*A+iz*B+seed*C sum can exceed 32 bits and its high
// bits feed the low bits after the shift), so the full low-64 wrapping math is required.
//
// GAUSSIAN (array_ops.rs::gaussian_filter_nearest) IS NOT HERE. It is the one operator the
// recipes need that is NOT a per-point function: it is a SEPARABLE whole-field blur (blur
// down axis 0, then across axis 1; scipy mode='nearest' = clamp-to-edge; truncate=4.0;
// radius = int(truncate*sigma + 0.5); kernel normalized to sum 1). On the GPU it is realized
// as multi-pass dispatches over the apron grid INSIDE the page pipeline (Task 4a.5's
// biome_page shader), with the 1-D kernel built CPU-side per distinct sigma and uploaded
// (the kernel depends only on sigma, not on data). The mountain recipe uses sigmas
// {1.15, 1.20, 1.80, 2.00, 5.00, 7.00, valley_width_px, floor_smooth_px}. The CPU kernel
// build MUST match array_ops.rs::gaussian_kernel1d exactly (same radius / truncate / phi /
// normalization) or the Tier-2 height parity drifts. Flow accumulation is likewise NOT here
// (it is the iterative relaxation in flow_accum_spike.glsl, reused by the page pipeline).

// ----------------------------------------------------------------------------
// int64 emulation as uvec2(x = high 32 bits, y = low 32 bits)
// ----------------------------------------------------------------------------

// Sign-extend a 32-bit int into a uvec2 64-bit value.
// hi = 0xFFFFFFFF if v<0 else 0 (arithmetic shift of the signed int by 31).
uvec2 i64_from_int(int v) {
    return uvec2(uint(v >> 31), uint(v));
}

// 64-bit wrapping add (low-64). Carry from the low word into the high word.
uvec2 u64_add(uvec2 a, uvec2 b) {
    uint lo = a.y + b.y;
    uint carry = (lo < a.y) ? 1u : 0u;   // unsigned overflow -> carry
    uint hi = a.x + b.x + carry;
    return uvec2(hi, lo);
}

// 64-bit wrapping multiply (low-64). 16-bit-limb schoolbook on the two LOW words so the
// carry of the low-word product into bit 32 is exact; the high*low cross terms only
// contribute their low 32 bits (anything above bit 63 is dropped = wrapping).
uvec2 u64_mul(uvec2 a, uvec2 b) {
    uint al = a.y & 0xFFFFu;
    uint ah = a.y >> 16;
    uint bl = b.y & 0xFFFFu;
    uint bh = b.y >> 16;

    uint p0 = al * bl;   // bits 0..31
    uint p1 = al * bh;   // bits 16..47
    uint p2 = ah * bl;   // bits 16..47
    uint p3 = ah * bh;   // bits 32..63

    // Accumulate the middle column (bits 16..) with the carry-out of p0's high half.
    uint cross = (p0 >> 16) + (p1 & 0xFFFFu) + (p2 & 0xFFFFu);
    uint lo = (cross << 16) | (p0 & 0xFFFFu);
    uint hi_from_low = (p1 >> 16) + (p2 >> 16) + (cross >> 16) + p3;

    // Cross terms a.y*b.x and a.x*b.y land at the 2^32 boundary -> only their low 32 bits
    // matter for a low-64 result. (uint mul wraps to 32 bits, which is exactly that.)
    uint hi = hi_from_low + a.y * b.x + a.x * b.y;
    return uvec2(hi, lo);
}

// Componentwise XOR.
uvec2 u64_xor(uvec2 a, uvec2 b) {
    return uvec2(a.x ^ b.x, a.y ^ b.y);
}

// ARITHMETIC right shift by 13 of a SIGNED 64-bit value-as-uvec2.
// High word: arithmetic shift of the signed high word (GLSL int >> is arithmetic).
// Low word : logical shift of the low word, OR the bottom 13 bits of the high word
//            shifted up into the top (32 - 13 = 19).
uvec2 i64_ashr13(uvec2 v) {
    uint hi = uint(int(v.x) >> 13);
    uint lo = (v.y >> 13) | (v.x << 19);
    return uvec2(hi, lo);
}

// ----------------------------------------------------------------------------
// hash2 : the proven-first primitive. Mirror of worldgen_proto._hash2.
//   h = ix*374761393 + iz*668265263 + seed*362437   (wrapping i64)
//   h = (h ^ (h >> 13)) * 1274126177                (wrapping i64, >> arithmetic)
//   masked = h & 0x7fffffff                         (low word, bits 0..30)
//   return masked / 0x7fffffff
// ----------------------------------------------------------------------------
float hash2(int ix, int iz, int seed) {
    uvec2 h = u64_add(
        u64_mul(i64_from_int(ix), i64_from_int(374761393)),
        u64_mul(i64_from_int(iz), i64_from_int(668265263))
    );
    h = u64_add(h, u64_mul(i64_from_int(seed), i64_from_int(362437)));

    h = u64_mul(u64_xor(h, i64_ashr13(h)), i64_from_int(1274126177));

    // 0x7fffffff fits in 31 bits -> entirely in the low word.
    uint masked = h.y & 0x7FFFFFFFu;
    return float(masked) / float(0x7FFFFFFFu);
}

// ----------------------------------------------------------------------------
// f32 primitives built on the proven hash. Mirror worldgen_proto EXACTLY:
//   octave seed is seed+i (NOT a salt); recursive_domain_warp updates amount/freq
//   AFTER the displacement; fbm uses gain/lacunarity params.
// All run in f32 on the GPU; the parity gate compares against the f64 oracle within
// an f32 epsilon (ABS_EPS = 2e-4 for primitives in [-1,1]/[0,1]).
// ----------------------------------------------------------------------------

// Quintic smootherstep (C2): t*t*t*(t*(t*6-15)+10).
float fade(float t) {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

// Value noise on raw world coords (callers pre-multiply by freq) -> [-1,1].
float value_noise(float wx, float wz, int seed) {
    float fx = floor(wx);
    float fz = floor(wz);
    int x0 = int(fx);
    int z0 = int(fz);
    float tx = fade(wx - fx);
    float tz = fade(wz - fz);
    float c00 = hash2(x0,     z0,     seed);
    float c10 = hash2(x0 + 1, z0,     seed);
    float c01 = hash2(x0,     z0 + 1, seed);
    float c11 = hash2(x0 + 1, z0 + 1, seed);
    float top = c00 + (c10 - c00) * tx;
    float bot = c01 + (c11 - c01) * tx;
    return (top + (bot - top) * tz) * 2.0 - 1.0;
}

// Multi-octave value-noise fBm, normalized to ~[-1,1]. Mirror of fbm(...).
// Per-octave seed = seed + i (NOT a salt).
float fbm(float wx, float wz, float base_freq, int octaves, int seed,
          float gain, float lacunarity) {
    float h = 0.0;
    float amp = 1.0;
    float norm = 0.0;
    float freq = base_freq;
    for (int i = 0; i < octaves; ++i) {
        h += amp * value_noise(wx * freq, wz * freq, seed + i);
        norm += amp;
        amp *= gain;
        freq *= lacunarity;
    }
    return h / max(norm, 1e-9);
}

// Musgrave-style ridged multifractal -> [0,1]. Mirror of ridged_multifractal(...)
// with offset=1.0, weight_gain=1.35 (the oracle defaults).
float ridged_multifractal(float wx, float wz, float base_freq, int octaves, int seed,
                          float gain, float lacunarity, float offset, float weight_gain) {
    float h = 0.0;
    float weight = 1.0;
    float amp = 1.0;
    float norm = 0.0;
    float freq = base_freq;
    for (int i = 0; i < octaves; ++i) {
        float signal = offset - abs(value_noise(wx * freq, wz * freq, seed + i));
        signal = max(signal, 0.0);
        signal = signal * signal;
        signal = signal * weight;
        h += amp * signal;
        norm += amp;
        weight = clamp(signal * weight_gain, 0.0, 1.0);
        amp *= gain;
        freq *= lacunarity;
    }
    return clamp(h / max(norm, 1e-9), 0.0, 1.0);
}

// Recursive low-frequency coordinate bending. Mirror of recursive_domain_warp(...)
// with steps=3, decay=0.55, freq_mul=1.9. amount/freq update AFTER the displacement.
// Inner fbm uses octaves=3, gain=0.5, lacunarity=2.0 (the oracle fbm defaults).
// Returns the warped coords (ox, oz).
vec2 recursive_domain_warp(float wx, float wz, float warp_amount, float warp_freq, int seed) {
    int steps = 3;
    float decay = 0.55;
    float freq_mul = 1.9;
    if (warp_amount == 0.0 || steps <= 0) {
        return vec2(wx, wz);
    }
    float ox = wx;
    float oz = wz;
    float amount = warp_amount;
    float freq = warp_freq;
    for (int i = 0; i < steps; ++i) {
        float dx = fbm(ox, oz, freq, 3, seed + 101 + i * 37, 0.5, 2.0);
        float dz = fbm(ox, oz, freq, 3, seed + 151 + i * 37, 0.5, 2.0);
        ox = ox + amount * dx;
        oz = oz + amount * dz;
        amount *= decay;
        freq *= freq_mul;
    }
    return vec2(ox, oz);
}
