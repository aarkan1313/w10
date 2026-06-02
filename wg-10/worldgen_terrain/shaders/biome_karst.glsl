// WorldGen10 Slice-4b: KARST biome FRAGMENT (the seam-safe recipe's biome-specific math).
//
// Concatenated AFTER recipe_primitives.glsl + biome_page.glsl (the MACHINE). It defines the
// karst-specific constants + helpers + biome_pass(), implementing the BIOME PASS_* values the
// machine forwards to it. The MACHINE owns all bindings, the `Params` push constant `P`, the
// generic leaf helpers (cell_idx / clamp_idx / affine_remap / ss / clip01 / rotated0 /
// recipe_recursive_domain_warp / rmf / fbm5), the global storage buffers (INCLUDING the generic
// scratch POOL pool0..poolN + the pool_read/pool_write switch), and the generic passes
// (MESHGRID, COPY, COPY_POOL, POOL_FROM_GAUSS, GAUSS_*, FLOW_PRE_*, ACC_INIT, FLOW_RELAX,
// DISCHARGE, CROP). This fragment carries NO #version / #[compute].
//
// This is the GPU mirror of recipes_karst.rs::generate_seamsafe (the f64 parity ORACLE),
// style = STYLES[0] = tower_karst (seed_offset=0 -> sseed=seed; the recipe adds seed+N directly).
// EVERY constant / seed-offset / weight / sigma / rotation / scaling below is transcribed
// VERBATIM from recipes_karst.rs. EDIT-BOTH-SIDES: changes here must keep parity.
//
// KARST DRY-VALLEY FLOW: the dry_valleys drainage is the SHARED flow_channels(power=0.54,
// width=2.6) -- pre-blur sigma=1.15 (the shared path), MFD accumulation power=0.54, log1p
// fixed-max normalize, spread blur sigma=max(2.6,0.1)=2.6, clip [0,1]. NO custom pre-blur (it
// does NOT need flow_channels_ex), so on the Rust side it is the proven flow_channels(0.54, 2.6).
//
// LOCAL PRIMITIVES: karst's tower cone uses ridged_multifractal with an EXPLICIT weight_gain=1.62
// (the machine's rmf() hardcodes 1.35), so a local rmf_wg() is defined here. karst's cellular
// network uses cellular_edges (NOT in recipe_primitives.glsl), so it is copied locally here
// (byte-identical to recipe_noise.rs::cellular_edges, same as biome_desert.glsl / biome_coast.glsl).
//
// SCRATCH-POOL CONTRACT (karst's pool-slot map; needs 16 slots -> POOL_SLOTS 16 covers it):
//   pool0  = w_x          top-level warped X coord (-> regional + ALL warped-coord sub-fields)
//   pool1  = w_z          top-level warped Z coord (-> the same warped-coord sub-fields)
//   pool2  = regional     macro fBm field (-> plateau blur, base) ; then REUSED as fine (post-base)
//   pool3  = plateau      plateau field (-> base + ALL masks)
//   pool4  = towers       tower field (-> tower_mask) ; then REUSED as karren (post tower_mask)
//   pool5  = dolines      doline field (-> dv_surface, doline_mask, cockpit)
//   pool6  = lineaments   lineament field (-> dv_surface, lineament_mask)
//   pool7  = cellular     cellular network (-> cockpit) ; then REUSED as karren? NO -> see fine/karren
//   pool8  = cockpit_noise cockpit fbm (-> cockpit)
//   pool9  = cockpit      cockpit depression field (-> cockpit_mask, floor_mask)
//   pool10 = base         base surface (-> dv_surface, assemble, floor blend)
//   pool11 = dry_valleys  dry-valley drainage mask (-> tower_mask, masks, assemble, floor_mask)
//   pool12 = tower_mask   tower mask (-> assemble)
//   pool13 = cockpit_mask cockpit mask (-> assemble, floor_mask)
//   pool14 = doline_mask  doline mask (-> tower_mask finalize, assemble, floor_mask)
//   pool15 = TRANSIENT    pre-blur staging: sparse_pow / pits_pow / cellular_raw (each consumed by
//                         the very next gaussian, then overwritten) ; then REUSED as lineament_mask
//                         (NO blur) after the cellular blur is done.
// REUSE: pool2 (regional, dead after KS_BASE) -> fine ; pool7 (cellular, dead after KS_COCKPIT) ->
// karren ; pool15 (blur staging, dead after KS_CELLULAR) -> lineament_mask. A biome reads/writes a
// slot with pool_read(slot,i)/pool_write(slot,i,v). To gaussian-blur a slot: PASS_COPY_POOL
// (pool_sel=slot) -> gauss(sigma) -> read gauss_out. The fixed named buffers (0..23) are mountain's;
// karst touches only the GENERIC ones (wx/wz/flow_pre/gauss_*/height/floor_mask) plus the pool.

