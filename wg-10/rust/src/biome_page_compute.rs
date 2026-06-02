//! WorldGen10 Slice-4a: GPU apron PAGE pipeline for the MOUNTAIN seam-safe recipe.
//!
//! `Wg10BiomePageCompute` mirrors `recipes.rs::mountain::generate_seamsafe` (the f64 parity
//! ORACLE) as a MULTI-DISPATCH GPU pipeline. Slice-4b concat-selection: it concatenates three
//! GLSL parts -- `recipe_primitives.glsl` (proven f32 noise/warp leaves) + `biome_page.glsl`
//! (the GENERIC pass machine: bindings, leaf helpers, generic passes + main()) + the selected
//! per-biome FRAGMENT `biome_<name>.glsl` (the biome-specific `biome_pass()` body) -- compiles
//! one compute shader per biome, and dispatches it once per pass with a different `pass`
//! push-constant. The primitives + machine are the STABLE two parts (loaded once via
//! `load_shaders`); the fragment is selected + concatenated per `generate_core_page` call.
//!
//! The whole-field operators become their own passes:
//!   * gaussian = separable (COPY src -> gauss_in, AXIS0 down rows, AXIS1 across cols),
//!     with the 1-D kernel built CPU-side (a port of `array_ops::gaussian_kernel1d`) and
//!     uploaded via `buffer_update` per distinct sigma (clamp-to-edge 'nearest', truncate
//!     4.0, radius int(truncate*sigma+0.5), normalized) -> EXACTLY array_ops.
//!   * flow accumulation = the PULL relaxation from `flow_accum_spike.glsl`, K=STABLE_ITERS
//!     ping-pong steps (an APPROXIMATION of the CPU sorted sweep; spec 4 Tier-2).
//!
//! Mirrors `primitive_probe.rs`/`flow_spike.rs` for the godot RenderingDevice API
//! (concat+strip+compile, storage buffers, uniform set, compute_list, submit/sync,
//! buffer_get_data, free + rd.free()). Readback happens ONLY in the `generate_core_page`
//! TEST entry (never the render path). WINDOWED only (local RD is null headless on this box).

use godot::prelude::*;
use godot::classes::{
    RenderingServer, RdShaderSource, RdUniform, RenderingDevice,
    rendering_device::{UniformType, ShaderStage},
};

// ---------------------------------------------------------------------------
// pass selector codes -- MUST match biome_page.glsl PASS_* consts.
// ---------------------------------------------------------------------------
const PASS_MESHGRID: i32 = 0;
const PASS_POINTWISE: i32 = 1;
const PASS_COPY: i32 = 2;
const PASS_GAUSS_AXIS0: i32 = 3;
const PASS_GAUSS_AXIS1: i32 = 4;
const PASS_RANGE_ENV: i32 = 5;
const PASS_LOWLAND: i32 = 6;
const PASS_MASSIF_INNER: i32 = 7;
const PASS_BASE: i32 = 8;
const PASS_FLOW_PRE_BASE: i32 = 9;
const PASS_FLOW_PRE_ROUGH: i32 = 10;
const PASS_FLOW_RELAX: i32 = 11;
const PASS_DISCHARGE: i32 = 12;
const PASS_PRIMARY_MASK: i32 = 13;
const PASS_TRIB_MASK: i32 = 14;
const PASS_MASKS: i32 = 15;
const PASS_ASSEMBLE: i32 = 16;
const PASS_FLOOR_MASK: i32 = 17;
const PASS_FLOOR_BLEND: i32 = 18;
const PASS_FINAL: i32 = 19;
const PASS_CROP: i32 = 20;
const PASS_FLOW_PRE_PREBLUR_IN: i32 = 21;
const PASS_FLOW_PRE_FROM_GAUSS: i32 = 22;
const PASS_MASSIF_WRITEBACK: i32 = 23;
const PASS_ACC_INIT: i32 = 24;
const PASS_COPY_POOL: i32 = 25;       // gauss_in <- pool[pool_sel] (to blur a pool slot)
/// Generic capability for biomes that need to stash a blur back into a slot. Grassland/desert/coast
/// read the blur straight from gauss_out (no stash); WETLAND uses it (stash gaussian(channels,2.2)
/// for the levee DoG, and flat_base back into its slot). Matches biome_page.glsl PASS_POOL_FROM_GAUSS.
const PASS_POOL_FROM_GAUSS: i32 = 26; // pool[pool_sel] <- gauss_out (stash a blur)

// GRASSLAND biome-private PASS_* codes (start at 32) -- MUST match biome_grassland.glsl GL_*.
const GL_POINTWISE: i32 = 32;
const GL_COMBO: i32 = 33;
const GL_SWELLS: i32 = 34;
const GL_ONE_MINUS_SWELLS: i32 = 35;
const GL_PANS: i32 = 36;
const GL_SANDHILL_PRE: i32 = 37;
const GL_SANDHILL_FINAL: i32 = 38;
const GL_ESC_PRE: i32 = 39;
const GL_ESC_FINAL: i32 = 40;
const GL_BASE_FOR_FLOW: i32 = 41;
const GL_DRAWS: i32 = 42;
const GL_TEXTURE: i32 = 43;
const GL_ASSEMBLE: i32 = 44;
const GL_OPEN_FLOOR_BLEND: i32 = 45;
const GL_FINAL: i32 = 46;

// DESERT biome-private PASS_* codes (start at 32) -- MUST match biome_desert.glsl DS_*.
const DS_POINTWISE: i32 = 32;
const DS_BASIN: i32 = 33;
const DS_PLAYA: i32 = 34;
const DS_DUNE_PRE: i32 = 35;
const DS_DUNE_FINAL: i32 = 36;
const DS_YARDANG: i32 = 37;
const DS_BLOCK_PRE: i32 = 38;
const DS_BLOCK_CORES: i32 = 39;
const DS_MESAS: i32 = 40;
const DS_BASE: i32 = 41;
const DS_WASH_FLOW_PRE: i32 = 42;
const DS_WASH_FINAL: i32 = 43;
const DS_FINE_SALT: i32 = 44;
const DS_ASSEMBLE: i32 = 45;
const DS_FLOOR_BLEND: i32 = 46;
const DS_FINAL: i32 = 47;

// COAST biome-private PASS_* codes (start at 32) -- MUST match biome_coast.glsl CO_*.
const CO_POINTWISE: i32 = 32;
const CO_FLOW_PRE: i32 = 33;
const CO_CHANNELS: i32 = 34;
const CO_CHANNEL_RELIEF: i32 = 35;
const CO_ISLANDS_SEED: i32 = 36;
const CO_ISLANDS: i32 = 37;
const CO_ASSEMBLE: i32 = 38;
const CO_SEA_BLEND: i32 = 39;
const CO_FINAL: i32 = 40;

// WETLAND biome-private PASS_* codes (start at 32) -- MUST match biome_wetland.glsl WL_*.
const WL_POINTWISE: i32 = 32;
const WL_ONE_MINUS_MACRO: i32 = 33;
const WL_BASIN: i32 = 34;
const WL_FLOODPLAIN_PRE: i32 = 35;
const WL_FLOODPLAIN: i32 = 36;
const WL_CHANNELS_FIRST: i32 = 37;
const WL_FLOW_PRE: i32 = 38;
const WL_CHANNELS_FLOW: i32 = 39;
const WL_LEVEES: i32 = 40;
const WL_FLAT_BASE_PRE: i32 = 41;
const WL_ASSEMBLE: i32 = 42;
const WL_FINAL: i32 = 43;

// TUNDRA biome-private PASS_* codes (start at 32) -- MUST match biome_tundra.glsl TU_*.
const TU_POINTWISE: i32 = 32;
const TU_PLAIN_PRE: i32 = 33;
const TU_PLAIN: i32 = 34;
const TU_PATTERN_PRE: i32 = 35;
const TU_PATTERN: i32 = 36;
const TU_FRINGE: i32 = 37;
const TU_FLOW_PRE: i32 = 38;
const TU_DRAINAGE: i32 = 39;
const TU_BASE_PRE: i32 = 40;
const TU_ASSEMBLE: i32 = 41;
const TU_FINAL: i32 = 42;

// GLACIAL biome-private PASS_* codes (start at 32) -- MUST match biome_glacial.glsl GC_*.
const GC_POINTWISE: i32 = 32;
const GC_RELIEF_RAW: i32 = 33;
const GC_RELIEF: i32 = 34;
const GC_RELIEF_ENV: i32 = 35;
const GC_ICE_INNER: i32 = 36;
const GC_ICEFIELD: i32 = 37;
const GC_MASSIF_INNER: i32 = 38;
const GC_MASSIF: i32 = 39;
const GC_BASE: i32 = 40;
const GC_FLOW_PRE_PRIMARY: i32 = 41;
const GC_FLOW_PRIMARY_STASH: i32 = 42;
const GC_AXIAL_RAW: i32 = 43;
const GC_AXIAL: i32 = 44;
const GC_PRIMARY_MASK: i32 = 45;
const GC_BRANCH_SURFACE: i32 = 46;
const GC_TRIB_MASK: i32 = 47;
const GC_SCRAPES: i32 = 48;
const GC_ASSEMBLE: i32 = 49;
const GC_FLOOR_MASK: i32 = 50;
const GC_FLOOR_BLEND: i32 = 51;
const GC_ICE_BLEND: i32 = 52;
const GC_FINAL: i32 = 53;

