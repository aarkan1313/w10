use crate::grammar;
use crate::pack;

const GOLDEN: &str = include_str!("../../worldgen_terrain/fixtures/golden_pack.json");

fn golden() -> pack::Pack {
    pack::load_pack_str(GOLDEN).expect("golden loads")
}

#[test]
fn region_of_uses_floor_for_negatives() {
    let p = golden();
    // region_size_m = 32768; a point just below 0 is region -1, not 0.
    let (rx, _rz) = grammar::region_of(-1.0, 5.0, &p);
    assert_eq!(rx, -1);
    let (rx0, rz0) = grammar::region_of(0.0, 0.0, &p);
    assert_eq!((rx0, rz0), (0, 0));
    let (rxp, _) = grammar::region_of(40000.0, 0.0, &p);
    assert_eq!(rxp, 1); // 40000 / 32768 = 1.22 -> floor 1
}

#[test]
fn province_of_floors_by_province_size() {
    let p = golden();
    // province_size_regions = 4. region -1 -> province -1 (floor of -0.25).
    assert_eq!(grammar::province_of(-1, &p), -1);
    assert_eq!(grammar::province_of(0, &p), 0);
    assert_eq!(grammar::province_of(7, &p), 1);
}

#[test]
fn palette_for_region_is_deterministic_and_valid() {
    let p = golden();
    let a = grammar::palette_for_region(3, -7, 1337, &p);
    let b = grammar::palette_for_region(3, -7, 1337, &p);
    assert_eq!(a, b); // deterministic
    // result is a real palette index in range
    assert!(a < p.palettes.len());
}

#[test]
fn palette_for_region_varies_across_grid() {
    // Across a grid of regions, more than one palette must appear (no collapse).
    let p = golden();
    let mut seen = std::collections::BTreeSet::new();
    for rx in -10..10 {
        for rz in -10..10 {
            seen.insert(grammar::palette_for_region(rx, rz, 1337, &p));
        }
    }
    assert!(seen.len() >= 2, "palette selection collapsed to one palette");
}

#[test]
fn families_for_region_returns_three_with_normalized_bias() {
    let p = golden();
    let (fams, bias) = grammar::families_for_region(2, 9, 1337, &p);
    assert_eq!(fams.len(), pack::FAMILIES_PER_PALETTE);
    assert_eq!(bias.len(), pack::FAMILIES_PER_PALETTE);
    // bias is a probability split: non-negative and sums to 1.
    let sum: f64 = bias.iter().sum();
    assert!((sum - 1.0).abs() < 1e-12, "bias must sum to 1, got {sum}");
    assert!(bias.iter().all(|b| *b >= 0.0));
    // family ids are real indices into the pack family table.
    for f in fams {
        assert!((f as usize) < p.family_ids.len());
    }
}

#[test]
fn families_for_region_is_deterministic() {
    let p = golden();
    let a = grammar::families_for_region(-5, 11, 1337, &p);
    let b = grammar::families_for_region(-5, 11, 1337, &p);
    assert_eq!(a, b);
}
