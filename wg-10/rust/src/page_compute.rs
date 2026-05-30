//! WorldGen10 page compute — Wg10PageCompute GodotClass + `compute_into_texture` producer.
//!
//! `Wg10PageCompute` is a pack loader: call `load_pack_dir` once, then pass the
//! loaded pack + buffers to `compute_into_texture` for each dispatch.
//!
//! `compute_into_texture` writes terrain heights into a **caller-owned** R32F
//! texture RID on the GLOBAL RenderingDevice.  The texture-pool (Task 3) is the
//! single texture owner; this module never creates or frees texture RIDs.
//!
//! Key differences from Wg10GpuCompute (M2):
//!   1. Uses the GLOBAL rd (get_rendering_device), not create_local_rendering_device.
//!   2. Output is an R32F image2D (binding 0), not storage buffers.
//!   3. Bindings 1 and 2 are absent (no Coords / OutSig).
//!   4. No buffer_get_data / texture_get_data — pure fire-and-forget dispatch.

use godot::prelude::*;
use godot::classes::{
    RenderingDevice, RdShaderSource, RdUniform,
    rendering_device::{UniformType, ShaderStage},
};
use crate::pack;
use crate::gpu_compute::{build_pack_buffers, bytes_to_pba, make_storage_uniform, PackBuffers};
use std::path::Path;

// ---------------------------------------------------------------------------
// Push-constant builder — page-specific layout
// ---------------------------------------------------------------------------

/// Build the page push-constant bytes (std430).
///
/// Layout (13 × 4 = 52 bytes; padded to 64 — next multiple of 16):
///   f32  region_size_m
///   i32  province_size_regions
///   u32  palette_primary_pct
///   u32  palette_compatible_pct
///   f32  moderation_min
///   f32  moderation_strength
///   i32  seed
///   i32  num_palettes
///   i32  num_coords        (kept for layout stability; unused in page shader)
///   f32  origin_x
///   f32  origin_z
///   f32  world_span
///   i32  page_px
///   i32  _pad×3            (3 × 4 = 12 bytes padding to reach 64)
pub(crate) fn build_page_push_constant(
    gc: &pack::GrammarConstants,
    seed: i32,
    num_palettes: i32,
    origin_x: f32,
    origin_z: f32,
    world_span: f32,
    page_px: i32,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&(gc.region_size_m as f32).to_le_bytes());
    buf.extend_from_slice(&(gc.province_size_regions as i32).to_le_bytes());
    buf.extend_from_slice(&gc.palette_primary_pct.to_le_bytes());
    buf.extend_from_slice(&gc.palette_compatible_pct.to_le_bytes());
    buf.extend_from_slice(&(gc.moderation_min as f32).to_le_bytes());
    buf.extend_from_slice(&(gc.moderation_strength as f32).to_le_bytes());
    buf.extend_from_slice(&seed.to_le_bytes());
    buf.extend_from_slice(&num_palettes.to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes()); // num_coords — unused, kept for layout
    buf.extend_from_slice(&origin_x.to_le_bytes());
    buf.extend_from_slice(&origin_z.to_le_bytes());
    buf.extend_from_slice(&world_span.to_le_bytes());
    buf.extend_from_slice(&page_px.to_le_bytes());
    // Pad to 64 bytes (3 trailing i32 = 12 bytes)
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    debug_assert_eq!(buf.len(), 64, "page push constant must be 64 bytes");
    buf
}

/// Make an image-binding RdUniform at the given binding.
fn make_image_uniform(binding: i32, rid: Rid) -> Gd<RdUniform> {
    let mut u = RdUniform::new_gd();
    u.set_uniform_type(UniformType::IMAGE);
    u.set_binding(binding);
    u.add_id(rid);
    u
}

// ---------------------------------------------------------------------------
// PageComputeContext — per-page-INVARIANT GPU resources, built once, reused per page
// ---------------------------------------------------------------------------

/// The compute resources that are IDENTICAL for every page: the compiled shader, the compute
/// pipeline, and the six pack-data storage buffers (incl. the ~25 MB kernel atlas). Built ONCE
/// (at pool configure) and reused for every page dispatch — only the push constant (origin/span)
/// and the target image (binding 0) vary per page. Owned by `Wg10PagePool` (built at configure,
/// freed at free_all), so the pool stays the single owner of all its GPU RIDs.
///
/// Rebuilding these per page (the old `compute_into_texture`) was the 90 ms boundary-crossing
/// spike the M3 p99 gate caught: recompiling GLSL→SPIRV + re-uploading the atlas every page.
pub(crate) struct PageComputeContext {
    pub shader: Rid,
    pub pipeline: Rid,
    pub palettes: Rid,
    pub compat_off: Rid,
    pub compat_flat: Rid,
    pub krec: Rid,
    pub kparam: Rid,
    pub kdata: Rid,
}

