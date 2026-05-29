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

/// A plain XZ vertex (y filled by the shader at render time).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vert3 { pub x: f64, pub y: f64, pub z: f64 }

/// Flat mesh data for one ring band: shared positions + triangle indices into them.
pub struct RingMesh {
    pub positions: Vec<Vert3>,
    pub indices: Vec<u32>,
}

/// Build the mesh for `level` with `grid_res` cells across the outer span. Level 0 is a
/// full grid; level L>0 drops the center cells inside the hole, leaving a square annulus.
/// Vertices are a full (grid_res+1)^2 lattice (shared); only triangles for kept cells are
/// emitted. Unused vertices are harmless (a few extra positions; the GPU ignores them).
pub fn band_mesh(layout: &RingLayout, level: i32, grid_res: i32) -> RingMesh {
    assert!(grid_res >= 1, "grid_res must be >= 1");
    let span = layout.level_span(level);
    let half = span * 0.5;
    let cell = span / grid_res as f64;
    let n = grid_res + 1; // verts per side

    // Full shared vertex lattice, centered.
    let mut positions = Vec::with_capacity((n * n) as usize);
    for iz in 0..n {
        for ix in 0..n {
            positions.push(Vert3 {
                x: -half + ix as f64 * cell,
                y: 0.0,
                z: -half + iz as f64 * cell,
            });
        }
    }

    // Emit two triangles per KEPT cell. A cell is kept unless its center lies inside the
    // hole (both axes within +/- hole_half). idx(ix,iz) maps lattice coords -> vert index.
    let hole_half = layout.inner_hole_span(level) * 0.5;
    let idx = |ix: i32, iz: i32| -> u32 { (iz * n + ix) as u32 };
    let mut indices = Vec::new();
    for cz in 0..grid_res {
        for cx in 0..grid_res {
            // cell center in world space
            let center_x = -half + (cx as f64 + 0.5) * cell;
            let center_z = -half + (cz as f64 + 0.5) * cell;
            let in_hole = center_x.abs() < hole_half && center_z.abs() < hole_half;
            if in_hole { continue; }
            let v00 = idx(cx, cz);
            let v10 = idx(cx + 1, cz);
            let v01 = idx(cx, cz + 1);
            let v11 = idx(cx + 1, cz + 1);
            // two CCW triangles (viewed from +y)
            indices.push(v00); indices.push(v01); indices.push(v11);
            indices.push(v00); indices.push(v11); indices.push(v10);
        }
    }

    RingMesh { positions, indices }
}
