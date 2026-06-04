//! Cached runtime GPU context for biome page compute.

use godot::classes::{
    rendering_device::ShaderStage, RdShaderSource, RdUniform, RenderingDevice,
};
use godot::prelude::*;

use super::abi::PASS_CROP_IMG;
use super::helpers::{f32s_to_bytes, make_image_uniform, make_storage_uniform};
use super::kernels::biome_apron_dim;
use super::runtime_buffers::{alloc_apron_buffers, ApronBuffers};
use super::schedule_coast::schedule_coast;
use super::schedule_desert::schedule_desert;
use super::schedule_glacial::schedule_glacial;
use super::schedule_grassland::schedule_grassland;
use super::schedule_karst::schedule_karst;
use super::schedule_mountain::schedule_mountain;
use super::schedule_rainforest::schedule_rainforest;
use super::schedule_temperate::schedule_temperate;
use super::schedule_tundra::schedule_tundra;
use super::schedule_volcanic::schedule_volcanic;
use super::schedule_wetland::schedule_wetland;
use super::scheduler::Scheduler;
use super::sigma_registry::{biome_sigmas, mountain_kernels_anchored, KernelParams};

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

/// The per-page-INVARIANT GPU resources for the RUNTIME mountain page producer: the compiled
/// shader, the compute pipeline, and the full apron buffer set (`ApronBuffers`). Built ONCE
/// (`build_biome_page_context`) on the GLOBAL rd and reused for every page; only the per-page
/// uniform set (cached buffers + this page's image) + push constant vary. Mirrors
/// page_compute.rs::PageComputeContext. Owns every RID -> `free_biome_page_context` frees them all.
///
/// `apron_dim` is the padded working-grid dim (core + 2*apron); `core_px` the core dim;
/// `apron_px` the apron each side; `flow_iters` the PULL-relaxation step count (STABLE_ITERS for
/// the parity-proven path). Volcanic contexts also own the CPU-packed vent buffer generated from
/// the configured runtime seed and feature span.
pub(crate) struct BiomePageComputeContext {
    pub biome: String,
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
    build_biome_page_context_for_biome(
        rd,
        primitives_src,
        machine_src,
        mountain_fragment_src,
        "mountain",
        core_px,
        apron_px,
        flow_iters,
        relief_m,
        0,
        0.0,
    )
}

/// Build one cached runtime context for a named biome schedule. `build_biome_page_context` keeps
/// the proven mountain call shape; this variant is used by the world-routed producer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_biome_page_context_for_biome(
    rd: &mut Gd<RenderingDevice>,
    primitives_src: &str,
    machine_src: &str,
    biome_fragment_src: &str,
    biome: &str,
    core_px: usize,
    apron_px: usize,
    flow_iters: usize,
    relief_m: f32,
    seed: i64,
    feature_span_m: f64,
) -> Result<BiomePageComputeContext, String> {
    if biome_sigmas(biome).is_none() {
        return Err(format!("build_biome_page_context: no runtime schedule for biome '{biome}'"));
    }
    if seed < i32::MIN as i64 || seed > i32::MAX as i64 {
        return Err(format!(
            "build_biome_page_context: seed {seed} outside i32 range (GPU hash is 32-bit-seed)"
        ));
    }
    let apron_dim = biome_apron_dim(core_px, apron_px);
    if apron_dim <= 2 * apron_px {
        return Err(format!(
            "build_biome_page_context: apron {apron_px} too large for core {core_px}"
        ));
    }
    // concat primitives + (machine + "\n" + fragment), hoisting the machine's #version to line 1 --
    // byte-identical to run_inner's compile path.
    let machine_plus_fragment = format!("{machine_src}\n{biome_fragment_src}");
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
    let bufs = match alloc_apron_buffers(
        rd,
        apron_dim,
        apron_dim,
        core_n,
        biome,
        seed as i32,
        feature_span_m as f32,
    ) {
        Ok(b) => b,
        Err(e) => {
            rd.free_rid(pipeline);
            rd.free_rid(shader);
            return Err(format!("build_biome_page_context: {e}"));
        }
    };

    Ok(BiomePageComputeContext {
        biome: biome.to_string(),
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

    let kparams = runtime_kernel_params(rd, ctx, spacing_f64)?;

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
    // open list would leak). Mountain uses spacing-anchored params keyed by the reference sigmas;
    // other schedules use their context's prebuilt sigma table.
    let sigmas = biome_sigmas(&ctx.biome)
        .ok_or_else(|| format!("compute_biome_page_cached: no sigma list for '{}'", ctx.biome))?;
    for &sg in sigmas.iter() {
        let _ = kparams.kp(sg);
    }

    let cl = rd.compute_list_begin();
    rd.compute_list_bind_compute_pipeline(cl, ctx.pipeline);

    // Scheduler over the cached buffers + open list.
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
        vent_count: ctx.bufs.vent_count,
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
    // Run the selected proven single-biome schedule (ends with PASS_CROP into the core storage
    // buffer -- inert here, we don't read it), then crop to the IMAGE.
    dispatch_biome_schedule(&ctx.biome, &mut sched)?;
    sched.dispatch(PASS_CROP_IMG, 0, 0, 0, 0, 0.0, 0, wg_core_x, wg_core_y);

    rd.compute_list_end();
    // RUNTIME (global RD): fire-and-forget - do NOT submit()/sync() here. This producer runs on the
    // MAIN RenderingDevice (the one the renderer owns), where manual submit/sync is ILLEGAL
    // ("Only local devices can submit and sync" - rendering_device.cpp:6551). The engine auto-submits
    // the global RD's queued work at draw, exactly like the legacy `compute_page_cached`
    // (page_compute.rs:166: "no submit/sync; the engine auto-submits at draw"). Intra-schedule
    // ordering is enforced by the `compute_list_add_barrier` calls RECORDED INTO the list (Scheduler),
    // which are honored at submission regardless of who submits. (The readback test entries use a
    // LOCAL rd via create_local_rendering_device, where submit/sync IS legal - those keep theirs.)
    rd.free_rid(uset); // free ONLY the per-page uniform set; cached resources persist
    Ok(())
}

fn runtime_kernel_params(
    rd: &mut Gd<RenderingDevice>,
    ctx: &BiomePageComputeContext,
    spacing_f64: f64,
) -> Result<KernelParams, String> {
    if ctx.biome != "mountain" {
        return Ok(ctx.bufs.kparams.clone());
    }

    // SCALE-INVARIANCE: rebuild the WORLD-anchored gaussian kernels for THIS mountain dispatch's
    // spacing and re-fill the cached kernel buffer (binding 19) in place.
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
    Ok(kparams_anchored)
}

pub(crate) fn dispatch_biome_schedule(
    biome: &str,
    sched: &mut Scheduler<'_>,
) -> Result<(), String> {
    match biome {
        "mountain" => schedule_mountain(sched),
        "grassland" => schedule_grassland(sched),
        "desert" => schedule_desert(sched),
        "coast" => schedule_coast(sched),
        "wetland" => schedule_wetland(sched),
        "tundra" => schedule_tundra(sched),
        "glacial" => schedule_glacial(sched),
        "karst" => schedule_karst(sched),
        "temperate" => schedule_temperate(sched),
        "rainforest" => schedule_rainforest(sched),
        "volcanic" => schedule_volcanic(sched),
        other => return Err(format!("no schedule for biome '{other}'")),
    }
    Ok(())
}
