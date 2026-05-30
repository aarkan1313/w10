use crate::edit_layer::{EditProvider, NoEdits};

#[test]
fn no_edits_delta_is_zero_everywhere() {
    let p = NoEdits;
    for (x, z) in [(0.0, 0.0), (1234.5, -9876.0), (1.0e6, -1.0e6)] {
        assert_eq!(p.delta(x, z), 0.0, "NoEdits must return 0 @ ({x},{z})");
    }
}
