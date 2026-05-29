use crate::ring_geometry::RingLayout;

fn layout() -> RingLayout {
    // 3 levels, base_span 8192 (one page span at level 0)
    RingLayout::new(3, 8192.0)
}

#[test]
fn level_span_doubles_per_level() {
    let l = layout();
    assert_eq!(l.level_span(0), 8192.0);
    assert_eq!(l.level_span(1), 16384.0);
    assert_eq!(l.level_span(2), 32768.0);
}

#[test]
fn inner_hole_of_band_equals_inner_level_outer_span() {
    let l = layout();
    // Level 0 is filled: no hole.
    assert_eq!(l.inner_hole_span(0), 0.0);
    // Level L's hole == level (L-1)'s full span, so the inner level exactly fills it.
    assert_eq!(l.inner_hole_span(1), l.level_span(0));
    assert_eq!(l.inner_hole_span(2), l.level_span(1));
}

#[test]
fn num_levels_accessor() {
    assert_eq!(layout().num_levels(), 3);
}
