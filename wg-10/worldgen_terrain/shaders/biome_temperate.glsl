// WorldGen10 Slice-4b: TEMPERATE biome FRAGMENT (the seam-safe recipe's biome-specific math).
//
// Concatenated AFTER recipe_primitives.glsl + biome_page.glsl (the MACHINE). It defines the
// temperate-specific constants + helpers + biome_pass(), implementing the BIOME PASS_* values the
// machine forwards to it. The MACHINE owns all bindings, the `Params` push constant `P`, the
// generic leaf helpers (cell_idx / clamp_idx / affine_remap / ss / clip01 / rotated0 /
// recipe_recursive_domain_warp / rmf / fbm5), the global storage buffers (INCLUDING the generic
// scratch POOL pool0..poolN + the pool_read/pool_write switch), and the generic passes
// (MESHGRID, COPY, COPY_POOL, POOL_FROM_GAUSS, GAUSS_*, FLOW_PRE_*, ACC_INIT, FLOW_RELAX,
// DISCHARGE, CROP). This fragment carries NO #version / #[compute].
//
// This is the GPU mirror of recipes_temperate.rs::generate_seamsafe (the f64 parity ORACLE),
// style = STYLES[0] = appalachian_ridges (seed_offset=0 -> sseed=seed; the recipe adds seed+N
// directly). EVERY constant / seed-offset / weight / sigma / rotation / scaling below is
// transcribed VERBATIM from recipes_temperate.rs. EDIT-BOTH-SIDES: changes here must keep parity.
//
// TEMPERATE RAW-DISCHARGE FLOW (the machine-extension the whole port hangs on): temperate's
// valley drainage uses the SHARED pre-blur 1.15 + MFD accumulation (power=0.43) + log1p fixed-max
// normalize, then STOPS -- it does NOT trail a single spread blur, because temperate spreads the
// RAW discharge at TWO different sigmas (1.8 for `valleys`, 4.2 for `broad_valleys`). On the Rust
// side this is the new flow_discharge(0.43, 1.15) (the common prefix of flow_channels_ex, up to
// and including PASS_DISCHARGE, which leaves the raw log1p discharge in gauss_in), followed by two
// independent spreads. CRUCIAL buffer flow: PASS_DISCHARGE writes the raw discharge into gauss_in;
// the generic gaussian (gauss(sigma)) reads gauss_in (AXIS0 -> gauss_mid) then gauss_mid (AXIS1 ->
// gauss_out) and NEVER modifies gauss_in. So after flow_discharge the schedule calls gauss(1.8)
// (TE_VALLEYS reads the spread from gauss_out -> pool9), then gauss(4.2) (which re-reads the SAME
// intact gauss_in; TE_BROAD_VALLEYS reads gauss_out -> pool10). No pool staging of the raw
// discharge is needed because gauss leaves gauss_in untouched.
//
// LOCAL PRIMITIVES: temperate uses ONLY primitives already in recipe_primitives.glsl/biome_page.glsl
// (recursive_domain_warp via recipe_recursive_domain_warp, fbm via fbm5, ridged_multifractal via
// rmf with the DEFAULT weight_gain=1.35, rotation via rotated0). It needs NO cellular_edges /
// range_spine_field / fault_block_field, and NO custom-weight_gain rmf_wg. So no local primitives
// are defined here.
//
// SCRATCH-POOL CONTRACT (temperate's pool-slot map; needs 12 slots -> POOL_SLOTS 16 covers it):
//   pool0  = w_x          top-level warped X coord (-> macro/folded/hills_raw/fine sub-fields)
//   pool1  = w_z          top-level warped Z coord (-> the same warped-coord sub-fields)
//   pool2  = macro_f      macro fBm field (-> upland blur input, flow_source, rounded, NOT assemble)
//   pool3  = folded_remap clip(affine(folded ridged_mf)) (-> ridges blur input)
//   pool4  = hills_raw    ridged_mf hills (-> hills blur input)
//   pool5  = fine         fine fbm detail (-> assemble)
//   pool6  = ridges       smoothstep(gaussian(folded_remap,1.1)) (-> flow_source, assemble)
//   pool7  = hills        clip(affine(gaussian(hills_raw,2.4))) (-> flow_source, rounded, assemble)
//   pool8  = upland       smoothstep(gaussian(macro,4.2)) (-> flow_source, assemble)
//   pool9  = valleys      smoothstep(gaussian(discharge,1.8)) (-> assemble)
//   pool10 = broad_valleys smoothstep(gaussian(discharge,4.2)) (-> assemble)
//   pool11 = rounded      gaussian(affine(0.52*macro+0.48*hills), smoothing_px=1.8) (-> assemble)
// The flow source (flow_source, NO clip) goes straight into the GENERIC flow_pre buffer (like
// karst's KS_DV_SURFACE -> flow_pre); the raw discharge stays in gauss_in after flow_discharge.
// A biome reads/writes a slot with pool_read(slot,i)/pool_write(slot,i,v). To gaussian-blur a slot:
// PASS_COPY_POOL (pool_sel=slot) -> gauss(sigma) -> read gauss_out. The fixed named buffers (0..23)
// are mountain's; temperate touches only the GENERIC ones (wx/wz/flow_pre/gauss_*/height) + the pool.

