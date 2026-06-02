// WorldGen10 Slice-4b: RAINFOREST biome FRAGMENT (the seam-safe recipe's biome-specific math).
//
// Concatenated AFTER recipe_primitives.glsl + biome_page.glsl (the MACHINE). It defines the
// rainforest-specific constants + biome_pass(), implementing the BIOME PASS_* values the machine
// forwards to it. The MACHINE owns all bindings, the `Params` push constant `P`, the generic leaf
// helpers (cell_idx / clamp_idx / affine_remap / ss / clip01 / rotated0 /
// recipe_recursive_domain_warp / rmf / fbm5), the global storage buffers (INCLUDING the generic
// scratch POOL pool0..poolN + the pool_read/pool_write switch), and the generic passes
// (MESHGRID, COPY, COPY_POOL, POOL_FROM_GAUSS, GAUSS_*, FLOW_PRE_*, ACC_INIT, FLOW_RELAX,
// DISCHARGE, CROP). This fragment carries NO #version / #[compute].
//
// This is the GPU mirror of recipes_rainforest.rs::generate_seamsafe (the f64 parity ORACLE),
// style = STYLES[0] = humid_dissected_hills (seed_offset=0 -> sseed=seed; the recipe adds seed+N
// directly). EVERY constant / seed-offset / weight / sigma / rotation / scaling below is
// transcribed VERBATIM from recipes_rainforest.rs. EDIT-BOTH-SIDES: changes here must keep parity.
//
// RAINFOREST DUAL-MASK DRAINAGE (the two-spread crux, like temperate): the drainage carve emits
// TWO masks from ONE discharge. On the Rust side this is `flow_discharge(power=0.38, pre-blur 1.15)`
// (the common PREFIX of flow_channels_ex up to + including PASS_DISCHARGE -- leaving the raw log1p
// discharge in gauss_in), NOT a single-spread flow_channels. It then spreads that RAW discharge at
// TWO sigmas: gauss(1.15) -> RF_TRIBUTARIES reads gauss_out (smoothstep 0.42,0.88 -> pool7), then
// gauss(2.2) -> RF_TRUNK reads gauss_out (smoothstep 0.68,0.95 -> pool8). The second gauss(2.2)
// re-reads the SAME intact gauss_in (the generic gaussian only writes gauss_mid/gauss_out, never
// gauss_in), so no pool staging of the raw discharge is needed. drainage = clip(0.68*trib +
// 0.58*trunk). This is EXACTLY temperate's two-spread sequencing -- NO machine extension.
//
// LOCAL PRIMITIVES: rainforest uses ONLY primitives already in recipe_primitives.glsl /
// biome_page.glsl. Its warp is recipe_recursive_domain_warp with steps=4 (the machine helper takes
// steps as a parameter); its ridged fields use ridged_multifractal with the DEFAULT weight_gain=1.35
// (gain=0.52/0.50), so the machine's rmf() covers them -- NO local rmf_wg. fbm via fbm5, rotation
// about the fixed world origin via rotated0. So no local primitives are defined here.
//
// SCRATCH-POOL CONTRACT (rainforest's pool-slot map; needs 12 slots -> POOL_SLOTS 16 covers it):
//   pool0  = w_x          top-level warped X coord (-> macro/hills_raw/ridges/plateau_seed/close)
//   pool1  = w_z          top-level warped Z coord (-> the same warped-coord sub-fields)
//   pool2  = macro_f      macro fBm field (-> lowland(1-macro), flow_source, wet_rounding)
//   pool3  = plateau_seed plateau seed fbm ; then REUSED as plateau (smoothstep blur * (1-0.38*ridges))
//   pool4  = hills_raw    ridged_mf hills (pre gaussian 1.7) ; then REUSED as hills (clip(affine(blur)))
//   pool5  = ridges       smoothstep(0.42,0.83, ridged_mf(rotated)) (-> plateau, flow_source, assemble)
//   pool6  = lowland      smoothstep(gaussian(1 - macro, 5.4)) (-> flow_source, assemble)
//   pool7  = tributaries  smoothstep(0.42,0.88, gaussian(discharge,1.15)) ; then REUSED as drainage
//   pool8  = trunk        smoothstep(0.68,0.95, gaussian(discharge,2.2)) (consumed into drainage)
//   pool9  = close        very-low-freq fbm (-> assemble texture term)
//   pool10 = wet_rounding gaussian(affine(0.62*macro+0.36*hills+0.26*plateau), smoothing_px=2.6)
//   pool11 = TRANSIENT    pre-gaussian staging for wet_rounding_inner (consumed by the next gaussian)
// REUSE: pool3 (plateau_seed, dead once plateau blurred) -> plateau ; pool4 (hills_raw, dead once
// hills blurred) -> hills ; pool7 (tributaries) -> drainage (after combining with trunk in pool8).
// A biome reads/writes a slot with pool_read(slot,i)/pool_write(slot,i,v). To gaussian-blur a slot:
// PASS_COPY_POOL (pool_sel=slot) -> gauss(sigma) -> read gauss_out (or POOL_FROM_GAUSS to stash).
// The fixed named buffers (0..23) are mountain's; rainforest touches only the GENERIC ones
// (wx/wz/flow_pre/gauss_*/height) + the pool.

