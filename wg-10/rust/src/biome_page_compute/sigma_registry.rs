//! Gaussian sigma registry for WG10 biome page compute.
//!
//! This module is the Rust-side spec table for every biome blur used by the GPU page machine.
//! Each list is intentionally explicit and covered by unit tests in the parent module.

use super::abi::{COMPOSE_RELIEF_SIGMA, TRUNCATE};
use super::kernels::{gaussian_kernel1d, gaussian_radius, KERNEL_STRIDE};

/// Distinct gaussian sigmas the mountain recipe uses, in a FIXED order. Each gets a slot in
/// the packed kernel buffer at index `slot * KERNEL_STRIDE`. (valley_width=2.4, trib=0.6
/// after max(.,0.6), floor_smooth=4.0 -- but 4.0 already appears, and 0.6/2.4 are distinct.)
/// Order here defines koffset; the orchestrator looks each sigma up by value.
/// sigma list (deduped): 1.15, 1.20, 1.80, 2.00, 5.00, 7.00, 2.40 (valley), 0.60 (trib width
/// = max(2.4*0.42,0.6)=1.008 -> actually 1.008; floor_smooth=4.0 distinct). See sigma_slots().
pub(crate) fn mountain_sigmas() -> Vec<f64> {
    let valley_width_px = 2.4_f64;
    let trib_width = (valley_width_px * 0.42).max(0.6); // 1.008
    let floor_smooth = 4.0_f64.max(0.2);
    // All distinct sigmas used by run_gaussian / run_flow_channels.
    vec![1.15, 1.20, 1.80, 2.00, 5.00, 7.00, valley_width_px, trib_width, floor_smooth]
}

/// SCALE-INVARIANCE (Task: per-level spacing kernel-anchoring). Build the packed gaussian-kernel
/// buffer + `KernelParams` for ONE dispatch at `spacing_m`, world-anchoring every mountain blur via
/// `recipes::helpers::sigma_cells(ref, spacing_m)` -- EXACTLY the CPU oracle's anchoring. The slot
/// LOOKUP key stays the REFERENCE cell sigma (`mountain_sigmas()` values, what `schedule_mountain`
/// passes to `gauss(...)`/`flow_channels(...)`), but the kernel CONTENT at that slot and its
/// `kradius` reflect the ANCHORED sigma `sigma_cells(ref, spacing)` -- so the GLSL machine (which
/// reads kradius/koffset from the push constant and taps the packed buffer) is UNCHANGED, it just
/// runs the spacing-anchored kernels. At `spacing_m == S_REF` (32.0) this is the identity
/// (sigma_cells == ref), reproducing the cell-sigma kernels byte-for-byte.
///
/// Returns `(packed_kernel_f32, KernelParams)` or an Err if an anchored kernel exceeds
/// `KERNEL_STRIDE` (would happen only at a spacing far finer than S_REF -- the production finest
/// level is ~32 m/px == S_REF, so the largest anchored sigma ~7.0 -> radius ~28 << 64; guarded
/// regardless). The kernel content MUST match `array_ops::gaussian_filter_nearest` at the SAME
/// anchored sigma bit-for-bit (same `gaussian_kernel1d` port, same TRUNCATE), or the 576 parity
/// drifts -- this is the exact thing the windowed gate verifies.
pub(crate) fn mountain_kernels_anchored(spacing_m: f64) -> Result<(Vec<f32>, KernelParams), String> {
    let refs = mountain_sigmas();
    let n_slots = refs.len();
    let mut packed = vec![0.0_f32; n_slots * KERNEL_STRIDE];
    let mut slots: Vec<(f64, i32, i32)> = Vec::with_capacity(n_slots);
    for (slot, &ref_sigma) in refs.iter().enumerate() {
        // ANCHOR: the blur covers the same WORLD distance at any spacing (macro structure identical
        // across clipmap levels). Mirror of every CPU `h::sigma_cells(ref, spacing_m)` call.
        let anchored = crate::recipes::helpers::sigma_cells(ref_sigma, spacing_m);
        let k = gaussian_kernel1d(anchored, TRUNCATE);
        if k.len() > KERNEL_STRIDE {
            return Err(format!(
                "mountain_kernels_anchored: anchored kernel len {} (ref sigma {ref_sigma} -> \
                 anchored {anchored} at spacing {spacing_m}) > KERNEL_STRIDE {KERNEL_STRIDE}",
                k.len()
            ));
        }
        let base = slot * KERNEL_STRIDE;
        packed[base..base + k.len()].copy_from_slice(&k);
        // KEY by the REFERENCE sigma (the value the schedule looks up); CONTENT/RADIUS are anchored.
        slots.push((
            ref_sigma,
            (slot * KERNEL_STRIDE) as i32,
            gaussian_radius(anchored, TRUNCATE) as i32,
        ));
    }
    Ok((packed, KernelParams { slots }))
}

