// WorldGen10 Slice-4b: DESERT biome FRAGMENT (the seam-safe recipe's biome-specific math).
//
// Concatenated AFTER recipe_primitives.glsl + biome_page.glsl (the MACHINE). It defines the
// desert-specific constants + helpers + biome_pass(), implementing the BIOME PASS_* values
// the machine forwards to it. The MACHINE owns all bindings, the `Params` push constant `P`,
// the generic leaf helpers (cell_idx / clamp_idx / affine_remap / ss / clip01 / rotated0 /
// recipe_recursive_domain_warp / rmf / fbm5), the global storage buffers (INCLUDING the generic
// scratch POOL pool0..poolN + the pool_read/pool_write switch), and the generic passes
// (MESHGRID, COPY, COPY_POOL, POOL_FROM_GAUSS, GAUSS_*, FLOW_PRE_*, ACC_INIT, FLOW_RELAX,
// DISCHARGE, CROP). This fragment carries NO #version / #[compute].
//
// This is the GPU mirror of recipes_desert.rs::generate_seamsafe (the f64 parity ORACLE).
// EVERY constant / seed-offset / weight below is transcribed VERBATIM from recipes_desert.rs.
// EDIT-BOTH-SIDES: changes here must keep parity with that oracle.
//
// SCRATCH-POOL CONTRACT (desert's pool-slot map; needs 16 slots -> POOL_SLOTS bumped to 16):
//   pool0  = w_x          warped X coord (kept for dunes / yardangs / block / fine / salt)
//   pool1  = w_z          warped Z coord (kept for the same downstream sub-fields)
//   pool2  = regional     regional fbm field (-> basin, mesas blur, base_surface)
//   pool3  = basin        basin field (-> playa, mesas, base, masks, floor blend)
//   pool4  = playa        playa field (-> washes, masks, assemble)
//   pool5  = dunes        dune sub-field (-> dune_mask in assemble)
//   pool6  = yardangs     yardang sub-field (-> yardang_mask in assemble)
//   pool7  = mesas        mesa sub-field (-> base, wash surface, masks, assemble)
//   pool8  = base_surface base surface (-> wash surface, assemble)
//   pool9  = washes       carved wash channels (-> wash_mask in assemble + floor blend)
//   pool10 = fine         fine fbm texture (-> assemble detail)
//   pool11 = salt         salt ridged texture (-> assemble yardang detail)
//   pool12 = scratch_a    TRANSIENT: 1-block_edges (pre-blur staging) then free
//   pool13 = rocky_relief TRANSIENT: rocky_relief (consumed by mesas) then free
//   pool14 = block_cores  TRANSIENT: block_cores (consumed by mesas) then free
//   pool15 = scratch_b    sub-pipeline pointwise pre-blur staging (dune raw)
// A biome reads/writes a slot with pool_read(slot,i) / pool_write(slot,i,v). To gaussian-blur a
// pool slot: dispatch PASS_COPY_POOL (pool_sel=slot) -> gauss(sigma) -> read gauss_out (the blur).
// The fixed named buffers (0..23) are mountain's; desert touches only the GENERIC ones
// (wx/wz/flow_pre/gauss_*/height) plus the pool.

