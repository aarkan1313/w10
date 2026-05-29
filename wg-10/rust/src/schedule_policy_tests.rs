use crate::schedule_policy::{ScheduleConfig, SchedulePolicy};
use crate::page_policy::PageKey;

fn cfg() -> ScheduleConfig {
    ScheduleConfig {
        num_levels: 3,
        base_span: 1000.0,
        radius_pages: 1,
        lead_frames: 0.0,
        max_per_frame: 2,
    }
}

#[test]
fn page_origin_floor_quantizes_to_level_span() {
    let p = SchedulePolicy::new(cfg());
    // level 0 span = 1000: centre 1500 -> origin 1000; centre -1 -> origin -1000
    assert_eq!(p.page_origin(0, 1500.0, 1500.0), (1000, 1000));
    assert_eq!(p.page_origin(0, -1.0, 0.0), (-1000, 0));
    // level 1 span = 2000: centre 1500 -> origin 0; centre 2500 -> origin 2000
    assert_eq!(p.page_origin(1, 1500.0, 2500.0), (0, 2000));
    // exact multiple stays put (no off-by-one at the seam)
    assert_eq!(p.page_origin(0, 1000.0, 0.0), (1000, 0));
    assert_eq!(p.page_origin(0, 0.0, 0.0), (0, 0));
}

#[test]
fn coverage_size_is_levels_times_ring_area() {
    let p = SchedulePolicy::new(cfg()); // 3 levels, radius 1 -> 3x3 = 9 per level
    let keys = p.coverage(0.0, 0.0, 0.0, 0.0);
    assert_eq!(keys.len(), 3 * 9);
    // all levels 0..3 represented
    for level in 0..3 {
        assert!(keys.iter().any(|k| k.level == level),
            "level {level} missing from coverage");
    }
}

#[test]
fn coverage_keys_are_unique() {
    let p = SchedulePolicy::new(cfg());
    let keys = p.coverage(500.0, 500.0, 0.0, 0.0);
    let set: std::collections::HashSet<_> = keys.iter().cloned().collect();
    assert_eq!(set.len(), keys.len(), "coverage must not emit duplicate keys");
}

#[test]
fn velocity_lead_shifts_centre_in_travel_direction() {
    let mut c = cfg();
    c.lead_frames = 10.0; // lead = vel * 10
    let p = SchedulePolicy::new(c);
    // Stationary: level-0 ring centred on origin page (0,0) -> contains (0,0).
    let still = p.coverage(0.0, 0.0, 0.0, 0.0);
    assert!(still.iter().any(|k| k.level == 0 && k.origin_x == 0 && k.origin_z == 0));
    // Moving +x fast: centre = 0 + 200*10 = 2000 -> level-0 (span 1000) ring is
    // centred on page origin 2000; the origin-0 page is no longer covered at L0.
    let moving = p.coverage(0.0, 0.0, 200.0, 0.0);
    assert!(moving.iter().any(|k| k.level == 0 && k.origin_x == 2000),
        "fast +x travel should pull level-0 coverage ahead to x=2000");
    assert!(!moving.iter().any(|k| k.level == 0 && k.origin_x == 0),
        "origin-0 level-0 page should fall outside the led ring");
}

use std::collections::HashSet;

fn keyset(items: &[PageKey]) -> HashSet<PageKey> {
    items.iter().cloned().collect()
}

#[test]
fn coarser_fallback_returns_resident_ancestor() {
    let p = SchedulePolicy::new(cfg()); // base_span 1000, 3 levels
    // A level-1 page (span 2000) at origin (0,0) covers world [0,2000): it is the
    // ancestor of the level-0 page (span 1000) at origin (0,0).
    let l1_ancestor = PageKey { level: 1, origin_x: 0, origin_z: 0 };
    let resident = keyset(&[l1_ancestor]);
    let missing = PageKey { level: 0, origin_x: 0, origin_z: 0 };
    assert_eq!(p.coarser_fallback(missing, &resident), Some(l1_ancestor));
}

#[test]
fn coarser_fallback_walks_up_multiple_levels() {
    let p = SchedulePolicy::new(cfg());
    // Only the level-2 page (span 4000) at (0,0) is resident; a missing level-0
    // page at (0,0) must walk past the (absent) level-1 ancestor to level 2.
    let l2 = PageKey { level: 2, origin_x: 0, origin_z: 0 };
    let resident = keyset(&[l2]);
    let missing = PageKey { level: 0, origin_x: 0, origin_z: 0 };
    assert_eq!(p.coarser_fallback(missing, &resident), Some(l2));
}

#[test]
fn coarser_fallback_none_when_no_coarser_resident() {
    let p = SchedulePolicy::new(cfg());
    // Nothing resident -> no fallback. Also: a coarsest-level miss has no coarser.
    let empty = HashSet::new();
    let missing = PageKey { level: 0, origin_x: 0, origin_z: 0 };
    assert_eq!(p.coarser_fallback(missing, &empty), None);
    let coarsest_miss = PageKey { level: 2, origin_x: 0, origin_z: 0 };
    assert_eq!(p.coarser_fallback(coarsest_miss, &empty), None);
}
