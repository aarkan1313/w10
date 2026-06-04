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
    RenderingServer, RenderingDevice,
    RdTextureFormat, RdTextureView,
    rendering_device::{DataFormat, TextureUsageBits},
};

mod abi;
mod compose_api;
mod helpers;
mod kernels;
mod schedule_coast;
mod schedule_desert;
mod schedule_glacial;
mod schedule_grassland;
mod schedule_karst;
mod schedule_mountain;
mod schedule_rainforest;
mod schedule_temperate;
mod schedule_tundra;
mod schedule_volcanic;
mod schedule_wetland;
mod runtime_buffers;
mod runtime_context;
mod local_compose;
mod local_readback;
mod scheduler;
mod sigma_registry;

use abi::*;
pub(crate) use helpers::*;
pub(crate) use kernels::*;
pub(crate) use runtime_context::{
    build_biome_page_context, compute_biome_page_cached, free_biome_page_context,
    BiomePageComputeContext,
};
pub(crate) use sigma_registry::*;

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10BiomePageCompute {
    primitives_src: Option<String>,
    /// The GENERIC machine (biome_page.glsl): bindings + leaf helpers + generic passes + main().
    /// One of the two STABLE parts (the other being primitives); loaded once via load_shaders.
    machine_src: Option<String>,
    /// A biome FRAGMENT (any -- mountain by convention) concatenated ONLY to satisfy the machine's
    /// `biome_pass()` declaration during compose. The compose passes are inline in main() and never
    /// reach the fragment, so the choice is irrelevant. Loaded once via `load_compose_fragment`.
    compose_fragment: Option<String>,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10BiomePageCompute {
    fn init(base: Base<RefCounted>) -> Self {
        Self { primitives_src: None, machine_src: None, compose_fragment: None, base }
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
            &fragment, &biome, STABLE_ITERS,
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

    /// MEASUREMENT entry: `generate_core_page` with the flow PULL-relaxation step count made a
    /// caller parameter (`flow_iters`), so a windowed harness can sweep it at the REAL 576
    /// production apron to find the production convergence count (decides whether live-per-page
    /// flow fits the budget, i.e. whether the coarse-drainage-fact subsystem is needed). NOT a
    /// runtime entry; same readback-only caveat as `generate_core_page`. `generate_core_page`
    /// itself passes STABLE_ITERS, so the parity-proven path is unchanged.
    #[allow(clippy::too_many_arguments)]
    #[func]
    pub fn generate_core_page_iters(
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
        flow_iters: i64,
    ) -> PackedFloat64Array {
        let rows = padded_rows as usize;
        let cols = padded_cols as usize;
        let apron = apron_px as usize;
        if seed < i32::MIN as i64 || seed > i32::MAX as i64 {
            godot_error!("Wg10BiomePageCompute::generate_core_page_iters: seed {seed} outside i32 range");
            return PackedFloat64Array::new();
        }
        if flow_iters < 1 {
            godot_error!("Wg10BiomePageCompute::generate_core_page_iters: flow_iters must be >= 1");
            return PackedFloat64Array::new();
        }
        let frag_path = biome_fragment_path.to_string();
        let fragment = match std::fs::read_to_string(&frag_path) {
            Ok(s) => s,
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_core_page_iters: biome fragment glsl: {e}");
                return PackedFloat64Array::new();
            }
        };
        let biome = biome_stem(&frag_path);
        match self.run_inner(
            spacing as f32, ox as f32, oz as f32, rows, cols, apron, seed as i32, feature_span_m as f32,
            &fragment, &biome, flow_iters as usize,
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
                godot_error!("Wg10BiomePageCompute::generate_core_page_iters error: {e}");
                PackedFloat64Array::new()
            }
        }
    }

    /// RUNTIME-producer readback entry (Slice-4b, Task 3): exercises the REAL runtime mountain
    /// page producer (`build_biome_page_context` + `compute_biome_page_cached` + the crop-to-image
    /// PASS_CROP_IMG path) end-to-end, but on a LOCAL RenderingDevice + a scratch R32F TEXTURE so
    /// it is test-runnable from a WINDOWED gate. Builds a context, dispatches one page into the
    /// scratch texture, reads the texture back (`texture_get_data`), frees the context + texture +
    /// rd, and returns the CORE f64 height (length core_px*core_px). The LATER windowed 576 parity
    /// gate (Task 4) compares THIS against `generate_core_page` to PROVE the runtime producer
    /// matches the proven readback core bit-for-bit.
    ///
    /// Convention matches `generate_core_page`: `ox`/`oz` are the PADDED-grid origin and `spacing`
    /// the metres/px. The runtime producer takes (origin, world_span, page_px) instead, so this
    /// converts: `page_px = padded_rows - 2*apron_px`, `world_span = spacing*(page_px-1)`,
    /// `origin = ox + apron_px*spacing` (the producer re-subtracts the apron). MOUNTAIN only.
    ///
    /// `flow_iters` = the flow PULL-relaxation step count threaded into `build_biome_page_context`
    /// (mirrors `generate_core_page_iters`). The 576 production page needs MORE than the recipe-path
    /// STABLE_ITERS=128 to converge to the exact f64 sweep oracle (~192 measured), so the windowed
    /// 576 parity gate sweeps this to separate UNDER-CONVERGENCE from a real divergence.
    /// Returns an EMPTY array on error (see godot_error log). WINDOWED only (local RD null headless).
    #[allow(clippy::too_many_arguments)]
    #[func]
    pub fn generate_runtime_page_576(
        &self,
        spacing: f64,
        ox: f64,
        oz: f64,
        padded_rows: i64,
        padded_cols: i64,
        apron_px: i64,
        seed: i64,
        feature_span_m: f64,
        mountain_fragment_path: GString,
        flow_iters: i64,
    ) -> PackedFloat64Array {
        if padded_rows != padded_cols {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: padded grid must be square (got {padded_rows}x{padded_cols})");
            return PackedFloat64Array::new();
        }
        let apron = apron_px as usize;
        let padded = padded_rows as usize;
        if padded <= 2 * apron {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: apron {apron} too large for padded {padded}");
            return PackedFloat64Array::new();
        }
        let core_px = padded - 2 * apron;
        if seed < i32::MIN as i64 || seed > i32::MAX as i64 {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: seed {seed} outside i32 range");
            return PackedFloat64Array::new();
        }
        if flow_iters < 1 {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: flow_iters must be >= 1");
            return PackedFloat64Array::new();
        }
        let prim = match self.primitives_src.as_deref() {
            Some(s) => s,
            None => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: no GLSL source loaded (call load_shaders)");
                return PackedFloat64Array::new();
            }
        };
        let machine = match self.machine_src.as_deref() {
            Some(s) => s,
            None => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: no GLSL source loaded (call load_shaders)");
                return PackedFloat64Array::new();
            }
        };
        let frag_path = mountain_fragment_path.to_string();
        let fragment = match std::fs::read_to_string(&frag_path) {
            Ok(s) => s,
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: mountain fragment glsl: {e}");
                return PackedFloat64Array::new();
            }
        };

        // LOCAL rd (test entry; the production caller passes the GLOBAL rd instead).
        let mut rd: Gd<RenderingDevice> = match RenderingServer::singleton().create_local_rendering_device() {
            Some(d) => d,
            None => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: create_local_rendering_device returned null (headless / no device)");
                return PackedFloat64Array::new();
            }
        };

        // Build the cached runtime context (compile + pipeline + all buffers, on this local rd).
        // relief_m = 1.0: the 576 PARITY readback must stay in NORMALIZED units to match the f64
        // oracle (the runtime render path uses the configured metre relief; parity does not).
        let ctx = match build_biome_page_context(
            &mut rd, prim, machine, &fragment, core_px, apron, flow_iters as usize, 1.0,
        ) {
            Ok(c) => c,
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: {e}");
                rd.free();
                return PackedFloat64Array::new();
            }
        };

        // Scratch R32F output texture (caller-owned model; here the test owns + frees it).
        let mut fmt = RdTextureFormat::new_gd();
        fmt.set_width(core_px as u32);
        fmt.set_height(core_px as u32);
        fmt.set_format(DataFormat::R32_SFLOAT);
        fmt.set_usage_bits(
            TextureUsageBits::STORAGE_BIT
                | TextureUsageBits::SAMPLING_BIT
                | TextureUsageBits::CAN_COPY_FROM_BIT,
        );
        let view = RdTextureView::new_gd();
        let tex = rd.texture_create(&fmt, &view);
        if tex.is_invalid() {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: texture_create returned invalid RID");
            free_biome_page_context(&mut rd, &ctx);
            rd.free();
            return PackedFloat64Array::new();
        }

        // Reconcile the padded-origin convention to the producer's (origin, world_span, page_px).
        let page_px = core_px as i64;
        let world_span = spacing * (page_px as f64 - 1.0);
        let origin_x = ox + apron as f64 * spacing;
        let origin_z = oz + apron as f64 * spacing;

        if let Err(e) = compute_biome_page_cached(
            // flow_on=true: the 576 PARITY readback must match the flow-ON f64 oracle. The
            // spacing-anchored kernels are now built INSIDE from world_span/(page_px-1), so the
            // regenerated oracle (at the SAME spacing) must match bit-for-bit (Tier-2).
            &mut rd, &ctx, tex, origin_x, origin_z, world_span, page_px, feature_span_m, seed, true,
        ) {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: {e}");
            rd.free_rid(tex);
            free_biome_page_context(&mut rd, &ctx);
            rd.free();
            return PackedFloat64Array::new();
        }

        // Read the page texture back (layer 0). R32F -> 4 bytes/texel, core_px*core_px texels.
        let raw = rd.texture_get_data(tex, 0);
        let core = bytes_to_f32s(&raw.to_vec());

        rd.free_rid(tex);
        free_biome_page_context(&mut rd, &ctx);
        rd.free();

        let core_n = core_px * core_px;
        if core.len() != core_n {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: texture readback expected {core_n} f32, got {}", core.len());
            return PackedFloat64Array::new();
        }
        let mut out = PackedFloat64Array::new();
        out.resize(core.len());
        let sl = out.as_mut_slice();
        for i in 0..core.len() {
            sl[i] = core[i] as f64;
        }
        out
    }

}

// ---------------------------------------------------------------------------
// Unit tests (pure helpers; no Godot runtime)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod biome_page_compute_tests;
