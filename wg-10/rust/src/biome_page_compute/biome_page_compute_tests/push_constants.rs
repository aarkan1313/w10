//! Push-constant ABI tests.

use super::super::*;

#[test]
fn push_constant_is_96_bytes() {
    let p = build_push(
        0, 344, 344, 160, 0, 4, 0, 0, 0, 0, 0, 3913.04, 12000.0, -31000.0, 90000.0, 0.48, 0.0, 0.0,
        0.0,
    );
    assert_eq!(p.len(), 96);
}

#[test]
fn push_constant_packs_ints_then_floats() {
    let p = build_push(
        7, 344, 343, 160, 5, 28, 2, 1, 128, 9, 4, 3913.0, 12000.0, -31000.0, 90000.0, 0.34, 0.0,
        0.0, 0.0,
    );
    assert_eq!(i32::from_le_bytes([p[0], p[1], p[2], p[3]]), 7);
    assert_eq!(i32::from_le_bytes([p[4], p[5], p[6], p[7]]), 344);
    assert_eq!(i32::from_le_bytes([p[8], p[9], p[10], p[11]]), 343);
    assert_eq!(i32::from_le_bytes([p[12], p[13], p[14], p[15]]), 160);
    assert_eq!(i32::from_le_bytes([p[16], p[17], p[18], p[19]]), 5);
    assert_eq!(i32::from_le_bytes([p[20], p[21], p[22], p[23]]), 28);
    assert_eq!(i32::from_le_bytes([p[24], p[25], p[26], p[27]]), 2);
    assert_eq!(i32::from_le_bytes([p[28], p[29], p[30], p[31]]), 1);
    assert_eq!(i32::from_le_bytes([p[32], p[33], p[34], p[35]]), 128);
    assert_eq!(i32::from_le_bytes([p[36], p[37], p[38], p[39]]), 9);
    assert_eq!(i32::from_le_bytes([p[40], p[41], p[42], p[43]]), 4);
    let spacing = f32::from_le_bytes([p[48], p[49], p[50], p[51]]);
    assert!((spacing - 3913.0).abs() < 1e-1);
    let flow_power = f32::from_le_bytes([p[64], p[65], p[66], p[67]]);
    assert!((flow_power - 0.34).abs() < 1e-6);
}

#[test]
fn non_volcanic_push_vent_count_is_zero_byte_identical() {
    let p = build_push(
        8, 344, 344, 160, 0, 0, 0, 0, 0, 0, 0, 2608.7, 12000.0, -31000.0, 60000.0, 0.0, 0.0, 0.0,
        0.0,
    );
    assert_eq!(i32::from_le_bytes([p[40], p[41], p[42], p[43]]), 0);
    assert_eq!(p.len(), 96);
    assert_eq!(f32::from_le_bytes([p[68], p[69], p[70], p[71]]), 0.0);
    assert_eq!(f32::from_le_bytes([p[72], p[73], p[74], p[75]]), 0.0);
}

#[test]
fn push_constant_carries_compose_params_in_pads() {
    let p = build_push(
        64, 32, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 1e-3, 0.0,
    );
    let favor = f32::from_le_bytes([p[68], p[69], p[70], p[71]]);
    let floor = f32::from_le_bytes([p[72], p[73], p[74], p[75]]);
    assert!((favor - 2.0).abs() < 1e-7, "favor_strength not in pad0");
    assert!(
        (floor - 1e-3).abs() < 1e-9,
        "relief_confidence_floor not in pad1"
    );
    for off in (76..96).step_by(4) {
        assert_eq!(
            f32::from_le_bytes([p[off], p[off + 1], p[off + 2], p[off + 3]]),
            0.0
        );
    }
    assert_eq!(p.len(), 96);
}
