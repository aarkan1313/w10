// WorldGen10 Slice-4b: GENERIC apron PAGE pipeline MACHINE (biome-agnostic).
//
// This is the reusable pass-state-machine that drives EVERY biome's seam-safe page bake.
// It is NOT a standalone shader: the Rust orchestrator (biome_page_compute.rs) CONCATENATES
//   recipe_primitives.glsl  (proven f32 noise/warp leaves + i64-emulated hash)
//   + THIS file              (the generic machine: bindings, leaf helpers, generic passes)
//   + biome_<name>.glsl      (the selected per-biome FRAGMENT)
// in that order, then compiles one compute shader per biome variant. Godot GLSL has no
// #include, so concat is how the parts compose. The #version lives HERE (the machine);
// concat_glsl_hoist_version() hoists it to line 1 over the whole joined text.
//
// SHAPE: one compiled compute shader, dispatched ONCE PER PASS with a different `pass`
// push-constant int. Every pass reads its input field buffer(s) + writes one output buffer
// over the apron grid (rows*cols). Whole-field operators (gaussian blur, flow accumulation)
// are themselves realized as their own GENERIC passes:
//   * GAUSS is separable: COPY a source field into gauss_in, then GAUSS_AXIS0 (blur down
//     rows -> gauss_mid) then GAUSS_AXIS1 (blur across cols -> gauss_out), using a CPU-built
//     uploaded 1-D kernel (clamp-to-edge 'nearest' on BOTH axes). Matches
//     array_ops.rs::gaussian_filter_nearest EXACTLY (axis0 then axis1).
//   * FLOW is the PULL relaxation from flow_accum_spike.glsl: one dispatch = one relaxation
//     step, ping-ponging acc_a/acc_b for STABLE_ITERS steps, on flow_pre.
//
// ===========================================================================================
// FRAGMENT INTERFACE CONTRACT (the generic/biome split)
// ===========================================================================================
// The MACHINE owns (so fragments must NOT re-declare these):
//   * ALL PASS_* + CP_* constants, the `Params` push constant `P`, and ALL storage-buffer
//     `layout(set=0, binding=N) ... buffer ...` bindings (0..23). Fragments READ/WRITE these
//     global buffers directly (regional, ranges, ridge_detail, near_detail, massif, base,
//     range_envelope, lowland, primary_mask, tributary_mask, high_mask, valley_mask, height,
//     floor_mask, gauss_in/mid/out, flow_pre, acc_a/acc_b, ...). They are in scope because the
//     fragment is concatenated AFTER this machine.
//   * The generic LEAF HELPERS (fragments reuse these directly):
//       cell_idx, clamp_idx, affine_remap, ss, clip01, rotated0,
//       recipe_recursive_domain_warp, rmf, fbm5
//   * The GENERIC PASS bodies (handled inline in main()):
//       MESHGRID, COPY, GAUSS_AXIS0, GAUSS_AXIS1, FLOW_PRE_BASE, FLOW_PRE_PREBLUR_IN,
//       FLOW_PRE_FROM_GAUSS, ACC_INIT, FLOW_RELAX, DISCHARGE, CROP
//   * The flow MECHANISM (FLOW_RELAX/DISCHARGE) is generic; note however that the flow SOURCE
//     surfaces are biome-specific. FLOW_PRE_BASE (flow_pre <- base) is the one generic source
//     (just copies `base`); any biome that needs a DIFFERENT flow source (e.g. mountain's
//     FLOW_PRE_ROUGH, which mixes RANGES_ZSCORE into base) implements it as a biome PASS.
//
// Every biome_<name>.glsl FRAGMENT MUST define exactly:
//       void biome_pass(int pass, int cx, int cy, int i)
//   covering the biome-specific PASS_* values for that biome (POINTWISE, RANGE_ENV, BASE,
//   ASSEMBLE, FINAL, etc. plus any biome-only source-surface / mask passes). main() dispatches
//   the generic passes inline and forwards everything else to biome_pass(). A fragment MAY
//   define its own consts/helpers (e.g. mountain's STYLE_* + oriented_ridges_point); those
//   names must not collide with the machine's.
// ===========================================================================================
//
// EDIT-BOTH-SIDES: the per-biome math (in the fragments) is transcribed verbatim from the f64
// recipes.rs oracle; changes there must keep parity. This machine carries NO biome math.

#version 450

layout(local_size_x = 16, local_size_y = 16) in;

