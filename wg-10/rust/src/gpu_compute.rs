//! WorldGen10 GPU compute — Wg10GpuCompute GodotClass.
//!
//! Strategy: all-Rust RenderingDevice dispatch.
//! - load_pack_dir(dir, pack_file, glsl_path) loads the pack + GLSL source.
//! - heights(xs, zs, seed)      -> PackedFloat64Array (one f64 per coord)
//! - signatures(xs, zs, seed)   -> PackedInt64Array   (one u64 as i64 per coord)
//!
//! Buffer layout mirrors the GLSL shader exactly (std430):
//!   bind 0  Coords      vec2 xz[]        f32 pairs  (x,z)
//!   bind 1  OutH        float h[]        f32
//!   bind 2  OutSig      uint sig[]       u32
//!   bind 3  Palettes    int fam[]        i32   palettes_flat[p*3+k]
//!   bind 4  CompatOff   ivec2 oc[]       i32 pairs (offset, count) — 8-byte stride
//!   bind 5  CompatFlat  int pal[]        i32 indices (-1 = unknown)
//!   bind 6  KRec        ivec4 rec[]      i32 quads (dataOffset,rows,cols,0) — 16-byte stride
//!   bind 7  KParam      vec2 rf[]        f32 pairs (relief_m, footprint_m) — 8-byte stride
//!   bind 8  KData       float v[]        f32 kernel data concatenated
//!
//! Push constant (std430, padded to 48 bytes):
//!   f32 region_size_m, i32 province_size_regions, u32 palette_primary_pct,
//!   u32 palette_compatible_pct, f32 moderation_min, f32 moderation_strength,
//!   i32 seed, i32 num_palettes, i32 num_coords, i32 _pad0, i32 _pad1, i32 _pad2
//!   (9 × 4 = 36 bytes; padded to 48 with 3 i32 trailing zeros)

use godot::prelude::*;
use godot::classes::{
    RenderingServer, RdShaderSource, RdUniform,
    rendering_device::{UniformType, ShaderStage},
};
use crate::pack;
use std::path::Path;

// ---------------------------------------------------------------------------
// Packed-buffer helpers (pure / testable)
// ---------------------------------------------------------------------------

/// All constant buffers built from a loaded Pack (pack-invariant, coord-independent).
pub struct PackBuffers {
    pub palettes_bytes:   Vec<u8>,
    pub compat_off_bytes: Vec<u8>,
    pub compat_flat_bytes: Vec<u8>,
    pub krec_bytes:       Vec<u8>,
    pub kparam_bytes:     Vec<u8>,
    pub kdata_bytes:      Vec<u8>,
    pub num_palettes:     i32,
}