/// Distinct gaussian sigmas the GRASSLAND recipe uses (recipes_grassland.rs::generate_seamsafe),
/// in a FIXED order. Order defines koffset; the orchestrator looks each up by value. The recipe's
/// blurs (read directly from the oracle):
///   * pre_swells   = gaussian(combo,           smoothing_px = 3.7)
///   * pans         = gaussian(1 - swells,      5.2)
///   * sandhill     = gaussian(pre,             1.55)            [_sandhill_field]
///   * escarpment   = gaussian(edge,            1.4)             [_escarpment_field]
///   * draws        = flow_channels(width=2.1, power=0.50): pre-blur 1.15 + spread max(2.1,0.1)=2.1
///   * floor smooth = gaussian(height, max(smoothing_px, 0.5) = 3.7)   [dup of smoothing_px]
///   * final blend  = gaussian(height, 1.1)
/// Deduped: 1.10, 1.15, 1.40, 1.55, 2.10, 3.70, 5.20.
pub(crate) fn grassland_sigmas() -> Vec<f64> {
    let smoothing_px = 3.7_f64;        // ROLLING_PRAIRIE.smoothing_px
    let floor_smooth = smoothing_px.max(0.5); // 3.7 (dedups against smoothing_px)
    let draw_spread = 2.1_f64.max(0.1);       // flow_channels width.max(0.1) = 2.1
    vec![1.10, 1.15, 1.40, 1.55, draw_spread, smoothing_px, 5.20, floor_smooth]
}

/// Distinct gaussian sigmas the DESERT recipe uses (recipes_desert.rs::generate_seamsafe), in a
/// FIXED order. Order defines koffset; the orchestrator looks each up by value. The recipe's
/// blurs (read directly from the oracle):
///   * basin        = gaussian(regional,              6.2)
///   * playa        = gaussian(basin,                 5.0)
///   * dunes        = gaussian(dune_raw,              0.70)            [_dune_field]
///   * block_cores  = gaussian(1 - block_edges,       3.2)
///   * mesa_blocks  = gaussian(regional,              2.2)
///   * washes       = flow_channels(width=1.8, power=0.43): pre-blur 1.15 + spread max(1.8,0.1)=1.8
///   * floor smooth = gaussian(height, max(floor_smooth_px=5.2, 0.2) = 5.2)
///   * final blend  = gaussian(height,               0.95)
/// Deduped: 0.70, 0.95, 1.15, 1.80, 2.20, 3.20, 5.00, 5.20, 6.20.
pub(crate) fn desert_sigmas() -> Vec<f64> {
    let floor_smooth = 5.2_f64.max(0.2);   // DUNE_SEA.floor_smooth_px.max(0.2)
    let wash_spread = 1.8_f64.max(0.1);     // flow_channels width.max(0.1) = 1.8
    vec![0.70, 0.95, 1.15, wash_spread, 2.20, 3.20, 5.00, floor_smooth, 6.20]
}