// ---------------------------------------------------------------------------
// pass selector codes (MUST match biome_page_compute.rs)
// ---------------------------------------------------------------------------
const int PASS_MESHGRID    = 0;  // [GENERIC] build wx/wz from grid params
const int PASS_POINTWISE   = 1;  // [BIOME]   warp -> regional / ranges / ridge_detail / near_detail
const int PASS_COPY        = 2;  // [GENERIC] copy a selected field buffer -> gauss_in
const int PASS_GAUSS_AXIS0 = 3;  // [GENERIC] gauss_in  -> gauss_mid (blur down rows, nearest)
const int PASS_GAUSS_AXIS1 = 4;  // [GENERIC] gauss_mid -> gauss_out (blur across cols, nearest)
const int PASS_RANGE_ENV   = 5;  // [BIOME]   range_envelope = smoothstep(0.24,0.58, gauss_out)
const int PASS_LOWLAND     = 6;  // [BIOME]   lowland = combine(gauss_out=broad_range, regional)
const int PASS_MASSIF_INNER= 7;  // [BIOME]   massif = clip(affine(0.58*reg+0.86*env+0.28*gauss_out))
const int PASS_BASE        = 8;  // [BIOME]   base   = affine(uplift*(1.5*massif+0.18*ranges-0.46*lowland))
const int PASS_FLOW_PRE_BASE = 9;   // [GENERIC] flow_pre <- base (for primary)
const int PASS_FLOW_PRE_ROUGH = 10; // [BIOME]   flow_pre <- biome rough-source surface
const int PASS_FLOW_RELAX  = 11; // [GENERIC] one PULL relaxation step (acc_prev -> acc_next) on flow_pre
const int PASS_DISCHARGE   = 12; // [GENERIC] gauss_in <- clip(log1p(acc)/log1p(n)) ; (then GAUSS spread)
const int PASS_PRIMARY_MASK= 13; // [BIOME]   primary_mask  = smoothstep(PLO,PHI, gauss_out)
const int PASS_TRIB_MASK   = 14; // [BIOME]   tributary_mask= smoothstep(TLO,THI, gauss_out)
const int PASS_MASKS       = 15; // [BIOME]   high_mask / valley_mask
const int PASS_ASSEMBLE    = 16; // [BIOME]   height = base + ridge/detail - carve/branch
const int PASS_FLOOR_MASK  = 17; // [BIOME]   floor_mask = clip(smoothstep(0.48,0.86,gauss_out)+0.24*lowland)
const int PASS_FLOOR_BLEND = 18; // [BIOME]   height = blend(height, gauss_out=floor) ; -0.18*floor_mask
const int PASS_FINAL       = 19; // [BIOME]   height = affine(0.74*height + 0.26*gauss_out)
const int PASS_CROP        = 20; // [GENERIC] core_out[core] <- height[apron-offset apron grid]
const int PASS_FLOW_PRE_PREBLUR_IN = 21; // [GENERIC] gauss_in <- flow_pre (to pre-blur sigma=1.15)
const int PASS_FLOW_PRE_FROM_GAUSS = 22; // [GENERIC] flow_pre <- gauss_out (after pre-blur)
const int PASS_MASSIF_WRITEBACK    = 23; // [BIOME]   massif <- gauss_out (the gaussian(massif,2.0))
const int PASS_ACC_INIT            = 24; // [GENERIC] acc_a = acc_b = 1.0 (matches CPU acc init)

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
// GENERIC leaf helpers (mirror recipes.rs::helpers). Fragments reuse these directly.
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

// ---------------------------------------------------------------------------
// FRAGMENT-PROVIDED: every biome_<name>.glsl defines this (the biome-specific passes).
// Declared here so main() can call it; defined in the concatenated fragment below.
// ---------------------------------------------------------------------------
void biome_pass(int pass, int cx, int cy, int i);

// ---------------------------------------------------------------------------
// main: one cell, one pass. Generic passes inline; biome passes -> biome_pass().
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

    // ===== GENERIC passes (biome-agnostic machine) =====
    if (pass == PASS_MESHGRID) {
        // xs[c] = (c - apron)*spacing + ox ; zs[r] = (r - apron)*spacing + oz
        float a = float(P.apron_px);
        wx.v[i] = (float(cx) - a) * P.spacing + P.ox;
        wz.v[i] = (float(cy) - a) * P.spacing + P.oz;
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

    if (pass == PASS_FLOW_PRE_BASE) {
        flow_pre.v[i] = base.v[i];
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

    // ===== BIOME-SPECIFIC passes -> the concatenated fragment =====
    biome_pass(pass, cx, cy, i);
}
