//! WorldGen10 page compute — Wg10PageCompute GodotClass.
//!
//! Writes one height page to an R32F Texture2DRD on the GLOBAL RenderingDevice.
//! No CPU readback — the texture stays on the GPU for the renderer to sample.
//!
//! Key differences from Wg10GpuCompute (M2):
//!   1. Uses the GLOBAL rd (get_rendering_device), not create_local_rendering_device.
//!   2. Output is an R32F image2D (binding 0), not storage buffers.
//!   3. Bindings 1 and 2 are absent (no Coords / OutSig).
//!   4. No buffer_get_data / texture_get_data — pure fire-and-forget dispatch.
//!
//! Lifetime: tex_rid is NOT freed when compute_page returns. It is stored on self
//! (alongside the wrapping Gd<Texture2Drd>) so Godot keeps it referenced.
//! For slice-1 (one page, never freed during the run) a leak at process-exit is
//! acceptable. A future free call should go through self.free_texture().

use godot::prelude::*;
use godot::classes::{
    RenderingServer, RdShaderSource, RdTextureFormat, RdTextureView, RdUniform, Texture2Drd,
    rendering_device::{DataFormat, TextureUsageBits, UniformType, ShaderStage},
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
fn build_page_push_constant(
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
// Godot class
// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10PageCompute {
    pack: Option<pack::Pack>,
    pack_buffers: Option<PackBuffers>,
    /// Raw texture RID kept alive on the global RD (NOT freed until self drops or
    /// free_texture() is called). The renderer holds a reference through tex_wrapper.
    tex_rid: Option<Rid>,
    /// Wrapping Texture2DRD — holds the above RID for Godot's scene-side use.
    /// Storing it here keeps the refcount alive as long as Wg10PageCompute lives.
    tex_wrapper: Option<Gd<Texture2Drd>>,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10PageCompute {
    fn init(base: Base<RefCounted>) -> Self {
        Self {
            pack: None,
            pack_buffers: None,
            tex_rid: None,
            tex_wrapper: None,
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

    /// Dispatch a compute pass that writes terrain heights into a fresh R32F texture.
    ///
    /// Returns the Texture2DRD on success (keep it alive; dropping it may release the
    /// underlying RID), or `None` on error (check Godot's error output).
    ///
    /// Parameters:
    ///   glsl_path  — OS path to `height_page.glsl`
    ///   origin_x   — world-space X of the page's top-left corner (metres)
    ///   origin_z   — world-space Z of the page's top-left corner (metres)
    ///   world_span — world-space size of the page in metres (width == height)
    ///   page_px    — page resolution in pixels (width == height, must be a multiple of 16)
    ///   seed       — grammar seed (cast to i32 internally)
    #[func]
    pub fn compute_page(
        &mut self,
        glsl_path: GString,
        origin_x: f64,
        origin_z: f64,
        world_span: f64,
        page_px: i64,
        seed: i64,
    ) -> Option<Gd<Texture2Drd>> {
        let pack = match self.pack.as_ref() {
            Some(p) => p,
            None => {
                godot_error!("Wg10PageCompute::compute_page: no pack loaded (call load_pack_dir first)");
                return None;
            }
        };
        let pb = match self.pack_buffers.as_ref() {
            Some(pb) => pb,
            None => {
                godot_error!("Wg10PageCompute::compute_page: no pack buffers (call load_pack_dir first)");
                return None;
            }
        };

        // --- Step 1: Get the GLOBAL RenderingDevice ---
        let mut rd = match RenderingServer::singleton().get_rendering_device() {
            Some(rd) => rd,
            None => {
                godot_error!("Wg10PageCompute::compute_page: get_rendering_device() returned null \
                              (requires Vulkan/Metal/DX12 renderer, not Compatibility or headless)");
                return None;
            }
        };

        // --- Step 2: Compile the GLSL compute shader ---
        let glsl = match std::fs::read_to_string(glsl_path.to_string()) {
            Ok(s) => s,
            Err(e) => {
                godot_error!("Wg10PageCompute::compute_page: cannot read GLSL file '{}': {}", glsl_path, e);
                return None;
            }
        };

        // Strip Godot-specific annotations (e.g. `#[compute]`) that are not valid GLSL.
        let glsl_stripped: String = glsl.lines()
            .filter(|l| !l.trim_start().starts_with("#["))
            .collect::<Vec<_>>()
            .join("\n");

        let mut src = RdShaderSource::new_gd();
        src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);

        let spirv = match rd.shader_compile_spirv_from_source(&src) {
            Some(s) => s,
            None => {
                godot_error!("Wg10PageCompute::compute_page: shader_compile_spirv_from_source returned null");
                return None;
            }
        };

        {
            let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
            if !err.is_empty() {
                godot_error!("Wg10PageCompute::compute_page: GLSL compile error: {}", err);
                return None;
            }
        }

        let shader = rd.shader_create_from_spirv(&spirv);
        if shader.is_invalid() {
            godot_error!("Wg10PageCompute::compute_page: shader_create_from_spirv returned invalid RID");
            return None;
        }

        // --- Step 3: Create the output R32F texture on the global RD ---
        let px = page_px as u32;
        let mut fmt = RdTextureFormat::new_gd();
        fmt.set_width(px);
        fmt.set_height(px);
        fmt.set_format(DataFormat::R32_SFLOAT);
        // STORAGE for compute imageStore; SAMPLING for the vertex shader to read it.
        // No CAN_UPDATE: the texture is GPU-written only, never CPU-uploaded.
        fmt.set_usage_bits(TextureUsageBits::STORAGE_BIT | TextureUsageBits::SAMPLING_BIT);
        let view = RdTextureView::new_gd();

        // texture_create(format, view) — data array defaults to empty in the _ex builder
        let tex_rid = rd.texture_create(&fmt, &view);
        if tex_rid.is_invalid() {
            godot_error!("Wg10PageCompute::compute_page: texture_create returned invalid RID");
            rd.free_rid(shader);
            return None;
        }

        // --- Step 4: Build pack-data storage buffers ---
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

        // --- Step 5: Build uniform set ---
        // binding 0 = R32F image (output)
        // bindings 1, 2 absent (dropped in page shader)
        // bindings 3–8 = pack data storage buffers
        let mut uniforms: Array<Gd<RdUniform>> = Array::new();
        uniforms.push(&make_image_uniform(0, tex_rid));
        uniforms.push(&make_storage_uniform(3, palettes_rid));
        uniforms.push(&make_storage_uniform(4, compat_off_rid));
        uniforms.push(&make_storage_uniform(5, compat_flat_rid));
        uniforms.push(&make_storage_uniform(6, krec_rid));
        uniforms.push(&make_storage_uniform(7, kparam_rid));
        uniforms.push(&make_storage_uniform(8, kdata_rid));

        let uset = rd.uniform_set_create(&uniforms, shader, 0);

        // --- Step 6: Push constant ---
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

        // --- Step 7: Pipeline + 2D dispatch ---
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

        // --- Cleanup: free transient GPU resources; do NOT free tex_rid ---
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
        // tex_rid intentionally NOT freed here — the renderer needs it.

        // --- Step 9: Wrap the texture and store for lifetime management ---
        let mut t = Texture2Drd::new_gd();
        t.set_texture_rd_rid(tex_rid);

        // Store both the raw RID and the wrapper so they live as long as this object.
        // LIFETIME: tex_rid is valid as long as we hold it on self and don't free it.
        // The caller should hold on to the returned Gd<Texture2Drd> (or store it on
        // a material/mesh); dropping it only decrements the Godot refcount of the
        // Texture2DRD wrapper — the underlying tex_rid remains alive via self.tex_rid.
        self.tex_rid = Some(tex_rid);
        self.tex_wrapper = Some(t.clone());

        Some(t)
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
