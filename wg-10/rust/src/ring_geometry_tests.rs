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

use crate::ring_geometry::band_mesh;

#[test]
fn level0_full_grid_vertex_and_index_counts() {
    let l = layout();
    // grid_res = number of CELLS per side; a full grid has (res+1)^2 verts, res^2*2 tris.
    let m = band_mesh(&l, 0, 4);
    assert_eq!(m.positions.len(), (4 + 1) * (4 + 1)); // 25 vertices
    assert_eq!(m.indices.len(), 4 * 4 * 2 * 3);        // 16 cells * 2 tris * 3 idx = 96
}

#[test]
fn level0_grid_is_centered_and_spans_full_band() {
    let l = layout();
    let m = band_mesh(&l, 0, 4);
    let span = l.level_span(0);
    // every vertex within [-span/2, +span/2] in x and z; corners hit the extremes.
    let half = span * 0.5;
    let mut min_x = f64::INFINITY; let mut max_x = f64::NEG_INFINITY;
    for v in &m.positions {
        assert!(v.x >= -half - 1e-9 && v.x <= half + 1e-9);
        assert!(v.z >= -half - 1e-9 && v.z <= half + 1e-9);
        assert_eq!(v.y, 0.0); // flat; displacement happens in the shader
        if v.x < min_x { min_x = v.x; }
        if v.x > max_x { max_x = v.x; }
    }
    assert!((min_x + half).abs() < 1e-9);
    assert!((max_x - half).abs() < 1e-9);
}

#[test]
fn outer_band_has_hollow_center() {
    let l = layout();
    // Level 1: outer span 16384, hole 8192. With grid_res cells over the outer span,
    // cells whose center lies within [-4096, +4096] in BOTH axes are removed.
    let full = band_mesh(&l, 0, 8);              // filled reference at same res
    let band = band_mesh(&l, 1, 8);              // hollow
    // The hollow band must have FEWER triangles than a full grid of the same res.
    assert!(band.indices.len() < full.indices.len(),
        "hollow band must drop center cells: band={} full={}", band.indices.len(), full.indices.len());
    // And it must have SOME triangles (the annulus ring).
    assert!(band.indices.len() > 0, "band must not be empty");
    // No triangle's centroid may fall strictly inside the hole.
    let hole_half = l.inner_hole_span(1) * 0.5; // 4096
    let mut i = 0;
    while i < band.indices.len() {
        let a = band.positions[band.indices[i] as usize];
        let b = band.positions[band.indices[i + 1] as usize];
        let c = band.positions[band.indices[i + 2] as usize];
        let cx = (a.x + b.x + c.x) / 3.0;
        let cz = (a.z + b.z + c.z) / 3.0;
        let inside = cx.abs() < hole_half - 1e-6 && cz.abs() < hole_half - 1e-6;
        assert!(!inside, "triangle centroid ({cx},{cz}) fell inside the hole");
        i += 3;
    }
}
