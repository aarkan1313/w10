// WorldGen10 Slice-4b: GLACIAL biome FRAGMENT (the seam-safe recipe's biome-specific math).
//
// Concatenated AFTER recipe_primitives.glsl + biome_page.glsl (the MACHINE). It defines the
// glacial-specific constants + helpers + biome_pass(), implementing the BIOME PASS_* values
// the machine forwards to it. The MACHINE owns all bindings, the `Params` push constant `P`,
// the generic leaf helpers (cell_idx / clamp_idx / affine_remap / ss / clip01 / rotated0 /
// recipe_recursive_domain_warp / rmf / fbm5), the global storage buffers (INCLUDING the generic
// scratch POOL pool0..poolN + the pool_read/pool_write switch), and the generic passes
// (MESHGRID, COPY, COPY_POOL, POOL_FROM_GAUSS, GAUSS_*, FLOW_PRE_*, ACC_INIT, FLOW_RELAX,
// DISCHARGE, CROP). This fragment carries NO #version / #[compute].
//
// This is the GPU mirror of recipes_glacial.rs::generate_seamsafe (the f64 parity ORACLE),
// style = STYLES[0] = fjorded_troughs (seed_offset=0 -> sseed=seed; the recipe adds seed+N
// directly). EVERY constant / seed-offset / weight / sigma / rotation below is transcribed
// VERBATIM from recipes_glacial.rs. EDIT-BOTH-SIDES: changes here must keep parity.
//
// GLACIAL-SPECIFIC DIVERGENCE: glacial's trough flow channels pre-blur the surface with gaussian
// sigma=1.85 (NOT the shared 1.15). On the Rust side that is flow_channels_ex(power, width, 1.85);
// the pre-blur is just a different gaussian sigma fed into the SAME generic FLOW_PRE_PREBLUR_IN /
// gauss / FLOW_PRE_FROM_GAUSS mechanism. The fragment math is unaffected (the flow result still
// lands in gauss_out); 1.85 lives in glacial_sigmas() so kparams pre-validation covers it.
//
// SCRATCH-POOL CONTRACT (glacial's pool-slot map; needs 16 slots -> POOL_SLOTS 16 covers it):
//   pool0  = w_x          top-level warped X coord (-> regional/ridge_detail/close_detail + sub-fields)
//   pool1  = w_z          top-level warped Z coord (-> the same warped-coord sub-fields)
//   pool2  = regional     macro fBm field (-> icefield, massif, base)
//   pool3  = ridge_detail ridged-multifractal detail (-> height ridge term)
//   pool4  = close_detail close fBm detail (-> height detail term)
//   pool5  = relief       oriented_relief AFTER gaussian(1.25) (-> relief_env, massif, base, relief_z)
//   pool6  = relief_env   relief_envelope (-> icefield, massif, tributary_mask, ridge_wall)
//   pool7  = icefield     ice cap field (-> base, high_ice) ; then REUSED as ice_mask after assemble
//   pool8  = massif       massif field (-> base)
//   pool9  = base         base surface (-> flow_primary, branch_surface, height)
//   pool10 = flow_primary primary trough discharge ; then REUSED as trough_floor after primary_mask
//   pool11 = axial        axial trough field ; then REUSED as high_ice after primary_mask
//   pool12 = primary_mask primary trough mask (-> branch_surface, assemble)
//   pool13 = trib_mask    tributary mask (-> assemble)
//   pool14 = scrapes      striation field (-> height striation term)
//   pool15 = TRANSIENT    pre-blur staging: relief_raw / massif_inner / axial_raw (each consumed by
//                         the very next gaussian, then overwritten)
// A biome reads/writes a slot with pool_read(slot,i) / pool_write(slot,i,v). To gaussian-blur a
// pool slot: dispatch PASS_COPY_POOL (pool_sel=slot) -> gauss(sigma) -> read gauss_out (the blur).
// To stash a blur back into a slot: PASS_POOL_FROM_GAUSS (pool_sel=slot). The fixed named buffers
// (0..23) are mountain's; glacial touches only the GENERIC ones (wx/wz/flow_pre/gauss_*/height/
// floor_mask) plus the pool.