// ---------------------------------------------------------------------------
// ===== DESERT biome-private PASS_* codes (start at 32 per the interface convention) =====
// MUST match biome_page_compute.rs DS_* consts.
// ---------------------------------------------------------------------------
const int DS_POINTWISE    = 32; // warp -> pool0=w_x, pool1=w_z, pool2=regional
const int DS_BASIN        = 33; // pool3 = smoothstep(0.34,0.78, 1 - gauss_out[=gaussian(regional,6.2)])
const int DS_PLAYA        = 34; // pool4 = smoothstep(0.56,0.90, gauss_out[=gaussian(basin,5.0)])
const int DS_DUNE_PRE     = 35; // pool15 = dune raw (pointwise ridges on w_x/w_z)
const int DS_DUNE_FINAL   = 36; // pool5 = clip(affine(gauss_out[=gaussian(pool15,0.70)], DUNE))
const int DS_YARDANG      = 37; // pool6 = yardang_field (pointwise, no blur)
const int DS_BLOCK_PRE    = 38; // pool12 = 1-block_edges ; pool13 = rocky_relief (rot angle+0.78)
const int DS_BLOCK_CORES  = 39; // pool14 = smoothstep(0.22,0.76, gauss_out[=gaussian(pool12,3.2)])
const int DS_MESAS        = 40; // pool7 = mesas (gauss_out=gaussian(regional,2.2), block_cores, basin, rocky)
const int DS_BASE         = 41; // pool8 = affine(0.72*reg + 0.24*mesas - 0.62*basin, BASE)
const int DS_WASH_FLOW_PRE= 42; // flow_pre <- base_surface + 0.16*mesas (wash flow source)
const int DS_WASH_FINAL   = 43; // pool9 = smoothstep(0.57,0.94, gauss_out)*(0.35+0.65*(1-playa))
const int DS_FINE_SALT    = 44; // pool10 = fine ; pool11 = salt (on w_x/w_z)
const int DS_ASSEMBLE     = 45; // height = base_surface + relief sum (dune/yardang/wash/playa/mesa + detail)
const int DS_FLOOR_BLEND  = 46; // height = floor blend (gauss_out = gaussian(height, max(floor_smooth,0.2)))
const int DS_FINAL        = 47; // height = affine(0.82*height + 0.18*gauss_out[=gaussian(height,0.95)], FINAL)

// ---------------------------------------------------------------------------
// ===== DESERT constants (verbatim from recipes_desert.rs) =====
// ---------------------------------------------------------------------------
// affine-remap constants
const float REGIONAL_CENTER = -0.668;
const float REGIONAL_SCALE  =  0.716;
const float DUNE_CENTER     =  0.018;
const float DUNE_SCALE      =  1.596;
const float YARDANG_CENTER  =  0.001;
const float YARDANG_SCALE   =  1.093;
const float BASE_CENTER     =  0.113;
const float BASE_SCALE      =  2.312;
const float FINE_CENTER     =  0.000;
const float FINE_SCALE      =  3.543;
const float SALT_CENTER     =  0.365;
const float SALT_SCALE      =  4.185;
const float FINAL_CENTER    =  0.000;
const float FINAL_SCALE     =  0.85;

// DUNE_SEA style (STYLES[0]) fields the seam-safe pipeline reads.
const float STYLE_ANGLE_RAD         = 0.48;
const float STYLE_DUNE_GAIN         = 1.42;
const float STYLE_YARDANG_GAIN      = 0.28;
const float STYLE_WASH_GAIN         = 0.34;
const float STYLE_MESA_GAIN         = 0.20;
const float STYLE_PLAYA_GAIN        = 0.52;
const float STYLE_BASIN_GAIN        = 0.92;
const float STYLE_DUNE_SPACING_M    = 2400.0;
const float STYLE_DUNE_WIDTH        = 0.36;
const float STYLE_YARDANG_ANISOTROPY= 0.30;
const float STYLE_FLOOR_SMOOTH_PX   = 5.2;
const float STYLE_DETAIL_GAIN       = 0.24;
// seed_offset = 0 (sseed = P.seed + 0; we add it explicitly to mirror sseed = seed + offset).
const int   STYLE_SEED_OFFSET       = 0;

const float DESERT_PI = 3.14159265358979323846;

