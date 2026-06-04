//! Pure unit tests for biome page compute helpers.

use super::*;

mod compose_kernels;
mod kernels;
mod push_constants;

#[test]
fn grassland_sigmas_fit_stride() {
    for &sg in &grassland_sigmas() {
        let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
        assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
    }
}

#[test]
fn grassland_sigmas_cover_all_pipeline_blurs() {
    // every sigma schedule_grassland asks for must be present (kparams panics otherwise).
    // The schedule's gauss(...) calls + flow_channels(power, 2.1) pre-blur(1.15)/spread(2.1).
    let smoothing_px = 3.7_f64;
    let floor_smooth = smoothing_px.max(0.5);
    let draw_spread = 2.1_f64.max(0.1);
    let s = grassland_sigmas();
    for need in [
        smoothing_px, 5.2_f64, 1.55, 1.4, 1.15, draw_spread, floor_smooth, 1.1,
    ] {
        assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
    }
}

#[test]
fn biome_sigmas_known_biomes() {
    assert!(biome_sigmas("mountain").is_some());
    assert!(biome_sigmas("grassland").is_some());
    assert!(biome_sigmas("desert").is_some());
    assert!(biome_sigmas("coast").is_some());
    assert!(biome_sigmas("wetland").is_some());
    assert!(biome_sigmas("tundra").is_some());
    assert!(biome_sigmas("nope").is_none());
}

#[test]
fn wetland_sigmas_fit_stride() {
    for &sg in &wetland_sigmas() {
        let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
        assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
    }
}

#[test]
fn wetland_sigmas_cover_all_pipeline_blurs() {
    // every sigma schedule_wetland asks for must be present (kparams panics otherwise).
    // The schedule's gauss(...)/gauss_pool(...) calls + flow_channels(power, 1.8)
    // pre-blur(1.15)/spread(1.8). Levee DoG uses 2.2 and 5.2; flat_base uses smoothing_px=4.4.
    let smoothing_px = 4.4_f64;
    let flow_spread = 1.8_f64.max(0.1);
    let s = wetland_sigmas();
    for need in [5.8_f64, 5.2, 1.15, flow_spread, 2.2, smoothing_px, 1.2] {
        assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
    }
}

#[test]
fn pool_slots_matches_wetland_pool_map() {
    // wetland's biome_wetland.glsl uses pool0..pool10 (11 slots). POOL_SLOTS must cover it.
    assert!(POOL_SLOTS >= 11, "POOL_SLOTS {POOL_SLOTS} < wetland's 11 pool slots");
}

#[test]
fn coast_sigmas_fit_stride() {
    for &sg in &coast_sigmas() {
        let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
        assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
    }
}

#[test]
fn coast_sigmas_cover_all_pipeline_blurs() {
    // every sigma schedule_coast asks for must be present (kparams panics otherwise).
    // The schedule's gauss(...)/gauss_pool(...) calls + flow_channels(power, 1.9)
    // pre-blur(1.15)/spread(1.9).
    let channel_spread = 1.9_f64.max(0.1);
    let s = coast_sigmas();
    for need in [1.15_f64, channel_spread, 2.0, 3.0, 0.9] {
        assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
    }
}

#[test]
fn pool_slots_matches_coast_pool_map() {
    // coast's biome_coast.glsl uses pool0..pool15 (16 slots, pool12 reused). POOL_SLOTS covers it.
    assert!(POOL_SLOTS >= 16, "POOL_SLOTS {POOL_SLOTS} < coast's 16 pool slots");
}

#[test]
fn pool_slots_matches_grassland_pool_map() {
    // grassland's biome_grassland.glsl uses pool0..pool11 (12 slots). POOL_SLOTS must cover it.
    assert!(POOL_SLOTS >= 12, "POOL_SLOTS {POOL_SLOTS} < grassland's 12 pool slots");
}

#[test]
fn pool_slots_matches_desert_pool_map() {
    // desert's biome_desert.glsl uses pool0..pool15 (16 slots). POOL_SLOTS must cover it.
    assert!(POOL_SLOTS >= 16, "POOL_SLOTS {POOL_SLOTS} < desert's 16 pool slots");
}

#[test]
fn desert_sigmas_fit_stride() {
    for &sg in &desert_sigmas() {
        let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
        assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
    }
}

