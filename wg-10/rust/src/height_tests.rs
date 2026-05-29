use crate::height;
use crate::pack::{self, FamilyKernel};
use std::path::Path;

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../worldgen_terrain/fixtures")
}
fn height_pack() -> pack::Pack {
    pack::load_pack_dir(&fixtures_dir(), "height_pack.json").expect("height pack loads")
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
