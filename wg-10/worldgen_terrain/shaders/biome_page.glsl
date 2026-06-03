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
//   * The generic scratch POOL: POOL_SLOTS reusable float[rows*cols] buffers + pool_read /
//     pool_write(slot,idx[,val]). Biomes with more sub-fields than the fixed named buffers
//     (grassland has 12) stage intermediate fields in pool slots; to blur a slot, COPY_POOL it
//     into gauss_in, GAUSS, then read gauss_out (or POOL_FROM_GAUSS to stash back). The pool is
//     ADDITIVE -- mountain never touches it; its named buffers/behavior are byte-unchanged.
//   * The GENERIC PASS bodies (handled inline in main()):
//       MESHGRID, COPY, COPY_POOL, POOL_FROM_GAUSS, GAUSS_AXIS0, GAUSS_AXIS1, FLOW_PRE_BASE,
//       FLOW_PRE_PREBLUR_IN, FLOW_PRE_FROM_GAUSS, ACC_INIT, FLOW_RELAX, DISCHARGE, CROP
//   * The flow MECHANISM (FLOW_RELAX/DISCHARGE) is generic; note however that the flow SOURCE
//     surfaces are biome-specific. FLOW_PRE_BASE (flow_pre <- base) is the one generic source
//     (just copies `base`); any biome that needs a DIFFERENT flow source (e.g. mountain's
//     FLOW_PRE_ROUGH, which mixes RANGES_ZSCORE into base) implements it as a biome PASS.
//
// Every biome_<name>.glsl FRAGMENT MUST define exactly:
//       void biome_pass(int pass, int cx, int cy, int i)
//   covering the biome-specific PASS_* values for that biome. Shared/generic codes occupy 0..26
//   (27..31 reserved for future generic passes); BIOME-private pass codes start at 32 so two
//   biomes' private codes never collide with the shared machine codes. (Mountain predates this
//   convention and reuses some shared 1..23 slots as its biome passes -- that is grandfathered;
//   NEW biomes use 32+.) main() dispatches
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
const int PASS_COPY_POOL           = 25; // [GENERIC] gauss_in <- pool[pool_sel] (to blur a pool slot)
const int PASS_POOL_FROM_GAUSS     = 26; // [GENERIC] pool[pool_sel] <- gauss_out (stash a blur)
const int PASS_CROP_IMG            = 27; // [GENERIC] out_img[core] <- height[apron-offset] (RUNTIME crop-to-texture sibling of PASS_CROP; writes the R32F image at binding 41 instead of the core_out storage buffer. The readback TEST harness uses PASS_CROP; the runtime producer uses this.)
// 28..31 reserved for future GENERIC passes. BIOME-private passes start at 32 (see fragments).

