//! Cached runtime GPU context for biome page compute.

use godot::classes::{rendering_device::ShaderStage, RdShaderSource, RenderingDevice};
use godot::prelude::*;

use super::kernels::biome_apron_dim;
use super::runtime_buffers::{alloc_apron_buffers, alloc_compose_buffers, ApronBuffers};
use super::sigma_registry::biome_sigmas;

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
    pub(super) bufs: ApronBuffers,
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
        return Err(format!(
            "build_biome_page_context: no runtime schedule for biome '{biome}'"
        ));
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
    let spirv = rd.shader_compile_spirv_from_source(&src).ok_or_else(|| {
        "build_biome_page_context: shader_compile_spirv_from_source returned null".to_string()
    })?;
    {
        let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
        if !err.is_empty() {
            return Err(format!(
                "build_biome_page_context: GLSL compile error: {err}"
            ));
        }
    }
    let shader = rd.shader_create_from_spirv(&spirv);
    if shader.is_invalid() {
        return Err(
            "build_biome_page_context: shader_create_from_spirv returned invalid RID".into(),
        );
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

/// Build a cached runtime context for WORLD compose. The shader is the same generic machine plus
/// any biome fragment, because compose passes are handled inline before `biome_pass()` is called.
/// Buffers are sized to the core page (apron=0): recipe contexts produce cropped core buffers,
/// then the compose context folds those core fields and writes the final page image.
pub(crate) fn build_biome_compose_context(
    rd: &mut Gd<RenderingDevice>,
    primitives_src: &str,
    machine_src: &str,
    any_biome_fragment_src: &str,
    core_px: usize,
    relief_m: f32,
) -> Result<BiomePageComputeContext, String> {
    if core_px < 2 {
        return Err(format!(
            "build_biome_compose_context: core_px {core_px} must be >= 2"
        ));
    }

    let machine_plus_fragment = format!("{machine_src}\n{any_biome_fragment_src}");
    let glsl_stripped =
        crate::primitive_probe::concat_glsl_hoist_version(primitives_src, &machine_plus_fragment);
    let mut src = RdShaderSource::new_gd();
    src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
    let spirv = rd.shader_compile_spirv_from_source(&src).ok_or_else(|| {
        "build_biome_compose_context: shader_compile_spirv_from_source returned null".to_string()
    })?;
    {
        let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
        if !err.is_empty() {
            return Err(format!(
                "build_biome_compose_context: GLSL compile error: {err}"
            ));
        }
    }
    let shader = rd.shader_create_from_spirv(&spirv);
    if shader.is_invalid() {
        return Err(
            "build_biome_compose_context: shader_create_from_spirv returned invalid RID".into(),
        );
    }
    let pipeline = rd.compute_pipeline_create(shader);
    if pipeline.is_invalid() {
        rd.free_rid(shader);
        return Err(
            "build_biome_compose_context: compute_pipeline_create returned invalid RID".into(),
        );
    }

    let core_n = core_px * core_px;
    let bufs = match alloc_compose_buffers(rd, core_px, core_px, core_n) {
        Ok(b) => b,
        Err(e) => {
            rd.free_rid(pipeline);
            rd.free_rid(shader);
            return Err(format!("build_biome_compose_context: {e}"));
        }
    };

    Ok(BiomePageComputeContext {
        biome: "compose".to_string(),
        shader,
        pipeline,
        bufs,
        apron_dim: core_px,
        core_px,
        apron_px: 0,
        flow_iters: 1,
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
