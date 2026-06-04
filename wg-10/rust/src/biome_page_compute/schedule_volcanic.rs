//! Volcanic biome dispatch schedule.

use super::abi::*;
use super::scheduler::Scheduler;

/// The VOLCANIC dispatch schedule (style = stratovolcano_cluster). Mirrors the field DAG of
/// recipes_volcanic.rs::generate_seamsafe ONE-FOR-ONE: warp+regional/rift -> VENT ACCUMULATION
/// (cones/craters/shields raw + raw flows, looping the uploaded vent buffer) -> flows (blur raw
/// flows 1.1) -> cones/craters/shields affine-remap -> lava_texture/rough_aa -> base -> gullies
/// (SHARED flow_discharge pre-blur 1.15, then a FIXED 1.2 spread) -> caldera bowl/rim + cone_lift
/// (uses gaussian(shields+cones,2.6)) -> assemble -> ash_plain blend (gaussian(max(cones,flows),3.0)
/// + gaussian(height,2.6)) -> final. All intermediate fields live in the GENERIC scratch POOL
/// (pool0..pool15; pool15 is TRANSIENT: raw flows staging, then REUSED for max_cf_blur; see
/// biome_volcanic.glsl for the slot map). The sigmas (1.1, 1.15, 1.2, 2.6, 3.0, 0.85) are all in
/// volcanic_sigmas().
///
/// THE KEY INSIGHT (the most novel port): VOLCANIC's vents are placed by numpy PCG64 RNG, but that
/// RNG STAYS IN RUST (recipes_volcanic::packed_vents, parity-exact). The CPU builds a SMALL packed
/// vent buffer (vx,vz,amp + 4 flow dirs per vent) BEFORE the compute list opens; the GPU's
/// VO_VENT_ACCUM pass loops `vent_count` vents doing PURE f32 cone/crater/shield/flow math -- NO RNG
/// on the GPU. The vent buffer is bound at binding 40 (ADDITIVE; the 10 proven biomes never read it).
///
/// VOLCANIC's gully carve uses `flow_discharge(0.40, 1.15)` (the common PREFIX leaving the raw log1p
/// discharge in gauss_in), then a SINGLE spread at sigma=1.2 -- the gully_channels_seam_safe FIXED
/// spread (NOT the flow-channels width.max(0.1)), so it cannot use flow_channels (whose spread sigma
/// IS the width). It rides the proven flow_discharge prefix + a dedicated gauss(1.2).
pub(crate) fn schedule_volcanic(s: &mut Scheduler) {
    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; regional=pool2 ; rift=pool3
    s.dispatch_full(VO_POINTWISE, 0, 0, 0.0);

    // 2) vent accumulation: loop the uploaded vent buffer (PURE f32 math; RNG already done in Rust)
    //    -> cones(pool4), craters(pool5), shields(pool6) RAW ; raw flows -> pool15 (transient)
    s.dispatch_full(VO_VENT_ACCUM, 0, 0, 0.0);

    // 3) flows = clip(affine(gaussian(raw flows, 1.1), FLOWS)) ; pool15(raw flows) -> blur -> pool7
    s.gauss_pool(15, 1.1);                           // gauss_out = gaussian(raw flows, 1.1)
    s.dispatch_full(VO_FLOWS_FINAL, 0, 0, 0.0);      // pool7 = flows

    // 4) finalize cones/craters/shields: clip(affine(raw, *)) in place (pool4/5/6)
    s.dispatch_full(VO_REMAP, 0, 0, 0.0);

    // 5) lava_texture (pool8) + rough_aa (pool9), pointwise on w_x/w_z (NO clip)
    s.dispatch_full(VO_LAVA_ROUGH, 0, 0, 0.0);

    // 6) base = affine(0.58*regional + 0.52*shields*shield_gain + 0.22*rift, BASE) = pool10
    s.dispatch_full(VO_BASE, 0, 0, 0.0);

    // 7) gullies: radial_surface = base + 1.12*cones - 0.78*craters -> flow_pre ; gully_channels
    //    = flow_discharge(power=0.40, pre-blur 1.15) [raw discharge in gauss_in] -> spread 1.2 ;
    //    gullies = smoothstep(0.52,0.92, discharge) * (0.30 + 0.70*cones) = pool11
    s.dispatch_full(VO_RADIAL, 0, 0, 0.0);           // flow_pre <- radial_surface (NO clip)
    s.flow_discharge(0.40_f32, 1.15);                // gauss_in = raw log1p discharge
    s.gauss(1.2);                                    // gauss_out = gaussian(discharge, 1.2)
    s.dispatch_full(VO_GULLIES, 0, 0, 0.0);          // pool11 = gullies

    // 8) caldera: spc_blur = gaussian(shields + cones, 2.6) ; caldera_bowl(pool12),
    //    caldera_rim(pool13), cone_lift(pool14)
    s.dispatch_full(VO_SPC_PRE, 0, 0, 0.0);          // gauss_in <- shields + cones
    s.gauss(2.6);                                    // gauss_out = gaussian(shields+cones, 2.6)
    s.dispatch_full(VO_CALDERA, 0, 0, 0.0);          // pool12/13/14

    // 9) assemble height (base + cone_lift/shields/rift/flows/caldera_rim - caldera_bowl/gullies + detail)
    s.dispatch_full(VO_ASSEMBLE, 0, 0, 0.0);

    // 10) ash_plain blend: max_cf_blur = gaussian(max(cones,flows), 3.0) -> pool15 (REUSE) ;
    //     smoothed_plain = gaussian(height, 2.6) ; height = blend by ash_plain
    s.dispatch_full(VO_ASH_PRE, 0, 0, 0.0);          // gauss_in <- max(cones, flows)
    s.gauss(3.0);                                    // gauss_out = gaussian(max_cf, 3.0)
    s.dispatch_pool(PASS_POOL_FROM_GAUSS, 15);       // pool15 = max_cf_blur (transient reuse)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(2.6);                                    // gauss_out = gaussian(height, 2.6) = smoothed_plain
    s.dispatch_full(VO_ASH_BLEND, 0, 0, 0.0);        // height = blend (reads pool15 + gauss_out)

    // 11) final: height_blur = gaussian(height, 0.85); final_blend = 0.82*h + 0.18*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(0.85);                                   // gauss_out = gaussian(height, 0.85)
    s.dispatch_full(VO_FINAL, 0, 0, 0.0);

    // 12) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}
