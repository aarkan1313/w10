// WorldGen10 Slice-4a: GLSL apron PAGE pipeline for the MOUNTAIN seam-safe recipe.
//
// This is the GPU mirror of recipes.rs::mountain::generate_seamsafe (the f64 parity
// ORACLE). It is NOT a standalone shader on its own: the Rust orchestrator
// (biome_page_compute.rs) CONCATENATES recipe_primitives.glsl (the proven f32
// noise/warp leaves + the i64-emulated hash) BEFORE this file, because Godot GLSL has
// no #include. Keep the #version/layout/main HERE; the primitives file has only helpers.
//
// SHAPE: one compiled compute shader, dispatched ONCE PER PASS with a different `pass`
// push-constant int. Every pass reads its input field buffer(s) + writes one output
// buffer over the apron grid (rows*cols). The whole-field operators (gaussian blur, flow
// accumulation) are themselves realized as their own passes:
//   * GAUSS is separable: COPY a source field into gauss_in, then GAUSS_AXIS0
//     (blur down rows -> gauss_mid) then GAUSS_AXIS1 (blur across cols -> gauss_out),
//     using a CPU-built, uploaded 1-D kernel (clamp-to-edge 'nearest' on BOTH axes).
//     Matches array_ops.rs::gaussian_filter_nearest EXACTLY (axis0 then axis1).
//   * FLOW is the PULL relaxation from flow_accum_spike.glsl: one dispatch = one
//     relaxation step, ping-ponging acc_a/acc_b for STABLE_ITERS steps, on flow_pre.
//
// The pointwise field math + EVERY constant/seed-offset/weight is transcribed verbatim
// from recipes.rs::mountain. EDIT-BOTH-SIDES: changes here must keep parity with that.
//
// NOTE ON THE WARP: recipe_primitives.glsl's recursive_domain_warp HARDCODES
// steps=3/decay=0.55/freq_mul=1.9 (the probe convention). The mountain recipe calls the
// warp with DIFFERENT decay/freq_mul (0.58/1.75 and 0.54/1.85), so we define our own
// parameterized recipe_recursive_domain_warp() below that mirrors
// recipe_noise.rs::recursive_domain_warp (decay/freq_mul as params) and reuses the proven
// leaf fbm(). We do NOT touch the proven primitives file.

#version 450

layout(local_size_x = 16, local_size_y = 16) in;

// ---------------------------------------------------------------------------
// pass selector codes (MUST match biome_page_compute.rs)
// ---------------------------------------------------------------------------
const int PASS_MESHGRID    = 0;  // build wx/wz from grid params
const int PASS_POINTWISE   = 1;  // warp -> regional / ranges / ridge_detail / near_detail
const int PASS_COPY        = 2;  // copy a selected field buffer -> gauss_in
const int PASS_GAUSS_AXIS0 = 3;  // gauss_in  -> gauss_mid (blur down rows, nearest)
const int PASS_GAUSS_AXIS1 = 4;  // gauss_mid -> gauss_out (blur across cols, nearest)
const int PASS_RANGE_ENV   = 5;  // range_envelope = smoothstep(0.24,0.58, gauss_out)
const int PASS_LOWLAND     = 6;  // lowland = combine(gauss_out=broad_range, regional)
const int PASS_MASSIF_INNER= 7;  // massif = clip(affine(0.58*reg+0.86*env+0.28*gauss_out))
const int PASS_BASE        = 8;  // base   = affine(uplift*(1.5*massif+0.18*ranges-0.46*lowland))
const int PASS_FLOW_PRE_BASE = 9;   // flow_pre <- base (for primary)
const int PASS_FLOW_PRE_ROUGH = 10; // flow_pre <- rough_surface (base + 0.18*affine(ranges))
const int PASS_FLOW_RELAX  = 11; // one PULL relaxation step (acc_prev -> acc_next) on flow_pre
const int PASS_DISCHARGE   = 12; // gauss_in <- clip(log1p(acc)/log1p(n)) ; (then GAUSS spread)
const int PASS_PRIMARY_MASK= 13; // primary_mask  = smoothstep(PLO,PHI, gauss_out)
const int PASS_TRIB_MASK   = 14; // tributary_mask= smoothstep(TLO,THI, gauss_out)
const int PASS_MASKS       = 15; // high_mask / valley_mask
const int PASS_ASSEMBLE    = 16; // height = base + ridge/detail - carve/branch
const int PASS_FLOOR_MASK  = 17; // floor_mask = clip(smoothstep(0.48,0.86,gauss_out)+0.24*lowland)
const int PASS_FLOOR_BLEND = 18; // height = blend(height, gauss_out=floor) ; -0.18*floor_mask
const int PASS_FINAL       = 19; // height = affine(0.74*height + 0.26*gauss_out)
const int PASS_CROP        = 20; // core_out[core] <- height[apron-offset apron grid]
const int PASS_FLOW_PRE_PREBLUR_IN = 21; // gauss_in <- flow_pre (to pre-blur sigma=1.15)
const int PASS_FLOW_PRE_FROM_GAUSS = 22; // flow_pre <- gauss_out (after pre-blur)
const int PASS_MASSIF_WRITEBACK    = 23; // massif <- gauss_out (the gaussian(massif,2.0))
const int PASS_ACC_INIT            = 24; // acc_a = acc_b = 1.0 (matches CPU acc init)

