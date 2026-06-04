//! Rainforest biome dispatch schedule.

use super::abi::*;
use super::scheduler::Scheduler;

/// The RAINFOREST dispatch schedule (style = humid_dissected_hills). Mirrors the field DAG of
/// recipes_rainforest.rs::generate_seamsafe ONE-FOR-ONE: warp+macro/plateau_seed/hills_raw/ridges
/// -> hills (blur hills_raw 1.7) -> plateau (blur plateau_seed 4.5, * (1-0.38*ridges)) -> lowland
/// (blur 1-macro 5.4) -> flow_source -> RAW discharge (flow_discharge, pre-blur 1.15, NO trailing
/// spread) -> tributaries (spread discharge 1.15) + trunk (spread the SAME raw discharge 2.2) ->
/// drainage (clip(0.68*trib+0.58*trunk)) -> close (low-freq fbm) -> wet_rounding (blur of affine
/// combo, smoothing_px=2.6) -> assemble -> final. All intermediate fields live in the GENERIC
/// scratch POOL (pool0..pool11; see biome_rainforest.glsl for the slot map). The sigmas
/// (1.0, 1.15, 1.7, 2.2, 2.6, 4.5, 5.4) are all in rainforest_sigmas().
///
/// RAINFOREST DUAL-MASK FLOW (the two-spread crux, like temperate): drainage uses `flow_discharge(
/// 0.38, 1.15)` (the common PREFIX of flow_channels_ex up to + including PASS_DISCHARGE -- leaving
/// the raw log1p discharge in gauss_in), NOT the single-spread flow_channels/flow_channels_ex the 8
/// pre-temperate biomes use. It then spreads that RAW discharge at TWO sigmas: gauss(1.15) ->
/// RF_TRIBUTARIES reads gauss_out (-> pool7), then gauss(2.2) -> RF_TRUNK reads gauss_out (-> pool8).
/// The second gauss(2.2) re-reads the SAME intact gauss_in (the generic gaussian only writes
/// gauss_mid/gauss_out, never gauss_in), so no pool staging of the raw discharge is needed. This is
/// EXACTLY temperate's two-spread sequencing -- NO machine extension.
pub(crate) fn schedule_rainforest(s: &mut Scheduler) {
    let smoothing_px = 2.6_f64.max(0.2);             // humid_dissected_hills.smoothing_px (wet_rounding blur)

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; macro=pool2 ; plateau_seed=pool3 ;
    //    hills_raw=pool4 ; ridges=pool5
    s.dispatch_full(RF_POINTWISE, 0, 0, 0.0);

    // 2) hills = clip(affine(gaussian(hills_raw, 1.7), HILLS))  (REUSE pool4)
    s.gauss_pool(4, 1.7);                            // gauss_out = gaussian(hills_raw, 1.7)
    s.dispatch_full(RF_HILLS, 0, 0, 0.0);            // pool4 = hills

    // 3) plateau = smoothstep(0.54,0.80, gaussian(plateau_seed, 4.5)) * (1 - 0.38*ridges)  (REUSE pool3)
    s.gauss_pool(3, 4.5);                            // gauss_out = gaussian(plateau_seed, 4.5)
    s.dispatch_full(RF_PLATEAU, 0, 0, 0.0);          // pool3 = plateau

    // 4) lowland = smoothstep(lo_e0,lo_e1, gaussian(1 - macro, 5.4))
    s.dispatch_full(RF_ONE_MINUS_MACRO, 0, 0, 0.0);  // gauss_in <- 1 - macro
    s.gauss(5.4);                                    // gauss_out = gaussian(1 - macro, 5.4)
    s.dispatch_full(RF_LOWLAND, 0, 0, 0.0);          // pool6 = lowland

    // 5) flow_source = affine(0.66*macro + 0.46*hills + 0.28*ridges + 0.20*plateau - 0.36*lowland, FLOW) -> flow_pre
    s.dispatch_full(RF_FLOW_PRE, 0, 0, 0.0);         // flow_pre <- flow_source (NO clip)

    // 6) RAW discharge: flow_discharge(power=0.38, pre-blur 1.15) leaves the raw log1p discharge in
    //    gauss_in (NO trailing spread). Then spread it at TWO sigmas (gauss never clobbers gauss_in):
    s.flow_discharge(0.38_f32, 1.15);                // gauss_in = raw log1p discharge
    s.gauss(1.15);                                   // gauss_out = gaussian(discharge, 1.15)
    s.dispatch_full(RF_TRIBUTARIES, 0, 0, 0.0);      // pool7 = tributaries (reads gauss_out)
    s.gauss(2.2);                                    // gauss_out = gaussian(discharge, 2.2) (re-reads gauss_in)
    s.dispatch_full(RF_TRUNK, 0, 0, 0.0);            // pool8 = trunk (reads gauss_out)

    // 7) drainage = clip(0.68*tributaries + 0.58*trunk)  (REUSE pool7)
    s.dispatch_full(RF_DRAINAGE, 0, 0, 0.0);         // pool7 = drainage

    // 8) close = affine(fbm(w_x,w_z, 1/(span*0.030),4,sseed+210,0.45), CLOSE) (NO clip)
    s.dispatch_full(RF_CLOSE, 0, 0, 0.0);            // pool9 = close

    // 9) wet_rounding = gaussian(affine(0.62*macro + 0.36*hills + 0.26*plateau, WET_ROUNDING), smoothing_px=2.6)
    s.dispatch_full(RF_WET_PRE, 0, 0, 0.0);          // pool11 = wet_inner
    s.gauss_pool(11, smoothing_px);                  // gauss_out = gaussian(pool11, 2.6)
    s.dispatch_pool(PASS_POOL_FROM_GAUSS, 10);       // pool10 = wet_rounding

    // 10) assemble height (hills/ridges/plateau - lowland/drainage + close texture; then 0.72*h + 0.28*wet_rounding)
    s.dispatch_full(RF_ASSEMBLE, 0, 0, 0.0);

    // 11) final: height_blur = gaussian(height, 1.0); final_blend = 0.84*h + 0.16*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(1.0);                                    // gauss_out = gaussian(height, 1.0)
    s.dispatch_full(RF_FINAL, 0, 0, 0.0);

    // 12) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}
