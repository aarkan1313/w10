use crate::schedule_policy::{ScheduleConfig, SchedulePolicy};
use crate::page_policy::PageKey;

fn cfg() -> ScheduleConfig {
    ScheduleConfig {
        num_levels: 3,
        base_span: 1000.0,
        radius_pages: 1,
        lead_seconds: 0.0,
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
fn velocity_lead_biases_centre_but_clamps_to_keep_camera_covered() {
    let mut c = cfg(); // base_span 1000, radius 1 -> max_lead = (1-0.5)*1000 = 500 m
    c.lead_seconds = 0.5;
    let p = SchedulePolicy::new(c);
    // Stationary: led centre == camera; camera's page (0,0) covered.
    assert_eq!(p.coverage_center(0.0, 0.0, 0.0, 0.0), (0.0, 0.0));
    let still = p.coverage(0.0, 0.0, 0.0, 0.0);
    assert!(still.iter().any(|k| k.level == 0 && k.origin_x == 0 && k.origin_z == 0));
    // Moving +x at 300 m/s: raw lead = 300*0.5 = 150 m (< 500 clamp) -> centre 150, biases the
    // ring toward +x but the camera's own page (origin 0) is STILL covered.
    let (cx, _) = p.coverage_center(0.0, 0.0, 300.0, 0.0);
    assert_eq!(cx, 150.0, "sub-clamp lead applies directly");
    // Moving +x FAST (sprint, e.g. 8000 m/s): raw lead 4000 m would be 4 pages ahead, but it is
    // CLAMPED to +500 m so the camera can never fall out of its ring. The camera's page (0,0)
    // MUST remain covered at any speed — this is the never-bare-ground-under-you guarantee.
    let (cxf, _) = p.coverage_center(0.0, 0.0, 8000.0, 0.0);
    assert_eq!(cxf, 500.0, "lead clamps to (radius-0.5)*span = 500 m, not 4000 m");
    let sprint = p.coverage(0.0, 0.0, 8000.0, 0.0);
    assert!(sprint.iter().any(|k| k.level == 0 && k.origin_x == 0 && k.origin_z == 0),
        "camera's own level-0 page must stay covered even at sprint speed (clamped lead)");
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

#[test]
fn coarser_fallback_non_zero_origin_quantizes_correctly() {
    let p = SchedulePolicy::new(cfg()); // base_span=1000
    // Level-0 page at (1000,0): centre=(1500,500), level-1 span=2000 -> ancestor at (0,0).
    let ancestor = PageKey { level: 1, origin_x: 0, origin_z: 0 };
    let resident = keyset(&[ancestor]);
    let missing = PageKey { level: 0, origin_x: 1000, origin_z: 0 };
    assert_eq!(p.coarser_fallback(missing, &resident), Some(ancestor));
    // Level-0 page at (2000,0): centre=(2500,500), level-1 span=2000 -> ancestor at (2000,0).
    let ancestor2 = PageKey { level: 1, origin_x: 2000, origin_z: 0 };
    let resident2 = keyset(&[ancestor2]);
    let missing2 = PageKey { level: 0, origin_x: 2000, origin_z: 0 };
    assert_eq!(p.coarser_fallback(missing2, &resident2), Some(ancestor2));
}

#[test]
fn plan_frame_caps_acquires_at_max_per_frame() {
    let p = SchedulePolicy::new(cfg()); // max_per_frame = 2, coverage = 27 keys
    let empty = HashSet::new();
    let plan = p.plan_frame(0.0, 0.0, 0.0, 0.0, &empty);
    assert!(plan.acquire.len() <= 2,
        "acquire must be capped at max_per_frame, got {}", plan.acquire.len());
    assert!(plan.release.is_empty(), "nothing resident -> nothing to release");
}

#[test]
fn plan_frame_prioritizes_coarsest_level_first() {
    let p = SchedulePolicy::new(cfg()); // 3 levels, coarsest == 2
    let empty = HashSet::new();
    let plan = p.plan_frame(0.0, 0.0, 0.0, 0.0, &empty);
    // With everything missing and max=2, the two acquires must be the COARSEST
    // (level 2) pages — the coarse ring is the never-black blanket and must be
    // acquired before fine detail (else a fast camera outruns it -> black).
    let coarsest = p.config().num_levels - 1;
    assert!(plan.acquire.iter().all(|k| k.level == coarsest),
        "coarsest level must win priority, got {:?}", plan.acquire);
}

#[test]
fn plan_frame_releases_pages_no_longer_needed() {
    let p = SchedulePolicy::new(cfg());
    // A page far from coverage that is resident -> released.
    let stale = PageKey { level: 0, origin_x: 1_000_000, origin_z: 0 };
    let resident = keyset(&[stale]);
    let plan = p.plan_frame(0.0, 0.0, 0.0, 0.0, &resident);
    assert!(plan.release.contains(&stale), "stale resident page must be released");
}

#[test]
fn plan_frame_empty_when_fully_resident() {
    let p = SchedulePolicy::new(cfg());
    let needed = p.coverage(0.0, 0.0, 0.0, 0.0);
    let resident = keyset(&needed);
    let plan = p.plan_frame(0.0, 0.0, 0.0, 0.0, &resident);
    assert!(plan.acquire.is_empty(), "fully resident -> no acquires");
    assert!(plan.release.is_empty(), "fully resident, all needed -> no releases");
}

#[test]
fn plan_frame_is_deterministic() {
    let p = SchedulePolicy::new(cfg());
    // A resident set mixing covered and far-away (to-be-released) pages, so both
    // acquire AND release are non-empty and their ORDER is actually exercised.
    let resident = keyset(&[
        PageKey { level: 0, origin_x: 5_000_000, origin_z: 0 },
        PageKey { level: 1, origin_x: 5_000_000, origin_z: 0 },
        PageKey { level: 2, origin_x: 5_000_000, origin_z: 7_000_000 },
    ]);
    let a = p.plan_frame(123.0, 456.0, 50.0, -20.0, &resident);
    let b = p.plan_frame(123.0, 456.0, 50.0, -20.0, &resident);
    assert_eq!(a.acquire, b.acquire);
    assert_eq!(a.release, b.release);
    // all three far pages are out of coverage near the origin -> all released
    assert_eq!(a.release.len(), 3, "all far resident pages should be released");
    // release is sorted (deterministic) — verify ascending PageKey order
    let mut sorted = a.release.clone();
    sorted.sort();
    assert_eq!(a.release, sorted, "release must be sorted for determinism");
}

#[test]
fn never_black_every_missing_covered_page_has_coarser_fallback() {
    let p = SchedulePolicy::new(cfg()); // 3 levels
    let coarsest = p.config().num_levels - 1;

    // Deterministic LCG sweep (no rand crate, no clock): visit many positions.
    let mut state: u64 = 0x1234_5678_9abc_def0;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as f64 / (1u64 << 31) as f64 // in [0,1)
    };

    for _ in 0..2000 {
        let pos_x = (next() - 0.5) * 100_000.0;
        let pos_z = (next() - 0.5) * 100_000.0;
        let vel_x = (next() - 0.5) * 2000.0;
        let vel_z = (next() - 0.5) * 2000.0;

        // Resident set = the full coarsest-level ring for this frame (the warm
        // coarse blanket the streamer keeps resident first). Finer levels absent.
        let coverage = p.coverage(pos_x, pos_z, vel_x, vel_z);
        let resident: HashSet<PageKey> = coverage
            .iter()
            .filter(|k| k.level == coarsest)
            .cloned()
            .collect();

        // Every covered page that is not resident must have a coarser fallback.
        for k in &coverage {
            if resident.contains(k) { continue; }
            assert!(
                p.coarser_fallback(*k, &resident).is_some(),
                "never-black violated: missing page {:?} at pos ({pos_x},{pos_z}) \
                 vel ({vel_x},{vel_z}) had no coarser resident fallback",
                k
            );
        }
    }
}
