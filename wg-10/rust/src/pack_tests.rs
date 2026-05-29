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
fn rejects_pct_sum_at_overflow_boundary() {
    // primary u32::MAX + compatible 2 would wrap to 1 in release with naive u32
    // addition; the u64-widened guard must still reject it.
    let bad = r#"{"schema":"worldgen10.terrain_pack.v1","version":1,"grammar_constants":{"region_size_m":1.0,"province_size_regions":4,"palette_primary_pct":4294967295,"palette_compatible_pct":2},"palettes":[],"compatibility":{},"families":{}}"#;
    let err = pack::load_pack_str(bad).expect_err("must reject overflow-prone pct sum");
    assert!(err.contains("pct") || err.contains("100"), "error should mention pct range: {err}");
}
