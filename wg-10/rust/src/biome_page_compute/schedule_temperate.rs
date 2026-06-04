//! Temperate biome dispatch schedule.

use super::abi::*;
use super::scheduler::Scheduler;

/// The TEMPERATE dispatch schedule (style = appalachian_ridges). Mirrors the field DAG of
/// recipes_temperate.rs::generate_seamsafe ONE-FOR-ONE: warp+macro/folded_remap/hills_raw/fine ->
/// ridges (blur folded_remap 1.1) -> hills (blur hills_raw 2.4) -> upland (blur macro 4.2) ->
/// flow_source -> RAW discharge (flow_discharge, pre-blur 1.15, NO trailing spread) -> valleys
/// (spread discharge 1.8) + broad_valleys (spread the SAME raw discharge 4.2) -> rounded (blur of
/// affine combo, smoothing_px=1.8) -> assemble -> final. All intermediate fields live in the
/// GENERIC scratch POOL (pool0..pool11; see biome_temperate.glsl for the slot map). The sigmas
/// (1.0, 1.1, 1.15, 1.8, 2.4, 4.2) are all in temperate_sigmas().
///
/// TEMPERATE DIVERGENCE (the two-spread crux): temperate's drainage uses `flow_discharge(0.43,
/// 1.15)` (the common PREFIX of flow_channels_ex up to + including PASS_DISCHARGE -- leaving the
/// raw log1p discharge in gauss_in), NOT the single-spread flow_channels/flow_channels_ex the 8
/// proven biomes use. It then spreads that RAW discharge at TWO sigmas: gauss(1.8) -> TE_VALLEYS
/// reads gauss_out (-> pool9), then gauss(4.2) -> TE_BROAD_VALLEYS reads gauss_out (-> pool10). The
/// second gauss(4.2) re-reads the SAME intact gauss_in (the generic gaussian only writes
/// gauss_mid/gauss_out, never gauss_in), so no pool staging of the raw discharge is needed. This is
/// the byte-flow that keeps the 8 proven biomes untouched while temperate gets its dual spread.
pub(crate) fn schedule_temperate(s: &mut Scheduler) {
    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; macro=pool2 ; folded_remap=pool3 ;
    //    hills_raw=pool4 ; fine=pool5
    s.dispatch_full(TE_POINTWISE, 0, 0, 0.0);

    // 2) ridges = smoothstep(0.40,0.82, gaussian(folded_remap, 1.1))
    s.gauss_pool(3, 1.1);                            // gauss_out = gaussian(folded_remap, 1.1)
    s.dispatch_full(TE_RIDGES, 0, 0, 0.0);           // pool6 = ridges

    // 3) hills = clip(affine(gaussian(hills_raw, 2.4), HILLS))
    s.gauss_pool(4, 2.4);                            // gauss_out = gaussian(hills_raw, 2.4)
    s.dispatch_full(TE_HILLS, 0, 0, 0.0);            // pool7 = hills

    // 4) upland = smoothstep(0.50,0.82, gaussian(macro, 4.2))
    s.gauss_pool(2, 4.2);                            // gauss_out = gaussian(macro, 4.2)
    s.dispatch_full(TE_UPLAND, 0, 0, 0.0);           // pool8 = upland

    // 5) flow_source = affine(0.72*macro + 0.32*ridges + 0.28*hills + 0.26*upland, FLOW_SRC) -> flow_pre
    s.dispatch_full(TE_FLOW_PRE, 0, 0, 0.0);         // flow_pre <- flow_source (NO clip)

    // 6) RAW discharge: flow_discharge(power=0.43, pre-blur 1.15) leaves the raw log1p discharge in
    //    gauss_in (NO trailing spread). Then spread it at TWO sigmas (gauss never clobbers gauss_in):
    s.flow_discharge(0.43_f32, 1.15);                // gauss_in = raw log1p discharge
    s.gauss(1.8);                                    // gauss_out = gaussian(discharge, 1.8)
    s.dispatch_full(TE_VALLEYS, 0, 0, 0.0);          // pool9 = valleys (reads gauss_out)
    s.gauss(4.2);                                    // gauss_out = gaussian(discharge, 4.2) (re-reads gauss_in)
    s.dispatch_full(TE_BROAD_VALLEYS, 0, 0, 0.0);    // pool10 = broad_valleys (reads gauss_out)

    // 7) rounded = gaussian(affine(0.52*macro + 0.48*hills, ROUNDED), max(smoothing_px,0.2)=1.8)
    s.dispatch_full(TE_ROUNDED_PRE, 0, 0, 0.0);      // pool11 = rounded_inner
    s.gauss_pool(11, 1.8);                           // gauss_out = gaussian(pool11, 1.8)
    s.dispatch_pool(PASS_POOL_FROM_GAUSS, 11);       // pool11 = rounded

    // 8) assemble height (hills/ridges/upland - valleys/broad_valleys + fine; then 0.76*h + 0.24*rounded)
    s.dispatch_full(TE_ASSEMBLE, 0, 0, 0.0);

    // 9) final: height_blur = gaussian(height, 1.0); final_blend = 0.85*h + 0.15*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(1.0);                                    // gauss_out = gaussian(height, 1.0)
    s.dispatch_full(TE_FINAL, 0, 0, 0.0);

    // 10) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}
