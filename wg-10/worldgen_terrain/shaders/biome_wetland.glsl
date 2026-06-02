// WorldGen10 Slice-4b: WETLAND biome FRAGMENT (the seam-safe recipe's biome-specific math).
//
// Concatenated AFTER recipe_primitives.glsl + biome_page.glsl (the MACHINE). It defines the
// wetland-specific constants + helpers + biome_pass(), implementing the BIOME PASS_* values
// the machine forwards to it. The MACHINE owns all bindings, the `Params` push constant `P`,
// the generic leaf helpers (cell_idx / clamp_idx / affine_remap / ss / clip01 / rotated0 /
// recipe_recursive_domain_warp / rmf / fbm5), the global storage buffers (INCLUDING the generic
// scratch POOL pool0..poolN + the pool_read/pool_write switch), and the generic passes
// (MESHGRID, COPY, COPY_POOL, POOL_FROM_GAUSS, GAUSS_*, FLOW_PRE_*, ACC_INIT, FLOW_RELAX,
// DISCHARGE, CROP). This fragment carries NO #version / #[compute].
//
// This is the GPU mirror of recipes_wetland.rs::generate_seamsafe (the f64 parity ORACLE),
// style = STYLES[0] = delta_distributary (seed_offset=0 -> sseed=seed). EVERY constant /
// seed-offset / weight below is transcribed VERBATIM from recipes_wetland.rs.
// EDIT-BOTH-SIDES: changes here must keep parity with that oracle.
//
// SCRATCH-POOL CONTRACT (wetland's pool-slot map; needs 11 slots -> POOL_SLOTS 16 covers it):
//   pool0  = w_x          warped X coord (-> macro/micro/meander)
//   pool1  = w_z          warped Z coord (-> the same warped-coord sub-fields)
//   pool2  = macro_f      macro fBm field (-> basin, floodplain, flow_input, flat_base, height)
//   pool3  = micro        micro fBm texture (-> height texture term)
//   pool4  = meander      meander field (-> channels first assignment)
//   pool5  = basin        basin field (-> flow_input, flat_base, height)
//   pool6  = floodplain   floodplain field (-> channels, flat_base, height, texture weight)
//   pool7  = channels     channel field (first = meander*floodplain; reassigned w/ fine_flow)
//   pool8  = chan_blur22  TRANSIENT: gaussian(channels, 2.2) staged for the levee DoG, then free
//   pool9  = levees       levee field (-> height)
//   pool10 = flat_base    flat-base surface = gaussian(affine(...), smoothing_px) (-> height)
// A biome reads/writes a slot with pool_read(slot,i) / pool_write(slot,i,v). To gaussian-blur a
// pool slot: dispatch PASS_COPY_POOL (pool_sel=slot) -> gauss(sigma) -> read gauss_out (the blur).
// To stash a blur back into a slot: PASS_POOL_FROM_GAUSS (pool_sel=slot). The fixed named buffers
// (0..23) are mountain's; wetland touches only the GENERIC ones (wx/wz/flow_pre/gauss_*/height)
// plus the pool.

