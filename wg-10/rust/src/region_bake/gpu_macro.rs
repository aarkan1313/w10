//! GPU region-macro readback: run the proven seam-safe macro (compute_biome_page_cached) over a
//! whole region+apron grid, read it back, return the region-core RAW as Vec<f64>.
//! OFF-FRAME / worker only (deliberate GPU->CPU stall). Bare local RD, no scene/viewport.
#![allow(dead_code)]
use godot::classes::rendering_device::{DataFormat, TextureUsageBits};
use godot::classes::{RdTextureFormat, RdTextureView, RenderingDevice, RenderingServer};
use godot::prelude::*;

// These are re-exported at the biome_page_compute module root, NOT at their private submodule paths.
use crate::biome_page_compute::{
    build_biome_page_context, bytes_to_f32s, compute_biome_page_cached, free_biome_page_context,
};

/// Returns the region-core RAW macro field (core_px*core_px, row-major f64).
/// `core_px` is the region grid side; `apron_px` the seam apron. `region_origin_*` is the PADDED
/// grid origin (top-left of the apron-padded region); the core origin is offset inward by the apron.
#[allow(clippy::too_many_arguments)]
pub fn gpu_macro_region(
    primitives_src: &str,
    machine_src: &str,
    mountain_fragment_src: &str,
    region_origin_x: f64,
    region_origin_z: f64,
    spacing_m: f64,
    core_px: usize,
    apron_px: usize,
    flow_iters: usize,
    feature_span_m: f64,
    seed: i64,
    flow_on: bool,
) -> Result<Vec<f64>, String> {
    let mut rd: Gd<RenderingDevice> = RenderingServer::singleton()
        .create_local_rendering_device()
        .ok_or("gpu_macro_region: create_local_rendering_device returned null")?;

    let ctx = match build_biome_page_context(
        &mut rd,
        primitives_src,
        machine_src,
        mountain_fragment_src,
        core_px,
        apron_px,
        flow_iters.max(1),
        1.0,
    ) {
        Ok(c) => c,
        Err(e) => {
            rd.free();
            return Err(format!("gpu_macro_region: {e}"));
        }
    };

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
        free_biome_page_context(&mut rd, &ctx);
        rd.free();
        return Err("gpu_macro_region: texture_create invalid".into());
    }

    let page_px = core_px as i64;
    let world_span = spacing_m * (page_px as f64 - 1.0);
    let origin_x = region_origin_x + apron_px as f64 * spacing_m;
    let origin_z = region_origin_z + apron_px as f64 * spacing_m;

    if let Err(e) = compute_biome_page_cached(
        &mut rd,
        &ctx,
        tex,
        origin_x,
        origin_z,
        world_span,
        page_px,
        feature_span_m,
        seed,
        flow_on,
    ) {
        rd.free_rid(tex);
        free_biome_page_context(&mut rd, &ctx);
        rd.free();
        return Err(format!("gpu_macro_region: {e}"));
    }

    let raw = rd.texture_get_data(tex, 0);
    let core = bytes_to_f32s(&raw.to_vec());
    rd.free_rid(tex);
    free_biome_page_context(&mut rd, &ctx);
    rd.free();

    if core.len() != core_px * core_px {
        return Err(format!(
            "gpu_macro_region: readback {} != {}",
            core.len(),
            core_px * core_px
        ));
    }
    Ok(core.iter().map(|&v| v as f64).collect())
}