#[test]
fn desert_sigmas_cover_all_pipeline_blurs() {
    // every sigma schedule_desert asks for must be present (kparams panics otherwise).
    // The schedule's gauss(...)/gauss_pool(...) calls + flow_channels(power, 1.8)
    // pre-blur(1.15)/spread(1.8).
    let floor_smooth = 5.2_f64.max(0.2);
    let wash_spread = 1.8_f64.max(0.1);
    let s = desert_sigmas();
    for need in [
        6.2_f64, 5.0, 0.70, 3.2, 2.2, 1.15, wash_spread, floor_smooth, 0.95,
    ] {
        assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
    }
}

#[test]
fn tundra_sigmas_fit_stride() {
    for &sg in &tundra_sigmas() {
        let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
        assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
    }
}

#[test]
fn tundra_sigmas_cover_all_pipeline_blurs() {
    // every sigma schedule_tundra asks for must be present (kparams panics otherwise).
    // The schedule's gauss(...)/gauss_pool(...) calls + flow_channels(power, 2.0)
    // pre-blur(1.15)/spread(2.0). plain=5.8, pattern=1.2, fringe=1.8, base=smoothing_px=5.0,
    // final=1.1.
    let smoothing_px = 5.0_f64;
    let flow_spread = 2.0_f64.max(0.1);
    let s = tundra_sigmas();
    for need in [5.8_f64, 1.2, 1.8, 1.15, flow_spread, smoothing_px, 1.1] {
        assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
    }
}

#[test]
fn pool_slots_matches_tundra_pool_map() {
    // tundra's biome_tundra.glsl uses pool0..pool12 (13 slots). POOL_SLOTS must cover it.
    assert!(POOL_SLOTS >= 13, "POOL_SLOTS {POOL_SLOTS} < tundra's 13 pool slots");
}

#[test]
fn glacial_sigmas_fit_stride() {
    for &sg in &glacial_sigmas() {
        let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
        assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
    }
}

#[test]
fn glacial_sigmas_cover_all_pipeline_blurs() {
    // every sigma schedule_glacial asks for must be present (kparams panics otherwise).
    // The schedule's gauss(...)/gauss_pool(...) calls + flow_channels_ex(power, width, 1.85)
    // PRE-BLUR(1.85)/spread(width). GLACIAL DIVERGENCE: pre-blur is 1.85 (NOT the shared 1.15),
    // so 1.85 MUST be covered (the machine-hook the whole port hangs on).
    let trough_width_px = 6.8_f64;
    let axial_sigma = (trough_width_px * 0.18).max(0.8);   // 1.224
    let primary_spread = trough_width_px.max(0.1);          // 6.8
    let trib_spread = (trough_width_px * 0.48).max(0.8).max(0.1); // 3.264
    let ice_smooth_px = 6.2_f64;
    let floor = ice_smooth_px.max(0.2);                     // 6.2
    let ice_smooth = (ice_smooth_px * 0.65).max(0.2);       // 4.03
    let s = glacial_sigmas();
    for need in [
        1.25_f64, 5.8, 7.0, 2.8, 1.85, axial_sigma, 1.6, trib_spread, primary_spread,
        floor, ice_smooth, 1.35,
    ] {
        assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
    }
    // The custom pre-blur 1.85 must be present AND distinct from the shared 1.15 (the proven
    // biomes' pre-blur), proving glacial's flow_channels_ex hook is wired, not the default.
    assert!(s.iter().any(|&v| (v - 1.85).abs() < 1e-9), "glacial pre-blur 1.85 missing");
    assert!(!s.iter().any(|&v| (v - 1.15).abs() < 1e-9), "glacial must NOT use the shared 1.15 pre-blur");
}

#[test]
fn pool_slots_matches_glacial_pool_map() {
    // glacial's biome_glacial.glsl uses pool0..pool15 (16 slots; pool15 transient,
    // pool10/pool11/pool7 reused post-mask). POOL_SLOTS must cover it.
    assert!(POOL_SLOTS >= 16, "POOL_SLOTS {POOL_SLOTS} < glacial's 16 pool slots");
}

#[test]
fn glacial_sigmas_is_known_biome() {
    assert!(biome_sigmas("glacial").is_some());
}

#[test]
fn karst_sigmas_fit_stride() {
    for &sg in &karst_sigmas() {
        let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
        assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
    }
}