// ---------------------------------------------------------------------------
// ===== KARST biome-private PASS_* codes (start at 32 per the interface convention) =====
// MUST match biome_page_compute.rs KS_* consts.
// ---------------------------------------------------------------------------
const int KS_POINTWISE     = 32; // warp -> pool0=w_x,pool1=w_z ; regional=pool2
const int KS_PLATEAU       = 33; // pool3 = plateau = smoothstep(0.30,0.72, gauss_out[=gaussian(regional,5.8)])
const int KS_TOWER_PRE     = 34; // pool15 = sparse_pow (tower cone+local, pre gaussian(tower_width_px=2.0))
const int KS_TOWER_FINAL   = 35; // pool4 = towers = clip(affine(gauss_out[=gaussian(pool15,2.0)], TOWER_FINAL))
const int KS_DOLINE_PRE    = 36; // pool15 = pits_pow (doline pits, pre gaussian(doline_width_px=2.6))
const int KS_DOLINE_FINAL  = 37; // pool5 = dolines = clip(affine(gauss_out[=gaussian(pool15,2.6)], DOLINE_BOWLS))
const int KS_LINEAMENTS    = 38; // pool6 = lineaments (pointwise, no blur)
const int KS_CELLULAR_RAW  = 39; // pool15 = cellular_edges raw (pre gaussian(3.8))
const int KS_CELLULAR      = 40; // pool7 = cellular = gauss_out[=gaussian(pool15, 3.8)]
const int KS_COCKPIT_NOISE = 41; // pool8 = cockpit_noise = clip(affine(fbm, COCKPIT_NOISE))
const int KS_COCKPIT       = 42; // pool9 = cockpit = smoothstep(0.52,0.90, clip(affine(combo, COCKPIT)))
const int KS_BASE          = 43; // pool10 = base = affine(plateau_gain*(1.06*plateau + 0.18*regional), BASE)
const int KS_FINE_KARREN   = 44; // pool2 = fine ; pool7 = karren (REUSE regional/cellular slots, post-base)
const int KS_DV_SURFACE    = 45; // flow_pre <- base - 0.30*lineaments - 0.10*dolines (dry-valley flow source)
const int KS_DV_FINAL      = 46; // pool11 = dry_valleys = clip(smoothstep(0.58,0.92, gauss_out)*dv_scale)
const int KS_MASKS         = 47; // pool13=cockpit_mask, pool14=doline_mask, pool15=lineament_mask, pool12=tower_mask
const int KS_ASSEMBLE      = 48; // height = base + tower/lineament - cockpit/doline/valley + detail
const int KS_FLOOR_MASK    = 49; // floor_mask = clip(0.72*doline_mask + 0.56*cockpit_mask + 0.48*dry_valleys)
const int KS_FLOOR_BLEND   = 50; // height = height*(1-0.34*floor_mask) + gauss_out[=gaussian(height,2.8)]*(0.34*floor_mask)
const int KS_FINAL         = 51; // height = affine(0.80*height + 0.20*gauss_out[=gaussian(h,0.95)], FINAL)

// ---------------------------------------------------------------------------
// ===== KARST constants (verbatim from recipes_karst.rs) =====
// ---------------------------------------------------------------------------
// affine-remap constants (replace per-window zscore / norm01)
const float REGIONAL_CENTER      = -0.673;
const float REGIONAL_SCALE       =  0.679;
const float TOWER_CONE_CENTER    =  0.0005;
const float TOWER_CONE_SCALE     =  1.104;
const float TOWER_FINAL_CENTER   =  0.00;
const float TOWER_FINAL_SCALE    =  1.437;
const float DOLINE_PITS_CENTER   =  0.0003;
const float DOLINE_PITS_SCALE    =  1.082;
const float DOLINE_BOWLS_CENTER  =  0.00;
const float DOLINE_BOWLS_SCALE   =  4.274;
const float LINEAMENT_CENTER     =  0.001;
const float LINEAMENT_SCALE      =  1.092;
const float COCKPIT_NOISE_CENTER = -0.880;
const float COCKPIT_NOISE_SCALE  =  0.565;
const float COCKPIT_CENTER       =  0.072;
const float COCKPIT_SCALE        =  1.360;
const float BASE_CENTER          =  0.560;
const float BASE_SCALE           =  2.090;
const float FINE_CENTER          =  0.00;
const float FINE_SCALE           =  3.539;
const float KARREN_CENTER        =  0.356;
const float KARREN_SCALE         =  4.257;
const float FINAL_CENTER         =  0.08;
const float FINAL_SCALE          =  0.964;

