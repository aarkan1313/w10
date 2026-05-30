use crate::facts;

#[test]
fn composed_height_no_edit_equals_base() {
    let h = facts::composed_height(123.0, 0.0, f64::NEG_INFINITY, f64::INFINITY);
    assert_eq!(h, 123.0);
}

#[test]
fn composed_height_adds_delta() {
    let h = facts::composed_height(100.0, -30.0, f64::NEG_INFINITY, f64::INFINITY);
    assert_eq!(h, 70.0, "a -30 m edit lowers the surface");
}

#[test]
fn composed_height_clamps_to_bedrock_floor() {
    // base 10, dig -100 -> -90, but bedrock floor at -5 stops it at -5.
    let h = facts::composed_height(10.0, -100.0, -5.0, f64::INFINITY);
    assert_eq!(h, -5.0, "bedrock floor clamps the dig");
}

#[test]
fn composed_height_clamps_to_ceiling() {
    let h = facts::composed_height(10.0, 1000.0, f64::NEG_INFINITY, 50.0);
    assert_eq!(h, 50.0, "ceiling clamps a tall mound");
}

#[test]
fn collision_field_samples_grid_row_major_matching_point() {
    // 3x3 grid, world_size 200 centred at (1000,2000). Each cell must equal the height closure
    // (x+z) at that cell's world point — verifies geometry/ordering independent of the real formula.
    let n = 3;
    let center = (1000.0, 2000.0);
    let size = 200.0;
    let grid = facts::collision_field(center.0, center.1, size, n, |x, z| x + z);
    assert_eq!(grid.len(), n * n);
    let corner = (center.0 - size / 2.0, center.1 - size / 2.0);
    let step = size / (n as f64 - 1.0);
    for j in 0..n {
        for i in 0..n {
            let wx = corner.0 + i as f64 * step;
            let wz = corner.1 + j as f64 * step;
            let expected = (wx + wz) as f32;
            let got = grid[j * n + i];
            assert!((got - expected).abs() < 1e-3, "cell ({i},{j}) got {got} expected {expected}");
        }
    }
}

#[test]
fn collision_field_rejects_bad_args() {
    assert!(facts::collision_field(0.0, 0.0, 200.0, 1, |_, _| 0.0).is_empty(), "n<2 -> empty");
    assert!(facts::collision_field(0.0, 0.0, 0.0, 3, |_, _| 0.0).is_empty(), "size<=0 -> empty");
}
