#[compute]
#version 450

// WorldGen10 height field — the GPU side of the CPU/GPU parity contract.
// EDIT BOTH SIDES: every function here mirrors a Rust fn (hash.rs / grammar.rs /
// height.rs / parity.rs). The parity gate (gpu_parity_check.gd) enforces sync.
// Pure u32 + f32 math: no strings, no maps, no 64-bit ints (GLSL base profile).

layout(local_size_x = 64) in;

// ---- uniforms / push data ----
layout(set = 0, binding = 0, std430) restrict readonly buffer Coords { vec2 xz[]; } coords;
layout(set = 0, binding = 1, std430) restrict writeonly buffer OutH { float h[]; } out_h;
layout(set = 0, binding = 2, std430) restrict writeonly buffer OutSig { uint sig[]; } out_sig;
// palette table: palettes_flat[p*3 + k] = family index of slot k in palette p
layout(set = 0, binding = 3, std430) restrict readonly buffer Palettes { int fam[]; } palettes;
// compatibility: per palette an (offset,count) into compat_flat
layout(set = 0, binding = 4, std430) restrict readonly buffer CompatOff { ivec2 oc[]; } compat_off;
layout(set = 0, binding = 5, std430) restrict readonly buffer CompatFlat { int pal[]; } compat_flat;
// per-family kernel record: (dataOffset, rows, cols, _pad), then relief/footprint
layout(set = 0, binding = 6, std430) restrict readonly buffer KRec { ivec4 rec[]; } krec;
layout(set = 0, binding = 7, std430) restrict readonly buffer KParam { vec2 rf[]; } kparam; // (relief_m, footprint_m)
layout(set = 0, binding = 8, std430) restrict readonly buffer KData { float v[]; } kdata;

layout(push_constant, std430) uniform Params {
    float region_size_m;
    int province_size_regions;
    uint palette_primary_pct;
    uint palette_compatible_pct;
    float moderation_min;
    float moderation_strength;
    int seed;          // grammar seed (fits i32 for the test seeds)
    int num_palettes;
    int num_coords;
} P;

const uint FNV1A_INITIAL = 0x811c9dc5u;
const uint FNV1A_MULTIPLY = 0x01000193u;
const uint SALT_PROVINCE_PALETTE = 0x5052_4f56u;
const uint SALT_PALETTE_LOCAL    = 0x4c4f_4341u;
const uint SALT_PALETTE_COMPATIBLE = 0x434f_4d50u;
const uint SALT_PALETTE_RARE     = 0x5241_5245u;
const uint SALT_FAMILY_ROLL      = 0x46414d49u & 0xffffffffu;
const uint SALT_SIG              = 0x5349_4753u;
const int FAMILIES_PER_PALETTE = 3;

uint fold_u32(uint h, uint word) {
    for (int s = 0; s < 32; s += 8) {
        uint b = (word >> uint(s)) & 0xffu;
        h ^= b;
        h *= FNV1A_MULTIPLY;
    }
    return h;
}
// args are i64 on CPU; here they fit in i32 range for our coords/seed. Fold each
// as low u32 then high u32 (sign-extended) to match the CPU's i64 halves.
uint hash_ints1(uint salt, int a0) {
    uint h = FNV1A_INITIAL; h = fold_u32(h, salt);
    h = fold_u32(h, uint(a0)); h = fold_u32(h, uint(a0 >> 31)); // high half = sign extension
    h ^= h >> 16; h *= 0x7feb352du; h ^= h >> 15; return h;
}
uint hash_ints3(uint salt, int a0, int a1, int a2) {
    uint h = FNV1A_INITIAL; h = fold_u32(h, salt);
    h = fold_u32(h, uint(a0)); h = fold_u32(h, uint(a0 >> 31));
    h = fold_u32(h, uint(a1)); h = fold_u32(h, uint(a1 >> 31));
    h = fold_u32(h, uint(a2)); h = fold_u32(h, uint(a2 >> 31));
    h ^= h >> 16; h *= 0x7feb352du; h ^= h >> 15; return h;
}
uint hash_ints5(uint salt, int a0, int a1, int a2, int a3, int a4) {
    uint h = FNV1A_INITIAL; h = fold_u32(h, salt);
    h = fold_u32(h, uint(a0)); h = fold_u32(h, uint(a0 >> 31));
    h = fold_u32(h, uint(a1)); h = fold_u32(h, uint(a1 >> 31));
    h = fold_u32(h, uint(a2)); h = fold_u32(h, uint(a2 >> 31));
    h = fold_u32(h, uint(a3)); h = fold_u32(h, uint(a3 >> 31));
    h = fold_u32(h, uint(a4)); h = fold_u32(h, uint(a4 >> 31));
    h ^= h >> 16; h *= 0x7feb352du; h ^= h >> 15; return h;
}

int floor_div(float a, float b) { return int(floor(a / b)); }
int div_euclid(int a, int b) { int q = a / b; if ((a % b != 0) && ((a < 0) != (b < 0))) q -= 1; return q; }

int province_primary_palette(int prx, int prz) {
    uint h = hash_ints3(SALT_PROVINCE_PALETTE, prx, prz, P.seed);
    return int(h % uint(P.num_palettes));
}
int palette_for_region(int rx, int rz) {
    int prx = div_euclid(rx, P.province_size_regions);
    int prz = div_euclid(rz, P.province_size_regions);
    int primary = province_primary_palette(prx, prz);
    uint roll = hash_ints5(SALT_PALETTE_LOCAL, rx, rz, prx, prz, P.seed) % 100u;
    if (roll < P.palette_primary_pct) return primary;
    if (roll < P.palette_primary_pct + P.palette_compatible_pct) {
        ivec2 oc = compat_off.oc[primary];
        if (oc.y > 0) {
            uint pick = hash_ints3(SALT_PALETTE_COMPATIBLE, rx, rz, P.seed) % uint(oc.y);
            int idx = compat_flat.pal[oc.x + int(pick)];
            if (idx >= 0) return idx;
        }
        return primary;
    }
    return int(hash_ints3(SALT_PALETTE_RARE, rx, rz, P.seed) % uint(P.num_palettes));
}

