//! Shader ABI constants for the WG10 biome page compute machine.
//!
//! These values are coupled to `biome_page.glsl` and the per-biome fragments. Keeping them in one
//! module makes the Rust/shader boundary explicit while leaving scheduling and runtime ownership in
//! the parent module.

// ---------------------------------------------------------------------------
// pass selector codes -- MUST match biome_page.glsl PASS_* consts.
// ---------------------------------------------------------------------------
pub(super) const PASS_MESHGRID: i32 = 0;
pub(super) const PASS_POINTWISE: i32 = 1;
pub(super) const PASS_COPY: i32 = 2;
pub(super) const PASS_GAUSS_AXIS0: i32 = 3;
pub(super) const PASS_GAUSS_AXIS1: i32 = 4;
pub(super) const PASS_RANGE_ENV: i32 = 5;
pub(super) const PASS_LOWLAND: i32 = 6;
pub(super) const PASS_MASSIF_INNER: i32 = 7;
pub(super) const PASS_BASE: i32 = 8;
pub(super) const PASS_FLOW_PRE_BASE: i32 = 9;
pub(super) const PASS_FLOW_PRE_ROUGH: i32 = 10;
pub(super) const PASS_FLOW_RELAX: i32 = 11;
pub(super) const PASS_DISCHARGE: i32 = 12;
pub(super) const PASS_PRIMARY_MASK: i32 = 13;
pub(super) const PASS_TRIB_MASK: i32 = 14;
pub(super) const PASS_MASKS: i32 = 15;
pub(super) const PASS_ASSEMBLE: i32 = 16;
pub(super) const PASS_FLOOR_MASK: i32 = 17;
pub(super) const PASS_FLOOR_BLEND: i32 = 18;
pub(super) const PASS_FINAL: i32 = 19;
pub(super) const PASS_CROP: i32 = 20;
/// RUNTIME crop-to-texture pass (sibling of PASS_CROP). Writes the core into the R32F output
/// image at binding 41 instead of the core_out storage buffer. MUST match biome_page.glsl
/// PASS_CROP_IMG = 27 (a GENERIC code in the 27..31 reserved block, collision-proof against the
/// biome-private codes which start at 32). The readback TEST harness uses PASS_CROP; the runtime
/// producer (`compute_biome_page_cached`) uses this.
pub(super) const PASS_CROP_IMG: i32 = 27;
/// flow_on==false branch (coarse clipmap levels): zero primary_mask + tributary_mask so the two
/// carve terms in PASS_ASSEMBLE vanish -> the MACRO surface (CPU `else: primary_mask =
/// tributary_mask = zeros_like(base)`). A GENERIC pass in the 28..31 reserved block. MUST match
/// biome_page.glsl PASS_ZERO_FLOW_MASKS = 28.
pub(super) const PASS_ZERO_FLOW_MASKS: i32 = 28;
pub(super) const PASS_FLOW_PRE_PREBLUR_IN: i32 = 21;
pub(super) const PASS_FLOW_PRE_FROM_GAUSS: i32 = 22;
pub(super) const PASS_MASSIF_WRITEBACK: i32 = 23;
pub(super) const PASS_ACC_INIT: i32 = 24;
pub(super) const PASS_COPY_POOL: i32 = 25;       // gauss_in <- pool[pool_sel] (to blur a pool slot)
/// Generic capability for biomes that need to stash a blur back into a slot. Grassland/desert/coast
/// read the blur straight from gauss_out (no stash); WETLAND uses it (stash gaussian(channels,2.2)
/// for the levee DoG, and flat_base back into its slot). Matches biome_page.glsl PASS_POOL_FROM_GAUSS.
pub(super) const PASS_POOL_FROM_GAUSS: i32 = 26; // pool[pool_sel] <- gauss_out (stash a blur)

