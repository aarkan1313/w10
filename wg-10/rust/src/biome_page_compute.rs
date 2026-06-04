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
    RdTextureFormat, RdTextureView,
    rendering_device::{UniformType, ShaderStage, DataFormat, TextureUsageBits},
};

mod abi;
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
mod scheduler;
mod sigma_registry;

use abi::*;
pub(crate) use kernels::*;
pub(crate) use schedule_coast::schedule_coast;
pub(crate) use schedule_desert::schedule_desert;
pub(crate) use schedule_glacial::schedule_glacial;
pub(crate) use schedule_grassland::schedule_grassland;
pub(crate) use schedule_karst::schedule_karst;
pub(crate) use schedule_mountain::schedule_mountain;
pub(crate) use schedule_rainforest::schedule_rainforest;
pub(crate) use schedule_temperate::schedule_temperate;
pub(crate) use schedule_tundra::schedule_tundra;
pub(crate) use schedule_volcanic::schedule_volcanic;
pub(crate) use schedule_wetland::schedule_wetland;
pub(crate) use scheduler::*;
pub(crate) use sigma_registry::*;

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

/// f32 slice -> PackedFloat64Array (the GPU result widened back to f64 for the GDScript caller).
fn f32s_to_packed_f64(v: &[f32]) -> PackedFloat64Array {
    let mut out = PackedFloat64Array::new();
    out.resize(v.len());
    let sl = out.as_mut_slice();
    for i in 0..v.len() {
        sl[i] = v[i] as f64;
    }
    out
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

/// Image-binding RdUniform (the RUNTIME producer binds the caller's R32F page texture at
/// binding 41 via this). Same shape as page_compute.rs::make_image_uniform (replicated here --
/// 6 lines -- rather than exposing that module-private helper cross-module).
fn make_image_uniform(binding: i32, rid: Rid) -> Gd<RdUniform> {
    let mut u = RdUniform::new_gd();
    u.set_uniform_type(UniformType::IMAGE);
    u.set_binding(binding);
    u.add_id(rid);
    u
}

/// Allocate a 1x1 R32F STORAGE image. The biome_page machine now DECLARES (and statically uses,
/// in the PASS_CROP_IMG branch) a write-only image2D at binding 41 for the RUNTIME producer.
/// Godot's `uniform_set_create` validates a set against EVERY binding the shader statically uses,
/// so the THREE readback-only paths (run_inner / run_compose_engine / run_blend_inner) -- which
/// never dispatch PASS_CROP_IMG -- must still BIND something at 41 or the set is rejected. They
/// bind this throwaway 1x1 image: it is NEVER written (none of their passes touch out_img), so
/// every readback (core_out / height) is byte-identical to before the PASS_CROP_IMG addition.
/// The caller frees it alongside the other RIDs. (The runtime producer binds the REAL page
/// texture at 41 instead; this scratch is only for the readback harness.)
fn make_scratch_image_1x1(rd: &mut Gd<RenderingDevice>) -> Rid {
    let mut fmt = RdTextureFormat::new_gd();
    fmt.set_width(1);
    fmt.set_height(1);
    fmt.set_format(DataFormat::R32_SFLOAT);
    fmt.set_usage_bits(TextureUsageBits::STORAGE_BIT | TextureUsageBits::CAN_COPY_FROM_BIT);
    let view = RdTextureView::new_gd();
    rd.texture_create(&fmt, &view)
}

/// Build the 96-byte push constant (std430): 12 i32 (48B) then 12 f32 (48B).
/// Layout MUST match biome_page.glsl Params.
///
/// `vent_count` occupies the former `ipad1` int slot (index 10). It is 0 for every non-volcanic
/// biome (the 10 proven biomes pass vent_count=0 -> the exact bytes the hardcoded `0` produced
/// before, so their push is byte-identical). VOLCANIC passes its actual vent count there.
/// Compose params carried in the two leading float PADS (pad0 = favor_strength,
/// pad1 = relief_confidence_floor). 0.0 for every non-compose dispatch -> the bytes are
/// byte-identical to the former all-zero pad block, so the 11 proven biomes are unaffected.
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
    vent_count: i32,
    spacing: f32,
    ox: f32,
    oz: f32,
    feature_span_m: f32,
    flow_power: f32,
    favor_strength: f32,         // -> pad0
    relief_confidence_floor: f32, // -> pad1
    relief_m: f32,                // -> pad2 (RUNTIME crop-to-image height scale; 0 elsewhere)
) -> Vec<u8> {
    let mut b = Vec::with_capacity(96);
    // 12 ints: pass,rows,cols,apron,seed,kradius,copy_sel,flow_dir,koffset,pool_sel,vent_count + 1 pad.
    for v in [pass, rows, cols, apron_px, seed, kradius, copy_sel, flow_dir, koffset, pool_sel, vent_count, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    // 12 floats: spacing,ox,oz,feature_span_m,flow_power, pad0(favor_strength), pad1(relief_conf_floor),
    // pad2(relief_m) + 4 pad.
    for v in [spacing, ox, oz, feature_span_m, flow_power, favor_strength, relief_confidence_floor, relief_m] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for _ in 0..4 {
        b.extend_from_slice(&0.0_f32.to_le_bytes());
    }
    b
}

// ===========================================================================
// RUNTIME mountain page producer (Slice-4b, Task 3): the runtime sibling of the readback TEST
// harness `run_inner`. Runs the SAME parity-proven `schedule_mountain` dispatch sequence, but on
// the GLOBAL RenderingDevice with a CACHED context (compiled once, all buffers allocated once)
// and writes each page into a CALLER-OWNED R32F texture (PASS_CROP_IMG -> binding 41) instead of
// reading the core back. Mirrors page_compute.rs's PageComputeContext ownership model.
//
// The math is proven (the existing biome_page parity gate). This is the runtime PLUMBING. The
// LATER windowed 576 parity gate (Task 4) is what PROVES the dispatch is correct (it asserts the
// runtime texture matches the readback core); cargo-green here only proves it compiles + the pure
// helpers + that the existing harness is byte-identical (the 210 stay green).
// ===========================================================================

/// All the per-page-INVARIANT apron working-grid buffers the machine's uniform set binds
/// (the 19 named fields 0..18, the packed kernel buffer 19, flow_pre/acc_a/acc_b 20..22, the core
/// 23, the POOL_SLOTS pool buffers 24..24+SLOTS-1, and the vent buffer 40). Allocated ONCE by
/// `alloc_apron_buffers` and owned by `BiomePageComputeContext`. `kparams` resolves each sigma to
/// its packed-kernel (koffset, kradius). The buffer ROLES + bindings are byte-identical to what
/// `run_inner` allocates inline (same sizes, same zero-init, same kernel packing).
struct ApronBuffers {
    /// bindings 0..=18 (wx, wz, regional, ranges, ridge_detail, near_detail, range_envelope,
    /// lowland, massif, base, primary_mask, tributary_mask, high_mask, valley_mask, height,
    /// floor_mask, gauss_in, gauss_mid, gauss_out) -- the 19 fixed named fields, in binding order.
    fields: Vec<Rid>,
    kernel: Rid,    // binding 19 (packed gaussian kernels at slot*KERNEL_STRIDE)
    flow_pre: Rid,  // binding 20
    acc_a: Rid,     // binding 21
    acc_b: Rid,     // binding 22
    core: Rid,      // binding 23 (storage; schedule_mountain's trailing PASS_CROP writes it, inert)
    pool: Vec<Rid>, // bindings 24..24+POOL_SLOTS-1
    vents: Rid,     // binding 40
    kparams: KernelParams,
}

impl ApronBuffers {
    /// (binding, rid) pairs for the WHOLE machine uniform set EXCEPT the runtime output image
    /// (binding 41). Same binding map `run_inner` builds. The runtime uniform set appends the
    /// image; the test harness (run_inner) does not (and never dispatches PASS_CROP_IMG).
    fn buffer_bindings(&self) -> Vec<(i32, Rid)> {
        let mut b: Vec<(i32, Rid)> = Vec::with_capacity(24 + POOL_SLOTS + 1);
        for (i, &rid) in self.fields.iter().enumerate() {
            b.push((i as i32, rid)); // 0..=18
        }
        b.push((19, self.kernel));
        b.push((20, self.flow_pre));
        b.push((21, self.acc_a));
        b.push((22, self.acc_b));
        b.push((23, self.core));
        for (k, &rid) in self.pool.iter().enumerate() {
            b.push((24 + k as i32, rid));
        }
        b.push((40, self.vents));
        b
    }

    /// Free every RID this owns. The B1 RID-leak lesson: miss none (19 fields + kernel +
    /// flow_pre/acc_a/acc_b + core + POOL_SLOTS pool + vents).
    fn free(&self, rd: &mut Gd<RenderingDevice>) {
        for &rid in &self.fields {
            rd.free_rid(rid);
        }
        rd.free_rid(self.kernel);
        rd.free_rid(self.flow_pre);
        rd.free_rid(self.acc_a);
        rd.free_rid(self.acc_b);
        rd.free_rid(self.core);
        for &rid in &self.pool {
            rd.free_rid(rid);
        }
        rd.free_rid(self.vents);
    }
}

/// Allocate the full apron working-grid buffer set on `rd` (the SAME set `run_inner` allocates
/// inline, in the same binding order with the same zero-init + kernel packing). Factored so the
/// runtime context builder shares it; `run_inner` is left byte-identical (it still allocates
/// inline -- not worth churning the parity-proven path). `n = rows*cols`, `core_n =
/// core_rows*core_cols`. Returns the buffer set or an Err (freeing nothing partial -- the caller
/// only proceeds on Ok, and on Err the few buffers already created are leaked only on a hard
/// pre-list failure that aborts the whole context build, where the rd is the global one; we free
/// what we hold via the returned-on-error path below). `biome` selects the sigma list.
//
// NOTE: run_inner allocates this SAME buffer set inline (parity-frozen path); keep the two in
// sync until Task 4's 576 gate is green and run_inner can consume this helper.
fn alloc_apron_buffers(
    rd: &mut Gd<RenderingDevice>,
    rows: usize,
    cols: usize,
    core_n: usize,
    biome: &str,
    seed: i32,
    feature_span_m: f32,
) -> Result<ApronBuffers, String> {
    let n = rows * cols;
    let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };
    let field_bytes = n * 4;
    let zeros = vec![0.0_f32; n];
    let zeros_pba = PackedByteArray::from(f32s_to_bytes(&zeros).as_slice());
    let mk_field = |rd: &mut Gd<RenderingDevice>| -> Rid {
        rd.storage_buffer_create_ex(bsize(field_bytes)).data(&zeros_pba).done()
    };

    // 19 named fields (bindings 0..=18), in the SAME order as run_inner.
    let mut fields: Vec<Rid> = Vec::with_capacity(19);
    for _ in 0..19 {
        fields.push(mk_field(rd));
    }

    // packed kernel buffer (19): all distinct biome sigmas' kernels at slot*KERNEL_STRIDE.
    let helper_free = |rd: &mut Gd<RenderingDevice>, fields: &[Rid]| {
        for &rid in fields {
            rd.free_rid(rid);
        }
    };
    let sigmas = match biome_sigmas(biome) {
        Some(s) => s,
        None => {
            helper_free(rd, &fields);
            return Err(format!("no sigma list for biome '{biome}' (add a biome_sigmas arm)"));
        }
    };
    let n_slots = sigmas.len();
    let mut packed = vec![0.0_f32; n_slots * KERNEL_STRIDE];
    for (slot, &sg) in sigmas.iter().enumerate() {
        let k = gaussian_kernel1d(sg, TRUNCATE);
        if k.len() > KERNEL_STRIDE {
            helper_free(rd, &fields);
            return Err(format!(
                "gaussian kernel len {} (sigma {sg}) > KERNEL_STRIDE {KERNEL_STRIDE}",
                k.len()
            ));
        }
        let base = slot * KERNEL_STRIDE;
        packed[base..base + k.len()].copy_from_slice(&k);
    }
    let packed_pba = PackedByteArray::from(f32s_to_bytes(&packed).as_slice());
    let kernel = rd
        .storage_buffer_create_ex(bsize(packed.len() * 4))
        .data(&packed_pba)
        .done(); // 19
    let kparams = KernelParams::from_sigmas(&sigmas);

    let flow_pre = mk_field(rd); // 20
    let acc_a = mk_field(rd); // 21
    let acc_b = mk_field(rd); // 22

    // core output (23)
    let core_zeros = vec![0.0_f32; core_n];
    let core_pba = PackedByteArray::from(f32s_to_bytes(&core_zeros).as_slice());
    let core = rd
        .storage_buffer_create_ex(bsize(core_n * 4))
        .data(&core_pba)
        .done(); // 23

    // POOL (24..24+POOL_SLOTS-1)
    let pool: Vec<Rid> = (0..POOL_SLOTS).map(|_| mk_field(rd)).collect();

    // VENT buffer (40): zeroed for non-volcanic biomes (mountain never reads it).
    let (vent_packed, _vent_count): (Vec<f32>, usize) = if biome == "volcanic" {
        crate::recipes_volcanic::volcanic::packed_vents(
            &crate::recipes_volcanic::volcanic::STRATOVOLCANO_CLUSTER,
            seed as i64,
            feature_span_m as f64,
        )
    } else {
        let stride = crate::recipes_volcanic::volcanic::VENT_STRIDE;
        let maxv = crate::recipes_volcanic::volcanic::MAX_VENTS;
        (vec![0.0_f32; maxv * stride], 0)
    };
    let vent_pba = PackedByteArray::from(f32s_to_bytes(&vent_packed).as_slice());
    let vents = rd
        .storage_buffer_create_ex(bsize(vent_packed.len() * 4))
        .data(&vent_pba)
        .done(); // 40

    Ok(ApronBuffers {
        fields,
        kernel,
        flow_pre,
        acc_a,
        acc_b,
        core,
        pool,
        vents,
        kparams,
    })
}