/// Distinct gaussian sigmas the COAST recipe uses (recipes_coast.rs::generate_seamsafe), in a
/// FIXED order. Order defines koffset; the orchestrator looks each up by value. The recipe's
/// blurs (read directly from the oracle):
///   * channels     = flow_channels_seam_safe(ridge_source, width=1.9, power=0.47):
///                    pre-blur 1.15 + spread max(1.9,0.1)=1.9
///   * islands      = gaussian(islands_seed,          2.0)
///   * smoothed_sea = gaussian(height,                3.0)
///   * final blend  = gaussian(height,                0.9)
/// Deduped: 0.90, 1.15, 1.90, 2.00, 3.00.
pub(crate) fn coast_sigmas() -> Vec<f64> {
    let channel_spread = 1.9_f64.max(0.1);  // flow_channels width.max(0.1) = 1.9
    vec![0.90, 1.15, channel_spread, 2.00, 3.00]
}

/// Distinct gaussian sigmas the WETLAND recipe uses (recipes_wetland.rs::generate_seamsafe), in a
/// FIXED order. Order defines koffset; the orchestrator looks each up by value. The recipe's
/// blurs (read directly from the oracle):
///   * basin        = gaussian(1 - macro,             5.8)
///   * floodplain   = gaussian(1 - |macro - 0.42|,    5.2)
///   * fine_flow    = flow_channels_seam_safe(flow_input, width=1.8, power=0.44):
///                    pre-blur 1.15 + spread max(1.8,0.1)=1.8
///   * levees       = gaussian(channels, 2.2) - gaussian(channels, 5.2)   [DoG; 5.2 dedups]
///   * flat_base    = gaussian(flat_base_inner, smoothing_px = 4.4)
///   * final blend  = gaussian(height,               1.2)
/// Deduped: 1.15, 1.20, 1.80, 2.20, 4.40, 5.20, 5.80.
pub(crate) fn wetland_sigmas() -> Vec<f64> {
    let smoothing_px = 4.4_f64;             // delta_distributary.smoothing_px (flat_base blur)
    let flow_spread = 1.8_f64.max(0.1);     // flow_channels width.max(0.1) = 1.8
    vec![1.15, 1.20, flow_spread, 2.20, smoothing_px, 5.20, 5.80]
}

/// Distinct gaussian sigmas the TUNDRA recipe uses (recipes_tundra.rs::generate_seamsafe), in a
/// FIXED order. Order defines koffset; the orchestrator looks each up by value. The recipe's
/// blurs (read directly from the oracle):
///   * plain        = gaussian(1 - |macro - 0.46|,    5.8)
///   * pattern      = gaussian(0.56*polygons + 0.44*stripes, 1.2)
///   * fringe       = gaussian(fringe_ridges,          1.8)
///   * channels     = flow_channels_seam_safe(flow_source, width=2.0, power=0.48):
///                    pre-blur 1.15 + spread max(2.0,0.1)=2.0
///   * base         = gaussian(base_inner,             smoothing_px = 5.0)
///   * final blend  = gaussian(height,                 1.1)
/// Deduped: 1.10, 1.15, 1.20, 1.80, 2.00, 5.00, 5.80.
pub(crate) fn tundra_sigmas() -> Vec<f64> {
    let smoothing_px = 5.0_f64;             // arctic_plain.smoothing_px (base blur)
    let flow_spread = 2.0_f64.max(0.1);     // flow_channels width.max(0.1) = 2.0
    vec![1.10, 1.15, 1.20, 1.80, flow_spread, smoothing_px, 5.80]
}

