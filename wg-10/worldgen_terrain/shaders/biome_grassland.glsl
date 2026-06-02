// WorldGen10 Slice-4b: GRASSLAND biome FRAGMENT (the seam-safe recipe's biome-specific math).
//
// Concatenated AFTER recipe_primitives.glsl + biome_page.glsl (the MACHINE). It defines the
// grassland-specific constants + helpers + biome_pass(), implementing the BIOME PASS_* values
// the machine forwards to it. The MACHINE owns all bindings, the `Params` push constant `P`,
// the generic leaf helpers (cell_idx / clamp_idx / affine_remap / ss / clip01 / rotated0 /
// recipe_recursive_domain_warp / rmf / fbm5), the global storage buffers (INCLUDING the generic
// scratch POOL pool0..poolN + the pool_read/pool_write switch), and the generic passes
// (MESHGRID, COPY, COPY_POOL, POOL_FROM_GAUSS, GAUSS_*, FLOW_PRE_*, ACC_INIT, FLOW_RELAX,
// DISCHARGE, CROP). This fragment carries NO #version / #[compute].
//
// This is the GPU mirror of recipes_grassland.rs::generate_seamsafe (the f64 parity ORACLE).
// EVERY constant / seed-offset / weight below is transcribed VERBATIM from recipes_grassland.rs.
// EDIT-BOTH-SIDES: changes here must keep parity with that oracle.
//
// SCRATCH-POOL CONTRACT (grassland's pool-slot map; the pattern the other 9 biomes reuse):
//   pool0 = w_x          warped X coord (kept for sandhills / escarpments / texture)
//   pool1 = w_z          warped Z coord (kept for the same downstream sub-fields)
//   pool2 = macro_f      macro fBm field (consumed by the swells combo)
//   pool3 = secondary    secondary fBm field (consumed by the swells combo)
//   pool4 = swells       swells field (-> pans, base_for_flow, height)
//   pool5 = pans         pans field (-> base_for_flow, draws, height, floor blend)
//   pool6 = sandhills    sandhill sub-field (-> height texture)
//   pool7 = escarpments  escarpment sub-field (-> base_for_flow, height, floor blend)
//   pool8 = draws        carved draw channels (-> height)
//   pool9 = fine_grain   rotated fine-grain texture (-> height)
//   pool10 = low_ripple  rotated low-ripple ridged texture (-> height)
//   pool11 = scratch     sub-pipeline pointwise pre-blur staging (sandhill `pre` / esc `edge`)
// A biome reads/writes a slot with pool_read(slot,i) / pool_write(slot,i,v). To gaussian-blur a
// pool slot: dispatch PASS_COPY_POOL (pool_sel=slot) -> gauss(sigma) -> read gauss_out (the blur).
// To stash a blur back into a slot: PASS_POOL_FROM_GAUSS (pool_sel=slot). The fixed named buffers
// (0..23) are mountain's; grassland touches only the GENERIC ones (wx/wz/flow_pre/gauss_*/height)
// plus the pool. Other biomes just request more POOL_SLOTS (one constant in the machine + Rust).