// tower_karst style (STYLES[0]) fields the seam-safe pipeline reads.
const float STYLE_ANGLE_RAD      = 0.42;
const float STYLE_PLATEAU_GAIN   = 0.86;
const float STYLE_TOWER_GAIN     = 1.45;
const float STYLE_COCKPIT_GAIN   = 1.02;
const float STYLE_DOLINE_GAIN    = 0.82;
const float STYLE_VALLEY_GAIN    = 0.62;
const float STYLE_LINEAMENT_GAIN = 0.74;
const float STYLE_TOWER_WIDTH_PX = 2.0;
const float STYLE_DOLINE_WIDTH_PX= 2.6;
const float STYLE_FLOOR_SMOOTH_PX= 2.8;
const float STYLE_DETAIL_GAIN    = 0.54;
const float STYLE_ANISOTROPY     = 0.48;
// seed_offset = 0 (sseed = P.seed + 0; mirrored explicitly).
const int   STYLE_SEED_OFFSET    = 0;

// ---------------------------------------------------------------------------
// rmf_wg: ridged_multifractal with an EXPLICIT weight_gain (the tower cone uses 1.62, NOT the
// machine rmf()'s fixed 1.35). Mirror of recipes_karst.rs::ridged_mf_wg (offset=1.0, lac=2.0).
// ---------------------------------------------------------------------------
float rmf_wg(float x, float z, float base_freq, int octaves, int seed, float gain, float weight_gain) {
    return ridged_multifractal(x, z, base_freq, octaves, seed, gain, 2.0, 1.0, weight_gain);
}

// ---------------------------------------------------------------------------
// cellular_edges: cheap Worley/cellular edge network -> [0,1], high near cell borders. Mirror of
// recipe_noise.rs::cellular_edges(wx,wz,freq,seed,sharpness). NOT in the primitives file, so
// defined locally here (byte-identical to biome_desert.glsl / biome_coast.glsl). The feature
// offsets use hash2(cx,cz,seed+11)/seed+29; the karst call uses freq=1/(span*0.145) -> grid
// indices ix/iz stay small (well within i32, matching the 32-bit-seed GLSL hash2).
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
// lineaments_point: mirror of recipes_karst.rs::lineaments_point (_lineaments seam_safe). Rotates
// (wx,wz) about world origin, builds two ridged lines, combines + affine_remap + smoothstep.
// ---------------------------------------------------------------------------
float lineaments_point(float wx_in, float wz_in, float span, int seed) {
    vec2 r = rotated0(wx_in, wz_in, STYLE_ANGLE_RAD);
    float rx = r.x;
    float rz = r.y;
    // line_a = ridged_multifractal(rx, rz*anisotropy, 1/(span*0.18), 4, seed+100, 0.54)
    float line_a = rmf(rx, rz * STYLE_ANISOTROPY, 1.0 / (span * 0.18), 4, seed + 100, 0.54);
    // line_b = ridged_multifractal(rx*0.58 - rz*0.32, rz*0.58 + rx*0.32, 1/(span*0.11), 3, seed+130, 0.48)
    float line_b = rmf(rx * 0.58 - rz * 0.32, rz * 0.58 + rx * 0.32, 1.0 / (span * 0.11), 3, seed + 130, 0.48);
    float combo = 0.68 * line_a + 0.32 * line_b;
    // smoothstep(0.46, 0.82, clip(affine_remap(combo, LINEAMENT), 0, 1))
    return ss(0.46, 0.82, clip01(affine_remap(combo, LINEAMENT_CENTER, LINEAMENT_SCALE)));
}

