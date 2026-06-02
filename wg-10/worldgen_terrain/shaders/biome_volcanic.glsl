// WorldGen10 Slice-4b: VOLCANIC biome FRAGMENT (the seam-safe recipe's biome-specific math).
//
// Concatenated AFTER recipe_primitives.glsl + biome_page.glsl (the MACHINE). It defines the
// volcanic-specific constants + biome_pass(), implementing the BIOME PASS_* values the machine
// forwards to it. The MACHINE owns all bindings, the `Params` push constant `P`, the generic leaf
// helpers (cell_idx / clamp_idx / affine_remap / ss / clip01 / rotated0 /
// recipe_recursive_domain_warp / rmf / fbm5), the global storage buffers (INCLUDING the generic
// scratch POOL pool0..poolN + pool_read/pool_write AND the VENT buffer + VENT_STRIDE/MAX_VENTS),
// and the generic passes (MESHGRID, COPY, COPY_POOL, POOL_FROM_GAUSS, GAUSS_*, FLOW_PRE_*,
// ACC_INIT, FLOW_RELAX, DISCHARGE, CROP). This fragment carries NO #version / #[compute].
//
// This is the GPU mirror of recipes_volcanic.rs::generate_seamsafe (the f64 parity ORACLE),
// style = STYLES[0] = stratovolcano_cluster (seed_offset=0 -> sseed=seed; the recipe adds seed+N
// directly). EVERY constant / seed-offset / weight / sigma / rotation / scaling below is
// transcribed VERBATIM from recipes_volcanic.rs. EDIT-BOTH-SIDES: changes here must keep parity.
//
// ===== THE KEY INSIGHT (the most novel port): RNG stays in RUST, the GPU consumes a vent buffer.
// VOLCANIC places its vents with numpy PCG64 RNG (vent positions from default_rng(sseed+offset+500),
// per-vent flow directions from a SECOND stream default_rng(sseed+offset+900)). That RNG is
// parity-exact in recipes_volcanic.rs (the npy_random module) and is run ON THE CPU
// (recipes_volcanic::packed_vents) BEFORE the compute list opens. The CPU packs a SMALL fixed
// buffer -- (vx, vz, amp, dir0, dir1, dir2, dir3) = VENT_STRIDE(7) floats per vent, padded to
// MAX_VENTS(8), with the active count in the `P.vent_count` push constant -- and uploads it at
// binding 40. The GPU's VO_VENT_ACCUM pass below loops [0, vent_count) doing PURE f32 cone/crater/
// shield/flow math. THERE IS NO RNG IN THIS SHADER. The vents are drawn about the FIXED world
// origin (0,0) with the caller's fixed feature_span_m, so the vent set is window-independent
// (seam-safe): every adjacent page uploads the identical buffer.
//
// LOCAL PRIMITIVES: volcanic uses ONLY primitives already in recipe_primitives.glsl /
// biome_page.glsl, plus a tiny local `angle_delta` (atan2(sin,cos)) used by the vent flow lobes.
// Its warp is recipe_recursive_domain_warp with steps=3; its ridged fields use ridged_multifractal
// with the DEFAULT weight_gain=1.35 (gains 0.52/0.48), so the machine's rmf() covers them -- NO
// local rmf_wg. fbm via fbm5, rotation about the fixed world origin via rotated0.
//
// SCRATCH-POOL CONTRACT (volcanic's pool-slot map; needs 16 slots -> POOL_SLOTS 16 covers it):
//   pool0  = w_x          top-level warped X coord
//   pool1  = w_z          top-level warped Z coord
//   pool2  = regional     clip(affine(fbm)) macro field (-> base)
//   pool3  = rift         clip(smoothstep(ridged_mf(rotated))*rift_gain) (-> base, assemble)
//   pool4  = cones        RAW vent cones ; then REUSED in place as clip(affine(cones, CONES))
//   pool5  = craters      RAW vent craters ; then REUSED as clip(affine(craters, CRATERS))
//   pool6  = shields      RAW vent shields ; then REUSED as clip(affine(shields, SHIELDS))
//   pool7  = flows        clip(affine(gaussian(raw flows,1.1), FLOWS))
//   pool8  = lava_texture affine(fbm) (NO clip)
//   pool9  = rough_aa     affine(ridged_mf) (NO clip)
//   pool10 = base         affine(0.58*regional + 0.52*shields*shield_gain + 0.22*rift, BASE)
//   pool11 = gullies      smoothstep(0.52,0.92, gully_discharge) * (0.30 + 0.70*cones)
//   pool12 = caldera_bowl craters * smoothstep(0.52,0.88, gaussian(shields+cones, 2.6))
//   pool13 = caldera_rim  smoothstep(0.38,0.78,cones) * (1 - smoothstep(0.25,0.72,craters))
//   pool14 = cone_lift    cones * (1 - 0.88*smoothstep(0.12,0.78,craters))
//   pool15 = TRANSIENT    raw vent flows (pre gaussian(1.1)) ; then REUSED for max_cf_blur
//                         (gaussian(max(cones,flows), 3.0) for the ash_plain blend)
// The fixed named buffers (0..23) are mountain's; volcanic touches only the GENERIC ones
// (wx/wz/flow_pre/gauss_*/height) + the pool + the vent buffer.