// GENERIC COMPOSE pass codes (Slice-4b.11) -- MUST match biome_page.glsl PASS_COMPOSE_*. A high
// generic block (60..) handled inline in the machine BEFORE biome_pass(), so they are
// collision-proof against every biome-private code (currently <=53). The compose math is a
// bit-close port of biome_compose.rs (blend_field / blend_height_favored / compose_biomes).
pub(super) const PASS_COMPOSE_RELIEF_A_STORE: i32 = 60; // range_envelope <- abs(height - gauss_out)
pub(super) const PASS_COMPOSE_RELIEF_B_STORE: i32 = 61; // massif <- abs(pool0 - gauss_out)
pub(super) const PASS_COMPOSE_WACC: i32 = 62;           // lowland <- base/(base+pool1+1e-12)
pub(super) const PASS_COMPOSE_BLEND_FIELD: i32 = 63;    // height <- lowland*height + (1-lowland)*pool0
pub(super) const PASS_COMPOSE_BLEND_FAVORED: i32 = 64;  // height <- favored-blend(height, pool0, lowland)
pub(super) const PASS_COMPOSE_ACCW_ADD: i32 = 65;       // base += pool1  (acc_w += w)
pub(super) const PASS_COMPOSE_COPY_ACC: i32 = 66;       // gauss_in <- height (to blur the accumulator)

/// The relief-proxy gaussian sigma for the compose layer == BlendConfig::relief_sigma_px default.
/// Mirrors biome_compose.rs GAUSSIAN_TRUNCATE-driven gaussian_filter_nearest(..., sigma=6.0).
pub(super) const COMPOSE_RELIEF_SIGMA: f64 = 6.0;

// GRASSLAND biome-private PASS_* codes (start at 32) -- MUST match biome_grassland.glsl GL_*.
pub(super) const GL_POINTWISE: i32 = 32;
pub(super) const GL_COMBO: i32 = 33;
pub(super) const GL_SWELLS: i32 = 34;
pub(super) const GL_ONE_MINUS_SWELLS: i32 = 35;
pub(super) const GL_PANS: i32 = 36;
pub(super) const GL_SANDHILL_PRE: i32 = 37;
pub(super) const GL_SANDHILL_FINAL: i32 = 38;
pub(super) const GL_ESC_PRE: i32 = 39;
pub(super) const GL_ESC_FINAL: i32 = 40;
pub(super) const GL_BASE_FOR_FLOW: i32 = 41;
pub(super) const GL_DRAWS: i32 = 42;
pub(super) const GL_TEXTURE: i32 = 43;
pub(super) const GL_ASSEMBLE: i32 = 44;
pub(super) const GL_OPEN_FLOOR_BLEND: i32 = 45;
pub(super) const GL_FINAL: i32 = 46;

// DESERT biome-private PASS_* codes (start at 32) -- MUST match biome_desert.glsl DS_*.
pub(super) const DS_POINTWISE: i32 = 32;
pub(super) const DS_BASIN: i32 = 33;
pub(super) const DS_PLAYA: i32 = 34;
pub(super) const DS_DUNE_PRE: i32 = 35;
pub(super) const DS_DUNE_FINAL: i32 = 36;
pub(super) const DS_YARDANG: i32 = 37;
pub(super) const DS_BLOCK_PRE: i32 = 38;
pub(super) const DS_BLOCK_CORES: i32 = 39;
pub(super) const DS_MESAS: i32 = 40;
pub(super) const DS_BASE: i32 = 41;
pub(super) const DS_WASH_FLOW_PRE: i32 = 42;
pub(super) const DS_WASH_FINAL: i32 = 43;
pub(super) const DS_FINE_SALT: i32 = 44;
pub(super) const DS_ASSEMBLE: i32 = 45;
pub(super) const DS_FLOOR_BLEND: i32 = 46;
pub(super) const DS_FINAL: i32 = 47;

// COAST biome-private PASS_* codes (start at 32) -- MUST match biome_coast.glsl CO_*.
pub(super) const CO_POINTWISE: i32 = 32;
pub(super) const CO_FLOW_PRE: i32 = 33;
pub(super) const CO_CHANNELS: i32 = 34;
pub(super) const CO_CHANNEL_RELIEF: i32 = 35;
pub(super) const CO_ISLANDS_SEED: i32 = 36;
pub(super) const CO_ISLANDS: i32 = 37;
pub(super) const CO_ASSEMBLE: i32 = 38;
pub(super) const CO_SEA_BLEND: i32 = 39;
pub(super) const CO_FINAL: i32 = 40;

