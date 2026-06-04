//! Wetland biome dispatch schedule.

use super::abi::*;
use super::scheduler::Scheduler;

/// The WETLAND dispatch schedule (style = delta_distributary). Mirrors the field DAG of
/// recipes_wetland.rs::generate_seamsafe ONE-FOR-ONE: warp+macro/micro/meander -> basin (blur
/// 1-macro) -> floodplain (blur 1-|macro-0.42|) -> channels (meander*floodplain) -> fine_flow
/// (flow channels on flow_input) -> channels reassigned -> levees (DoG of channels) -> flat_base
/// (blur of affine combo) -> assemble -> final. All intermediate fields live in the GENERIC
/// scratch POOL (pool0..pool10; see biome_wetland.glsl for the slot map). pool8 is TRANSIENT
/// (stages gaussian(channels,2.2) for the levee DoG). The sigmas (5.8, 5.2, flow pre-blur 1.15 +
/// spread 1.8, 2.2, smoothing_px=4.4, final 1.2) are all in wetland_sigmas(). Same PATTERN as
/// schedule_grassland/desert/coast: pointwise passes write pool slots; blur a slot via
/// gauss_pool(slot,sigma) then read gauss_out (or POOL_FROM_GAUSS to stash); flow channels reuse
/// the proven flow_channels().
pub(crate) fn schedule_wetland(s: &mut Scheduler) {
    let smoothing_px = 4.4_f64;          // delta_distributary.smoothing_px (flat_base blur)

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; macro_f=pool2 ; micro=pool3 ; meander=pool4
    s.dispatch_full(WL_POINTWISE, 0, 0, 0.0);

    // 2) basin = smoothstep(0.48,0.86, gaussian(1 - macro, 5.8))
    s.dispatch_full(WL_ONE_MINUS_MACRO, 0, 0, 0.0); // gauss_in <- 1 - macro_f
    s.gauss(5.8);                                    // gauss_out = gaussian(1-macro, 5.8)
    s.dispatch_full(WL_BASIN, 0, 0, 0.0);            // pool5 = basin

    // 3) floodplain = smoothstep(0.36,0.78, gaussian(1 - |macro-0.42|, 5.2))
    s.dispatch_full(WL_FLOODPLAIN_PRE, 0, 0, 0.0);   // gauss_in <- 1 - |macro_f - 0.42|
    s.gauss(5.2);                                    // gauss_out = gaussian(., 5.2)
    s.dispatch_full(WL_FLOODPLAIN, 0, 0, 0.0);       // pool6 = floodplain

    // 4) channels = meander * floodplain (first assignment)
    s.dispatch_full(WL_CHANNELS_FIRST, 0, 0, 0.0);   // pool7 = channels

    // 5) fine_flow: flow_input = affine(macro - 0.34*basin, FLOW_INPUT) -> flow_pre ;
    //    fine_flow = flow_channels_seam_safe(flow_input, width=1.8, power=0.44) ; channels reassigned
    s.dispatch_full(WL_FLOW_PRE, 0, 0, 0.0);         // flow_pre <- flow_input (NO clip)
    s.flow_channels(0.44_f32, 1.8);                  // gauss_out = spread discharge
    s.dispatch_full(WL_CHANNELS_FLOW, 0, 0, 0.0);    // pool7 = clip(0.68*channels + 0.50*ss(fine_flow))

    // 6) levees = smoothstep(0.02,0.18, gaussian(channels,2.2) - gaussian(channels,5.2))
    //             * (1 - smoothstep(0.42,0.86, channels))
    // stash gaussian(channels,2.2) into pool8 (transient), then compute gaussian(channels,5.2)
    // into gauss_out so WL_LEVEES has BOTH blurs live (pool8 = blur22, gauss_out = blur52).
    s.gauss_pool(7, 2.2);                            // gauss_out = gaussian(channels, 2.2)
    s.dispatch_pool(PASS_POOL_FROM_GAUSS, 8);        // pool8 = gaussian(channels, 2.2)
    s.gauss_pool(7, 5.2);                            // gauss_out = gaussian(channels, 5.2)
    s.dispatch_full(WL_LEVEES, 0, 0, 0.0);           // pool9 = levees

    // 7) flat_base = gaussian(affine(0.42*macro - 0.58*basin + 0.20*floodplain, FLAT_BASE), smoothing_px)
    s.dispatch_full(WL_FLAT_BASE_PRE, 0, 0, 0.0);    // pool10 = flat_base_inner
    s.gauss_pool(10, smoothing_px);                  // gauss_out = gaussian(pool10, smoothing_px)
    s.dispatch_pool(PASS_POOL_FROM_GAUSS, 10);       // pool10 = flat_base

    // 8) assemble height (macro/basin/floodplain/channels/levees/micro + flat_base blend)
    s.dispatch_full(WL_ASSEMBLE, 0, 0, 0.0);

    // 9) final: height_blur = gaussian(height, 1.2); final_blend = 0.88*h + 0.12*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(1.2);                                    // gauss_out = gaussian(height, 1.2)
    s.dispatch_full(WL_FINAL, 0, 0, 0.0);

    // 10) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}
