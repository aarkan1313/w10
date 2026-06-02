// WorldGen10 Slice-4b: COAST biome FRAGMENT (the seam-safe recipe's biome-specific math).
//
// Concatenated AFTER recipe_primitives.glsl + biome_page.glsl (the MACHINE). It defines the
// coast-specific constants + helpers + biome_pass(), implementing the BIOME PASS_* values the
// machine forwards to it. The MACHINE owns all bindings, the `Params` push constant `P`, the
// generic leaf helpers (cell_idx / clamp_idx / affine_remap / ss / clip01 / rotated0 /
// recipe_recursive_domain_warp / rmf / fbm5), the global storage buffers (INCLUDING the generic
// scratch POOL pool0..poolN + the pool_read/pool_write switch), and the generic passes
// (MESHGRID, COPY, COPY_POOL, POOL_FROM_GAUSS, GAUSS_*, FLOW_PRE_*, ACC_INIT, FLOW_RELAX,
// DISCHARGE, CROP). This fragment carries NO #version / #[compute].
//
// This is the GPU mirror of recipes_coast.rs::generate_seamsafe (the f64 parity ORACLE).
// EVERY constant / seed-offset / weight below is transcribed VERBATIM from recipes_coast.rs.
// EDIT-BOTH-SIDES: changes here must keep parity with that oracle.
//
// SCRATCH-POOL CONTRACT (coast's pool-slot map; needs 16 slots -> POOL_SLOTS is 16):
//   pool0  = rx           rotated RAW x coord (-> signed, fjord_grooves)
//   pool1  = rz           rotated RAW z coord (-> fjord_grooves)
//   pool2  = w_x          warped X coord (-> coast_warp/inland/headlands/islands_seed/texture/sea_floor)
//   pool3  = w_z          warped Z coord (-> the same warped-coord sub-fields)
//   pool4  = signed       signed coast distance (-> sea/shelf/nearshore/fjord_grooves/islands)
//   pool5  = sea          sea mask (-> islands, height assembly, sea-smoothing blend)
//   pool6  = land         land mask (-> channels, fjords, fjord_grooves, height assembly)
//   pool7  = nearshore    nearshore falloff (-> scarp, fjords)
//   pool8  = shelf        shelf mask (-> height assembly)
//   pool9  = inland_raw   raw inland fbm (-> land_height affine)
//   pool10 = headlands    headlands mask (-> ridge_source, land_height)
//   pool11 = scarp        scarp field (-> ridge_source, land_height)
//   pool12 = ridge_source / islands_seed  TRANSIENT: ridge_source feeds the channel flow, consumed
//                          by the flow pass; the slot is then reused to stage islands_seed pre-blur.
//   pool13 = channels     carved channel field (-> channel_relief, fjords)
//   pool14 = channel_relief  combined channel/fjord relief (-> land_height)
//   pool15 = islands      island relief (-> height assembly)
// A biome reads/writes a slot with pool_read(slot,i) / pool_write(slot,i,v). To gaussian-blur a
// pool slot: dispatch PASS_COPY_POOL (pool_sel=slot) -> gauss(sigma) -> read gauss_out (the blur).
// The fixed named buffers (0..23) are mountain's; coast touches only the GENERIC ones
// (wx/wz/flow_pre/gauss_*/height) plus the pool.

// ---------------------------------------------------------------------------
// ===== COAST biome-private PASS_* codes (start at 32 per the interface convention) =====
// MUST match biome_page_compute.rs CO_* consts.
// ---------------------------------------------------------------------------
const int CO_POINTWISE     = 32; // rotation/warp -> pool0..pool11 + pool12=ridge_source
const int CO_FLOW_PRE      = 33; // flow_pre <- ridge_source (the channel flow source)
const int CO_CHANNELS      = 34; // pool13 = smoothstep(0.53,0.92, gauss_out)*land
const int CO_CHANNEL_RELIEF= 35; // pool14 = clip(channels + fjords + fjord_grooves combo)
const int CO_ISLANDS_SEED  = 36; // pool12 = cellular_edges(w_x,w_z, 1/(span*0.18), sseed+160, 1.30)
const int CO_ISLANDS       = 37; // pool15 = smoothstep(0.50,0.86, gauss_out)*sea * smoothstep(...)
const int CO_ASSEMBLE      = 38; // height = land*land_height + sea*sea_floor + islands - shelf
const int CO_SEA_BLEND     = 39; // height = height*(1-0.34*sea) + gauss_out*(0.34*sea)
const int CO_FINAL         = 40; // height = affine(0.86*height + 0.14*gauss_out[=gaussian(h,0.9)], FINAL)

