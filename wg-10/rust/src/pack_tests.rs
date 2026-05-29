use crate::pack;

const GOLDEN: &str = include_str!("../../worldgen_terrain/fixtures/golden_pack.json");
const BAD_ARITY: &str = include_str!("../../worldgen_terrain/fixtures/bad_pack_arity.json");

#[test]
fn loads_golden_pack() {
    let p = pack::load_pack_str(GOLDEN).expect("golden pack should load");
    assert_eq!(p.palettes.len(), 4);
    assert_eq!(p.grammar_constants.province_size_regions, 4);
    assert_eq!(p.grammar_constants.region_size_m, 32768.0);
    // every palette has exactly FAMILIES_PER_PALETTE families
    for pal in &p.palettes {
        assert_eq!(pal.families.len(), pack::FAMILIES_PER_PALETTE);
    }
}

#[test]
fn rejects_wrong_family_arity() {
    let err = pack::load_pack_str(BAD_ARITY).expect_err("must reject !=3 families");
    assert!(err.contains("families"), "error should mention families: {err}");
}

#[test]
fn rejects_bad_schema() {
    let bad = r#"{"schema":"nope","version":1,"grammar_constants":{"region_size_m":1.0,"province_size_regions":4,"palette_primary_pct":72,"palette_compatible_pct":22},"palettes":[],"compatibility":{},"families":{}}"#;
    let err = pack::load_pack_str(bad).expect_err("must reject bad schema");
    assert!(err.contains("schema"), "error should mention schema: {err}");
}

#[test]
fn rejects_palette_referencing_unknown_family() {
    // palette references "ghost" which is not in families{}
    let bad = r#"{"schema":"worldgen10.terrain_pack.v1","version":1,"grammar_constants":{"region_size_m":1.0,"province_size_regions":4,"palette_primary_pct":72,"palette_compatible_pct":22},"palettes":[{"id":"p","families":["mountain","ghost","karst"]}],"compatibility":{"p":[]},"families":{"mountain":{},"karst":{}}}"#;
    let err = pack::load_pack_str(bad).expect_err("must reject unknown family");
    assert!(err.contains("ghost") || err.contains("unknown family"), "error should name the unknown family: {err}");
}

#[test]
fn rejects_pct_out_of_range() {
    // primary 80 + compatible 30 = 110 > 100
    let bad = r#"{"schema":"worldgen10.terrain_pack.v1","version":1,"grammar_constants":{"region_size_m":1.0,"province_size_regions":4,"palette_primary_pct":80,"palette_compatible_pct":30},"palettes":[],"compatibility":{},"families":{}}"#;
    let err = pack::load_pack_str(bad).expect_err("must reject pct sum > 100");
    assert!(err.contains("pct") || err.contains("100"), "error should mention pct range: {err}");
}

#[test]
fn rejects_empty_palettes() {
    let bad = r#"{"schema":"worldgen10.terrain_pack.v1","version":1,"grammar_constants":{"region_size_m":1.0,"province_size_regions":4,"palette_primary_pct":72,"palette_compatible_pct":22},"palettes":[],"compatibility":{},"families":{}}"#;
    let err = pack::load_pack_str(bad).expect_err("must reject empty palettes");
    assert!(err.contains("palette"), "error should mention palettes: {err}");
}

#[test]
fn rejects_pct_sum_at_overflow_boundary() {
    // primary u32::MAX + compatible 2 would wrap to 1 in release with naive u32
    // addition; the u64-widened guard must still reject it.
    let bad = r#"{"schema":"worldgen10.terrain_pack.v1","version":1,"grammar_constants":{"region_size_m":1.0,"province_size_regions":4,"palette_primary_pct":4294967295,"palette_compatible_pct":2},"palettes":[],"compatibility":{},"families":{}}"#;
    let err = pack::load_pack_str(bad).expect_err("must reject overflow-prone pct sum");
    assert!(err.contains("pct") || err.contains("100"), "error should mention pct range: {err}");
}

use std::path::Path;

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../worldgen_terrain/fixtures")
}

#[test]
fn load_pack_dir_loads_kernels() {
    let dir = fixtures_dir();
    let p = pack::load_pack_dir(&dir, "height_pack.json").expect("height pack loads");
    // flat + ramp kernels referenced by 5 families -> 2 distinct kernels resolved.
    let fa = p.family_kernel("fa").expect("fa has a kernel");
    assert_eq!((fa.kernel.rows, fa.kernel.cols), (4, 4));
    assert!((fa.relief_m - 1000.0).abs() < 1e-9);
    assert!((fa.footprint_m - 8192.0).abs() < 1e-9);
    assert!(fa.kernel.data.iter().all(|v| (*v - 0.5).abs() < 1e-6));
    let ra = p.family_kernel("ra").expect("ra has a kernel");
    assert!((ra.relief_m - 600.0).abs() < 1e-9);
}

#[test]
fn grammar_only_pack_still_loads_without_kernels() {
    // the synthetic golden pack ({} families, no kernels) must still load via load_pack_str.
    let p = pack::load_pack_str(GOLDEN).expect("golden still loads");
    assert_eq!(p.palettes.len(), 4);
    // a {} family simply has no kernel entry.
    assert!(p.family_kernel("mountain").is_none());
}

#[test]
fn load_pack_dir_rejects_missing_kernel_file() {
    // a pack referencing a kernel path that does not exist must error on load.
    let dir = fixtures_dir();
    let bad = r#"{"schema":"worldgen10.terrain_pack.v1","version":1,"grammar_constants":{"region_size_m":1.0,"province_size_regions":4,"palette_primary_pct":72,"palette_compatible_pct":22,"moderation_min":0.4,"moderation_strength":0.5},"palettes":[{"id":"p","families":["x","y","z"]}],"compatibility":{"p":[]},"families":{"x":{"kernel":"kernels/missing.npy","relief_m":1.0,"footprint_m":1.0},"y":{},"z":{}}}"#;
    let err = pack::load_pack_with_base(bad, &dir).expect_err("must reject missing kernel file");
    assert!(err.contains("missing.npy") || err.contains("kernel"), "error should name the missing kernel: {err}");
}

#[test]
fn load_pack_dir_rejects_bad_relief() {
    let dir = fixtures_dir();
    let bad = r#"{"schema":"worldgen10.terrain_pack.v1","version":1,"grammar_constants":{"region_size_m":1.0,"province_size_regions":4,"palette_primary_pct":72,"palette_compatible_pct":22,"moderation_min":0.4,"moderation_strength":0.5},"palettes":[{"id":"p","families":["x","y","z"]}],"compatibility":{"p":[]},"families":{"x":{"kernel":"kernels/flat.npy","relief_m":0.0,"footprint_m":1.0},"y":{},"z":{}}}"#;
    let err = pack::load_pack_with_base(bad, &dir).expect_err("must reject relief_m<=0");
    assert!(err.contains("relief_m"), "error should mention relief_m: {err}");
}