// copy_sel codes for PASS_COPY (which field buffer to copy into gauss_in)
const int CP_RANGES   = 0;
const int CP_MASSIF   = 1;
const int CP_VALLEY   = 2;
const int CP_HEIGHT   = 3;

// ---------------------------------------------------------------------------
// storage buffers (all std430 float[], length rows*cols unless noted)
// ---------------------------------------------------------------------------
layout(set = 0, binding = 0,  std430) restrict buffer Wx            { float v[]; } wx;
layout(set = 0, binding = 1,  std430) restrict buffer Wz            { float v[]; } wz;
layout(set = 0, binding = 2,  std430) restrict buffer Regional      { float v[]; } regional;
layout(set = 0, binding = 3,  std430) restrict buffer Ranges        { float v[]; } ranges;
layout(set = 0, binding = 4,  std430) restrict buffer RidgeDetail   { float v[]; } ridge_detail;
layout(set = 0, binding = 5,  std430) restrict buffer NearDetail    { float v[]; } near_detail;
layout(set = 0, binding = 6,  std430) restrict buffer RangeEnvelope { float v[]; } range_envelope;
layout(set = 0, binding = 7,  std430) restrict buffer Lowland       { float v[]; } lowland;
layout(set = 0, binding = 8,  std430) restrict buffer Massif        { float v[]; } massif;
layout(set = 0, binding = 9,  std430) restrict buffer Base          { float v[]; } base;
layout(set = 0, binding = 10, std430) restrict buffer PrimaryMask   { float v[]; } primary_mask;
layout(set = 0, binding = 11, std430) restrict buffer TribMask      { float v[]; } tributary_mask;
layout(set = 0, binding = 12, std430) restrict buffer HighMask      { float v[]; } high_mask;
layout(set = 0, binding = 13, std430) restrict buffer ValleyMask    { float v[]; } valley_mask;
layout(set = 0, binding = 14, std430) restrict buffer Height        { float v[]; } height;
layout(set = 0, binding = 15, std430) restrict buffer FloorMask     { float v[]; } floor_mask;
layout(set = 0, binding = 16, std430) restrict buffer GaussIn       { float v[]; } gauss_in;
layout(set = 0, binding = 17, std430) restrict buffer GaussMid      { float v[]; } gauss_mid;
layout(set = 0, binding = 18, std430) restrict buffer GaussOut      { float v[]; } gauss_out;
layout(set = 0, binding = 19, std430) restrict readonly buffer Kernel { float v[]; } kern;
layout(set = 0, binding = 20, std430) restrict buffer FlowPre       { float v[]; } flow_pre;
layout(set = 0, binding = 21, std430) restrict buffer AccA          { float v[]; } acc_a;
layout(set = 0, binding = 22, std430) restrict buffer AccB          { float v[]; } acc_b;
layout(set = 0, binding = 23, std430) restrict buffer CoreOut       { float v[]; } core_out;

