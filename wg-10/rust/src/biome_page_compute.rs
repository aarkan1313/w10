//! WorldGen10 Slice-4a: GPU apron PAGE pipeline for the MOUNTAIN seam-safe recipe.
//!
//! `Wg10BiomePageCompute` mirrors `recipes.rs::mountain::generate_seamsafe` (the f64 parity
//! ORACLE) as a MULTI-DISPATCH GPU pipeline: it concatenates `recipe_primitives.glsl` (the
//! proven f32 noise/warp leaves) + `biome_page_4a.glsl` (the recipe pass chain), compiles
//! one compute shader, and dispatches it once per pass with a different `pass` push-constant.
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
// pass selector codes -- MUST match biome_page_4a.glsl PASS_* consts.
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

// copy_sel codes -- MUST match biome_page_4a.glsl CP_* consts.
const CP_RANGES: i32 = 0;
const CP_MASSIF: i32 = 1;
const CP_VALLEY: i32 = 2;
const CP_HEIGHT: i32 = 3;

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

fn make_storage_uniform(binding: i32, rid: Rid) -> Gd<RdUniform> {
    let mut u = RdUniform::new_gd();
    u.set_uniform_type(UniformType::STORAGE_BUFFER);
    u.set_binding(binding);
    u.add_id(rid);
    u
}

