//! Grassland biome dispatch schedule.

use super::abi::*;
use super::scheduler::Scheduler;

/// The GRASSLAND dispatch schedule (style = ROLLING_PRAIRIE). Mirrors the field DAG of
/// recipes_grassland.rs::generate_seamsafe ONE-FOR-ONE: warp+macro/secondary -> swells (blur) ->
/// pans (blur 1-swells) -> sandhills/escarpments (whole-field sub-pipelines) -> base_for_flow ->
/// draws (flow channels) -> fine_grain/low_ripple -> assemble -> floor blend -> final. All
/// intermediate fields live in the GENERIC scratch POOL (pool0..pool11; see biome_grassland.glsl
/// for the slot map). The sigmas (smoothing_px=3.7, 5.2, 1.55, 1.4, flow pre-blur 1.15 + spread
/// 2.1, floor 3.7, final 1.1) are all in grassland_sigmas(). This is the PATTERN the other 9 ports
/// copy: pointwise passes write pool slots; blur a slot via gauss_pool(slot,sigma) then read
/// gauss_out (or POOL_FROM_GAUSS to stash); flow channels reuse the proven flow_channels().
pub(crate) fn schedule_grassland(s: &mut Scheduler) {
    let smoothing_px = 3.7_f64;          // ROLLING_PRAIRIE.smoothing_px
    let floor_smooth = smoothing_px.max(0.5);

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; macro_f=pool2 ; secondary=pool3
    s.dispatch_full(GL_POINTWISE, 0, 0, 0.0);

    // 2) swells = clip(affine(gaussian(0.74*macro + 0.26*secondary, smoothing_px), SWELLS))
    s.dispatch_full(GL_COMBO, 0, 0, 0.0);   // gauss_in <- combo
    s.gauss(smoothing_px);                  // gauss_out = gaussian(combo, smoothing_px)
    s.dispatch_full(GL_SWELLS, 0, 0, 0.0);  // pool4 = swells

    // 3) pans = smoothstep(0.54,0.88, gaussian(1 - swells, 5.2))
    s.dispatch_full(GL_ONE_MINUS_SWELLS, 0, 0, 0.0); // gauss_in <- 1 - swells
    s.gauss(5.2);                                    // gauss_out = gaussian(1-swells, 5.2)
    s.dispatch_full(GL_PANS, 0, 0, 0.0);             // pool5 = pans

    // 4) sandhills sub-pipeline: pre (pool11) -> gaussian(1.55) -> clip(affine(., SH_FINAL)) = pool6
    s.dispatch_full(GL_SANDHILL_PRE, 0, 0, 0.0);     // pool11 = softened*envelope*broken
    s.gauss_pool(11, 1.55);                          // gauss_out = gaussian(pool11, 1.55)
    s.dispatch_full(GL_SANDHILL_FINAL, 0, 0, 0.0);   // pool6 = sandhills

    // 5) escarpments sub-pipeline: edge (pool11) -> gaussian(1.4) -> clip(affine(., ESC_FINAL)) = pool7
    s.dispatch_full(GL_ESC_PRE, 0, 0, 0.0);          // pool11 = smoothstep(|bands|)*plateau
    s.gauss_pool(11, 1.4);                           // gauss_out = gaussian(pool11, 1.4)
    s.dispatch_full(GL_ESC_FINAL, 0, 0, 0.0);        // pool7 = escarpments

    // 6) base_for_flow = affine(0.82*swells + 0.28*esc - 0.34*pans, BASE_FLOW) (NO clip) -> flow_pre
    s.dispatch_full(GL_BASE_FOR_FLOW, 0, 0, 0.0);

    // 7) draws = smoothstep(0.60,0.94, flow_channels(base_for_flow, width=2.1, power=0.50))
    //            * (0.42 + 0.58*(1 - pans))    [flow_channels leaves spread discharge in gauss_out]
    s.flow_channels(0.50_f32, 2.1);
    s.dispatch_full(GL_DRAWS, 0, 0, 0.0);            // pool8 = draws

    // 8) texture: fine_grain (pool9) + low_ripple (pool10), rotated angle+1.10 on w_x/w_z
    s.dispatch_full(GL_TEXTURE, 0, 0, 0.0);

    // 9) assemble height (swells/sandhills/escarpments/pans/draws/texture weighted sum)
    s.dispatch_full(GL_ASSEMBLE, 0, 0, 0.0);

    // 10) floor blend: smooth = gaussian(height, max(smoothing_px,0.5)); open_floor blend
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(floor_smooth);                           // gauss_out = gaussian(height, floor_smooth)
    s.dispatch_full(GL_OPEN_FLOOR_BLEND, 0, 0, 0.0);

    // 11) final: height_blur = gaussian(height, 1.1); final_blend = 0.86*h + 0.14*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(1.1);                                    // gauss_out = gaussian(height, 1.1)
    s.dispatch_full(GL_FINAL, 0, 0, 0.0);

    // 12) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}
