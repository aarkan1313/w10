// WorldGen10 Slice-4b: TUNDRA biome FRAGMENT (the seam-safe recipe's biome-specific math).
//
// Concatenated AFTER recipe_primitives.glsl + biome_page.glsl (the MACHINE). It defines the
// tundra-specific constants + helpers + biome_pass(), implementing the BIOME PASS_* values
// the machine forwards to it. The MACHINE owns all bindings, the `Params` push constant `P`,
// the generic leaf helpers (cell_idx / clamp_idx / affine_remap / ss / clip01 / rotated0 /
// recipe_recursive_domain_warp / rmf / fbm5), the global storage buffers (INCLUDING the generic
// scratch POOL pool0..poolN + the pool_read/pool_write switch), and the generic passes
// (MESHGRID, COPY, COPY_POOL, POOL_FROM_GAUSS, GAUSS_*, FLOW_PRE_*, ACC_INIT, FLOW_RELAX,
// DISCHARGE, CROP). This fragment carries NO #version / #[compute].
//
// This is the GPU mirror of recipes_tundra.rs::generate_seamsafe (the f64 parity ORACLE),
// style = STYLES[0] = arctic_plain (seed_offset=0 -> sseed=seed). EVERY constant / seed-offset /
// weight / sigma below is transcribed VERBATIM from recipes_tundra.rs.
// EDIT-BOTH-SIDES: changes here must keep parity with that oracle.
//
// SCRATCH-POOL CONTRACT (tundra's pool-slot map; needs 13 slots -> POOL_SLOTS 16 covers it):
//   pool0  = w_x          warped X coord (-> fringe_ridges unrotated + rotated rx/rz source)
//   pool1  = w_z          warped Z coord (-> the same warped-coord sub-fields)
//   pool2  = macro_f      macro fBm field (-> plain-pre, flow_source, base, height macro_zsc)
//   pool3  = polygons     cellular_edges sub-field (-> pattern_inner only)
//   pool4  = stripes      ridged stripe sub-field (-> pattern_inner only)
//   pool5  = fringe_ridges raw ridged fringe (-> fringe blur input, sigma 1.8)
//   pool6  = foothills    foothill field (-> flow_source, base, height)
//   pool7  = fine         fine fBm texture (-> height texture term)
//   pool8  = plain        broad-plain mask (-> pattern multiply, flow_source)
//   pool9  = pattern      patterned-ground field (-> height + texture weight)
//   pool10 = fringe       fringe ridge mask (-> flow_source, height)
//   pool11 = drainage     drainage channel field (-> height)
//   pool12 = base         flat-base surface = gaussian(affine(...), smoothing_px) (-> height)
// A biome reads/writes a slot with pool_read(slot,i) / pool_write(slot,i,v). To gaussian-blur a
// pool slot: dispatch PASS_COPY_POOL (pool_sel=slot) -> gauss(sigma) -> read gauss_out (the blur).
// To stash a blur back into a slot: PASS_POOL_FROM_GAUSS (pool_sel=slot). The fixed named buffers
// (0..23) are mountain's; tundra touches only the GENERIC ones (wx/wz/flow_pre/gauss_*/height)
// plus the pool.

// ---------------------------------------------------------------------------
// ===== TUNDRA biome-private PASS_* codes (start at 32 per the interface convention) =====
// MUST match biome_page_compute.rs TU_* consts.
// ---------------------------------------------------------------------------
const int TU_POINTWISE   = 32; // warp -> pool0=w_x,pool1=w_z ; macro=pool2 ; polygons=pool3 ;
                               //         stripes=pool4 ; fringe_ridges=pool5 ; foothills=pool6 ; fine=pool7
