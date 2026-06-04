//! Runtime WORLD composition dispatch.

use godot::classes::{RdUniform, RenderingDevice};
use godot::prelude::*;
use std::collections::BTreeMap;

use super::abi::{COMPOSE_RELIEF_SIGMA, PASS_CROP_IMG};
use super::helpers::{f32s_to_bytes, make_image_uniform, make_storage_uniform};
use super::runtime_context::BiomePageComputeContext;
use super::runtime_dispatch::{compute_biome_page_cached, compute_biome_page_core_cached};
use super::scheduler::Scheduler;

#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_biome_world_page_composed(
    rd: &mut Gd<RenderingDevice>,
    contexts: &BTreeMap<String, BiomePageComputeContext>,
    compose_ctx: &BiomePageComputeContext,
    target_rid: Rid,
    origin_x: f64,
    origin_z: f64,
    world_span: f64,
    page_px: i64,
    feature_span_m: f64,
    seed: i64,
    flow_on: bool,
    biome_names: &[String],
    weight_fields: &[Vec<f32>],
) -> Result<(), String> {
    if biome_names.is_empty() {
        return Err("compute_biome_world_page_composed: no active biomes".into());
    }
    if biome_names.len() != weight_fields.len() {
        return Err(format!(
            "compute_biome_world_page_composed: names/weights mismatch {} vs {}",
            biome_names.len(),
            weight_fields.len()
        ));
    }
    let page_n = (page_px as usize)
        .checked_mul(page_px as usize)
        .ok_or("compute_biome_world_page_composed: page_px overflow")?;
    for (i, weights) in weight_fields.iter().enumerate() {
        if weights.len() != page_n {
            return Err(format!(
                "compute_biome_world_page_composed: weight field {i} len {} != {page_n}",
                weights.len()
            ));
        }
    }
    if compose_ctx.core_px != page_px as usize || compose_ctx.apron_px != 0 {
        return Err(format!(
            "compute_biome_world_page_composed: compose context shape core={} apron={} != page_px={} apron=0",
            compose_ctx.core_px, compose_ctx.apron_px, page_px
        ));
    }

    if biome_names.len() == 1 {
        let ctx = contexts.get(&biome_names[0]).ok_or_else(|| {
            format!(
                "compute_biome_world_page_composed: no context for '{}'",
                biome_names[0]
            )
        })?;
        return compute_biome_page_cached(
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
        );
    }

    let core_bytes = u32::try_from(page_n * 4)
        .map_err(|_| "compute_biome_world_page_composed: core byte size overflows u32")?;
    let use_favored = biome_names.len() == 2;

    let first_ctx = contexts.get(&biome_names[0]).ok_or_else(|| {
        format!(
            "compute_biome_world_page_composed: no context for '{}'",
            biome_names[0]
        )
    })?;
    compute_biome_page_core_cached(
        rd,
        first_ctx,
        target_rid,
        origin_x,
        origin_z,
        world_span,
        page_px,
        feature_span_m,
        seed,
        flow_on,
    )?;
    copy_buffer(
        rd,
        first_ctx.bufs.core_rid(),
        compose_ctx.bufs.field_rid(14),
        core_bytes,
        "first recipe core -> compose height",
    )?;
    update_buffer_f32(
        rd,
        compose_ctx.bufs.field_rid(9),
        &weight_fields[0],
        "first weight -> compose acc_w",
    )?;

    for (biome, weights) in biome_names.iter().skip(1).zip(weight_fields.iter().skip(1)) {
        let ctx = contexts.get(biome).ok_or_else(|| {
            format!("compute_biome_world_page_composed: no context for '{biome}'")
        })?;
        compute_biome_page_core_cached(
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
        )?;
        copy_buffer(
            rd,
            ctx.bufs.core_rid(),
            compose_ctx.bufs.pool_rid(0),
            core_bytes,
            "recipe core -> compose pool0",
        )?;
        update_buffer_f32(
            rd,
            compose_ctx.bufs.pool_rid(1),
            weights,
            "recipe weight -> compose pool1",
        )?;
        dispatch_compose_step(rd, compose_ctx, target_rid, use_favored)?;
    }

    dispatch_compose_crop_to_image(rd, compose_ctx, target_rid)
}