// ---------------------------------------------------------------------------
// ===== VOLCANIC biome-private PASS_* codes (start at 32 per the interface convention) =====
// MUST match biome_page_compute.rs VO_* consts.
// ---------------------------------------------------------------------------
const int VO_POINTWISE   = 32; // warp -> pool0=w_x,pool1=w_z ; regional=pool2 ; rift=pool3
const int VO_VENT_ACCUM  = 33; // loop vent buffer -> RAW cones(pool4)/craters(pool5)/shields(pool6) ; raw flows -> pool15
const int VO_FLOWS_FINAL = 34; // pool7 = clip(affine(gauss_out[=gaussian(raw flows,1.1)], FLOWS))
const int VO_REMAP       = 35; // pool4/5/6 = clip(affine(raw cones/craters/shields, CONES/CRATERS/SHIELDS)) in place
const int VO_LAVA_ROUGH  = 36; // pool8 = lava_texture = affine(fbm,LAVA) ; pool9 = rough_aa = affine(ridged_mf,ROUGH) (NO clip)
const int VO_BASE        = 37; // pool10 = base = affine(0.58*regional + 0.52*shields*shield_gain + 0.22*rift, BASE)
const int VO_RADIAL      = 38; // flow_pre <- radial_surface = base + 1.12*cones - 0.78*craters (NO clip)
const int VO_GULLIES     = 39; // pool11 = smoothstep(0.52,0.92, clip(gauss_out)) * (0.30 + 0.70*cones)
const int VO_SPC_PRE     = 40; // gauss_in <- shields + cones (pre gaussian(2.6))
const int VO_CALDERA     = 41; // pool12=caldera_bowl ; pool13=caldera_rim ; pool14=cone_lift
const int VO_ASSEMBLE    = 42; // height = base + cone/shield/rift/flow/caldera_rim - caldera_bowl/gully + detail
const int VO_ASH_PRE     = 43; // gauss_in <- max(cones, flows) (pre gaussian(3.0))
const int VO_ASH_BLEND   = 44; // ash_plain = smoothstep(0.52,0.86, 1 - pool15[max_cf_blur]) ; blend height with gauss_out[smoothed_plain]
const int VO_FINAL       = 45; // height = affine(0.82*height + 0.18*gauss_out[=gaussian(h,0.85)], FINAL)

// ---------------------------------------------------------------------------
// ===== VOLCANIC constants (verbatim from recipes_volcanic.rs) =====
// ---------------------------------------------------------------------------
// affine-remap constants (replace per-window zscore / norm01).
const float REGIONAL_CENTER     = -0.492;
const float REGIONAL_SCALE      =  1.004;
const float CONES_CENTER        =  0.003;
const float CONES_SCALE         =  0.712;
const float CRATERS_CENTER      =  0.000;
const float CRATERS_SCALE       =  0.898;
const float SHIELDS_CENTER      =  0.010;
const float SHIELDS_SCALE       =  0.434;
const float FLOWS_CENTER        =  0.003;
const float FLOWS_SCALE         =  1.459;
const float BASE_CENTER         =  0.459;
const float BASE_SCALE          =  5.30;
const float LAVA_TEXTURE_CENTER = -0.002;
const float LAVA_TEXTURE_SCALE  =  3.63;
const float ROUGH_AA_CENTER     =  0.335;
const float ROUGH_AA_SCALE      =  4.47;
const float FINAL_CENTER        =  0.376;
const float FINAL_SCALE         =  0.82;