/// Distinct gaussian sigmas the GLACIAL recipe uses (recipes_glacial.rs::generate_seamsafe), in a
/// FIXED order. Order defines koffset; the orchestrator looks each up by value. The recipe's blurs
/// (read directly from the oracle, style = FJORDED_TROUGHS: trough_width_px=6.8, ice_smooth_px=6.2):
///   * relief       = gaussian(oriented_relief raw,      1.25)            [_oriented_relief trailing]
///   * relief_env   = gaussian(relief,                   5.8)
///   * icefield     = gaussian(0.56*regional+0.44*env,   7.0)
///   * massif       = gaussian(massif_inner,             2.8)
///   * flow_primary = trough_channels_seam_safe(base, width=6.8, power=0.58):
///                    PRE-BLUR 1.85 (NOT 1.15) + spread max(6.8,0.1)=6.8
///   * axial        = gaussian(axial_pre, max(trough_width_px*0.18, 0.8) = max(1.224,0.8) = 1.224)
///   * primary_mask blur  = gaussian(primary_mask,       1.6)             [branch_surface term]
///   * tributary    = trough_channels_seam_safe(branch_surface, width=max(6.8*0.48,0.8)=3.264,
///                    power=0.36): PRE-BLUR 1.85 + spread max(3.264,0.1)=3.264
///   * floor_mask blur    = gaussian(trough_floor,       1.6)             [dup of primary blur]
///   * floor        = gaussian(height, max(ice_smooth_px, 0.2) = 6.2)
///   * ice_smooth   = gaussian(height, max(ice_smooth_px*0.65, 0.2) = max(4.03,0.2) = 4.03)
///   * final blend  = gaussian(height,                   1.35)
/// Deduped: 1.224, 1.25, 1.35, 1.6, 1.85, 2.8, 3.264, 4.03, 5.8, 6.2, 6.8, 7.0. The 1.85 pre-blur
/// (glacial's machine-hook divergence) MUST be here so kparams pre-validation covers it.
pub(crate) fn glacial_sigmas() -> Vec<f64> {
    let trough_width_px = 6.8_f64;
    let axial_sigma = (trough_width_px * 0.18).max(0.8);   // 1.224
    let primary_spread = trough_width_px.max(0.1);          // 6.8
    let trib_width = (trough_width_px * 0.48).max(0.8);     // 3.264
    let trib_spread = trib_width.max(0.1);                  // 3.264
    let ice_smooth_px = 6.2_f64;
    let floor = ice_smooth_px.max(0.2);                     // 6.2
    let ice_smooth = (ice_smooth_px * 0.65).max(0.2);       // 4.03
    vec![axial_sigma, 1.25, 1.35, 1.6, 1.85, 2.8, trib_spread, ice_smooth, 5.8, floor, primary_spread, 7.0]
}

/// Distinct gaussian sigmas the KARST recipe uses (recipes_karst.rs::generate_seamsafe), in a
/// FIXED order. Order defines koffset; the orchestrator looks each up by value. The recipe's blurs
/// (read directly from the oracle, style = tower_karst: tower_width_px=2.0, doline_width_px=2.6,
/// floor_smooth_px=2.8):
///   * plateau      = gaussian(regional,                5.8)
///   * towers       = gaussian(sparse_pow, max(tower_width_px, 0.2) = 2.0)   [_tower_field]
///   * dolines      = gaussian(pits_pow,   max(doline_width_px, 0.2) = 2.6)  [_doline_field]
///   * cellular     = gaussian(cellular_edges raw,      3.8)
///   * dry_valleys  = flow_channels(width=2.6, power=0.54): pre-blur 1.15 + spread max(2.6,0.1)=2.6
///   * floor smooth = gaussian(height, max(floor_smooth_px=2.8, 0.2) = 2.8)
///   * final blend  = gaussian(height,                  0.95)
/// Deduped: 0.95, 1.15, 2.0, 2.6, 2.8, 3.8, 5.8. (the dv spread 2.6 dedups against doline_width_px).
pub(crate) fn karst_sigmas() -> Vec<f64> {
    let tower_width = 2.0_f64.max(0.2);       // tower_width_px.max(0.2) = 2.0
    let doline_width = 2.6_f64.max(0.2);      // doline_width_px.max(0.2) = 2.6
    let dv_spread = 2.6_f64.max(0.1);         // flow_channels width.max(0.1) = 2.6 (dedups doline_width)
    let floor_smooth = 2.8_f64.max(0.2);      // tower_karst.floor_smooth_px.max(0.2) = 2.8
    let _ = dv_spread;                         // identical to doline_width; not a distinct slot
    vec![0.95, 1.15, tower_width, doline_width, floor_smooth, 3.8, 5.8]
}