/// Build the cached compute context ONCE: compile the shader, create the pipeline, upload the
/// six pack buffers. Returns Err on compile/create failure (the pool surfaces it from configure).
pub(crate) fn build_page_compute_context(
    rd: &mut Gd<RenderingDevice>,
    pb: &PackBuffers,
    glsl_source: &str,
) -> Result<PageComputeContext, String> {
    let glsl_stripped: String = glsl_source.lines()
        .filter(|l| !l.trim_start().starts_with("#["))
        .collect::<Vec<_>>()
        .join("\n");
    let mut src = RdShaderSource::new_gd();
    src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
    let spirv = rd.shader_compile_spirv_from_source(&src)
        .ok_or_else(|| "build_page_compute_context: shader_compile_spirv_from_source returned null".to_string())?;
    {
        let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
        if !err.is_empty() {
            return Err(format!("build_page_compute_context: GLSL compile error: {err}"));
        }
    }
    let shader = rd.shader_create_from_spirv(&spirv);
    if shader.is_invalid() {
        return Err("build_page_compute_context: shader_create_from_spirv returned invalid RID".to_string());
    }
    let pipeline = rd.compute_pipeline_create(shader);
    if pipeline.is_invalid() {
        rd.free_rid(shader);
        return Err("build_page_compute_context: compute_pipeline_create returned invalid RID".to_string());
    }

    let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };
    let palettes    = rd.storage_buffer_create_ex(bsize(pb.palettes_bytes.len())).data(&bytes_to_pba(&pb.palettes_bytes)).done();
    let compat_off  = rd.storage_buffer_create_ex(bsize(pb.compat_off_bytes.len())).data(&bytes_to_pba(&pb.compat_off_bytes)).done();
    let compat_flat = rd.storage_buffer_create_ex(bsize(pb.compat_flat_bytes.len())).data(&bytes_to_pba(&pb.compat_flat_bytes)).done();
    let krec        = rd.storage_buffer_create_ex(bsize(pb.krec_bytes.len())).data(&bytes_to_pba(&pb.krec_bytes)).done();
    let kparam      = rd.storage_buffer_create_ex(bsize(pb.kparam_bytes.len())).data(&bytes_to_pba(&pb.kparam_bytes)).done();
    let kdata       = rd.storage_buffer_create_ex(bsize(pb.kdata_bytes.len())).data(&bytes_to_pba(&pb.kdata_bytes)).done();

    Ok(PageComputeContext { shader, pipeline, palettes, compat_off, compat_flat, krec, kparam, kdata })
}

/// Free all cached compute RIDs. Called from the pool's free_all. Per-page uniform sets are
/// freed per page inside `compute_page_cached`, so only these persistent RIDs remain to free.
pub(crate) fn free_page_compute_context(rd: &mut Gd<RenderingDevice>, ctx: &PageComputeContext) {
    rd.free_rid(ctx.palettes);
    rd.free_rid(ctx.compat_off);
    rd.free_rid(ctx.compat_flat);
    rd.free_rid(ctx.krec);
    rd.free_rid(ctx.kparam);
    rd.free_rid(ctx.kdata);
    rd.free_rid(ctx.pipeline);
    rd.free_rid(ctx.shader);
}

/// Dispatch one page into `target_rid` using the CACHED context. Per-page work only: build the
/// uniform set (cached buffers + this page's image), set the push constant, dispatch
/// (fire-and-forget on the global RD — no submit/sync; the engine auto-submits at draw), then
/// free the per-page uniform set. No shader recompile, no buffer re-upload. `target_rid` is NOT
/// freed (the pool owns it).
pub(crate) fn compute_page_cached(
    rd: &mut Gd<RenderingDevice>,
    ctx: &PageComputeContext,
    gc: &pack::GrammarConstants,
    num_palettes: i32,
    target_rid: Rid,
    origin_x: f64,
    origin_z: f64,
    world_span: f64,
    page_px: i64,
    seed: i64,
) -> Result<(), String> {
    let mut uniforms: Array<Gd<RdUniform>> = Array::new();
    uniforms.push(&make_image_uniform(0, target_rid));
    uniforms.push(&make_storage_uniform(3, ctx.palettes));
    uniforms.push(&make_storage_uniform(4, ctx.compat_off));
    uniforms.push(&make_storage_uniform(5, ctx.compat_flat));
    uniforms.push(&make_storage_uniform(6, ctx.krec));
    uniforms.push(&make_storage_uniform(7, ctx.kparam));
    uniforms.push(&make_storage_uniform(8, ctx.kdata));
    let uset = rd.uniform_set_create(&uniforms, ctx.shader, 0);
    if uset.is_invalid() {
        return Err("compute_page_cached: uniform_set_create returned invalid RID".to_string());
    }

    let push_bytes = build_page_push_constant(
        gc,
        seed as i32,
        num_palettes,
        origin_x as f32,
        origin_z as f32,
        world_span as f32,
        page_px as i32,
    );
    let push_pba = bytes_to_pba(&push_bytes);

    let px = page_px as u32;
    let groups = (px + 15) / 16; // ceil(page_px / 16)
    let cl = rd.compute_list_begin();
    rd.compute_list_bind_compute_pipeline(cl, ctx.pipeline);
    rd.compute_list_bind_uniform_set(cl, uset, 0);
    rd.compute_list_set_push_constant(cl, &push_pba, push_pba.len() as u32);
    rd.compute_list_dispatch(cl, groups, groups, 1);
    rd.compute_list_end();

    // Free ONLY the per-page uniform set; the cached shader/pipeline/buffers persist.
    rd.free_rid(uset);
    Ok(())
}

