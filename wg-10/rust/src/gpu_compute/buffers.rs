//! Shared GPU pack-buffer, push-constant, and uniform helpers.

use godot::classes::{rendering_device::UniformType, RdUniform};
use godot::prelude::*;

use crate::pack;

// ---------------------------------------------------------------------------
// Packed-buffer helpers (pure / testable)
// ---------------------------------------------------------------------------

/// All constant buffers built from a loaded Pack (pack-invariant, coord-independent).
pub(crate) struct PackBuffers {
    pub(crate) palettes_bytes:   Vec<u8>,
    pub(crate) compat_off_bytes: Vec<u8>,
    pub(crate) compat_flat_bytes: Vec<u8>,
    pub(crate) krec_bytes:       Vec<u8>,
    pub(crate) kparam_bytes:     Vec<u8>,
    pub(crate) kdata_bytes:      Vec<u8>,
    pub(crate) num_palettes:     i32,
}

/// Build the six static pack buffers from a loaded pack. Pure function, no Godot types.
pub(crate) fn build_pack_buffers(p: &pack::Pack) -> PackBuffers {
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
pub(crate) fn bytes_to_pba(bytes: &[u8]) -> PackedByteArray {
    PackedByteArray::from(bytes)
}

/// Make a storage-buffer RdUniform at the given binding.
pub(crate) fn make_storage_uniform(binding: i32, rid: Rid) -> Gd<RdUniform> {
    let mut u = RdUniform::new_gd();
    u.set_uniform_type(UniformType::STORAGE_BUFFER);
    u.set_binding(binding);
    u.add_id(rid);
    u
}