// families + normalized bias for a region (mirrors families_for_region).
void families_for_region(int rx, int rz, out int fams[3], out float bias[3]) {
    int pal = palette_for_region(rx, rz);
    for (int i = 0; i < 3; i++) fams[i] = palettes.fam[pal * 3 + i];
    float base[3] = float[3](0.55, 0.30, 0.15);
    uint roll = hash_ints3(SALT_FAMILY_ROLL, rx, rz, P.seed) % 3u;
    for (int i = 0; i < 3; i++) bias[i] = base[(uint(i) + roll) % 3u];
}

float smoothstep_unit(float t) { float v = clamp(t, 0.0, 1.0); return v * v * (3.0 - 2.0 * v); }

// tiled bilinear sample of kernel `f` at world (x,z), scaled to relief (mirrors sample_kernel).
float sample_kernel(int f, float x, float z) {
    ivec4 r = krec.rec[f]; int off = r.x; int rows = r.y; int cols = r.z;
    float relief = kparam.rf[f].x; float footprint = kparam.rf[f].y;
    float u = (fract(x / footprint)) * float(cols);
    float v = (fract(z / footprint)) * float(rows);
    // GLSL fract on negatives already returns [0,1); matches rem_euclid(1.0).
    int u0 = int(floor(u)); int v0 = int(floor(v));
    float tu = u - float(u0); float tv = v - float(v0);
    int u1 = (u0 + 1) % cols; int v1 = (v0 + 1) % rows;
    u0 = ((u0 % cols) + cols) % cols; v0 = ((v0 % rows) + rows) % rows;
    float c00 = kdata.v[off + v0 * cols + u0];
    float c10 = kdata.v[off + v0 * cols + u1];
    float c01 = kdata.v[off + v1 * cols + u0];
    float c11 = kdata.v[off + v1 * cols + u1];
    float top = c00 + (c10 - c00) * tu;
    float bot = c01 + (c11 - c01) * tu;
    return (top + (bot - top) * tv) * relief;
}
float moderation(float slope) { return clamp(1.0 - P.moderation_strength * slope, P.moderation_min, 1.0); }
float local_slope(int f, float x, float z) {
    ivec4 r = krec.rec[f]; float footprint = kparam.rf[f].y; float relief = kparam.rf[f].x;
    float dx = footprint / float(r.z); float dz = footprint / float(r.y);
    float sx = (sample_kernel(f, x + dx, z) - sample_kernel(f, x - dx, z)) / (2.0 * relief);
    float sz = (sample_kernel(f, x, z + dz) - sample_kernel(f, x, z - dz)) / (2.0 * relief);
    return sqrt(sx * sx + sz * sz);
}

void main() {
    uint gid = gl_GlobalInvocationID.x;
    if (int(gid) >= P.num_coords) return;
    float x = coords.xz[gid].x; float z = coords.xz[gid].y;
    float s = P.region_size_m;
    float gx = x / s; float gz = z / s;
    int rx = int(floor(gx)); int rz = int(floor(gz));
    float tx = smoothstep_unit(gx - float(rx));
    float tz = smoothstep_unit(gz - float(rz));
    // 4 corners, accumulate weighted families into a small fixed buffer (<=12).
    int ids[12]; float wts[12]; int n = 0;
    ivec2 cr[4] = ivec2[4](ivec2(rx, rz), ivec2(rx + 1, rz), ivec2(rx, rz + 1), ivec2(rx + 1, rz + 1));
    float cw[4] = float[4]((1.0 - tx) * (1.0 - tz), tx * (1.0 - tz), (1.0 - tx) * tz, tx * tz);
    for (int c = 0; c < 4; c++) {
        if (cw[c] == 0.0) continue;
        int fams[3]; float bias[3]; families_for_region(cr[c].x, cr[c].y, fams, bias);
        for (int i = 0; i < 3; i++) {
            int fam = fams[i]; float add = cw[c] * bias[i];
            int found = -1; for (int j = 0; j < n; j++) if (ids[j] == fam) { found = j; break; }
            if (found >= 0) wts[found] += add; else { ids[n] = fam; wts[n] = add; n++; }
        }
    }
    float total = 0.0; for (int j = 0; j < n; j++) total += wts[j]; total = max(total, 1e-12);
    float height = 0.0;
    for (int j = 0; j < n; j++) {
        float w = wts[j] / total; int f = ids[j];
        float slope = local_slope(f, x, z);
        height += w * moderation(slope) * sample_kernel(f, x, z);
    }
    out_h.h[gid] = height;
    // family signature: sorted ascending ids folded via stable_hash_ints (mirrors parity.rs).
    // insertion sort the n (<=12) ids.
    for (int a = 1; a < n; a++) { int key = ids[a]; int b = a - 1; while (b >= 0 && ids[b] > key) { ids[b+1] = ids[b]; b--; } ids[b+1] = key; }
    uint h = FNV1A_INITIAL; h = fold_u32(h, SALT_SIG);
    for (int j = 0; j < n; j++) { h = fold_u32(h, uint(ids[j])); h = fold_u32(h, uint(ids[j] >> 31)); }
    h ^= h >> 16; h *= 0x7feb352du; h ^= h >> 15;
    out_sig.sig[gid] = h;
}