// ---------------------------------------------------------------------------
// ===== GRASSLAND biome-private PASS_* codes (start at 32 per the interface convention) =====
// MUST match biome_page_compute.rs GL_* consts.
// ---------------------------------------------------------------------------
const int GL_POINTWISE        = 32; // warp -> pool0=w_x, pool1=w_z, pool2=macro_f, pool3=secondary
const int GL_COMBO            = 33; // gauss_in <- 0.74*macro_f + 0.26*secondary (pre-swells blur)
const int GL_SWELLS           = 34; // pool4=swells = clip(affine(gauss_out, SWELLS))
const int GL_ONE_MINUS_SWELLS = 35; // gauss_in <- 1 - swells (pre-pans blur sigma=5.2)
const int GL_PANS             = 36; // pool5=pans = smoothstep(0.54,0.88, gauss_out)
const int GL_SANDHILL_PRE     = 37; // pool11 = softened*envelope*broken (on w_x/w_z)
const int GL_SANDHILL_FINAL   = 38; // pool6=sandhills = clip(affine(gauss_out=blur1.55(pool11), SH_FINAL))
const int GL_ESC_PRE          = 39; // pool11 = smoothstep(0.18,0.62,|bands|)*plateau (on w_x/w_z)
const int GL_ESC_FINAL        = 40; // pool7=escarpments = clip(affine(gauss_out=blur1.4(pool11), ESC_FINAL))
const int GL_BASE_FOR_FLOW    = 41; // flow_pre <- affine(0.82*swells+0.28*esc-0.34*pans) (NO clip)
const int GL_DRAWS            = 42; // pool8=draws = smoothstep(0.60,0.94,gauss_out)*(0.42+0.58*(1-pans))
const int GL_TEXTURE          = 43; // pool9=fine_grain, pool10=low_ripple (rotated angle+1.10)
const int GL_ASSEMBLE         = 44; // height = weighted sum (swells/sandhills/esc/pans/draws/texture)
const int GL_OPEN_FLOOR_BLEND = 45; // height = floor blend (gauss_out = gaussian(height, max(smoothing,0.5)))
const int GL_FINAL            = 46; // height = affine(0.86*height + 0.14*gauss_out, FINAL)

// ---------------------------------------------------------------------------
// ===== GRASSLAND constants (verbatim from recipes_grassland.rs) =====
// ---------------------------------------------------------------------------
// affine-remap constants
const float MACRO_CENTER         = -0.50;
const float MACRO_SCALE          =  1.14;
const float SECONDARY_CENTER     = -0.69;
const float SECONDARY_SCALE      =  0.72;
const float SWELLS_CENTER        =  0.13;
const float SWELLS_SCALE         =  1.37;
const float SWELLS_ZSCORE_CENTER =  0.507;
const float SWELLS_ZSCORE_SCALE  =  4.49;
const float BASE_FLOW_CENTER     =  0.503;
const float BASE_FLOW_SCALE      =  5.11;
const float FINE_GRAIN_CENTER    =  0.00;
const float FINE_GRAIN_SCALE     =  3.47;
const float LOW_RIPPLE_CENTER    =  0.353;
const float LOW_RIPPLE_SCALE     =  4.27;
const float SH_ENVELOPE_CENTER   = -0.38;
const float SH_ENVELOPE_SCALE    =  1.01;
const float SH_BROKEN_CENTER     = -0.87;
const float SH_BROKEN_SCALE      =  0.58;
const float SH_FINAL_CENTER      =  0.00;
const float SH_FINAL_SCALE       =  1.00;
const float ESC_PLATEAU_CENTER   = -0.51;
const float ESC_PLATEAU_SCALE    =  0.90;
const float ESC_FINAL_CENTER     =  0.00;
const float ESC_FINAL_SCALE      =  1.00;
const float FINAL_CENTER         =  0.00;
const float FINAL_SCALE          =  0.82;

// ROLLING_PRAIRIE style (STYLES[0]) fields the seam-safe pipeline reads.
const float STYLE_ANGLE_RAD       = 0.34;
const float STYLE_SWELL_GAIN      = 1.18;
const float STYLE_DRAW_GAIN       = 0.72;
const float STYLE_SANDHILL_GAIN   = 0.00;
const float STYLE_PAN_GAIN        = 0.18;
const float STYLE_ESCARPMENT_GAIN = 0.18;
const float STYLE_TEXTURE_GAIN    = 0.42;
const float STYLE_SMOOTHING_PX    = 3.7;
// seed_offset = 0 (sseed = P.seed + 0; we add it explicitly to mirror sseed = seed + offset).
const int   STYLE_SEED_OFFSET     = 0;

const float PI = 3.14159265358979323846;

