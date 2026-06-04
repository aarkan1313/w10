//! Compose-kernel ABI tests.

use super::super::*;

#[test]
fn compose_sigmas_has_relief_sigma() {
    let s = compose_sigmas();
    assert_eq!(s.len(), 1);
    assert!((s[0] - 6.0).abs() < 1e-12);
    let len = 2 * gaussian_radius(s[0], TRUNCATE) + 1;
    assert!(
        len <= KERNEL_STRIDE,
        "compose kernel len {len} > {KERNEL_STRIDE}"
    );
    assert_eq!(gaussian_radius(6.0, TRUNCATE), 24);
    assert_eq!(len, 49);
}

#[test]
fn compose_kernel_matches_array_ops_relief_sigma() {
    let k = gaussian_kernel1d(6.0, TRUNCATE);
    let sum: f32 = k.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-6,
        "compose relief kernel not normalized (sum={sum})"
    );
    let n = k.len();
    for i in 0..n {
        assert!(
            (k[i] - k[n - 1 - i]).abs() < 1e-7,
            "compose relief kernel not symmetric at {i}"
        );
    }
}