// KARST biome-private PASS_* codes (start at 32) -- MUST match biome_karst.glsl KS_*.
const KS_POINTWISE: i32 = 32;
const KS_PLATEAU: i32 = 33;
const KS_TOWER_PRE: i32 = 34;
const KS_TOWER_FINAL: i32 = 35;
const KS_DOLINE_PRE: i32 = 36;
const KS_DOLINE_FINAL: i32 = 37;
const KS_LINEAMENTS: i32 = 38;
const KS_CELLULAR_RAW: i32 = 39;
const KS_CELLULAR: i32 = 40;
const KS_COCKPIT_NOISE: i32 = 41;
const KS_COCKPIT: i32 = 42;
const KS_BASE: i32 = 43;
const KS_FINE_KARREN: i32 = 44;
const KS_DV_SURFACE: i32 = 45;
const KS_DV_FINAL: i32 = 46;
const KS_MASKS: i32 = 47;
const KS_ASSEMBLE: i32 = 48;
const KS_FLOOR_MASK: i32 = 49;
const KS_FLOOR_BLEND: i32 = 50;
const KS_FINAL: i32 = 51;

// copy_sel codes -- MUST match biome_page.glsl CP_* consts.
const CP_RANGES: i32 = 0;
const CP_MASSIF: i32 = 1;
const CP_VALLEY: i32 = 2;
const CP_HEIGHT: i32 = 3;

/// GENERIC scratch-pool slot count -- MUST match biome_page.glsl POOL_SLOTS. One additional
/// storage buffer is allocated + bound per slot (bindings 24..24+POOL_SLOTS-1), reusable by ANY
/// biome that needs more sub-fields than the fixed named buffers. Grassland uses all 12; to add a
/// biome needing more, bump this AND the GLSL POOL_SLOTS together. Mountain ignores the pool
/// entirely (its named buffers are untouched), so this is purely additive. Desert needs 16
/// (grassland uses 12); the 4 extra slots (12..15) are simply unused by mountain/grassland.
const POOL_SLOTS: usize = 16;

/// scipy gaussian truncate (array_ops::TRUNCATE).
const TRUNCATE: f64 = 4.0;

/// Flow PULL-relaxation step count. The flow-accum spike converged at 128 (memory
/// worldgen10-m3-rough-streaming-spike / flow_spike). This is the APPROXIMATION knob:
/// raise it if the parity gate's channel-region delta exceeds the Tier-2 epsilon.
const STABLE_ITERS: usize = 128;

// ---------------------------------------------------------------------------
// CPU gaussian kernel: port of array_ops::gaussian_kernel1d. The GLSL gaussian passes
// use this uploaded kernel; it MUST match the Rust oracle bit-for-bit (radius / truncate
// / phi / normalization), or Tier-2 height parity drifts.
// ---------------------------------------------------------------------------

/// scipy `_gaussian_kernel1d(sigma, order=0, radius=lw)`: normalized half-width-`lw`
/// Gaussian taps indexed `0..=2*lw` (offsets `-lw..=lw`). Port of array_ops::gaussian_kernel1d.
/// `lw = int(truncate*sigma + 0.5)` (truncation toward zero); `phi[x]=exp(-0.5/sigma^2 * x^2)`;
/// normalized so sum == 1. Computed in f64 then narrowed to f32 for upload.
pub fn gaussian_kernel1d(sigma: f64, truncate: f64) -> Vec<f32> {
    let lw_i: i64 = (truncate * sigma + 0.5) as i64; // int(...) truncates toward zero
    let lw = lw_i.max(0) as usize;
    let sigma2 = sigma * sigma;
    let size = 2 * lw + 1;
    let mut phi = Vec::with_capacity(size);
    let mut sum = 0.0_f64;
    for k in 0..size {
        let x = (k as i64 - lw as i64) as f64;
        let v = (-0.5 / sigma2 * x * x).exp();
        phi.push(v);
        sum += v;
    }
    phi.iter().map(|&v| (v / sum) as f32).collect()
}

/// Kernel half-width `lw` for a given sigma/truncate (kernel length = 2*lw+1). Mirror of
/// array_ops radius `int(truncate*sigma + 0.5)` (clamped >= 0).
pub fn gaussian_radius(sigma: f64, truncate: f64) -> usize {
    let lw_i: i64 = (truncate * sigma + 0.5) as i64;
    lw_i.max(0) as usize
}

/// Working-grid (padded) dim helper: core + an apron on each side.
pub fn apron_dim(core_px: usize, apron_px: usize) -> usize {
    core_px + 2 * apron_px
}

// ---------------------------------------------------------------------------
// byte helpers
// ---------------------------------------------------------------------------

fn f32s_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for &x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

fn bytes_to_f32s(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

/// Biome selector from a fragment path: the file stem with a leading `biome_` stripped.
/// e.g. ".../biome_mountain.glsl" -> "mountain", ".../biome_grassland.glsl" -> "grassland".
/// Falls back to the bare stem (then the whole string) if the conventions don't match, so the
/// `run_inner` match arm reports a precise "no schedule for biome '<x>'" error.
fn biome_stem(path: &str) -> String {
    let file = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path);
    let stem = file.strip_suffix(".glsl").unwrap_or(file);
    stem.strip_prefix("biome_").unwrap_or(stem).to_string()
}

fn make_storage_uniform(binding: i32, rid: Rid) -> Gd<RdUniform> {
    let mut u = RdUniform::new_gd();
    u.set_uniform_type(UniformType::STORAGE_BUFFER);
    u.set_binding(binding);
    u.add_id(rid);
    u
}

