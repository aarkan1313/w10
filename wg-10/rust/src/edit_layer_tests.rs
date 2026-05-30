use crate::edit_layer::{EditProvider, NoEdits, StampEdits};

#[test]
fn no_edits_delta_is_zero_everywhere() {
    let p = NoEdits;
    for (x, z) in [(0.0, 0.0), (1234.5, -9876.0), (1.0e6, -1.0e6)] {
        assert_eq!(p.delta(x, z), 0.0, "NoEdits must return 0 @ ({x},{z})");
    }
}

#[test]
fn empty_stamps_delta_is_zero() {
    let s = StampEdits::new();
    assert_eq!(s.delta(0.0, 0.0), 0.0);
}

#[test]
fn single_stamp_full_depth_at_center_zero_at_edge() {
    let mut s = StampEdits::new();
    s.add(0.0, 0.0, 100.0, -50.0, 1.0); // radius 100, depth -50, full cosine falloff
    assert!((s.delta(0.0, 0.0) - (-50.0)).abs() < 1e-4, "center = full depth");
    assert_eq!(s.delta(200.0, 0.0), 0.0, "outside radius = 0");
    assert!(s.delta(100.0, 0.0).abs() < 1.0, "edge ~ 0 via falloff");
}

#[test]
fn overlapping_stamps_sum() {
    let mut s = StampEdits::new();
    s.add(0.0, 0.0, 100.0, -10.0, 0.0); // falloff 0 = flat dent within radius
    s.add(0.0, 0.0, 100.0, -10.0, 0.0);
    assert!((s.delta(0.0, 0.0) - (-20.0)).abs() < 1e-4, "two -10 stamps sum to -20");
}

#[test]
fn stamps_are_deterministic() {
    let mut s = StampEdits::new();
    s.add(12.0, 34.0, 50.0, -5.0, 1.0);
    assert_eq!(s.delta(20.0, 30.0), s.delta(20.0, 30.0));
}

#[test]
fn radius_zero_stamp_is_ignored() {
    let mut s = StampEdits::new();
    s.add(0.0, 0.0, 0.0, -50.0, 1.0); // radius 0 -> ignored
    assert_eq!(s.len(), 0);
    assert_eq!(s.delta(0.0, 0.0), 0.0);
}