/// Build the six static pack buffers from a loaded pack. Pure function, no Godot types.
pub fn build_pack_buffers(p: &pack::Pack) -> PackBuffers {
    let num_fam = p.family_ids.len();
    let num_pal = p.palettes.len();

    // ---- binding 3: Palettes flat (i32 per slot, p*3+k) ----
    // palettes_flat[p*3+k] = global family index of slot k in palette p.
    let mut palettes_bytes = Vec::with_capacity(num_pal * 3 * 4);
    for pal in &p.palettes {
        for fam_id in &pal.families {
            let idx = p.family_ids.iter().position(|f| f == fam_id)
                .expect("family in palette must be in family_ids");
            palettes_bytes.extend_from_slice(&(idx as i32).to_le_bytes());
        }
    }

    // ---- binding 4: CompatOff (ivec2 = i32 pair, 8 bytes/element) ----
    // ---- binding 5: CompatFlat (i32 per entry) ----
    // For each palette p: oc[p] = (offset_into_compat_flat, count).
    // compat_flat lists indices of compatible palettes.
    let mut compat_off_bytes  = Vec::with_capacity(num_pal * 8);
    let mut compat_flat_bytes = Vec::new();

    for pal in &p.palettes {
        let offset = (compat_flat_bytes.len() / 4) as i32;
        let compat_list = p.compatibility.get(&pal.id);
        let count = compat_list.map(|v| v.len()).unwrap_or(0) as i32;

        compat_off_bytes.extend_from_slice(&offset.to_le_bytes());
        compat_off_bytes.extend_from_slice(&count.to_le_bytes());

        if let Some(list) = compat_list {
            for cpal_id in list {
                let idx = p.palette_index(cpal_id).map(|i| i as i32).unwrap_or(-1);
                compat_flat_bytes.extend_from_slice(&idx.to_le_bytes());
            }
        }
    }
    // GPU dislikes zero-size buffers
    if compat_flat_bytes.is_empty() {
        compat_flat_bytes.extend_from_slice(&0i32.to_le_bytes());
    }

    // ---- binding 6: KRec (ivec4 per family = 4×i32 = 16 bytes) ----
    // rec[f] = (dataOffset, rows, cols, 0) in family_ids order.
    // ---- binding 7: KParam (vec2 per family = 2×f32 = 8 bytes) ----
    // ---- binding 8: KData (f32 per kernel element, concatenated in family_ids order) ----
    let mut krec_bytes   = Vec::with_capacity(num_fam * 16);
    let mut kparam_bytes = Vec::with_capacity(num_fam * 8);
    let mut kdata_bytes: Vec<u8> = Vec::new();

    let mut data_offset: i32 = 0;
    for fam_id in &p.family_ids {
        let fk = p.family_kernels.get(fam_id)
            .expect("every family_id must have kernel data in a pack loaded via load_pack_dir");
        let rows = fk.kernel.rows as i32;
        let cols = fk.kernel.cols as i32;
        let n = rows * cols;

        // ivec4: (dataOffset, rows, cols, 0)
        krec_bytes.extend_from_slice(&data_offset.to_le_bytes());
        krec_bytes.extend_from_slice(&rows.to_le_bytes());
        krec_bytes.extend_from_slice(&cols.to_le_bytes());
        krec_bytes.extend_from_slice(&0i32.to_le_bytes());

        // vec2: (relief_m, footprint_m)
        kparam_bytes.extend_from_slice(&(fk.relief_m as f32).to_le_bytes());
        kparam_bytes.extend_from_slice(&(fk.footprint_m as f32).to_le_bytes());

        // kernel data (row-major f32)
        for &v in &fk.kernel.data {
            kdata_bytes.extend_from_slice(&v.to_le_bytes());
        }

        data_offset += n;
    }

    // Guard against empty packs (shouldn't happen given pack validation)
    if krec_bytes.is_empty()   { krec_bytes.extend_from_slice(&[0u8; 16]); }
    if kparam_bytes.is_empty() { kparam_bytes.extend_from_slice(&[0u8; 8]); }
    if kdata_bytes.is_empty()  { kdata_bytes.extend_from_slice(&[0u8; 4]); }

    PackBuffers {
        palettes_bytes,
        compat_off_bytes,
        compat_flat_bytes,
        krec_bytes,
        kparam_bytes,
        kdata_bytes,
        num_palettes: num_pal as i32,
    }
}

/// Build push-constant bytes (std430, padded to 48 bytes).
///
/// Layout (9 × 4 = 36 + 12 pad = 48):
///   f32 region_size_m, i32 province_size_regions, u32 palette_primary_pct,
///   u32 palette_compatible_pct, f32 moderation_min, f32 moderation_strength,
///   i32 seed, i32 num_palettes, i32 num_coords, i32 _pad×3
pub fn build_push_constant(
    gc: &pack::GrammarConstants,
    seed: i32,
    num_palettes: i32,
    num_coords: i32,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(48);
    buf.extend_from_slice(&(gc.region_size_m as f32).to_le_bytes());
    buf.extend_from_slice(&(gc.province_size_regions as i32).to_le_bytes());
    buf.extend_from_slice(&gc.palette_primary_pct.to_le_bytes());
    buf.extend_from_slice(&gc.palette_compatible_pct.to_le_bytes());
    buf.extend_from_slice(&(gc.moderation_min as f32).to_le_bytes());
    buf.extend_from_slice(&(gc.moderation_strength as f32).to_le_bytes());
    buf.extend_from_slice(&seed.to_le_bytes());
    buf.extend_from_slice(&num_palettes.to_le_bytes());
    buf.extend_from_slice(&num_coords.to_le_bytes());
    // Pad to 48 bytes (3 trailing i32 = 12 bytes)
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    debug_assert_eq!(buf.len(), 48);
    buf
}