// ---------------------------------------------------------------------------
// biome_pass: the karst-specific PASS bodies. The machine has already handled the generic passes +
// guards; (cx,cy,i) are the cell coords and linear index for this invocation (cx<cols, cy<rows
// guaranteed by the machine). All pool access goes through pool_read/pool_write (defined in the
// machine). EDIT-BOTH-SIDES with recipes_karst.rs::generate_seamsafe.
// ---------------------------------------------------------------------------
void biome_pass(int pass, int cx, int cy, int i) {
    float span = max(P.feature_span_m, 1.0);
    int sseed = P.seed + STYLE_SEED_OFFSET;

    if (pass == KS_POINTWISE) {
        // w_x, w_z = recursive_domain_warp(wx, wz, span*0.035, 1/(span*0.62), sseed+10, 3, 0.55, 1.82)
        vec2 w = recipe_recursive_domain_warp(
            wx.v[i], wz.v[i],
            span * 0.035, 1.0 / (span * 0.62),
            sseed + 10, 3, 0.55, 1.82);
        float w_x = w.x;
        float w_z = w.y;
        pool_write(0, i, w_x);
        pool_write(1, i, w_z);
        // regional = clip(affine(fbm(w_x,w_z, 1/(span*0.74),5,sseed+30,0.56), REGIONAL), 0, 1)
        float reg = fbm5(w_x, w_z, 1.0 / (span * 0.74), 5, sseed + 30, 0.56);
        pool_write(2, i, clip01(affine_remap(reg, REGIONAL_CENTER, REGIONAL_SCALE)));
        return;
    }

    if (pass == KS_PLATEAU) {
        // plateau = smoothstep(0.30, 0.72, gaussian(regional, 5.8))  [gauss_out]
        pool_write(3, i, ss(0.30, 0.72, gauss_out.v[i]));
        return;
    }

    if (pass == KS_TOWER_PRE) {
        // _tower_field sparse_pow raw (pre gaussian(max(tower_width_px,0.2)=2.0)).
        // cone = ridged_mf_wg(w_x,w_z, 1/(span*0.055), 5, sseed+210, 0.52, weight_gain=1.62)
        // local = ridged_multifractal(w_x,w_z, 1/(span*0.026), 3, sseed+240, 0.45)
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        float cone = rmf_wg(w_x, w_z, 1.0 / (span * 0.055), 5, sseed + 210, 0.52, 1.62);
        float local = rmf(w_x, w_z, 1.0 / (span * 0.026), 3, sseed + 240, 0.45);
        float combo = 0.78 * cone + 0.22 * local;
        // sparse = smoothstep(0.46, 0.84, clip(affine_remap(combo, TOWER_CONE), 0, 1))
        float sparse = ss(0.46, 0.84, clip01(affine_remap(combo, TOWER_CONE_CENTER, TOWER_CONE_SCALE)));
        pool_write(15, i, pow(sparse, 1.20));
        return;
    }

    if (pass == KS_TOWER_FINAL) {
        // towers = clip(affine_remap(gaussian(sparse_pow, 2.0), TOWER_FINAL), 0, 1)  [gauss_out]
        pool_write(4, i, clip01(affine_remap(gauss_out.v[i], TOWER_FINAL_CENTER, TOWER_FINAL_SCALE)));
        return;
    }

    if (pass == KS_DOLINE_PRE) {
        // _doline_field pits_pow raw (pre gaussian(max(doline_width_px,0.2)=2.6)).
        // pits_a = ridged_multifractal(w_x,w_z, 1/(span*0.040), 4, sseed+310, 0.50)
        // pits_b = ridged_multifractal(w_x + 0.31*w_z, w_z - 0.17*w_x, 1/(span*0.022), 3, sseed+330, 0.46)
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        float pits_a = rmf(w_x, w_z, 1.0 / (span * 0.040), 4, sseed + 310, 0.50);
        float pits_b = rmf(w_x + 0.31 * w_z, w_z - 0.17 * w_x, 1.0 / (span * 0.022), 3, sseed + 330, 0.46);
        float combo = 0.66 * pits_a + 0.34 * pits_b;
        // pits = smoothstep(0.55, 0.90, clip(affine_remap(combo, DOLINE_PITS), 0, 1))
        float pits = ss(0.55, 0.90, clip01(affine_remap(combo, DOLINE_PITS_CENTER, DOLINE_PITS_SCALE)));
        pool_write(15, i, pow(pits, 1.45));
        return;
    }

    if (pass == KS_DOLINE_FINAL) {
        // dolines = clip(affine_remap(gaussian(pits_pow, 2.6), DOLINE_BOWLS), 0, 1)  [gauss_out]
        pool_write(5, i, clip01(affine_remap(gauss_out.v[i], DOLINE_BOWLS_CENTER, DOLINE_BOWLS_SCALE)));
        return;
    }

    if (pass == KS_LINEAMENTS) {
        // lineaments = lineaments_point(w_x, w_z, span, sseed)  (pointwise, no blur)
        pool_write(6, i, lineaments_point(pool_read(0, i), pool_read(1, i), span, sseed));
        return;
    }

    if (pass == KS_CELLULAR_RAW) {
        // cellular_raw = cellular_edges(w_x, w_z, 1/(span*0.145), sseed+160, 1.45)  (pre gaussian(3.8))
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        pool_write(15, i, cellular_edges(w_x, w_z, 1.0 / (span * 0.145), sseed + 160, 1.45));
        return;
    }

    if (pass == KS_CELLULAR) {
        // cellular = gaussian(cellular_raw, 3.8)  [gauss_out]
        pool_write(7, i, gauss_out.v[i]);
        return;
    }

    if (pass == KS_COCKPIT_NOISE) {
        // cockpit_noise = clip(affine(fbm(w_x,w_z, 1/(span*0.052),4,sseed+180,0.54), COCKPIT_NOISE), 0, 1)
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        float cn = fbm5(w_x, w_z, 1.0 / (span * 0.052), 4, sseed + 180, 0.54);
        pool_write(8, i, clip01(affine_remap(cn, COCKPIT_NOISE_CENTER, COCKPIT_NOISE_SCALE)));
        return;
    }

    if (pass == KS_COCKPIT) {
        // inner = 0.50*dolines + 0.26*(1 - cellular) + 0.24*cockpit_noise
        // cockpit = smoothstep(0.52, 0.90, clip(affine_remap(inner, COCKPIT), 0, 1))
        float dolines = pool_read(5, i);
        float cellular = pool_read(7, i);
        float cockpit_noise = pool_read(8, i);
        float inner = 0.50 * dolines + 0.26 * (1.0 - cellular) + 0.24 * cockpit_noise;
        pool_write(9, i, ss(0.52, 0.90, clip01(affine_remap(inner, COCKPIT_CENTER, COCKPIT_SCALE))));
        return;
    }

    if (pass == KS_BASE) {
        // base = affine(plateau_gain*(1.06*plateau + 0.18*regional), BASE)
        float inner = STYLE_PLATEAU_GAIN * (1.06 * pool_read(3, i) + 0.18 * pool_read(2, i));
        pool_write(10, i, affine_remap(inner, BASE_CENTER, BASE_SCALE));
        return;
    }

    if (pass == KS_FINE_KARREN) {
        // fine = affine(fbm(w_x,w_z, 1/(span*0.018),4,sseed+410,0.48), FINE)  -> REUSE pool2 (regional dead)
        // karren = affine(ridged_multifractal(w_x,w_z, 1/(span*0.016),3,sseed+430,0.46), KARREN) -> REUSE pool7 (cellular dead)
        float w_x = pool_read(0, i);
        float w_z = pool_read(1, i);
        float f = fbm5(w_x, w_z, 1.0 / (span * 0.018), 4, sseed + 410, 0.48);
        pool_write(2, i, affine_remap(f, FINE_CENTER, FINE_SCALE));
        float k = rmf(w_x, w_z, 1.0 / (span * 0.016), 3, sseed + 430, 0.46);
        pool_write(7, i, affine_remap(k, KARREN_CENTER, KARREN_SCALE));
        return;
    }

    if (pass == KS_DV_SURFACE) {
        // dry_valleys = _dry_valleys_seam_safe(base - 0.30*lineaments - 0.10*dolines, power=0.54).
        // flow_pre <- dv_surface ; the generic flow machinery (pre-blur 1.15 + relax + discharge +
        // spread 2.6) runs next via flow_channels(0.54, 2.6).
        flow_pre.v[i] = pool_read(10, i) - 0.30 * pool_read(6, i) - 0.10 * pool_read(5, i);
        return;
    }

    if (pass == KS_DV_FINAL) {
        // dry_valleys = smoothstep(0.58, 0.92, discharge[gauss_out])
        // dry_valleys = clip(dry_valleys * (0.72 + 0.28*valley_gain), 0, 1)
        float dv_scale = 0.72 + 0.28 * STYLE_VALLEY_GAIN;
        float s = ss(0.58, 0.92, gauss_out.v[i]);
        pool_write(11, i, clip01(s * dv_scale));
        return;
    }

    if (pass == KS_MASKS) {
        // tower_mask    = smoothstep(0.22,0.74,towers) * (0.50 + 0.50*plateau)
        // cockpit_mask  = smoothstep(0.46,0.86,cockpit) * (0.35 + 0.65*plateau)
        // doline_mask   = smoothstep(0.46,0.88,dolines) * (0.30 + 0.70*plateau)
        // lineament_mask= clip(lineament_gain * lineaments * (0.35 + 0.65*plateau), 0, 1)
        // tower_mask    = tower_mask * (1 - 0.50*doline_mask) * (1 - 0.30*dry_valleys)
        float pl = pool_read(3, i);
        float towers = pool_read(4, i);
        float dolines = pool_read(5, i);
        float lineaments = pool_read(6, i);
        float cockpit = pool_read(9, i);
        float dry_valleys = pool_read(11, i);

        float tm = ss(0.22, 0.74, towers) * (0.50 + 0.50 * pl);
        float cockpit_mask = ss(0.46, 0.86, cockpit) * (0.35 + 0.65 * pl);
        float doline_mask = ss(0.46, 0.88, dolines) * (0.30 + 0.70 * pl);
        float lineament_mask = clip01(STYLE_LINEAMENT_GAIN * lineaments * (0.35 + 0.65 * pl));
        tm = tm * (1.0 - 0.50 * doline_mask) * (1.0 - 0.30 * dry_valleys);

        pool_write(12, i, tm);            // tower_mask
        pool_write(13, i, cockpit_mask);
        pool_write(14, i, doline_mask);
        pool_write(15, i, lineament_mask); // REUSE pool15 (blur staging dead after KS_CELLULAR)
        return;
    }

    if (pass == KS_ASSEMBLE) {
        // height = base
        // height += tower_gain*(0.84*tower_mask + 0.20*tower_mask*karren)
        // height += lineament_gain*0.20*lineament_mask
        // height -= cockpit_gain*0.26*cockpit_mask
        // height -= doline_gain*0.72*doline_mask
        // height -= valley_gain*0.40*dry_valleys
        // height += detail_gain*(0.08 + 0.24*tower_mask + 0.10*lineament_mask)*fine
        float base = pool_read(10, i);
        float tm = pool_read(12, i);
        float cockpit_mask = pool_read(13, i);
        float doline_mask = pool_read(14, i);
        float lineament_mask = pool_read(15, i);
        float dry_valleys = pool_read(11, i);
        float karren = pool_read(7, i);   // REUSED slot (fine/karren)
        float fine = pool_read(2, i);     // REUSED slot (fine)

        float hv = base;
        hv += STYLE_TOWER_GAIN * (0.84 * tm + 0.20 * tm * karren);
        hv += STYLE_LINEAMENT_GAIN * 0.20 * lineament_mask;
        hv -= STYLE_COCKPIT_GAIN * 0.26 * cockpit_mask;
        hv -= STYLE_DOLINE_GAIN * 0.72 * doline_mask;
        hv -= STYLE_VALLEY_GAIN * 0.40 * dry_valleys;
        hv += STYLE_DETAIL_GAIN * (0.08 + 0.24 * tm + 0.10 * lineament_mask) * fine;
        height.v[i] = hv;
        return;
    }

    if (pass == KS_FLOOR_MASK) {
        // floor_mask = clip(0.72*doline_mask + 0.56*cockpit_mask + 0.48*dry_valleys, 0, 1)
        float doline_mask = pool_read(14, i);
        float cockpit_mask = pool_read(13, i);
        float dry_valleys = pool_read(11, i);
        floor_mask.v[i] = clip01(0.72 * doline_mask + 0.56 * cockpit_mask + 0.48 * dry_valleys);
        return;
    }

    if (pass == KS_FLOOR_BLEND) {
        // smoothed_floor = gaussian(height, max(floor_smooth_px,0.2)=2.8)  [gauss_out]
        // height = height*(1 - 0.34*floor_mask) + smoothed_floor*(0.34*floor_mask)
        float fm = floor_mask.v[i];
        height.v[i] = height.v[i] * (1.0 - 0.34 * fm) + gauss_out.v[i] * (0.34 * fm);
        return;
    }

    if (pass == KS_FINAL) {
        // final_blend = 0.80*height + 0.20*gaussian(height, 0.95)[gauss_out]
        // height = affine_remap(final_blend, FINAL)
        float final_blend = 0.80 * height.v[i] + 0.20 * gauss_out.v[i];
        height.v[i] = affine_remap(final_blend, FINAL_CENTER, FINAL_SCALE);
        return;
    }
}
