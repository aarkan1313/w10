//! Glacial biome dispatch schedule.

use super::abi::*;
use super::scheduler::Scheduler;

/// The GLACIAL dispatch schedule (style = fjorded_troughs). Mirrors the field DAG of
/// recipes_glacial.rs::generate_seamsafe ONE-FOR-ONE: warp+regional/ridge_detail/close_detail ->
/// oriented_relief (raw -> blur 1.25) -> relief_env (blur 5.8) -> icefield (blur 7.0) -> massif
/// (raw -> blur 2.8) -> base -> flow_primary (TROUGH flow, pre-blur 1.85) -> axial (raw -> blur
/// 1.224) -> primary_mask -> branch_surface (uses gaussian(primary_mask,1.6)) -> tributary (TROUGH
/// flow, pre-blur 1.85) + trib_mask -> scrapes -> assemble -> floor/ice masks + blends -> final.
/// All intermediate fields live in the GENERIC scratch POOL (pool0..pool15; pool15 is the transient
/// pre-blur staging slot; pool10/pool11/pool7 are REUSED post-mask; see biome_glacial.glsl for the
/// slot map). GLACIAL DIVERGENCE: its trough flow uses flow_channels_ex(power, width, 1.85) (the
/// machine-hook), NOT the shared flow_channels (1.15) -- 1.85 is in glacial_sigmas(). The sigmas
/// (1.224, 1.25, 1.35, 1.6, 1.85, 2.8, 3.264, 4.03, 5.8, 6.2, 6.8, 7.0) are all in glacial_sigmas().
/// Same PATTERN as schedule_tundra: pointwise passes write pool slots; blur a slot via
/// gauss_pool(slot,sigma) then read gauss_out (or POOL_FROM_GAUSS to stash); flow channels reuse
/// the proven flow_channels_ex().
pub(crate) fn schedule_glacial(s: &mut Scheduler) {
    let trough_width_px = 6.8_f64;
    let axial_sigma = (trough_width_px * 0.18).max(0.8);    // 1.224
    let primary_spread = trough_width_px;                    // 6.8 (flow_channels width.max(0.1)=6.8)
    let trib_width = (trough_width_px * 0.48).max(0.8);      // 3.264
    let ice_smooth_px = 6.2_f64;
    let floor_smooth = ice_smooth_px.max(0.2);              // 6.2
    let ice_smooth = (ice_smooth_px * 0.65).max(0.2);       // 4.03

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; regional=pool2 ; ridge_detail=pool3 ; close_detail=pool4
    s.dispatch_full(GC_POINTWISE, 0, 0, 0.0);

    // 2) relief = gaussian(oriented_relief raw, 1.25)
    s.dispatch_full(GC_RELIEF_RAW, 0, 0, 0.0);       // pool15 = oriented_relief raw
    s.gauss_pool(15, 1.25);                          // gauss_out = gaussian(pool15, 1.25)
    s.dispatch_full(GC_RELIEF, 0, 0, 0.0);           // pool5 = relief

    // 3) relief_env = smoothstep(0.22,0.62, gaussian(relief, 5.8))
    s.gauss_pool(5, 5.8);                            // gauss_out = gaussian(relief, 5.8)
    s.dispatch_full(GC_RELIEF_ENV, 0, 0, 0.0);       // pool6 = relief_env

    // 4) icefield = smoothstep(0.48,0.78, gaussian(0.56*regional + 0.44*relief_env, 7.0))
    s.dispatch_full(GC_ICE_INNER, 0, 0, 0.0);        // gauss_in <- ice_inner
    s.gauss(7.0);                                    // gauss_out = gaussian(ice_inner, 7.0)
    s.dispatch_full(GC_ICEFIELD, 0, 0, 0.0);         // pool7 = icefield

    // 5) massif = gaussian(massif_inner, 2.8)
    s.dispatch_full(GC_MASSIF_INNER, 0, 0, 0.0);     // pool15 = massif_inner
    s.gauss_pool(15, 2.8);                           // gauss_out = gaussian(pool15, 2.8)
    s.dispatch_full(GC_MASSIF, 0, 0, 0.0);           // pool8 = massif

    // 6) base = affine(uplift*(1.34*massif + 0.22*relief - 0.16*(1-icefield)), BASE)
    s.dispatch_full(GC_BASE, 0, 0, 0.0);             // pool9 = base

    // 7) flow_primary = trough_channels_seam_safe(base, width=6.8, power=0.58, PRE-BLUR 1.85)
    s.dispatch_full(GC_FLOW_PRE_PRIMARY, 0, 0, 0.0); // flow_pre <- base
    s.flow_channels_ex(0.58_f32, primary_spread, 1.85); // gauss_out = spread discharge (sigma=6.8)
    s.dispatch_full(GC_FLOW_PRIMARY_STASH, 0, 0, 0.0); // pool10 = flow_primary

    // 8) axial = gaussian(axial_troughs raw, max(trough_width_px*0.18, 0.8) = 1.224)
    s.dispatch_full(GC_AXIAL_RAW, 0, 0, 0.0);        // pool15 = axial raw
    s.gauss_pool(15, axial_sigma);                   // gauss_out = gaussian(pool15, 1.224)
    s.dispatch_full(GC_AXIAL, 0, 0, 0.0);            // pool11 = axial

    // 9) primary_mask = smoothstep(0.34,0.84, clip(affine(0.58*flow_primary + 1.18*axial, PRIMARY)))
    s.dispatch_full(GC_PRIMARY_MASK, 0, 0, 0.0);     // pool12 = primary_mask

    // 10) tributary = trough_channels_seam_safe(branch_surface, width=3.264, power=0.36, PRE-BLUR 1.85)
    //     branch_surface = base + 0.10*affine(relief,RELIEF_ZSCORE) - 0.18*gaussian(primary_mask,1.6)
    s.gauss_pool(12, 1.6);                           // gauss_out = gaussian(primary_mask, 1.6)
    s.dispatch_full(GC_BRANCH_SURFACE, 0, 0, 0.0);   // flow_pre <- branch_surface (uses gauss_out)
    s.flow_channels_ex(0.36_f32, trib_width, 1.85);  // gauss_out = spread discharge (sigma=3.264)
    s.dispatch_full(GC_TRIB_MASK, 0, 0, 0.0);        // pool13 = tributary_mask

    // 11) scrapes = striations raw (pointwise, no blur)
    s.dispatch_full(GC_SCRAPES, 0, 0, 0.0);          // pool14 = scrapes

    // 12) assemble height (base + ridge/detail/striation - trough - branch; trough_floor->pool10,
    //     high_ice->pool11)
    s.dispatch_full(GC_ASSEMBLE, 0, 0, 0.0);

    // 13) floor/ice masks: floor_mask = clip(smoothstep(0.36,0.80, gaussian(trough_floor,1.6)));
    //     ice_mask = clip(smoothstep(0.50,0.90, high_ice)) -> pool7
    s.gauss_pool(10, 1.6);                           // gauss_out = gaussian(trough_floor, 1.6)
    s.dispatch_full(GC_FLOOR_MASK, 0, 0, 0.0);       // floor_mask (named buf) ; pool7 = ice_mask

    // 14) floor blend: floor = gaussian(height, max(ice_smooth_px,0.2)=6.2); blend by floor_mask
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(floor_smooth);                           // gauss_out = gaussian(height, 6.2)
    s.dispatch_full(GC_FLOOR_BLEND, 0, 0, 0.0);

    // 15) ice blend: ice_smooth = gaussian(height, max(ice_smooth_px*0.65,0.2)=4.03); blend by
    //     ice_mask; then height -= 0.16*floor_mask
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(ice_smooth);                             // gauss_out = gaussian(height, 4.03)
    s.dispatch_full(GC_ICE_BLEND, 0, 0, 0.0);

    // 16) final: height_blur = gaussian(height, 1.35); final_blend = 0.66*h + 0.34*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(1.35);                                   // gauss_out = gaussian(height, 1.35)
    s.dispatch_full(GC_FINAL, 0, 0, 0.0);

    // 17) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}