// ---------------------------------------------------------------------------
// ===== WETLAND biome-private PASS_* codes (start at 32 per the interface convention) =====
// MUST match biome_page_compute.rs WL_* consts.
// ---------------------------------------------------------------------------
const int WL_POINTWISE      = 32; // warp -> pool0=w_x,pool1=w_z ; macro_f=pool2 ; micro=pool3 ; meander=pool4
const int WL_ONE_MINUS_MACRO= 33; // gauss_in <- 1 - macro_f (pre-basin blur sigma=5.8)
const int WL_BASIN          = 34; // pool5 = basin = smoothstep(0.48,0.86, gauss_out)
const int WL_FLOODPLAIN_PRE = 35; // gauss_in <- 1 - |macro_f - 0.42| (pre-floodplain blur sigma=5.2)
const int WL_FLOODPLAIN     = 36; // pool6 = floodplain = smoothstep(0.36,0.78, gauss_out)
const int WL_CHANNELS_FIRST = 37; // pool7 = channels = meander * floodplain
const int WL_FLOW_PRE       = 38; // flow_pre <- affine(macro_f - 0.34*basin, FLOW_INPUT) (NO clip)
const int WL_CHANNELS_FLOW  = 39; // pool7 = clip(0.68*channels + 0.50*smoothstep(0.56,0.94, gauss_out))
const int WL_LEVEES         = 40; // pool9 = smoothstep(0.02,0.18, blur22-blur52) * (1 - ss(0.42,0.86,channels))
const int WL_FLAT_BASE_PRE  = 41; // pool10 = affine(0.42*macro - 0.58*basin + 0.20*floodplain, FLAT_BASE)
const int WL_ASSEMBLE       = 42; // height = weighted sum (macro/basin/floodplain/channels/levees/micro) blended w/ flat_base
const int WL_FINAL          = 43; // height = affine(0.88*height + 0.12*gauss_out[=gaussian(h,1.2)], FINAL)

// ---------------------------------------------------------------------------
// ===== WETLAND constants (verbatim from recipes_wetland.rs) =====
// ---------------------------------------------------------------------------
// affine-remap constants
const float MACRO_CENTER        = -0.38;
const float MACRO_SCALE         =  1.14;
const float FLOW_INPUT_CENTER   =  0.28;
const float FLOW_INPUT_SCALE    =  3.00;
const float MICRO_CENTER        =  0.00;
const float MICRO_SCALE         =  3.29;
const float FLAT_BASE_CENTER    =  0.13;
const float FLAT_BASE_SCALE     =  3.49;
const float MACRO_ZSCORE_CENTER =  0.50;
const float MACRO_ZSCORE_SCALE  =  4.00;
const float FINAL_CENTER        =  0.00;
const float FINAL_SCALE         =  0.82;

// delta_distributary style (STYLES[0]) fields the seam-safe pipeline reads.
const float STYLE_ANGLE_RAD       = 0.08;
const float STYLE_CHANNEL_GAIN    = 1.32;
const float STYLE_FLOODPLAIN_GAIN = 1.08;
const float STYLE_LEVEE_GAIN      = 0.90;
const float STYLE_BASIN_GAIN      = 0.74;
const float STYLE_TEXTURE_GAIN    = 0.32;
const float STYLE_SMOOTHING_PX    = 4.4;
// seed_offset = 0 (sseed = P.seed + 0; we add it explicitly to mirror sseed = seed + offset).
const int   STYLE_SEED_OFFSET     = 0;

const float WETLAND_PI = 3.14159265358979323846;

// ---------------------------------------------------------------------------
// meander_field_point: mirror of recipes_wetland.rs::meander_field_point
//   (_meander_field for a single point, seam_safe_mode=True). wx/wz are the ALREADY
//   domain-warped coords (w_x/w_z). Rotation centre is fixed at the world origin (cx=cz=0).
//     (rx, rz) = rotated(wx, wz, angle_rad, 0, 0)
//     meander  = fbm(rx, rz, 1/(span*0.24), 5, seed+120, gain=0.55) * span*0.050
//     trunk_phase = (rz + meander) / max(span*0.090, 1.0) * 2*pi
//     trunk = exp(-((sin(trunk_phase)/0.18)^2))
//     distributary = ridged_multifractal(rx+meander, rz*0.38, 1/(span*0.13), 4, seed+140, 0.50)
//     -> clip(0.62*trunk + 0.58*smoothstep(0.50,0.88, distributary), 0, 1)
//   `seed` here is sseed (the call site passes sseed, NOT an extra offset).
// ---------------------------------------------------------------------------
float meander_field_point(float wx_w, float wz_w, float span, int seed) {
    vec2 r = rotated0(wx_w, wz_w, STYLE_ANGLE_RAD);
    float rx = r.x;
    float rz = r.y;
    float meander = fbm5(rx, rz, 1.0 / (span * 0.24), 5, seed + 120, 0.55) * span * 0.050;
    float trunk_phase = (rz + meander) / max(span * 0.090, 1.0) * WETLAND_PI * 2.0;
    float s = sin(trunk_phase) / 0.18;
    float trunk = exp(-(s * s));
    float distributary = rmf(rx + meander, rz * 0.38, 1.0 / (span * 0.13), 4, seed + 140, 0.50);
    return clip01(0.62 * trunk + 0.58 * ss(0.50, 0.88, distributary));
}

