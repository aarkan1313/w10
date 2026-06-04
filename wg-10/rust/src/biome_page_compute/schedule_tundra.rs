//! Tundra biome dispatch schedule.

use super::abi::*;
use super::scheduler::Scheduler;

/// The TUNDRA dispatch schedule (style = arctic_plain). Mirrors the field DAG of
/// recipes_tundra.rs::generate_seamsafe ONE-FOR-ONE: warp+macro/polygons/stripes/fringe_ridges/
/// foothills/fine -> plain (blur 1-|macro-0.46|) -> pattern (blur 0.56*polygons+0.44*stripes, then
/// *plain) -> fringe (blur fringe_ridges) -> flow_source -> drainage (flow channels) -> base (blur
/// of affine combo) -> assemble -> final. All intermediate fields live in the GENERIC scratch POOL
/// (pool0..pool12; see biome_tundra.glsl for the slot map). The sigmas (5.8, 1.2, 1.8, flow
/// pre-blur 1.15 + spread 2.0, smoothing_px=5.0, final 1.1) are all in tundra_sigmas(). Same
/// PATTERN as schedule_grassland/desert/coast/wetland: pointwise passes write pool slots; blur a
/// slot via gauss_pool(slot,sigma) then read gauss_out (or POOL_FROM_GAUSS to stash); flow channels
/// reuse the proven flow_channels().
pub(crate) fn schedule_tundra(s: &mut Scheduler) {
    let smoothing_px = 5.0_f64;          // arctic_plain.smoothing_px (base blur)

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; macro=pool2 ; polygons=pool3 ; stripes=pool4 ;
    //    fringe_ridges=pool5 ; foothills=pool6 ; fine=pool7
    s.dispatch_full(TU_POINTWISE, 0, 0, 0.0);

    // 2) plain = smoothstep(0.36,0.76, gaussian(1 - |macro - 0.46|, 5.8))
    s.dispatch_full(TU_PLAIN_PRE, 0, 0, 0.0);        // gauss_in <- 1 - |macro - 0.46|
    s.gauss(5.8);                                    // gauss_out = gaussian(., 5.8)
    s.dispatch_full(TU_PLAIN, 0, 0, 0.0);            // pool8 = plain

    // 3) pattern = smoothstep(0.46,0.86, gaussian(0.56*polygons + 0.44*stripes, 1.2)) * plain
    s.dispatch_full(TU_PATTERN_PRE, 0, 0, 0.0);      // gauss_in <- 0.56*polygons + 0.44*stripes
    s.gauss(1.2);                                    // gauss_out = gaussian(., 1.2)
    s.dispatch_full(TU_PATTERN, 0, 0, 0.0);          // pool9 = pattern

    // 4) fringe = smoothstep(0.42,0.84, gaussian(fringe_ridges, 1.8))
    s.gauss_pool(5, 1.8);                            // gauss_out = gaussian(fringe_ridges, 1.8)
    s.dispatch_full(TU_FRINGE, 0, 0, 0.0);           // pool10 = fringe

    // 5) drainage: flow_source = affine(0.62*macro+0.26*foothills+0.22*fringe-0.22*plain,
    //    FLOW_SOURCE) -> flow_pre ; channels = flow_channels_seam_safe(flow_source, width=2.0,
    //    power=0.48) ; drainage = smoothstep(0.58,0.94, channels)
    s.dispatch_full(TU_FLOW_PRE, 0, 0, 0.0);         // flow_pre <- flow_source (NO clip)
    s.flow_channels(0.48_f32, 2.0);                  // gauss_out = spread discharge
    s.dispatch_full(TU_DRAINAGE, 0, 0, 0.0);         // pool11 = drainage

    // 6) base = gaussian(affine(0.74*macro + 0.26*foothills, BASE), smoothing_px)
    s.dispatch_full(TU_BASE_PRE, 0, 0, 0.0);         // pool12 = base_inner
    s.gauss_pool(12, smoothing_px);                  // gauss_out = gaussian(pool12, smoothing_px)
    s.dispatch_pool(PASS_POOL_FROM_GAUSS, 12);       // pool12 = base

    // 7) assemble height (macro_zsc/pattern/fringe/foothills/drainage/fine + base blend)
    s.dispatch_full(TU_ASSEMBLE, 0, 0, 0.0);

    // 8) final: height_blur = gaussian(height, 1.1); final_blend = 0.86*h + 0.14*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(1.1);                                    // gauss_out = gaussian(height, 1.1)
    s.dispatch_full(TU_FINAL, 0, 0, 0.0);

    // 9) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}