// ---------------------------------------------------------------------------
// fault_block_field: signed broad fault bands -> [-1,1]. Mirror of
// recipe_noise.rs::fault_block_field(wx,wz,cell_size,width,seed,neighborhood). Grassland's
// escarpment field calls it with neighborhood=2 (the Python default). The grid indices gx/gz
// are small (world coords / (span*0.54)) -> well within i32 (matches the GLSL hash2 32-bit seed).
// ---------------------------------------------------------------------------
float fault_block_field(float wxc, float wzc, float cell_size, float width, int seed, int neighborhood) {
    int gx = int(floor(wxc / cell_size));
    int gz = int(floor(wzc / cell_size));
    float acc_out = 0.0;  // `out` is a GLSL reserved keyword -> use acc_out.
    float norm = 0.0;
    for (int dz = -neighborhood; dz <= neighborhood; ++dz) {
        for (int dx = -neighborhood; dx <= neighborhood; ++dx) {
            int cx = gx + dx;
            int cz = gz + dz;
            float center_x = (float(cx) + 0.5 + (hash2(cx, cz, seed + 10) - 0.5) * 0.45) * cell_size;
            float center_z = (float(cz) + 0.5 + (hash2(cx, cz, seed + 11) - 0.5) * 0.45) * cell_size;
            float angle = hash2(cx, cz, seed + 12) * PI * 2.0;
            float nx = -sin(angle);
            float nz =  cos(angle);
            float signed_d = (wxc - center_x) * nx + (wzc - center_z) * nz;
            float amp = hash2(cx, cz, seed + 13) * 2.0 - 1.0;
            float t = signed_d / (cell_size * 0.55);
            float influence = exp(-(t * t));
            acc_out += amp * tanh(signed_d / width) * influence;
            norm += 1.0;
        }
    }
    return clamp(acc_out / max(norm * 0.22, 1e-9), -1.0, 1.0);
}

