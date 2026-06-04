//! Kernel packing, anchoring, and page-shape tests.

use super::super::*;

#[test]
fn kernel_sums_to_one() {
    for &sigma in &[1.0_f64, 1.15, 1.2, 1.8, 2.0, 5.0, 7.0, 2.4] {
        let k = gaussian_kernel1d(sigma, TRUNCATE);
        let s: f64 = k.iter().map(|&v| v as f64).sum();
        assert!((s - 1.0).abs() < 1e-5, "sigma {sigma}: sum {s} != 1");
    }
}

#[test]
fn kernel_is_symmetric() {
    let k = gaussian_kernel1d(2.0, TRUNCATE);
    let n = k.len();
    for i in 0..n {
        assert!(
            (k[i] - k[n - 1 - i]).abs() < 1e-7,
            "kernel not symmetric at {i}"
        );
    }
}

#[test]
fn kernel_length_matches_radius() {
    // array_ops: lw = int(truncate*sigma + 0.5); length = 2*lw+1.
    // sigma 1.0, truncate 4.0 -> lw = int(4.5) = 4 -> length 9.
    let k = gaussian_kernel1d(1.0, TRUNCATE);
    assert_eq!(k.len(), 9);
    assert_eq!(gaussian_radius(1.0, TRUNCATE), 4);
    // sigma 7.0 -> lw = int(28.5) = 28 -> length 57.
    assert_eq!(gaussian_radius(7.0, TRUNCATE), 28);
    assert_eq!(gaussian_kernel1d(7.0, TRUNCATE).len(), 57);
    // sigma 2.4 -> lw = int(10.1) = 10 -> length 21.
    assert_eq!(gaussian_radius(2.4, TRUNCATE), 10);
}

#[test]
fn kernel_center_is_peak() {
    let k = gaussian_kernel1d(2.0, TRUNCATE);
    let lw = (k.len() - 1) / 2;
    for i in 0..k.len() {
        assert!(k[lw] >= k[i], "center not peak");
    }
}

#[test]
fn all_mountain_kernels_fit_stride() {
    for &sg in &mountain_sigmas() {
        let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
        assert!(
            len <= KERNEL_STRIDE,
            "sigma {sg} kernel len {len} > {KERNEL_STRIDE}"
        );
    }
}

#[test]
fn anchored_kernels_identity_at_s_ref() {
    use crate::recipes::helpers::S_REF;

    let (packed, kp) = mountain_kernels_anchored(S_REF).expect("kernels fit at S_REF");
    let refs = mountain_sigmas();
    for (slot, &ref_sigma) in refs.iter().enumerate() {
        let want = gaussian_kernel1d(ref_sigma, TRUNCATE);
        let base = slot * KERNEL_STRIDE;
        for (j, &w) in want.iter().enumerate() {
            assert_eq!(
                packed[base + j],
                w,
                "slot {slot} (sigma {ref_sigma}) tap {j} differs at S_REF"
            );
        }
        let (ko, kr) = kp.kp(ref_sigma);
        assert_eq!(ko, (slot * KERNEL_STRIDE) as i32, "koffset drift at S_REF");
        assert_eq!(
            kr,
            gaussian_radius(ref_sigma, TRUNCATE) as i32,
            "kradius drift at S_REF"
        );
    }
}

#[test]
fn anchored_kernels_shrink_and_key_by_reference_when_coarser() {
    use crate::recipes::helpers::{sigma_cells, S_REF};

    let coarse = S_REF * 4.0;
    let (_packed, kp) = mountain_kernels_anchored(coarse).expect("coarse kernels fit");
    for &ref_sigma in &mountain_sigmas() {
        let anchored = sigma_cells(ref_sigma, coarse);
        assert!(
            anchored < ref_sigma + 1e-12,
            "coarser spacing must shrink sigma"
        );
        let (_ko, kr) = kp.kp(ref_sigma);
        assert_eq!(
            kr,
            gaussian_radius(anchored, TRUNCATE) as i32,
            "kradius must reflect the anchored sigma, keyed by the reference sigma"
        );
    }
}

#[test]
fn mountain_sigmas_cover_all_pipeline_blurs() {
    let valley = 2.4_f64;
    let trib = (valley * 0.42_f64).max(0.6);
    let floor = 4.0_f64.max(0.2);
    let s = mountain_sigmas();
    for need in [
        1.15_f64,
        1.20,
        1.80,
        2.00,
        5.00,
        7.00,
        valley,
        trib,
        floor,
        valley.max(0.1),
        trib.max(0.1),
    ] {
        assert!(
            s.iter().any(|&v| (v - need).abs() < 1e-9),
            "missing sigma {need}"
        );
    }
}

#[test]
fn apron_dim_adds_two_aprons() {
    assert_eq!(apron_dim(24, 160), 344);
    assert_eq!(apron_dim(256, 160), 576);
}
