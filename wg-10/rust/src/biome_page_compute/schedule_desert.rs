//! Desert biome dispatch schedule.

use super::abi::*;
use super::scheduler::Scheduler;

/// The DESERT dispatch schedule (style = DUNE_SEA). Mirrors the field DAG of
/// recipes_desert.rs::generate_seamsafe ONE-FOR-ONE: warp+regional -> basin (blur 1-regional) ->
/// playa (blur basin) -> dunes (whole-field sub-pipeline) -> yardangs (pointwise) ->
/// block_cores/mesas -> base_surface -> washes (flow channels) -> fine/salt -> assemble ->
/// floor blend -> final. All intermediate fields live in the GENERIC scratch POOL (pool0..pool15;
/// see biome_desert.glsl for the slot map). The sigmas (6.2, 5.0, 0.70, 3.2, 2.2, flow pre-blur
/// 1.15 + spread 1.8, floor 5.2, final 0.95) are all in desert_sigmas(). Same PATTERN as
/// schedule_grassland: pointwise passes write pool slots; blur a slot via gauss_pool(slot,sigma)
/// then read gauss_out; flow channels reuse the proven flow_channels().
pub(crate) fn schedule_desert(s: &mut Scheduler) {
    let floor_smooth = 5.2_f64.max(0.2);

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; regional=pool2
    s.dispatch_full(DS_POINTWISE, 0, 0, 0.0);

    // 2) basin = smoothstep(0.34,0.78, 1 - gaussian(regional, 6.2))
    s.gauss_pool(2, 6.2);                            // gauss_out = gaussian(regional, 6.2)
    s.dispatch_full(DS_BASIN, 0, 0, 0.0);            // pool3 = basin

    // 3) playa = smoothstep(0.56,0.90, gaussian(basin, 5.0))
    s.gauss_pool(3, 5.0);                            // gauss_out = gaussian(basin, 5.0)
    s.dispatch_full(DS_PLAYA, 0, 0, 0.0);            // pool4 = playa

    // 4) dunes sub-pipeline: raw (pool15) -> gaussian(0.70) -> clip(affine(., DUNE)) = pool5
    s.dispatch_full(DS_DUNE_PRE, 0, 0, 0.0);         // pool15 = dune raw
    s.gauss_pool(15, 0.70);                          // gauss_out = gaussian(pool15, 0.70)
    s.dispatch_full(DS_DUNE_FINAL, 0, 0, 0.0);       // pool5 = dunes

    // 5) yardangs (pointwise, no blur) = pool6
    s.dispatch_full(DS_YARDANG, 0, 0, 0.0);

    // 6) block_cores: pre (pool12=1-block_edges, pool13=rocky_relief) -> gaussian(3.2) -> pool14
    s.dispatch_full(DS_BLOCK_PRE, 0, 0, 0.0);        // pool12 = 1-block_edges ; pool13 = rocky_relief
    s.gauss_pool(12, 3.2);                           // gauss_out = gaussian(1-block_edges, 3.2)
    s.dispatch_full(DS_BLOCK_CORES, 0, 0, 0.0);      // pool14 = block_cores

    // 7) mesas = clip(0.68*mesa_blocks + 0.32*rocky_relief*(1-0.42*basin)); mesa_blocks uses
    //    gaussian(regional, 2.2) * block_cores * (1-0.68*basin)
    s.gauss_pool(2, 2.2);                            // gauss_out = gaussian(regional, 2.2)
    s.dispatch_full(DS_MESAS, 0, 0, 0.0);            // pool7 = mesas

    // 8) base_surface = affine(0.72*regional + 0.24*mesas - 0.62*basin, BASE) = pool8
    s.dispatch_full(DS_BASE, 0, 0, 0.0);

    // 9) washes = smoothstep(0.57,0.94, flow_channels(base_surface+0.16*mesas, width=1.8,
    //    power=0.43)) * (0.35 + 0.65*(1 - playa))    [flow_channels leaves spread in gauss_out]
    s.dispatch_full(DS_WASH_FLOW_PRE, 0, 0, 0.0);    // flow_pre <- base_surface + 0.16*mesas
    s.flow_channels(0.43_f32, 1.8);
    s.dispatch_full(DS_WASH_FINAL, 0, 0, 0.0);       // pool9 = washes

    // 10) fine (pool10) + salt (pool11), pointwise on w_x/w_z
    s.dispatch_full(DS_FINE_SALT, 0, 0, 0.0);

    // 11) assemble height (base + dune/yardang/wash/playa/mesa relief + detail)
    s.dispatch_full(DS_ASSEMBLE, 0, 0, 0.0);

    // 12) floor blend: smooth_floor = gaussian(height, max(floor_smooth_px,0.2)=5.2); floor blend
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(floor_smooth);                           // gauss_out = gaussian(height, floor_smooth)
    s.dispatch_full(DS_FLOOR_BLEND, 0, 0, 0.0);

    // 13) final: height_blur = gaussian(height, 0.95); final_blend = 0.82*h + 0.18*blur; affine
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(0.95);                                   // gauss_out = gaussian(height, 0.95)
    s.dispatch_full(DS_FINAL, 0, 0, 0.0);

    // 14) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}