/// Build the 96-byte push constant (std430): 12 i32 (48B) then 12 f32 (48B).
/// Layout MUST match biome_page.glsl Params.
#[allow(clippy::too_many_arguments)]
fn build_push(
    pass: i32,
    rows: i32,
    cols: i32,
    apron_px: i32,
    seed: i32,
    kradius: i32,
    copy_sel: i32,
    flow_dir: i32,
    koffset: i32,
    pool_sel: i32,
    spacing: f32,
    ox: f32,
    oz: f32,
    feature_span_m: f32,
    flow_power: f32,
) -> Vec<u8> {
    let mut b = Vec::with_capacity(96);
    // 12 ints: pass,rows,cols,apron,seed,kradius,copy_sel,flow_dir,koffset,pool_sel + 2 pad.
    for v in [pass, rows, cols, apron_px, seed, kradius, copy_sel, flow_dir, koffset, pool_sel, 0, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    // 12 floats: spacing,ox,oz,feature_span_m,flow_power + 7 pad.
    for v in [spacing, ox, oz, feature_span_m, flow_power] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for _ in 0..7 {
        b.extend_from_slice(&0.0_f32.to_le_bytes());
    }
    b
}

/// Distinct gaussian sigmas the mountain recipe uses, in a FIXED order. Each gets a slot in
/// the packed kernel buffer at index `slot * KERNEL_STRIDE`. (valley_width=2.4, trib=0.6
/// after max(.,0.6), floor_smooth=4.0 -- but 4.0 already appears, and 0.6/2.4 are distinct.)
/// Order here defines koffset; the orchestrator looks each sigma up by value.
const KERNEL_STRIDE: usize = 64;
/// sigma list (deduped): 1.15, 1.20, 1.80, 2.00, 5.00, 7.00, 2.40 (valley), 0.60 (trib width
/// = max(2.4*0.42,0.6)=1.008 -> actually 1.008; floor_smooth=4.0 distinct). See sigma_slots().
fn mountain_sigmas() -> Vec<f64> {
    let valley_width_px = 2.4_f64;
    let trib_width = (valley_width_px * 0.42).max(0.6); // 1.008
    let floor_smooth = 4.0_f64.max(0.2);
    // All distinct sigmas used by run_gaussian / run_flow_channels.
    vec![1.15, 1.20, 1.80, 2.00, 5.00, 7.00, valley_width_px, trib_width, floor_smooth]
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
fn grassland_sigmas() -> Vec<f64> {
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
fn desert_sigmas() -> Vec<f64> {
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
fn coast_sigmas() -> Vec<f64> {
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
fn wetland_sigmas() -> Vec<f64> {
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
fn tundra_sigmas() -> Vec<f64> {
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
fn glacial_sigmas() -> Vec<f64> {
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
fn karst_sigmas() -> Vec<f64> {
    let tower_width = 2.0_f64.max(0.2);       // tower_width_px.max(0.2) = 2.0
    let doline_width = 2.6_f64.max(0.2);      // doline_width_px.max(0.2) = 2.6
    let dv_spread = 2.6_f64.max(0.1);         // flow_channels width.max(0.1) = 2.6 (dedups doline_width)
    let floor_smooth = 2.8_f64.max(0.2);      // tower_karst.floor_smooth_px.max(0.2) = 2.8
    let _ = dv_spread;                         // identical to doline_width; not a distinct slot
    vec![0.95, 1.15, tower_width, doline_width, floor_smooth, 3.8, 5.8]
}

/// Per-biome gaussian sigma list (FIXED order -> koffset). Add a biome's `*_sigmas()` arm here so
/// `run_inner` builds + pre-validates the right packed kernel buffer for that biome's schedule.
fn biome_sigmas(biome: &str) -> Option<Vec<f64>> {
    match biome {
        "mountain" => Some(mountain_sigmas()),
        "grassland" => Some(grassland_sigmas()),
        "desert" => Some(desert_sigmas()),
        "coast" => Some(coast_sigmas()),
        "wetland" => Some(wetland_sigmas()),
        "tundra" => Some(tundra_sigmas()),
        "glacial" => Some(glacial_sigmas()),
        "karst" => Some(karst_sigmas()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Scheduler: the per-biome dispatch SEAM. Holds all per-dispatch state so a biome's pass
// chain can live in a standalone `schedule_<biome>()` fn (instead of inline closures/macros).
// `run_inner` allocates buffers + opens ONE compute list, builds a Scheduler, then hands it
// to the selected schedule fn. Every future biome adds a `schedule_<biome>()` + one match arm.
//
// IMPORTANT: this is a PURE code-structure seam. `dispatch`/`gauss`/`flow_channels` carry the
// SAME bodies the old `dispatch` closure + `gauss!`/`flow_channels!` macros had; the GPU
// dispatch sequence, push-constant values, STABLE_ITERS loop, and discharge_fd invariant are
// byte-identical to the pre-refactor inline schedule.
// ---------------------------------------------------------------------------

/// Resolved gaussian sigma -> (koffset, kradius) for the packed kernel buffer. The sigma set is
/// pre-validated (see `kp`) BEFORE the compute list opens, so the in-list lookups are
/// provably-unreachable failures. Stored as a small fixed Vec rather than a borrowed closure to
/// keep the borrow-checker happy across the open-list `&mut rd` reborrows.
struct KernelParams {
    /// (sigma, koffset, kradius) in the FIXED `mountain_sigmas()` order.
    slots: Vec<(f64, i32, i32)>,
}

impl KernelParams {
    fn from_sigmas(sigmas: &[f64]) -> Self {
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
    fn kp(&self, sigma: f64) -> (i32, i32) {
        let (_, ko, kr) = self
            .slots
            .iter()
            .copied()
            .find(|&(s, _, _)| (s - sigma).abs() < 1e-9)
            .expect("sigma not in mountain_sigmas()");
        (ko, kr)
    }
}

/// Per-dispatch state for one open compute list. Built once `run_inner` has the list open; the
/// schedule fn drives it. `cl` matches the type `compute_list_begin()` returns (i64 in the
/// Godot 4.6 bindings).
struct Scheduler<'a> {
    rd: &'a mut Gd<RenderingDevice>,
    cl: i64,
    uset: Rid,
    rows: i32,
    cols: i32,
    apron: i32,
    seed: i32,
    spacing: f32,
    ox: f32,
    oz: f32,
    feature_span_m: f32,
    wg_full_x: u32,
    wg_full_y: u32,
    wg_core_x: u32,
    wg_core_y: u32,
    kparams: KernelParams,
}

impl<'a> Scheduler<'a> {
    /// One full pass dispatch + trailing barrier (so the next reader sees the writes). Same body
    /// as the old `dispatch` closure, plus the additive `pool_sel` push-constant for the generic
    /// pool passes (0 for every mountain dispatch -> byte-identical to the pre-pool push for
    /// mountain, since pool_sel maps to a former int-pad slot).
    #[allow(clippy::too_many_arguments)]
    fn dispatch(
        &mut self,
        pass: i32,
        kradius: i32,
        koffset: i32,
        copy_sel: i32,
        flow_dir: i32,
        flow_power: f32,
        pool_sel: i32,
        wgx: u32,
        wgy: u32,
    ) {
        self.rd.compute_list_bind_uniform_set(self.cl, self.uset, 0);
        let pc = PackedByteArray::from(
            build_push(
                pass, self.rows, self.cols, self.apron, self.seed, kradius, copy_sel, flow_dir,
                koffset, pool_sel, self.spacing, self.ox, self.oz, self.feature_span_m, flow_power,
            )
            .as_slice(),
        );
        self.rd.compute_list_set_push_constant(self.cl, &pc, pc.len() as u32);
        self.rd.compute_list_dispatch(self.cl, wgx, wgy, 1);
        self.rd.compute_list_add_barrier(self.cl);
    }

    /// Full-field dispatch (the overwhelmingly common case): wgx/wgy = full padded dims, and the
    /// no-kernel/no-copy/no-flow/no-pool params default to 0. Convenience wrapper so schedule fns
    /// read cleanly.
    fn dispatch_full(&mut self, pass: i32, copy_sel: i32, flow_dir: i32, flow_power: f32) {
        self.dispatch(pass, 0, 0, copy_sel, flow_dir, flow_power, 0, self.wg_full_x, self.wg_full_y);
    }

    /// Full-field POOL copy/stash dispatch (pool_sel selects the slot). Used by COPY_POOL (slot
    /// -> gauss_in) and POOL_FROM_GAUSS (gauss_out -> slot) so a biome can blur ANY pool slot.
    fn dispatch_pool(&mut self, pass: i32, pool_sel: i32) {
        self.dispatch(pass, 0, 0, 0, 0, 0.0, pool_sel, self.wg_full_x, self.wg_full_y);
    }

    /// gaussian(sigma) on gauss_in -> gauss_out (AXIS0 then AXIS1, packed kernel by koffset).
    /// Same body as the old `gauss!` macro.
    fn gauss(&mut self, sigma: f64) {
        let (ko, kr) = self.kparams.kp(sigma);
        self.dispatch(PASS_GAUSS_AXIS0, kr, ko, 0, 0, 0.0, 0, self.wg_full_x, self.wg_full_y);
        self.dispatch(PASS_GAUSS_AXIS1, kr, ko, 0, 0, 0.0, 0, self.wg_full_x, self.wg_full_y);
    }

    /// Blur a scratch-pool slot in place: COPY_POOL(slot) -> gauss_in, gaussian(sigma), then the
    /// blur lives in gauss_out (the caller reads gauss_out, or POOL_FROM_GAUSS stashes it back).
    fn gauss_pool(&mut self, slot: i32, sigma: f64) {
        self.dispatch_pool(PASS_COPY_POOL, slot);
        self.gauss(sigma);
    }

    /// flow_channels_seam_safe(flow_pre, width_px, power): pre-blur 1.15 -> K relax ->
    /// log1p discharge -> spread gaussian(width). Leaves spread discharge in gauss_out.
    /// Same body as the old `flow_channels!` macro (incl STABLE_ITERS loop + discharge_fd
    /// invariant). Thin wrapper over `flow_channels_ex` with the SHARED pre-blur sigma=1.15
    /// (the 6 proven biomes call THIS -> byte-identical dispatch sequence as before the refactor).
    fn flow_channels(&mut self, power: f32, width: f64) {
        self.flow_channels_ex(power, width, 1.15);
    }

    /// flow_channels_seam_safe with a PARAMETERIZED pre-blur sigma (the machine hook GLACIAL
    /// needs: its troughs pre-blur with sigma=1.85, NOT the shared 1.15). Identical body to the
    /// old `flow_channels` otherwise (pre-blur -> K relax -> log1p discharge -> spread
    /// gaussian(width)). `preblur_sigma` MUST be present in the biome's `*_sigmas()` list so
    /// `kparams` pre-validation covers it (the `gauss(preblur_sigma)` below resolves a kernel slot).
    fn flow_channels_ex(&mut self, power: f32, width: f64, preblur_sigma: f64) {
        // pre-blur sigma=preblur_sigma (1.15 for the shared path; 1.85 for glacial)
        self.dispatch_full(PASS_FLOW_PRE_PREBLUR_IN, 0, 0, 0.0);
        self.gauss(preblur_sigma);
        self.dispatch_full(PASS_FLOW_PRE_FROM_GAUSS, 0, 0, 0.0);
        // acc init = 1.0 (both buffers)
        self.dispatch_full(PASS_ACC_INIT, 0, 0, 0.0);
        // K ping-pong relaxation steps. In PASS_FLOW_RELAX, flow_dir selects the WRITE
        // target: fd=0 reads acc_a writes acc_b; fd=1 reads acc_b writes acc_a. The last
        // step is i=STABLE_ITERS-1, fd=(STABLE_ITERS-1)%2, so it writes:
        //   STABLE_ITERS even -> last fd=1 -> final result in acc_a
        //   STABLE_ITERS odd  -> last fd=0 -> final result in acc_b
        for i in 0..STABLE_ITERS {
            let fd = if i % 2 == 0 { 0 } else { 1 };
            self.dispatch_full(PASS_FLOW_RELAX, 0, fd, power);
        }
        // PASS_DISCHARGE: here flow_dir selects the READ buffer holding the final acc
        // (OPPOSITE of PASS_FLOW_RELAX, where it selects the write target) -> fd=0 reads
        // acc_a, fd=1 reads acc_b. So discharge_fd must equal the parity of the LAST write:
        //   STABLE_ITERS odd  -> final in acc_b -> discharge_fd=1
        //   STABLE_ITERS even -> final in acc_a -> discharge_fd=0
        // This trap is live ONLY if STABLE_ITERS changes (the flagged convergence knob).
        let discharge_fd: i32 = if STABLE_ITERS % 2 == 1 { 1 } else { 0 };
        debug_assert_eq!(
            discharge_fd,
            1 - ((STABLE_ITERS as i32 - 1) % 2),
            "discharge_fd must read the buffer the LAST relax step wrote"
        );
        self.dispatch_full(PASS_DISCHARGE, 0, discharge_fd, 0.0);
        // spread sigma = max(width, 0.1) (all widths here are >= 0.1)
        self.gauss(width.max(0.1));
    }
}

/// The MOUNTAIN dispatch schedule (style = ALPINE_BRANCHING). EXACTLY the pre-refactor sequence
/// + params; this is the reference pattern every future biome `schedule_<biome>()` copies. The
/// constants here (valley_width_px/trib_width/floor_smooth) mirror `mountain_sigmas()` so the
/// gauss/flow widths resolve to pre-validated kernel slots.
fn schedule_mountain(s: &mut Scheduler) {
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

    // 6) primary channels: flow_channels_seam_safe(base, valley_width, power=0.48)
    s.dispatch_full(PASS_FLOW_PRE_BASE, 0, 0, 0.0);
    s.flow_channels(0.48_f32, valley_width_px);
    s.dispatch_full(PASS_PRIMARY_MASK, 0, 0, 0.0);

    // 7) tributaries: flow_channels_seam_safe(rough_surface, trib_width, power=0.34)
    s.dispatch_full(PASS_FLOW_PRE_ROUGH, 0, 0, 0.0);
    s.flow_channels(0.34_f32, trib_width);
    s.dispatch_full(PASS_TRIB_MASK, 0, 0, 0.0);

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

/// The GRASSLAND dispatch schedule (style = ROLLING_PRAIRIE). Mirrors the field DAG of
/// recipes_grassland.rs::generate_seamsafe ONE-FOR-ONE: warp+macro/secondary -> swells (blur) ->
/// pans (blur 1-swells) -> sandhills/escarpments (whole-field sub-pipelines) -> base_for_flow ->
/// draws (flow channels) -> fine_grain/low_ripple -> assemble -> floor blend -> final. All
/// intermediate fields live in the GENERIC scratch POOL (pool0..pool11; see biome_grassland.glsl
/// for the slot map). The sigmas (smoothing_px=3.7, 5.2, 1.55, 1.4, flow pre-blur 1.15 + spread
/// 2.1, floor 3.7, final 1.1) are all in grassland_sigmas(). This is the PATTERN the other 9 ports
/// copy: pointwise passes write pool slots; blur a slot via gauss_pool(slot,sigma) then read
/// gauss_out (or POOL_FROM_GAUSS to stash); flow channels reuse the proven flow_channels().
fn schedule_grassland(s: &mut Scheduler) {
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

/// The DESERT dispatch schedule (style = DUNE_SEA). Mirrors the field DAG of
/// recipes_desert.rs::generate_seamsafe ONE-FOR-ONE: warp+regional -> basin (blur 1-regional) ->
/// playa (blur basin) -> dunes (whole-field sub-pipeline) -> yardangs (pointwise) ->
/// block_cores/mesas -> base_surface -> washes (flow channels) -> fine/salt -> assemble ->
/// floor blend -> final. All intermediate fields live in the GENERIC scratch POOL (pool0..pool15;
/// see biome_desert.glsl for the slot map). The sigmas (6.2, 5.0, 0.70, 3.2, 2.2, flow pre-blur
/// 1.15 + spread 1.8, floor 5.2, final 0.95) are all in desert_sigmas(). Same PATTERN as
/// schedule_grassland: pointwise passes write pool slots; blur a slot via gauss_pool(slot,sigma)
/// then read gauss_out; flow channels reuse the proven flow_channels().
fn schedule_desert(s: &mut Scheduler) {
    let floor_smooth = 5.2_f64.max(0.2);

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; regional=pool2
    s.dispatch_full(DS_POINTWISE, 0, 0, 0.0);

    // 2) basin = smoothstep(0.34,0.78, 1 - gaussian(regional, 6.2))
    s.gauss_pool(2, 6.2);                            // gauss_out = gaussian(regional, 6.2)
    s.dispatch_full(DS_BASIN, 0, 0, 0.0);            // pool3 = basin

    // 3) playa = smoothstep(0.56,0.90, gaussian(basin, 5.0))
    s.gauss_pool(3, 5.0);                            // gauss_out = gaussian(basin, 5.0)
    s.dispatch_full(DS_PLAYA, 0, 0, 0.0);            // pool4 = playa

    // 4) dunes sub-pipeline: raw (pool15) -> gaussian(0.70) -> clip(affine(., DUNE)) = pool5
    s.dispatch_full(DS_DUNE_PRE, 0, 0, 0.0);         // pool15 = dune raw
    s.gauss_pool(15, 0.70);                          // gauss_out = gaussian(pool15, 0.70)
    s.dispatch_full(DS_DUNE_FINAL, 0, 0, 0.0);       // pool5 = dunes

    // 5) yardangs (pointwise, no blur) = pool6
    s.dispatch_full(DS_YARDANG, 0, 0, 0.0);

    // 6) block_cores: pre (pool12=1-block_edges, pool13=rocky_relief) -> gaussian(3.2) -> pool14
    s.dispatch_full(DS_BLOCK_PRE, 0, 0, 0.0);        // pool12 = 1-block_edges ; pool13 = rocky_relief
    s.gauss_pool(12, 3.2);                           // gauss_out = gaussian(1-block_edges, 3.2)
    s.dispatch_full(DS_BLOCK_CORES, 0, 0, 0.0);      // pool14 = block_cores

    // 7) mesas = clip(0.68*mesa_blocks + 0.32*rocky_relief*(1-0.42*basin)); mesa_blocks uses
    //    gaussian(regional, 2.2) * block_cores * (1-0.68*basin)
    s.gauss_pool(2, 2.2);                            // gauss_out = gaussian(regional, 2.2)
    s.dispatch_full(DS_MESAS, 0, 0, 0.0);            // pool7 = mesas

    // 8) base_surface = affine(0.72*regional + 0.24*mesas - 0.62*basin, BASE) = pool8
    s.dispatch_full(DS_BASE, 0, 0, 0.0);

    // 9) washes = smoothstep(0.57,0.94, flow_channels(base_surface+0.16*mesas, width=1.8,
    //    power=0.43)) * (0.35 + 0.65*(1 - playa))    [flow_channels leaves spread in gauss_out]
    s.dispatch_full(DS_WASH_FLOW_PRE, 0, 0, 0.0);    // flow_pre <- base_surface + 0.16*mesas
    s.flow_channels(0.43_f32, 1.8);
    s.dispatch_full(DS_WASH_FINAL, 0, 0, 0.0);       // pool9 = washes

    // 10) fine (pool10) + salt (pool11), pointwise on w_x/w_z
    s.dispatch_full(DS_FINE_SALT, 0, 0, 0.0);

    // 11) assemble height (base + dune/yardang/wash/playa/mesa relief + detail)
    s.dispatch_full(DS_ASSEMBLE, 0, 0, 0.0);

    // 12) floor blend: smooth_floor = gaussian(height, max(floor_smooth_px,0.2)=5.2); floor blend
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(floor_smooth);                           // gauss_out = gaussian(height, floor_smooth)
    s.dispatch_full(DS_FLOOR_BLEND, 0, 0, 0.0);

    // 13) final: height_blur = gaussian(height, 0.95); final_blend = 0.82*h + 0.18*blur; affine
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(0.95);                                   // gauss_out = gaussian(height, 0.95)
    s.dispatch_full(DS_FINAL, 0, 0, 0.0);

    // 14) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

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
fn schedule_coast(s: &mut Scheduler) {
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
fn schedule_wetland(s: &mut Scheduler) {
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
fn schedule_tundra(s: &mut Scheduler) {
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

/// The GLACIAL dispatch schedule (style = fjorded_troughs). Mirrors the field DAG of
/// recipes_glacial.rs::generate_seamsafe ONE-FOR-ONE: warp+regional/ridge_detail/close_detail ->
/// oriented_relief (raw -> blur 1.25) -> relief_env (blur 5.8) -> icefield (blur 7.0) -> massif
/// (raw -> blur 2.8) -> base -> flow_primary (TROUGH flow, pre-blur 1.85) -> axial (raw -> blur
/// 1.224) -> primary_mask -> branch_surface (uses gaussian(primary_mask,1.6)) -> tributary (TROUGH
/// flow, pre-blur 1.85) + trib_mask -> scrapes -> assemble -> floor/ice masks + blends -> final.
/// All intermediate fields live in the GENERIC scratch POOL (pool0..pool15; pool15 is the transient
/// pre-blur staging slot; pool10/pool11/pool7 are REUSED post-mask; see biome_glacial.glsl for the
/// slot map). GLACIAL DIVERGENCE: its trough flow uses flow_channels_ex(power, width, 1.85) (the
/// machine-hook), NOT the shared flow_channels (1.15) -- 1.85 is in glacial_sigmas(). The sigmas
/// (1.224, 1.25, 1.35, 1.6, 1.85, 2.8, 3.264, 4.03, 5.8, 6.2, 6.8, 7.0) are all in glacial_sigmas().
/// Same PATTERN as schedule_tundra: pointwise passes write pool slots; blur a slot via
/// gauss_pool(slot,sigma) then read gauss_out (or POOL_FROM_GAUSS to stash); flow channels reuse
/// the proven flow_channels_ex().
fn schedule_glacial(s: &mut Scheduler) {
    let trough_width_px = 6.8_f64;
    let axial_sigma = (trough_width_px * 0.18).max(0.8);    // 1.224
    let primary_spread = trough_width_px;                    // 6.8 (flow_channels width.max(0.1)=6.8)
    let trib_width = (trough_width_px * 0.48).max(0.8);      // 3.264
    let ice_smooth_px = 6.2_f64;
    let floor_smooth = ice_smooth_px.max(0.2);              // 6.2
    let ice_smooth = (ice_smooth_px * 0.65).max(0.2);       // 4.03

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; regional=pool2 ; ridge_detail=pool3 ; close_detail=pool4
    s.dispatch_full(GC_POINTWISE, 0, 0, 0.0);

    // 2) relief = gaussian(oriented_relief raw, 1.25)
    s.dispatch_full(GC_RELIEF_RAW, 0, 0, 0.0);       // pool15 = oriented_relief raw
    s.gauss_pool(15, 1.25);                          // gauss_out = gaussian(pool15, 1.25)
    s.dispatch_full(GC_RELIEF, 0, 0, 0.0);           // pool5 = relief

    // 3) relief_env = smoothstep(0.22,0.62, gaussian(relief, 5.8))
    s.gauss_pool(5, 5.8);                            // gauss_out = gaussian(relief, 5.8)
    s.dispatch_full(GC_RELIEF_ENV, 0, 0, 0.0);       // pool6 = relief_env

    // 4) icefield = smoothstep(0.48,0.78, gaussian(0.56*regional + 0.44*relief_env, 7.0))
    s.dispatch_full(GC_ICE_INNER, 0, 0, 0.0);        // gauss_in <- ice_inner
    s.gauss(7.0);                                    // gauss_out = gaussian(ice_inner, 7.0)
    s.dispatch_full(GC_ICEFIELD, 0, 0, 0.0);         // pool7 = icefield

    // 5) massif = gaussian(massif_inner, 2.8)
    s.dispatch_full(GC_MASSIF_INNER, 0, 0, 0.0);     // pool15 = massif_inner
    s.gauss_pool(15, 2.8);                           // gauss_out = gaussian(pool15, 2.8)
    s.dispatch_full(GC_MASSIF, 0, 0, 0.0);           // pool8 = massif

    // 6) base = affine(uplift*(1.34*massif + 0.22*relief - 0.16*(1-icefield)), BASE)
    s.dispatch_full(GC_BASE, 0, 0, 0.0);             // pool9 = base

    // 7) flow_primary = trough_channels_seam_safe(base, width=6.8, power=0.58, PRE-BLUR 1.85)
    s.dispatch_full(GC_FLOW_PRE_PRIMARY, 0, 0, 0.0); // flow_pre <- base
    s.flow_channels_ex(0.58_f32, primary_spread, 1.85); // gauss_out = spread discharge (sigma=6.8)
    s.dispatch_full(GC_FLOW_PRIMARY_STASH, 0, 0, 0.0); // pool10 = flow_primary

    // 8) axial = gaussian(axial_troughs raw, max(trough_width_px*0.18, 0.8) = 1.224)
    s.dispatch_full(GC_AXIAL_RAW, 0, 0, 0.0);        // pool15 = axial raw
    s.gauss_pool(15, axial_sigma);                   // gauss_out = gaussian(pool15, 1.224)
    s.dispatch_full(GC_AXIAL, 0, 0, 0.0);            // pool11 = axial

    // 9) primary_mask = smoothstep(0.34,0.84, clip(affine(0.58*flow_primary + 1.18*axial, PRIMARY)))
    s.dispatch_full(GC_PRIMARY_MASK, 0, 0, 0.0);     // pool12 = primary_mask

    // 10) tributary = trough_channels_seam_safe(branch_surface, width=3.264, power=0.36, PRE-BLUR 1.85)
    //     branch_surface = base + 0.10*affine(relief,RELIEF_ZSCORE) - 0.18*gaussian(primary_mask,1.6)
    s.gauss_pool(12, 1.6);                           // gauss_out = gaussian(primary_mask, 1.6)
    s.dispatch_full(GC_BRANCH_SURFACE, 0, 0, 0.0);   // flow_pre <- branch_surface (uses gauss_out)
    s.flow_channels_ex(0.36_f32, trib_width, 1.85);  // gauss_out = spread discharge (sigma=3.264)
    s.dispatch_full(GC_TRIB_MASK, 0, 0, 0.0);        // pool13 = tributary_mask

    // 11) scrapes = striations raw (pointwise, no blur)
    s.dispatch_full(GC_SCRAPES, 0, 0, 0.0);          // pool14 = scrapes

    // 12) assemble height (base + ridge/detail/striation - trough - branch; trough_floor->pool10,
    //     high_ice->pool11)
    s.dispatch_full(GC_ASSEMBLE, 0, 0, 0.0);

    // 13) floor/ice masks: floor_mask = clip(smoothstep(0.36,0.80, gaussian(trough_floor,1.6)));
    //     ice_mask = clip(smoothstep(0.50,0.90, high_ice)) -> pool7
    s.gauss_pool(10, 1.6);                           // gauss_out = gaussian(trough_floor, 1.6)
    s.dispatch_full(GC_FLOOR_MASK, 0, 0, 0.0);       // floor_mask (named buf) ; pool7 = ice_mask

    // 14) floor blend: floor = gaussian(height, max(ice_smooth_px,0.2)=6.2); blend by floor_mask
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(floor_smooth);                           // gauss_out = gaussian(height, 6.2)
    s.dispatch_full(GC_FLOOR_BLEND, 0, 0, 0.0);

    // 15) ice blend: ice_smooth = gaussian(height, max(ice_smooth_px*0.65,0.2)=4.03); blend by
    //     ice_mask; then height -= 0.16*floor_mask
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(ice_smooth);                             // gauss_out = gaussian(height, 4.03)
    s.dispatch_full(GC_ICE_BLEND, 0, 0, 0.0);

    // 16) final: height_blur = gaussian(height, 1.35); final_blend = 0.66*h + 0.34*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(1.35);                                   // gauss_out = gaussian(height, 1.35)
    s.dispatch_full(GC_FINAL, 0, 0, 0.0);

    // 17) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

/// The KARST dispatch schedule (style = tower_karst). Mirrors the field DAG of
/// recipes_karst.rs::generate_seamsafe ONE-FOR-ONE: warp+regional -> plateau (blur 5.8) -> towers
/// (raw sparse_pow -> blur 2.0) -> dolines (raw pits_pow -> blur 2.6) -> lineaments (pointwise) ->
/// cellular (raw -> blur 3.8) -> cockpit_noise (pointwise) -> cockpit (pointwise) -> base ->
/// fine/karren (pointwise; REUSE the dead regional/cellular slots) -> dry_valleys (SHARED flow
/// channels, pre-blur 1.15, spread 2.6) -> masks (tower/cockpit/doline/lineament, tower modulated
/// by doline_mask + dry_valleys) -> assemble -> floor mask + blend -> final. All intermediate
/// fields live in the GENERIC scratch POOL (pool0..pool15; pool15 is the transient blur-staging
/// slot, then REUSED for lineament_mask; pool2/pool7 are REUSED for fine/karren post-base; see
/// biome_karst.glsl for the slot map). KARST uses the PROVEN flow_channels (pre-blur 1.15), NOT the
/// flow_channels_ex hook -- its "custom" flow is just power=0.54, width=2.6 (the spread sigma is the
/// existing width param). The sigmas (0.95, 1.15, 2.0, 2.6, 2.8, 3.8, 5.8) are all in karst_sigmas().
/// Same PATTERN as schedule_desert/glacial: pointwise passes write pool slots; blur a slot via
/// gauss_pool(slot,sigma) then read gauss_out; flow channels reuse the proven flow_channels().
fn schedule_karst(s: &mut Scheduler) {
    let tower_width = 2.0_f64.max(0.2);      // tower_width_px.max(0.2) = 2.0
    let doline_width = 2.6_f64.max(0.2);     // doline_width_px.max(0.2) = 2.6
    let floor_smooth = 2.8_f64.max(0.2);     // floor_smooth_px.max(0.2) = 2.8

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; regional=pool2
    s.dispatch_full(KS_POINTWISE, 0, 0, 0.0);

    // 2) plateau = smoothstep(0.30,0.72, gaussian(regional, 5.8))
    s.gauss_pool(2, 5.8);                            // gauss_out = gaussian(regional, 5.8)
    s.dispatch_full(KS_PLATEAU, 0, 0, 0.0);          // pool3 = plateau

    // 3) towers sub-pipeline: sparse_pow (pool15) -> gaussian(2.0) -> clip(affine(., TOWER_FINAL)) = pool4
    s.dispatch_full(KS_TOWER_PRE, 0, 0, 0.0);        // pool15 = sparse_pow
    s.gauss_pool(15, tower_width);                   // gauss_out = gaussian(pool15, 2.0)
    s.dispatch_full(KS_TOWER_FINAL, 0, 0, 0.0);      // pool4 = towers

    // 4) dolines sub-pipeline: pits_pow (pool15) -> gaussian(2.6) -> clip(affine(., DOLINE_BOWLS)) = pool5
    s.dispatch_full(KS_DOLINE_PRE, 0, 0, 0.0);       // pool15 = pits_pow
    s.gauss_pool(15, doline_width);                  // gauss_out = gaussian(pool15, 2.6)
    s.dispatch_full(KS_DOLINE_FINAL, 0, 0, 0.0);     // pool5 = dolines

    // 5) lineaments (pointwise, no blur) = pool6
    s.dispatch_full(KS_LINEAMENTS, 0, 0, 0.0);

    // 6) cellular = gaussian(cellular_edges raw, 3.8)
    s.dispatch_full(KS_CELLULAR_RAW, 0, 0, 0.0);     // pool15 = cellular_raw
    s.gauss_pool(15, 3.8);                           // gauss_out = gaussian(pool15, 3.8)
    s.dispatch_full(KS_CELLULAR, 0, 0, 0.0);         // pool7 = cellular

    // 7) cockpit_noise (pointwise) = pool8 ; cockpit (pointwise, uses dolines/cellular/cockpit_noise) = pool9
    s.dispatch_full(KS_COCKPIT_NOISE, 0, 0, 0.0);    // pool8 = cockpit_noise
    s.dispatch_full(KS_COCKPIT, 0, 0, 0.0);          // pool9 = cockpit

    // 8) base = affine(plateau_gain*(1.06*plateau + 0.18*regional), BASE) = pool10
    s.dispatch_full(KS_BASE, 0, 0, 0.0);

    // 9) fine/karren (pointwise on w_x/w_z); REUSE pool2 (regional dead) = fine, pool7 (cellular dead) = karren
    s.dispatch_full(KS_FINE_KARREN, 0, 0, 0.0);

    // 10) dry_valleys: flow_pre <- base - 0.30*lineaments - 0.10*dolines ; dry_valleys =
    //     flow_channels(width=2.6, power=0.54) [pre-blur 1.15] ; then smoothstep + scale = pool11
    s.dispatch_full(KS_DV_SURFACE, 0, 0, 0.0);       // flow_pre <- dv_surface (NO clip)
    s.flow_channels(0.54_f32, 2.6);                  // gauss_out = spread discharge (sigma=2.6)
    s.dispatch_full(KS_DV_FINAL, 0, 0, 0.0);         // pool11 = dry_valleys

    // 11) masks: cockpit_mask=pool13, doline_mask=pool14, lineament_mask=pool15 (REUSE),
    //     tower_mask=pool12 (finalized w/ doline_mask + dry_valleys)
    s.dispatch_full(KS_MASKS, 0, 0, 0.0);

    // 12) assemble height (base + tower/lineament relief - cockpit/doline/valley + detail)
    s.dispatch_full(KS_ASSEMBLE, 0, 0, 0.0);

    // 13) floor mask + blend: floor_mask = clip(0.72*doline_mask + 0.56*cockpit_mask + 0.48*dry_valleys);
    //     smoothed_floor = gaussian(height, max(floor_smooth_px,0.2)=2.8); floor blend
    s.dispatch_full(KS_FLOOR_MASK, 0, 0, 0.0);       // floor_mask (named buf)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(floor_smooth);                           // gauss_out = gaussian(height, 2.8)
    s.dispatch_full(KS_FLOOR_BLEND, 0, 0, 0.0);

    // 14) final: height_blur = gaussian(height, 0.95); final_blend = 0.80*h + 0.20*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(0.95);                                   // gauss_out = gaussian(height, 0.95)
    s.dispatch_full(KS_FINAL, 0, 0, 0.0);

    // 15) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10BiomePageCompute {
    primitives_src: Option<String>,
    /// The GENERIC machine (biome_page.glsl): bindings + leaf helpers + generic passes + main().
    /// One of the two STABLE parts (the other being primitives); loaded once via load_shaders.
    machine_src: Option<String>,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10BiomePageCompute {
    fn init(base: Base<RefCounted>) -> Self {
        Self { primitives_src: None, machine_src: None, base }
    }
}

#[godot_api]
impl Wg10BiomePageCompute {
    /// Load the two STABLE GLSL parts (primitives helpers + the GENERIC machine) from OS paths
    /// and keep them. The per-biome FRAGMENT is loaded separately, per call, by
    /// `generate_core_page` (it selects which biome to bake). At compile time all three are
    /// concatenated as primitives + machine + fragment (Godot GLSL has no #include). Returns ""
    /// on success, an error string otherwise. Mirrors `Wg10PrimitiveProbe::load_shader`.
    #[func]
    pub fn load_shaders(&mut self, primitives_path: GString, machine_path: GString) -> GString {
        let prim = match std::fs::read_to_string(primitives_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(format!("primitives glsl: {e}").as_str()),
        };
        let machine = match std::fs::read_to_string(machine_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(format!("machine glsl: {e}").as_str()),
        };
        self.primitives_src = Some(prim);
        self.machine_src = Some(machine);
        GString::new()
    }

    /// Run the FULL mountain pass chain for ONE page (style = ALPINE_BRANCHING, matching the
    /// fixture's `style_key`) on a local RenderingDevice and return the CORE f64 height
    /// (length core_rows*core_cols, NORMALIZED recipe units, pre-relief). The apron meshgrid
    /// is rebuilt on the GPU from (spacing, ox, oz, apron_px, padded dims). Readback ONLY
    /// here (test entry). Returns an EMPTY array on error (see godot_error log).
    #[allow(clippy::too_many_arguments)]
    #[func]
    pub fn generate_core_page(
        &self,
        spacing: f64,
        ox: f64,
        oz: f64,
        padded_rows: i64,
        padded_cols: i64,
        apron_px: i64,
        seed: i64,
        feature_span_m: f64,
        biome_fragment_path: GString,
    ) -> PackedFloat64Array {
        let rows = padded_rows as usize;
        let cols = padded_cols as usize;
        let apron = apron_px as usize;
        // The GLSL lattice hash is 32-bit-seed throughout (push constant `int seed`), so a seed
        // outside i32 range cannot reach the GPU intact. Fail LOUDLY instead of silently
        // truncating (which would diverge from the i64 CPU oracle without warning). Real fixtures
        // use small seeds; this guards future records / callers.
        if seed < i32::MIN as i64 || seed > i32::MAX as i64 {
            godot_error!("Wg10BiomePageCompute::generate_core_page: seed {seed} outside i32 range (GPU hash is 32-bit-seed); CPU oracle is i64 -> parity impossible. Use a seed in i32 range.");
            return PackedFloat64Array::new();
        }
        // Load the selected per-biome FRAGMENT (the biome_pass() body) for this call.
        let frag_path = biome_fragment_path.to_string();
        let fragment = match std::fs::read_to_string(&frag_path) {
            Ok(s) => s,
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_core_page: biome fragment glsl: {e}");
                return PackedFloat64Array::new();
            }
        };
        // Biome selector = the fragment path stem with a leading `biome_` stripped, e.g.
        // ".../biome_mountain.glsl" -> "mountain". `run_inner` matches on this to pick the
        // per-biome schedule fn.
        let biome = biome_stem(&frag_path);
        match self.run_inner(
            spacing as f32, ox as f32, oz as f32, rows, cols, apron, seed as i32, feature_span_m as f32,
            &fragment, &biome,
        ) {
            Ok(core) => {
                let mut out = PackedFloat64Array::new();
                out.resize(core.len());
                let sl = out.as_mut_slice();
                for i in 0..core.len() {
                    sl[i] = core[i] as f64;
                }
                out
            }
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_core_page error: {e}");
                PackedFloat64Array::new()
            }
        }
    }

    // ---- internal GPU pipeline ----
    #[allow(clippy::too_many_arguments)]
    fn run_inner(
        &self,
        spacing: f32,
        ox: f32,
        oz: f32,
        rows: usize,
        cols: usize,
        apron: usize,
        seed: i32,
        feature_span_m: f32,
        biome_fragment: &str,
        biome: &str,
    ) -> Result<Vec<f32>, String> {
        if rows <= 2 * apron || cols <= 2 * apron {
            return Err(format!("apron {apron} too large for padded {rows}x{cols}"));
        }
        let prim = self.primitives_src.as_deref().ok_or("no GLSL source loaded")?;
        let machine = self.machine_src.as_deref().ok_or("no GLSL source loaded")?;
        let n = rows * cols;
        let core_rows = rows - 2 * apron;
        let core_cols = cols - 2 * apron;
        let core_n = core_rows * core_cols;

        let mut rd: Gd<RenderingDevice> = RenderingServer::singleton()
            .create_local_rendering_device()
            .ok_or_else(|| {
                "create_local_rendering_device returned null (headless / no device)".to_string()
            })?;

        // --- compile: concat primitives + machine + biome fragment, strip #[...] lines AND hoist
        // #version to line 1. The machine (NOT the primitives helpers, NOT the fragment) carries
        // the single #version; concat_glsl_hoist_version scans the WHOLE joined text and pulls the
        // first #version to line 1, so passing (primitives, machine + "\n" + fragment) keeps the
        // machine's #version as the first non-helper line and appends the fragment last. ---
        let machine_plus_fragment = format!("{machine}\n{biome_fragment}");
        let glsl_stripped = crate::primitive_probe::concat_glsl_hoist_version(prim, &machine_plus_fragment);
        let mut src = RdShaderSource::new_gd();
        src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
        let spirv = match rd.shader_compile_spirv_from_source(&src) {
            Some(s) => s,
            None => {
                rd.free();
                return Err("shader_compile_spirv_from_source returned null".to_string());
            }
        };
        {
            let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
            if !err.is_empty() {
                rd.free();
                return Err(format!("GLSL compile error: {err}"));
            }
        }
        let shader = rd.shader_create_from_spirv(&spirv);
        if shader.is_invalid() {
            rd.free();
            return Err("shader_create_from_spirv returned invalid RID".into());
        }

        // --- allocate buffers ---
        let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };
        let field_bytes = n * 4;
        let zeros = vec![0.0_f32; n];
        let zeros_pba = PackedByteArray::from(f32s_to_bytes(&zeros).as_slice());
        let mk_field = |rd: &mut Gd<RenderingDevice>| -> Rid {
            rd.storage_buffer_create_ex(bsize(field_bytes)).data(&zeros_pba).done()
        };
        let b_wx = mk_field(&mut rd);          // 0
        let b_wz = mk_field(&mut rd);          // 1
        let b_regional = mk_field(&mut rd);    // 2
        let b_ranges = mk_field(&mut rd);      // 3
        let b_ridge_detail = mk_field(&mut rd);// 4
        let b_near_detail = mk_field(&mut rd); // 5
        let b_range_env = mk_field(&mut rd);   // 6
        let b_lowland = mk_field(&mut rd);     // 7
        let b_massif = mk_field(&mut rd);      // 8
        let b_base = mk_field(&mut rd);        // 9
        let b_primary = mk_field(&mut rd);     // 10
        let b_trib = mk_field(&mut rd);        // 11
        let b_high = mk_field(&mut rd);        // 12
        let b_valley = mk_field(&mut rd);      // 13
        let b_height = mk_field(&mut rd);      // 14
        let b_floor = mk_field(&mut rd);       // 15
        let b_gauss_in = mk_field(&mut rd);    // 16
        let b_gauss_mid = mk_field(&mut rd);   // 17
        let b_gauss_out = mk_field(&mut rd);   // 18

        // PACKED kernel buffer (19): all distinct sigmas' kernels at fixed offsets
        // (slot * KERNEL_STRIDE). Built + uploaded ONCE so the whole pipeline runs inside a
        // SINGLE compute list (no mid-list buffer_update); the active kernel is selected by
        // the `koffset` push constant. Build the kernels in the fixed sigma order. The sigma SET
        // is biome-specific (each schedule_<biome> requests its own blurs); pick it BEFORE the
        // list opens so a wrong/missing biome errors cleanly.
        let sigmas = match biome_sigmas(biome) {
            Some(s) => s,
            None => {
                rd.free_rid(shader);
                rd.free();
                return Err(format!("no sigma list for biome '{biome}' (add a biome_sigmas arm)"));
            }
        };
        let n_slots = sigmas.len();
        let mut packed = vec![0.0_f32; n_slots * KERNEL_STRIDE];
        for (slot, &sg) in sigmas.iter().enumerate() {
            let k = gaussian_kernel1d(sg, TRUNCATE);
            if k.len() > KERNEL_STRIDE {
                rd.free_rid(shader);
                rd.free();
                return Err(format!("gaussian kernel len {} (sigma {sg}) > KERNEL_STRIDE {KERNEL_STRIDE}", k.len()));
            }
            let base = slot * KERNEL_STRIDE;
            packed[base..base + k.len()].copy_from_slice(&k);
        }
        let packed_pba = PackedByteArray::from(f32s_to_bytes(&packed).as_slice());
        let b_kernel = rd
            .storage_buffer_create_ex(bsize(packed.len() * 4))
            .data(&packed_pba)
            .done(); // 19
        // sigma -> (koffset, kradius) lookup, resolved BEFORE the compute list opens. Stored as a
        // small fixed Vec (KernelParams) so the in-list `.expect` is provably-unreachable and the
        // borrow-checker is happy across the open-list `&mut rd` reborrows in the Scheduler.
        let kparams = KernelParams::from_sigmas(&sigmas);

        let b_flow_pre = mk_field(&mut rd);    // 20
        let b_acc_a = mk_field(&mut rd);       // 21
        let b_acc_b = mk_field(&mut rd);       // 22

        // core output (23)
        let core_zeros = vec![0.0_f32; core_n];
        let core_pba = PackedByteArray::from(f32s_to_bytes(&core_zeros).as_slice());
        let b_core = rd
            .storage_buffer_create_ex(bsize(core_n * 4))
            .data(&core_pba)
            .done(); // 23

        // GENERIC scratch POOL (bindings 24..24+POOL_SLOTS-1): POOL_SLOTS reusable field buffers
        // any biome can stage sub-fields in (grassland uses all 12). Allocated for EVERY biome so
        // the uniform set always satisfies the machine's pool bindings (mountain just never reads
        // them -> its result is unchanged). Additive: the fixed named buffers above are untouched.
        let b_pool: Vec<Rid> = (0..POOL_SLOTS).map(|_| mk_field(&mut rd)).collect();

        // one uniform set binding the 24 fixed buffers + POOL_SLOTS pool buffers (build once).
        let mut bindings: Vec<(i32, Rid)> = vec![
            (0, b_wx), (1, b_wz), (2, b_regional), (3, b_ranges), (4, b_ridge_detail),
            (5, b_near_detail), (6, b_range_env), (7, b_lowland), (8, b_massif), (9, b_base),
            (10, b_primary), (11, b_trib), (12, b_high), (13, b_valley), (14, b_height),
            (15, b_floor), (16, b_gauss_in), (17, b_gauss_mid), (18, b_gauss_out),
            (19, b_kernel), (20, b_flow_pre), (21, b_acc_a), (22, b_acc_b), (23, b_core),
        ];
        for (k, &rid) in b_pool.iter().enumerate() {
            bindings.push((24 + k as i32, rid));
        }
        let mut uniforms: Array<Gd<RdUniform>> = Array::new();
        for (bind, rid) in bindings.iter() {
            uniforms.push(&make_storage_uniform(*bind, *rid));
        }
        let uset = rd.uniform_set_create(&uniforms, shader, 0);
        let pipeline = rd.compute_pipeline_create(shader);

        // workgroup counts (local_size 16x16). full-field uses padded dims; crop uses core.
        let wg_full_x = (cols as u32).div_ceil(16);
        let wg_full_y = (rows as u32).div_ceil(16);
        let wg_core_x = (core_cols as u32).div_ceil(16);
        let wg_core_y = (core_rows as u32).div_ceil(16);

        // PRE-VALIDATE every sigma the pipeline will request, BEFORE the compute list is open:
        // KernelParams::kp uses `.expect()`, and a panic AFTER compute_list_begin would unwind
        // with an active list and leak the local RD. Every sigma a schedule_<biome> asks for MUST
        // be in that biome's `*_sigmas()` (the per-biome unit tests, e.g.
        // `mountain_sigmas_cover_all_pipeline_blurs` / `grassland_sigmas_cover_all_pipeline_blurs`,
        // guard this); resolving the whole list here proves the in-list lookups cannot fail.
        for &s in &sigmas {
            let _ = kparams.kp(s);
        }

        // ===== record the WHOLE pipeline into ONE compute list, with a barrier after every
        // dependent dispatch (the proven flow_spike pattern). Then submit + sync once. The
        // per-biome dispatch SEQUENCE lives in a standalone `schedule_<biome>()` fn, driven via
        // the Scheduler seam. =====
        let cl = rd.compute_list_begin();
        rd.compute_list_bind_compute_pipeline(cl, pipeline);

        // Build the Scheduler over the open list, then run the selected biome's schedule. The
        // schedule fns own the dispatch SEQUENCE (byte-identical to the old inline schedule).
        let mut sched = Scheduler {
            rd: &mut rd,
            cl,
            uset,
            rows: rows as i32,
            cols: cols as i32,
            apron: apron as i32,
            seed,
            spacing,
            ox,
            oz,
            feature_span_m,
            wg_full_x,
            wg_full_y,
            wg_core_x,
            wg_core_y,
            kparams,
        };
        // Biome selector (derived from the fragment path stem in generate_core_page). Each biome
        // adds a `schedule_<name>()` + one match arm here + a `*_sigmas()` arm in `biome_sigmas`.
        match biome {
            "mountain" => schedule_mountain(&mut sched),
            "grassland" => schedule_grassland(&mut sched),
            "desert" => schedule_desert(&mut sched),
            "coast" => schedule_coast(&mut sched),
            "wetland" => schedule_wetland(&mut sched),
            "tundra" => schedule_tundra(&mut sched),
            "glacial" => schedule_glacial(&mut sched),
            "karst" => schedule_karst(&mut sched),
            other => {
                // drop the Scheduler's &mut borrow before freeing the RD.
                let _ = sched;
                rd.compute_list_end();
                rd.submit();
                rd.sync();
                for (_, rid) in bindings.iter() {
                    rd.free_rid(*rid);
                }
                rd.free_rid(pipeline);
                rd.free_rid(shader);
                rd.free();
                return Err(format!("no schedule for biome '{other}'"));
            }
        }

        rd.compute_list_end();
        rd.submit();
        rd.sync();

        // --- read back the core ---
        let core_out_pba = rd.buffer_get_data(b_core);
        let core = bytes_to_f32s(&core_out_pba.to_vec());

        // --- free everything ---
        for (_, rid) in bindings.iter() {
            rd.free_rid(*rid);
        }
        rd.free_rid(pipeline);
        rd.free_rid(shader); // cascades the uniform set
        rd.free();

        if core.len() != core_n {
            return Err(format!("core readback: expected {core_n} f32, got {}", core.len()));
        }
        Ok(core)
    }
}

// ---------------------------------------------------------------------------
// Unit tests (pure helpers; no Godot runtime)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod biome_page_compute_tests {
    use super::*;

    #[test]
    fn kernel_sums_to_one() {
        for &sigma in &[1.0_f64, 1.15, 1.2, 1.8, 2.0, 5.0, 7.0, 2.4] {
            let k = gaussian_kernel1d(sigma, TRUNCATE);
            let s: f64 = k.iter().map(|&v| v as f64).sum();
            assert!((s - 1.0).abs() < 1e-5, "sigma {sigma}: sum {s} != 1");
        }
    }

    #[test]
    fn kernel_is_symmetric() {
        let k = gaussian_kernel1d(2.0, TRUNCATE);
        let n = k.len();
        for i in 0..n {
            assert!((k[i] - k[n - 1 - i]).abs() < 1e-7, "kernel not symmetric at {i}");
        }
    }

    #[test]
    fn kernel_length_matches_radius() {
        // array_ops: lw = int(truncate*sigma + 0.5); length = 2*lw+1.
        // sigma 1.0, truncate 4.0 -> lw = int(4.5) = 4 -> length 9.
        let k = gaussian_kernel1d(1.0, TRUNCATE);
        assert_eq!(k.len(), 9);
        assert_eq!(gaussian_radius(1.0, TRUNCATE), 4);
        // sigma 7.0 -> lw = int(28.5) = 28 -> length 57.
        assert_eq!(gaussian_radius(7.0, TRUNCATE), 28);
        assert_eq!(gaussian_kernel1d(7.0, TRUNCATE).len(), 57);
        // sigma 2.4 -> lw = int(10.1) = 10 -> length 21.
        assert_eq!(gaussian_radius(2.4, TRUNCATE), 10);
    }

    #[test]
    fn kernel_center_is_peak() {
        let k = gaussian_kernel1d(2.0, TRUNCATE);
        let lw = (k.len() - 1) / 2;
        for i in 0..k.len() {
            assert!(k[lw] >= k[i], "center not peak");
        }
    }

    #[test]
    fn all_mountain_kernels_fit_stride() {
        for &sg in &mountain_sigmas() {
            let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
            assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
        }
    }

    #[test]
    fn mountain_sigmas_cover_all_pipeline_blurs() {
        // every sigma the pass chain asks for must be present (kparams panics otherwise).
        let valley = 2.4_f64;
        let trib = (valley * 0.42_f64).max(0.6);
        let floor = 4.0_f64.max(0.2);
        let s = mountain_sigmas();
        for need in [1.15_f64, 1.20, 1.80, 2.00, 5.00, 7.00, valley, trib, floor, valley.max(0.1), trib.max(0.1)] {
            assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
        }
    }

    #[test]
    fn apron_dim_adds_two_aprons() {
        assert_eq!(apron_dim(24, 160), 344);
        assert_eq!(apron_dim(256, 160), 576);
    }

    #[test]
    fn push_constant_is_96_bytes() {
        let p = build_push(0, 344, 344, 160, 0, 4, 0, 0, 0, 0, 3913.04, 12000.0, -31000.0, 90000.0, 0.48);
        assert_eq!(p.len(), 96);
    }

    #[test]
    fn push_constant_packs_ints_then_floats() {
        // build_push(pass,rows,cols,apron,seed,kradius,copy_sel,flow_dir,koffset,pool_sel,spacing,ox,oz,span,power)
        let p = build_push(7, 344, 343, 160, 5, 28, 2, 1, 128, 9, 3913.0, 12000.0, -31000.0, 90000.0, 0.34);
        assert_eq!(i32::from_le_bytes([p[0], p[1], p[2], p[3]]), 7);
        assert_eq!(i32::from_le_bytes([p[4], p[5], p[6], p[7]]), 344);
        assert_eq!(i32::from_le_bytes([p[8], p[9], p[10], p[11]]), 343);
        assert_eq!(i32::from_le_bytes([p[12], p[13], p[14], p[15]]), 160);
        assert_eq!(i32::from_le_bytes([p[16], p[17], p[18], p[19]]), 5);
        assert_eq!(i32::from_le_bytes([p[20], p[21], p[22], p[23]]), 28);
        assert_eq!(i32::from_le_bytes([p[24], p[25], p[26], p[27]]), 2);
        assert_eq!(i32::from_le_bytes([p[28], p[29], p[30], p[31]]), 1);
        assert_eq!(i32::from_le_bytes([p[32], p[33], p[34], p[35]]), 128); // koffset
        assert_eq!(i32::from_le_bytes([p[36], p[37], p[38], p[39]]), 9);   // pool_sel
        // 2 int pad at 40..48; floats start at byte 48.
        let spacing = f32::from_le_bytes([p[48], p[49], p[50], p[51]]);
        assert!((spacing - 3913.0).abs() < 1e-1);
        // floats: spacing(48),ox(52),oz(56),span(60),power(64)
        let flow_power = f32::from_le_bytes([p[64], p[65], p[66], p[67]]);
        assert!((flow_power - 0.34).abs() < 1e-6);
    }

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
}