// ---------------------------------------------------------------------------
// ===== RAINFOREST biome-private PASS_* codes (start at 32 per the interface convention) =====
// MUST match biome_page_compute.rs RF_* consts.
// ---------------------------------------------------------------------------
const int RF_POINTWISE    = 32; // warp -> pool0=w_x,pool1=w_z ; macro=pool2,plateau_seed=pool3,hills_raw=pool4,ridges=pool5
const int RF_HILLS        = 33; // pool4 = hills = clip(affine(gauss_out[=gaussian(hills_raw,1.7)], HILLS))
const int RF_PLATEAU      = 34; // pool3 = plateau = smoothstep(0.54,0.80, gauss_out[=gaussian(plateau_seed,4.5)]) * (1-0.38*ridges)
const int RF_ONE_MINUS_MACRO = 35; // gauss_in <- 1 - macro (pre gaussian(5.4) for lowland)
const int RF_LOWLAND      = 36; // pool6 = lowland = smoothstep(lo_e0,lo_e1, gauss_out[=gaussian(1-macro,5.4)])
const int RF_FLOW_PRE     = 37; // flow_pre <- flow_source = affine(0.66*macro+0.46*hills+0.28*ridges+0.20*plateau-0.36*lowland, FLOW) (NO clip)
const int RF_TRIBUTARIES  = 38; // pool7 = tributaries = smoothstep(0.42,0.88, gauss_out[=gaussian(discharge,1.15)])
const int RF_TRUNK        = 39; // pool8 = trunk = smoothstep(0.68,0.95, gauss_out[=gaussian(discharge,2.2)])
const int RF_DRAINAGE     = 40; // pool7 = drainage = clip(0.68*tributaries + 0.58*trunk) (REUSE pool7)
const int RF_CLOSE        = 41; // pool9 = close = affine(fbm(w_x,w_z, 1/(span*0.030),4,sseed+210,0.45), CLOSE) (NO clip)
const int RF_WET_PRE      = 42; // pool11 = affine(0.62*macro+0.36*hills+0.26*plateau, WET_ROUNDING) (pre gaussian(smoothing_px=2.6))
const int RF_ASSEMBLE     = 43; // height = hills/ridges/plateau - lowland/drainage + close texture; then 0.72*h + 0.28*wet_rounding
const int RF_FINAL        = 44; // height = affine(0.84*height + 0.16*gauss_out[=gaussian(h,1.0)], FINAL)

// ---------------------------------------------------------------------------
// ===== RAINFOREST constants (verbatim from recipes_rainforest.rs) =====
// ---------------------------------------------------------------------------
// affine-remap constants (replace per-window zscore / norm01).
const float MACRO_CENTER         = -0.667;
const float MACRO_SCALE          =  0.717;
const float HILLS_CENTER         =  0.000;
const float HILLS_SCALE          =  1.199;
const float PLATEAU_SEED_CENTER  = -0.847;
const float PLATEAU_SEED_SCALE   =  0.626;
const float FLOW_CENTER          =  0.481;
const float FLOW_SCALE           =  3.059;
const float CLOSE_CENTER         =  0.000;
const float CLOSE_SCALE          =  3.436;
const float WET_ROUNDING_CENTER  =  0.503;
const float WET_ROUNDING_SCALE   =  5.066;
const float HILLS_ZSCORE_CENTER  =  0.386;
const float HILLS_ZSCORE_SCALE   =  3.960;
const float FINAL_CENTER         =  0.000;
const float FINAL_SCALE          =  1.70;