// ---------------------------------------------------------------------------
// GENERIC COMPOSE passes (Slice-4b.11): the biome-AGNOSTIC blend layer that composes the OUTPUTS
// of N per-recipe height fields into one. A bit-close port of biome_compose.rs
// (blend_field / blend_height_favored / compose_biomes). These are handled INLINE in main()
// (they `return` BEFORE biome_pass()), so they never collide with any biome-private code, no
// matter which biome FRAGMENT is concatenated. They live in a HIGH generic block (60..) to stay
// clear of every biome-private code (currently <=53). Fixed compose buffer roles (compose runs
// standalone -- no meshgrid/pointwise -- so it freely reuses the named buffers):
//   acc      = height (binding 14)        -- the running accumulator (init = fields[0])
//   acc_w    = base   (binding 9)         -- the running weight       (init = weights[0])
//   f        = pool slot 0                -- the current recipe's field
//   w        = pool slot 1                -- the current recipe's weight
//   w_acc    = lowland (binding 7)        -- per-step blend weight acc_w/(acc_w+w+1e-12)
//   relief_a = range_envelope (binding 6) -- |acc - gaussian_nearest(acc, relief_sigma_px)|
//   relief_b = massif (binding 8)         -- |f   - gaussian_nearest(f,   relief_sigma_px)|
// The relief proxy reuses the SAME separable gaussian (COPY src -> gauss_in, GAUSS_AXIS0/1 ->
// gauss_out) the biomes use, sigma = relief_sigma_px = 6.0 (clamp-to-edge nearest, truncate 4.0).
// Compose params arrive via spare push-constant float pads: P.pad0 = favor_strength,
// P.pad1 = relief_confidence_floor (0 for every non-compose dispatch -> byte-identical push).
const int PASS_COMPOSE_RELIEF_A_STORE = 60; // [GENERIC] range_envelope <- abs(height - gauss_out)
const int PASS_COMPOSE_RELIEF_B_STORE = 61; // [GENERIC] massif <- abs(pool0 - gauss_out)
// Relief-proxy f32 self-noise dead-zone. The oracle is f64, where gaussian(constant)==constant to
// ~1e-15, so a FLAT field has relief==0 exactly and the favored nudge vanishes (signal==0 ->
// w_adj==w_a -> a plain field blend). On the GPU the separable blur is f32: gaussian(constant c)
// != c (the per-tap c*tap products round independently; this does NOT cancel and is NOT fixable by
// renormalizing the kernel taps). That leaves a spurious relief ~= |c|*9.6e-7 on a flat field. Tiny
// in itself, but signal = total/(total + relief_confidence_floor=1e-3) AMPLIFIES it, so the nudge no
// longer vanishes and w_adj drifts ~1e-3 from w_a -> out = w_adj*a+(1-w_adj)*b is off by ~drift*|a-b|,
// up to ~13 m / 2.76% when |a-b| ~ 900 (the windowed rec=4 favored_ramp_flat_flat failure). Fix: in
// the relief STORE passes, snap relief that is below this RELATIVE floor (== pure blur self-noise) to
// exactly 0, so a flat field reproduces the oracle's relief==0. The floor is ~10x the measured worst
// self-noise (9.6e-7) and ~60x BELOW the smallest genuine local relief (a slope-0.01 m/px ramp gives
// relief ~0.06 m), so structured/favored fields (rec=1/2/3/6) are untouched. This targets ONLY the
// compose relief proxy; the 11 biome blurs (which read gauss_out directly) are byte-unchanged.
const float COMPOSE_RELIEF_F32_FLOOR_REL = 1e-5;
const int PASS_COMPOSE_WACC           = 62; // [GENERIC] lowland <- base/(base+pool1+1e-12)
const int PASS_COMPOSE_BLEND_FIELD    = 63; // [GENERIC] height <- lowland*height + (1-lowland)*pool0
const int PASS_COMPOSE_BLEND_FAVORED  = 64; // [GENERIC] height <- favored-blend(height, pool0, lowland)
const int PASS_COMPOSE_ACCW_ADD       = 65; // [GENERIC] base += pool1  (acc_w += w)
const int PASS_COMPOSE_COPY_ACC       = 66; // [GENERIC] gauss_in <- height (to blur the accumulator)

// copy_sel codes for PASS_COPY (which field buffer to copy into gauss_in)
const int CP_RANGES   = 0;
const int CP_MASSIF   = 1;
const int CP_VALLEY   = 2;
const int CP_HEIGHT   = 3;