// ---------------------------------------------------------------------------
// push constant. 16-byte-aligned: 12 ints (48B) then 12 floats (48B) = 96B.
// MUST match biome_page_compute.rs::build_push byte layout exactly.
// ---------------------------------------------------------------------------
layout(push_constant, std430) uniform Params {
    int pass;        // which pass (see PASS_*)
    int rows;        // PADDED rows
    int cols;        // PADDED cols
    int apron_px;    // apron cells cropped each side
    int seed;        // recipe seed (integer; offsets added in-shader)
    int kradius;     // gaussian kernel half-width lw (kernel length = 2*lw+1)
    int copy_sel;    // PASS_COPY: which field to copy into gauss_in
    int flow_dir;    // PASS_FLOW_RELAX: 0 = acc_a->acc_b, 1 = acc_b->acc_a
    int koffset;     // base index of the active kernel in the packed kernel buffer
    int ipad0;
    int ipad1;
    int ipad2;
    float spacing;   // grid spacing (metres/px)
    float ox;        // grid origin x
    float oz;        // grid origin z
    float feature_span_m;  // CORE feature span (NOT padded extent)
    float flow_power;      // MFD exponent for the active flow pass
    float pad0;
    float pad1;
    float pad2;
    float pad3;
    float pad4;
    float pad5;
    float pad6;
} P;

// ---------------------------------------------------------------------------
// ===== MOUNTAIN constants (verbatim from recipes.rs::mountain) =====
// ---------------------------------------------------------------------------
// affine-remap constants
const float REGIONAL_CENTER     = -0.50;
const float REGIONAL_SCALE      =  1.00;
const float RIDGES_CENTER       =  0.10;
const float RIDGES_SCALE        =  1.15;
const float MASSIF_CENTER       =  0.12;
const float MASSIF_SCALE        =  0.72;
const float BASE_CENTER         =  0.83;
const float BASE_SCALE          =  2.28;
const float RANGES_ZSCORE_CENTER=  0.42;
const float RANGES_ZSCORE_SCALE =  7.00;
const float RIDGE_DETAIL_CENTER =  0.31;
const float RIDGE_DETAIL_SCALE  =  4.85;
const float NEAR_DETAIL_CENTER  =  0.00;
const float NEAR_DETAIL_SCALE   =  3.60;
const float FINAL_CENTER        =  0.00;
const float FINAL_SCALE         =  0.80;

// LOOK levers (seam-safe path only)
const float PRIMARY_THRESH_LO    = 0.26;
const float PRIMARY_THRESH_HI    = 0.40;
const float TRIBUTARY_THRESH_LO  = 0.24;
const float TRIBUTARY_THRESH_HI  = 0.40;
const float SEAMSAFE_CARVE_GAIN  = 2.00;
const float SEAMSAFE_BRANCH_GAIN = 1.70;
const float SEAMSAFE_RIDGE_GAIN  = 1.12;
const float SEAMSAFE_DETAIL_GAIN = 1.05;

// ALPINE_BRANCHING style (STYLES[0]) fields the seam-safe pipeline reads
const float STYLE_ANGLE_RAD      = 0.42;
const float STYLE_UPLIFT_GAIN    = 1.12;
const float STYLE_RIDGE_GAIN     = 1.18;
const float STYLE_CARVE_GAIN     = 1.08;
const float STYLE_BRANCH_GAIN    = 1.18;
const float STYLE_VALLEY_WIDTH_PX= 2.4;
const float STYLE_FLOOR_SMOOTH_PX= 4.0;
const float STYLE_DETAIL_GAIN    = 0.72;
const float STYLE_ANISOTROPY     = 0.72;

// ---------------------------------------------------------------------------
// helpers (mirror recipes.rs::helpers)
// ---------------------------------------------------------------------------
int cell_idx(int r, int c) { return r * P.cols + c; }

