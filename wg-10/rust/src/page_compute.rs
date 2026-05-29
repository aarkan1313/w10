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
// compute_into_texture — free function, writes into a caller-owned texture RID
// ---------------------------------------------------------------------------

/// Compile and dispatch the page compute shader, writing terrain heights into
/// `target_rid` (an R32F texture already created by the caller / pool).
///
/// The caller owns `target_rid`; this function never frees it.
/// The shader, pipeline, and 6 pack-data storage buffers ARE freed before return.
///
/// `rd`         — mutable reference to the GLOBAL RenderingDevice singleton
/// `pack`       — loaded terrain pack (grammar constants + kernel data)
/// `pb`         — pre-built GPU byte buffers for the pack
/// `target_rid` — R32F STORAGE+SAMPLING texture RID provided by the caller
/// `glsl_source`— source text of `height_page.glsl` (caller reads the file)
/// `origin_x/z` — world-space top-left corner of the page (metres)
/// `world_span` — world-space size of the page in metres
/// `page_px`    — page resolution in pixels (width == height, multiple of 16)
/// `seed`       — grammar seed
pub(crate) fn compute_into_texture(
    rd: &mut Gd<RenderingDevice>,
    pack: &pack::Pack,
    pb: &PackBuffers,
    target_rid: Rid,
    glsl_source: &str,
    origin_x: f64,
    origin_z: f64,
    world_span: f64,
    page_px: i64,
    seed: i64,
) -> Result<(), String> {
    // --- Step 1: Strip Godot-specific annotations and compile the GLSL shader ---
    let glsl_stripped: String = glsl_source.lines()
        .filter(|l| !l.trim_start().starts_with("#["))
        .collect::<Vec<_>>()
        .join("\n");

    let mut src = RdShaderSource::new_gd();
    src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);

    let spirv = rd.shader_compile_spirv_from_source(&src)
        .ok_or_else(|| "compute_into_texture: shader_compile_spirv_from_source returned null".to_string())?;

    {
        let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
        if !err.is_empty() {
            return Err(format!("compute_into_texture: GLSL compile error: {err}"));
        }
    }

    let shader = rd.shader_create_from_spirv(&spirv);
    if shader.is_invalid() {
        return Err("compute_into_texture: shader_create_from_spirv returned invalid RID".to_string());
    }

    // --- Step 2: Build pack-data storage buffers ---
    let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };

    let palettes_pba    = bytes_to_pba(&pb.palettes_bytes);
    let compat_off_pba  = bytes_to_pba(&pb.compat_off_bytes);
    let compat_flat_pba = bytes_to_pba(&pb.compat_flat_bytes);
    let krec_pba        = bytes_to_pba(&pb.krec_bytes);
    let kparam_pba      = bytes_to_pba(&pb.kparam_bytes);
    let kdata_pba       = bytes_to_pba(&pb.kdata_bytes);

    let palettes_rid    = rd.storage_buffer_create_ex(bsize(pb.palettes_bytes.len())).data(&palettes_pba).done();
    let compat_off_rid  = rd.storage_buffer_create_ex(bsize(pb.compat_off_bytes.len())).data(&compat_off_pba).done();
    let compat_flat_rid = rd.storage_buffer_create_ex(bsize(pb.compat_flat_bytes.len())).data(&compat_flat_pba).done();
    let krec_rid        = rd.storage_buffer_create_ex(bsize(pb.krec_bytes.len())).data(&krec_pba).done();
    let kparam_rid      = rd.storage_buffer_create_ex(bsize(pb.kparam_bytes.len())).data(&kparam_pba).done();
    let kdata_rid       = rd.storage_buffer_create_ex(bsize(pb.kdata_bytes.len())).data(&kdata_pba).done();

    // --- Step 3: Build uniform set ---
    // binding 0 = R32F image (output — caller-owned target_rid)
    // bindings 1, 2 absent (dropped in page shader)
    // bindings 3–8 = pack data storage buffers
    let mut uniforms: Array<Gd<RdUniform>> = Array::new();
    uniforms.push(&make_image_uniform(0, target_rid));
    uniforms.push(&make_storage_uniform(3, palettes_rid));
    uniforms.push(&make_storage_uniform(4, compat_off_rid));
    uniforms.push(&make_storage_uniform(5, compat_flat_rid));
    uniforms.push(&make_storage_uniform(6, krec_rid));
    uniforms.push(&make_storage_uniform(7, kparam_rid));
    uniforms.push(&make_storage_uniform(8, kdata_rid));

    let uset = rd.uniform_set_create(&uniforms, shader, 0);

    // --- Step 4: Push constant ---
    let push_bytes = build_page_push_constant(
        &pack.grammar_constants,
        seed as i32,
        pb.num_palettes,
        origin_x as f32,
        origin_z as f32,
        world_span as f32,
        page_px as i32,
    );
    let push_pba = bytes_to_pba(&push_bytes);

    // --- Step 5: Pipeline + 2D dispatch ---
    let px = page_px as u32;
    let pipeline = rd.compute_pipeline_create(shader);
    let groups = (px + 15) / 16; // ceil(page_px / 16)

    let cl = rd.compute_list_begin();
    rd.compute_list_bind_compute_pipeline(cl, pipeline);
    rd.compute_list_bind_uniform_set(cl, uset, 0);
    rd.compute_list_set_push_constant(cl, &push_pba, push_pba.len() as u32);
    rd.compute_list_dispatch(cl, groups, groups, 1);
    rd.compute_list_end();
    // NOTE: global RenderingDevice — do NOT submit()/sync() (those are local-
    // device only; the engine auto-submits recorded compute work at the next
    // frame draw). The caller renders frames (which flushes this dispatch)
    // before sampling the texture.

    // --- Cleanup: free transient GPU resources; do NOT free target_rid ---
    // Uniform set is freed transitively when the shader RID is freed.
    // Free buffers, pipeline, then shader (which cascades uset).
    rd.free_rid(palettes_rid);
    rd.free_rid(compat_off_rid);
    rd.free_rid(compat_flat_rid);
    rd.free_rid(krec_rid);
    rd.free_rid(kparam_rid);
    rd.free_rid(kdata_rid);
    rd.free_rid(pipeline);
    rd.free_rid(shader);
    // target_rid intentionally NOT freed here — the caller (pool) owns it.

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
