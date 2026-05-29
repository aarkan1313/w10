use crate::parity;
use crate::pack;
use std::path::Path;

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../worldgen_terrain/fixtures")
}
fn height_pack() -> pack::Pack {
    pack::load_pack_dir(&fixtures_dir(), "height_pack.json").expect("height pack loads")
}

#[test]
fn family_signature_is_deterministic() {
    let p = height_pack();
    let a = parity::family_signature(-1024.5, 2048.25, 1337, &p);
    let b = parity::family_signature(-1024.5, 2048.25, 1337, &p);
    assert_eq!(a, b);
}

#[test]
fn family_signature_varies_across_grid() {
    let p = height_pack();
    let mut seen = std::collections::BTreeSet::new();
    for i in -20..20 {
        seen.insert(parity::family_signature(i as f64 * 40000.0, 0.0, 1337, &p));
    }
    assert!(seen.len() >= 2, "signature collapsed to one value");
}

#[test]
fn family_signature_stable_for_same_coord() {
    let p = height_pack();
    let s = parity::family_signature(0.0, 0.0, 1337, &p);
    assert_eq!(s, parity::family_signature(0.0, 0.0, 1337, &p));
}