// clamp index to [0, n-1] ('nearest' / edge-replicate boundary)
int clamp_idx(int i, int n) {
    if (i < 0) return 0;
    if (i >= n) return n - 1;
    return i;
}

// affine_remap: (v - center) * scale
float affine_remap(float v, float center, float scale) { return (v - center) * scale; }

// smoothstep with the +1e-9 denominator guard (mirror of mountain_synthesis.smoothstep).
// NOTE: do NOT use GLSL's built-in smoothstep (no guard / different clamp form).
float ss(float edge0, float edge1, float x) {
    float t = clamp((x - edge0) / (edge1 - edge0 + 1e-9), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

float clip01(float v) { return clamp(v, 0.0, 1.0); }

// rotate (wx,wz) about world origin (cx=cz=0) by angle (seam-safe).
vec2 rotated0(float x, float z, float angle) {
    float c = cos(angle);
    float s = sin(angle);
    return vec2(c * x + s * z, -s * x + c * z);
}

// Parameterized recursive domain warp. Mirror of recipe_noise.rs::recursive_domain_warp:
//   per step i: dx = fbm(out, freq, 3, seed+101+i*37, 0.5, 2.0)
//               dz = fbm(out, freq, 3, seed+151+i*37, 0.5, 2.0)
//               out += amount*dx/dz ; THEN amount*=decay ; freq*=freq_mul (update AFTER).
// Uses the PROVEN leaf fbm() from recipe_primitives.glsl. steps fixed at 3 (recipe uses 3).
vec2 recipe_recursive_domain_warp(float wxc, float wzc, float warp_amount, float warp_freq,
                                  int seed, int steps, float decay, float freq_mul) {
    if (warp_amount == 0.0 || steps <= 0) return vec2(wxc, wzc);
    float ox = wxc;
    float oz = wzc;
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

// ridged_multifractal with the recipe defaults (offset=1.0, weight_gain=1.35, lac=2.0).
float rmf(float x, float z, float base_freq, int octaves, int seed, float gain) {
    return ridged_multifractal(x, z, base_freq, octaves, seed, gain, 2.0, 1.0, 1.35);
}

// fbm with the recipe lacunarity=2.0 made explicit.
float fbm5(float x, float z, float base_freq, int octaves, int seed, float gain) {
    return fbm(x, z, base_freq, octaves, seed, gain, 2.0);
}

// Mirror of mountain::oriented_ridges_point (seam-safe, rotation centre = world origin).
float oriented_ridges_point(float wxc, float wzc, float span, int seed) {
    vec2 r = rotated0(wxc, wzc, STYLE_ANGLE_RAD);
    // recursive_domain_warp(rx, rz*aniso, span*0.065, 1/(span*0.58), seed+100, 3, 0.54, 1.85)
    vec2 w = recipe_recursive_domain_warp(
        r.x, r.y * STYLE_ANISOTROPY,
        span * 0.065, 1.0 / (span * 0.58),
        seed + 100, 3, 0.54, 1.85);
    float w_rx = w.x;
    float w_rz = w.y;
    float lng = rmf(w_rx, w_rz, 1.0 / (span * 0.34), 5, seed + 120, 0.58);
    float mid = rmf(w_rx, w_rz, 1.0 / (span * 0.15), 4, seed + 130, 0.54);
    // organic uses w_x := w_rx + 0.28*w_rz, w_z := w_rz - 0.18*w_rx (Python walrus).
    float w_x = w_rx + 0.28 * w_rz;
    float w_z = w_rz - 0.18 * w_rx;
    float organic = rmf(w_x, w_z, 1.0 / (span * 0.22), 5, seed + 140, 0.56);
    float crossv  = rmf(w_x, w_z, 1.0 / (span * 0.095), 3, seed + 150, 0.50);
    float raw = 0.42 * lng + 0.24 * mid + 0.48 * organic + 0.18 * crossv;
    return clip01(affine_remap(raw, RIDGES_CENTER, RIDGES_SCALE));
}

// ---------------------------------------------------------------------------
// main: one cell, one pass.
// ---------------------------------------------------------------------------
void main() {
    int rows = P.rows;
    int cols = P.cols;
    int cx = int(gl_GlobalInvocationID.x);   // column
    int cy = int(gl_GlobalInvocationID.y);   // row
    int pass = P.pass;

    // PASS_CROP iterates over CORE cells (core_cols x core_rows); guard separately.
    if (pass == PASS_CROP) {
        int a = P.apron_px;
        int core_cols = cols - 2 * a;
        int core_rows = rows - 2 * a;
        if (cx >= core_cols || cy >= core_rows) return;
        // height[(cy+a)*cols + (cx+a)] -> core_out[cy*core_cols + cx]
        int src = (cy + a) * cols + (cx + a);
        core_out.v[cy * core_cols + cx] = height.v[src];
        return;
    }

    if (cx >= cols || cy >= rows) return;
    int i = cell_idx(cy, cx);

    if (pass == PASS_MESHGRID) {
        // xs[c] = (c - apron)*spacing + ox ; zs[r] = (r - apron)*spacing + oz
        float a = float(P.apron_px);
        wx.v[i] = (float(cx) - a) * P.spacing + P.ox;
        wz.v[i] = (float(cy) - a) * P.spacing + P.oz;
        return;
    }

    if (pass == PASS_POINTWISE) {
        float span = max(P.feature_span_m, 1.0);
        // recursive_domain_warp(wx,wz, span*0.050, 1/(span*0.72), seed+10, 3, 0.58, 1.75)
        vec2 w = recipe_recursive_domain_warp(
            wx.v[i], wz.v[i],
            span * 0.050, 1.0 / (span * 0.72),
            P.seed + 10, 3, 0.58, 1.75);
        float w_x = w.x;
        float w_z = w.y;
        // regional = clip(affine(fbm(w_x,w_z, 1/(span*0.88),5,seed+20,0.56)))
        float reg = fbm5(w_x, w_z, 1.0 / (span * 0.88), 5, P.seed + 20, 0.56);
        regional.v[i] = clip01(affine_remap(reg, REGIONAL_CENTER, REGIONAL_SCALE));
        // ranges = oriented_ridges_point(w_x, w_z, span, seed)
        ranges.v[i] = oriented_ridges_point(w_x, w_z, span, P.seed);
        // ridge_detail = affine(ridged_multifractal(w_x,w_z,1/(span*0.045),5,seed+40,0.52))
        float rd = rmf(w_x, w_z, 1.0 / (span * 0.045), 5, P.seed + 40, 0.52);
        ridge_detail.v[i] = affine_remap(rd, RIDGE_DETAIL_CENTER, RIDGE_DETAIL_SCALE);
        // near_detail = affine(fbm(w_x,w_z,1/(span*0.020),4,seed+50,0.48))
        float nd = fbm5(w_x, w_z, 1.0 / (span * 0.020), 4, P.seed + 50, 0.48);
        near_detail.v[i] = affine_remap(nd, NEAR_DETAIL_CENTER, NEAR_DETAIL_SCALE);
        return;
    }

    if (pass == PASS_COPY) {
        float v = 0.0;
        if (P.copy_sel == CP_RANGES) v = ranges.v[i];
        else if (P.copy_sel == CP_MASSIF) v = massif.v[i];
        else if (P.copy_sel == CP_VALLEY) v = valley_mask.v[i];
        else if (P.copy_sel == CP_HEIGHT) v = height.v[i];
        gauss_in.v[i] = v;
        return;
    }

    if (pass == PASS_GAUSS_AXIS0) {
        // blur DOWN rows (vary the row index cy), clamp-to-edge. center tap first, then
        // symmetric pairs outward (mirrors array_ops correlate1d accumulation order).
        // kernel taps live at kern.v[koffset .. koffset+2*lw]; center = koffset+lw.
        int lw = P.kradius;
        int ko = P.koffset;
        float s = gauss_in.v[i] * kern.v[ko + lw];
        for (int j = 1; j <= lw; ++j) {
            int pr = clamp_idx(cy + j, rows);
            int mr = clamp_idx(cy - j, rows);
            float plus  = gauss_in.v[pr * cols + cx];
            float minus = gauss_in.v[mr * cols + cx];
            s += (plus + minus) * kern.v[ko + lw + j];
        }
        gauss_mid.v[i] = s;
        return;
    }

    if (pass == PASS_GAUSS_AXIS1) {
        // blur ACROSS cols (vary the col index cx), clamp-to-edge.
        int lw = P.kradius;
        int ko = P.koffset;
        int baserow = cy * cols;
        float s = gauss_mid.v[i] * kern.v[ko + lw];
        for (int j = 1; j <= lw; ++j) {
            int pc = clamp_idx(cx + j, cols);
            int mc = clamp_idx(cx - j, cols);
            float plus  = gauss_mid.v[baserow + pc];
            float minus = gauss_mid.v[baserow + mc];
            s += (plus + minus) * kern.v[ko + lw + j];
        }
        gauss_out.v[i] = s;
        return;
    }

    if (pass == PASS_RANGE_ENV) {
        // range_envelope = smoothstep(0.24, 0.58, gaussian(ranges, sigma=5.0))  [gauss_out]
        range_envelope.v[i] = ss(0.24, 0.58, gauss_out.v[i]);
        return;
    }

    if (pass == PASS_LOWLAND) {
        // gauss_out = broad_range = gaussian(ranges, sigma=7.0).
        // low = smoothstep(0.48,0.84, 1-broad_range)
        // regional_low = smoothstep(0.44,0.78, 1-regional)
        // out = clip(low*(0.35+0.65*regional_low), 0,1)
        float low = ss(0.48, 0.84, 1.0 - gauss_out.v[i]);
        float regional_low = ss(0.44, 0.78, 1.0 - regional.v[i]);
        lowland.v[i] = clip01(low * (0.35 + 0.65 * regional_low));
        return;
    }

    if (pass == PASS_MASSIF_INNER) {
        // gauss_out = gaussian(ranges, sigma=1.8).
        // massif_inner = 0.58*regional + 0.86*range_envelope + 0.28*gauss_out
        // massif = clip(affine(massif_inner, MASSIF_CENTER, MASSIF_SCALE))
        float massif_inner = 0.58 * regional.v[i] + 0.86 * range_envelope.v[i] + 0.28 * gauss_out.v[i];
        massif.v[i] = clip01(affine_remap(massif_inner, MASSIF_CENTER, MASSIF_SCALE));
        return;
    }

    if (pass == PASS_MASSIF_WRITEBACK) {
        // Rust: `let massif = gaussian(massif, 2.0)` -> ALL later uses (base, high_mask) read
        // the BLURRED massif. Write gauss_out (the sigma=2.0 blur) back into the massif buffer.
        massif.v[i] = gauss_out.v[i];
        return;
    }

    if (pass == PASS_BASE) {
        // massif here is ALREADY the gaussian(massif, 2.0) (Rust reassigns massif to its blur).
        // base = affine(uplift*(1.50*massif + 0.18*ranges - 0.46*lowland), BASE)
        float inner = STYLE_UPLIFT_GAIN * (1.50 * massif.v[i] + 0.18 * ranges.v[i] - 0.46 * lowland.v[i]);
        base.v[i] = affine_remap(inner, BASE_CENTER, BASE_SCALE);
        return;
    }

    if (pass == PASS_FLOW_PRE_BASE) {
        flow_pre.v[i] = base.v[i];
        return;
    }

    if (pass == PASS_FLOW_PRE_ROUGH) {
        // rough_surface = base + 0.18*affine(ranges, RANGES_ZSCORE_CENTER, RANGES_ZSCORE_SCALE)
        flow_pre.v[i] = base.v[i] + 0.18 * affine_remap(ranges.v[i], RANGES_ZSCORE_CENTER, RANGES_ZSCORE_SCALE);
        return;
    }

    if (pass == PASS_FLOW_PRE_PREBLUR_IN) {
        // copy flow_pre -> gauss_in so the gaussian (sigma=1.15) pre-blur can run on it.
        gauss_in.v[i] = flow_pre.v[i];
        return;
    }

    if (pass == PASS_FLOW_PRE_FROM_GAUSS) {
        // flow_pre <- gauss_out (the sigma=1.15 pre-blurred surface).
        flow_pre.v[i] = gauss_out.v[i];
        return;
    }

    if (pass == PASS_ACC_INIT) {
        acc_a.v[i] = 1.0;
        acc_b.v[i] = 1.0;
        return;
    }

    if (pass == PASS_FLOW_RELAX) {
        // PULL relaxation step (mirror of flow_accum_spike.glsl) on flow_pre, MFD weights.
        // acc_prev/acc_next selected by flow_dir: 0 = a->b, 1 = b->a.
        float p = P.flow_power;
        float hc = flow_pre.v[i];
        float acc = 1.0;
        // 8-neighbour table (dy,dx,dist) verbatim (diagonal = literal 1.41421356237).
        for (int k = 0; k < 8; ++k) {
            int oy, ox; float dist;
            if      (k == 0) { oy = -1; ox = -1; dist = 1.41421356237; }
            else if (k == 1) { oy = -1; ox =  0; dist = 1.0; }
            else if (k == 2) { oy = -1; ox =  1; dist = 1.41421356237; }
            else if (k == 3) { oy =  0; ox = -1; dist = 1.0; }
            else if (k == 4) { oy =  0; ox =  1; dist = 1.0; }
            else if (k == 5) { oy =  1; ox = -1; dist = 1.41421356237; }
            else if (k == 6) { oy =  1; ox =  0; dist = 1.0; }
            else             { oy =  1; ox =  1; dist = 1.41421356237; }
            int nx = cx + ox;
            int ny = cy + oy;
            if (nx < 0 || nx >= cols || ny < 0 || ny >= rows) continue;
            int ni = ny * cols + nx;
            float hn = flow_pre.v[ni];
            float drop_nc = (hn - hc) / dist;   // c downhill of n?
            if (drop_nc <= 0.0) continue;
            float w_nc = pow(drop_nc, p);
            // n's total downhill weight (normalization). Sum over n's 8 strictly-lower nbrs.
            float total_n = 0.0;
            for (int j = 0; j < 8; ++j) {
                int ojy, ojx; float jdist;
                if      (j == 0) { ojy = -1; ojx = -1; jdist = 1.41421356237; }
                else if (j == 1) { ojy = -1; ojx =  0; jdist = 1.0; }
                else if (j == 2) { ojy = -1; ojx =  1; jdist = 1.41421356237; }
                else if (j == 3) { ojy =  0; ojx = -1; jdist = 1.0; }
                else if (j == 4) { ojy =  0; ojx =  1; jdist = 1.0; }
                else if (j == 5) { ojy =  1; ojx = -1; jdist = 1.41421356237; }
                else if (j == 6) { ojy =  1; ojx =  0; jdist = 1.0; }
                else             { ojy =  1; ojx =  1; jdist = 1.41421356237; }
                int dxn = nx + ojx;
                int dyn = ny + ojy;
                if (dxn < 0 || dxn >= cols || dyn < 0 || dyn >= rows) continue;
                float hd = flow_pre.v[dyn * cols + dxn];
                float drop_nd = (hn - hd) / jdist;
                if (drop_nd > 0.0) total_n += pow(drop_nd, p);
            }
            float prev_n = (P.flow_dir == 0) ? acc_a.v[ni] : acc_b.v[ni];
            acc += prev_n * (w_nc / (total_n + 1e-12));
        }
        if (P.flow_dir == 0) acc_b.v[i] = acc;
        else                 acc_a.v[i] = acc;
        return;
    }

    if (pass == PASS_DISCHARGE) {
        // discharge = clip(log1p(acc)/log1p(rows*cols), 0,1) into gauss_in (then GAUSS spread).
        // acc lives in acc_a or acc_b depending on the LAST relax write; the orchestrator
        // tells us via flow_dir: flow_dir==0 means the final acc is in acc_a, ==1 in acc_b.
        // (orchestrator sets flow_dir to the buffer holding the final acc for this pass.)
        float acc = (P.flow_dir == 0) ? acc_a.v[i] : acc_b.v[i];
        float n = float(rows) * float(cols);
        float log_size = log(1.0 + n);
        gauss_in.v[i] = clamp(log(1.0 + acc) / log_size, 0.0, 1.0);
        return;
    }

    if (pass == PASS_PRIMARY_MASK) {
        // primary = clip(gauss_out [spread discharge], 0,1) ; primary_mask = smoothstep(PLO,PHI,primary)
        float primary = clip01(gauss_out.v[i]);
        primary_mask.v[i] = ss(PRIMARY_THRESH_LO, PRIMARY_THRESH_HI, primary);
        return;
    }

    if (pass == PASS_TRIB_MASK) {
        float tributary = clip01(gauss_out.v[i]);
        tributary_mask.v[i] = ss(TRIBUTARY_THRESH_LO, TRIBUTARY_THRESH_HI, tributary);
        return;
    }

    if (pass == PASS_MASKS) {
        // high_mask = smoothstep(0.48,0.86, massif)*(1 - 0.38*lowland)
        // valley_mask = clip(0.72*primary_mask + 0.46*tributary_mask, 0,1)
        high_mask.v[i] = ss(0.48, 0.86, massif.v[i]) * (1.0 - 0.38 * lowland.v[i]);
        valley_mask.v[i] = clip01(0.72 * primary_mask.v[i] + 0.46 * tributary_mask.v[i]);
        return;
    }

    if (pass == PASS_ASSEMBLE) {
        // gains: ridge_g = STYLE_RIDGE_GAIN * SEAMSAFE_RIDGE_GAIN, etc.
        float ridge_g  = STYLE_RIDGE_GAIN  * SEAMSAFE_RIDGE_GAIN;
        float detail_g = STYLE_DETAIL_GAIN * SEAMSAFE_DETAIL_GAIN;
        float carve_g  = STYLE_CARVE_GAIN  * SEAMSAFE_CARVE_GAIN;
        float branch_g = STYLE_BRANCH_GAIN * SEAMSAFE_BRANCH_GAIN;
        float hm = high_mask.v[i];
        float hv = base.v[i];
        hv += ridge_g  * (0.08 + 0.58 * hm) * (0.24 * ridge_detail.v[i]);
        hv += detail_g * (0.04 + 0.34 * hm) * (0.34 * near_detail.v[i]);
        hv -= carve_g  * (0.42 + 0.58 * hm) * primary_mask.v[i];
        hv -= branch_g * (0.18 + 0.42 * hm) * tributary_mask.v[i];
        height.v[i] = hv;
        return;
    }

    if (pass == PASS_FLOOR_MASK) {
        // gauss_out = gaussian(valley_mask, sigma=1.2) = valley_blur.
        // floor_mask = clip(smoothstep(0.48,0.86, valley_blur) + 0.24*lowland, 0,1)
        floor_mask.v[i] = clip01(ss(0.48, 0.86, gauss_out.v[i]) + 0.24 * lowland.v[i]);
        return;
    }

    if (pass == PASS_FLOOR_BLEND) {
        // gauss_out = gaussian(height, sigma=max(floor_smooth_px,0.2)) = floor.
        // height = height*(1-0.38*floor_mask) + floor*(0.38*floor_mask) ; height -= 0.18*floor_mask
        float fm = floor_mask.v[i];
        float h = height.v[i] * (1.0 - 0.38 * fm) + gauss_out.v[i] * (0.38 * fm);
        h -= 0.18 * fm;
        height.v[i] = h;
        return;
    }

    if (pass == PASS_FINAL) {
        // gauss_out = gaussian(height, sigma=1.20) = height_blur.
        // final_blend = 0.74*height + 0.26*height_blur ; height = affine(final_blend, FINAL)
        float final_blend = 0.74 * height.v[i] + 0.26 * gauss_out.v[i];
        height.v[i] = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
        return;
    }
}
