use crate::hash;

#[test]
fn stable_hash_matches_wg9_fixture_cases() {
    // Cases taken verbatim from worldgen_terrain/fixtures/hash_reference.json
    // (joined_text -> hash_u32). Hardcoded here so the Rust test is standalone.
    let cases: &[(&str, u32)] = &[
        ("province_palette|0|0|1337", 1924655373),
        ("province_palette|-6|12|1337", 4166305643),
        ("palette_local|-24|-24|-6|-6|1337", 1435408736),
        ("palette_compatible|17|-9|1337", 2856444241),
    ];
    for (text, expected) in cases {
        assert_eq!(hash::fnv1a_32(text), *expected, "case {text}");
    }
}

#[test]
fn hash_grid_is_deterministic_and_unit_range() {
    let a = hash::hash_grid(3, -7, 1337, 0);
    let b = hash::hash_grid(3, -7, 1337, 0);
    assert_eq!(a, b);                       // deterministic
    assert!((0.0..=1.0).contains(&a));      // normalized
    assert_ne!(hash::hash_grid(3, -7, 1337, 0), hash::hash_grid(4, -7, 1337, 0));
}

#[test]
fn hash_grid_matches_wg9_fixture_cases() {
    // Exact values from hash_reference.json `hash_grid_cases`. A property test
    // (range + determinism) passes even for a subtly-wrong mix; only these
    // exact values catch the full-width-vs-u32 multiply divergence (DESIGN §4).
    // (ix, iz, seed, salt, value_0_to_1)
    let cases: &[(i64, i64, i64, i64, f64)] = &[
        (0, 0, 1337, 0, 0.3299432826065326),
        (1, 0, 1337, 0, 0.6236977061777603),
        (0, 1, 1337, 1, 0.9689791416677133),
        (-1, 2, 2049, 3, 0.14776273587433686),
        (12345, -6789, 8191, 7, 0.6217125054965058),
    ];
    for (ix, iz, seed, salt, want) in cases {
        let got = hash::hash_grid(*ix, *iz, *seed, *salt);
        assert!(
            (got - want).abs() < 1e-15,
            "hash_grid({ix},{iz},{seed},{salt}) got={got:.17} want={want:.17}"
        );
    }
}

#[test]
fn value_noise_deterministic_and_bounded() {
    let n1 = hash::value_noise(123.5, -88.25, 600.0, 1337, 0);
    let n2 = hash::value_noise(123.5, -88.25, 600.0, 1337, 0);
    assert_eq!(n1, n2);
    assert!((-1.0..=1.0).contains(&n1));
}

#[test]
fn fbm_deterministic_and_bounded() {
    let f1 = hash::fbm(10.0, 20.0, 800.0, 1337, 4);
    let f2 = hash::fbm(10.0, 20.0, 800.0, 1337, 4);
    assert_eq!(f1, f2);
    assert!((-1.0..=1.0).contains(&f1));
}

#[test]
fn value_noise_and_fbm_match_wg9_fixture_cases() {
    // Exact values from hash_reference.json `noise_cases`. Locks the float
    // bilerp + fade + octave layering against ground truth, not just bounds.
    // (x, z, scale_m, seed, salt, value_noise, fbm_4)
    let cases: &[(f64, f64, f64, i64, i64, f64, f64)] = &[
        (0.0, 0.0, 32768.0, 1337, 0, -0.3401134347869348, -0.20429246866554623),
        (2048.0, 0.0, 32768.0, 1337, 0, -0.3388101953665653, -0.2219334909335803),
        (12345.0, -6789.0, 12000.0, 1348, 2, 0.26346598436864155, -0.11290855275643222),
        (-32768.0, 65536.0, 52000.0, 4099, 4, 0.23198917690638643, 0.1005024653142447),
    ];
    for (x, z, scale, seed, salt, want_vn, want_fbm) in cases {
        let vn = hash::value_noise(*x, *z, *scale, *seed, *salt);
        assert!(
            (vn - want_vn).abs() < 1e-15,
            "value_noise({x},{z},{scale},{seed},{salt}) got={vn:.17} want={want_vn:.17}"
        );
        // fbm cases in the fixture use 4 octaves with the case seed (salt is the
        // per-octave index inside fbm, so the case `salt` does not apply here).
        let fbm = hash::fbm(*x, *z, *scale, *seed, 4);
        assert!(
            (fbm - want_fbm).abs() < 1e-15,
            "fbm({x},{z},{scale},{seed},4) got={fbm:.17} want={want_fbm:.17}"
        );
    }
}