#[test]
fn karst_sigmas_cover_all_pipeline_blurs() {
    // every sigma schedule_karst asks for must be present (kparams panics otherwise).
    // The schedule's gauss(...)/gauss_pool(...) calls + flow_channels(power, 2.6) pre-blur(1.15)/
    // spread(2.6). KARST uses the SHARED flow_channels (pre-blur 1.15), NOT the glacial-style
    // flow_channels_ex hook -- its "custom" flow is just power=0.54, width=2.6 (the spread sigma
    // is the existing width param). plateau=5.8, towers=2.0, dolines=2.6, cellular=3.8,
    // floor=2.8, final=0.95.
    let tower_width = 2.0_f64.max(0.2);     // 2.0
    let doline_width = 2.6_f64.max(0.2);    // 2.6
    let dv_spread = 2.6_f64.max(0.1);       // 2.6 (dedups against doline_width)
    let floor_smooth = 2.8_f64.max(0.2);    // 2.8
    let s = karst_sigmas();
    for need in [
        5.8_f64, tower_width, doline_width, 3.8, 1.15, dv_spread, floor_smooth, 0.95,
    ] {
        assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
    }
    // KARST uses the SHARED pre-blur 1.15 (NOT a glacial-style custom pre-blur). Assert it is
    // present, proving the dry-valley flow rides the proven flow_channels() path.
    assert!(s.iter().any(|&v| (v - 1.15).abs() < 1e-9), "karst shared pre-blur 1.15 missing");
}

#[test]
fn pool_slots_matches_karst_pool_map() {
    // karst's biome_karst.glsl uses pool0..pool15 (16 slots; pool15 transient -> lineament_mask,
    // pool2/pool7 reused for fine/karren post-base). POOL_SLOTS must cover it.
    assert!(POOL_SLOTS >= 16, "POOL_SLOTS {POOL_SLOTS} < karst's 16 pool slots");
}

#[test]
fn karst_sigmas_is_known_biome() {
    assert!(biome_sigmas("karst").is_some());
}

#[test]
fn temperate_sigmas_fit_stride() {
    for &sg in &temperate_sigmas() {
        let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
        assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
    }
}

#[test]
fn temperate_sigmas_cover_all_pipeline_blurs() {
    // every sigma schedule_temperate asks for must be present (kparams panics otherwise).
    // The schedule's gauss(...)/gauss_pool(...) calls + flow_discharge(power=0.43) PRE-BLUR(1.15)
    // and the TWO independent spreads (1.8 for valleys, 4.2 for broad_valleys). TEMPERATE uses
    // the RAW-discharge flow_discharge (NO single trailing spread); the two spreads ARE the
    // distinct sigmas. ridges=1.1, hills=2.4, upland/broad_valleys=4.2, valleys/rounded=1.8,
    // final=1.0.
    let smoothing_px = 1.8_f64.max(0.2); // rounded blur (dedups against valleys spread 1.8)
    let s = temperate_sigmas();
    for need in [1.0_f64, 1.1, 2.4, 4.2, 1.15, 1.8, smoothing_px] {
        assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
    }
    // TEMPERATE uses the SHARED pre-blur 1.15 (NOT a glacial-style custom pre-blur). Assert it
    // is present, proving the valley flow rides the proven flow_discharge(.., 1.15) prefix.
    assert!(s.iter().any(|&v| (v - 1.15).abs() < 1e-9), "temperate shared pre-blur 1.15 missing");
    // The TWO spread sigmas (1.8 and 4.2) MUST BOTH be present AND distinct -- that is the
    // two-spread crux of the temperate port (one raw discharge, spread twice).
    assert!(s.iter().any(|&v| (v - 1.8).abs() < 1e-9), "temperate valleys spread 1.8 missing");
    assert!(s.iter().any(|&v| (v - 4.2).abs() < 1e-9), "temperate broad_valleys spread 4.2 missing");
}

#[test]
fn pool_slots_matches_temperate_pool_map() {
    // temperate's biome_temperate.glsl uses pool0..pool11 (12 slots). POOL_SLOTS must cover it.
    assert!(POOL_SLOTS >= 12, "POOL_SLOTS {POOL_SLOTS} < temperate's 12 pool slots");
}

#[test]
fn temperate_sigmas_is_known_biome() {
    assert!(biome_sigmas("temperate").is_some());
}

#[test]
fn rainforest_sigmas_fit_stride() {
    for &sg in &rainforest_sigmas() {
        let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
        assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
    }
}

