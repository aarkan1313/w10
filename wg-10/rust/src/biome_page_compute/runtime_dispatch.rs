//! Per-page biome dispatch for cached runtime contexts.

use godot::classes::{RdUniform, RenderingDevice};
use godot::prelude::*;

use super::abi::PASS_CROP_IMG;
use super::helpers::{f32s_to_bytes, make_image_uniform, make_storage_uniform};
use super::runtime_context::BiomePageComputeContext;
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
    dispatch_biome_page_cached(
        rd,
        ctx,
        target_rid,
        origin_x,
        origin_z,
        world_span,
        page_px,
        feature_span_m,
        seed,
        flow_on,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compute_biome_page_core_cached(
    rd: &mut Gd<RenderingDevice>,
    ctx: &BiomePageComputeContext,
    image_binding_rid: Rid,
    origin_x: f64,
    origin_z: f64,
    world_span: f64,
    page_px: i64,
    feature_span_m: f64,
    seed: i64,
    flow_on: bool,
) -> Result<(), String> {
    dispatch_biome_page_cached(
        rd,
        ctx,
        image_binding_rid,
        origin_x,
        origin_z,
        world_span,
        page_px,
        feature_span_m,
        seed,
        flow_on,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn dispatch_biome_page_cached(
    rd: &mut Gd<RenderingDevice>,
    ctx: &BiomePageComputeContext,
    image_binding_rid: Rid,
    origin_x: f64,
    origin_z: f64,
    world_span: f64,
    page_px: i64,
    feature_span_m: f64,
    seed: i64,
    flow_on: bool,
    write_image: bool,
) -> Result<(), String> {
    if page_px as usize != ctx.core_px {
        return Err(format!(
            "compute_biome_page_cached: page_px {page_px} != context core_px {} (rebuild the context)",
            ctx.core_px
        ));
    }
    if page_px < 2 {
        return Err(format!(
            "compute_biome_page_cached: page_px {page_px} must be >= 2"
        ));
    }
    if seed < i32::MIN as i64 || seed > i32::MAX as i64 {
        return Err(format!(
            "compute_biome_page_cached: seed {seed} outside i32 range (GPU hash is 32-bit-seed)"
        ));
    }

    let spacing_f64 = world_span / (page_px as f64 - 1.0);
    let spacing = spacing_f64 as f32;
    let ox = (origin_x - ctx.apron_px as f64 * spacing as f64) as f32;
    let oz = (origin_z - ctx.apron_px as f64 * spacing as f64) as f32;
    let kparams = runtime_kernel_params(rd, ctx, spacing_f64)?;

    let bindings = ctx.bufs.buffer_bindings();
    let mut uniforms: Array<Gd<RdUniform>> = Array::new();
    for (bind, rid) in bindings.iter() {
        uniforms.push(&make_storage_uniform(*bind, *rid));
    }
    uniforms.push(&make_image_uniform(41, image_binding_rid));
    let uset = rd.uniform_set_create(&uniforms, ctx.shader, 0);
    if uset.is_invalid() {
        return Err("compute_biome_page_cached: uniform_set_create returned invalid RID".into());
    }

    let rows = ctx.apron_dim;
    let cols = ctx.apron_dim;
    let core_rows = ctx.core_px;
    let core_cols = ctx.core_px;
    let wg_full_x = (cols as u32).div_ceil(16);
    let wg_full_y = (rows as u32).div_ceil(16);
    let wg_core_x = (core_cols as u32).div_ceil(16);
    let wg_core_y = (core_rows as u32).div_ceil(16);

    let sigmas = biome_sigmas(&ctx.biome).ok_or_else(|| {
        format!(
            "compute_biome_page_cached: no sigma list for '{}'",
            ctx.biome
        )
    })?;
    for &sg in sigmas.iter() {
        let _ = kparams.kp(sg);
    }

    let cl = rd.compute_list_begin();
    rd.compute_list_bind_compute_pipeline(cl, ctx.pipeline);
    let mut sched = Scheduler {
        rd,
        cl,
        uset,
        rows: rows as i32,
        cols: cols as i32,
        apron: ctx.apron_px as i32,
        seed: seed as i32,
        spacing,
        ox,
        oz,
        feature_span_m: feature_span_m as f32,
        vent_count: ctx.bufs.vent_count,
        favor_strength: 0.0,
        relief_confidence_floor: 0.0,
        relief_m: ctx.relief_m,
        wg_full_x,
        wg_full_y,
        wg_core_x,
        wg_core_y,
        kparams,
        flow_iters: ctx.flow_iters,
        flow_on,
    };
    dispatch_biome_schedule(&ctx.biome, &mut sched)?;
    if write_image {
        sched.dispatch(PASS_CROP_IMG, 0, 0, 0, 0, 0.0, 0, wg_core_x, wg_core_y);
    }

    rd.compute_list_end();
    rd.free_rid(uset);
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
        "anchored kparams must key by the same reference sigmas at the same koffsets"
    );
    Ok(kparams_anchored)
}

fn dispatch_biome_schedule(biome: &str, sched: &mut Scheduler<'_>) -> Result<(), String> {
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
