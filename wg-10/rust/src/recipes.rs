//! Seam-safe biome RECIPES, ported bit-close (f64) from the Python originals in
//! `tools/dem_pack/*_synthesis.py`.
//!
//! This is the FIRST of 11 biome ports; the MOUNTAIN recipe
//! (`tools/dem_pack/mountain_synthesis.py::generate`, `apron_px > 0` seam-safe path)
//! is the TEMPLATE. The shared `helpers` submodule below holds everything the other
//! 10 biomes reuse (affine_remap, smoothstep, the pointwise-noise grid driver, the
//! seam-safe flow-channels wrapper, the rotation helper). Biome-specific math
//! (constants, style fields, the assembly pipeline) lives in the per-biome submodule
//! (`mountain` here). Add the next biome as a sibling submodule that leans on `helpers`.
//!
//! Parity contract: the public `mountain_seamsafe(...)` reproduces the Python core-
//! cropped height within a tight epsilon, verified against the committed fixture
//! `tools/dem_pack/fixtures/recipe_mountain_fixture.json` (`recipes_tests.rs`).
//!
//! Whole-array building blocks come from [`crate::array_ops`] (gaussian_filter_nearest,
//! flow_accumulation_mfd) and the per-point noise from [`crate::recipe_noise`]; both are
//! already fixture-proven, so this module only has to compose them faithfully.

// The recipes are consumed by the GPU/CPU producer seam that is not wired yet; until
// then several entry points are exercised only by the parity test.
#![allow(dead_code)]

/// Shared recipe helpers reused by every biome port. Keep additions here SMALL and
/// genuinely shared; biome-specific math belongs in the per-biome submodule.
pub mod helpers;

/// MOUNTAIN biome — the template port. Mirrors `tools/dem_pack/mountain_synthesis.py`.
pub mod mountain;

/// Public template entry point: MOUNTAIN seam-safe height, core-cropped.
///
/// `wx`/`wz` are apron-padded world-coord grids (flat row-major, PADDED `rows*cols`);
/// returns the inner core height (length `(rows-2*apron_px)*(cols-2*apron_px)`), exactly
/// like the Python `generate(...)["height"]`. Uses `STYLES[0]` (alpine_branching).
///
/// `spacing_m` world-anchors every seam-safe blur sigma (pass `helpers::S_REF` for the
/// reference-level identity). `flow_on` enables the drainage carve (pass `false` on coarse
/// clipmap levels to skip the two flow passes -> MACRO surface, parallel to the Python).
#[allow(clippy::too_many_arguments)]
pub fn mountain_seamsafe(
    wx: &[f64],
    wz: &[f64],
    rows: usize,
    cols: usize,
    seed: i64,
    feature_span_m: f64,
    apron_px: usize,
    spacing_m: f64,
    flow_on: bool,
) -> Vec<f64> {
    mountain::generate_seamsafe(
        wx,
        wz,
        rows,
        cols,
        seed,
        &mountain::ALPINE_BRANCHING,
        feature_span_m,
        apron_px,
        spacing_m,
        flow_on,
    )
}