// ---------------------------------------------------------------------------
// ===== TEMPERATE biome-private PASS_* codes (start at 32 per the interface convention) =====
// MUST match biome_page_compute.rs TE_* consts.
// ---------------------------------------------------------------------------
const int TE_POINTWISE      = 32; // warp -> pool0=w_x,pool1=w_z ; macro=pool2,folded_remap=pool3,hills_raw=pool4,fine=pool5
const int TE_RIDGES         = 33; // pool6 = ridges = smoothstep(0.40,0.82, gauss_out[=gaussian(folded_remap,1.1)])
const int TE_HILLS          = 34; // pool7 = hills = clip(affine(gauss_out[=gaussian(hills_raw,2.4)], HILLS))
const int TE_UPLAND         = 35; // pool8 = upland = smoothstep(0.50,0.82, gauss_out[=gaussian(macro,4.2)])
const int TE_FLOW_PRE       = 36; // flow_pre <- flow_source = affine(0.72*macro+0.32*ridges+0.28*hills+0.26*upland, FLOW_SRC) (NO clip)
const int TE_VALLEYS        = 37; // pool9 = valleys = smoothstep(VALLEY, gauss_out[=gaussian(discharge,1.8)])
const int TE_BROAD_VALLEYS  = 38; // pool10 = broad_valleys = smoothstep(BROAD, gauss_out[=gaussian(discharge,4.2)])
const int TE_ROUNDED_PRE    = 39; // pool11 = affine(0.52*macro + 0.48*hills, ROUNDED) (pre gaussian(smoothing_px=1.8))
const int TE_ASSEMBLE       = 40; // height = hills/ridges/upland - valleys/broad + fine; then 0.76*h + 0.24*rounded
const int TE_FINAL          = 41; // height = affine(0.85*height + 0.15*gauss_out[=gaussian(h,1.0)], FINAL)

// ---------------------------------------------------------------------------
// ===== TEMPERATE constants (verbatim from recipes_temperate.rs) =====
// ---------------------------------------------------------------------------
// affine-remap constants (replace per-window zscore / norm01)
const float MACRO_CENTER    = -0.428;
const float MACRO_SCALE     =  1.061;
const float FOLDED_CENTER   =  0.004;
const float FOLDED_SCALE    =  1.085;
const float HILLS_CENTER    =  0.008;
const float HILLS_SCALE     =  1.339;
const float FLOW_SRC_CENTER =  0.583;
const float FLOW_SRC_SCALE  =  3.895;
const float FINE_CENTER     =  0.000;
const float FINE_SCALE      =  3.436;
const float ROUNDED_CENTER  =  0.458;
const float ROUNDED_SCALE   =  6.390;
const float FINAL_CENTER    =  0.079;
const float FINAL_SCALE     =  1.995;

