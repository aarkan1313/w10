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