// ---------------------------------------------------------------------------
// cellular_edges: cheap Worley/cellular edge network -> [0,1], high near cell borders.
// Mirror of recipe_noise.rs::cellular_edges(wx,wz,freq,seed,sharpness). NOT in the primitives
// file, so defined locally here (like grassland's fault_block_field). The feature offset uses
// hash2(cx,cz,seed+11)/seed+29; the desert block_edges call uses freq=1/(span*0.210) -> grid
// indices ix/iz are small (well within i32, matching the 32-bit-seed GLSL hash2).
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
// biome_pass: the desert-specific PASS bodies. The machine has already handled the generic
// passes + guards; (cx,cy,i) are the cell coords and linear index for this invocation
// (cx<cols, cy<rows guaranteed by the machine). All pool access goes through pool_read/pool_write
// (defined in the machine). EDIT-BOTH-SIDES with recipes_desert.rs::generate_seamsafe.
// ---------------------------------------------------------------------------
void biome_pass(int pass, int cx, int cy, int i) {
    float span = max(P.feature_span_m, 1.0);
    int sseed = P.seed + STYLE_SEED_OFFSET;

    if (pass == DS_POINTWISE) {
        // recursive_domain_warp(wx,wz, span*0.030, 1/(span*0.72), sseed+10, 3, 0.52, 1.78)
        vec2 w = recipe_recursive_domain_warp(
            wx.v[i], wz.v[i],
            span * 0.030, 1.0 / (span * 0.72),
            sseed + 10, 3, 0.52, 1.78);
        float w_x = w.x;
        float w_z = w.y;
        pool_write(0, i, w_x);
        pool_write(1, i, w_z);
        // regional = clip(affine(fbm(w_x,w_z, 1/(span*0.86),5,sseed+30,0.58), REGIONAL))
        float reg = fbm5(w_x, w_z, 1.0 / (span * 0.86), 5, sseed + 30, 0.58);
        pool_write(2, i, clip01(affine_remap(reg, REGIONAL_CENTER, REGIONAL_SCALE)));
        return;
    }

    if (pass == DS_BASIN) {
        // pool3 = basin = smoothstep(0.34, 0.78, 1 - gauss_out[=gaussian(regional, 6.2)])
        pool_write(3, i, ss(0.34, 0.78, 1.0 - gauss_out.v[i]));
        return;
    }

    if (pass == DS_PLAYA) {
        // pool4 = playa = smoothstep(0.56, 0.90, gauss_out[=gaussian(basin, 5.0)])
        pool_write(4, i, ss(0.56, 0.90, gauss_out.v[i]));
        return;
    }

    if (pass == DS_DUNE_PRE) {
        // _dune_field pointwise raw ridges (on warped coords w_x=pool0, w_z=pool1).
        // rotated by style.angle_rad about origin. spacing = dune_spacing_m (>=1).
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        float spacing = max(STYLE_DUNE_SPACING_M, 1.0);
        float secondary_spacing = max(STYLE_DUNE_SPACING_M * 1.75, 1.0);
        vec2 r = rotated0(w_x, w_z, STYLE_ANGLE_RAD);
        float rx = r.x;
        float rz = r.y;
        float warp = fbm5(w_x, w_z, 1.0 / (span * 0.20), 4, sseed + 120, 0.52)
            * STYLE_DUNE_SPACING_M * 0.72;
        float phase = (rx + warp) / spacing * DESERT_PI * 2.0;
        float crest = 1.0 - abs(sin(phase));
        float secondary = 1.0 - abs(sin(
            (rx * 0.62 + rz * 0.16 + warp * 0.35) / secondary_spacing * DESERT_PI * 2.0));
        float base_v = clip01(0.78 * crest + 0.22 * secondary);
        pool_write(15, i, pow(base_v, 1.0 + 1.8 * STYLE_DUNE_WIDTH));
        return;
    }

    if (pass == DS_DUNE_FINAL) {
        // pool5 = dunes = clip(affine(gauss_out[=gaussian(pool15, 0.70)], DUNE))
        pool_write(5, i, clip01(affine_remap(gauss_out.v[i], DUNE_CENTER, DUNE_SCALE)));
        return;
    }

    if (pass == DS_YARDANG) {
        // _yardang_field pointwise (on warped coords). rotated by style.angle_rad about origin.
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        vec2 r = rotated0(w_x, w_z, STYLE_ANGLE_RAD);
        float rx = r.x;
        float rz = r.y;
        // ridges = rmf(rx, rz*anisotropy, 1/(span*0.075), 5, sseed+210, 0.50)
        float ridges = rmf(rx, rz * STYLE_YARDANG_ANISOTROPY,
            1.0 / (span * 0.075), 5, sseed + 210, 0.50);
        // fine = rmf(rx + 0.22*rz, rz*0.18, 1/(span*0.038), 3, sseed+230, 0.46)
        float fine = rmf(rx + 0.22 * rz, rz * 0.18,
            1.0 / (span * 0.038), 3, sseed + 230, 0.46);
        float combo = 0.72 * ridges + 0.28 * fine;
        pool_write(6, i, ss(0.42, 0.86,
            clip01(affine_remap(combo, YARDANG_CENTER, YARDANG_SCALE))));
        return;
    }

    if (pass == DS_BLOCK_PRE) {
        // rot = angle_rad + 0.78 about fixed origin (on warped coords w_x/w_z).
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        vec2 r = rotated0(w_x, w_z, STYLE_ANGLE_RAD + 0.78);
        float rx = r.x;
        float rz = r.y;
        // block_edges = cellular_edges(rx, rz, 1/(span*0.210), sseed+310, sharpness=1.25)
        float block_edges = cellular_edges(rx, rz, 1.0 / (span * 0.210), sseed + 310, 1.25);
        pool_write(12, i, 1.0 - block_edges);
        // rocky_relief = smoothstep(0.36, 0.84, rmf(rx, rz*0.42, 1/(span*0.18), 4, sseed+330, 0.52))
        float rr = rmf(rx, rz * 0.42, 1.0 / (span * 0.18), 4, sseed + 330, 0.52);
        pool_write(13, i, ss(0.36, 0.84, rr));
        return;
    }

    if (pass == DS_BLOCK_CORES) {
        // pool14 = block_cores = smoothstep(0.22, 0.76, gauss_out[=gaussian(1-block_edges, 3.2)])
        pool_write(14, i, ss(0.22, 0.76, gauss_out.v[i]));
        return;
    }

    if (pass == DS_MESAS) {
        // gauss_out = regional_blur22 = gaussian(regional, 2.2).
        // mesa_blocks = smoothstep(0.52,0.82, regional_blur22) * block_cores * (1 - 0.68*basin)
        // mesas = clip(0.68*mesa_blocks + 0.32*rocky_relief*(1 - 0.42*basin), 0, 1)
        float basin = pool_read(3, i);
        float block_cores = pool_read(14, i);
        float rocky_relief = pool_read(13, i);
        float mesa_blocks =
            ss(0.52, 0.82, gauss_out.v[i]) * block_cores * (1.0 - 0.68 * basin);
        pool_write(7, i, clip01(
            0.68 * mesa_blocks + 0.32 * rocky_relief * (1.0 - 0.42 * basin)));
        return;
    }

    if (pass == DS_BASE) {
        // base_surface = affine(0.72*regional + 0.24*mesas - 0.62*basin, BASE)
        float inner = 0.72 * pool_read(2, i) + 0.24 * pool_read(7, i) - 0.62 * pool_read(3, i);
        pool_write(8, i, affine_remap(inner, BASE_CENTER, BASE_SCALE));
        return;
    }

    if (pass == DS_WASH_FLOW_PRE) {
        // flow_pre <- base_surface + 0.16*mesas (the wash flow source surface; NO clip).
        flow_pre.v[i] = pool_read(8, i) + 0.16 * pool_read(7, i);
        return;
    }

    if (pass == DS_WASH_FINAL) {
        // gauss_out = spread discharge from flow_channels(wash_surface, width=1.8, power=0.43).
        // washes = smoothstep(0.57, 0.94, washes) * (0.35 + 0.65*(1 - playa))
        float w = ss(0.57, 0.94, gauss_out.v[i]);
        pool_write(9, i, w * (0.35 + 0.65 * (1.0 - pool_read(4, i))));
        return;
    }

    if (pass == DS_FINE_SALT) {
        // fine = affine(fbm(w_x,w_z, 1/(span*0.018),4,sseed+410,0.48), FINE)
        // salt = affine(rmf(w_x,w_z, 1/(span*0.025),3,sseed+430,0.42), SALT)
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        float fv = fbm5(w_x, w_z, 1.0 / (span * 0.018), 4, sseed + 410, 0.48);
        pool_write(10, i, affine_remap(fv, FINE_CENTER, FINE_SCALE));
        float sv = rmf(w_x, w_z, 1.0 / (span * 0.025), 3, sseed + 430, 0.42);
        pool_write(11, i, affine_remap(sv, SALT_CENTER, SALT_SCALE));
        return;
    }

    if (pass == DS_ASSEMBLE) {
        // masks:
        //   sand_mask    = clip((0.42 + 0.58*basin) * (1 - 0.42*mesas), 0, 1)
        //   dune_mask    = dunes * sand_mask * (0.25 + 0.75*basin)
        //   yardang_mask = yardangs * (0.45 + 0.55*basin) * (1 - 0.35*dune_mask)
        //   wash_mask    = washes * (0.45 + 0.55*(1 - basin + 0.35*mesas))
        //   playa_mask   = playa * (1 - 0.45*dune_mask)
        // relief = mask * style.<gain> ; assemble height as in the oracle.
        float basin = pool_read(3, i);
        float playa = pool_read(4, i);
        float dunes = pool_read(5, i);
        float yardangs = pool_read(6, i);
        float mesas = pool_read(7, i);
        float base_surface = pool_read(8, i);
        float washes = pool_read(9, i);
        float fine = pool_read(10, i);
        float salt = pool_read(11, i);

        float sand_mask = clip01((0.42 + 0.58 * basin) * (1.0 - 0.42 * mesas));
        float dune_mask = dunes * sand_mask * (0.25 + 0.75 * basin);
        float yardang_mask = yardangs * (0.45 + 0.55 * basin) * (1.0 - 0.35 * dune_mask);
        float wash_mask = washes * (0.45 + 0.55 * (1.0 - basin + 0.35 * mesas));
        float playa_mask = playa * (1.0 - 0.45 * dune_mask);

        float d_relief = dune_mask * STYLE_DUNE_GAIN;
        float y_relief = yardang_mask * STYLE_YARDANG_GAIN;
        float w_relief = wash_mask * STYLE_WASH_GAIN;
        float p_relief = playa_mask * STYLE_PLAYA_GAIN;
        float m_relief = mesas * STYLE_MESA_GAIN;

        // height  = base_surface
        // height += basin_gain * 0.24 * (1 - basin)
        // height += 0.50*mesa_relief + 0.14*mesa_relief*fine
        // height += 0.44*dune_relief + 0.10*dune_relief*fine
        // height += 0.34*yardang_relief + 0.08*yardang_relief*salt
        // height -= 0.36*wash_relief
        // height -= 0.38*playa_relief
        // height += detail_gain * (0.08 + 0.12*mesas + 0.12*yardang_mask) * fine
        float hv = base_surface;
        hv += STYLE_BASIN_GAIN * 0.24 * (1.0 - basin);
        hv += 0.50 * m_relief + 0.14 * m_relief * fine;
        hv += 0.44 * d_relief + 0.10 * d_relief * fine;
        hv += 0.34 * y_relief + 0.08 * y_relief * salt;
        hv -= 0.36 * w_relief;
        hv -= 0.38 * p_relief;
        hv += STYLE_DETAIL_GAIN * (0.08 + 0.12 * mesas + 0.12 * yardang_mask) * fine;
        height.v[i] = hv;
        return;
    }

    if (pass == DS_FLOOR_BLEND) {
        // gauss_out = smooth_floor = gaussian(height, max(floor_smooth_px, 0.2)).
        // floor_mask uses playa_relief + basin + wash_relief; playa_relief / wash_relief are
        // recomputed here (their inputs basin/playa/dunes/mesas/washes are all still live).
        //   floor_mask = clip(0.68*playa_relief + 0.46*basin + 0.34*wash_relief, 0, 1)
        //   height = height*(1 - 0.34*floor_mask) + smooth_floor*(0.34*floor_mask)
        float basin = pool_read(3, i);
        float playa = pool_read(4, i);
        float dunes = pool_read(5, i);
        float mesas = pool_read(7, i);
        float washes = pool_read(9, i);

        float sand_mask = clip01((0.42 + 0.58 * basin) * (1.0 - 0.42 * mesas));
        float dune_mask = dunes * sand_mask * (0.25 + 0.75 * basin);
        float wash_mask = washes * (0.45 + 0.55 * (1.0 - basin + 0.35 * mesas));
        float playa_mask = playa * (1.0 - 0.45 * dune_mask);
        float p_relief = playa_mask * STYLE_PLAYA_GAIN;
        float w_relief = wash_mask * STYLE_WASH_GAIN;

        float floor_mask = clip01(0.68 * p_relief + 0.46 * basin + 0.34 * w_relief);
        height.v[i] = height.v[i] * (1.0 - 0.34 * floor_mask) + gauss_out.v[i] * (0.34 * floor_mask);
        return;
    }

    if (pass == DS_FINAL) {
        // gauss_out = height_blur = gaussian(height, 0.95).
        // final_blend = 0.82*height + 0.18*height_blur ; height = affine(final_blend, FINAL)
        float final_blend = 0.82 * height.v[i] + 0.18 * gauss_out.v[i];
        height.v[i] = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
        return;
    }
}
