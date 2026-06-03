//! Mountain biome dispatch schedule.

use super::abi::*;
use super::scheduler::Scheduler;

/// The MOUNTAIN dispatch schedule (style = ALPINE_BRANCHING), `pub(crate)` so the readback test
/// harness (`run_inner`) AND the runtime producer run the SAME proven sequence (DRY -- one
/// schedule, two hosts). Operates on an already-built `Scheduler` bound to the apron buffers; pure
/// dispatch, no rd/buffer ownership.
///
/// EXACTLY the pre-refactor sequence + params; this is the reference pattern every future biome
/// `schedule_<biome>()` copies. The constants here (valley_width_px/trib_width/floor_smooth) mirror
/// `mountain_sigmas()` so the gauss/flow widths resolve to pre-validated kernel slots.
pub(crate) fn schedule_mountain(s: &mut Scheduler) {
    let valley_width_px = 2.4_f64;
    let trib_width = (valley_width_px * 0.42).max(0.6);
    let floor_smooth = 4.0_f64.max(0.2);

    // 0) meshgrid ; 1) pointwise
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);
    s.dispatch_full(PASS_POINTWISE, 0, 0, 0.0);

    // 2) range_envelope = smoothstep(0.24,0.58, gaussian(ranges, 5.0))
    s.dispatch_full(PASS_COPY, CP_RANGES, 0, 0.0);
    s.gauss(5.0);
    s.dispatch_full(PASS_RANGE_ENV, 0, 0, 0.0);

    // 3) lowland: broad_range = gaussian(ranges, 7.0); combine with regional
    s.dispatch_full(PASS_COPY, CP_RANGES, 0, 0.0);
    s.gauss(7.0);
    s.dispatch_full(PASS_LOWLAND, 0, 0, 0.0);

    // 4) massif: gaussian(ranges,1.8) -> massif_inner; then gaussian(massif,2.0) writeback
    s.dispatch_full(PASS_COPY, CP_RANGES, 0, 0.0);
    s.gauss(1.8);
    s.dispatch_full(PASS_MASSIF_INNER, 0, 0, 0.0);
    s.dispatch_full(PASS_COPY, CP_MASSIF, 0, 0.0);
    s.gauss(2.0);
    s.dispatch_full(PASS_MASSIF_WRITEBACK, 0, 0, 0.0);

    // 5) base
    s.dispatch_full(PASS_BASE, 0, 0, 0.0);

    // 6 + 7) primary + tributary channels (flow_on gated). When flow_on==false (coarse clipmap
    // levels) BOTH expensive flow_channels_seam_safe passes are SKIPPED and the two masks are
    // zeroed -> the carve terms in PASS_ASSEMBLE vanish -> the MACRO surface. EXACTLY the CPU
    // oracle's `if flow_on { ... } else { primary_mask = tributary_mask = zeros }` branch.
    if s.flow_on {
        // 6) primary channels: flow_channels_seam_safe(base, valley_width, power=0.48)
        s.dispatch_full(PASS_FLOW_PRE_BASE, 0, 0, 0.0);
        s.flow_channels(0.48_f32, valley_width_px);
        s.dispatch_full(PASS_PRIMARY_MASK, 0, 0, 0.0);

        // 7) tributaries: flow_channels_seam_safe(rough_surface, trib_width, power=0.34)
        s.dispatch_full(PASS_FLOW_PRE_ROUGH, 0, 0, 0.0);
        s.flow_channels(0.34_f32, trib_width);
        s.dispatch_full(PASS_TRIB_MASK, 0, 0, 0.0);
    } else {
        // primary_mask = tributary_mask = 0 (the cached buffers persist across pages -> re-zero).
        s.dispatch_full(PASS_ZERO_FLOW_MASKS, 0, 0, 0.0);
    }

    // 8) high_mask / valley_mask
    s.dispatch_full(PASS_MASKS, 0, 0, 0.0);

    // 9) assemble height
    s.dispatch_full(PASS_ASSEMBLE, 0, 0, 0.0);

    // 10) floor blend
    s.dispatch_full(PASS_COPY, CP_VALLEY, 0, 0.0);
    s.gauss(1.2);
    s.dispatch_full(PASS_FLOOR_MASK, 0, 0, 0.0);
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);
    s.gauss(floor_smooth);
    s.dispatch_full(PASS_FLOOR_BLEND, 0, 0, 0.0);

    // 11) final: height_blur = gaussian(height,1.2); final_blend; affine
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);
    s.gauss(1.2);
    s.dispatch_full(PASS_FINAL, 0, 0, 0.0);

    // 12) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}
