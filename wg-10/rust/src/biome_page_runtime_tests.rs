//! Unit tests for the RUNTIME mountain page producer's PURE helpers (Task 3, Slice-4b).
//!
//! These pin the apron-grid geometry the runtime producer relies on (core + apron each side,
//! and the core->apron index offset). The GPU dispatch itself (context build/free + the
//! crop-to-image PASS_CROP_IMG path) is proven by the LATER windowed 576 parity gate (Task 4),
//! NOT by these cargo unit tests -- a local RenderingDevice is null headless on this box, so the
//! GPU path is not test-runnable under `cargo test`. Here we only guard the seam-safe geometry.

use crate::biome_page_compute::{biome_apron_dim, core_to_apron_index};

#[test]
fn apron_dim_576() {
    // Mountain production page: 256 core + 2*160 apron = 576 working grid.
    assert_eq!(biome_apron_dim(256, 160), 576);
}

#[test]
fn core_to_apron_offsets() {
    // (0,0) core maps to (apron, apron); the last core cell maps to (core-1+apron, ...).
    assert_eq!(core_to_apron_index(0, 0, 160), (160, 160));
    assert_eq!(core_to_apron_index(255, 255, 160), (415, 415));
}