/// Create a `PackedByteArray` from a byte slice.
fn bytes_to_pba(bytes: &[u8]) -> PackedByteArray {
    PackedByteArray::from(bytes)
}

/// Make a storage-buffer RdUniform at the given binding.
fn make_storage_uniform(binding: i32, rid: Rid) -> Gd<RdUniform> {
    let mut u = RdUniform::new_gd();
    u.set_uniform_type(UniformType::STORAGE_BUFFER);
    u.set_binding(binding);
    u.add_id(rid);
    u
}

// ---------------------------------------------------------------------------
// Godot class
// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10GpuCompute {
    pack: Option<pack::Pack>,
    pack_buffers: Option<PackBuffers>,
    glsl_source: Option<String>,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10GpuCompute {
    fn init(base: Base<RefCounted>) -> Self {
        Self { pack: None, pack_buffers: None, glsl_source: None, base }
    }
}

#[godot_api]
impl Wg10GpuCompute {
    /// Load the terrain pack (resolving kernel .npy files relative to `dir`) and the
    /// GLSL compute shader source.  Returns "" on success, error string on failure.
    ///
    /// `dir`       — OS path to the pack directory
    ///               (GDScript: `ProjectSettings.globalize_path("res://...")`)
    /// `pack_file` — filename within `dir`, e.g. `"terrain_pack.json"`
    /// `glsl_path` — OS path to `height_field.glsl`
    ///               (GDScript: `ProjectSettings.globalize_path("res://shaders/height_field.glsl")`)
    #[func]
    pub fn load_pack_dir(&mut self, dir: GString, pack_file: GString, glsl_path: GString) -> GString {
        // Load pack
        match pack::load_pack_dir(Path::new(&dir.to_string()), &pack_file.to_string()) {
            Ok(p) => {
                let pb = build_pack_buffers(&p);
                self.pack_buffers = Some(pb);
                self.pack = Some(p);
            }
            Err(e) => {
                let msg = format!("pack: {e}");
                return GString::from(msg.as_str());
            }
        }
        // Load GLSL
        match std::fs::read_to_string(glsl_path.to_string()) {
            Ok(s) => { self.glsl_source = Some(s); }
            Err(e) => {
                let msg = format!("glsl: {e}");
                return GString::from(msg.as_str());
            }
        }
        GString::new()
    }

    /// Evaluate heights for `(xs[i], zs[i])` coordinates using the GPU shader.
    /// Returns one `f64` per coordinate.  Returns an empty array on error.
    #[func]
    pub fn heights(&self, xs: PackedFloat64Array, zs: PackedFloat64Array, seed: i64) -> PackedFloat64Array {
        let n = xs.len();
        if n == 0 { return PackedFloat64Array::new(); }
        if xs.len() != zs.len() {
            godot_error!("Wg10GpuCompute::heights: xs/zs length mismatch");
            return PackedFloat64Array::new();
        }
        match self.dispatch_inner(xs.as_slice(), zs.as_slice(), seed) {
            Ok((h_bytes, _sig_bytes)) => {
                let mut out = PackedFloat64Array::new();
                out.resize(n);
                let sl = out.as_mut_slice();
                for i in 0..n {
                    let f = f32::from_le_bytes(h_bytes[i*4..i*4+4].try_into().unwrap());
                    sl[i] = f as f64;
                }
                out
            }
            Err(e) => {
                godot_error!("Wg10GpuCompute::heights GPU error: {e}");
                PackedFloat64Array::new()
            }
        }
    }

    /// Evaluate family signatures for `(xs[i], zs[i])` using the GPU shader.
    /// Returns one `i64` (u32 → i64) per coordinate.
    #[func]
    pub fn signatures(&self, xs: PackedFloat64Array, zs: PackedFloat64Array, seed: i64) -> PackedInt64Array {
        let n = xs.len();
        if n == 0 { return PackedInt64Array::new(); }
        if xs.len() != zs.len() {
            godot_error!("Wg10GpuCompute::signatures: xs/zs length mismatch");
            return PackedInt64Array::new();
        }
        match self.dispatch_inner(xs.as_slice(), zs.as_slice(), seed) {
            Ok((_h_bytes, sig_bytes)) => {
                let mut out = PackedInt64Array::new();
                out.resize(n);
                let sl = out.as_mut_slice();
                for i in 0..n {
                    let u = u32::from_le_bytes(sig_bytes[i*4..i*4+4].try_into().unwrap());
                    sl[i] = u as i64;
                }
                out
            }
            Err(e) => {
                godot_error!("Wg10GpuCompute::signatures GPU error: {e}");
                PackedInt64Array::new()
            }
        }
    }