// ---------------------------------------------------------------------------
// ===== COAST constants (verbatim from recipes_coast.rs) =====
// ---------------------------------------------------------------------------
// affine-remap constants
const float INLAND_CENTER         = -0.551;
const float INLAND_SCALE          =  0.923;
const float RIDGE_SOURCE_CENTER   =  0.500;
const float RIDGE_SOURCE_SCALE    =  4.474;
const float TEXTURE_CENTER        =  0.350;
const float TEXTURE_SCALE         =  4.437;
const float SEA_FLOOR_CENTER      = -0.708;
const float SEA_FLOOR_SCALE       =  0.713;
const float INLAND_ZSCORE_CENTER  = -0.045;
const float INLAND_ZSCORE_SCALE   =  4.499;
const float FINAL_CENTER          = -0.518;
const float FINAL_SCALE           =  1.662;

// CLIFFED_HEADLANDS style (STYLES[0]) fields the seam-safe pipeline reads.
const float STYLE_ANGLE_RAD       = 0.12;
const float STYLE_SCARP_GAIN      = 1.28;
const float STYLE_FJORD_GAIN      = 0.28;
const float STYLE_ISLAND_GAIN     = 0.34;
const float STYLE_SHELF_GAIN      = 0.82;
const float STYLE_HEADLAND_GAIN   = 1.14;
const float STYLE_TEXTURE_GAIN    = 0.72;
const float STYLE_COASTLINE_WARP  = 0.92;
// seed_offset = 0 (sseed = P.seed + 0; we add it explicitly to mirror sseed = seed + offset).
const int   STYLE_SEED_OFFSET     = 0;