const int TU_PLAIN_PRE   = 33; // gauss_in <- 1 - |macro - 0.46| (pre-plain blur sigma=5.8)
const int TU_PLAIN       = 34; // pool8 = plain = smoothstep(0.36,0.76, gauss_out)
const int TU_PATTERN_PRE = 35; // gauss_in <- 0.56*polygons + 0.44*stripes (pre-pattern blur sigma=1.2)
const int TU_PATTERN     = 36; // pool9 = pattern = smoothstep(0.46,0.86, gauss_out) * plain
const int TU_FRINGE      = 37; // pool10 = fringe = smoothstep(0.42,0.84, gauss_out[=gaussian(fringe_ridges,1.8)])
const int TU_FLOW_PRE    = 38; // flow_pre <- affine(0.62*macro+0.26*foothills+0.22*fringe-0.22*plain, FLOW_SOURCE)
const int TU_DRAINAGE    = 39; // pool11 = drainage = smoothstep(0.58,0.94, gauss_out[=flow spread discharge])
const int TU_BASE_PRE    = 40; // pool12 = affine(0.74*macro + 0.26*foothills, BASE) (blurred next -> base)
const int TU_ASSEMBLE    = 41; // height = weighted sum (macro/pattern/fringe/foothills/drainage/fine) blend w/ base
const int TU_FINAL       = 42; // height = affine(0.86*height + 0.14*gauss_out[=gaussian(h,1.1)], FINAL)

// ---------------------------------------------------------------------------
// ===== TUNDRA constants (verbatim from recipes_tundra.rs) =====
// ---------------------------------------------------------------------------
// affine-remap constants
const float MACRO_CENTER        = -0.668;
const float MACRO_SCALE         =  0.715;
const float MACRO_ZSCORE_CENTER =  0.497;
const float MACRO_ZSCORE_SCALE  =  4.24;
const float FLOW_SOURCE_CENTER  =  0.153;
const float FLOW_SOURCE_SCALE   =  5.68;
const float FINE_CENTER         =  0.000;
const float FINE_SCALE          =  3.24;
const float BASE_CENTER         =  0.405;
const float BASE_SCALE          =  5.41;
const float FINAL_CENTER        =  0.000;
const float FINAL_SCALE         =  0.82;

// arctic_plain style (STYLES[0]) fields the seam-safe pipeline reads.
const float STYLE_ANGLE_RAD     = 0.10;
const float STYLE_PLAIN_GAIN    = 1.30;
const float STYLE_PATTERN_GAIN  = 0.32;
const float STYLE_FRINGE_GAIN   = 0.18;
const float STYLE_FOOTHILL_GAIN = 0.22;
const float STYLE_DRAINAGE_GAIN = 0.48;
const float STYLE_TEXTURE_GAIN  = 0.22;
const float STYLE_SMOOTHING_PX  = 5.0;
// seed_offset = 0 (sseed = P.seed + 0; we add it explicitly to mirror sseed = seed + offset).
const int   STYLE_SEED_OFFSET   = 0;

// ---------------------------------------------------------------------------
// cellular_edges: cheap Worley/cellular edge network -> [0,1], high near cell borders.
// Mirror of recipe_noise.rs::cellular_edges(wx,wz,freq,seed,sharpness). NOT in the primitives
// file, so defined locally here (identical body to desert/coast's cellular_edges; tundra calls
// it with sharpness=1.70 for the polygon field). The feature offset uses hash2(cx,cz,seed+11)/
// seed+29; tundra's polygon call uses freq=1/(span*0.030) -> grid indices ix/iz are small (well
// within i32, matching the 32-bit-seed GLSL hash2).
// ---------------------------------------------------------------------------
float cellular_edges(float wxc, float wzc, float freq, int seed, float sharpness) {
    float x = wxc * freq;
    float z = wzc * freq;
    int ix = int(floor(x));
    int iz = int(floor(z));
    float fx = x - float(ix);
    float fz = z - float(iz);
    float f1 = 1.0 / 0.0;  // +inf
    float f2 = 1.0 / 0.0;  // +inf
    for (int dz = -1; dz <= 1; ++dz) {
        for (int dx = -1; dx <= 1; ++dx) {
            int cx = ix + dx;
            int cz = iz + dz;
            float px = float(dx) + hash2(cx, cz, seed + 11);
            float pz = float(dz) + hash2(cx, cz, seed + 29);
            float d2 = (px - fx) * (px - fx) + (pz - fz) * (pz - fz);
            // old_f1 = f1; f1 = min(f1, d2); f2 = min(max(old_f1, d2), f2)
            float old_f1 = f1;
            f1 = min(f1, d2);
            f2 = min(max(old_f1, d2), f2);
        }
    }
    float gap = sqrt(f2) - sqrt(f1);
    return 1.0 - clamp(gap * sharpness, 0.0, 1.0);
}