    /// Internal GPU dispatch — the single path for both heights and signatures.
    /// Returns `(h_bytes: Vec<u8>, sig_bytes: Vec<u8>)`, each `n*4` bytes (f32 / u32 LE).
    fn dispatch_inner(
        &self,
        xs: &[f64],
        zs: &[f64],
        seed: i64,
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
        let pack   = self.pack.as_ref().ok_or("no pack loaded")?;
        let pb     = self.pack_buffers.as_ref().ok_or("no pack buffers")?;
        let glsl   = self.glsl_source.as_deref().ok_or("no GLSL source loaded")?;

        let n = xs.len();
        let seed_i32 = seed as i32;

        // --- Create local RenderingDevice ---
        let mut rd = RenderingServer::singleton()
            .create_local_rendering_device()
            .ok_or_else(|| "create_local_rendering_device returned null (headless or Compatibility renderer)".to_string())?;

        // --- Compile shader from GLSL source ---
        // Strip any Godot .gdshader annotations (e.g. `#[compute]`) that are not
        // valid GLSL directives — the raw GLSL compiler rejects them.
        let glsl_stripped: String = glsl.lines()
            .filter(|l| !l.trim_start().starts_with("#["))
            .collect::<Vec<_>>()
            .join("\n");

        let mut src = RdShaderSource::new_gd();
        src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);

        let spirv = rd.shader_compile_spirv_from_source(&src)
            .ok_or_else(|| "shader_compile_spirv_from_source returned null".to_string())?;

        {
            let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
            if !err.is_empty() {
                let msg = format!("GLSL compile error: {err}");
                return Err(msg);
            }
        }

        let shader = rd.shader_create_from_spirv(&spirv);
        if shader.is_invalid() {
            return Err("shader_create_from_spirv returned invalid RID".into());
        }

        // --- Build coord buffer (binding 0): vec2 xz[] ---
        let mut coords_bytes = Vec::with_capacity(n * 8);
        for i in 0..n {
            coords_bytes.extend_from_slice(&(xs[i] as f32).to_le_bytes());
            coords_bytes.extend_from_slice(&(zs[i] as f32).to_le_bytes());
        }

