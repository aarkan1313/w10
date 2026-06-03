//! Coast biome dispatch schedule.

use super::abi::*;
use super::scheduler::Scheduler;

/// The COAST dispatch schedule (style = CLIFFED_HEADLANDS). Mirrors the field DAG of
/// recipes_coast.rs::generate_seamsafe ONE-FOR-ONE: rotation+warp pointwise (rx/rz/w_x/w_z + the
/// sea/land/nearshore/shelf/inland/headlands/scarp masks + ridge_source) -> channels (flow on
/// ridge_source) -> channel_relief (fjords + grooves) -> islands (cellular_edges seed blurred) ->
/// assemble (texture/sea_floor computed inline) -> sea-smoothing blend -> final. All intermediate
/// fields live in the GENERIC scratch POOL (pool0..pool15; see biome_coast.glsl for the slot map).
/// pool12 is REUSED: it holds ridge_source (consumed by the flow pass) then stages islands_seed.
/// The sigmas (flow pre-blur 1.15 + spread 1.9, islands 2.0, sea 3.0, final 0.9) are all in
/// coast_sigmas(). Same PATTERN as schedule_grassland/desert: pointwise passes write pool slots;
/// blur a slot via gauss_pool(slot,sigma) then read gauss_out; flow channels reuse flow_channels().
pub(crate) fn schedule_coast(s: &mut Scheduler) {
    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: rotation -> pool0=rx, pool1=rz ; warp -> pool2=w_x, pool3=w_z ;
    //    signed=pool4 ; sea=pool5 ; land=pool6 ; nearshore=pool7 ; shelf=pool8 ;
    //    inland_raw=pool9 ; headlands=pool10 ; scarp=pool11 ; ridge_source=pool12
    s.dispatch_full(CO_POINTWISE, 0, 0, 0.0);

    // 2) channels = smoothstep(0.53,0.94->0.92, flow_channels(ridge_source, width=1.9, power=0.47))
    //    * land    [flow_channels leaves spread discharge in gauss_out]
    s.dispatch_full(CO_FLOW_PRE, 0, 0, 0.0);         // flow_pre <- ridge_source (pool12)
    s.flow_channels(0.47_f32, 1.9);
    s.dispatch_full(CO_CHANNELS, 0, 0, 0.0);         // pool13 = channels

    // 3) channel_relief = clip(channels + fjords + fjord_grooves combo) (pointwise)
    s.dispatch_full(CO_CHANNEL_RELIEF, 0, 0, 0.0);   // pool14 = channel_relief

    // 4) islands sub-pipeline: islands_seed (pool12, reused) -> gaussian(2.0) ->
    //    smoothstep(0.50,0.86)*sea*smoothstep(...) = pool15
    s.dispatch_full(CO_ISLANDS_SEED, 0, 0, 0.0);     // pool12 = islands_seed (cellular_edges)
    s.gauss_pool(12, 2.0);                           // gauss_out = gaussian(islands_seed, 2.0)
    s.dispatch_full(CO_ISLANDS, 0, 0, 0.0);          // pool15 = islands

    // 5) assemble height (land*land_height + sea*sea_floor + islands - shelf; texture/sea_floor
    //    computed inline in the pass from w_x/w_z)
    s.dispatch_full(CO_ASSEMBLE, 0, 0, 0.0);

    // 6) sea-smoothing blend: smoothed_sea = gaussian(height, 3.0); sea-weighted blend
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(3.0);                                    // gauss_out = gaussian(height, 3.0)
    s.dispatch_full(CO_SEA_BLEND, 0, 0, 0.0);

    // 7) final: height_blur = gaussian(height, 0.9); final_blend = 0.86*h + 0.14*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(0.9);                                    // gauss_out = gaussian(height, 0.9)
    s.dispatch_full(CO_FINAL, 0, 0, 0.0);

    // 8) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}