// ---------------------------------------------------------------------------
// ===== GLACIAL biome-private PASS_* codes (start at 32 per the interface convention) =====
// MUST match biome_page_compute.rs GC_* consts.
// ---------------------------------------------------------------------------
const int GC_POINTWISE     = 32; // warp -> pool0=w_x,pool1=w_z ; regional=pool2 ; ridge_detail=pool3 ; close_detail=pool4
const int GC_RELIEF_RAW    = 33; // pool15 = oriented_relief raw (pre gaussian(1.25))
const int GC_RELIEF        = 34; // pool5  = relief = gauss_out[=gaussian(pool15, 1.25)]
const int GC_RELIEF_ENV    = 35; // pool6  = relief_env = smoothstep(0.22,0.62, gauss_out[=gaussian(relief, 5.8)])
const int GC_ICE_INNER     = 36; // gauss_in <- 0.56*regional + 0.44*relief_env (pre-icefield blur sigma=7.0)
const int GC_ICEFIELD      = 37; // pool7  = icefield = smoothstep(0.48,0.78, gauss_out)
const int GC_MASSIF_INNER  = 38; // pool15 = clip(affine(0.72*reg+0.72*env+0.20*relief, MASSIF),0,1) (pre-massif blur 2.8)
const int GC_MASSIF        = 39; // pool8  = massif = gauss_out[=gaussian(pool15, 2.8)]
const int GC_BASE          = 40; // pool9  = base = affine(uplift*(1.34*massif+0.22*relief-0.16*(1-icefield)), BASE)
const int GC_FLOW_PRE_PRIMARY = 41; // flow_pre <- base (for flow_primary; pre-blur 1.85 done generically)
const int GC_FLOW_PRIMARY_STASH = 42; // pool10 = flow_primary = gauss_out[=spread discharge]
const int GC_AXIAL_RAW     = 43; // pool15 = axial_troughs raw (pre gaussian(max(trough_width_px*0.18,0.8)))
const int GC_AXIAL         = 44; // pool11 = axial = gauss_out[=gaussian(pool15, axial_sigma)]
const int GC_PRIMARY_MASK  = 45; // pool12 = smoothstep(0.34,0.84, clip(affine(0.58*flow_primary+1.18*axial, PRIMARY),0,1))
const int GC_BRANCH_SURFACE= 46; // flow_pre <- base + 0.10*affine(relief, RELIEF_ZSCORE) - 0.18*gauss_out[=gaussian(primary_mask,1.6)]
const int GC_TRIB_MASK     = 47; // pool13 = smoothstep(0.54,0.96, gauss_out[=tributary]) * (0.45+0.55*relief_env)
const int GC_SCRAPES       = 48; // pool14 = striations raw (NO clip, NO blur)
const int GC_ASSEMBLE      = 49; // height = base + ridge/detail/striation - trough - branch ; pool10=trough_floor ; pool11=high_ice
const int GC_FLOOR_MASK    = 50; // floor_mask = clip(smoothstep(0.36,0.80, gauss_out[=gaussian(trough_floor,1.6)])) ; pool7=ice_mask
const int GC_FLOOR_BLEND   = 51; // height = height*(1-0.52*floor_mask) + gauss_out[=gaussian(height,6.2)]*(0.52*floor_mask)
const int GC_ICE_BLEND     = 52; // height = height*(1-0.28*ice_mask) + gauss_out[=gaussian(height,4.03)]*(0.28*ice_mask) ; -= 0.16*floor_mask
const int GC_FINAL         = 53; // height = affine(0.66*height + 0.34*gauss_out[=gaussian(h,1.35)], FINAL)