// ---------------------------------------------------------------------------
// GENERIC scratch POOL slot count. Biomes with more sub-fields (grassland uses 12) index
// pool0..pool(POOL_SLOTS-1) by slot via pool_read/pool_write. To add a biome that needs more
// slots: bump POOL_SLOTS here AND the matching POOL_SLOTS in biome_page_compute.rs (one buffer
// per slot is allocated + bound). MUST match biome_page_compute.rs::POOL_SLOTS.
// ---------------------------------------------------------------------------
const int POOL_SLOTS = 16;

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
// GENERIC scratch POOL: POOL_SLOTS reusable float[rows*cols] buffers (bindings 24..24+SLOTS-1).
// Biome fragments read/write these by slot via pool_read/pool_write. They are ADDITIVE to the
// fixed named buffers above (mountain never touches them; its bindings/behavior are unchanged).
// One binding per slot (a fixed indexed set is portable; SSBO-arrays-of-blocks are not).
// ---------------------------------------------------------------------------
layout(set = 0, binding = 24, std430) restrict buffer Pool0  { float v[]; } pool0;
layout(set = 0, binding = 25, std430) restrict buffer Pool1  { float v[]; } pool1;
layout(set = 0, binding = 26, std430) restrict buffer Pool2  { float v[]; } pool2;
layout(set = 0, binding = 27, std430) restrict buffer Pool3  { float v[]; } pool3;
layout(set = 0, binding = 28, std430) restrict buffer Pool4  { float v[]; } pool4;
layout(set = 0, binding = 29, std430) restrict buffer Pool5  { float v[]; } pool5;
layout(set = 0, binding = 30, std430) restrict buffer Pool6  { float v[]; } pool6;
layout(set = 0, binding = 31, std430) restrict buffer Pool7  { float v[]; } pool7;
layout(set = 0, binding = 32, std430) restrict buffer Pool8  { float v[]; } pool8;
layout(set = 0, binding = 33, std430) restrict buffer Pool9  { float v[]; } pool9;
layout(set = 0, binding = 34, std430) restrict buffer Pool10 { float v[]; } pool10;
layout(set = 0, binding = 35, std430) restrict buffer Pool11 { float v[]; } pool11;
layout(set = 0, binding = 36, std430) restrict buffer Pool12 { float v[]; } pool12;
layout(set = 0, binding = 37, std430) restrict buffer Pool13 { float v[]; } pool13;
layout(set = 0, binding = 38, std430) restrict buffer Pool14 { float v[]; } pool14;
layout(set = 0, binding = 39, std430) restrict buffer Pool15 { float v[]; } pool15;

// ---------------------------------------------------------------------------
// VENT BUFFER (binding 40): a SMALL packed vent list for VOLCANIC. ADDITIVE -- the 10 proven
// biomes never read it (they leave it zeroed). The numpy-PCG64 RNG that places the vents +
// per-vent flow directions stays in RUST (recipes_volcanic::packed_vents, parity-exact); the GPU
// only CONSUMES this uploaded buffer doing pure f32 cone/crater/shield/flow math -- NO RNG in GLSL.
// Packed `(vx, vz, amp, dir0, dir1, dir2, dir3)` = VENT_STRIDE(7) floats per vent, padded to
// MAX_VENTS(8) entries; the active count is the `vent_count` push constant. MUST match
// recipes_volcanic::{MAX_VENTS, VENT_STRIDE} + biome_page_compute.rs.
// ---------------------------------------------------------------------------
const int VENT_STRIDE = 7;
const int MAX_VENTS   = 8;
layout(set = 0, binding = 40, std430) restrict readonly buffer Vents { float v[]; } vents;

// ---------------------------------------------------------------------------
// RUNTIME OUTPUT IMAGE (binding 41): the caller-owned R32F page texture the RUNTIME producer
// writes via PASS_CROP_IMG (the crop-to-texture sibling of PASS_CROP). ADDITIVE -- binding 0 is
// already taken by Wx here (unlike legacy height_page.glsl), so the image goes at binding 41.
// The readback TEST harness (run_inner) does NOT bind this image and never dispatches
// PASS_CROP_IMG, so a write-only image left unwritten in those dispatches is inert; the runtime
// producer's uniform set DOES bind it (every binding the shader declares must be satisfied by
// whatever uniform set a given pipeline uses -- so the runtime set provides binding 41, and the
// test-harness set still provides 0..40 exactly as before, byte-identical).
// ---------------------------------------------------------------------------
layout(set = 0, binding = 41, r32f) writeonly uniform image2D out_img;