/// Distinct gaussian sigmas the TEMPERATE recipe uses (recipes_temperate.rs::generate_seamsafe),
/// in a FIXED order. Order defines koffset; the orchestrator looks each up by value. The recipe's
/// blurs (read directly from the oracle, style = appalachian_ridges: smoothing_px=1.8):
///   * ridges       = smoothstep(gaussian(folded_remap,           1.1))
///   * hills        = clip(affine(gaussian(hills_raw,             2.4)))
///   * upland       = smoothstep(gaussian(macro,                  4.2))
///   * discharge    = flow_discharge(power=0.43): PRE-BLUR 1.15 -> MFD -> log1p (NO spread)
///   * valleys      = smoothstep(gaussian(discharge,              1.8))   [first spread]
///   * broad_valleys= smoothstep(gaussian(discharge,              4.2))   [second spread; dedups upland]
///   * rounded      = gaussian(rounded_inner, max(smoothing_px=1.8,0.2) = 1.8)   [dedups valleys]
///   * final blend  = gaussian(height,                            1.0)
/// Deduped: 1.0, 1.1, 1.15, 1.8, 2.4, 4.2. TEMPERATE DIVERGENCE: the RAW-discharge flow uses the
/// SHARED pre-blur 1.15 (NOT a glacial-style custom pre-blur), so 1.15 MUST be present; the
/// two-spread sequencing is what's new, not the pre-blur.
pub(crate) fn temperate_sigmas() -> Vec<f64> {
    let smoothing_px = 1.8_f64.max(0.2);    // appalachian_ridges.smoothing_px (rounded blur; dedups valleys 1.8)
    let _ = smoothing_px;                    // identical to valleys spread 1.8; not a distinct slot
    vec![1.0, 1.1, 1.15, 1.8, 2.4, 4.2]
}

/// Distinct gaussian sigmas the RAINFOREST recipe uses (recipes_rainforest.rs::generate_seamsafe),
/// in a FIXED order. Order defines koffset; the orchestrator looks each up by value. The recipe's
/// blurs (read directly from the oracle, style = humid_dissected_hills: smoothing_px=2.6):
///   * hills        = clip(affine(gaussian(hills_raw,                1.7)))
///   * plateau      = smoothstep(gaussian(plateau_seed,              4.5)) * (1-0.38*ridges)
///   * lowland      = smoothstep(gaussian(1 - macro,                 5.4))
///   * discharge    = drainage_seam_safe(power=0.38): PRE-BLUR 1.15 -> MFD -> log1p (NO spread)
///   * tributaries  = smoothstep(0.42,0.88, gaussian(discharge,      1.15))  [first spread; dedups pre-blur]
///   * trunk        = smoothstep(0.68,0.95, gaussian(discharge,      2.2))   [second spread]
///   * wet_rounding = gaussian(wet_inner, max(smoothing_px=2.6,0.2) = 2.6)
///   * final blend  = gaussian(height,                               1.0)
/// Deduped: 1.0, 1.15, 1.7, 2.2, 2.6, 4.5, 5.4. RAINFOREST DUAL-MASK FLOW: like temperate, the
/// RAW-discharge flow uses the SHARED pre-blur 1.15, then spreads the SAME discharge at TWO sigmas
/// (1.15 for tributaries, 2.2 for trunk). The tributaries spread (1.15) dedups against the shared
/// pre-blur 1.15; the trunk spread (2.2) is its own distinct slot.
pub(crate) fn rainforest_sigmas() -> Vec<f64> {
    let smoothing_px = 2.6_f64.max(0.2);    // humid_dissected_hills.smoothing_px (wet_rounding blur)
    let _ = smoothing_px;                    // identical to the listed 2.6; not a separate slot
    vec![1.0, 1.15, 1.7, 2.2, 2.6, 4.5, 5.4]
}