// ---------------------------------------------------------------------------
// ===== GLACIAL constants (verbatim from recipes_glacial.rs) =====
// ---------------------------------------------------------------------------
// affine-remap constants
const float REGIONAL_CENTER      = -0.446;
const float REGIONAL_SCALE       =  1.181;
const float RELIEF_CENTER        = -0.008;
const float RELIEF_SCALE         =  1.465;
const float MASSIF_CENTER        =  0.154;
const float MASSIF_SCALE         =  0.787;
const float BASE_CENTER          =  0.758;
const float BASE_SCALE           =  2.487;
const float PRIMARY_CENTER       =  0.003;
const float PRIMARY_SCALE        =  0.690;
const float AXIAL_GATE_CENTER    = -0.430;
const float AXIAL_GATE_SCALE     =  1.010;
const float RELIEF_ZSCORE_CENTER =  0.503;
const float RELIEF_ZSCORE_SCALE  =  5.102;
const float RIDGE_DETAIL_CENTER  =  0.331;
const float RIDGE_DETAIL_SCALE   =  4.616;
const float CLOSE_DETAIL_CENTER  =  0.003;
const float CLOSE_DETAIL_SCALE   =  3.478;
const float STRIATIONS_CENTER    =  0.001;
const float STRIATIONS_SCALE     =  4.516;
const float FINAL_CENTER         = -0.096;
const float FINAL_SCALE          =  0.820;

// fjorded_troughs style (STYLES[0]) fields the seam-safe pipeline reads.
const float STYLE_ANGLE_RAD      = 0.56;
const float STYLE_UPLIFT_GAIN    = 1.16;
const float STYLE_TROUGH_GAIN    = 1.34;
const float STYLE_RIDGE_GAIN     = 1.02;
const float STYLE_BRANCH_GAIN    = 0.82;
const float STYLE_TROUGH_WIDTH_PX= 6.8;
const float STYLE_ICE_SMOOTH_PX  = 6.2;
const float STYLE_DETAIL_GAIN    = 0.40;
const float STYLE_STRIATION_GAIN = 0.82;
const float STYLE_ANISOTROPY     = 0.72;
// seed_offset = 0 (the recipe adds seed+N directly; sseed = P.seed + 0). Mirrored explicitly.
const int   STYLE_SEED_OFFSET    = 0;