        // --- Create storage buffers ---
        // Buffer SIZE args use try_from so an impossible-in-practice >u32 size is a
        // loud panic rather than a silent truncation.
        let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };

        let out_h_size   = bsize(n * 4);
        let out_sig_size = bsize(n * 4);

        let coords_pba     = bytes_to_pba(&coords_bytes);
        let palettes_pba   = bytes_to_pba(&pb.palettes_bytes);
        let compat_off_pba = bytes_to_pba(&pb.compat_off_bytes);
        let compat_flat_pba= bytes_to_pba(&pb.compat_flat_bytes);
        let krec_pba       = bytes_to_pba(&pb.krec_bytes);
        let kparam_pba     = bytes_to_pba(&pb.kparam_bytes);
        let kdata_pba      = bytes_to_pba(&pb.kdata_bytes);

        let coords_rid     = rd.storage_buffer_create_ex(bsize(coords_bytes.len())).data(&coords_pba).done();
        let out_h_rid      = rd.storage_buffer_create(out_h_size);
        let out_sig_rid    = rd.storage_buffer_create(out_sig_size);
        let palettes_rid   = rd.storage_buffer_create_ex(bsize(pb.palettes_bytes.len())).data(&palettes_pba).done();
        let compat_off_rid = rd.storage_buffer_create_ex(bsize(pb.compat_off_bytes.len())).data(&compat_off_pba).done();
        let compat_flat_rid= rd.storage_buffer_create_ex(bsize(pb.compat_flat_bytes.len())).data(&compat_flat_pba).done();
        let krec_rid       = rd.storage_buffer_create_ex(bsize(pb.krec_bytes.len())).data(&krec_pba).done();
        let kparam_rid     = rd.storage_buffer_create_ex(bsize(pb.kparam_bytes.len())).data(&kparam_pba).done();
        let kdata_rid      = rd.storage_buffer_create_ex(bsize(pb.kdata_bytes.len())).data(&kdata_pba).done();

        // --- Build uniform set ---
        let mut uniforms: Array<Gd<RdUniform>> = Array::new();
        let u0 = make_storage_uniform(0, coords_rid);
        let u1 = make_storage_uniform(1, out_h_rid);
        let u2 = make_storage_uniform(2, out_sig_rid);
        let u3 = make_storage_uniform(3, palettes_rid);
        let u4 = make_storage_uniform(4, compat_off_rid);
        let u5 = make_storage_uniform(5, compat_flat_rid);
        let u6 = make_storage_uniform(6, krec_rid);
        let u7 = make_storage_uniform(7, kparam_rid);
        let u8_ = make_storage_uniform(8, kdata_rid);
        uniforms.push(&u0);
        uniforms.push(&u1);
        uniforms.push(&u2);
        uniforms.push(&u3);
        uniforms.push(&u4);
        uniforms.push(&u5);
        uniforms.push(&u6);
        uniforms.push(&u7);
        uniforms.push(&u8_);

        let uset = rd.uniform_set_create(&uniforms, shader, 0);

        // --- Push constant ---
        let push_bytes = build_push_constant(
            &pack.grammar_constants,
            seed_i32,
            pb.num_palettes,
            n as i32,
        );
        let push_pba = bytes_to_pba(&push_bytes);

        // --- Pipeline + dispatch ---
        let pipeline = rd.compute_pipeline_create(shader);
        let workgroups = ((n as u32) + 63) / 64; // local_size_x = 64

        let cl = rd.compute_list_begin();
        rd.compute_list_bind_compute_pipeline(cl, pipeline);
        rd.compute_list_bind_uniform_set(cl, uset, 0);
        rd.compute_list_set_push_constant(cl, &push_pba, push_pba.len() as u32);
        rd.compute_list_dispatch(cl, workgroups, 1, 1);
        rd.compute_list_end();

        rd.submit();
        rd.sync();

        // --- Readback ---
        let h_pba   = rd.buffer_get_data(out_h_rid);
        let sig_pba = rd.buffer_get_data(out_sig_rid);

        // Free GPU resources
        rd.free_rid(coords_rid);
        rd.free_rid(out_h_rid);
        rd.free_rid(out_sig_rid);
        rd.free_rid(palettes_rid);
        rd.free_rid(compat_off_rid);
        rd.free_rid(compat_flat_rid);
        rd.free_rid(krec_rid);
        rd.free_rid(kparam_rid);
        rd.free_rid(kdata_rid);
        rd.free_rid(uset);
        rd.free_rid(pipeline);
        rd.free_rid(shader);

        let h_bytes  : Vec<u8> = h_pba.to_vec();
        let sig_bytes: Vec<u8> = sig_pba.to_vec();

        if h_bytes.len() != n * 4 {
            return Err(format!("h readback: expected {} bytes, got {}", n*4, h_bytes.len()));
        }
        if sig_bytes.len() != n * 4 {
            return Err(format!("sig readback: expected {} bytes, got {}", n*4, sig_bytes.len()));
        }

        Ok((h_bytes, sig_bytes))
    }
}