/// Distinct gaussian sigmas the VOLCANIC recipe uses (recipes_volcanic.rs::generate_seamsafe), in a
/// FIXED order. Order defines koffset; the orchestrator looks each up by value. The recipe's blurs
/// (read directly from the oracle, style = stratovolcano_cluster):
///   * flows        = gaussian(vent flows raw,             1.1)              [_vent_fields trailing]
///   * gullies      = gully_channels_seam_safe(radial_surface, power=0.40):
///                    PRE-BLUR 1.15 (shared) + spread 1.2 (NOT max(width,0.1); a FIXED 1.2)
///   * spc_blur     = gaussian(shields + cones,            2.6)              [caldera bowl/rim]
///   * max_cf_blur  = gaussian(max(cones, flows),          3.0)              [ash_plain]
///   * smoothed_plain = gaussian(height,                   2.6)              [ash blend; dedups spc]
///   * final blend  = gaussian(height,                     0.85)
/// Deduped: 0.85, 1.1, 1.15, 1.2, 2.6, 3.0. VOLCANIC uses the SHARED pre-blur 1.15 (flow_discharge),
/// then a dedicated spread sigma=1.2 (the gully_channels_seam_safe FIXED spread, NOT the flow width)
/// -- so it spreads the RAW discharge once at 1.2 via the flow_discharge prefix + a separate gauss,
/// exactly like temperate/rainforest spread their raw discharge (minus the second spread).
pub(crate) fn volcanic_sigmas() -> Vec<f64> {
    vec![0.85, 1.1, 1.15, 1.2, 2.6, 3.0]
}

/// The COMPOSE layer's only gaussian sigma: the relief proxy at relief_sigma_px=6.0 (the
/// BlendConfig default). One slot in the packed kernel buffer. Used by `run_compose_inner`.
pub(crate) fn compose_sigmas() -> Vec<f64> {
    vec![COMPOSE_RELIEF_SIGMA]
}

/// Per-biome gaussian sigma list (FIXED order -> koffset). Add a biome's `*_sigmas()` arm here so
/// `run_inner` builds + pre-validates the right packed kernel buffer for that biome's schedule.
pub(crate) fn biome_sigmas(biome: &str) -> Option<Vec<f64>> {
    match biome {
        "mountain" => Some(mountain_sigmas()),
        "grassland" => Some(grassland_sigmas()),
        "desert" => Some(desert_sigmas()),
        "coast" => Some(coast_sigmas()),
        "wetland" => Some(wetland_sigmas()),
        "tundra" => Some(tundra_sigmas()),
        "glacial" => Some(glacial_sigmas()),
        "karst" => Some(karst_sigmas()),
        "temperate" => Some(temperate_sigmas()),
        "rainforest" => Some(rainforest_sigmas()),
        "volcanic" => Some(volcanic_sigmas()),
        _ => None,
    }
}

/// Resolved gaussian sigma -> (koffset, kradius) for the packed kernel buffer. The sigma set is
/// pre-validated (see `kp`) BEFORE the compute list opens, so the in-list lookups are
/// provably-unreachable failures. Stored as a small fixed Vec rather than a borrowed closure to
/// keep the borrow-checker happy across the open-list `&mut rd` reborrows.
#[derive(Clone)]
pub(crate) struct KernelParams {
    /// (sigma, koffset, kradius) in the FIXED `mountain_sigmas()` order.
    pub(crate) slots: Vec<(f64, i32, i32)>,
}

impl KernelParams {
    pub(crate) fn from_sigmas(sigmas: &[f64]) -> Self {
        let slots = sigmas
            .iter()
            .enumerate()
            .map(|(slot, &sg)| {
                (sg, (slot * KERNEL_STRIDE) as i32, gaussian_radius(sg, TRUNCATE) as i32)
            })
            .collect();
        Self { slots }
    }

    /// sigma -> (koffset, kradius). Pre-validated by `run_inner` before the list opens, so the
    /// `.expect` here is provably-unreachable inside the open compute list (same `.expect`
    /// semantics as the old `kparams` closure).
    pub(crate) fn kp(&self, sigma: f64) -> (i32, i32) {
        let (_, ko, kr) = self
            .slots
            .iter()
            .copied()
            .find(|&(s, _, _)| (s - sigma).abs() < 1e-9)
            .expect("sigma not in mountain_sigmas()");
        (ko, kr)
    }
}