// ---------------------------------------------------------------------------
// biome_pass: the wetland-specific PASS bodies. The machine has already handled the generic
// passes + guards; (cx,cy,i) are the cell coords and linear index for this invocation
// (cx<cols, cy<rows guaranteed by the machine). All pool access goes through pool_read/pool_write
// (defined in the machine). EDIT-BOTH-SIDES with recipes_wetland.rs::generate_seamsafe.
// ---------------------------------------------------------------------------
void biome_pass(int pass, int cx, int cy, int i) {
    float span = max(P.feature_span_m, 1.0);
    int sseed = P.seed + STYLE_SEED_OFFSET;

    if (pass == WL_POINTWISE) {
        // recursive_domain_warp(wx,wz, span*0.018, 1/(span*0.88), sseed+10, 3, 0.54, 1.68)
        vec2 w = recipe_recursive_domain_warp(
            wx.v[i], wz.v[i],
            span * 0.018, 1.0 / (span * 0.88),
            sseed + 10, 3, 0.54, 1.68);
        float w_x = w.x;
        float w_z = w.y;
        pool_write(0, i, w_x);
        pool_write(1, i, w_z);
        // macro_f = clip(affine(fbm(w_x,w_z, 1/(span*0.96),5,sseed+30,0.58), MACRO), 0, 1)
        float m = fbm5(w_x, w_z, 1.0 / (span * 0.96), 5, sseed + 30, 0.58);
        pool_write(2, i, clip01(affine_remap(m, MACRO_CENTER, MACRO_SCALE)));
        // micro = affine(fbm(w_x,w_z, 1/(span*0.026),3,sseed+220,0.44), MICRO)  (NO clip)
        float mi = fbm5(w_x, w_z, 1.0 / (span * 0.026), 3, sseed + 220, 0.44);
        pool_write(3, i, affine_remap(mi, MICRO_CENTER, MICRO_SCALE));
        // meander field (pointwise, rotates the warped coords about fixed origin; seed = sseed)
        pool_write(4, i, meander_field_point(w_x, w_z, span, sseed));
        return;
    }

    if (pass == WL_ONE_MINUS_MACRO) {
        // gauss_in <- 1 - macro_f (then gaussian(., 5.8) -> basin source)
        gauss_in.v[i] = 1.0 - pool_read(2, i);
        return;
    }

    if (pass == WL_BASIN) {
        // pool5 = basin = smoothstep(0.48, 0.86, gauss_out[=gaussian(1-macro, 5.8)])
        pool_write(5, i, ss(0.48, 0.86, gauss_out.v[i]));
        return;
    }

    if (pass == WL_FLOODPLAIN_PRE) {
        // gauss_in <- 1 - |macro_f - 0.42| (then gaussian(., 5.2) -> floodplain source)
        gauss_in.v[i] = 1.0 - abs(pool_read(2, i) - 0.42);
        return;
    }

    if (pass == WL_FLOODPLAIN) {
        // pool6 = floodplain = smoothstep(0.36, 0.78, gauss_out[=gaussian(1-|macro-0.42|, 5.2)])
        pool_write(6, i, ss(0.36, 0.78, gauss_out.v[i]));
        return;
    }

    if (pass == WL_CHANNELS_FIRST) {
        // pool7 = channels = meander * floodplain  (first assignment; reassigned after fine_flow)
        pool_write(7, i, pool_read(4, i) * pool_read(6, i));
        return;
    }

    if (pass == WL_FLOW_PRE) {
        // flow_pre <- affine_remap(macro_f - 0.34*basin, FLOW_INPUT) (NO clip)
        float inner = pool_read(2, i) - 0.34 * pool_read(5, i);
        flow_pre.v[i] = affine_remap(inner, FLOW_INPUT_CENTER, FLOW_INPUT_SCALE);
        return;
    }

    if (pass == WL_CHANNELS_FLOW) {
        // gauss_out = spread discharge from flow_channels(flow_input, width=1.8, power=0.44).
        // channels = clip(0.68*channels + 0.50*smoothstep(0.56,0.94, fine_flow), 0, 1)
        pool_write(7, i, clip01(
            0.68 * pool_read(7, i) + 0.50 * ss(0.56, 0.94, gauss_out.v[i])));
        return;
    }

    if (pass == WL_LEVEES) {
        // dog = gaussian(channels,2.2)[=pool8] - gaussian(channels,5.2)[=gauss_out]
        // levees = smoothstep(0.02,0.18, dog) * (1 - smoothstep(0.42,0.86, channels))
        float dog = pool_read(8, i) - gauss_out.v[i];
        float lv = ss(0.02, 0.18, dog);
        pool_write(9, i, lv * (1.0 - ss(0.42, 0.86, pool_read(7, i))));
        return;
    }

    if (pass == WL_FLAT_BASE_PRE) {
        // pool10 = affine_remap(0.42*macro - 0.58*basin + 0.20*floodplain, FLAT_BASE)
        //          (the inner combo; blurred by smoothing_px next -> flat_base)
        float inner = 0.42 * pool_read(2, i) - 0.58 * pool_read(5, i) + 0.20 * pool_read(6, i);
        pool_write(10, i, affine_remap(inner, FLAT_BASE_CENTER, FLAT_BASE_SCALE));
        return;
    }

    if (pass == WL_ASSEMBLE) {
        // height  = affine_remap(macro, MACRO_ZSCORE) * 0.18
        // height -= 0.32 * basin_gain * basin
        // height -= 0.28 * floodplain_gain * floodplain
        // height -= 0.30 * channel_gain * channels
        // height += 0.54 * levee_gain * levees
        // height += 0.045 * texture_gain * micro * (0.30 + 0.70*floodplain)
        // height = 0.66*height + 0.34*flat_base
        float macro_f = pool_read(2, i);
        float micro = pool_read(3, i);
        float basin = pool_read(5, i);
        float floodplain = pool_read(6, i);
        float channels = pool_read(7, i);
        float levees = pool_read(9, i);
        float flat_base = pool_read(10, i);

        float hv = affine_remap(macro_f, MACRO_ZSCORE_CENTER, MACRO_ZSCORE_SCALE) * 0.18;
        hv -= 0.32 * STYLE_BASIN_GAIN * basin;
        hv -= 0.28 * STYLE_FLOODPLAIN_GAIN * floodplain;
        hv -= 0.30 * STYLE_CHANNEL_GAIN * channels;
        hv += 0.54 * STYLE_LEVEE_GAIN * levees;
        hv += 0.045 * STYLE_TEXTURE_GAIN * micro * (0.30 + 0.70 * floodplain);
        hv = 0.66 * hv + 0.34 * flat_base;
        height.v[i] = hv;
        return;
    }

    if (pass == WL_FINAL) {
        // gauss_out = height_blur = gaussian(height, 1.2).
        // final_blend = 0.88*height + 0.12*height_blur ; height = affine(final_blend, FINAL)
        float final_blend = 0.88 * height.v[i] + 0.12 * gauss_out.v[i];
        height.v[i] = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
        return;
    }
}