// ---------------------------------------------------------------------------
// biome_pass: the tundra-specific PASS bodies. The machine has already handled the generic
// passes + guards; (cx,cy,i) are the cell coords and linear index for this invocation
// (cx<cols, cy<rows guaranteed by the machine). All pool access goes through pool_read/pool_write
// (defined in the machine). EDIT-BOTH-SIDES with recipes_tundra.rs::generate_seamsafe.
// ---------------------------------------------------------------------------
void biome_pass(int pass, int cx, int cy, int i) {
    float span = max(P.feature_span_m, 1.0);
    int sseed = P.seed + STYLE_SEED_OFFSET;

    if (pass == TU_POINTWISE) {
        // w_x, w_z = recursive_domain_warp(wx, wz, span*0.020, 1/(span*0.86), sseed+10, 3, 0.54, 1.72)
        vec2 w = recipe_recursive_domain_warp(
            wx.v[i], wz.v[i],
            span * 0.020, 1.0 / (span * 0.86),
            sseed + 10, 3, 0.54, 1.72);
        float w_x = w.x;
        float w_z = w.y;
        pool_write(0, i, w_x);
        pool_write(1, i, w_z);

        // macro = clip(affine(fbm(w_x,w_z, 1/(span*0.94),5,sseed+30,gain=0.58), MACRO), 0, 1)
        float macro_raw = fbm5(w_x, w_z, 1.0 / (span * 0.94), 5, sseed + 30, 0.58);
        pool_write(2, i, clip01(affine_remap(macro_raw, MACRO_CENTER, MACRO_SCALE)));

        // rx, rz = rotated(w_x, w_z, angle, cx=0, cz=0)  (seam-safe fixed centre)
        vec2 r = rotated0(w_x, w_z, STYLE_ANGLE_RAD);
        float rx = r.x;
        float rz = r.y;

        // polygons = cellular_edges(rx, rz, 1/(span*0.030), sseed+70, sharpness=1.70)
        pool_write(3, i, cellular_edges(rx, rz, 1.0 / (span * 0.030), sseed + 70, 1.70));
        // stripes = ridged_multifractal(rx, rz*0.18, 1/(span*0.055), 4, sseed+90, gain=0.48)
        pool_write(4, i, rmf(rx, rz * 0.18, 1.0 / (span * 0.055), 4, sseed + 90, 0.48));

        // fringe_ridges = ridged_multifractal(w_x, w_z*0.48, 1/(span*0.16), 5, sseed+130, gain=0.52)
        // NOTE: UNROTATED warped coords (w_x/w_z), z anisotropy 0.48.
        pool_write(5, i, rmf(w_x, w_z * 0.48, 1.0 / (span * 0.16), 5, sseed + 130, 0.52));

        // foothills = smoothstep(0.40, 0.80, ridged_multifractal(rx, rz*0.48, 1/(span*0.22), 5, sseed+160, 0.52))
        // NOTE: ROTATED coords (rx/rz), z anisotropy 0.48, NO trailing blur.
        float foothills_raw = rmf(rx, rz * 0.48, 1.0 / (span * 0.22), 5, sseed + 160, 0.52);
        pool_write(6, i, ss(0.40, 0.80, foothills_raw));

        // fine = affine(fbm(w_x,w_z, 1/(span*0.026),3,sseed+220,gain=0.44), FINE)
        float fine_raw = fbm5(w_x, w_z, 1.0 / (span * 0.026), 3, sseed + 220, 0.44);
        pool_write(7, i, affine_remap(fine_raw, FINE_CENTER, FINE_SCALE));
        return;
    }

    if (pass == TU_PLAIN_PRE) {
        // plain_inner = 1 - |macro - 0.46| (then gaussian(., 5.8) -> plain source)
        gauss_in.v[i] = 1.0 - abs(pool_read(2, i) - 0.46);
        return;
    }

    if (pass == TU_PLAIN) {
        // pool8 = plain = smoothstep(0.36, 0.76, gauss_out[=gaussian(1-|macro-0.46|, 5.8)])
        pool_write(8, i, ss(0.36, 0.76, gauss_out.v[i]));
        return;
    }

    if (pass == TU_PATTERN_PRE) {
        // pattern_inner = 0.56*polygons + 0.44*stripes (then gaussian(., 1.2) -> pattern source)
        gauss_in.v[i] = 0.56 * pool_read(3, i) + 0.44 * pool_read(4, i);
        return;
    }

    if (pass == TU_PATTERN) {
        // pool9 = pattern = smoothstep(0.46, 0.86, gauss_out[=gaussian(.,1.2)]) * plain
        pool_write(9, i, ss(0.46, 0.86, gauss_out.v[i]) * pool_read(8, i));
        return;
    }

    if (pass == TU_FRINGE) {
        // pool10 = fringe = smoothstep(0.42, 0.84, gauss_out[=gaussian(fringe_ridges, 1.8)])
        pool_write(10, i, ss(0.42, 0.84, gauss_out.v[i]));
        return;
    }

    if (pass == TU_FLOW_PRE) {
        // flow_source_inner = 0.62*macro + 0.26*foothills + 0.22*fringe - 0.22*plain
        // flow_pre <- affine_remap(flow_source_inner, FLOW_SOURCE)  (NO clip)
        float inner = 0.62 * pool_read(2, i) + 0.26 * pool_read(6, i)
            + 0.22 * pool_read(10, i) - 0.22 * pool_read(8, i);
        flow_pre.v[i] = affine_remap(inner, FLOW_SOURCE_CENTER, FLOW_SOURCE_SCALE);
        return;
    }

    if (pass == TU_DRAINAGE) {
        // gauss_out = spread discharge from flow_channels_seam_safe(flow_source, width=2.0, power=0.48).
        // drainage = smoothstep(0.58, 0.94, channels)
        pool_write(11, i, ss(0.58, 0.94, gauss_out.v[i]));
        return;
    }

    if (pass == TU_BASE_PRE) {
        // base_inner = affine_remap(0.74*macro + 0.26*foothills, BASE) (blurred by smoothing_px next -> base)
        float inner = 0.74 * pool_read(2, i) + 0.26 * pool_read(6, i);
        pool_write(12, i, affine_remap(inner, BASE_CENTER, BASE_SCALE));
        return;
    }

    if (pass == TU_ASSEMBLE) {
        // macro_zsc = affine_remap(macro, MACRO_ZSCORE)
        // height  = 0.24 * plain_gain * macro_zsc
        // height += 0.10 * pattern_gain * pattern
        // height += 0.34 * fringe_gain * fringe
        // height += 0.40 * foothill_gain * foothills
        // height -= 0.22 * drainage_gain * drainage
        // height += 0.045 * texture_gain * fine * (0.45 + 0.55*pattern)
        // height = 0.72*height + 0.28*base
        float macro_f = pool_read(2, i);
        float foothills = pool_read(6, i);
        float fine = pool_read(7, i);
        float pattern = pool_read(9, i);
        float fringe = pool_read(10, i);
        float drainage = pool_read(11, i);
        float base_v = pool_read(12, i);

        float macro_zsc = affine_remap(macro_f, MACRO_ZSCORE_CENTER, MACRO_ZSCORE_SCALE);
        float hv = 0.24 * STYLE_PLAIN_GAIN * macro_zsc;
        hv += 0.10 * STYLE_PATTERN_GAIN * pattern;
        hv += 0.34 * STYLE_FRINGE_GAIN * fringe;
        hv += 0.40 * STYLE_FOOTHILL_GAIN * foothills;
        hv -= 0.22 * STYLE_DRAINAGE_GAIN * drainage;
        hv += 0.045 * STYLE_TEXTURE_GAIN * fine * (0.45 + 0.55 * pattern);
        hv = 0.72 * hv + 0.28 * base_v;
        height.v[i] = hv;
        return;
    }

    if (pass == TU_FINAL) {
        // gauss_out = height_blur = gaussian(height, 1.1).
        // final_blend = 0.86*height + 0.14*height_blur ; height = affine(final_blend, FINAL)
        float final_blend = 0.86 * height.v[i] + 0.14 * gauss_out.v[i];
        height.v[i] = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
        return;
    }
}