// WETLAND biome-private PASS_* codes (start at 32) -- MUST match biome_wetland.glsl WL_*.
pub(super) const WL_POINTWISE: i32 = 32;
pub(super) const WL_ONE_MINUS_MACRO: i32 = 33;
pub(super) const WL_BASIN: i32 = 34;
pub(super) const WL_FLOODPLAIN_PRE: i32 = 35;
pub(super) const WL_FLOODPLAIN: i32 = 36;
pub(super) const WL_CHANNELS_FIRST: i32 = 37;
pub(super) const WL_FLOW_PRE: i32 = 38;
pub(super) const WL_CHANNELS_FLOW: i32 = 39;
pub(super) const WL_LEVEES: i32 = 40;
pub(super) const WL_FLAT_BASE_PRE: i32 = 41;
pub(super) const WL_ASSEMBLE: i32 = 42;
pub(super) const WL_FINAL: i32 = 43;

// TUNDRA biome-private PASS_* codes (start at 32) -- MUST match biome_tundra.glsl TU_*.
pub(super) const TU_POINTWISE: i32 = 32;
pub(super) const TU_PLAIN_PRE: i32 = 33;
pub(super) const TU_PLAIN: i32 = 34;
pub(super) const TU_PATTERN_PRE: i32 = 35;
pub(super) const TU_PATTERN: i32 = 36;
pub(super) const TU_FRINGE: i32 = 37;
pub(super) const TU_FLOW_PRE: i32 = 38;
pub(super) const TU_DRAINAGE: i32 = 39;
pub(super) const TU_BASE_PRE: i32 = 40;
pub(super) const TU_ASSEMBLE: i32 = 41;
pub(super) const TU_FINAL: i32 = 42;

// GLACIAL biome-private PASS_* codes (start at 32) -- MUST match biome_glacial.glsl GC_*.
pub(super) const GC_POINTWISE: i32 = 32;
pub(super) const GC_RELIEF_RAW: i32 = 33;
pub(super) const GC_RELIEF: i32 = 34;
pub(super) const GC_RELIEF_ENV: i32 = 35;
pub(super) const GC_ICE_INNER: i32 = 36;
pub(super) const GC_ICEFIELD: i32 = 37;
pub(super) const GC_MASSIF_INNER: i32 = 38;
pub(super) const GC_MASSIF: i32 = 39;
pub(super) const GC_BASE: i32 = 40;
pub(super) const GC_FLOW_PRE_PRIMARY: i32 = 41;
pub(super) const GC_FLOW_PRIMARY_STASH: i32 = 42;
pub(super) const GC_AXIAL_RAW: i32 = 43;
pub(super) const GC_AXIAL: i32 = 44;
pub(super) const GC_PRIMARY_MASK: i32 = 45;
pub(super) const GC_BRANCH_SURFACE: i32 = 46;
pub(super) const GC_TRIB_MASK: i32 = 47;
pub(super) const GC_SCRAPES: i32 = 48;
pub(super) const GC_ASSEMBLE: i32 = 49;
pub(super) const GC_FLOOR_MASK: i32 = 50;
pub(super) const GC_FLOOR_BLEND: i32 = 51;
pub(super) const GC_ICE_BLEND: i32 = 52;
pub(super) const GC_FINAL: i32 = 53;

// KARST biome-private PASS_* codes (start at 32) -- MUST match biome_karst.glsl KS_*.
pub(super) const KS_POINTWISE: i32 = 32;
pub(super) const KS_PLATEAU: i32 = 33;
pub(super) const KS_TOWER_PRE: i32 = 34;
pub(super) const KS_TOWER_FINAL: i32 = 35;
pub(super) const KS_DOLINE_PRE: i32 = 36;
pub(super) const KS_DOLINE_FINAL: i32 = 37;
pub(super) const KS_LINEAMENTS: i32 = 38;
pub(super) const KS_CELLULAR_RAW: i32 = 39;
pub(super) const KS_CELLULAR: i32 = 40;
pub(super) const KS_COCKPIT_NOISE: i32 = 41;
pub(super) const KS_COCKPIT: i32 = 42;
pub(super) const KS_BASE: i32 = 43;
pub(super) const KS_FINE_KARREN: i32 = 44;
pub(super) const KS_DV_SURFACE: i32 = 45;
pub(super) const KS_DV_FINAL: i32 = 46;
pub(super) const KS_MASKS: i32 = 47;
pub(super) const KS_ASSEMBLE: i32 = 48;
pub(super) const KS_FLOOR_MASK: i32 = 49;
pub(super) const KS_FLOOR_BLEND: i32 = 50;
pub(super) const KS_FINAL: i32 = 51;