#[test]
fn rainforest_sigmas_cover_all_pipeline_blurs() {
    // every sigma schedule_rainforest asks for must be present (kparams panics otherwise).
    // The schedule's gauss(...)/gauss_pool(...) calls + flow_discharge(power=0.38) PRE-BLUR(1.15)
    // and the TWO independent spreads (1.15 for tributaries, 2.2 for trunk). RAINFOREST uses the
    // RAW-discharge flow_discharge (NO single trailing spread); the two spreads ARE the distinct
    // sigmas. hills=1.7, plateau=4.5, lowland=5.4, wet_rounding=smoothing_px=2.6, final=1.0.
    let smoothing_px = 2.6_f64.max(0.2); // wet_rounding blur (dedups against the listed 2.6)
    let s = rainforest_sigmas();
    for need in [1.0_f64, 1.15, 1.7, 2.2, 2.6, 4.5, 5.4, smoothing_px] {
        assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
    }
    // RAINFOREST uses the SHARED pre-blur 1.15 (NOT a glacial-style custom pre-blur). Assert it
    // is present, proving the drainage rides the proven flow_discharge(.., 1.15) prefix.
    assert!(s.iter().any(|&v| (v - 1.15).abs() < 1e-9), "rainforest shared pre-blur 1.15 missing");
    // The TWO spread sigmas (1.15 and 2.2) MUST BOTH be present -- that is the dual-mask crux of
    // the rainforest port (one raw discharge, spread twice). The tributaries spread (1.15) dedups
    // against the shared pre-blur; the trunk spread (2.2) is its own distinct slot.
    assert!(s.iter().any(|&v| (v - 2.2).abs() < 1e-9), "rainforest trunk spread 2.2 missing");
}

#[test]
fn pool_slots_matches_rainforest_pool_map() {
    // rainforest's biome_rainforest.glsl uses pool0..pool11 (12 slots; pool3/pool4/pool7 reused
    // for plateau/hills/drainage). POOL_SLOTS must cover it.
    assert!(POOL_SLOTS >= 12, "POOL_SLOTS {POOL_SLOTS} < rainforest's 12 pool slots");
}

#[test]
fn rainforest_sigmas_is_known_biome() {
    assert!(biome_sigmas("rainforest").is_some());
}

#[test]
fn volcanic_sigmas_fit_stride() {
    for &sg in &volcanic_sigmas() {
        let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
        assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
    }
}

#[test]
fn volcanic_sigmas_cover_all_pipeline_blurs() {
    // every sigma schedule_volcanic asks for must be present (kparams panics otherwise).
    // flows blur=1.1 ; gully flow_discharge PRE-BLUR(1.15) + FIXED spread(1.2) ;
    // caldera spc_blur=2.6 ; ash max_cf_blur=3.0 ; smoothed_plain=2.6 (dedups) ; final=0.85.
    let s = volcanic_sigmas();
    for need in [0.85_f64, 1.1, 1.15, 1.2, 2.6, 3.0] {
        assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
    }
    // VOLCANIC uses the SHARED pre-blur 1.15 (flow_discharge prefix), NOT a glacial-style custom
    // pre-blur -- assert it is present.
    assert!(s.iter().any(|&v| (v - 1.15).abs() < 1e-9), "volcanic shared pre-blur 1.15 missing");
    // The gully spread is a FIXED 1.2 (the gully_channels_seam_safe spread, NOT the flow width),
    // and is distinct from the pre-blur -- assert it is present.
    assert!(s.iter().any(|&v| (v - 1.2).abs() < 1e-9), "volcanic gully spread 1.2 missing");
}

#[test]
fn pool_slots_matches_volcanic_pool_map() {
    // volcanic's biome_volcanic.glsl uses pool0..pool15 (16 slots; pool15 transient -> raw flows,
    // then REUSED for max_cf_blur). POOL_SLOTS must cover it.
    assert!(POOL_SLOTS >= 16, "POOL_SLOTS {POOL_SLOTS} < volcanic's 16 pool slots");
}

#[test]
fn volcanic_sigmas_is_known_biome() {
    assert!(biome_sigmas("volcanic").is_some());
}

#[test]
fn volcanic_vent_count_fits_max_vents() {
    // The CPU vent packing for the fixture seeds must produce vent_count <= MAX_VENTS (so the
    // packed buffer is never truncated). STYLES[0] (stratovolcano_cluster) draws vent_count=4.
    use crate::recipes_volcanic::volcanic;
    for &seed in &[0_i64, 7] {
        let (packed, count) = volcanic::packed_vents(&volcanic::STRATOVOLCANO_CLUSTER, seed, 60000.0);
        assert!(count <= volcanic::MAX_VENTS, "seed {seed}: vent_count {count} > MAX_VENTS");
        assert_eq!(count, 4, "stratovolcano_cluster vent_count should be 4 (got {count})");
        assert_eq!(packed.len(), volcanic::MAX_VENTS * volcanic::VENT_STRIDE);
    }
}

