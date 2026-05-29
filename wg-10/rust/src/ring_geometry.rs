//! Clipmap ring geometry (DESIGN §5.1), pure — no `godot` imports.
//!
//! Computes the per-level band layout: level 0 is a filled grid square of side
//! `base_span`; level L (>0) is a hollow square ring band of outer side
//! `base_span * 2^L` whose inner hole is `base_span * 2^(L-1)` — exactly the outer
//! span of the level inside it, so the levels tile gaplessly. This module returns
//! plain vertex/index lists; the godot layer (`clipmap_rings`) turns them into
//! ArrayMeshes. Engine-agnostic and unit-testable.

/// Per-level clipmap layout. Levels: 0 = finest (filled), num_levels-1 = coarsest.
pub struct RingLayout {
    num_levels: i32,
    base_span: f64,
}

impl RingLayout {
    pub fn new(num_levels: i32, base_span: f64) -> Self {
        assert!(num_levels >= 1, "num_levels must be >= 1");
        assert!(base_span > 0.0, "base_span must be > 0");
        Self { num_levels, base_span }
    }

    pub fn num_levels(&self) -> i32 { self.num_levels }

    /// World-space outer side length of the band at `level` (= base_span * 2^level).
    pub fn level_span(&self, level: i32) -> f64 {
        self.base_span * 2f64.powi(level)
    }

    /// Side length of the hollow hole in the band at `level`. Level 0 is filled (0.0);
    /// level L's hole equals level (L-1)'s full span so the inner level fills it.
    pub fn inner_hole_span(&self, level: i32) -> f64 {
        if level == 0 { 0.0 } else { self.level_span(level - 1) }
    }
}
