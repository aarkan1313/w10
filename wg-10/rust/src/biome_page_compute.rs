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

    /// Load the biome FRAGMENT used to satisfy the machine's `biome_pass()` declaration during
    /// COMPOSE (any biome works -- mountain by convention; the compose passes are inline in main()
    /// and never reach the fragment). Returns "" on success, an error string otherwise. Call once
    /// before `compose_fields` / `blend_pair`.
    #[func]
    pub fn load_compose_fragment(&mut self, fragment_path: GString) -> GString {
        match std::fs::read_to_string(fragment_path.to_string()) {
            Ok(s) => {
                self.compose_fragment = Some(s);
                GString::new()
            }
            Err(e) => GString::from(format!("compose fragment glsl: {e}").as_str()),
        }
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

    /// COMPOSE entry (Slice-4b.11): GPU port of `biome_compose::compose_biomes`. Composes
    /// `n_fields` per-recipe height fields (concatenated row-major in `fields_flat`, each
    /// `rows*cols` long) by their per-pixel weights (`weights_flat`, same layout) into one field.
    /// `mode_is_field` chooses the blend mode (true="field", false="height_favored"); the favored
    /// path is applied EXACTLY for n_fields==2 (the fold uses FIELD blend for 3+ -- mirrors the
    /// oracle). Returns the composed field (length rows*cols) or an EMPTY array on error.
    /// Readback ONLY here (test/gate entry). WINDOWED only (local RD null headless).
    #[allow(clippy::too_many_arguments)]
    #[func]
    pub fn compose_fields(
        &self,
        fields_flat: PackedFloat64Array,
        weights_flat: PackedFloat64Array,
        n_fields: i64,
        rows: i64,
        cols: i64,
        mode_is_field: bool,
        favor_strength: f64,
        relief_confidence_floor: f64,
    ) -> PackedFloat64Array {
        let rows = rows as usize;
        let cols = cols as usize;
        let n = rows * cols;
        let nf = n_fields as usize;
        if nf == 0 {
            godot_error!("compose_fields: n_fields must be >= 1");
            return PackedFloat64Array::new();
        }
        if fields_flat.len() != nf * n || weights_flat.len() != nf * n {
            godot_error!(
                "compose_fields: fields/weights flat len mismatch (got {}/{}, expected {})",
                fields_flat.len(), weights_flat.len(), nf * n
            );
            return PackedFloat64Array::new();
        }
        // un-flatten into per-recipe f32 fields (the GPU is f32 throughout).
        let ff = fields_flat.as_slice();
        let wf = weights_flat.as_slice();
        let mut fields: Vec<Vec<f32>> = Vec::with_capacity(nf);
        let mut weights: Vec<Vec<f32>> = Vec::with_capacity(nf);
        for k in 0..nf {
            fields.push(ff[k * n..(k + 1) * n].iter().map(|&x| x as f32).collect());
            weights.push(wf[k * n..(k + 1) * n].iter().map(|&x| x as f32).collect());
        }
        match self.run_compose_inner(
            &fields, &weights, rows, cols, mode_is_field,
            favor_strength as f32, relief_confidence_floor as f32,
        ) {
            Ok(out) => f32s_to_packed_f64(&out),
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::compose_fields error: {e}");
                PackedFloat64Array::new()
            }
        }
    }

    /// BLEND-PAIR entry (Slice-4b.11): GPU port of `biome_compose::blend_field` (mode_is_field=true)
    /// and `biome_compose::blend_height_favored` (mode_is_field=false). A SINGLE blend of `a`/`b`
    /// at per-pixel weight `w_a` (NOT the running-accumulator fold -- w_a is used DIRECTLY, exactly
    /// as the two standalone oracle functions do). Returns the blended field (length rows*cols) or
    /// an EMPTY array on error. Readback ONLY here. WINDOWED only.
    #[allow(clippy::too_many_arguments)]
    #[func]
    pub fn blend_pair(
        &self,
        a: PackedFloat64Array,
        b: PackedFloat64Array,
        w_a: PackedFloat64Array,
        rows: i64,
        cols: i64,
        mode_is_field: bool,
        favor_strength: f64,
        relief_confidence_floor: f64,
    ) -> PackedFloat64Array {
        let rows = rows as usize;
        let cols = cols as usize;
        let n = rows * cols;
        if a.len() != n || b.len() != n || w_a.len() != n {
            godot_error!(
                "blend_pair: a/b/w_a len mismatch (got {}/{}/{}, expected {})",
                a.len(), b.len(), w_a.len(), n
            );
            return PackedFloat64Array::new();
        }
        let a32: Vec<f32> = a.as_slice().iter().map(|&x| x as f32).collect();
        let b32: Vec<f32> = b.as_slice().iter().map(|&x| x as f32).collect();
        let w32: Vec<f32> = w_a.as_slice().iter().map(|&x| x as f32).collect();
        match self.run_blend_inner(
            &a32, &b32, &w32, rows, cols, mode_is_field,
            favor_strength as f32, relief_confidence_floor as f32,
        ) {
            Ok(out) => f32s_to_packed_f64(&out),
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::blend_pair error: {e}");
                PackedFloat64Array::new()
            }
        }
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

    /// SCALE-INVARIANCE identity: at spacing == S_REF (32.0), `sigma_cells` is the identity, so the
    /// anchored kernels MUST equal the cell-sigma kernels (the parity-proven content) BYTE-for-byte,
    /// and the kparams slot layout (key + koffset + kradius) MUST match `alloc_apron_buffers`'. This
    /// pins the property the windowed 576 gate depends on: at the production finest level (~32 m/px)
    /// the GPU kernels are unchanged from the cell-sigma path.
    #[test]
    fn anchored_kernels_identity_at_s_ref() {
        use crate::recipes::helpers::S_REF;
        let (packed, kp) = mountain_kernels_anchored(S_REF).expect("kernels fit at S_REF");
        // Reference packed buffer (the cell-sigma path: gaussian_kernel1d(ref, TRUNCATE)).
        let refs = mountain_sigmas();
        for (slot, &ref_sigma) in refs.iter().enumerate() {
            let want = gaussian_kernel1d(ref_sigma, TRUNCATE);
            let base = slot * KERNEL_STRIDE;
            for (j, &w) in want.iter().enumerate() {
                assert_eq!(packed[base + j], w, "slot {slot} (sigma {ref_sigma}) tap {j} differs at S_REF");
            }
            // slot layout: keyed by the REFERENCE sigma, anchored koffset/kradius == cell-sigma ones.
            let (ko, kr) = kp.kp(ref_sigma);
            assert_eq!(ko, (slot * KERNEL_STRIDE) as i32, "koffset drift at S_REF");
            assert_eq!(kr, gaussian_radius(ref_sigma, TRUNCATE) as i32, "kradius drift at S_REF");
        }
    }

    /// Anchoring DIRECTION + lookup-key stability. At a COARSER spacing (> S_REF) every anchored
    /// sigma SHRINKS (covers the same world distance with fewer cells) -> radius <= the cell radius;
    /// the slot KEY stays the reference sigma (so `schedule_mountain`'s `gauss(5.0)` still resolves).
    /// At the production finest level (~32) all kernels fit the stride; this also confirms the
    /// over-stride guard only trips at an unrealistically fine spacing.
    #[test]
    fn anchored_kernels_shrink_and_key_by_reference_when_coarser() {
        use crate::recipes::helpers::{sigma_cells, S_REF};
        let coarse = S_REF * 4.0; // a coarse clipmap level (4x the reference spacing)
        let (_packed, kp) = mountain_kernels_anchored(coarse).expect("coarse kernels fit");
        for &ref_sigma in &mountain_sigmas() {
            let anchored = sigma_cells(ref_sigma, coarse);
            assert!(anchored < ref_sigma + 1e-12, "coarser spacing must shrink sigma");
            // lookup by the REFERENCE sigma (the schedule's key) resolves to the ANCHORED radius.
            let (_ko, kr) = kp.kp(ref_sigma);
            assert_eq!(
                kr,
                gaussian_radius(anchored, TRUNCATE) as i32,
                "kradius must reflect the anchored sigma, keyed by the reference sigma"
            );
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
        let p = build_push(0, 344, 344, 160, 0, 4, 0, 0, 0, 0, 0, 3913.04, 12000.0, -31000.0, 90000.0, 0.48, 0.0, 0.0, 0.0);
        assert_eq!(p.len(), 96);
    }

    #[test]
    fn push_constant_packs_ints_then_floats() {
        // build_push(pass,rows,cols,apron,seed,kradius,copy_sel,flow_dir,koffset,pool_sel,vent_count,spacing,ox,oz,span,power,favor,floor)
        let p = build_push(7, 344, 343, 160, 5, 28, 2, 1, 128, 9, 4, 3913.0, 12000.0, -31000.0, 90000.0, 0.34, 0.0, 0.0, 0.0);
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
        assert_eq!(i32::from_le_bytes([p[40], p[41], p[42], p[43]]), 4);   // vent_count (former ipad1)
        // 1 int pad at 44..48; floats start at byte 48.
        let spacing = f32::from_le_bytes([p[48], p[49], p[50], p[51]]);
        assert!((spacing - 3913.0).abs() < 1e-1);
        // floats: spacing(48),ox(52),oz(56),span(60),power(64)
        let flow_power = f32::from_le_bytes([p[64], p[65], p[66], p[67]]);
        assert!((flow_power - 0.34).abs() < 1e-6);
    }

    #[test]
    fn non_volcanic_push_vent_count_is_zero_byte_identical() {
        // The 10 proven biomes pass vent_count=0 -> byte-identical to the former hardcoded `0` pad.
        // Build a representative mountain dispatch push with vent_count=0 and confirm the vent_count
        // int slot (bytes 40..44) is exactly zero (so mountain's 1.89e-6 parity is preserved).
        let p = build_push(8, 344, 344, 160, 0, 0, 0, 0, 0, 0, 0, 2608.7, 12000.0, -31000.0, 60000.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(i32::from_le_bytes([p[40], p[41], p[42], p[43]]), 0);
        assert_eq!(p.len(), 96);
        // The two compose param floats (pad0=favor_strength, pad1=relief_conf_floor) are at bytes
        // 68..72 and 72..76. For a non-compose dispatch they are 0.0 -> byte-identical to the former
        // all-zero pad block, so the 11 proven biomes' push bytes are unchanged.
        assert_eq!(f32::from_le_bytes([p[68], p[69], p[70], p[71]]), 0.0);
        assert_eq!(f32::from_le_bytes([p[72], p[73], p[74], p[75]]), 0.0);
    }

    #[test]
    fn push_constant_carries_compose_params_in_pads() {
        // favor_strength -> pad0 (bytes 68..72), relief_confidence_floor -> pad1 (bytes 72..76).
        let p = build_push(64, 32, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 1e-3, 0.0);
        let favor = f32::from_le_bytes([p[68], p[69], p[70], p[71]]);
        let floor = f32::from_le_bytes([p[72], p[73], p[74], p[75]]);
        assert!((favor - 2.0).abs() < 1e-7, "favor_strength not in pad0");
        assert!((floor - 1e-3).abs() < 1e-9, "relief_confidence_floor not in pad1");
        // the remaining 4 float pads (bytes 76..96) stay zero.
        for off in (76..96).step_by(4) {
            assert_eq!(f32::from_le_bytes([p[off], p[off + 1], p[off + 2], p[off + 3]]), 0.0);
        }
        assert_eq!(p.len(), 96);
    }

    #[test]
    fn compose_sigmas_has_relief_sigma() {
        // The compose relief proxy uses exactly sigma = relief_sigma_px default = 6.0.
        let s = compose_sigmas();
        assert_eq!(s.len(), 1);
        assert!((s[0] - 6.0).abs() < 1e-12);
        // its kernel must fit the packed-kernel stride.
        let len = 2 * gaussian_radius(s[0], TRUNCATE) + 1;
        assert!(len <= KERNEL_STRIDE, "compose kernel len {len} > {KERNEL_STRIDE}");
        // sigma 6.0 -> lw = int(4.0*6.0+0.5) = int(24.5) = 24 -> length 49.
        assert_eq!(gaussian_radius(6.0, TRUNCATE), 24);
        assert_eq!(len, 49);
    }

    #[test]
    fn compose_kernel_matches_array_ops_relief_sigma() {
        // The GPU relief proxy gaussian MUST use the SAME sigma=6.0 kernel as
        // biome_compose.rs::GAUSSIAN_TRUNCATE-driven gaussian_filter_nearest. Verify the kernel
        // sums to ~1 (normalized) and is symmetric (the array_ops contract).
        let k = gaussian_kernel1d(6.0, TRUNCATE);
        let sum: f32 = k.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "compose relief kernel not normalized (sum={sum})");
        let n = k.len();
        for i in 0..n {
            assert!((k[i] - k[n - 1 - i]).abs() < 1e-7, "compose relief kernel not symmetric at {i}");
        }
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
}