// ---------------------------------------------------------------------------
// cellular_edges: cheap Worley/cellular edge network -> [0,1], high near cell borders.
// Mirror of recipe_noise.rs::cellular_edges(wx,wz,freq,seed,sharpness). NOT in the primitives
// file, so defined locally here (identical body to desert's cellular_edges; coast calls it with
// sharpness=1.30). The feature offset uses hash2(cx,cz,seed+11)/seed+29; coast's islands_seed
// call uses freq=1/(span*0.18) -> grid indices ix/iz are small (well within i32, matching the
// 32-bit-seed GLSL hash2).
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
// biome_pass: the coast-specific PASS bodies. The machine has already handled the generic
// passes + guards; (cx,cy,i) are the cell coords and linear index for this invocation
// (cx<cols, cy<rows guaranteed by the machine). All pool access goes through pool_read/pool_write
// (defined in the machine). EDIT-BOTH-SIDES with recipes_coast.rs::generate_seamsafe.
// ---------------------------------------------------------------------------
void biome_pass(int pass, int cx, int cy, int i) {
    float span = max(P.feature_span_m, 1.0);
    int sseed = P.seed + STYLE_SEED_OFFSET;

    if (pass == CO_POINTWISE) {
        // rx, rz = rotated(wx, wz, angle_rad, cx=0, cz=0)  (RAW world coords, about origin)
        vec2 r = rotated0(wx.v[i], wz.v[i], STYLE_ANGLE_RAD);
        float rx = r.x;
        float rz = r.y;
        pool_write(0, i, rx);
        pool_write(1, i, rz);
        // w_x, w_z = recursive_domain_warp(wx, wz, span*0.026, 1/(span*0.82), sseed+10, 3, 0.55, 1.72)
        vec2 w = recipe_recursive_domain_warp(
            wx.v[i], wz.v[i],
            span * 0.026, 1.0 / (span * 0.82),
            sseed + 10, 3, 0.55, 1.72);
        float w_x = w.x;
        float w_z = w.y;
        pool_write(2, i, w_x);
        pool_write(3, i, w_z);
        // coast_warp = fbm(w_x, w_z, 1/(span*0.42), 5, sseed+30, gain=0.56)
        float coast_warp = fbm5(w_x, w_z, 1.0 / (span * 0.42), 5, sseed + 30, 0.56);
        // signed = rx + coast_warp * span * 0.15 * coastline_warp
        float signed_v = rx + coast_warp * span * 0.15 * STYLE_COASTLINE_WARP;
        pool_write(4, i, signed_v);
        // sea = smoothstep(span*0.030, -span*0.030, signed)
        float sea_v = ss(span * 0.030, -span * 0.030, signed_v);
        pool_write(5, i, sea_v);
        // land = 1 - sea
        float land_v = 1.0 - sea_v;
        pool_write(6, i, land_v);
        // nearshore = exp(-((signed / (span*0.045))^2))
        float tn = signed_v / (span * 0.045);
        float nearshore = exp(-(tn * tn));
        pool_write(7, i, nearshore);
        // shelf = smoothstep(span*0.20, -span*0.060, signed)
        pool_write(8, i, ss(span * 0.20, -span * 0.060, signed_v));
        // inland_raw = fbm(w_x, w_z, 1/(span*0.72), 5, sseed+60, gain=0.58)
        float ir = fbm5(w_x, w_z, 1.0 / (span * 0.72), 5, sseed + 60, 0.58);
        pool_write(9, i, ir);
        // headlands_raw = ridged_multifractal(w_x, w_z, 1/(span*0.22), 4, sseed+80, gain=0.52)
        float headlands_raw = rmf(w_x, w_z, 1.0 / (span * 0.22), 4, sseed + 80, 0.52);
        // headlands = smoothstep(0.50, 0.84, headlands_raw)
        float hl = ss(0.50, 0.84, headlands_raw);
        pool_write(10, i, hl);
        // scarp = nearshore * land * (0.55 + 0.75 * headlands)
        float sc = nearshore * land_v * (0.55 + 0.75 * hl);
        pool_write(11, i, sc);
        // inland = clip(affine_remap(inland_raw, INLAND), 0, 1)
        float inl = clip01(affine_remap(ir, INLAND_CENTER, INLAND_SCALE));
        // ridge_source = affine_remap(inland + 0.36*headlands + 0.18*scarp, RIDGE_SOURCE)  (NO clip)
        pool_write(12, i, affine_remap(
            inl + 0.36 * hl + 0.18 * sc,
            RIDGE_SOURCE_CENTER, RIDGE_SOURCE_SCALE));
        return;
    }

    if (pass == CO_FLOW_PRE) {
        // flow_pre <- ridge_source (the channel flow source surface; NO clip).
        flow_pre.v[i] = pool_read(12, i);
        return;
    }

    if (pass == CO_CHANNELS) {
        // gauss_out = spread discharge from flow_channels(ridge_source, width=1.9, power=0.47).
        // channels = smoothstep(0.53, 0.92, channels_raw) * land
        pool_write(13, i, ss(0.53, 0.92, gauss_out.v[i]) * pool_read(6, i));
        return;
    }

    if (pass == CO_CHANNEL_RELIEF) {
        // fjords = channels * nearshore * smoothstep(0.20, 0.80, land)
        // fjord_grooves = ridged_multifractal(rz, rx*0.24, 1/(span*0.11), 4, sseed+120, 0.50)
        // fjord_grooves = smoothstep(0.52, 0.88, fjord_grooves) * land
        //                 * smoothstep(span*0.25, -span*0.01, signed)
        // channel_relief = clip(
        //     channels * (0.34 + 0.34*fjord_gain)
        //     + fjords * fjord_gain
        //     + fjord_grooves * max(fjord_gain - 0.30, 0.0) * 0.44, 0, 1)
        float rx = pool_read(0, i);
        float rz = pool_read(1, i);
        float signed_v = pool_read(4, i);
        float land_v = pool_read(6, i);
        float nearshore = pool_read(7, i);
        float channels = pool_read(13, i);

        float fjords = channels * nearshore * ss(0.20, 0.80, land_v);
        float fg_raw = rmf(rz, rx * 0.24, 1.0 / (span * 0.11), 4, sseed + 120, 0.50);
        float fjord_grooves = ss(0.52, 0.88, fg_raw)
            * land_v
            * ss(span * 0.25, -span * 0.01, signed_v);
        pool_write(14, i, clip01(
            channels * (0.34 + 0.34 * STYLE_FJORD_GAIN)
            + fjords * STYLE_FJORD_GAIN
            + fjord_grooves * max(STYLE_FJORD_GAIN - 0.30, 0.0) * 0.44));
        return;
    }

    if (pass == CO_ISLANDS_SEED) {
        // islands_seed = cellular_edges(w_x, w_z, 1/(span*0.18), sseed+160, sharpness=1.30).
        // Staged into pool12 (free after the flow pass consumed ridge_source); blurred next.
        float w_x = pool_read(2, i);
        float w_z = pool_read(3, i);
        pool_write(12, i, cellular_edges(w_x, w_z, 1.0 / (span * 0.18), sseed + 160, 1.30));
        return;
    }

    if (pass == CO_ISLANDS) {
        // gauss_out = gaussian(islands_seed, sigma=2.0).
        // islands = smoothstep(0.50, 0.86, islands_blur) * sea
        // islands *= smoothstep(span*0.18, -span*0.02, signed)
        float signed_v = pool_read(4, i);
        float sea_v = pool_read(5, i);
        float isl = ss(0.50, 0.86, gauss_out.v[i]) * sea_v;
        pool_write(15, i, isl * ss(span * 0.18, -span * 0.02, signed_v));
        return;
    }

    if (pass == CO_ASSEMBLE) {
        // texture_raw = ridged_multifractal(w_x, w_z, 1/(span*0.050), 4, sseed+220, gain=0.44)
        // texture = affine_remap(texture_raw, TEXTURE)  (NO clip)
        // sea_floor_raw = fbm(w_x, w_z, 1/(span*0.34), 4, sseed+260, gain=0.55)
        // sea_floor = -0.74 - 0.22 * clip(affine_remap(sea_floor_raw, SEA_FLOOR), 0, 1)
        // land_height = 0.68 * affine_remap(inland_raw, INLAND_ZSCORE) + 0.26 * headland_gain * headlands
        // land_height += 0.48 * scarp_gain * scarp
        // land_height -= 0.48 * channel_relief
        // land_height += texture_gain * 0.09 * texture * (0.35 + 0.65*land)
        // height = land*land_height + sea*sea_floor
        // height += island_gain * 0.62 * islands
        // height -= shelf_gain * 0.22 * shelf * sea
        float w_x = pool_read(2, i);
        float w_z = pool_read(3, i);
        float sea_v = pool_read(5, i);
        float land_v = pool_read(6, i);
        float shelf = pool_read(8, i);
        float inland_raw = pool_read(9, i);
        float headlands = pool_read(10, i);
        float scarp = pool_read(11, i);
        float channel_relief = pool_read(14, i);
        float islands = pool_read(15, i);

        float texture_raw = rmf(w_x, w_z, 1.0 / (span * 0.050), 4, sseed + 220, 0.44);
        float texture = affine_remap(texture_raw, TEXTURE_CENTER, TEXTURE_SCALE);

        float sea_floor_raw = fbm5(w_x, w_z, 1.0 / (span * 0.34), 4, sseed + 260, 0.55);
        float sea_floor = -0.74
            - 0.22 * clip01(affine_remap(sea_floor_raw, SEA_FLOOR_CENTER, SEA_FLOOR_SCALE));

        float land_height = 0.68 * affine_remap(inland_raw, INLAND_ZSCORE_CENTER, INLAND_ZSCORE_SCALE)
            + 0.26 * STYLE_HEADLAND_GAIN * headlands;
        land_height += 0.48 * STYLE_SCARP_GAIN * scarp;
        land_height -= 0.48 * channel_relief;
        land_height += STYLE_TEXTURE_GAIN * 0.09 * texture * (0.35 + 0.65 * land_v);

        float hv = land_v * land_height + sea_v * sea_floor;
        hv += STYLE_ISLAND_GAIN * 0.62 * islands;
        hv -= STYLE_SHELF_GAIN * 0.22 * shelf * sea_v;
        height.v[i] = hv;
        return;
    }

    if (pass == CO_SEA_BLEND) {
        // gauss_out = smoothed_sea = gaussian(height, sigma=3.0).
        // height = height*(1 - 0.34*sea) + smoothed_sea*(0.34*sea)
        float sea_v = pool_read(5, i);
        height.v[i] = height.v[i] * (1.0 - 0.34 * sea_v) + gauss_out.v[i] * (0.34 * sea_v);
        return;
    }

    if (pass == CO_FINAL) {
        // gauss_out = height_blur = gaussian(height, 0.9).
        // final_blend = 0.86*height + 0.14*height_blur ; height = affine(final_blend, FINAL)
        float final_blend = 0.86 * height.v[i] + 0.14 * gauss_out.v[i];
        height.v[i] = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
        return;
    }
}