// stratovolcano_cluster style (STYLES[0]) fields the seam-safe pipeline reads.
const float STYLE_ANGLE_RAD     = 0.35;
const float STYLE_CONE_GAIN     = 1.28;
const float STYLE_SHIELD_GAIN   = 0.62;
const float STYLE_CALDERA_GAIN  = 0.72;
const float STYLE_FLOW_GAIN     = 0.78;
const float STYLE_RIFT_GAIN     = 0.34;
const float STYLE_GULLY_GAIN    = 1.12;
const float STYLE_CONE_WIDTH_M  = 6700.0;
const float STYLE_CRATER_WIDTH_M= 1500.0;
const float STYLE_FLOW_LENGTH_M = 27000.0;
const float STYLE_DETAIL_GAIN   = 0.58;
// seed_offset = 0 (sseed = P.seed + 0; mirrored explicitly).
const int   STYLE_SEED_OFFSET   = 0;

// ---------------------------------------------------------------------------
// _angle_delta(a, b) = atan2(sin(a-b), cos(a-b)). VOLCANIC-LOCAL (the only volcanic-private
// primitive). Used by the vent flow lobes.
// ---------------------------------------------------------------------------
float vol_angle_delta(float a, float b) {
    return atan(sin(a - b), cos(a - b));
}