/// The per-page-INVARIANT GPU resources for the RUNTIME mountain page producer: the compiled
/// shader, the compute pipeline, and the full apron buffer set (`ApronBuffers`). Built ONCE
/// (`build_biome_page_context`) on the GLOBAL rd and reused for every page; only the per-page
/// uniform set (cached buffers + this page's image) + push constant vary. Mirrors
/// page_compute.rs::PageComputeContext. Owns every RID -> `free_biome_page_context` frees them all.
///
/// `apron_dim` is the padded working-grid dim (core + 2*apron); `core_px` the core dim;
/// `apron_px` the apron each side; `flow_iters` the PULL-relaxation step count (STABLE_ITERS for
/// the parity-proven path). The vent_count is fixed to 0 here (mountain is the only wired biome;
/// volcanic would need its own context with the live vent count).
pub(crate) struct BiomePageComputeContext {
    pub shader: Rid,
    pub pipeline: Rid,
    bufs: ApronBuffers,
    pub apron_dim: usize,
    pub core_px: usize,
    pub apron_px: usize,
    pub flow_iters: usize,
    /// RUNTIME relief scale (metres): the normalized recipe height (~[-3,2]) is multiplied by this in
    /// PASS_CROP_IMG before the page texture is written, so the render shader (VERTEX.y = h *
    /// relief_scale) gets metres. Tunable via `configure_biome` (the vertical-scale knob).
    pub relief_m: f32,
}