// pool slot accessors. A small switch keeps the slot index data-driven (the `pool_sel` push
// constant for the generic pool passes; a literal for biome fragments). Out-of-range slots are
// inert (read 0 / no write) -- a wrong slot fails parity loudly rather than corrupting memory.
float pool_read(int slot, int idx) {
    if (slot == 0)  return pool0.v[idx];
    if (slot == 1)  return pool1.v[idx];
    if (slot == 2)  return pool2.v[idx];
    if (slot == 3)  return pool3.v[idx];
    if (slot == 4)  return pool4.v[idx];
    if (slot == 5)  return pool5.v[idx];
    if (slot == 6)  return pool6.v[idx];
    if (slot == 7)  return pool7.v[idx];
    if (slot == 8)  return pool8.v[idx];
    if (slot == 9)  return pool9.v[idx];
    if (slot == 10) return pool10.v[idx];
    if (slot == 11) return pool11.v[idx];
    if (slot == 12) return pool12.v[idx];
    if (slot == 13) return pool13.v[idx];
    if (slot == 14) return pool14.v[idx];
    if (slot == 15) return pool15.v[idx];
    return 0.0;
}

void pool_write(int slot, int idx, float val) {
    if (slot == 0)  { pool0.v[idx]  = val; return; }
    if (slot == 1)  { pool1.v[idx]  = val; return; }
    if (slot == 2)  { pool2.v[idx]  = val; return; }
    if (slot == 3)  { pool3.v[idx]  = val; return; }
    if (slot == 4)  { pool4.v[idx]  = val; return; }
    if (slot == 5)  { pool5.v[idx]  = val; return; }
    if (slot == 6)  { pool6.v[idx]  = val; return; }
    if (slot == 7)  { pool7.v[idx]  = val; return; }
    if (slot == 8)  { pool8.v[idx]  = val; return; }
    if (slot == 9)  { pool9.v[idx]  = val; return; }
    if (slot == 10) { pool10.v[idx] = val; return; }
    if (slot == 11) { pool11.v[idx] = val; return; }
    if (slot == 12) { pool12.v[idx] = val; return; }
    if (slot == 13) { pool13.v[idx] = val; return; }
    if (slot == 14) { pool14.v[idx] = val; return; }
    if (slot == 15) { pool15.v[idx] = val; return; }
}

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
    int pool_sel;    // PASS_COPY_POOL / PASS_POOL_FROM_GAUSS: which scratch pool slot
    int vent_count;  // VOLCANIC: active vents in the vent buffer (0 for the 10 non-volcanic biomes -> byte-identical to the former ipad1)
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

    // PASS_CROP_IMG: RUNTIME crop-to-texture. Same CORE iteration + apron-offset src index as
    // PASS_CROP, but writes the R32F output image (binding 41) at (cx, cy) instead of the
    // core_out storage buffer. The math is IDENTICAL to PASS_CROP -> the runtime texture matches
    // the readback core bit-for-bit (the LATER windowed 576 parity gate proves this).
    if (pass == PASS_CROP_IMG) {
        int a = P.apron_px;
        int core_cols = cols - 2 * a;
        int core_rows = rows - 2 * a;
        if (cx >= core_cols || cy >= core_rows) return;
        int src = (cy + a) * cols + (cx + a);
        imageStore(out_img, ivec2(cx, cy), vec4(height.v[src], 0.0, 0.0, 1.0));
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

    if (pass == PASS_COPY_POOL) {
        // copy a selected scratch-pool slot into gauss_in (so the gaussian passes can blur it).
        gauss_in.v[i] = pool_read(P.pool_sel, i);
        return;
    }

    if (pass == PASS_POOL_FROM_GAUSS) {
        // stash the gaussian result (gauss_out) back into a selected scratch-pool slot.
        pool_write(P.pool_sel, i, gauss_out.v[i]);
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

    // ===== GENERIC COMPOSE passes (Slice-4b.11) -- handled inline so they never reach a biome
    // fragment (collision-proof regardless of the concatenated biome). Bit-close port of
    // biome_compose.rs. =====
    if (pass == PASS_COMPOSE_COPY_ACC) {
        // gauss_in <- accumulator (height), so the gaussian passes can blur it (relief proxy of acc).
        gauss_in.v[i] = height.v[i];
        return;
    }

    if (pass == PASS_COMPOSE_RELIEF_A_STORE) {
        // relief_a = |acc - gaussian_nearest(acc, relief_sigma_px)|. acc=height, blur=gauss_out.
        // Snap pure f32-blur self-noise to 0 so a FLAT field matches the oracle's relief==0 (see
        // COMPOSE_RELIEF_F32_FLOOR_REL). Genuine structured relief is far above this floor -> inert.
        float fv = height.v[i];
        float r  = abs(fv - gauss_out.v[i]);
        if (r <= COMPOSE_RELIEF_F32_FLOOR_REL * max(abs(fv), 1.0)) r = 0.0;
        range_envelope.v[i] = r;
        return;
    }

    if (pass == PASS_COMPOSE_RELIEF_B_STORE) {
        // relief_b = |f - gaussian_nearest(f, relief_sigma_px)|. f=pool0, blur=gauss_out.
        // Same f32-blur self-noise dead-zone as relief_a (the flat-field smoking gun was rec=4's
        // relief_b on a flat 'b' surface; both proxies get the snap for symmetry / flat-acc cases).
        float fv = pool_read(0, i);
        float r  = abs(fv - gauss_out.v[i]);
        if (r <= COMPOSE_RELIEF_F32_FLOOR_REL * max(abs(fv), 1.0)) r = 0.0;
        massif.v[i] = r;
        return;
    }

    if (pass == PASS_COMPOSE_WACC) {
        // w_acc = acc_w / (acc_w + w + 1e-12). acc_w=base, w=pool1, w_acc=lowland.
        float aw = base.v[i];
        float w  = pool_read(1, i);
        lowland.v[i] = aw / (aw + w + 1e-12);
        return;
    }

    if (pass == PASS_COMPOSE_BLEND_FIELD) {
        // out = w_acc*acc + (1-w_acc)*f. acc=height, f=pool0, w_acc=lowland.
        float w = lowland.v[i];
        height.v[i] = w * height.v[i] + (1.0 - w) * pool_read(0, i);
        return;
    }

    if (pass == PASS_COMPOSE_BLEND_FAVORED) {
        // Relief-favored blend (bit-close to biome_compose::blend_height_favored). Term order:
        //   total  = relief_a + relief_b
        //   favor  = relief_a / (total + 1e-9)
        //   signal = total / (total + relief_confidence_floor)
        //   band   = 1 - |2*w_a - 1|
        //   w_adj  = clip(w_a + (favor-0.5)*favor_strength*band*signal, 0, 1)
        //   out    = w_adj*a + (1-w_adj)*b   (a=acc=height, b=f=pool0, w_a=w_acc=lowland)
        float relief_a = range_envelope.v[i];
        float relief_b = massif.v[i];
        float total    = relief_a + relief_b;
        float favor    = relief_a / (total + 1e-9);
        float signal   = total / (total + P.pad1);   // P.pad1 = relief_confidence_floor
        float w        = lowland.v[i];
        float band     = 1.0 - abs(2.0 * w - 1.0);
        float w_adj    = w + (favor - 0.5) * P.pad0 * band * signal;  // P.pad0 = favor_strength
        w_adj          = clamp(w_adj, 0.0, 1.0);
        float a = height.v[i];
        float b = pool_read(0, i);
        height.v[i] = w_adj * a + (1.0 - w_adj) * b;
        return;
    }

    if (pass == PASS_COMPOSE_ACCW_ADD) {
        // acc_w += w. acc_w=base, w=pool1.
        base.v[i] = base.v[i] + pool_read(1, i);
        return;
    }

    // ===== BIOME-SPECIFIC passes -> the concatenated fragment =====
    biome_pass(pass, cx, cy, i);
}