// ---------------------------------------------------------------------------
// Unit tests (pure buffer-builder; no Godot runtime needed)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::{Pack, Palette, FamilyKernel, GrammarConstants};
    use crate::npy::Kernel;
    use std::collections::BTreeMap;

    fn make_test_pack() -> Pack {
        // Minimal pack: 2 palettes, 4 families (each 2×2 kernel).
        let fam_ids = vec![
            "alpine".to_string(), "coastal".to_string(),
            "forest".to_string(),  "plains".to_string(),
        ];
        let mut family_kernels = BTreeMap::new();
        for id in &fam_ids {
            family_kernels.insert(id.clone(), FamilyKernel {
                kernel: Kernel { rows: 2, cols: 2, data: vec![1.0, 2.0, 3.0, 4.0] },
                relief_m: 100.0,
                footprint_m: 1000.0,
            });
        }
        let palettes = vec![
            Palette { id: "cold".to_string(),
                      families: vec!["alpine".to_string(), "forest".to_string(), "plains".to_string()] },
            Palette { id: "temperate".to_string(),
                      families: vec!["coastal".to_string(), "forest".to_string(), "plains".to_string()] },
        ];
        let mut compatibility = BTreeMap::new();
        compatibility.insert("cold".to_string(), vec!["temperate".to_string()]);

        Pack {
            grammar_constants: GrammarConstants {
                region_size_m: 500.0,
                province_size_regions: 8,
                palette_primary_pct: 60,
                palette_compatible_pct: 30,
                moderation_min: 0.4,
                moderation_strength: 0.5,
            },
            palettes,
            compatibility,
            family_ids: fam_ids,
            family_kernels,
        }
    }

    #[test]
    fn test_palettes_flat_length() {
        let pb = build_pack_buffers(&make_test_pack());
        // 2 palettes × 3 families × 4 bytes = 24 bytes
        assert_eq!(pb.palettes_bytes.len(), 2 * 3 * 4);
    }

    #[test]
    fn test_palettes_flat_values() {
        let pb = build_pack_buffers(&make_test_pack());
        // family_ids sorted: ["alpine"=0, "coastal"=1, "forest"=2, "plains"=3]
        // palette "cold":     alpine(0), forest(2), plains(3)
        // palette "temperate": coastal(1), forest(2), plains(3)
        let vals: Vec<i32> = pb.palettes_bytes.chunks(4)
            .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        assert_eq!(vals, vec![0, 2, 3,  1, 2, 3]);
    }

    #[test]
    fn test_krec_length() {
        let pb = build_pack_buffers(&make_test_pack());
        // 4 families × 16 bytes (ivec4) = 64 bytes
        assert_eq!(pb.krec_bytes.len(), 4 * 16);
    }

    #[test]
    fn test_kdata_length() {
        let pb = build_pack_buffers(&make_test_pack());
        // 4 families × 4 elements × 4 bytes = 64 bytes
        assert_eq!(pb.kdata_bytes.len(), 4 * 4 * 4);
    }

    #[test]
    fn test_kparam_length() {
        let pb = build_pack_buffers(&make_test_pack());
        // 4 families × 8 bytes (vec2) = 32 bytes
        assert_eq!(pb.kparam_bytes.len(), 4 * 8);
    }

    #[test]
    fn test_compat_off_length() {
        let pb = build_pack_buffers(&make_test_pack());
        // 2 palettes × 8 bytes (ivec2) = 16 bytes
        assert_eq!(pb.compat_off_bytes.len(), 2 * 8);
    }

    #[test]
    fn test_compat_off_values() {
        let pb = build_pack_buffers(&make_test_pack());
        // "cold"→(offset=0,count=1), "temperate"→(offset=1,count=0)
        let vals: Vec<i32> = pb.compat_off_bytes.chunks(4)
            .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        assert_eq!(vals, vec![0, 1,  1, 0]);
    }

    #[test]
    fn test_compat_flat_values() {
        let pb = build_pack_buffers(&make_test_pack());
        // compat for "cold" = ["temperate"] → palette index 1
        let vals: Vec<i32> = pb.compat_flat_bytes.chunks(4)
            .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        assert_eq!(vals, vec![1i32]);
    }

    #[test]
    fn test_push_constant_size() {
        let gc = GrammarConstants {
            region_size_m: 500.0,
            province_size_regions: 8,
            palette_primary_pct: 60,
            palette_compatible_pct: 30,
            moderation_min: 0.4,
            moderation_strength: 0.5,
        };
        let buf = build_push_constant(&gc, 42, 2, 100);
        assert_eq!(buf.len(), 48);
    }

    #[test]
    fn test_krec_data_offsets() {
        let pb = build_pack_buffers(&make_test_pack());
        // Each family has 2×2=4 floats, so dataOffset sequence = 0,4,8,12
        let offsets: Vec<i32> = pb.krec_bytes.chunks(16)
            .map(|c| i32::from_le_bytes(c[0..4].try_into().unwrap()))
            .collect();
        assert_eq!(offsets, vec![0, 4, 8, 12]);
    }

    #[test]
    fn test_num_palettes() {
        let pb = build_pack_buffers(&make_test_pack());
        assert_eq!(pb.num_palettes, 2);
    }
}