/// Build the 96-byte push constant (std430): 12 i32 (48B) then 12 f32 (48B).
/// Layout MUST match biome_page_4a.glsl Params.
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
    spacing: f32,
    ox: f32,
    oz: f32,
    feature_span_m: f32,
    flow_power: f32,
) -> Vec<u8> {
    let mut b = Vec::with_capacity(96);
    // 12 ints: pass,rows,cols,apron,seed,kradius,copy_sel,flow_dir,koffset + 3 pad.
    for v in [pass, rows, cols, apron_px, seed, kradius, copy_sel, flow_dir, koffset, 0, 0, 0] {
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

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10BiomePageCompute {
    primitives_src: Option<String>,
    page_src: Option<String>,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10BiomePageCompute {
    fn init(base: Base<RefCounted>) -> Self {
        Self { primitives_src: None, page_src: None, base }
    }
}

#[godot_api]
impl Wg10BiomePageCompute {
    /// Load BOTH GLSL files (primitives helpers + the page pass chain) from OS paths and keep
    /// them; they are concatenated (primitives first) before compile (Godot GLSL has no
    /// #include). Returns "" on success, an error string otherwise. Mirrors
    /// `Wg10PrimitiveProbe::load_shader`.
    #[func]
    pub fn load_shaders(&mut self, primitives_path: GString, page_path: GString) -> GString {
        let prim = match std::fs::read_to_string(primitives_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(format!("primitives glsl: {e}").as_str()),
        };
        let page = match std::fs::read_to_string(page_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(format!("page glsl: {e}").as_str()),
        };
        self.primitives_src = Some(prim);
        self.page_src = Some(page);
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
        match self.run_inner(
            spacing as f32, ox as f32, oz as f32, rows, cols, apron, seed as i32, feature_span_m as f32,
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
    ) -> Result<Vec<f32>, String> {
        if rows <= 2 * apron || cols <= 2 * apron {
            return Err(format!("apron {apron} too large for padded {rows}x{cols}"));
        }
        let prim = self.primitives_src.as_deref().ok_or("no GLSL source loaded")?;
        let page = self.page_src.as_deref().ok_or("no GLSL source loaded")?;
        let n = rows * cols;
        let core_rows = rows - 2 * apron;
        let core_cols = cols - 2 * apron;
        let core_n = core_rows * core_cols;

        let mut rd: Gd<RenderingDevice> = RenderingServer::singleton()
            .create_local_rendering_device()
            .ok_or_else(|| {
                "create_local_rendering_device returned null (headless / no device)".to_string()
            })?;

        // --- compile: concat primitives + page, strip non-GLSL #[...] header lines ---
        let joined = format!("{prim}\n{page}");
        let glsl_stripped: String = joined
            .lines()
            .filter(|l| !l.trim_start().starts_with("#["))
            .collect::<Vec<_>>()
            .join("\n");
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
        // the `koffset` push constant. Build the kernels in the fixed sigma order.
        let sigmas = mountain_sigmas();
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
        // sigma -> (koffset, kradius) lookup.
        let kparams = |sigma: f64| -> (i32, i32) {
            let slot = sigmas
                .iter()
                .position(|&s| (s - sigma).abs() < 1e-9)
                .expect("sigma not in mountain_sigmas()");
            ((slot * KERNEL_STRIDE) as i32, gaussian_radius(sigma, TRUNCATE) as i32)
        };

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

        // one uniform set binding all 24 buffers (build once).
        let bindings: [(i32, Rid); 24] = [
            (0, b_wx), (1, b_wz), (2, b_regional), (3, b_ranges), (4, b_ridge_detail),
            (5, b_near_detail), (6, b_range_env), (7, b_lowland), (8, b_massif), (9, b_base),
            (10, b_primary), (11, b_trib), (12, b_high), (13, b_valley), (14, b_height),
            (15, b_floor), (16, b_gauss_in), (17, b_gauss_mid), (18, b_gauss_out),
            (19, b_kernel), (20, b_flow_pre), (21, b_acc_a), (22, b_acc_b), (23, b_core),
        ];
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

        let valley_width_px = 2.4_f64;
        let trib_width = (valley_width_px * 0.42).max(0.6);
        let floor_smooth = 4.0_f64.max(0.2);

        // PRE-VALIDATE every sigma the pipeline will request, BEFORE the compute list is open:
        // kparams uses `.expect()`, and a panic AFTER compute_list_begin would unwind with an
        // active list and leak the local RD. Resolving them here proves the in-list lookups
        // cannot fail. (The `mountain_sigmas_cover_all_pipeline_blurs` unit test also guards this.)
        for s in [1.15_f64, 1.20, 1.80, 2.00, 5.00, 7.00, valley_width_px, trib_width, floor_smooth] {
            let _ = kparams(s);
        }

        // ===== record the WHOLE pipeline into ONE compute list, with a barrier after every
        // dependent dispatch (the proven flow_spike pattern). Then submit + sync once. =====
        let cl = rd.compute_list_begin();
        rd.compute_list_bind_compute_pipeline(cl, pipeline);

        // helper closures capture rows/cols/apron/seed/grid/cl/pipeline/uset.
        // dispatch a full-field pass + trailing barrier (so the next reader sees the writes).
        let mut dispatch = |rd: &mut Gd<RenderingDevice>, pass: i32, kradius: i32, koffset: i32, copy_sel: i32, flow_dir: i32, flow_power: f32, wgx: u32, wgy: u32| {
            rd.compute_list_bind_uniform_set(cl, uset, 0);
            let pc = PackedByteArray::from(
                build_push(pass, rows as i32, cols as i32, apron as i32, seed, kradius, copy_sel, flow_dir, koffset, spacing, ox, oz, feature_span_m, flow_power).as_slice(),
            );
            rd.compute_list_set_push_constant(cl, &pc, pc.len() as u32);
            rd.compute_list_dispatch(cl, wgx, wgy, 1);
            rd.compute_list_add_barrier(cl);
        };

        // gaussian(sigma) on gauss_in -> gauss_out (AXIS0 then AXIS1, packed kernel by koffset).
        macro_rules! gauss {
            ($rd:expr, $sigma:expr) => {{
                let (ko, kr) = kparams($sigma);
                dispatch($rd, PASS_GAUSS_AXIS0, kr, ko, 0, 0, 0.0, wg_full_x, wg_full_y);
                dispatch($rd, PASS_GAUSS_AXIS1, kr, ko, 0, 0, 0.0, wg_full_x, wg_full_y);
            }};
        }
        // flow_channels_seam_safe(flow_pre, width_px, power): pre-blur 1.15 -> K relax ->
        // log1p discharge -> spread gaussian(width). Leaves spread discharge in gauss_out.
        macro_rules! flow_channels {
            ($rd:expr, $power:expr, $width:expr) => {{
                // pre-blur sigma=1.15
                dispatch($rd, PASS_FLOW_PRE_PREBLUR_IN, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);
                gauss!($rd, 1.15);
                dispatch($rd, PASS_FLOW_PRE_FROM_GAUSS, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);
                // acc init = 1.0 (both buffers)
                dispatch($rd, PASS_ACC_INIT, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);
                // K ping-pong relaxation steps. In PASS_FLOW_RELAX, flow_dir selects the WRITE
                // target: fd=0 reads acc_a writes acc_b; fd=1 reads acc_b writes acc_a. The last
                // step is i=STABLE_ITERS-1, fd=(STABLE_ITERS-1)%2, so it writes:
                //   STABLE_ITERS even -> last fd=1 -> final result in acc_a
                //   STABLE_ITERS odd  -> last fd=0 -> final result in acc_b
                for i in 0..STABLE_ITERS {
                    let fd = if i % 2 == 0 { 0 } else { 1 };
                    dispatch($rd, PASS_FLOW_RELAX, 0, 0, 0, fd, $power, wg_full_x, wg_full_y);
                }
                // PASS_DISCHARGE: here flow_dir selects the READ buffer holding the final acc
                // (OPPOSITE of PASS_FLOW_RELAX, where it selects the write target) -> fd=0 reads
                // acc_a, fd=1 reads acc_b. So discharge_fd must equal the parity of the LAST write:
                //   STABLE_ITERS odd  -> final in acc_b -> discharge_fd=1
                //   STABLE_ITERS even -> final in acc_a -> discharge_fd=0
                // This trap is live ONLY if STABLE_ITERS changes (the flagged convergence knob).
                let discharge_fd: i32 = if STABLE_ITERS % 2 == 1 { 1 } else { 0 };
                debug_assert_eq!(discharge_fd, 1 - ((STABLE_ITERS as i32 - 1) % 2),
                    "discharge_fd must read the buffer the LAST relax step wrote");
                dispatch($rd, PASS_DISCHARGE, 0, 0, 0, discharge_fd, 0.0, wg_full_x, wg_full_y);
                // spread sigma = max(width, 0.1) (all widths here are >= 0.1)
                gauss!($rd, ($width as f64).max(0.1));
            }};
        }

        // 0) meshgrid ; 1) pointwise
        dispatch(&mut rd, PASS_MESHGRID, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);
        dispatch(&mut rd, PASS_POINTWISE, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);

        // 2) range_envelope = smoothstep(0.24,0.58, gaussian(ranges, 5.0))
        dispatch(&mut rd, PASS_COPY, 0, 0, CP_RANGES, 0, 0.0, wg_full_x, wg_full_y);
        gauss!(&mut rd, 5.0);
        dispatch(&mut rd, PASS_RANGE_ENV, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);

        // 3) lowland: broad_range = gaussian(ranges, 7.0); combine with regional
        dispatch(&mut rd, PASS_COPY, 0, 0, CP_RANGES, 0, 0.0, wg_full_x, wg_full_y);
        gauss!(&mut rd, 7.0);
        dispatch(&mut rd, PASS_LOWLAND, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);

        // 4) massif: gaussian(ranges,1.8) -> massif_inner; then gaussian(massif,2.0) writeback
        dispatch(&mut rd, PASS_COPY, 0, 0, CP_RANGES, 0, 0.0, wg_full_x, wg_full_y);
        gauss!(&mut rd, 1.8);
        dispatch(&mut rd, PASS_MASSIF_INNER, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);
        dispatch(&mut rd, PASS_COPY, 0, 0, CP_MASSIF, 0, 0.0, wg_full_x, wg_full_y);
        gauss!(&mut rd, 2.0);
        dispatch(&mut rd, PASS_MASSIF_WRITEBACK, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);

        // 5) base
        dispatch(&mut rd, PASS_BASE, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);

        // 6) primary channels: flow_channels_seam_safe(base, valley_width, power=0.48)
        dispatch(&mut rd, PASS_FLOW_PRE_BASE, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);
        flow_channels!(&mut rd, 0.48_f32, valley_width_px);
        dispatch(&mut rd, PASS_PRIMARY_MASK, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);

        // 7) tributaries: flow_channels_seam_safe(rough_surface, trib_width, power=0.34)
        dispatch(&mut rd, PASS_FLOW_PRE_ROUGH, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);
        flow_channels!(&mut rd, 0.34_f32, trib_width);
        dispatch(&mut rd, PASS_TRIB_MASK, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);

        // 8) high_mask / valley_mask
        dispatch(&mut rd, PASS_MASKS, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);

        // 9) assemble height
        dispatch(&mut rd, PASS_ASSEMBLE, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);

        // 10) floor blend
        dispatch(&mut rd, PASS_COPY, 0, 0, CP_VALLEY, 0, 0.0, wg_full_x, wg_full_y);
        gauss!(&mut rd, 1.2);
        dispatch(&mut rd, PASS_FLOOR_MASK, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);
        dispatch(&mut rd, PASS_COPY, 0, 0, CP_HEIGHT, 0, 0.0, wg_full_x, wg_full_y);
        gauss!(&mut rd, floor_smooth);
        dispatch(&mut rd, PASS_FLOOR_BLEND, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);

        // 11) final: height_blur = gaussian(height,1.2); final_blend; affine
        dispatch(&mut rd, PASS_COPY, 0, 0, CP_HEIGHT, 0, 0.0, wg_full_x, wg_full_y);
        gauss!(&mut rd, 1.2);
        dispatch(&mut rd, PASS_FINAL, 0, 0, 0, 0, 0.0, wg_full_x, wg_full_y);

        // 12) crop core (over core cells)
        dispatch(&mut rd, PASS_CROP, 0, 0, 0, 0, 0.0, wg_core_x, wg_core_y);

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
        let p = build_push(0, 344, 344, 160, 0, 4, 0, 0, 0, 3913.04, 12000.0, -31000.0, 90000.0, 0.48);
        assert_eq!(p.len(), 96);
    }

    #[test]
    fn push_constant_packs_ints_then_floats() {
        // build_push(pass,rows,cols,apron,seed,kradius,copy_sel,flow_dir,koffset,spacing,ox,oz,span,power)
        let p = build_push(7, 344, 343, 160, 5, 28, 2, 1, 128, 3913.0, 12000.0, -31000.0, 90000.0, 0.34);
        assert_eq!(i32::from_le_bytes([p[0], p[1], p[2], p[3]]), 7);
        assert_eq!(i32::from_le_bytes([p[4], p[5], p[6], p[7]]), 344);
        assert_eq!(i32::from_le_bytes([p[8], p[9], p[10], p[11]]), 343);
        assert_eq!(i32::from_le_bytes([p[12], p[13], p[14], p[15]]), 160);
        assert_eq!(i32::from_le_bytes([p[16], p[17], p[18], p[19]]), 5);
        assert_eq!(i32::from_le_bytes([p[20], p[21], p[22], p[23]]), 28);
        assert_eq!(i32::from_le_bytes([p[24], p[25], p[26], p[27]]), 2);
        assert_eq!(i32::from_le_bytes([p[28], p[29], p[30], p[31]]), 1);
        assert_eq!(i32::from_le_bytes([p[32], p[33], p[34], p[35]]), 128); // koffset
        // 3 int pad at 36..48; floats start at byte 48.
        let spacing = f32::from_le_bytes([p[48], p[49], p[50], p[51]]);
        assert!((spacing - 3913.0).abs() < 1e-1);
        // floats: spacing(48),ox(52),oz(56),span(60),power(64)
        let flow_power = f32::from_le_bytes([p[64], p[65], p[66], p[67]]);
        assert!((flow_power - 0.34).abs() < 1e-6);
    }
}