/// Build the cached runtime context ONCE on the GLOBAL `rd`: concat primitives + machine +
/// mountain fragment (EXACTLY as `run_inner` does, via `concat_glsl_hoist_version`), compile,
/// create the pipeline, allocate the full apron buffer set. `core_px`/`apron_px` size the working
/// grid (mountain: 256 / 160 -> apron_dim 576). Returns Err on any compile/create failure (freeing
/// what it already allocated). The producer is wired for "mountain" only (the proven recipe); the
/// biome string is hardcoded so the sigma list + schedule match.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_biome_page_context(
    rd: &mut Gd<RenderingDevice>,
    primitives_src: &str,
    machine_src: &str,
    mountain_fragment_src: &str,
    core_px: usize,
    apron_px: usize,
    flow_iters: usize,
    relief_m: f32,
) -> Result<BiomePageComputeContext, String> {
    let apron_dim = biome_apron_dim(core_px, apron_px);
    if apron_dim <= 2 * apron_px {
        return Err(format!(
            "build_biome_page_context: apron {apron_px} too large for core {core_px}"
        ));
    }
    // concat primitives + (machine + "\n" + fragment), hoisting the machine's #version to line 1 --
    // byte-identical to run_inner's compile path.
    let machine_plus_fragment = format!("{machine_src}\n{mountain_fragment_src}");
    let glsl_stripped =
        crate::primitive_probe::concat_glsl_hoist_version(primitives_src, &machine_plus_fragment);
    let mut src = RdShaderSource::new_gd();
    src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
    let spirv = rd
        .shader_compile_spirv_from_source(&src)
        .ok_or_else(|| {
            "build_biome_page_context: shader_compile_spirv_from_source returned null".to_string()
        })?;
    {
        let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
        if !err.is_empty() {
            return Err(format!("build_biome_page_context: GLSL compile error: {err}"));
        }
    }
    let shader = rd.shader_create_from_spirv(&spirv);
    if shader.is_invalid() {
        return Err("build_biome_page_context: shader_create_from_spirv returned invalid RID".into());
    }
    let pipeline = rd.compute_pipeline_create(shader);
    if pipeline.is_invalid() {
        rd.free_rid(shader);
        return Err(
            "build_biome_page_context: compute_pipeline_create returned invalid RID".into(),
        );
    }

    let core_n = core_px * core_px;
    let bufs = match alloc_apron_buffers(rd, apron_dim, apron_dim, core_n, "mountain", 0, 0.0) {
        Ok(b) => b,
        Err(e) => {
            rd.free_rid(pipeline);
            rd.free_rid(shader);
            return Err(format!("build_biome_page_context: {e}"));
        }
    };

    Ok(BiomePageComputeContext {
        shader,
        pipeline,
        bufs,
        apron_dim,
        core_px,
        apron_px,
        flow_iters,
        relief_m,
    })
}

/// Free EVERY RID the runtime context owns (all apron buffers, pipeline, shader). Per-page uniform
/// sets are freed per page inside `compute_biome_page_cached`. The B1 RID-leak lesson: miss none.
pub(crate) fn free_biome_page_context(rd: &mut Gd<RenderingDevice>, ctx: &BiomePageComputeContext) {
    ctx.bufs.free(rd);
    rd.free_rid(ctx.pipeline);
    rd.free_rid(ctx.shader); // cascades any remaining uniform sets created against it
}

