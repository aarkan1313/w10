//! Byte, Godot uniform, and push-constant helpers for biome page compute.

use godot::classes::{
    RdTextureFormat, RdTextureView, RdUniform, RenderingDevice,
    rendering_device::{DataFormat, TextureUsageBits, UniformType},
};
use godot::prelude::*;

// ---------------------------------------------------------------------------
// byte helpers
// ---------------------------------------------------------------------------

pub(crate) fn f32s_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for &x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

pub(crate) fn bytes_to_f32s(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

/// f32 slice -> PackedFloat64Array (the GPU result widened back to f64 for the GDScript caller).
pub(crate) fn f32s_to_packed_f64(v: &[f32]) -> PackedFloat64Array {
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
pub(crate) fn biome_stem(path: &str) -> String {
    let file = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path);
    let stem = file.strip_suffix(".glsl").unwrap_or(file);
    stem.strip_prefix("biome_").unwrap_or(stem).to_string()
}

pub(crate) fn make_storage_uniform(binding: i32, rid: Rid) -> Gd<RdUniform> {
    let mut u = RdUniform::new_gd();
    u.set_uniform_type(UniformType::STORAGE_BUFFER);
    u.set_binding(binding);
    u.add_id(rid);
    u
}

/// Image-binding RdUniform (the RUNTIME producer binds the caller's R32F page texture at
/// binding 41 via this). Same shape as page_compute.rs::make_image_uniform (replicated here --
/// 6 lines -- rather than exposing that module-private helper cross-module).
pub(crate) fn make_image_uniform(binding: i32, rid: Rid) -> Gd<RdUniform> {
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
pub(crate) fn make_scratch_image_1x1(rd: &mut Gd<RenderingDevice>) -> Rid {
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
pub(crate) fn build_push(
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
