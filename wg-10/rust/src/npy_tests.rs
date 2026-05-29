use crate::npy;

const FLAT: &[u8] = include_bytes!("../../worldgen_terrain/fixtures/kernels/flat.npy");
const RAMP: &[u8] = include_bytes!("../../worldgen_terrain/fixtures/kernels/ramp.npy");
const BAD_FORTRAN: &[u8] = include_bytes!("../../worldgen_terrain/fixtures/kernels/bad_fortran.npy");

#[test]
fn reads_flat_kernel_dims_and_values() {
    let k = npy::read_npy_f32(FLAT).expect("flat parses");
    assert_eq!((k.rows, k.cols), (4, 4));
    assert_eq!(k.data.len(), 16);
    assert!(k.data.iter().all(|v| (*v - 0.5).abs() < 1e-6));
}

#[test]
fn reads_ramp_kernel_row_values() {
    let k = npy::read_npy_f32(RAMP).expect("ramp parses");
    assert_eq!((k.rows, k.cols), (4, 4));
    assert!((k.data[0] - 0.0).abs() < 1e-6);
    assert!((k.data[1] - 1.0 / 3.0).abs() < 1e-6);
    assert!((k.data[3] - 1.0).abs() < 1e-6);
}

#[test]
fn rejects_fortran_order() {
    let err = npy::read_npy_f32(BAD_FORTRAN).expect_err("must reject fortran order");
    assert!(err.contains("fortran") || err.contains("order"), "error should mention order: {err}");
}

#[test]
fn rejects_bad_magic() {
    let err = npy::read_npy_f32(b"not an npy file at all").expect_err("must reject bad magic");
    assert!(err.contains("magic"), "error should mention magic: {err}");
}

#[test]
fn rejects_unsupported_dtype() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"\x93NUMPY\x01\x00");
    let header = b"{'descr': '<i4', 'fortran_order': False, 'shape': (1, 1), }";
    let mut hdr = header.to_vec();
    while (10 + hdr.len() + 1) % 64 != 0 { hdr.push(b' '); }
    hdr.push(b'\n');
    buf.extend_from_slice(&(hdr.len() as u16).to_le_bytes());
    buf.extend_from_slice(&hdr);
    buf.extend_from_slice(&0i32.to_le_bytes());
    let err = npy::read_npy_f32(&buf).expect_err("must reject non-float dtype");
    assert!(err.contains("dtype") || err.contains("descr"), "error should mention dtype: {err}");
}
