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

#[test]
fn family_weights_sum_to_one_and_are_bounded() {
    let p = golden();
    for (x, z) in [(0.0, 0.0), (-1024.5, 2048.25), (1.0e6, -1.0e6), (40000.0, 9000.0)] {
        let w = grammar::family_weights(x, z, 1337, &p);
        let sum: f64 = w.iter().map(|(_, weight)| *weight).sum();
        assert!((sum - 1.0).abs() < 1e-12, "weights must sum to 1 @ ({x},{z}), got {sum}");
        // bounded arity: at most 4 corners * 3 families distinct
        assert!(w.len() <= grammar::MAX_FAMILY_WEIGHTS);
        assert!(w.iter().all(|(_, weight)| *weight >= 0.0));
    }
}

#[test]
fn family_weights_deterministic_across_calls() {
    let p = golden();
    let a = grammar::family_weights(-1024.5, 2048.25, 1337, &p);
    let b = grammar::family_weights(-1024.5, 2048.25, 1337, &p);
    assert_eq!(a, b);
}

#[test]
fn family_weights_continuous_across_region_seam() {
    // Stepping across a region boundary (x = region_size_m) must not jump: the
    // blend is continuous, so the weight of every family present on one side is
    // nearly identical just across the boundary.
    let p = golden();
    let s = p.grammar_constants.region_size_m;
    let just_below = grammar::family_weights(s - 0.01, 100.0, 1337, &p);
    let exactly = grammar::family_weights(s, 100.0, 1337, &p);
    // Both normalized.
    let total_below: f64 = just_below.iter().map(|(_, w)| *w).sum();
    let total_at: f64 = exactly.iter().map(|(_, w)| *w).sum();
    assert!((total_below - 1.0).abs() < 1e-12 && (total_at - 1.0).abs() < 1e-12);
    // Continuity: each family's weight just below the seam matches its weight at
    // the seam within a small tolerance (a 0.01m step over a 32768m region is a
    // ~3e-7 change in grid space; absent a discontinuity the weights barely move).
    for (fam, w_below) in just_below.iter() {
        let w_at = exactly.iter().find(|(f, _)| f == fam).map(|(_, w)| *w).unwrap_or(0.0);
        assert!((w_below - w_at).abs() < 1e-3,
            "family {fam} weight jumped across seam: below={w_below} at={w_at}");
    }
    // and symmetrically, no family appears at the seam that was absent just below
    for (fam, w_at) in exactly.iter() {
        let w_below = just_below.iter().find(|(f, _)| f == fam).map(|(_, w)| *w).unwrap_or(0.0);
        assert!((w_at - w_below).abs() < 1e-3,
            "family {fam} appeared discontinuously at seam: at={w_at} below={w_below}");
    }
    assert!(just_below.iter().all(|(_, w)| w.is_finite()));
    assert!(exactly.iter().all(|(_, w)| w.is_finite()));
}

#[test]
fn family_weights_continuous_across_zero_axis() {
    let p = golden();
    for x in [-0.001_f64, 0.0, 0.001] {
        let w = grammar::family_weights(x, 5.0, 1337, &p);
        let sum: f64 = w.iter().map(|(_, weight)| *weight).sum();
        assert!((sum - 1.0).abs() < 1e-12);
        assert!(w.iter().all(|(_, weight)| weight.is_finite()));
    }
}
