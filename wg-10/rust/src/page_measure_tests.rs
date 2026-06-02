use crate::page_measure::{apron_dim, recipe_load_field};

#[test]
fn apron_dim_adds_two_aprons() {
    // core 256 + 2*160 apron = 576
    assert_eq!(apron_dim(256, 160), 576);
    assert_eq!(apron_dim(256, 0), 256);
}

#[test]
fn recipe_load_field_is_finite_and_right_size() {
    let f = recipe_load_field(64, 7);
    assert_eq!(f.len(), 64 * 64);
    assert!(f.iter().all(|v: &f32| v.is_finite()));
}

#[test]
fn recipe_load_field_deterministic() {
    assert_eq!(recipe_load_field(48, 3), recipe_load_field(48, 3));
}
