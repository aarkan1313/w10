use crate::height;
use crate::pack::{self, FamilyKernel};
use std::path::Path;

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../worldgen_terrain/fixtures")
}
fn height_pack() -> pack::Pack {
    pack::load_pack_dir(&fixtures_dir(), "height_pack.json").expect("height pack loads")
}
fn flat_pack() -> pack::Pack {
    pack::load_pack_dir(&fixtures_dir(), "flat_pack.json").expect("flat pack loads")
}

#[test]
fn sample_flat_kernel_is_constant_times_relief() {
    let p = height_pack();
    let fa: &FamilyKernel = p.family_kernel("fa").unwrap(); // flat = 0.5, relief 1000
    for (x, z) in [(0.0, 0.0), (1234.5, -9876.0), (1.0e6, 1.0e6)] {
        let s = height::sample_kernel(fa, x, z);
        assert!((s - 500.0).abs() < 1e-4, "flat sample should be 500 @ ({x},{z}), got {s}");
    }
}

#[test]
fn sample_kernel_is_deterministic() {
    let p = height_pack();
    let ra = p.family_kernel("ra").unwrap();
    let a = height::sample_kernel(ra, 321.0, 654.0);
    let b = height::sample_kernel(ra, 321.0, 654.0);
    assert_eq!(a, b);
}

#[test]
fn sample_ramp_is_finite_and_within_relief() {
    let p = height_pack();
    let ra = p.family_kernel("ra").unwrap(); // ramp 0..1, relief 600
    for (x, z) in [(0.0, 0.0), (-2048.0, 2048.0), (5000.0, 12345.0)] {
        let s = height::sample_kernel(ra, x, z);
        assert!(s.is_finite());
        assert!(s >= -1e-6 && s <= 600.0 + 1e-4, "ramp sample out of range @ ({x},{z}): {s}");
    }
}

#[test]
fn sample_is_continuous_no_jump_over_tiny_step() {
    // Tests INTERIOR continuity (x=1000 is mid-tile for footprint 4096). The
    // footprint-wrap seam itself is C0 by construction (the wrap makes the last
    // texel's right-neighbour be texel 0) but NOT C1 — a ramp kernel creases at
    // every footprint repeat. That crease is expected: anti-repetition / kernel
    // variety is explicitly deferred (design §1). This test does not assert
    // seam-crossing smoothness because none is claimed there.
    let p = height_pack();
    let ra = p.family_kernel("ra").unwrap();
    let a = height::sample_kernel(ra, 1000.0, 50.0);
    let b = height::sample_kernel(ra, 1000.0 + 0.001, 50.0);
    assert!((a - b).abs() < 1e-1, "sample jumped over a tiny step: {a} vs {b}");
}

#[test]
fn height_exact_anchor_all_flat_pack() {
    // In flat_pack EVERY palette uses only the flat kernel (0.5, relief 1000).
    // So at ANY coordinate, regardless of which palette/families the grammar
    // rolls: every contribution = 0.5 * 1000 * moderation(slope=0) = 500, and the
    // weights sum to 1 => height == 500.0 EXACTLY. Roll-independent ground truth.
    let p = flat_pack();
    for (x, z) in [(0.0, 0.0), (-1024.5, 2048.25), (1.0e6, -1.0e6), (131072.0, 9000.0), (40000.0, -77.0)] {
        let h = height::height(x, z, 1337, &p);
        assert!((h - 500.0).abs() < 1e-6, "flat anchor must be exactly 500 @ ({x},{z}), got {h}");
    }
}

#[test]
fn height_is_deterministic() {
    let p = height_pack();
    let a = height::height(-1024.5, 2048.25, 1337, &p);
    let b = height::height(-1024.5, 2048.25, 1337, &p);
    assert_eq!(a, b);
}

#[test]
fn height_is_finite_and_bounded() {
    let p = height_pack();
    // max relief across families in the pack is 1000 (fa/fb/fc). A convex blend
    // of per-family contributions (each <= its relief) is bounded by max relief.
    for (x, z) in [(0.0, 0.0), (-1e5, 2e5), (1.0e6, -1.0e6), (40000.0, 9000.0)] {
        let h = height::height(x, z, 1337, &p);
        assert!(h.is_finite(), "height not finite @ ({x},{z})");
        assert!(h >= -1e-4 && h <= 1000.0 + 1e-3, "height out of bounds @ ({x},{z}): {h}");
    }
}

#[test]
fn height_continuous_across_region_and_province_seams() {
    let p = height_pack();
    let s = p.grammar_constants.region_size_m;
    let prov = p.grammar_constants.province_size_regions as f64 * s;
    for boundary in [s, prov] {
        let below = height::height(boundary - 0.01, 50.0, 1337, &p);
        let at = height::height(boundary, 50.0, 1337, &p);
        assert!(below.is_finite() && at.is_finite());
        // height is a C0 blend (C0 weights * C0 samples) -> a 0.01m step is a tiny change.
        assert!((below - at).abs() < 1.0,
            "height jumped across seam @ x={boundary}: below={below} at={at}");
    }
}

#[test]
fn moderation_clamps_to_range() {
    let mm = 0.4;
    let strength = 0.5;
    assert!((height::moderation(0.0, mm, strength) - 1.0).abs() < 1e-12);
    assert!((height::moderation(1000.0, mm, strength) - mm).abs() < 1e-12);
    let m = height::moderation(0.5, mm, strength);
    assert!(m > mm && m < 1.0);
}
