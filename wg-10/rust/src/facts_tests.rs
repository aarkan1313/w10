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