// ---------------------------------------------------------------------------
// Godot class
// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10PageCompute {
    pack: Option<pack::Pack>,
    pack_buffers: Option<PackBuffers>,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10PageCompute {
    fn init(base: Base<RefCounted>) -> Self {
        Self {
            pack: None,
            pack_buffers: None,
            base,
        }
    }
}

#[godot_api]
impl Wg10PageCompute {
    /// Load the terrain pack (resolving kernel .npy files relative to `dir`).
    /// Returns "" on success, error string on failure.
    ///
    /// `dir`       — OS path to the pack directory
    /// `pack_file` — filename within `dir`, e.g. `"terrain_pack.json"`
    #[func]
    pub fn load_pack_dir(&mut self, dir: GString, pack_file: GString) -> GString {
        match pack::load_pack_dir(Path::new(&dir.to_string()), &pack_file.to_string()) {
            Ok(p) => {
                let pb = build_pack_buffers(&p);
                self.pack_buffers = Some(pb);
                self.pack = Some(p);
                GString::new()
            }
            Err(e) => GString::from(format!("pack: {e}").as_str()),
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests (push-constant builder; no Godot runtime needed)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::GrammarConstants;

    fn test_gc() -> GrammarConstants {
        GrammarConstants {
            region_size_m: 500.0,
            province_size_regions: 8,
            palette_primary_pct: 60,
            palette_compatible_pct: 30,
            moderation_min: 0.4,
            moderation_strength: 0.5,
        }
    }

    #[test]
    fn test_page_push_constant_size() {
        let buf = build_page_push_constant(&test_gc(), 42, 2, 0.0, 0.0, 1000.0, 256);
        assert_eq!(buf.len(), 64, "page push constant must be 64 bytes");
    }

    #[test]
    fn test_page_push_constant_fields() {
        let buf = build_page_push_constant(&test_gc(), 7, 3, -512.0f32, 1024.0f32, 2048.0f32, 512);
        // field 0: region_size_m = 500.0 f32
        let rsm = f32::from_le_bytes(buf[0..4].try_into().unwrap());
        assert!((rsm - 500.0f32).abs() < 1e-6, "region_size_m mismatch: {rsm}");
        // field 6: seed = 7 i32
        let s = i32::from_le_bytes(buf[24..28].try_into().unwrap());
        assert_eq!(s, 7, "seed mismatch");
        // field 9: origin_x = -512.0 f32 (offset 36 bytes = 9 * 4)
        let ox = f32::from_le_bytes(buf[36..40].try_into().unwrap());
        assert!((ox - (-512.0f32)).abs() < 1e-6, "origin_x mismatch: {ox}");
        // field 12: page_px = 512 i32 (offset 48)
        let ppx = i32::from_le_bytes(buf[48..52].try_into().unwrap());
        assert_eq!(ppx, 512, "page_px mismatch");
    }

    #[test]
    fn page_push_first_9_fields_match_m2() {
        // The page Params first 9 fields ARE the M2 Params (same shader formula needs
        // the same grammar constants in the same layout). Guard against drift.
        let gc = GrammarConstants {
            region_size_m: 32768.0, province_size_regions: 4,
            palette_primary_pct: 72, palette_compatible_pct: 22,
            moderation_min: 0.4, moderation_strength: 0.5,
        };
        let m2 = crate::gpu_compute::build_push_constant(&gc, 1337, 5, 0); // seed, num_palettes, num_coords
        let page = build_page_push_constant(&gc, 1337, 5, 0.0, 0.0, 0.0, 0); // + origin_x,z, span, page_px
        // M2 push is 48 bytes (9 fields*4 + 12 pad); page is 64. The first 36 bytes
        // (the 9 real fields) must match byte-for-byte.
        assert_eq!(&m2[..36], &page[..36], "page push first-9 fields diverged from M2 build_push_constant");
    }
}
