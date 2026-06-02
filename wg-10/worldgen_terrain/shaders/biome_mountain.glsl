// WorldGen10 Slice-4b: MOUNTAIN biome FRAGMENT (the seam-safe recipe's biome-specific math).
//
// Concatenated AFTER recipe_primitives.glsl + biome_page.glsl (the MACHINE). It defines the
// mountain-specific constants + helpers + biome_pass(), implementing the BIOME PASS_* values
// the machine forwards to it. The MACHINE owns all bindings, the `Params` push constant `P`,
// the generic leaf helpers (cell_idx / clamp_idx / affine_remap / ss / clip01 / rotated0 /
// recipe_recursive_domain_warp / rmf / fbm5), the global storage buffers, and the generic
// passes (MESHGRID, COPY, GAUSS_*, FLOW_PRE_BASE, FLOW_PRE_PREBLUR_IN, FLOW_PRE_FROM_GAUSS,
// ACC_INIT, FLOW_RELAX, DISCHARGE, CROP). This fragment carries NO #version / #[compute].
//
// This is the GPU mirror of recipes.rs::mountain::generate_seamsafe (the f64 parity ORACLE).
// EVERY constant / seed-offset / weight below is transcribed VERBATIM from recipes.rs::mountain.
// EDIT-BOTH-SIDES: changes here must keep parity with that oracle (proven overall_maxd 1.89e-6).

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
// Mirror of mountain::oriented_ridges_point (seam-safe, rotation centre = world origin).
// Reuses the machine's rotated0 / recipe_recursive_domain_warp / rmf / affine_remap / clip01.
// ---------------------------------------------------------------------------
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
// biome_pass: the mountain-specific PASS bodies (verbatim from biome_page_4a.glsl::main).
// The machine has already handled the generic passes + guards; (cx,cy,i) are the cell coords
// and linear index for this invocation (cx<cols, cy<rows guaranteed by the machine).
// ---------------------------------------------------------------------------
void biome_pass(int pass, int cx, int cy, int i) {
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

    if (pass == PASS_FLOW_PRE_ROUGH) {
        // rough_surface = base + 0.18*affine(ranges, RANGES_ZSCORE_CENTER, RANGES_ZSCORE_SCALE)
        flow_pre.v[i] = base.v[i] + 0.18 * affine_remap(ranges.v[i], RANGES_ZSCORE_CENTER, RANGES_ZSCORE_SCALE);
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