// MFD valley channel thresholds (seam-safe path).
const float VALLEY_THRESH_LO        = 0.24;
const float VALLEY_THRESH_HI        = 0.40;
const float BROAD_VALLEY_THRESH_LO  = 0.20;
const float BROAD_VALLEY_THRESH_HI  = 0.36;

// appalachian_ridges style (STYLES[0]) fields the seam-safe pipeline reads.
const float STYLE_ANGLE_RAD     = 0.78;
const float STYLE_RIDGE_GAIN    = 1.55;
const float STYLE_HILL_GAIN     = 0.72;
const float STYLE_VALLEY_GAIN   = 1.12;
const float STYLE_UPLAND_GAIN   = 0.62;
const float STYLE_SMOOTHING_PX  = 1.8;
const float STYLE_TEXTURE_GAIN  = 0.58;
// seed_offset = 0 (sseed = P.seed + 0; mirrored explicitly).
const int   STYLE_SEED_OFFSET   = 0;

// ---------------------------------------------------------------------------
// biome_pass: the temperate-specific PASS bodies. The machine has already handled the generic
// passes + guards; (cx,cy,i) are the cell coords and linear index for this invocation (cx<cols,
// cy<rows guaranteed by the machine). All pool access goes through pool_read/pool_write (defined
// in the machine). EDIT-BOTH-SIDES with recipes_temperate.rs::generate_seamsafe.
// ---------------------------------------------------------------------------
void biome_pass(int pass, int cx, int cy, int i) {
    float span = max(P.feature_span_m, 1.0);
    int sseed = P.seed + STYLE_SEED_OFFSET;

    if (pass == TE_POINTWISE) {
        // w_x, w_z = recursive_domain_warp(wx, wz, span*0.030, 1/(span*0.76), sseed+10, 3, 0.55, 1.72)
        vec2 w = recipe_recursive_domain_warp(
            wx.v[i], wz.v[i],
            span * 0.030, 1.0 / (span * 0.76),
            sseed + 10, 3, 0.55, 1.72);
        float w_x = w.x;
        float w_z = w.y;
        pool_write(0, i, w_x);
        pool_write(1, i, w_z);

        // macro = clip(affine(fbm(w_x,w_z, 1/(span*0.84),5,sseed+30,0.58), MACRO), 0, 1)
        float m = fbm5(w_x, w_z, 1.0 / (span * 0.84), 5, sseed + 30, 0.58);
        pool_write(2, i, clip01(affine_remap(m, MACRO_CENTER, MACRO_SCALE)));

        // folded = ridged_multifractal(rx, rz*0.22, 1/(span*0.13), 5, sseed+60, 0.54) on coords
        // rotated about the fixed world origin (cx=cz=0); folded_remap = clip(affine(folded, FOLDED))
        vec2 r = rotated0(w_x, w_z, STYLE_ANGLE_RAD);
        float folded = rmf(r.x, r.y * 0.22, 1.0 / (span * 0.13), 5, sseed + 60, 0.54);
        pool_write(3, i, clip01(affine_remap(folded, FOLDED_CENTER, FOLDED_SCALE)));

        // hills_raw = ridged_multifractal(w_x, w_z, 1/(span*0.28), 5, sseed+90, 0.52)
        float hills_raw = rmf(w_x, w_z, 1.0 / (span * 0.28), 5, sseed + 90, 0.52);
        pool_write(4, i, hills_raw);

        // fine = affine(fbm(w_x,w_z, 1/(span*0.035), 4, sseed+150, 0.45), FINE)
        float fg = fbm5(w_x, w_z, 1.0 / (span * 0.035), 4, sseed + 150, 0.45);
        pool_write(5, i, affine_remap(fg, FINE_CENTER, FINE_SCALE));
        return;
    }

    if (pass == TE_RIDGES) {
        // ridges = smoothstep(0.40, 0.82, gaussian(folded_remap, 1.1))  [gauss_out]
        pool_write(6, i, ss(0.40, 0.82, gauss_out.v[i]));
        return;
    }

    if (pass == TE_HILLS) {
        // hills = clip(affine(gaussian(hills_raw, 2.4), HILLS), 0, 1)  [gauss_out]
        pool_write(7, i, clip01(affine_remap(gauss_out.v[i], HILLS_CENTER, HILLS_SCALE)));
        return;
    }

    if (pass == TE_UPLAND) {
        // upland = smoothstep(0.50, 0.82, gaussian(macro, 4.2))  [gauss_out]
        pool_write(8, i, ss(0.50, 0.82, gauss_out.v[i]));
        return;
    }

    if (pass == TE_FLOW_PRE) {
        // flow_source = affine(0.72*macro + 0.32*ridges + 0.28*hills + 0.26*upland, FLOW_SRC) (NO clip)
        // -> flow_pre ; the generic flow machinery (pre-blur 1.15 + relax + discharge) runs next via
        // flow_discharge(0.43, 1.15), leaving the raw log1p discharge in gauss_in for the two spreads.
        float inner = 0.72 * pool_read(2, i)
                    + 0.32 * pool_read(6, i)
                    + 0.28 * pool_read(7, i)
                    + 0.26 * pool_read(8, i);
        flow_pre.v[i] = affine_remap(inner, FLOW_SRC_CENTER, FLOW_SRC_SCALE);
        return;
    }

    if (pass == TE_VALLEYS) {
        // valleys = smoothstep(VALLEY_LO, VALLEY_HI, gaussian(discharge, 1.8))  [gauss_out]
        pool_write(9, i, ss(VALLEY_THRESH_LO, VALLEY_THRESH_HI, gauss_out.v[i]));
        return;
    }

    if (pass == TE_BROAD_VALLEYS) {
        // broad_valleys = smoothstep(BROAD_LO, BROAD_HI, gaussian(discharge, 4.2))  [gauss_out]
        pool_write(10, i, ss(BROAD_VALLEY_THRESH_LO, BROAD_VALLEY_THRESH_HI, gauss_out.v[i]));
        return;
    }

    if (pass == TE_ROUNDED_PRE) {
        // rounded_inner = affine(0.52*macro + 0.48*hills, ROUNDED) (pre gaussian(max(smoothing_px,0.2)=1.8))
        float inner = 0.52 * pool_read(2, i) + 0.48 * pool_read(7, i);
        pool_write(11, i, affine_remap(inner, ROUNDED_CENTER, ROUNDED_SCALE));
        return;
    }

    if (pass == TE_ASSEMBLE) {
        // hv  = 0.42 * hill_gain * affine_remap(hills, 0.5, 2.0)
        // hv += 0.42 * ridge_gain * ridges
        // hv += 0.30 * upland_gain * upland
        // hv -= 0.30 * valley_gain * valleys
        // hv -= 0.16 * valley_gain * broad_valleys
        // hv += 0.060 * texture_gain * fine * (0.45 + 0.55 * ridges)
        // height = 0.76 * hv + 0.24 * rounded
        float hills = pool_read(7, i);
        float ridges = pool_read(6, i);
        float upland = pool_read(8, i);
        float valleys = pool_read(9, i);
        float broad_valleys = pool_read(10, i);
        float fine = pool_read(5, i);
        float rounded = pool_read(11, i);

        float hv = 0.42 * STYLE_HILL_GAIN * affine_remap(hills, 0.5, 2.0);
        hv += 0.42 * STYLE_RIDGE_GAIN * ridges;
        hv += 0.30 * STYLE_UPLAND_GAIN * upland;
        hv -= 0.30 * STYLE_VALLEY_GAIN * valleys;
        hv -= 0.16 * STYLE_VALLEY_GAIN * broad_valleys;
        hv += 0.060 * STYLE_TEXTURE_GAIN * fine * (0.45 + 0.55 * ridges);
        height.v[i] = 0.76 * hv + 0.24 * rounded;
        return;
    }

    if (pass == TE_FINAL) {
        // final_blend = 0.85*height + 0.15*gaussian(height, 1.0)[gauss_out]
        // height = affine_remap(final_blend, FINAL)
        float final_blend = 0.85 * height.v[i] + 0.15 * gauss_out.v[i];
        height.v[i] = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
        return;
    }
}
