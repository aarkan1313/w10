//! Karst biome dispatch schedule.

use super::abi::*;
use super::scheduler::Scheduler;

/// The KARST dispatch schedule (style = tower_karst). Mirrors the field DAG of
/// recipes_karst.rs::generate_seamsafe ONE-FOR-ONE: warp+regional -> plateau (blur 5.8) -> towers
/// (raw sparse_pow -> blur 2.0) -> dolines (raw pits_pow -> blur 2.6) -> lineaments (pointwise) ->
/// cellular (raw -> blur 3.8) -> cockpit_noise (pointwise) -> cockpit (pointwise) -> base ->
/// fine/karren (pointwise; REUSE the dead regional/cellular slots) -> dry_valleys (SHARED flow
/// channels, pre-blur 1.15, spread 2.6) -> masks (tower/cockpit/doline/lineament, tower modulated
/// by doline_mask + dry_valleys) -> assemble -> floor mask + blend -> final. All intermediate
/// fields live in the GENERIC scratch POOL (pool0..pool15; pool15 is the transient blur-staging
/// slot, then REUSED for lineament_mask; pool2/pool7 are REUSED for fine/karren post-base; see
/// biome_karst.glsl for the slot map). KARST uses the PROVEN flow_channels (pre-blur 1.15), NOT the
/// flow_channels_ex hook -- its "custom" flow is just power=0.54, width=2.6 (the spread sigma is the
/// existing width param). The sigmas (0.95, 1.15, 2.0, 2.6, 2.8, 3.8, 5.8) are all in karst_sigmas().
/// Same PATTERN as schedule_desert/glacial: pointwise passes write pool slots; blur a slot via
/// gauss_pool(slot,sigma) then read gauss_out; flow channels reuse the proven flow_channels().
pub(crate) fn schedule_karst(s: &mut Scheduler) {
    let tower_width = 2.0_f64.max(0.2);      // tower_width_px.max(0.2) = 2.0
    let doline_width = 2.6_f64.max(0.2);     // doline_width_px.max(0.2) = 2.6
    let floor_smooth = 2.8_f64.max(0.2);     // floor_smooth_px.max(0.2) = 2.8

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; regional=pool2
    s.dispatch_full(KS_POINTWISE, 0, 0, 0.0);

    // 2) plateau = smoothstep(0.30,0.72, gaussian(regional, 5.8))
    s.gauss_pool(2, 5.8);                            // gauss_out = gaussian(regional, 5.8)
    s.dispatch_full(KS_PLATEAU, 0, 0, 0.0);          // pool3 = plateau

    // 3) towers sub-pipeline: sparse_pow (pool15) -> gaussian(2.0) -> clip(affine(., TOWER_FINAL)) = pool4
    s.dispatch_full(KS_TOWER_PRE, 0, 0, 0.0);        // pool15 = sparse_pow
    s.gauss_pool(15, tower_width);                   // gauss_out = gaussian(pool15, 2.0)
    s.dispatch_full(KS_TOWER_FINAL, 0, 0, 0.0);      // pool4 = towers

    // 4) dolines sub-pipeline: pits_pow (pool15) -> gaussian(2.6) -> clip(affine(., DOLINE_BOWLS)) = pool5
    s.dispatch_full(KS_DOLINE_PRE, 0, 0, 0.0);       // pool15 = pits_pow
    s.gauss_pool(15, doline_width);                  // gauss_out = gaussian(pool15, 2.6)
    s.dispatch_full(KS_DOLINE_FINAL, 0, 0, 0.0);     // pool5 = dolines

    // 5) lineaments (pointwise, no blur) = pool6
    s.dispatch_full(KS_LINEAMENTS, 0, 0, 0.0);

    // 6) cellular = gaussian(cellular_edges raw, 3.8)
    s.dispatch_full(KS_CELLULAR_RAW, 0, 0, 0.0);     // pool15 = cellular_raw
    s.gauss_pool(15, 3.8);                           // gauss_out = gaussian(pool15, 3.8)
    s.dispatch_full(KS_CELLULAR, 0, 0, 0.0);         // pool7 = cellular

    // 7) cockpit_noise (pointwise) = pool8 ; cockpit (pointwise, uses dolines/cellular/cockpit_noise) = pool9
    s.dispatch_full(KS_COCKPIT_NOISE, 0, 0, 0.0);    // pool8 = cockpit_noise
    s.dispatch_full(KS_COCKPIT, 0, 0, 0.0);          // pool9 = cockpit

    // 8) base = affine(plateau_gain*(1.06*plateau + 0.18*regional), BASE) = pool10
    s.dispatch_full(KS_BASE, 0, 0, 0.0);

    // 9) fine/karren (pointwise on w_x/w_z); REUSE pool2 (regional dead) = fine, pool7 (cellular dead) = karren
    s.dispatch_full(KS_FINE_KARREN, 0, 0, 0.0);

    // 10) dry_valleys: flow_pre <- base - 0.30*lineaments - 0.10*dolines ; dry_valleys =
    //     flow_channels(width=2.6, power=0.54) [pre-blur 1.15] ; then smoothstep + scale = pool11
    s.dispatch_full(KS_DV_SURFACE, 0, 0, 0.0);       // flow_pre <- dv_surface (NO clip)
    s.flow_channels(0.54_f32, 2.6);                  // gauss_out = spread discharge (sigma=2.6)
    s.dispatch_full(KS_DV_FINAL, 0, 0, 0.0);         // pool11 = dry_valleys

    // 11) masks: cockpit_mask=pool13, doline_mask=pool14, lineament_mask=pool15 (REUSE),
    //     tower_mask=pool12 (finalized w/ doline_mask + dry_valleys)
    s.dispatch_full(KS_MASKS, 0, 0, 0.0);

    // 12) assemble height (base + tower/lineament relief - cockpit/doline/valley + detail)
    s.dispatch_full(KS_ASSEMBLE, 0, 0, 0.0);

    // 13) floor mask + blend: floor_mask = clip(0.72*doline_mask + 0.56*cockpit_mask + 0.48*dry_valleys);
    //     smoothed_floor = gaussian(height, max(floor_smooth_px,0.2)=2.8); floor blend
    s.dispatch_full(KS_FLOOR_MASK, 0, 0, 0.0);       // floor_mask (named buf)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(floor_smooth);                           // gauss_out = gaussian(height, 2.8)
    s.dispatch_full(KS_FLOOR_BLEND, 0, 0, 0.0);

    // 14) final: height_blur = gaussian(height, 0.95); final_blend = 0.80*h + 0.20*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(0.95);                                   // gauss_out = gaussian(height, 0.95)
    s.dispatch_full(KS_FINAL, 0, 0, 0.0);

    // 15) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}