// TEMPERATE biome-private PASS_* codes (start at 32) -- MUST match biome_temperate.glsl TE_*.
pub(super) const TE_POINTWISE: i32 = 32;
pub(super) const TE_RIDGES: i32 = 33;
pub(super) const TE_HILLS: i32 = 34;
pub(super) const TE_UPLAND: i32 = 35;
pub(super) const TE_FLOW_PRE: i32 = 36;
pub(super) const TE_VALLEYS: i32 = 37;
pub(super) const TE_BROAD_VALLEYS: i32 = 38;
pub(super) const TE_ROUNDED_PRE: i32 = 39;
pub(super) const TE_ASSEMBLE: i32 = 40;
pub(super) const TE_FINAL: i32 = 41;

// RAINFOREST biome-private PASS_* codes (start at 32) -- MUST match biome_rainforest.glsl RF_*.
pub(super) const RF_POINTWISE: i32 = 32;
pub(super) const RF_HILLS: i32 = 33;
pub(super) const RF_PLATEAU: i32 = 34;
pub(super) const RF_ONE_MINUS_MACRO: i32 = 35;
pub(super) const RF_LOWLAND: i32 = 36;
pub(super) const RF_FLOW_PRE: i32 = 37;
pub(super) const RF_TRIBUTARIES: i32 = 38;
pub(super) const RF_TRUNK: i32 = 39;
pub(super) const RF_DRAINAGE: i32 = 40;
pub(super) const RF_CLOSE: i32 = 41;
pub(super) const RF_WET_PRE: i32 = 42;
pub(super) const RF_ASSEMBLE: i32 = 43;
pub(super) const RF_FINAL: i32 = 44;

// VOLCANIC biome-private PASS_* codes (start at 32) -- MUST match biome_volcanic.glsl VO_*.
pub(super) const VO_POINTWISE: i32 = 32;
pub(super) const VO_VENT_ACCUM: i32 = 33;
pub(super) const VO_FLOWS_FINAL: i32 = 34;
pub(super) const VO_REMAP: i32 = 35;
pub(super) const VO_LAVA_ROUGH: i32 = 36;
pub(super) const VO_BASE: i32 = 37;
pub(super) const VO_RADIAL: i32 = 38;
pub(super) const VO_GULLIES: i32 = 39;
pub(super) const VO_SPC_PRE: i32 = 40;
pub(super) const VO_CALDERA: i32 = 41;
pub(super) const VO_ASSEMBLE: i32 = 42;
pub(super) const VO_ASH_PRE: i32 = 43;
pub(super) const VO_ASH_BLEND: i32 = 44;
pub(super) const VO_FINAL: i32 = 45;

// copy_sel codes -- MUST match biome_page.glsl CP_* consts.
pub(super) const CP_RANGES: i32 = 0;
pub(super) const CP_MASSIF: i32 = 1;
pub(super) const CP_VALLEY: i32 = 2;
pub(super) const CP_HEIGHT: i32 = 3;

/// GENERIC scratch-pool slot count -- MUST match biome_page.glsl POOL_SLOTS. One additional
/// storage buffer is allocated + bound per slot (bindings 24..24+POOL_SLOTS-1), reusable by ANY
/// biome that needs more sub-fields than the fixed named buffers. Grassland uses all 12; to add a
/// biome needing more, bump this AND the GLSL POOL_SLOTS together. Mountain ignores the pool
/// entirely (its named buffers are untouched), so this is purely additive. Desert needs 16
/// (grassland uses 12); the 4 extra slots (12..15) are simply unused by mountain/grassland.
pub(super) const POOL_SLOTS: usize = 16;

/// scipy gaussian truncate (array_ops::TRUNCATE).
pub(super) const TRUNCATE: f64 = 4.0;

/// Flow PULL-relaxation step count. The flow-accum spike converged at 128 (memory
/// worldgen10-m3-rough-streaming-spike / flow_spike). This is the APPROXIMATION knob:
/// raise it if the parity gate's channel-region delta exceeds the Tier-2 epsilon.
pub(super) const STABLE_ITERS: usize = 128;