// ---------------------------------------------------------------------------
// biome_pass: the volcanic-specific PASS bodies. The machine has already handled the generic
// passes + guards; (cx,cy,i) are the cell coords and linear index for this invocation (cx<cols,
// cy<rows guaranteed by the machine). All pool access goes through pool_read/pool_write; the vent
// buffer is read via vents.v[...]. EDIT-BOTH-SIDES with recipes_volcanic.rs::generate_seamsafe.
// ---------------------------------------------------------------------------
void biome_pass(int pass, int cx, int cy, int i) {
    float span = max(P.feature_span_m, 1.0);
    int sseed = P.seed + STYLE_SEED_OFFSET;

    if (pass == VO_POINTWISE) {
        // w_x, w_z = recursive_domain_warp(wx, wz, span*0.026, 1/(span*0.72), sseed+10, 3, 0.52, 1.82)
        vec2 w = recipe_recursive_domain_warp(
            wx.v[i], wz.v[i],
            span * 0.026, 1.0 / (span * 0.72),
            sseed + 10, 3, 0.52, 1.82);
        float w_x = w.x;
        float w_z = w.y;
        pool_write(0, i, w_x);
        pool_write(1, i, w_z);

        // regional = clip(affine(fbm(w_x,w_z, 1/(span*0.84),5,sseed+30,0.56), REGIONAL), 0, 1)
        float reg = fbm5(w_x, w_z, 1.0 / (span * 0.84), 5, sseed + 30, 0.56);
        pool_write(2, i, clip01(affine_remap(reg, REGIONAL_CENTER, REGIONAL_SCALE)));

        // rift: (rx,rz)=rotated0(w_x,w_z,angle); rift_raw=ridged_mf(rx, rz*0.22, 1/(span*0.16),4,sseed+80,0.52)
        // rift = clip(smoothstep(0.40,0.88,rift_raw) * rift_gain, 0, 1)
        vec2 r = rotated0(w_x, w_z, STYLE_ANGLE_RAD);
        float rift_raw = rmf(r.x, r.y * 0.22, 1.0 / (span * 0.16), 4, sseed + 80, 0.52);
        pool_write(3, i, clip01(ss(0.40, 0.88, rift_raw) * STYLE_RIFT_GAIN));
        return;
    }

    if (pass == VO_VENT_ACCUM) {
        // PURE f32 vent accumulation (RNG already done in Rust; the vent list + 4 flow dirs per vent
        // come from the uploaded vent buffer). Widths are pure functions of the style consts.
        float cone_w   = max(STYLE_CONE_WIDTH_M, 1.0);
        float shield_w = max(STYLE_CONE_WIDTH_M * 2.65, 1.0);
        float crater_w = max(STYLE_CRATER_WIDTH_M, 1.0);
        float rim_w    = max(STYLE_CRATER_WIDTH_M * 0.34, 1.0);
        float rim_center = STYLE_CRATER_WIDTH_M * 1.55;
        float flow_len = max(STYLE_FLOW_LENGTH_M, 1.0);
        float ds_e0    = STYLE_CRATER_WIDTH_M * 1.8;
        float ds_e1    = STYLE_CONE_WIDTH_M * 1.4;

        float wxi = pool_read(0, i);
        float wzi = pool_read(1, i);

        float cones = 0.0;
        float craters = 0.0;
        float shields = 0.0;
        float flows = 0.0;
        // (vents field = max(.., crater) is computed in the recipe but UNUSED by generate_seamsafe.)

        for (int vi = 0; vi < P.vent_count; ++vi) {
            int b = vi * VENT_STRIDE;
            float vx = vents.v[b];
            float vz = vents.v[b + 1];
            float amp = vents.v[b + 2];

            float dx = wxi - vx;
            float dz = wzi - vz;
            float rr = sqrt(dx * dx + dz * dz);

            float cone   = exp(-rr / cone_w);
            float shield = exp(-((rr / shield_w) * (rr / shield_w)));
            float crater = exp(-((rr / crater_w) * (rr / crater_w)));
            float rimd   = (rr - rim_center) / rim_w;
            float rim    = exp(-(rimd * rimd));

            cones   += amp * cone;
            shields += amp * shield;
            craters += amp * crater;
            cones   += 0.18 * amp * rim;

            float angle = atan(dz, dx);
            float downstream = ss(ds_e0, ds_e1, rr);
            float radial = exp(-rr / flow_len);
            float local_flow = 0.0;
            // 4 flow directions for this vent (packed dir0..dir3).
            for (int k = 0; k < 4; ++k) {
                float dir = vents.v[b + 3 + k];
                float ad = vol_angle_delta(angle, dir) / 0.25;
                float angular = exp(-(ad * ad));
                float lobe = angular * radial * downstream;
                local_flow = max(local_flow, lobe);
            }
            flows = max(flows, amp * local_flow);
        }

        pool_write(4, i, cones);    // RAW cones
        pool_write(5, i, craters);  // RAW craters
        pool_write(6, i, shields);  // RAW shields
        pool_write(15, i, flows);   // RAW flows (transient; blurred at sigma 1.1 next)
        return;
    }

    if (pass == VO_FLOWS_FINAL) {
        // flows = clip(affine(gaussian(raw flows, 1.1), FLOWS), 0, 1)  [gauss_out]
        pool_write(7, i, clip01(affine_remap(gauss_out.v[i], FLOWS_CENTER, FLOWS_SCALE)));
        return;
    }

    if (pass == VO_REMAP) {
        // finalize cones/craters/shields: clip(affine(raw, *)) in place.
        pool_write(4, i, clip01(affine_remap(pool_read(4, i), CONES_CENTER, CONES_SCALE)));
        pool_write(5, i, clip01(affine_remap(pool_read(5, i), CRATERS_CENTER, CRATERS_SCALE)));
        pool_write(6, i, clip01(affine_remap(pool_read(6, i), SHIELDS_CENTER, SHIELDS_SCALE)));
        return;
    }

    if (pass == VO_LAVA_ROUGH) {
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        // lava_texture = affine(fbm(w_x,w_z, 1/(span*0.020),5,sseed+210,0.48), LAVA_TEXTURE) (NO clip)
        float lt = fbm5(w_x, w_z, 1.0 / (span * 0.020), 5, sseed + 210, 0.48);
        pool_write(8, i, affine_remap(lt, LAVA_TEXTURE_CENTER, LAVA_TEXTURE_SCALE));
        // rough_aa = affine(ridged_mf(w_x,w_z, 1/(span*0.027),4,sseed+240,0.48), ROUGH_AA) (NO clip)
        float ra = rmf(w_x, w_z, 1.0 / (span * 0.027), 4, sseed + 240, 0.48);
        pool_write(9, i, affine_remap(ra, ROUGH_AA_CENTER, ROUGH_AA_SCALE));
        return;
    }

    if (pass == VO_BASE) {
        // base = affine(0.58*regional + 0.52*shields*shield_gain + 0.22*rift, BASE)
        float base_inner = 0.58 * pool_read(2, i)
                         + 0.52 * pool_read(6, i) * STYLE_SHIELD_GAIN
                         + 0.22 * pool_read(3, i);
        pool_write(10, i, affine_remap(base_inner, BASE_CENTER, BASE_SCALE));
        return;
    }

    if (pass == VO_RADIAL) {
        // radial_surface = base + 1.12*cones - 0.78*craters  -> flow_pre (gully_channels_seam_safe:
        // pre-blur 1.15 + MFD acc power=0.40 + log1p norm + spread 1.2 + clip; the generic
        // flow_discharge(0.40, 1.15) prefix runs next, then gauss(1.2)).
        flow_pre.v[i] = pool_read(10, i) + 1.12 * pool_read(4, i) - 0.78 * pool_read(5, i);
        return;
    }

    if (pass == VO_GULLIES) {
        // gullies = smoothstep(0.52,0.92, gully_discharge) * (0.30 + 0.70*cones)  [gauss_out =
        // gaussian(raw discharge, 1.2)]. The recipe clips the spread discharge to [0,1] before this;
        // clip01 it here to mirror exactly (smoothstep would clamp anyway, so this is equivalent).
        float discharge = clip01(gauss_out.v[i]);
        pool_write(11, i, ss(0.52, 0.92, discharge) * (0.30 + 0.70 * pool_read(4, i)));
        return;
    }

    if (pass == VO_SPC_PRE) {
        // gauss_in <- shields + cones  (pre gaussian(2.6) -> spc_blur for the caldera fields)
        gauss_in.v[i] = pool_read(6, i) + pool_read(4, i);
        return;
    }

    if (pass == VO_CALDERA) {
        float cones = pool_read(4, i);
        float craters = pool_read(5, i);
        float spc_blur = gauss_out.v[i];
        // caldera_bowl = craters * smoothstep(0.52,0.88, spc_blur)
        pool_write(12, i, craters * ss(0.52, 0.88, spc_blur));
        // caldera_rim = smoothstep(0.38,0.78, cones) * (1 - smoothstep(0.25,0.72, craters))
        pool_write(13, i, ss(0.38, 0.78, cones) * (1.0 - ss(0.25, 0.72, craters)));
        // cone_lift = cones * (1 - 0.88*smoothstep(0.12,0.78, craters))
        pool_write(14, i, cones * (1.0 - 0.88 * ss(0.12, 0.78, craters)));
        return;
    }

    if (pass == VO_ASSEMBLE) {
        float base = pool_read(10, i);
        float cones = pool_read(4, i);
        float shields = pool_read(6, i);
        float rift = pool_read(3, i);
        float flows = pool_read(7, i);
        float lava_texture = pool_read(8, i);
        float rough_aa = pool_read(9, i);
        float gullies = pool_read(11, i);
        float caldera_bowl = pool_read(12, i);
        float caldera_rim = pool_read(13, i);
        float cone_lift = pool_read(14, i);

        float hv = base;
        hv += STYLE_CONE_GAIN * (1.08 * cone_lift + 0.20 * cone_lift * rough_aa);
        hv += STYLE_SHIELD_GAIN * 0.54 * shields;
        hv += 0.22 * rift;
        hv += STYLE_FLOW_GAIN * (0.42 * flows + 0.13 * flows * lava_texture);
        hv += STYLE_CALDERA_GAIN * 0.22 * caldera_rim;
        hv -= STYLE_CALDERA_GAIN * 1.48 * caldera_bowl;
        hv -= STYLE_GULLY_GAIN * 0.30 * gullies;
        hv += STYLE_DETAIL_GAIN * (0.10 + 0.18 * flows + 0.20 * cones) * lava_texture;
        height.v[i] = hv;
        return;
    }

    if (pass == VO_ASH_PRE) {
        // gauss_in <- max(cones, flows)  (pre gaussian(3.0) -> max_cf_blur for the ash_plain blend)
        gauss_in.v[i] = max(pool_read(4, i), pool_read(7, i));
        return;
    }

    if (pass == VO_ASH_BLEND) {
        // ash_plain = smoothstep(0.52,0.86, 1 - max_cf_blur[pool15]) ; smoothed_plain = gaussian(height,2.6)[gauss_out]
        // height = height*(1 - 0.30*ash_plain) + smoothed_plain*(0.30*ash_plain)
        float ash_plain = ss(0.52, 0.86, 1.0 - pool_read(15, i));
        float smoothed_plain = gauss_out.v[i];
        height.v[i] = height.v[i] * (1.0 - 0.30 * ash_plain) + smoothed_plain * (0.30 * ash_plain);
        return;
    }

    if (pass == VO_FINAL) {
        // final_blend = 0.82*height + 0.18*gaussian(height, 0.85)[gauss_out]
        // height = affine_remap(final_blend, FINAL)
        float final_blend = 0.82 * height.v[i] + 0.18 * gauss_out.v[i];
        height.v[i] = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
        return;
    }
}