fn copy_buffer(
    rd: &mut Gd<RenderingDevice>,
    src: Rid,
    dst: Rid,
    bytes: u32,
    label: &str,
) -> Result<(), String> {
    let err = rd.buffer_copy(src, dst, 0, 0, bytes);
    if err != godot::global::Error::OK {
        return Err(format!(
            "compute_biome_world_page_composed: buffer_copy {label} failed: {err:?}"
        ));
    }
    Ok(())
}

fn update_buffer_f32(
    rd: &mut Gd<RenderingDevice>,
    dst: Rid,
    values: &[f32],
    label: &str,
) -> Result<(), String> {
    let bytes = f32s_to_bytes(values);
    let pba = PackedByteArray::from(bytes.as_slice());
    let err = rd.buffer_update(dst, 0, bytes.len() as u32, &pba);
    if err != godot::global::Error::OK {
        return Err(format!(
            "compute_biome_world_page_composed: buffer_update {label} failed: {err:?}"
        ));
    }
    Ok(())
}

fn dispatch_compose_step(
    rd: &mut Gd<RenderingDevice>,
    compose_ctx: &BiomePageComputeContext,
    target_rid: Rid,
    use_favored: bool,
) -> Result<(), String> {
    let mut sched = begin_compose_scheduler(rd, compose_ctx, target_rid)?;
    sched.compose_wacc();
    if use_favored {
        sched.blend_favored_step();
    } else {
        sched.blend_field_step();
    }
    sched.compose_accw_add();
    end_compose_scheduler(sched);
    Ok(())
}

fn dispatch_compose_crop_to_image(
    rd: &mut Gd<RenderingDevice>,
    compose_ctx: &BiomePageComputeContext,
    target_rid: Rid,
) -> Result<(), String> {
    let wg = (compose_ctx.core_px as u32).div_ceil(16);
    let mut sched = begin_compose_scheduler(rd, compose_ctx, target_rid)?;
    sched.dispatch(PASS_CROP_IMG, 0, 0, 0, 0, 0.0, 0, wg, wg);
    end_compose_scheduler(sched);
    Ok(())
}

fn begin_compose_scheduler<'a>(
    rd: &'a mut Gd<RenderingDevice>,
    compose_ctx: &BiomePageComputeContext,
    target_rid: Rid,
) -> Result<Scheduler<'a>, String> {
    let bindings = compose_ctx.bufs.buffer_bindings();
    let mut uniforms: Array<Gd<RdUniform>> = Array::new();
    for (bind, rid) in bindings.iter() {
        uniforms.push(&make_storage_uniform(*bind, *rid));
    }
    uniforms.push(&make_image_uniform(41, target_rid));
    let uset = rd.uniform_set_create(&uniforms, compose_ctx.shader, 0);
    if uset.is_invalid() {
        return Err(
            "compute_biome_world_page_composed: compose uniform_set_create returned invalid RID"
                .into(),
        );
    }

    let rows = compose_ctx.apron_dim;
    let cols = compose_ctx.apron_dim;
    let wg_full_x = (cols as u32).div_ceil(16);
    let wg_full_y = (rows as u32).div_ceil(16);
    let kparams = compose_ctx.bufs.kparams.clone();
    let _ = kparams.kp(COMPOSE_RELIEF_SIGMA);

    let cl = rd.compute_list_begin();
    rd.compute_list_bind_compute_pipeline(cl, compose_ctx.pipeline);
    Ok(Scheduler {
        rd,
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
        favor_strength: 2.0,
        relief_confidence_floor: 1.0e-3,
        relief_m: compose_ctx.relief_m,
        wg_full_x,
        wg_full_y,
        wg_core_x: wg_full_x,
        wg_core_y: wg_full_y,
        kparams,
        flow_iters: 1,
        flow_on: true,
    })
}

fn end_compose_scheduler(sched: Scheduler<'_>) {
    let rd = sched.rd;
    let uset = sched.uset;
    rd.compute_list_end();
    rd.free_rid(uset);
}
