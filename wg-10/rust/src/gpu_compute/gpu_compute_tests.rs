//! Pure buffer-builder tests for gpu_compute.

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