// ---------------------------------------------------------------------------
// biome_pass: the glacial-specific PASS bodies. The machine has already handled the generic
// passes + guards; (cx,cy,i) are the cell coords and linear index for this invocation
// (cx<cols, cy<rows guaranteed by the machine). All pool access goes through pool_read/pool_write
// (defined in the machine). EDIT-BOTH-SIDES with recipes_glacial.rs::generate_seamsafe.
// ---------------------------------------------------------------------------
void biome_pass(int pass, int cx, int cy, int i) {
    float span = max(P.feature_span_m, 1.0);
    int sseed = P.seed + STYLE_SEED_OFFSET;

    if (pass == GC_POINTWISE) {
        // w_x, w_z = recursive_domain_warp(wx, wz, span*0.044, 1/(span*0.78), sseed+10, 3, 0.58, 1.70)
        vec2 w = recipe_recursive_domain_warp(
            wx.v[i], wz.v[i],
            span * 0.044, 1.0 / (span * 0.78),
            sseed + 10, 3, 0.58, 1.70);
        float w_x = w.x;
        float w_z = w.y;
        pool_write(0, i, w_x);
        pool_write(1, i, w_z);

        // regional = clip(affine(fbm(w_x,w_z, 1/(span*0.96),5,sseed+20,gain=0.56), REGIONAL), 0, 1)
        float reg = fbm5(w_x, w_z, 1.0 / (span * 0.96), 5, sseed + 20, 0.56);
        pool_write(2, i, clip01(affine_remap(reg, REGIONAL_CENTER, REGIONAL_SCALE)));

        // ridge_detail = affine(ridged_multifractal(w_x,w_z, 1/(span*0.060),4,sseed+40,gain=0.50), RIDGE_DETAIL)
        float rd = rmf(w_x, w_z, 1.0 / (span * 0.060), 4, sseed + 40, 0.50);
        pool_write(3, i, affine_remap(rd, RIDGE_DETAIL_CENTER, RIDGE_DETAIL_SCALE));

        // close_detail = affine(fbm(w_x,w_z, 1/(span*0.026),4,sseed+50,gain=0.46), CLOSE_DETAIL)
        float cd = fbm5(w_x, w_z, 1.0 / (span * 0.026), 4, sseed + 50, 0.46);
        pool_write(4, i, affine_remap(cd, CLOSE_DETAIL_CENTER, CLOSE_DETAIL_SCALE));
        return;
    }

    if (pass == GC_RELIEF_RAW) {
        // _oriented_relief raw (pre gaussian(1.25)). Rotates the WARPED coords (pool0/pool1)
        // about origin, then runs its OWN recursive_domain_warp (decay 0.56, freq_mul 1.78).
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        vec2 r = rotated0(w_x, w_z, STYLE_ANGLE_RAD);
        float rx = r.x;
        float rz = r.y;
        // recursive_domain_warp(rx, rz*anisotropy, span*0.054, 1/(span*0.68), sseed+100, 3, 0.56, 1.78)
        vec2 ww = recipe_recursive_domain_warp(
            rx, rz * STYLE_ANISOTROPY,
            span * 0.054, 1.0 / (span * 0.68),
            sseed + 100, 3, 0.56, 1.78);
        float w_rx = ww.x;
        float w_rz = ww.y;
        // long = ridged_multifractal(w_rx, w_rz, 1/(span*0.44), 5, sseed+120, gain=0.56)
        float lng = rmf(w_rx, w_rz, 1.0 / (span * 0.44), 5, sseed + 120, 0.56);
        // mid = ridged_multifractal(w_rx, w_rz, 1/(span*0.22), 4, sseed+130, gain=0.52)
        float mid = rmf(w_rx, w_rz, 1.0 / (span * 0.22), 4, sseed + 130, 0.52);
        // cross = fbm(w_rx + 0.18*w_rz, w_rz - 0.10*w_rx, 1/(span*0.30), 5, sseed+140, gain=0.54)
        float cross = fbm5(w_rx + 0.18 * w_rz, w_rz - 0.10 * w_rx, 1.0 / (span * 0.30), 5, sseed + 140, 0.54);
        float raw = 0.60 * lng + 0.22 * mid + 0.14 * cross;
        // seam-safe: clip(affine_remap(raw, RELIEF), 0, 1)  (then gaussian(1.25) follows)
        pool_write(15, i, clip01(affine_remap(raw, RELIEF_CENTER, RELIEF_SCALE)));
        return;
    }

    if (pass == GC_RELIEF) {
        // relief = gaussian(pool15, sigma=1.25) -> in gauss_out.
        pool_write(5, i, gauss_out.v[i]);
        return;
    }

    if (pass == GC_RELIEF_ENV) {
        // relief_envelope = smoothstep(0.22, 0.62, gaussian(relief, 5.8))  [gauss_out]
        pool_write(6, i, ss(0.22, 0.62, gauss_out.v[i]));
        return;
    }

    if (pass == GC_ICE_INNER) {
        // ice_inner = 0.56*regional + 0.44*relief_envelope (then gaussian(., 7.0) -> ice source)
        gauss_in.v[i] = 0.56 * pool_read(2, i) + 0.44 * pool_read(6, i);
        return;
    }

    if (pass == GC_ICEFIELD) {
        // icefield = smoothstep(0.48, 0.78, gaussian(ice_inner, 7.0))  [gauss_out]
        pool_write(7, i, ss(0.48, 0.78, gauss_out.v[i]));
        return;
    }

    if (pass == GC_MASSIF_INNER) {
        // massif_inner = clip(affine(0.72*regional + 0.72*relief_env + 0.20*relief, MASSIF), 0, 1)
        // (then gaussian(., 2.8) -> massif)
        float inner = 0.72 * pool_read(2, i) + 0.72 * pool_read(6, i) + 0.20 * pool_read(5, i);
        pool_write(15, i, clip01(affine_remap(inner, MASSIF_CENTER, MASSIF_SCALE)));
        return;
    }

    if (pass == GC_MASSIF) {
        // massif = gaussian(massif_inner, 2.8)  [gauss_out]
        pool_write(8, i, gauss_out.v[i]);
        return;
    }

    if (pass == GC_BASE) {
        // base = affine(uplift_gain*(1.34*massif + 0.22*relief - 0.16*(1 - icefield)), BASE)
        float inner = STYLE_UPLIFT_GAIN * (1.34 * pool_read(8, i) + 0.22 * pool_read(5, i)
            - 0.16 * (1.0 - pool_read(7, i)));
        pool_write(9, i, affine_remap(inner, BASE_CENTER, BASE_SCALE));
        return;
    }

    if (pass == GC_FLOW_PRE_PRIMARY) {
        // flow_primary = trough_channels_seam_safe(base, width=trough_width_px, power=0.58, preblur=1.85).
        // flow_pre <- base ; the generic flow machinery (pre-blur 1.85 + relax + discharge + spread) runs next.
        flow_pre.v[i] = pool_read(9, i);
        return;
    }

    if (pass == GC_FLOW_PRIMARY_STASH) {
        // pool10 = flow_primary = spread discharge (gauss_out).
        pool_write(10, i, gauss_out.v[i]);
        return;
    }

    if (pass == GC_AXIAL_RAW) {
        // _axial_troughs raw (pre gaussian(sigma=max(trough_width_px*0.18, 0.8))).
        // Rotates the WARPED coords (pool0/pool1) about origin.
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        vec2 r = rotated0(w_x, w_z, STYLE_ANGLE_RAD);
        float rx = r.x;
        float rz = r.y;
        // width = span * (0.030 + 0.010 * clip(trough_width_px/7.0, 0.0, 1.4))
        float width = span * (0.030 + 0.010 * clamp(STYLE_TROUGH_WIDTH_PX / 7.0, 0.0, 1.4));
        float width_div = max(width, 1.0);
        // long_noise = fbm(rx, rz*0.10, 1/(span*0.70), 5, sseed+170, gain=0.55)
        float long_noise = fbm5(rx, rz * 0.10, 1.0 / (span * 0.70), 5, sseed + 170, 0.55);
        // mid_noise = fbm(rx + rz*0.05, rz*0.16, 1/(span*0.34), 4, sseed+180, gain=0.50)
        float mid_noise = fbm5(rx + rz * 0.05, rz * 0.16, 1.0 / (span * 0.34), 4, sseed + 180, 0.50);
        // meander = (0.72*long_noise + 0.28*mid_noise) * span * 0.13
        float meander = (0.72 * long_noise + 0.28 * mid_noise) * span * 0.13;
        // trough = max over offsets {-0.24, 0.0, 0.25} of exp(-(dist*dist)), dist=|rz-center|/max(width,1)
        float trough = 0.0;
        float offs[3] = float[3](-0.24, 0.0, 0.25);
        for (int o = 0; o < 3; ++o) {
            float center = meander + span * offs[o];
            float dist = abs(rz - center) / width_div;
            float g = exp(-(dist * dist));
            if (g > trough) trough = g;
        }
        // gate_raw = fbm(rx, rz, 1/(span*0.52), 4, sseed+190, gain=0.52)
        float gate_raw = fbm5(rx, rz, 1.0 / (span * 0.52), 4, sseed + 190, 0.52);
        // gate = smoothstep(0.28, 0.88, clip(affine(gate_raw, AXIAL_GATE), 0, 1))
        float gate = ss(0.28, 0.88, clip01(affine_remap(gate_raw, AXIAL_GATE_CENTER, AXIAL_GATE_SCALE)));
        // pre = clip(trough * (0.55 + 0.45*gate), 0, 1)  (then gaussian follows)
        pool_write(15, i, clip01(trough * (0.55 + 0.45 * gate)));
        return;
    }

    if (pass == GC_AXIAL) {
        // axial = gaussian(axial_pre, sigma=max(trough_width_px*0.18, 0.8))  [gauss_out]
        pool_write(11, i, gauss_out.v[i]);
        return;
    }

    if (pass == GC_PRIMARY_MASK) {
        // primary = clip(affine(0.58*flow_primary + 1.18*axial, PRIMARY), 0, 1)
        // primary_mask = smoothstep(0.34, 0.84, primary)
        float primary = clip01(affine_remap(0.58 * pool_read(10, i) + 1.18 * pool_read(11, i),
            PRIMARY_CENTER, PRIMARY_SCALE));
        pool_write(12, i, ss(0.34, 0.84, primary));
        return;
    }

    if (pass == GC_BRANCH_SURFACE) {
        // tributary = trough_channels_seam_safe(branch_surface, width=max(trough_width_px*0.48,0.8),
        //   power=0.36, preblur=1.85). branch_surface = base + 0.10*affine(relief, RELIEF_ZSCORE)
        //   - 0.18*gaussian(primary_mask, 1.6)  [gauss_out]. flow_pre <- branch_surface.
        float relief_z = affine_remap(pool_read(5, i), RELIEF_ZSCORE_CENTER, RELIEF_ZSCORE_SCALE);
        flow_pre.v[i] = pool_read(9, i) + 0.10 * relief_z - 0.18 * gauss_out.v[i];
        return;
    }

    if (pass == GC_TRIB_MASK) {
        // tributary_mask = smoothstep(0.54, 0.96, tributary[gauss_out]) * (0.45 + 0.55*relief_envelope)
        pool_write(13, i, ss(0.54, 0.96, gauss_out.v[i]) * (0.45 + 0.55 * pool_read(6, i)));
        return;
    }

    if (pass == GC_SCRAPES) {
        // _striations raw = affine_remap(0.72*long_scrape + 0.28*fine_scrape, STRIATIONS) (NO clip, NO blur).
        // Rotates the WARPED coords (pool0/pool1) about origin.
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        vec2 r = rotated0(w_x, w_z, STYLE_ANGLE_RAD);
        float rx = r.x;
        float rz = r.y;
        // long_scrape = fbm(rx, rz*0.18, 1/(span*0.030), 4, sseed+210, gain=0.48)
        float long_scrape = fbm5(rx, rz * 0.18, 1.0 / (span * 0.030), 4, sseed + 210, 0.48);
        // fine_scrape = fbm(rx + 0.18*rz, rz*0.12, 1/(span*0.014), 3, sseed+220, gain=0.44)
        float fine_scrape = fbm5(rx + 0.18 * rz, rz * 0.12, 1.0 / (span * 0.014), 3, sseed + 220, 0.44);
        float raw = 0.72 * long_scrape + 0.28 * fine_scrape;
        pool_write(14, i, affine_remap(raw, STRIATIONS_CENTER, STRIATIONS_SCALE));
        return;
    }

    if (pass == GC_ASSEMBLE) {
        // ridge_wall = smoothstep(0.48, 0.84, relief_envelope) * (1 - 0.52*primary_mask)
        // trough_floor = clip(0.90*primary_mask + 0.44*tributary_mask, 0, 1)
        // high_ice = clip(icefield * (1 - 0.30*primary_mask), 0, 1)
        float relief_env = pool_read(6, i);
        float primary_mask = pool_read(12, i);
        float trib_mask = pool_read(13, i);
        float icefield = pool_read(7, i);
        float ridge_wall = ss(0.48, 0.84, relief_env) * (1.0 - 0.52 * primary_mask);
        float trough_floor = clip01(0.90 * primary_mask + 0.44 * trib_mask);
        float high_ice = clip01(icefield * (1.0 - 0.30 * primary_mask));

        // height = base.copy()
        // height += ridge_gain * (0.10 + 0.52*ridge_wall) * (0.24*ridge_detail)
        // height += detail_gain * (0.04 + 0.18*ridge_wall) * (0.18*close_detail)
        // height += striation_gain * (0.04 + 0.22*(high_ice + trough_floor)) * (0.18*scrapes)
        // height -= trough_gain * (0.44 + 0.44*high_ice + 0.16*ridge_wall) * primary_mask
        // height -= branch_gain * (0.12 + 0.34*ridge_wall) * tributary_mask
        float hv = pool_read(9, i);
        hv += STYLE_RIDGE_GAIN * (0.10 + 0.52 * ridge_wall) * (0.24 * pool_read(3, i));
        hv += STYLE_DETAIL_GAIN * (0.04 + 0.18 * ridge_wall) * (0.18 * pool_read(4, i));
        hv += STYLE_STRIATION_GAIN * (0.04 + 0.22 * (high_ice + trough_floor)) * (0.18 * pool_read(14, i));
        hv -= STYLE_TROUGH_GAIN * (0.44 + 0.44 * high_ice + 0.16 * ridge_wall) * primary_mask;
        hv -= STYLE_BRANCH_GAIN * (0.12 + 0.34 * ridge_wall) * trib_mask;
        height.v[i] = hv;

        // stash trough_floor / high_ice for the floor/ice masks + blends (REUSE pool10/pool11,
        // dead since GC_PRIMARY_MASK consumed flow_primary/axial).
        pool_write(10, i, trough_floor);
        pool_write(11, i, high_ice);
        return;
    }

    if (pass == GC_FLOOR_MASK) {
        // floor_mask = clip(smoothstep(0.36, 0.80, gaussian(trough_floor, 1.6)[gauss_out]), 0, 1)
        floor_mask.v[i] = clip01(ss(0.36, 0.80, gauss_out.v[i]));
        // ice_mask = clip(smoothstep(0.50, 0.90, high_ice), 0, 1)  (high_ice = pool11, NO blur).
        // Stash into pool7 (icefield dead after GC_BASE / GC_ASSEMBLE high_ice).
        pool_write(7, i, clip01(ss(0.50, 0.90, pool_read(11, i))));
        return;
    }

    if (pass == GC_FLOOR_BLEND) {
        // floor = gaussian(height, max(ice_smooth_px, 0.2)=6.2)  [gauss_out]
        // height = height*(1 - 0.52*floor_mask) + floor*(0.52*floor_mask)
        float fm = floor_mask.v[i];
        height.v[i] = height.v[i] * (1.0 - 0.52 * fm) + gauss_out.v[i] * (0.52 * fm);
        return;
    }

    if (pass == GC_ICE_BLEND) {
        // ice_smooth = gaussian(height, max(ice_smooth_px*0.65, 0.2)=4.03)  [gauss_out]
        // height = height*(1 - 0.28*ice_mask) + ice_smooth*(0.28*ice_mask)
        // height -= 0.16*floor_mask
        float im = pool_read(7, i);   // ice_mask
        height.v[i] = height.v[i] * (1.0 - 0.28 * im) + gauss_out.v[i] * (0.28 * im);
        height.v[i] -= 0.16 * floor_mask.v[i];
        return;
    }

    if (pass == GC_FINAL) {
        // final_blend = 0.66*height + 0.34*gaussian(height, 1.35)[gauss_out]; height = affine(final_blend, FINAL)
        float final_blend = 0.66 * height.v[i] + 0.34 * gauss_out.v[i];
        height.v[i] = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
        return;
    }
}