// humid_dissected_hills style (STYLES[0]) fields the seam-safe pipeline reads.
const float STYLE_ANGLE_RAD      = 0.42;
const float STYLE_HILL_GAIN      = 1.18;
const float STYLE_RIDGE_GAIN     = 0.78;
const float STYLE_DRAINAGE_GAIN  = 1.18;
const float STYLE_PLATEAU_GAIN   = 0.36;
const float STYLE_LOWLAND_GAIN   = 0.30;
const float STYLE_TEXTURE_GAIN   = 0.58;
const float STYLE_SMOOTHING_PX   = 2.6;
// seed_offset = 0 (sseed = P.seed + 0; mirrored explicitly).
const int   STYLE_SEED_OFFSET    = 0;

// ---------------------------------------------------------------------------
// biome_pass: the rainforest-specific PASS bodies. The machine has already handled the generic
// passes + guards; (cx,cy,i) are the cell coords and linear index for this invocation (cx<cols,
// cy<rows guaranteed by the machine). All pool access goes through pool_read/pool_write (defined
// in the machine). EDIT-BOTH-SIDES with recipes_rainforest.rs::generate_seamsafe.
// ---------------------------------------------------------------------------
void biome_pass(int pass, int cx, int cy, int i) {
    float span = max(P.feature_span_m, 1.0);
    int sseed = P.seed + STYLE_SEED_OFFSET;

    if (pass == RF_POINTWISE) {
        // w_x, w_z = recursive_domain_warp(wx, wz, span*0.034, 1/(span*0.72), sseed+10, 4, 0.54, 1.74)
        vec2 w = recipe_recursive_domain_warp(
            wx.v[i], wz.v[i],
            span * 0.034, 1.0 / (span * 0.72),
            sseed + 10, 4, 0.54, 1.74);
        float w_x = w.x;
        float w_z = w.y;
        pool_write(0, i, w_x);
        pool_write(1, i, w_z);

        // macro = clip(affine(fbm(w_x,w_z, 1/(span*0.78),5,sseed+30,0.58), MACRO), 0, 1)
        float m = fbm5(w_x, w_z, 1.0 / (span * 0.78), 5, sseed + 30, 0.58);
        pool_write(2, i, clip01(affine_remap(m, MACRO_CENTER, MACRO_SCALE)));

        // plateau_seed = clip(affine(fbm(w_x,w_z, 1/(span*0.44),4,sseed+130,0.55), PLATEAU_SEED), 0, 1)
        float ps = fbm5(w_x, w_z, 1.0 / (span * 0.44), 4, sseed + 130, 0.55);
        pool_write(3, i, clip01(affine_remap(ps, PLATEAU_SEED_CENTER, PLATEAU_SEED_SCALE)));

        // hills_raw = ridged_multifractal(w_x, w_z, 1/(span*0.24), 5, sseed+60, 0.52)
        float hills_raw = rmf(w_x, w_z, 1.0 / (span * 0.24), 5, sseed + 60, 0.52);
        pool_write(4, i, hills_raw);

        // ridges: rotate (w_x,w_z) about the fixed world origin (cx=cz=0), then
        // ridged_multifractal(rx, rz*0.42, 1/(span*0.16), 5, sseed+90, 0.50) -> smoothstep(0.42,0.83)
        vec2 r = rotated0(w_x, w_z, STYLE_ANGLE_RAD);
        float rmf_v = rmf(r.x, r.y * 0.42, 1.0 / (span * 0.16), 5, sseed + 90, 0.50);
        pool_write(5, i, ss(0.42, 0.83, rmf_v));
        return;
    }

    if (pass == RF_HILLS) {
        // hills = clip(affine(gaussian(hills_raw, 1.7), HILLS), 0, 1)  [gauss_out]  -> REUSE pool4
        pool_write(4, i, clip01(affine_remap(gauss_out.v[i], HILLS_CENTER, HILLS_SCALE)));
        return;
    }

    if (pass == RF_PLATEAU) {
        // plateau = smoothstep(0.54,0.80, gaussian(plateau_seed,4.5)) * (1 - 0.38*ridges)  -> REUSE pool3
        float ridges = pool_read(5, i);
        pool_write(3, i, ss(0.54, 0.80, gauss_out.v[i]) * (1.0 - 0.38 * ridges));
        return;
    }

    if (pass == RF_ONE_MINUS_MACRO) {
        // gauss_in <- 1 - macro  (pre gaussian(5.4) for lowland)
        gauss_in.v[i] = 1.0 - pool_read(2, i);
        return;
    }

    if (pass == RF_LOWLAND) {
        // lowland = smoothstep(0.57 - 0.10*lg, 0.88 - 0.06*lg, gaussian(1 - macro, 5.4))  [gauss_out]
        float lo_e0 = 0.57 - 0.10 * STYLE_LOWLAND_GAIN;
        float lo_e1 = 0.88 - 0.06 * STYLE_LOWLAND_GAIN;
        pool_write(6, i, ss(lo_e0, lo_e1, gauss_out.v[i]));
        return;
    }

    if (pass == RF_FLOW_PRE) {
        // flow_source = affine(0.66*macro + 0.46*hills + 0.28*ridges + 0.20*plateau - 0.36*lowland, FLOW) (NO clip)
        // -> flow_pre ; the generic flow machinery (pre-blur 1.15 + relax + discharge) runs next via
        // flow_discharge(0.38, 1.15), leaving the raw log1p discharge in gauss_in for the two spreads.
        float inner = 0.66 * pool_read(2, i)
                    + 0.46 * pool_read(4, i)
                    + 0.28 * pool_read(5, i)
                    + 0.20 * pool_read(3, i)
                    - 0.36 * pool_read(6, i);
        flow_pre.v[i] = affine_remap(inner, FLOW_CENTER, FLOW_SCALE);
        return;
    }

    if (pass == RF_TRIBUTARIES) {
        // tributaries = smoothstep(0.42, 0.88, gaussian(discharge, 1.15))  [gauss_out]
        pool_write(7, i, ss(0.42, 0.88, gauss_out.v[i]));
        return;
    }

    if (pass == RF_TRUNK) {
        // trunk = smoothstep(0.68, 0.95, gaussian(discharge, 2.2))  [gauss_out]
        pool_write(8, i, ss(0.68, 0.95, gauss_out.v[i]));
        return;
    }

    if (pass == RF_DRAINAGE) {
        // drainage = clip(0.68*tributaries + 0.58*trunk, 0, 1)  -> REUSE pool7
        float drainage = clip01(0.68 * pool_read(7, i) + 0.58 * pool_read(8, i));
        pool_write(7, i, drainage);
        return;
    }

    if (pass == RF_CLOSE) {
        // close = affine(fbm(w_x,w_z, 1/(span*0.030),4,sseed+210,0.45), CLOSE) (NO clip)
        float cf = fbm5(pool_read(0, i), pool_read(1, i), 1.0 / (span * 0.030), 4, sseed + 210, 0.45);
        pool_write(9, i, affine_remap(cf, CLOSE_CENTER, CLOSE_SCALE));
        return;
    }

    if (pass == RF_WET_PRE) {
        // wet_inner = affine(0.62*macro + 0.36*hills + 0.26*plateau, WET_ROUNDING) (pre gaussian(smoothing_px=2.6))
        float inner = 0.62 * pool_read(2, i) + 0.36 * pool_read(4, i) + 0.26 * pool_read(3, i);
        pool_write(11, i, affine_remap(inner, WET_ROUNDING_CENTER, WET_ROUNDING_SCALE));
        return;
    }

    if (pass == RF_ASSEMBLE) {
        // hv  = 0.46 * hill_gain * affine_remap(hills, HILLS_ZSCORE)
        // hv += 0.34 * ridge_gain * ridges
        // hv += 0.30 * plateau_gain * plateau
        // hv -= 0.38 * lowland_gain * lowland
        // hv -= 0.34 * drainage_gain * drainage
        // hv += texture_gain * (0.055*close + 0.045*close*ridges)
        // height = 0.72 * hv + 0.28 * wet_rounding
        float hills = pool_read(4, i);
        float ridges = pool_read(5, i);
        float plateau = pool_read(3, i);
        float lowland = pool_read(6, i);
        float drainage = pool_read(7, i);
        float close = pool_read(9, i);
        float wet_rounding = pool_read(10, i);

        float hv = 0.46 * STYLE_HILL_GAIN * affine_remap(hills, HILLS_ZSCORE_CENTER, HILLS_ZSCORE_SCALE);
        hv += 0.34 * STYLE_RIDGE_GAIN * ridges;
        hv += 0.30 * STYLE_PLATEAU_GAIN * plateau;
        hv -= 0.38 * STYLE_LOWLAND_GAIN * lowland;
        hv -= 0.34 * STYLE_DRAINAGE_GAIN * drainage;
        hv += STYLE_TEXTURE_GAIN * (0.055 * close + 0.045 * close * ridges);
        height.v[i] = 0.72 * hv + 0.28 * wet_rounding;
        return;
    }

    if (pass == RF_FINAL) {
        // final_blend = 0.84*height + 0.16*gaussian(height, 1.0)[gauss_out]
        // height = affine_remap(final_blend, FINAL)
        float final_blend = 0.84 * height.v[i] + 0.16 * gauss_out.v[i];
        height.v[i] = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
        return;
    }
}