// ---------------------------------------------------------------------------
// biome_pass: the grassland-specific PASS bodies. The machine has already handled the generic
// passes + guards; (cx,cy,i) are the cell coords and linear index for this invocation
// (cx<cols, cy<rows guaranteed by the machine). All pool access goes through pool_read/pool_write
// (defined in the machine). EDIT-BOTH-SIDES with recipes_grassland.rs::generate_seamsafe.
// ---------------------------------------------------------------------------
void biome_pass(int pass, int cx, int cy, int i) {
    float span = max(P.feature_span_m, 1.0);
    int sseed = P.seed + STYLE_SEED_OFFSET;

    if (pass == GL_POINTWISE) {
        // recursive_domain_warp(wx,wz, span*0.020, 1/(span*0.78), sseed+10, 3, 0.55, 1.70)
        vec2 w = recipe_recursive_domain_warp(
            wx.v[i], wz.v[i],
            span * 0.020, 1.0 / (span * 0.78),
            sseed + 10, 3, 0.55, 1.70);
        float w_x = w.x;
        float w_z = w.y;
        pool_write(0, i, w_x);
        pool_write(1, i, w_z);
        // macro_f = clip(affine(fbm(w_x,w_z, 1/(span*0.92),5,sseed+30,0.58)))
        float m = fbm5(w_x, w_z, 1.0 / (span * 0.92), 5, sseed + 30, 0.58);
        pool_write(2, i, clip01(affine_remap(m, MACRO_CENTER, MACRO_SCALE)));
        // secondary = clip(affine(fbm(w_x,w_z, 1/(span*0.34),4,sseed+50,0.55)))
        float s = fbm5(w_x, w_z, 1.0 / (span * 0.34), 4, sseed + 50, 0.55);
        pool_write(3, i, clip01(affine_remap(s, SECONDARY_CENTER, SECONDARY_SCALE)));
        return;
    }

    if (pass == GL_COMBO) {
        // gauss_in <- 0.74*macro_f + 0.26*secondary (the combo blurred by smoothing_px -> swells)
        gauss_in.v[i] = 0.74 * pool_read(2, i) + 0.26 * pool_read(3, i);
        return;
    }

    if (pass == GL_SWELLS) {
        // pool4 = swells = clip(affine(gauss_out [= gaussian(combo, smoothing_px)], SWELLS))
        pool_write(4, i, clip01(affine_remap(gauss_out.v[i], SWELLS_CENTER, SWELLS_SCALE)));
        return;
    }

    if (pass == GL_ONE_MINUS_SWELLS) {
        // gauss_in <- 1 - swells (then gaussian(., 5.2) -> pans)
        gauss_in.v[i] = 1.0 - pool_read(4, i);
        return;
    }

    if (pass == GL_PANS) {
        // pool5 = pans = smoothstep(0.54, 0.88, gauss_out [= gaussian(1-swells, 5.2)])
        pool_write(5, i, ss(0.54, 0.88, gauss_out.v[i]));
        return;
    }

    if (pass == GL_SANDHILL_PRE) {
        // _sandhill_field pointwise pre (on warped coords w_x=pool0, w_z=pool1).
        // spacing = span*0.030 ; rotated by style.angle_rad about origin.
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        float spacing = span * 0.030;
        vec2 r = rotated0(w_x, w_z, STYLE_ANGLE_RAD);
        float rx = r.x;
        float rz = r.y;
        float warp = fbm5(w_x, w_z, 1.0 / (span * 0.30), 4, sseed + 120, 0.52) * spacing * 1.20;
        float cross = fbm5(
            w_x + rz * 0.18, w_z + rx * 0.08,
            1.0 / (span * 0.12), 3, sseed + 126, 0.50);
        float phase = (rx + warp + cross * spacing * 0.42) / max(spacing, 1.0) * PI * 2.0;
        float secondary = (rx * 0.74 + rz * 0.18 + warp * 0.30) / max(spacing * 1.65, 1.0) * PI * 2.0;
        float ridges = 0.74 * (1.0 - abs(sin(phase))) + 0.26 * (1.0 - abs(sin(secondary)));
        float softened = pow(clip01(ridges), 1.55);

        float envelope_raw = fbm5(w_x, w_z, 1.0 / (span * 0.76), 4, sseed + 130, 0.5);
        float envelope = ss(0.48, 0.80,
            clip01(affine_remap(envelope_raw, SH_ENVELOPE_CENTER, SH_ENVELOPE_SCALE)));
        float broken_raw = fbm5(w_x, w_z, 1.0 / (span * 0.055), 3, sseed + 136, 0.46);
        float broken = 0.55 + 0.45 * clip01(affine_remap(broken_raw, SH_BROKEN_CENTER, SH_BROKEN_SCALE));
        pool_write(11, i, softened * envelope * broken);
        return;
    }

    if (pass == GL_SANDHILL_FINAL) {
        // pool6 = sandhills = clip(affine(gauss_out [= gaussian(pool11, 1.55)], SH_FINAL))
        pool_write(6, i, clip01(affine_remap(gauss_out.v[i], SH_FINAL_CENTER, SH_FINAL_SCALE)));
        return;
    }

    if (pass == GL_ESC_PRE) {
        // _escarpment_field pointwise edge (on warped coords). Rotation = style.angle_rad + 0.58.
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        vec2 r = rotated0(w_x, w_z, STYLE_ANGLE_RAD + 0.58);
        float rx = r.x;
        float rz = r.y;
        // fault_block_field(rx, rz, span*0.54, span*0.040, sseed+210, neighborhood=2)
        float bands = fault_block_field(rx, rz, span * 0.54, span * 0.040, sseed + 210, 2);
        float plateau_raw = fbm5(w_x, w_z, 1.0 / (span * 0.64), 4, sseed + 230, 0.5);
        float plateau = ss(0.44, 0.78,
            clip01(affine_remap(plateau_raw, ESC_PLATEAU_CENTER, ESC_PLATEAU_SCALE)));
        pool_write(11, i, ss(0.18, 0.62, abs(bands)) * plateau);
        return;
    }

    if (pass == GL_ESC_FINAL) {
        // pool7 = escarpments = clip(affine(gauss_out [= gaussian(pool11, 1.4)], ESC_FINAL))
        pool_write(7, i, clip01(affine_remap(gauss_out.v[i], ESC_FINAL_CENTER, ESC_FINAL_SCALE)));
        return;
    }

    if (pass == GL_BASE_FOR_FLOW) {
        // flow_pre <- affine(0.82*swells + 0.28*escarpments - 0.34*pans, BASE_FLOW) (NO clip)
        float inner = 0.82 * pool_read(4, i) + 0.28 * pool_read(7, i) - 0.34 * pool_read(5, i);
        flow_pre.v[i] = affine_remap(inner, BASE_FLOW_CENTER, BASE_FLOW_SCALE);
        return;
    }

    if (pass == GL_DRAWS) {
        // gauss_out = spread discharge from flow_channels(base_for_flow, width=2.1, power=0.50).
        // d = smoothstep(0.60, 0.94, draws) ; draws = d * (0.42 + 0.58*(1 - pans))
        float d = ss(0.60, 0.94, gauss_out.v[i]);
        pool_write(8, i, d * (0.42 + 0.58 * (1.0 - pool_read(5, i))));
        return;
    }

    if (pass == GL_TEXTURE) {
        // fine_grain + low_ripple: seam-safe rotation (angle + 1.10) about origin, on w_x/w_z.
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        vec2 r = rotated0(w_x, w_z, STYLE_ANGLE_RAD + 1.10);
        float rx = r.x;
        float rz = r.y;
        float fg = fbm5(rx, rz, 1.0 / (span * 0.032), 4, sseed + 310, 0.46);
        pool_write(9, i, affine_remap(fg, FINE_GRAIN_CENTER, FINE_GRAIN_SCALE));
        // ridged_multifractal(rx, rz*0.34, 1/(span*0.075), 3, sseed+330, 0.44) -- NOTE rz*0.34.
        float lr = rmf(rx, rz * 0.34, 1.0 / (span * 0.075), 3, sseed + 330, 0.44);
        pool_write(10, i, affine_remap(lr, LOW_RIPPLE_CENTER, LOW_RIPPLE_SCALE));
        return;
    }

    if (pass == GL_ASSEMBLE) {
        // height  = affine_remap(swells, SWELLS_ZSCORE) * (0.52 * swell_gain)
        // height += 0.16 * sandhill_gain * sandhills
        // height += 0.34 * escarpment_gain * escarpments
        // height -= 0.28 * pan_gain * pans
        // height -= 0.24 * draw_gain * draws
        // height += texture_gain * (0.050*fine_grain + 0.050*low_ripple*(0.35 + 0.65*sandhills))
        float swells = pool_read(4, i);
        float sandhills = pool_read(6, i);
        float escarpments = pool_read(7, i);
        float pans = pool_read(5, i);
        float draws = pool_read(8, i);
        float fine_grain = pool_read(9, i);
        float low_ripple = pool_read(10, i);
        float hv = affine_remap(swells, SWELLS_ZSCORE_CENTER, SWELLS_ZSCORE_SCALE)
            * (0.52 * STYLE_SWELL_GAIN);
        hv += 0.16 * STYLE_SANDHILL_GAIN * sandhills;
        hv += 0.34 * STYLE_ESCARPMENT_GAIN * escarpments;
        hv -= 0.28 * STYLE_PAN_GAIN * pans;
        hv -= 0.24 * STYLE_DRAW_GAIN * draws;
        hv += STYLE_TEXTURE_GAIN
            * (0.050 * fine_grain + 0.050 * low_ripple * (0.35 + 0.65 * sandhills));
        height.v[i] = hv;
        return;
    }

    if (pass == GL_OPEN_FLOOR_BLEND) {
        // gauss_out = smooth = gaussian(height, sigma=max(smoothing_px, 0.5)).
        // open_floor = clip(0.62*pans + 0.26*(1 - escarpments), 0, 1)
        // height = height*(1 - 0.28*open_floor) + smooth*(0.28*open_floor)
        float open_floor = clip01(0.62 * pool_read(5, i) + 0.26 * (1.0 - pool_read(7, i)));
        height.v[i] = height.v[i] * (1.0 - 0.28 * open_floor) + gauss_out.v[i] * (0.28 * open_floor);
        return;
    }

    if (pass == GL_FINAL) {
        // gauss_out = height_blur = gaussian(height, sigma=1.1).
        // final_blend = 0.86*height + 0.14*height_blur ; height = affine(final_blend, FINAL)
        float final_blend = 0.86 * height.v[i] + 0.14 * gauss_out.v[i];
        height.v[i] = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
        return;
    }
}