/// Dispatch ONE mountain page into `target_rid` (a caller-owned R32F texture) using the CACHED
/// context. Per-page work only: build the uniform set (cached buffers + this page's image at
/// binding 41), open a compute list, construct a `Scheduler` over the cached buffers + open list,
/// run `schedule_mountain` (the SAME proven sequence the test harness runs -- its trailing
/// PASS_CROP into the core storage buffer is inert here), then dispatch PASS_CROP_IMG (core
/// workgroups) to write `target_rid`, submit + sync. NO readback. Frees ONLY the per-page uniform
/// set; the cached shader/pipeline/buffers persist. `target_rid` is NOT freed (the caller owns it).
///
/// `spacing = world_span / (page_px - 1)` (texel-CORNER convention: texel 0 -> origin, page_px-1
/// -> origin+span), matching height_page.glsl:191-195. The apron-padded origin is
/// `origin - apron_px*spacing` per axis (the meshgrid pass subtracts the apron back off).
///
/// SCALE-INVARIANCE: `spacing` is computed INTERNALLY (callers don't pass it) and world-anchors
/// every gaussian kernel via `mountain_kernels_anchored(spacing)` -> the cached kernel buffer is
/// RE-FILLED per dispatch (the buffer RID stays allocated; only its bytes change) so each clipmap
/// LEVEL bakes its blurs at its OWN spacing -> the macro structure matches across levels (no
/// geomorph warp). `flow_on` gates the drainage carve: `false` on coarse levels SKIPS the two
/// flow_channels passes (cheaper) -> the MACRO surface, mirroring the CPU oracle. At
/// `spacing == S_REF` (32.0) + `flow_on == true` this reproduces the parity-proven page byte-for-byte.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_biome_page_cached(
    rd: &mut Gd<RenderingDevice>,
    ctx: &BiomePageComputeContext,
    target_rid: Rid,
    origin_x: f64,
    origin_z: f64,
    world_span: f64,
    page_px: i64,
    feature_span_m: f64,
    seed: i64,
    flow_on: bool,
) -> Result<(), String> {
    if page_px as usize != ctx.core_px {
        return Err(format!(
            "compute_biome_page_cached: page_px {page_px} != context core_px {} (rebuild the context)",
            ctx.core_px
        ));
    }
    if page_px < 2 {
        return Err(format!("compute_biome_page_cached: page_px {page_px} must be >= 2"));
    }
    if seed < i32::MIN as i64 || seed > i32::MAX as i64 {
        return Err(format!(
            "compute_biome_page_cached: seed {seed} outside i32 range (GPU hash is 32-bit-seed)"
        ));
    }
    // spacing computed INSIDE (texel-corner): callers pass world_span+page_px, not spacing.
    let spacing_f64 = world_span / (page_px as f64 - 1.0);
    let spacing = spacing_f64 as f32;
    let ox = (origin_x - ctx.apron_px as f64 * spacing as f64) as f32;
    let oz = (origin_z - ctx.apron_px as f64 * spacing as f64) as f32;

    // SCALE-INVARIANCE: rebuild the WORLD-anchored gaussian kernels for THIS dispatch's spacing and
    // re-fill the cached kernel buffer (binding 19) in place. `kparams_anchored` keeps the same slot
    // LOOKUP keys (the reference cell sigmas `schedule_mountain` asks for) but with the anchored
    // koffset/kradius; the GLSL machine is unchanged (it reads kradius/koffset from the push constant
    // and taps the now-anchored packed buffer). Built BEFORE the compute list opens (a panic mid-list
    // would leak the open list); the buffer_update is RECORDED on the global RD and auto-submitted
    // before the compute dispatches, exactly like the create-time .data() upload.
    // NOTE (deferred optimization): the packed kernel is rebuilt + re-uploaded PER PAGE (~2.3 KB =
    // 9 slots * 64 taps * 4 B, non-stalling buffer_update). spacing takes only ~NUM_LEVELS distinct
    // values across the whole clipmap, so a cache-by-spacing (build each level's kernels once, keep
    // a small LUT, bind/select instead of re-upload) is the deferred optimization (scale-invariant
    // plan) IF this ever shows on a perf profile. Kept simple-per-page until measured.
    let (packed_kernel, kparams_anchored) = mountain_kernels_anchored(spacing_f64)?;
    let packed_pba = PackedByteArray::from(f32s_to_bytes(&packed_kernel).as_slice());
    let upd = rd.buffer_update(
        ctx.bufs.kernel,
        0,
        (packed_kernel.len() * 4) as u32,
        &packed_pba,
    );
    if upd != godot::global::Error::OK {
        return Err(format!(
            "compute_biome_page_cached: buffer_update(kernel) failed: {upd:?}"
        ));
    }
    // INVARIANT: the anchored kparams must key by the SAME reference sigmas (same slot LAYOUT) the
    // context allocated -- only koffset/kradius differ with spacing. If the slot KEYS ever diverged
    // the re-filled buffer would be indexed by a koffset the schedule never produced. (Also keeps
    // the context's build-time `kparams` a live read, documenting the layout contract.)
    debug_assert_eq!(
        ctx.bufs.kparams.slots.len(),
        kparams_anchored.slots.len(),
        "anchored kparams slot count must match the context's allocated kernel layout"
    );
    debug_assert!(
        ctx.bufs
            .kparams
            .slots
            .iter()
            .zip(kparams_anchored.slots.iter())
            .all(|(a, b)| (a.0 - b.0).abs() < 1e-9 && a.1 == b.1),
        "anchored kparams must key by the SAME reference sigmas at the SAME koffsets"
    );

    // per-page uniform set: the cached buffers (0..40) + this page's image (41).
    let bindings = ctx.bufs.buffer_bindings();
    let mut uniforms: Array<Gd<RdUniform>> = Array::new();
    for (bind, rid) in bindings.iter() {
        uniforms.push(&make_storage_uniform(*bind, *rid));
    }
    uniforms.push(&make_image_uniform(41, target_rid));
    let uset = rd.uniform_set_create(&uniforms, ctx.shader, 0);
    if uset.is_invalid() {
        return Err("compute_biome_page_cached: uniform_set_create returned invalid RID".into());
    }

    let rows = ctx.apron_dim;
    let cols = ctx.apron_dim;
    let apron = ctx.apron_px;
    let core_rows = ctx.core_px;
    let core_cols = ctx.core_px;
    let wg_full_x = (cols as u32).div_ceil(16);
    let wg_full_y = (rows as u32).div_ceil(16);
    let wg_core_x = (core_cols as u32).div_ceil(16);
    let wg_core_y = (core_rows as u32).div_ceil(16);

    // PRE-VALIDATE every sigma BEFORE the list opens (KernelParams::kp `.expect`s; a panic with an
    // open list would leak). The ANCHORED kparams key by the SAME reference sigmas mountain_sigmas()
    // lists (only koffset/kradius differ), so every lookup the schedule makes resolves.
    for &sg in mountain_sigmas().iter() {
        let _ = kparams_anchored.kp(sg);
    }

    let cl = rd.compute_list_begin();
    rd.compute_list_bind_compute_pipeline(cl, ctx.pipeline);

    // Scheduler over the cached buffers + open list. The ANCHORED kparams (built above from this
    // dispatch's spacing) is moved into the Scheduler so the gauss passes use the anchored radii.
    let kparams = kparams_anchored;
    let mut sched = Scheduler {
        rd,
        cl,
        uset,
        rows: rows as i32,
        cols: cols as i32,
        apron: apron as i32,
        seed: seed as i32,
        spacing,
        ox,
        oz,
        feature_span_m: feature_span_m as f32,
        vent_count: 0, // mountain never reads vents
        favor_strength: 0.0,
        relief_confidence_floor: 0.0,
        relief_m: ctx.relief_m, // RUNTIME: scale normalized height -> metres in PASS_CROP_IMG
        wg_full_x,
        wg_full_y,
        wg_core_x,
        wg_core_y,
        kparams,
        flow_iters: ctx.flow_iters,
        flow_on, // SCALE-INVARIANCE: coarse levels pass false -> macro surface (no carve).
    };
    // Run the PROVEN mountain schedule (ends with PASS_CROP into the core storage buffer -- inert
    // here, we don't read it), then crop to the IMAGE.
    schedule_mountain(&mut sched);
    sched.dispatch(PASS_CROP_IMG, 0, 0, 0, 0, 0.0, 0, wg_core_x, wg_core_y);

    rd.compute_list_end();
    // RUNTIME (global RD): fire-and-forget — do NOT submit()/sync() here. This producer runs on the
    // MAIN RenderingDevice (the one the renderer owns), where manual submit/sync is ILLEGAL
    // ("Only local devices can submit and sync" — rendering_device.cpp:6551). The engine auto-submits
    // the global RD's queued work at draw, exactly like the legacy `compute_page_cached`
    // (page_compute.rs:166: "no submit/sync; the engine auto-submits at draw"). Intra-schedule
    // ordering is enforced by the `compute_list_add_barrier` calls RECORDED INTO the list (Scheduler),
    // which are honored at submission regardless of who submits. (The readback test entries use a
    // LOCAL rd via create_local_rendering_device, where submit/sync IS legal — those keep theirs.)
    rd.free_rid(uset); // free ONLY the per-page uniform set; cached resources persist
    Ok(())
}

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
        flow_iters: usize,
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
        // NOTE: duplicates alloc_apron_buffers' recipe (kept inline so the 210 byte-identical
        // readback tests stay frozen). Unify once the 576 parity gate is green.
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

        // VENT buffer (binding 40): VOLCANIC's CPU-built packed vent list (vx,vz,amp + 4 flow dirs
        // per vent, padded to MAX_VENTS). THE KEY INSIGHT -- the numpy PCG64 RNG that places the
        // vents stays in RUST (recipes_volcanic::packed_vents, parity-exact); this buffer is the
        // only thing the GPU consumes (PURE f32 cone/crater/shield/flow math, NO RNG in GLSL).
        // ADDITIVE: every biome gets the binding (so the uniform set always satisfies the machine's
        // binding 40); the 10 non-volcanic biomes get a zeroed buffer + vent_count=0 (never read).
        let (vent_packed, vent_count): (Vec<f32>, usize) = if biome == "volcanic" {
            crate::recipes_volcanic::volcanic::packed_vents(
                &crate::recipes_volcanic::volcanic::STRATOVOLCANO_CLUSTER,
                seed as i64,
                feature_span_m as f64,
            )
        } else {
            let stride = crate::recipes_volcanic::volcanic::VENT_STRIDE;
            let maxv = crate::recipes_volcanic::volcanic::MAX_VENTS;
            (vec![0.0_f32; maxv * stride], 0)
        };
        let vent_pba = PackedByteArray::from(f32s_to_bytes(&vent_packed).as_slice());
        let b_vents = rd
            .storage_buffer_create_ex(bsize(vent_packed.len() * 4))
            .data(&vent_pba)
            .done(); // 40

        // one uniform set binding the 24 fixed buffers + POOL_SLOTS pool buffers + vent buffer.
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
        bindings.push((40, b_vents));
        let mut uniforms: Array<Gd<RdUniform>> = Array::new();
        for (bind, rid) in bindings.iter() {
            uniforms.push(&make_storage_uniform(*bind, *rid));
        }
        // Scratch 1x1 image at binding 41 (machine declares out_img for the RUNTIME PASS_CROP_IMG;
        // this readback path never dispatches it, so the image is never written -> core readback is
        // byte-identical, but the uniform set must still satisfy binding 41). Pushed into `bindings`
        // AFTER the storage-uniform loop so the trailing free loop frees its texture RID, while its
        // uniform is added as an IMAGE (not a storage buffer) below.
        let scratch_img = make_scratch_image_1x1(&mut rd);
        uniforms.push(&make_image_uniform(41, scratch_img));
        bindings.push((41, scratch_img));
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
            vent_count: vent_count as i32,
            favor_strength: 0.0,           // biome path never composes -> 0 (byte-identical push)
            relief_confidence_floor: 0.0,
            relief_m: 0.0,                 // readback harness crops to BUFFER (PASS_CROP, ignores pad2)
            wg_full_x,
            wg_full_y,
            wg_core_x,
            wg_core_y,
            kparams,
            flow_iters,
            // READBACK harness: ALWAYS flow_on (the parity-frozen sequence the 576/biome gates prove
            // against the flow-ON oracle). The flow_on=false coarse-level path is exercised ONLY via
            // the runtime producer (compute_biome_page_cached) + the pool.
            flow_on: true,
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
            "temperate" => schedule_temperate(&mut sched),
            "rainforest" => schedule_rainforest(&mut sched),
            "volcanic" => schedule_volcanic(&mut sched),
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

    /// COMPOSE engine (Slice-4b.11): the shared GPU setup for the compose layer. Allocates the SAME
    /// machine binding set (0..40) as `run_inner` (the machine declares them all, so the uniform set
    /// must satisfy every binding), uploads the compose initial buffers (height=acc0, base=acc_w0,
    /// pool0=f, pool1=w), builds the kernel buffer with the single compose relief sigma (6.0), opens
    /// one compute list, builds a Scheduler with the compose params, runs `op` (the fold/blend
    /// sequence), then reads back `height` (the composed result -- compose has NO apron / crop, the
    /// whole rows*cols field is the answer). WINDOWED only (local RD null headless). Concats the
    /// MOUNTAIN fragment purely to satisfy the machine's `biome_pass()` declaration -- the compose
    /// passes are handled INLINE in main() and never reach the fragment.
    fn run_compose_engine(
        &self,
        rows: usize,
        cols: usize,
        favor_strength: f32,
        relief_confidence_floor: f32,
        acc0: &[f32],   // -> height  (binding 14)
        accw0: &[f32],  // -> base    (binding 9)
        f0: &[f32],     // -> pool0   (binding 24)
        w0: &[f32],     // -> pool1   (binding 25)
        use_favored: bool,
    ) -> Result<(Vec<f32>, Vec<f32>), String> {
        let n = rows * cols;
        if acc0.len() != n || accw0.len() != n || f0.len() != n || w0.len() != n {
            return Err(format!("compose engine: buffer len != rows*cols ({n})"));
        }
        let prim = self.primitives_src.as_deref().ok_or("no GLSL source loaded")?;
        let machine = self.machine_src.as_deref().ok_or("no GLSL source loaded")?;
        let fragment = self.compose_fragment.as_deref()
            .ok_or("no compose fragment loaded (call load_compose_fragment)")?;

        let mut rd: Gd<RenderingDevice> = RenderingServer::singleton()
            .create_local_rendering_device()
            .ok_or_else(|| {
                "create_local_rendering_device returned null (headless / no device)".to_string()
            })?;

        // compile: machine + a fragment (mountain) so biome_pass() is defined (compose passes never
        // reach it -- they're inline in main()).
        let machine_plus_fragment = format!("{machine}\n{fragment}");
        let glsl_stripped = crate::primitive_probe::concat_glsl_hoist_version(prim, &machine_plus_fragment);
        let mut src = RdShaderSource::new_gd();
        src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
        let spirv = match rd.shader_compile_spirv_from_source(&src) {
            Some(s) => s,
            None => { rd.free(); return Err("shader_compile_spirv_from_source returned null".to_string()); }
        };
        {
            let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
            if !err.is_empty() { rd.free(); return Err(format!("GLSL compile error: {err}")); }
        }
        let shader = rd.shader_create_from_spirv(&spirv);
        if shader.is_invalid() { rd.free(); return Err("shader_create_from_spirv returned invalid RID".into()); }

        let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };
        let field_bytes = n * 4;
        let zeros = vec![0.0_f32; n];
        let zeros_pba = PackedByteArray::from(f32s_to_bytes(&zeros).as_slice());
        let mk_zero = |rd: &mut Gd<RenderingDevice>| -> Rid {
            rd.storage_buffer_create_ex(bsize(field_bytes)).data(&zeros_pba).done()
        };
        let mk_data = |rd: &mut Gd<RenderingDevice>, data: &[f32]| -> Rid {
            let pba = PackedByteArray::from(f32s_to_bytes(data).as_slice());
            rd.storage_buffer_create_ex(bsize(field_bytes)).data(&pba).done()
        };

        // Named buffers 0..18. Compose roles: 6=range_envelope(relief_a), 7=lowland(w_acc),
        // 8=massif(relief_b), 9=base(acc_w), 14=height(acc). The rest are inert scratch (zeroed).
        let mut named: Vec<Rid> = Vec::with_capacity(19);
        for b in 0..19usize {
            let rid = match b {
                9 => mk_data(&mut rd, accw0),  // base = acc_w0
                14 => mk_data(&mut rd, acc0),  // height = acc0
                _ => mk_zero(&mut rd),
            };
            named.push(rid);
        }

        // kernel buffer (19): single compose relief sigma (6.0) at slot 0.
        let sigmas = compose_sigmas();
        let n_slots = sigmas.len();
        let mut packed = vec![0.0_f32; n_slots * KERNEL_STRIDE];
        for (slot, &sg) in sigmas.iter().enumerate() {
            let k = gaussian_kernel1d(sg, TRUNCATE);
            if k.len() > KERNEL_STRIDE {
                rd.free_rid(shader); rd.free();
                return Err(format!("compose kernel len {} (sigma {sg}) > KERNEL_STRIDE {KERNEL_STRIDE}", k.len()));
            }
            let base = slot * KERNEL_STRIDE;
            packed[base..base + k.len()].copy_from_slice(&k);
        }
        let packed_pba = PackedByteArray::from(f32s_to_bytes(&packed).as_slice());
        let b_kernel = rd.storage_buffer_create_ex(bsize(packed.len() * 4)).data(&packed_pba).done();
        let kparams = KernelParams::from_sigmas(&sigmas);

        let b_flow_pre = mk_zero(&mut rd); // 20
        let b_acc_a = mk_zero(&mut rd);    // 21
        let b_acc_b = mk_zero(&mut rd);    // 22

        // core output (23): unused by compose (no CROP), but the binding must exist for the set.
        let b_core = mk_zero(&mut rd);

        // pool slots 24..39: pool0 = f, pool1 = w, the rest inert.
        let mut b_pool: Vec<Rid> = Vec::with_capacity(POOL_SLOTS);
        for slot in 0..POOL_SLOTS {
            let rid = match slot {
                0 => mk_data(&mut rd, f0),
                1 => mk_data(&mut rd, w0),
                _ => mk_zero(&mut rd),
            };
            b_pool.push(rid);
        }

        // vent buffer (40): inert for compose (machine declares the binding).
        let vent_stride = crate::recipes_volcanic::volcanic::VENT_STRIDE;
        let maxv = crate::recipes_volcanic::volcanic::MAX_VENTS;
        let vent_zeros = vec![0.0_f32; maxv * vent_stride];
        let vent_pba = PackedByteArray::from(f32s_to_bytes(&vent_zeros).as_slice());
        let b_vents = rd.storage_buffer_create_ex(bsize(vent_zeros.len() * 4)).data(&vent_pba).done();

        // uniform set (same binding map as run_inner).
        let mut bindings: Vec<(i32, Rid)> = Vec::new();
        for (b, &rid) in named.iter().enumerate() {
            bindings.push((b as i32, rid));
        }
        bindings.push((19, b_kernel));
        bindings.push((20, b_flow_pre));
        bindings.push((21, b_acc_a));
        bindings.push((22, b_acc_b));
        bindings.push((23, b_core));
        for (k, &rid) in b_pool.iter().enumerate() {
            bindings.push((24 + k as i32, rid));
        }
        bindings.push((40, b_vents));
        let mut uniforms: Array<Gd<RdUniform>> = Array::new();
        for (bind, rid) in bindings.iter() {
            uniforms.push(&make_storage_uniform(*bind, *rid));
        }
        // Scratch 1x1 image at binding 41 (machine declares out_img for PASS_CROP_IMG; compose
        // never dispatches it -> result byte-identical, but the set must satisfy binding 41).
        let scratch_img = make_scratch_image_1x1(&mut rd);
        uniforms.push(&make_image_uniform(41, scratch_img));
        bindings.push((41, scratch_img));
        let uset = rd.uniform_set_create(&uniforms, shader, 0);
        let pipeline = rd.compute_pipeline_create(shader);

        let wg_full_x = (cols as u32).div_ceil(16);
        let wg_full_y = (rows as u32).div_ceil(16);

        // pre-validate the compose sigma BEFORE the list opens (kp uses .expect()).
        for &s in &sigmas { let _ = kparams.kp(s); }

        let cl = rd.compute_list_begin();
        rd.compute_list_bind_compute_pipeline(cl, pipeline);
        let mut sched = Scheduler {
            rd: &mut rd,
            cl,
            uset,
            rows: rows as i32,
            cols: cols as i32,
            apron: 0,
            seed: 0,
            spacing: 0.0,
            ox: 0.0,
            oz: 0.0,
            feature_span_m: 0.0,
            vent_count: 0,
            favor_strength,
            relief_confidence_floor,
            relief_m: 0.0,                 // compose engine never crops to the runtime image
            wg_full_x,
            wg_full_y,
            wg_core_x: wg_full_x,
            wg_core_y: wg_full_y,
            kparams,
            flow_iters: STABLE_ITERS, // compose never runs flow; value is irrelevant but required
            flow_on: true,            // compose never runs schedule_mountain; irrelevant but required
        };

        // ONE compose_biomes fold step: acc=height, acc_w=base, f=pool0, w=pool1 pre-loaded.
        //   w_acc = acc_w/(acc_w+w+1e-12) -> lowland
        //   acc   = blend(acc, f, w_acc)  -> height
        //   acc_w += w (PASS_COMPOSE_ACCW_ADD) -> base
        // The engine reads back BOTH height (new acc) and base (new acc_w) for the next step.
        sched.compose_wacc();
        if use_favored {
            sched.blend_favored_step();
        } else {
            sched.blend_field_step();
        }
        sched.compose_accw_add();

        rd.compute_list_end();
        rd.submit();
        rd.sync();

        // read back the composed accumulator (height) AND the updated acc_w (base). BlendPair
        // callers ignore acc_w; the Fold path feeds it to the next step.
        let height_pba = rd.buffer_get_data(named[14]);
        let height = bytes_to_f32s(&height_pba.to_vec());
        let accw_pba = rd.buffer_get_data(named[9]);
        let accw = bytes_to_f32s(&accw_pba.to_vec());

        for (_, rid) in bindings.iter() { rd.free_rid(*rid); }
        rd.free_rid(pipeline);
        rd.free_rid(shader);
        rd.free();

        if height.len() != n {
            return Err(format!("compose readback: expected {n} f32, got {}", height.len()));
        }
        Ok((height, accw))
    }

    /// Run ONE compose fold step on the GPU and return (new_acc, new_acc_w). Thin wrapper over
    /// `run_compose_engine` (which reads back both the new acc=height and the GPU-updated
    /// acc_w=base via PASS_COMPOSE_ACCW_ADD). Used by `run_compose_inner` for the N>=2 case.
    #[allow(clippy::too_many_arguments)]
    fn run_compose_step(
        &self,
        rows: usize,
        cols: usize,
        favor_strength: f32,
        relief_confidence_floor: f32,
        acc: &[f32],
        acc_w: &[f32],
        f: &[f32],
        w: &[f32],
        use_favored: bool,
    ) -> Result<(Vec<f32>, Vec<f32>), String> {
        self.run_compose_engine(
            rows, cols, favor_strength, relief_confidence_floor, acc, acc_w, f, w, use_favored,
        )
    }

    /// COMPOSE fold (Slice-4b.11): GPU port of `biome_compose::compose_biomes`. Mirrors the oracle
    /// fold EXACTLY: n==1 returns fields[0]; use_favored = (!mode_is_field) && (n==2); running
    /// accumulator acc=fields[0], acc_w=weights[0], then for each subsequent (f,w):
    /// w_acc=acc_w/(acc_w+w+1e-12), acc=blend(acc,f,w_acc), acc_w+=w. The N>2 fold uses FIELD blend
    /// (use_favored is FALSE for n!=2), so each step is independent and can run as its own GPU
    /// engine call (the per-step acc/acc_w are carried on the CPU between calls).
    #[allow(clippy::too_many_arguments)]
    fn run_compose_inner(
        &self,
        fields: &[Vec<f32>],
        weights: &[Vec<f32>],
        rows: usize,
        cols: usize,
        mode_is_field: bool,
        favor_strength: f32,
        relief_confidence_floor: f32,
    ) -> Result<Vec<f32>, String> {
        let n = rows * cols;
        if fields.len() != weights.len() {
            return Err(format!("compose: fields/weights count mismatch {} vs {}", fields.len(), weights.len()));
        }
        if fields.is_empty() {
            return Err("compose requires at least one field".into());
        }
        if fields.len() == 1 {
            // n==1: return fields[0] unchanged (oracle short-circuit; no GPU needed).
            return Ok(fields[0].clone());
        }
        // use_favored EXACTLY for n==2 && height_favored mode (mirrors compose_biomes).
        let use_favored = (!mode_is_field) && (fields.len() == 2);

        let mut acc = fields[0].clone();
        let mut acc_w = weights[0].clone();
        for k in 1..fields.len() {
            let (new_acc, new_acc_w) = self.run_compose_step(
                rows, cols, favor_strength, relief_confidence_floor,
                &acc, &acc_w, &fields[k], &weights[k], use_favored,
            )?;
            acc = new_acc;
            acc_w = new_acc_w;
        }
        if acc.len() != n {
            return Err(format!("compose result len {} != {n}", acc.len()));
        }
        Ok(acc)
    }

    /// BLEND-PAIR (Slice-4b.11): GPU port of `biome_compose::blend_field` / `blend_height_favored`.
    /// A SINGLE blend with `w_a` used DIRECTLY (loaded into lowland=w_acc), NOT the accumulator
    /// fold. acc=a (height), f=b (pool0).
    #[allow(clippy::too_many_arguments)]
    fn run_blend_inner(
        &self,
        a: &[f32],
        b: &[f32],
        w_a: &[f32],
        rows: usize,
        cols: usize,
        mode_is_field: bool,
        favor_strength: f32,
        relief_confidence_floor: f32,
    ) -> Result<Vec<f32>, String> {
        // The engine loads height=acc0=a, pool0=f=b, and the BlendPair op uses lowland(=w_acc) as
        // the blend weight DIRECTLY -- so pre-load w_a into lowland. The engine writes height=acc0
        // and base=accw0; for BlendPair acc_w is irrelevant, so feed it zeros. We need w_a in
        // lowland, which the engine does NOT take as a param -> use a dedicated load via accw0 slot?
        // No: extend the engine call to also seed lowland. Simpler: the BlendPair op reads lowland,
        // so seed it through a thin wrapper that uploads w_a into binding 7. We thread w_a via the
        // engine's `accw0`? base is binding 9, not lowland. So run a SPECIALIZED engine path.
        self.run_blend_engine(
            rows, cols, favor_strength, relief_confidence_floor, a, b, w_a, !mode_is_field,
        )
    }

    /// BLEND-PAIR engine: like `run_compose_engine` but seeds lowland (w_acc) = w_a DIRECTLY and
    /// runs a single blend step (no acc_w fold). Kept separate so the compose Fold engine's buffer
    /// roles stay clean (Fold computes w_acc from acc_w; BlendPair uses w_a verbatim).
    #[allow(clippy::too_many_arguments)]
    fn run_blend_engine(
        &self,
        rows: usize,
        cols: usize,
        favor_strength: f32,
        relief_confidence_floor: f32,
        a: &[f32],   // -> height (acc)
        b: &[f32],   // -> pool0  (f)
        w_a: &[f32], // -> lowland (w_acc, used directly)
        use_favored: bool,
    ) -> Result<Vec<f32>, String> {
        let n = rows * cols;
        if a.len() != n || b.len() != n || w_a.len() != n {
            return Err(format!("blend engine: buffer len != rows*cols ({n})"));
        }
        let prim = self.primitives_src.as_deref().ok_or("no GLSL source loaded")?;
        let machine = self.machine_src.as_deref().ok_or("no GLSL source loaded")?;
        let fragment = self.compose_fragment.as_deref()
            .ok_or("no compose fragment loaded (call load_compose_fragment)")?;

        let mut rd: Gd<RenderingDevice> = RenderingServer::singleton()
            .create_local_rendering_device()
            .ok_or_else(|| "create_local_rendering_device returned null (headless / no device)".to_string())?;

        let machine_plus_fragment = format!("{machine}\n{fragment}");
        let glsl_stripped = crate::primitive_probe::concat_glsl_hoist_version(prim, &machine_plus_fragment);
        let mut src = RdShaderSource::new_gd();
        src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
        let spirv = match rd.shader_compile_spirv_from_source(&src) {
            Some(s) => s,
            None => { rd.free(); return Err("shader_compile_spirv_from_source returned null".to_string()); }
        };
        {
            let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
            if !err.is_empty() { rd.free(); return Err(format!("GLSL compile error: {err}")); }
        }
        let shader = rd.shader_create_from_spirv(&spirv);
        if shader.is_invalid() { rd.free(); return Err("shader_create_from_spirv returned invalid RID".into()); }

        let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };
        let field_bytes = n * 4;
        let zeros = vec![0.0_f32; n];
        let zeros_pba = PackedByteArray::from(f32s_to_bytes(&zeros).as_slice());
        let mk_zero = |rd: &mut Gd<RenderingDevice>| -> Rid {
            rd.storage_buffer_create_ex(bsize(field_bytes)).data(&zeros_pba).done()
        };
        let mk_data = |rd: &mut Gd<RenderingDevice>, data: &[f32]| -> Rid {
            let pba = PackedByteArray::from(f32s_to_bytes(data).as_slice());
            rd.storage_buffer_create_ex(bsize(field_bytes)).data(&pba).done()
        };

        // Named 0..18: 7=lowland(w_a, direct), 14=height(a). The rest inert.
        let mut named: Vec<Rid> = Vec::with_capacity(19);
        for b_i in 0..19usize {
            let rid = match b_i {
                7 => mk_data(&mut rd, w_a),
                14 => mk_data(&mut rd, a),
                _ => mk_zero(&mut rd),
            };
            named.push(rid);
        }

        let sigmas = compose_sigmas();
        let mut packed = vec![0.0_f32; sigmas.len() * KERNEL_STRIDE];
        for (slot, &sg) in sigmas.iter().enumerate() {
            let k = gaussian_kernel1d(sg, TRUNCATE);
            if k.len() > KERNEL_STRIDE {
                rd.free_rid(shader); rd.free();
                return Err(format!("compose kernel len {} (sigma {sg}) > KERNEL_STRIDE", k.len()));
            }
            packed[slot * KERNEL_STRIDE..slot * KERNEL_STRIDE + k.len()].copy_from_slice(&k);
        }
        let packed_pba = PackedByteArray::from(f32s_to_bytes(&packed).as_slice());
        let b_kernel = rd.storage_buffer_create_ex(bsize(packed.len() * 4)).data(&packed_pba).done();
        let kparams = KernelParams::from_sigmas(&sigmas);

        let b_flow_pre = mk_zero(&mut rd);
        let b_acc_a = mk_zero(&mut rd);
        let b_acc_b = mk_zero(&mut rd);
        let b_core = mk_zero(&mut rd);

        let mut b_pool: Vec<Rid> = Vec::with_capacity(POOL_SLOTS);
        for slot in 0..POOL_SLOTS {
            let rid = if slot == 0 { mk_data(&mut rd, b) } else { mk_zero(&mut rd) };
            b_pool.push(rid);
        }

        let vent_stride = crate::recipes_volcanic::volcanic::VENT_STRIDE;
        let maxv = crate::recipes_volcanic::volcanic::MAX_VENTS;
        let vent_zeros = vec![0.0_f32; maxv * vent_stride];
        let vent_pba = PackedByteArray::from(f32s_to_bytes(&vent_zeros).as_slice());
        let b_vents = rd.storage_buffer_create_ex(bsize(vent_zeros.len() * 4)).data(&vent_pba).done();

        let mut bindings: Vec<(i32, Rid)> = Vec::new();
        for (b_i, &rid) in named.iter().enumerate() { bindings.push((b_i as i32, rid)); }
        bindings.push((19, b_kernel));
        bindings.push((20, b_flow_pre));
        bindings.push((21, b_acc_a));
        bindings.push((22, b_acc_b));
        bindings.push((23, b_core));
        for (k, &rid) in b_pool.iter().enumerate() { bindings.push((24 + k as i32, rid)); }
        bindings.push((40, b_vents));
        let mut uniforms: Array<Gd<RdUniform>> = Array::new();
        for (bind, rid) in bindings.iter() { uniforms.push(&make_storage_uniform(*bind, *rid)); }
        // Scratch 1x1 image at binding 41 (machine declares out_img for PASS_CROP_IMG; blend never
        // dispatches it -> result byte-identical, but the set must satisfy binding 41).
        let scratch_img = make_scratch_image_1x1(&mut rd);
        uniforms.push(&make_image_uniform(41, scratch_img));
        bindings.push((41, scratch_img));
        let uset = rd.uniform_set_create(&uniforms, shader, 0);
        let pipeline = rd.compute_pipeline_create(shader);

        let wg_full_x = (cols as u32).div_ceil(16);
        let wg_full_y = (rows as u32).div_ceil(16);
        for &s in &sigmas { let _ = kparams.kp(s); }

        let cl = rd.compute_list_begin();
        rd.compute_list_bind_compute_pipeline(cl, pipeline);
        let mut sched = Scheduler {
            rd: &mut rd, cl, uset,
            rows: rows as i32, cols: cols as i32, apron: 0, seed: 0,
            spacing: 0.0, ox: 0.0, oz: 0.0, feature_span_m: 0.0, vent_count: 0,
            favor_strength, relief_confidence_floor, relief_m: 0.0,
            wg_full_x, wg_full_y, wg_core_x: wg_full_x, wg_core_y: wg_full_y, kparams,
            flow_iters: STABLE_ITERS, // blend never runs flow
            flow_on: true,            // blend never runs schedule_mountain; irrelevant but required
        };
        if use_favored { sched.blend_favored_step(); } else { sched.blend_field_step(); }

        rd.compute_list_end();
        rd.submit();
        rd.sync();

        let height_pba = rd.buffer_get_data(named[14]);
        let height = bytes_to_f32s(&height_pba.to_vec());

        for (_, rid) in bindings.iter() { rd.free_rid(*rid); }
        rd.free_rid(pipeline);
        rd.free_rid(shader);
        rd.free();

        if height.len() != n {
            return Err(format!("blend readback: expected {n} f32, got {}", height.len()));
        }
        Ok(height)
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
