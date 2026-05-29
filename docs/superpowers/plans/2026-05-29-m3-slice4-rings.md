# WorldGen10 M3 Slice 4 — Clipmap Rings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build N concentric clipmap ring meshes centered on the camera that render the scheduler's resident pages as seamless terrain — persistent meshes, recenter (not rebuild) on move, and an L↔L+1 geomorph that makes level boundaries crack-free.

**Architecture:** A pure-Rust `ring_geometry` module computes the clipmap band layout (level spans, the filled level-0 grid, the hollow ring-band vertex/index lists) — engine-agnostic and unit-tested. A godot `Wg10ClipmapRings` (Node3D) owns one persistent `MeshInstance3D`+`ShaderMaterial` per level (built from that geometry), recenters by quantized translate, and rebinds each level's resident page (with coarser fallback) from `Wg10PagePool`/`Wg10Streamer` each frame. The geomorph lives in `ring_displace.gdshader`: in each level's outer transition region the vertex shader blends its own page height toward the next-coarser page's height so the seam matches exactly. A windowed gate renders the assembly top-down and asserts no holes / real relief / seam continuity / morph-math / recenter-doesn't-rebuild.

**Tech Stack:** Rust (`gdext` 0.5.3, `godot` crate `api-4-6`), pure `f64`/`Vec` math (no glam — matches the codebase), Godot 4.6 `ArrayMesh` + spatial shader, GDScript SceneTree windowed gate via `tools/gate.py`.

---

## Conventions (read before Task 1)

- **Build/test the crate** from `wg-10/rust` with `CARGO_TARGET_DIR` UNSET (not empty —
  an empty string is rejected by cargo). Using the Bash tool:
  `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo test <one-filter>`
  (`cargo test` takes ONE filter arg; run `... cargo test ring_geometry` to run all of
  this module's tests). PowerShell form: `$env:CARGO_TARGET_DIR=$null; cargo test`.
- **Windowed gate** (project root `D:\workflows\worldgen10`, GODOT_BIN set):
  ```powershell
  $env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
  python tools/gate.py --suite m3   # WINDOWED; rings gate needs the global RenderingDevice
  ```
  The controller runs the windowed gate; a subagent that can't run windowed Godot should
  build + (where possible) sanity-check and report DONE, leaving the windowed run to the
  controller.
- **Commit trailer** on every commit:
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`
- **Pure modules** (`ring_geometry.rs`) carry **no** `godot` import — same discipline as
  `schedule_policy.rs` / `grammar.rs` / `height.rs`.
- **Stay on branch `main`** (the project commits per-task on main; the user pushes).
- Existing godot classes are all `#[class(base=RefCounted)]`. `Wg10ClipmapRings` is the
  first `Node3D` class — Task 2 verifies the gdext Node3D + `add_child` pattern compiles
  before any geometry work depends on it (probe-first).

---

## File Structure

**New:**
- `wg-10/rust/src/ring_geometry.rs` — pure: `RingLayout` (per-level spans/placement) +
  `band_mesh(...)` returning vertex positions (XZ, centered, y=0) + triangle indices for
  the filled level-0 grid and the hollow ring-bands. No godot. Unit-tested.
- `wg-10/rust/src/ring_geometry_tests.rs` — `#[cfg(test)]` unit tests.
- `wg-10/rust/src/clipmap_rings.rs` — godot `Wg10ClipmapRings` (Node3D): builds the N
  `MeshInstance3D`+`ShaderMaterial` children from `ring_geometry`, `recenter`,
  `update_pages` (bind resident/fallback pages + the coarser-neighbor texture for morph),
  `stats`. Owns no RIDs.
- `wg-10/worldgen_terrain/tests/m3_rings_check.gd` — the windowed gate.

**Modified:**
- `wg-10/worldgen_terrain/shaders/ring_displace.gdshader` — add the geomorph
  (backward-compatible no-morph default).
- `wg-10/rust/src/lib.rs` — `mod ring_geometry;` + `mod clipmap_rings;` +
  `#[cfg(test)] mod ring_geometry_tests;`.
- `tools/gate.py` — add `m3_rings_check.gd` to the `m3` suite.

---

## Task 1: `ring_geometry` — level layout (pure)

**Files:**
- Create: `wg-10/rust/src/ring_geometry.rs`
- Create: `wg-10/rust/src/ring_geometry_tests.rs`
- Modify: `wg-10/rust/src/lib.rs`

- [ ] **Step 1: Declare the modules in lib.rs**

In `wg-10/rust/src/lib.rs`, add `mod ring_geometry;` after the existing `mod streamer;`
line, and add `#[cfg(test)] mod ring_geometry_tests;` after the existing
`#[cfg(test)] mod schedule_policy_tests;` line. Example result:

```rust
mod streamer;
mod ring_geometry;

// ... in the #[cfg(test)] block ...
#[cfg(test)]
mod schedule_policy_tests;
#[cfg(test)]
mod ring_geometry_tests;
```

- [ ] **Step 2: Write the failing test**

Create `wg-10/rust/src/ring_geometry_tests.rs`:

```rust
use crate::ring_geometry::RingLayout;

fn layout() -> RingLayout {
    // 3 levels, base_span 8192 (one page span at level 0)
    RingLayout::new(3, 8192.0)
}

#[test]
fn level_span_doubles_per_level() {
    let l = layout();
    assert_eq!(l.level_span(0), 8192.0);
    assert_eq!(l.level_span(1), 16384.0);
    assert_eq!(l.level_span(2), 32768.0);
}

#[test]
fn inner_hole_of_band_equals_inner_level_outer_span() {
    let l = layout();
    // Level 0 is filled: no hole.
    assert_eq!(l.inner_hole_span(0), 0.0);
    // Level L's hole == level (L-1)'s full span, so the inner level exactly fills it.
    assert_eq!(l.inner_hole_span(1), l.level_span(0));
    assert_eq!(l.inner_hole_span(2), l.level_span(1));
}

#[test]
fn num_levels_accessor() {
    assert_eq!(layout().num_levels(), 3);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo test level_span_doubles`
Expected: FAIL to compile — `ring_geometry` / `RingLayout` not found.

- [ ] **Step 4: Write minimal implementation**

Create `wg-10/rust/src/ring_geometry.rs`:

```rust
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
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo test ring_geometry`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```powershell
git add wg-10/rust/src/ring_geometry.rs wg-10/rust/src/ring_geometry_tests.rs wg-10/rust/src/lib.rs
git commit -m "feat(m3): ring_geometry RingLayout (clipmap level spans)

Pure, no godot. Level 0 filled (side base_span); level L a hollow band of outer
side base_span*2^L with inner hole = level (L-1) span, so levels tile gaplessly.
level_span/inner_hole_span/num_levels tested.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `ring_geometry::band_mesh` — vertex/index generation (pure)

**Files:**
- Modify: `wg-10/rust/src/ring_geometry.rs`
- Test: `wg-10/rust/src/ring_geometry_tests.rs`

`band_mesh` produces a centered XZ grid (y=0) covering the band, as flat vertex and
index arrays. For level 0 it is a full `grid_res × grid_res` cell grid spanning
`[-span/2, +span/2]`. For level L>0 it is the same outer grid with the **center hole
cells removed** (cells fully inside `[-hole/2, +hole/2]` dropped), leaving a square
annulus. Vertices are shared via a single position list; indices reference into it.

- [ ] **Step 1: Write the failing tests**

Append to `wg-10/rust/src/ring_geometry_tests.rs`:

```rust
use crate::ring_geometry::band_mesh;

#[test]
fn level0_full_grid_vertex_and_index_counts() {
    let l = layout();
    // grid_res = number of CELLS per side; a full grid has (res+1)^2 verts, res^2*2 tris.
    let m = band_mesh(&l, 0, 4);
    assert_eq!(m.positions.len(), (4 + 1) * (4 + 1)); // 25 vertices
    assert_eq!(m.indices.len(), 4 * 4 * 2 * 3);        // 16 cells * 2 tris * 3 idx = 96
}

#[test]
fn level0_grid_is_centered_and_spans_full_band() {
    let l = layout();
    let m = band_mesh(&l, 0, 4);
    let span = l.level_span(0);
    // every vertex within [-span/2, +span/2] in x and z; corners hit the extremes.
    let half = span * 0.5;
    let mut min_x = f64::INFINITY; let mut max_x = f64::NEG_INFINITY;
    for v in &m.positions {
        assert!(v.x >= -half - 1e-9 && v.x <= half + 1e-9);
        assert!(v.z >= -half - 1e-9 && v.z <= half + 1e-9);
        assert_eq!(v.y, 0.0); // flat; displacement happens in the shader
        if v.x < min_x { min_x = v.x; }
        if v.x > max_x { max_x = v.x; }
    }
    assert!((min_x + half).abs() < 1e-9);
    assert!((max_x - half).abs() < 1e-9);
}

#[test]
fn outer_band_has_hollow_center() {
    let l = layout();
    // Level 1: outer span 16384, hole 8192. With grid_res cells over the outer span,
    // cells whose center lies within [-4096, +4096] in BOTH axes are removed.
    let full = band_mesh(&l, 0, 8);              // filled reference at same res
    let band = band_mesh(&l, 1, 8);              // hollow
    // The hollow band must have FEWER triangles than a full grid of the same res.
    assert!(band.indices.len() < full.indices.len(),
        "hollow band must drop center cells: band={} full={}", band.indices.len(), full.indices.len());
    // And it must have SOME triangles (the annulus ring).
    assert!(band.indices.len() > 0, "band must not be empty");
    // No triangle's centroid may fall strictly inside the hole.
    let hole_half = l.inner_hole_span(1) * 0.5; // 4096
    let mut i = 0;
    while i < band.indices.len() {
        let a = band.positions[band.indices[i] as usize];
        let b = band.positions[band.indices[i + 1] as usize];
        let c = band.positions[band.indices[i + 2] as usize];
        let cx = (a.x + b.x + c.x) / 3.0;
        let cz = (a.z + b.z + c.z) / 3.0;
        let inside = cx.abs() < hole_half - 1e-6 && cz.abs() < hole_half - 1e-6;
        assert!(!inside, "triangle centroid ({cx},{cz}) fell inside the hole");
        i += 3;
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo test band_mesh`
Expected: FAIL to compile — `band_mesh` / `RingMesh` / `Vert3` not found.

- [ ] **Step 3: Write minimal implementation**

Add to `wg-10/rust/src/ring_geometry.rs`:

```rust
/// A plain XZ vertex (y filled by the shader at render time).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vert3 { pub x: f64, pub y: f64, pub z: f64 }

/// Flat mesh data for one ring band: shared positions + triangle indices into them.
pub struct RingMesh {
    pub positions: Vec<Vert3>,
    pub indices: Vec<u32>,
}

impl RingLayout {
    // (level_span / inner_hole_span / num_levels already defined in Task 1)
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo test ring_geometry`
Expected: PASS (Task 1's 3 + Task 2's 3 = 6 tests).

- [ ] **Step 5: Commit**

```powershell
git add wg-10/rust/src/ring_geometry.rs wg-10/rust/src/ring_geometry_tests.rs
git commit -m "feat(m3): ring_geometry::band_mesh (filled grid + hollow ring bands)

band_mesh builds a centered XZ vertex lattice (y=0, shader displaces) and emits two
triangles per kept cell; level 0 is a full grid, level L>0 drops cells whose center
falls in the hole -> a square annulus. Tested: vertex/index counts, centered+full
span, hollow center (no triangle centroid inside the hole).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: gdext Node3D probe — `Wg10ClipmapRings` registers + builds one child mesh

**Files:**
- Create: `wg-10/rust/src/clipmap_rings.rs`
- Modify: `wg-10/rust/src/lib.rs`

This task de-risks the one unknown: a `Node3D`-based gdext class that builds an
`ArrayMesh` from `ring_geometry` and parents a `MeshInstance3D` child. It is verified by
COMPILING + a minimal `configure` that builds the meshes (the windowed render is proven
in Task 6's gate). No new behavior beyond "class registers, builds N meshes as children."

- [ ] **Step 1: Declare the module in lib.rs**

In `wg-10/rust/src/lib.rs`, add `mod clipmap_rings;` after `mod ring_geometry;`.

- [ ] **Step 2: Write the minimal implementation**

Create `wg-10/rust/src/clipmap_rings.rs`:

```rust
//! Wg10ClipmapRings (DESIGN §5.1) — the godot owner of the N concentric clipmap ring
//! meshes. Builds one persistent MeshInstance3D + ShaderMaterial per level from
//! `ring_geometry`, recenters by quantized translate (never rebuilds), and rebinds each
//! level's resident page (coarser fallback when not resident) each frame. Owns NO page
//! RIDs — it only samples textures the pool owns.

use godot::prelude::*;
use godot::classes::{
    ArrayMesh, MeshInstance3D, ShaderMaterial, Shader, Node3D, INode3D,
    mesh::{ArrayType, PrimitiveType},
};
use crate::ring_geometry::{RingLayout, band_mesh, RingMesh};

#[derive(GodotClass)]
#[class(base=Node3D)]
pub struct Wg10ClipmapRings {
    levels: Vec<Gd<MeshInstance3D>>,
    num_levels: i32,
    base_span: f64,
    grid_res: i32,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for Wg10ClipmapRings {
    fn init(base: Base<Node3D>) -> Self {
        Self { levels: Vec::new(), num_levels: 0, base_span: 0.0, grid_res: 0, base }
    }
}

#[godot_api]
impl Wg10ClipmapRings {
    /// Build the N persistent ring meshes as children. `shader_path` is the res:// path to
    /// ring_displace.gdshader. Idempotent-ish: call once after instancing.
    #[func]
    pub fn configure(&mut self, num_levels: i64, base_span: f64, grid_res: i64, shader_path: GString) {
        self.num_levels = num_levels as i32;
        self.base_span = base_span;
        self.grid_res = grid_res as i32;
        let layout = RingLayout::new(self.num_levels, self.base_span);

        let shader: Gd<Shader> = match try_load::<Shader>(&shader_path.to_string()) {
            Ok(s) => s,
            Err(_) => { godot_error!("Wg10ClipmapRings: failed to load shader {shader_path}"); return; }
        };

        for level in 0..self.num_levels {
            let mesh = build_array_mesh(&layout, level, self.grid_res);
            let mut mi = MeshInstance3D::new_alloc();
            mi.set_mesh(&mesh);
            let mut mat = ShaderMaterial::new_gd();
            mat.set_shader(&shader);
            mi.set_material_override(&mat);
            // parent as a child of this Node3D
            self.base_mut().add_child(&mi);
            self.levels.push(mi);
        }
    }

    /// Number of level mesh instances built (for the gate).
    #[func]
    pub fn level_count(&self) -> i64 { self.levels.len() as i64 }

    /// Total vertex count across all level meshes (for the recenter-doesn't-rebuild check).
    #[func]
    pub fn total_vertex_count(&self) -> i64 {
        let mut total = 0i64;
        for mi in &self.levels {
            if let Some(mesh) = mi.get_mesh() {
                if let Ok(am) = mesh.try_cast::<ArrayMesh>() {
                    if am.get_surface_count() > 0 {
                        let arrays = am.surface_get_arrays(0);
                        // ARRAY_VERTEX is index 0; it's a PackedVector3Array
                        let verts: PackedVector3Array = arrays.at(0).to();
                        total += verts.len() as i64;
                    }
                }
            }
        }
        total
    }
}

/// Build a Godot ArrayMesh for one level from ring_geometry's vertex/index lists.
fn build_array_mesh(layout: &RingLayout, level: i32, grid_res: i32) -> Gd<ArrayMesh> {
    let rm: RingMesh = band_mesh(layout, level, grid_res);
    let mut verts = PackedVector3Array::new();
    for v in &rm.positions {
        verts.push(Vector3::new(v.x as f32, v.y as f32, v.z as f32));
    }
    let mut indices = PackedInt32Array::new();
    for i in &rm.indices {
        indices.push(*i as i32);
    }

    let mut arrays = VariantArray::new();
    arrays.resize(ArrayType::MAX.ord() as usize, &Variant::nil());
    arrays.set(ArrayType::VERTEX.ord() as usize, &verts.to_variant());
    arrays.set(ArrayType::INDEX.ord() as usize, &indices.to_variant());

    let mut mesh = ArrayMesh::new_gd();
    mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);
    mesh
}
```

NOTE for the implementer: the exact gdext spellings (`new_alloc` vs `new_gd`,
`base_mut().add_child`, `try_load`, `ArrayType::VERTEX.ord()`, `arrays.at(0).to()`,
`add_surface_from_arrays`) must compile against gdext 0.5.3 / godot api-4-6. If any spelling
differs in this version, adjust it to the form that compiles — search the existing crate
(`page_pool.rs`, `page_compute.rs`) for how it constructs godot objects, loads resources,
and reads Packed arrays, and match those. The INTENT is fixed: a Node3D that builds one
ArrayMesh per level (from `band_mesh`) and adds a MeshInstance3D child per level, with a
ShaderMaterial using `ring_displace.gdshader`. Keep the public funcs (`configure`,
`level_count`, `total_vertex_count`) and their signatures.

- [ ] **Step 3: Build to verify it compiles + registers**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo build`
Expected: builds clean (pre-existing dead-code warnings OK; no errors). If a gdext API
spelling fails, fix per the NOTE until it compiles.

- [ ] **Step 4: Run the full cargo suite (no regressions)**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo test`
Expected: all pass (96 prior + 6 ring_geometry = 102; clipmap_rings has no unit tests —
it's verified by compile here and the windowed gate in Task 6).

- [ ] **Step 5: Commit**

```powershell
git add wg-10/rust/src/clipmap_rings.rs wg-10/rust/src/lib.rs
git commit -m "feat(m3): Wg10ClipmapRings Node3D scaffold (builds N ring meshes)

First Node3D gdext class. configure() builds one persistent ArrayMesh per level from
ring_geometry::band_mesh and adds a MeshInstance3D child per level with a ShaderMaterial
using ring_displace.gdshader. level_count()/total_vertex_count() accessors for the gate.
Owns no page RIDs. Page binding + recenter + morph land in the next tasks.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: page binding + recenter on `Wg10ClipmapRings`

**Files:**
- Modify: `wg-10/rust/src/clipmap_rings.rs`

Add `recenter(camera_x, camera_z)` (quantized translate per level — no rebuild) and
`bind_page(level, height_tex, coarse_tex, level_span, height_scale, ...)` so GDScript can
hand each level its resident page texture and the coarser-neighbor texture for the morph.
Page-key selection / fallback lives in GDScript (the gate) using the streamer's coverage —
the rings just receive textures and place themselves. (This keeps `Wg10ClipmapRings` a
pure presenter: it owns meshes + transforms + uniform binding, not scheduling.)

- [ ] **Step 1: Add the methods**

Add to the `#[godot_api] impl Wg10ClipmapRings` block in `wg-10/rust/src/clipmap_rings.rs`:

```rust
    /// Recenter all level meshes on the camera by translating each level's transform,
    /// quantized to that level's CELL spacing so vertices stay locked to the world grid
    /// (no sub-cell swimming). Vertex buffers are untouched — never a rebuild.
    #[func]
    pub fn recenter(&mut self, camera_x: f64, camera_z: f64) {
        for (level, mi) in self.levels.iter_mut().enumerate() {
            let span = self.base_span * 2f64.powi(level as i32);
            let cell = span / self.grid_res as f64;
            // snap to cell grid
            let qx = (camera_x / cell).floor() * cell;
            let qz = (camera_z / cell).floor() * cell;
            let mut t = mi.get_transform();
            t.origin = Vector3::new(qx as f32, 0.0, qz as f32);
            mi.set_transform(t);
        }
    }

    /// Bind a level's height page (and its coarser neighbor, for the morph). Pass the SAME
    /// texture for both `height_tex` and `coarse_tex` to disable the morph for that level
    /// (e.g. the coarsest level, or a fallback frame). `level_span` is the band's world
    /// span; `morph_region` is the transition width as a fraction of the span.
    #[func]
    pub fn bind_page(
        &mut self,
        level: i64,
        height_tex: Gd<godot::classes::Texture2D>,
        coarse_tex: Gd<godot::classes::Texture2D>,
        level_span: f64,
        coarse_span: f64,
        height_scale: f64,
        morph_region: f64,
        relief_ref: f64,
    ) {
        let li = level as usize;
        if li >= self.levels.len() {
            godot_error!("Wg10ClipmapRings::bind_page: level {level} out of range");
            return;
        }
        let mi = &mut self.levels[li];
        let Some(mat_res) = mi.get_material_override() else {
            godot_error!("Wg10ClipmapRings::bind_page: level {level} has no material");
            return;
        };
        let Ok(mut mat) = mat_res.try_cast::<ShaderMaterial>() else {
            godot_error!("Wg10ClipmapRings::bind_page: material is not a ShaderMaterial");
            return;
        };
        mat.set_shader_parameter("height_tex", &height_tex.to_variant());
        mat.set_shader_parameter("coarse_height_tex", &coarse_tex.to_variant());
        mat.set_shader_parameter("world_span", &level_span.to_variant());
        mat.set_shader_parameter("coarse_span", &coarse_span.to_variant());
        mat.set_shader_parameter("height_scale", &height_scale.to_variant());
        mat.set_shader_parameter("morph_region", &morph_region.to_variant());
        mat.set_shader_parameter("relief_ref", &relief_ref.to_variant());
    }
```

Add the `ShaderMaterial` import if not already present (it is, from Task 3). The
`Texture2D` type is referenced via the full path `godot::classes::Texture2D`.

- [ ] **Step 2: Build to verify it compiles**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo build`
Expected: builds clean. (Adjust gdext spellings — `get_material_override`,
`set_shader_parameter`, `get_transform`/`set_transform` — to what compiles in 0.5.3 if
needed; match existing usage. `set_shader_parameter` takes `&Variant` for the value in this
version — verify against how the gate GDScript or any rust caller sets params; if it takes
a typed value, pass that.)

- [ ] **Step 3: Run the full cargo suite**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo test`
Expected: all pass (102, unchanged — no new unit tests; this is godot-side, gated in Task 6).

- [ ] **Step 4: Commit**

```powershell
git add wg-10/rust/src/clipmap_rings.rs
git commit -m "feat(m3): Wg10ClipmapRings recenter + bind_page

recenter() translates each level's transform quantized to that level's cell spacing
(vertices stay locked to the world grid; never a rebuild). bind_page() sets a level's
height_tex + coarse_height_tex (+ spans/scale/morph_region) so GDScript hands each level
its resident page and coarser-neighbor for the morph; same tex for both disables morph.
The rings are a pure presenter — scheduling/fallback selection stays in the caller.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: the geomorph in `ring_displace.gdshader`

**Files:**
- Modify: `wg-10/worldgen_terrain/shaders/ring_displace.gdshader`

Add the L↔L+1 morph: in each level's outer transition region, blend its own height toward
the coarser level's height so the seam matches. Backward-compatible: the new uniforms have
defaults that produce the slice-1 behavior when a caller binds only the base uniforms.

- [ ] **Step 1: Rewrite the shader**

Replace the entire contents of `wg-10/worldgen_terrain/shaders/ring_displace.gdshader` with:

```glsl
shader_type spatial;
render_mode unshaded, cull_disabled;

uniform sampler2D height_tex;            // this level's R32F page (pool Texture2DRD)
uniform sampler2D coarse_height_tex;     // next-coarser level's page (for the morph)
uniform float world_span = 8192.0;       // this level's band world span
uniform float coarse_span = 8192.0;      // the coarser level's band world span
uniform float height_scale = 1.0;        // visual amplitude (config; 1.0 = raw metres)
uniform float morph_region = 0.0;        // transition width as a fraction of span (0 = no morph)
uniform float relief_ref = 2000.0;       // color gradient normalization

varying float v_height;

// World XZ of this vertex = mesh-local XZ + the instance origin (rings recenter by
// translating the instance). MODEL_MATRIX maps local -> world.
void vertex() {
	vec3 world = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;

	// This level samples its own page by local UV: local XZ in [-span/2, span/2] -> [0,1].
	vec2 uv_fine = (VERTEX.xz / world_span) + vec2(0.5);
	float h_fine = texture(height_tex, uv_fine).r;

	// Morph factor: 0 in the interior, rising to 1 at the outer edge over the transition
	// region. Use the Chebyshev (max-of-axes) distance from the level center, so the
	// transition is a square band matching the square ring edge. edge_t in [0,1] where 1
	// is the outer edge.
	float half_span = world_span * 0.5;
	float cheb = max(abs(VERTEX.x), abs(VERTEX.z)) / half_span; // 0 center .. 1 outer edge
	float region = max(morph_region, 1e-6);
	float t = clamp((cheb - (1.0 - region)) / region, 0.0, 1.0); // 0 until inner edge of region

	// Coarser sample at the SAME world position (coarser page is centered on the camera
	// too; convert world XZ to the coarser band's local UV).
	vec2 uv_coarse = (world.xz / coarse_span) + vec2(0.5);
	float h_coarse = texture(coarse_height_tex, uv_coarse).r;

	float h = mix(h_fine, h_coarse, t);
	v_height = h;
	VERTEX.y = h * height_scale;
}

void fragment() {
	float t = clamp(v_height / relief_ref * 0.5 + 0.5, 0.0, 1.0);
	ALBEDO = vec3(t, t * 0.8 + 0.1, 1.0 - t);
}
```

Notes baked into the design:
- When `morph_region = 0.0` (default) → `t = clamp(.../1e-6,...)` is 0 for any `cheb < 1`
  and only nonzero in the outermost sliver; binding `coarse_height_tex = height_tex` and
  `coarse_span = world_span` makes `h_coarse == h_fine`, so `mix` is a no-op → **identical
  to slice 1**. Slice-1/2 checks (which set only `height_tex`/`world_span`/`height_scale`/
  `relief_ref`, leaving `coarse_height_tex` unbound) get Godot's default sampler — to be
  safe the slice-1/2 gates already pass `morph_region` unset (0.0) so `t≈0`; the only risk
  is an unbound `coarse_height_tex` sampling black, but with `t≈0` it's never mixed in.
  Task 6 re-runs the full m3 suite to confirm slice-1/2 still pass.
- The coarser sample uses **world** XZ (via `MODEL_MATRIX`) so the fine and coarse samples
  agree at the same world point — that agreement is what closes the seam.

- [ ] **Step 2: Sanity-check the shader parses (build the project import)**

Run (project root, GODOT_BIN set):
```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
& $env:GODOT_BIN --headless --import --path "D:\workflows\worldgen10\wg-10" 2>&1 | Select-String -Pattern "ERROR|SHADER|ring_displace"
```
Expected: no shader parse error referencing ring_displace. (If you can't run Godot, leave
this to the controller and note it; the shader is fully exercised by Task 6's gate.)

- [ ] **Step 3: Commit**

```powershell
git add wg-10/worldgen_terrain/shaders/ring_displace.gdshader
git commit -m "feat(m3): geomorph in ring_displace (L<->L+1 seam blend)

In each level's outer transition region (square Chebyshev band of width morph_region),
blend this level's height toward the next-coarser page's height at the same WORLD position
(mix(h_fine,h_coarse,t), t=1 at the outer edge) so adjacent levels agree on the seam ->
no crack, no pop. Backward-compatible: morph_region=0 + coarse_tex==height_tex reproduces
the slice-1 displacement exactly. Coarser sample uses MODEL_MATRIX world XZ.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: `m3_rings_check.gd` windowed gate + wire into gate.py

**Files:**
- Create: `wg-10/worldgen_terrain/tests/m3_rings_check.gd`
- Modify: `tools/gate.py`

The gate assembles `Wg10ClipmapRings` fed by `Wg10PagePool` (+ a streamer at
`radius_pages=0`), renders top-down orthographic, and asserts no holes / real relief / seam
continuity / morph-math / recenter-doesn't-rebuild. WINDOWED; SKIP code 2 with no render
device. The controller runs the windowed gate.

- [ ] **Step 1: Add the check to the m3 suite in gate.py**

In `tools/gate.py`, the `"m3"` list currently has 3 entries (slice1, pool, stream). Add a
4th:

```python
    "m3": [
        "worldgen_terrain/tests/m3_slice1_check.gd",
        "worldgen_terrain/tests/m3_pool_check.gd",
        "worldgen_terrain/tests/m3_stream_check.gd",
        "worldgen_terrain/tests/m3_rings_check.gd",
    ],
```

- [ ] **Step 2: Write the gate check**

Create `wg-10/worldgen_terrain/tests/m3_rings_check.gd` (TAB indentation, matching the
other gates). It builds a 2-level ring assembly, binds real DEM pages per level (level 0
its own page; level 1 the coarser, with level 0 morphing toward it), renders top-down
ortho, and asserts the five things. Page selection uses the pool directly at the two levels'
origins for the camera at world origin (the simplest correct binding for a static capture;
the scheduler/streamer drive selection under motion, proven in slice 3 — here we prove the
RING render + morph, so we bind pages deterministically).

```gdscript
extends SceneTree

# M3 slice 4 gate: assemble Wg10ClipmapRings, bind real DEM pages per level, render
# TOP-DOWN ORTHO, and assert: (1) no holes (nonblack ~1 over terrain), (2) real relief
# (distinct colors), (3) seam continuity across the level-0/level-1 boundary (no crack),
# (4) morph math (finer edge height == coarser surface within eps), (5) recenter doesn't
# rebuild (vertex count unchanged after a camera move). Saves m3_rings.png. WINDOWED.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE    := "terrain_pack.gate.json"
const GLSL         := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER       := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const PAGE_PX      := 256
const SEED         := 1337
const NUM_LEVELS   := 2
const BASE_SPAN    := 8192.0
const GRID_RES     := 64
const MORPH_REGION := 0.15
const HEIGHT_SCALE := 0.35
const RELIEF_REF   := 2000.0
const CAPACITY     := 8
const VIEW_SIZE    := Vector2i(512, 512)
const MIN_DISTINCT := 8

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10ClipmapRings"):
		push_error("Wg10ClipmapRings not registered"); return 1
	if not ClassDB.class_exists("Wg10PagePool"):
		push_error("Wg10PagePool not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-m3-rings] status=skip reason=no-render-device"); return 2

	var pack_os: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os: String = ProjectSettings.globalize_path(GLSL)

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var cfg_err: String = str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if cfg_err != "":
		push_error("pool configure failed: %s" % cfg_err); return 1

	# Acquire one page per level at the camera origin. Level L span = BASE_SPAN*2^L; the
	# page for level L at origin has origin (0,0) in that level's grid.
	var tex0 = pool.call("acquire_page", 0, 0.0, 0.0)
	var tex1 = pool.call("acquire_page", 1, 0.0, 0.0)
	if tex0 == null or tex1 == null:
		push_error("acquire_page returned null"); return 1

	# Build the rings node.
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	if int(rings.call("level_count")) != NUM_LEVELS:
		push_error("expected %d levels, got %s" % [NUM_LEVELS, str(rings.call("level_count"))]); return 1

	# Bind pages: level 0 morphs toward level 1 (coarse); level 1 is coarsest -> no morph
	# (coarse_tex = its own tex, morph_region 0).
	rings.call("bind_page", 0, tex0, tex1, BASE_SPAN, BASE_SPAN * 2.0, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF)
	rings.call("bind_page", 1, tex1, tex1, BASE_SPAN * 2.0, BASE_SPAN * 2.0, HEIGHT_SCALE, 0.0, RELIEF_REF)

	# (5a) record vertex count before recenter
	var verts_before := int(rings.call("total_vertex_count"))

	# SubViewport + top-down ortho camera.
	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true

	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.0, 0.0, 0.0)   # BLACK bg so holes read as black

	var cam := Camera3D.new()
	cam.projection = Camera3D.PROJECTION_ORTHOGONAL
	cam.size = BASE_SPAN * 2.0            # ortho height = coarsest span (frame fills with terrain)
	cam.far = BASE_SPAN * 8.0
	cam.environment = env

	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-90.0, 0.0, 0.0)  # straight down
	light.light_energy = 1.2

	vp.add_child(rings)
	vp.add_child(cam)
	vp.add_child(light)
	get_root().add_child(vp)

	# top-down: eye above origin looking straight down (-Y); up = -Z so +X is right.
	cam.look_at_from_position(Vector3(0.0, BASE_SPAN * 2.0, 0.0), Vector3.ZERO, Vector3(0.0, 0.0, -1.0))

	rings.call("recenter", 0.0, 0.0)

	for i in range(8):
		await process_frame
	RenderingServer.force_draw()
	await process_frame

	var img := vp.get_texture().get_image()
	if img == null:
		push_error("get_image() returned null"); return 1

	# Save PNG for eyeball confirmation.
	img.save_png("user://m3_rings.png")

	var errs: Array[String] = []

	# (1) no holes + (2) real relief: over the inner region (the terrain disc), nonblack
	# frac high and distinct colors many. Background is black, terrain is colored.
	var distinct := {}
	var nonblack := 0
	var total := img.get_width() * img.get_height()
	for y in range(img.get_height()):
		for x in range(img.get_width()):
			var c := img.get_pixel(x, y)
			if not (is_finite(c.r) and is_finite(c.g) and is_finite(c.b)):
				push_error("non-finite pixel @ %d,%d" % [x,y]); return 1
			if c.r > 0.03 or c.g > 0.03 or c.b > 0.03:
				nonblack += 1
			distinct[Vector3i(int(c.r*16), int(c.g*16), int(c.b*16))] = true
	var frac := float(nonblack) / float(total)
	# Top-down ortho framed to the coarsest span: terrain fills ~the whole frame, so a
	# crack/hole would appear as black pixels INSIDE the terrain. Require high coverage.
	if frac < 0.95:
		errs.append("holes: nonblack_frac=%.3f < 0.95 (gap/crack shows as black)" % frac)
	if distinct.size() < MIN_DISTINCT:
		errs.append("no relief: distinct=%d < %d" % [distinct.size(), MIN_DISTINCT])

	# (3) seam continuity: scan a horizontal line through the center; the level-0/level-1
	# boundary is at |x| = BASE_SPAN/2 in world = at 1/4 and 3/4 across the frame (frame
	# spans the coarsest span = 2*BASE_SPAN). Assert no black gap at those boundaries.
	var midy := img.get_height() / 2
	var boundary_cols := [img.get_width()/4, (img.get_width()*3)/4]
	for bx in boundary_cols:
		var black_run := 0
		for dx in range(-2, 3):
			var c := img.get_pixel(int(clamp(bx + dx, 0, img.get_width()-1)), midy)
			if c.r <= 0.03 and c.g <= 0.03 and c.b <= 0.03:
				black_run += 1
		if black_run > 0:
			errs.append("seam crack: %d black px at boundary col %d (level-0/1 seam)" % [black_run, bx])

	# (4) morph math: at the level-0 outer boundary, the morphed fine height must equal the
	# coarse surface. Sample both pool pages at a boundary world point and compare. Use the
	# pool's CPU-readable path is not available, so verify via the rendered seam instead:
	# require the two pixels straddling the boundary to have CLOSE color (continuous height
	# -> continuous color), not a hard jump. (Color is a monotonic function of height.)
	for bx in boundary_cols:
		var c_in := img.get_pixel(int(clamp(bx - 2, 0, img.get_width()-1)), midy)
		var c_out := img.get_pixel(int(clamp(bx + 2, 0, img.get_width()-1)), midy)
		var dr: float = abs(c_in.r - c_out.r) + abs(c_in.g - c_out.g) + abs(c_in.b - c_out.b)
		if dr > 0.5:
			errs.append("morph discontinuity: color jump %.2f across boundary col %d" % [dr, bx])

	# (5) recenter doesn't rebuild: move camera, recenter, vertex count unchanged + still renders.
	rings.call("recenter", 3000.0, -1500.0)
	var verts_after := int(rings.call("total_vertex_count"))
	if verts_after != verts_before:
		errs.append("recenter rebuilt mesh: verts %d -> %d" % [verts_before, verts_after])

	pool.call("free_all")

	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m3-rings] status=fail errors=%d nonblack=%.3f distinct=%d" % [errs.size(), frac, distinct.size()])
		return 1
	print("[wg10-m3-rings] status=pass nonblack=%.3f distinct=%d verts=%d" % [frac, distinct.size(), verts_before])
	return 0
```

NOTE for the implementer / controller: the geometric specifics (where the level boundary
lands in pixels, the ortho framing, the up-vector for top-down) are best-effort and MUST be
validated by actually running the windowed gate and inspecting `m3_rings.png`. If the
boundary columns or framing are off, adjust the camera `size`/position and the
`boundary_cols` math so the assertions target the real seam — but do NOT weaken the
thresholds to force a pass. If a real crack appears, that is a true finding (fix the shader/
geometry, like slice 3's never-black finding). Save the PNG and eyeball it.

- [ ] **Step 3: Build the crate, then run the m3 gate (CONTROLLER runs windowed)**

```powershell
$env:CARGO_TARGET_DIR=$null
Push-Location wg-10/rust; cargo build; Pop-Location
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools/gate.py --suite m3
```
Expected: `[wg10-m3-rings] status=pass ...` and `[gate] suite=m3 checks=4 fail=0`. The
slice-1/2/3 checks must still pass (regression guard — especially that the shader change
didn't break slice 1/2). Inspect `user://m3_rings.png` (under the Godot user data dir) by
eye for real seamless relief.

- [ ] **Step 4: Run fast + gpu suites (no regressions)**

```powershell
python tools/gate.py --suite fast
python tools/gate.py --suite gpu
```
Expected: `suite=fast checks=5 fail=0`, `suite=gpu checks=2 fail=0 skip=0`.

- [ ] **Step 5: Commit**

```powershell
git add wg-10/worldgen_terrain/tests/m3_rings_check.gd tools/gate.py
git commit -m "test(m3): m3_rings_check windowed gate + wire into m3 suite

Assembles Wg10ClipmapRings (2 levels) fed by real DEM pages, renders top-down ortho on a
black bg, asserts: no holes (nonblack>=0.95), real relief (distinct colors), seam
continuity (no black gap at the level-0/1 boundary), morph continuity (no hard color jump
across the seam), and recenter-doesn't-rebuild (vertex count unchanged after a camera move).
Saves m3_rings.png. m3 suite now 4 checks.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Update STATUS + ROADMAP (rings done)

**Files:**
- Modify: `docs/plans/STATUS.md`
- Modify: `docs/plans/ROADMAP.md`

- [ ] **Step 1: Update STATUS.md**

In `docs/plans/STATUS.md`: update the `Last updated:` line and the **Phase** line to record
slice 4 done; add slice-4 bullets to "Current state" + "What works" mirroring the slice-3
bullets' style (name the files: `ring_geometry.rs`, `clipmap_rings.rs`, `m3_rings_check.gd`;
what's proven: hollow ring-band geometry, one-band-one-page binding, quantized recenter
without rebuild, L↔L+1 geomorph, gate asserts no-holes/relief/seam-continuity/morph/recenter;
m3 suite now 4 checks fail=0; cargo count up by 6 ring_geometry tests). Update the gate-runner
line to `--suite m3 ... checks=4`. Move "What's next" item 1 to the **fly-test harness**
slice (WASD/mouse fly camera + diagnostics/UI overlay + the renderer p99<6ms acceptance gate
at ~1000 m/s + manual fly). Keep the honest-baseline note: this slice proves geometry-seamless
+ recenter-cheap under a SCRIPTED move, NOT the perf target or interactive flight.

- [ ] **Step 2: Update ROADMAP.md**

In `docs/plans/ROADMAP.md`: update `Last updated:`; flip the `clipmap_rings` milestone-3
bullet from `[ ]` to `[x]` with a one-line summary (Wg10ClipmapRings + ring_geometry +
geomorph + m3_rings gate; concentric hollow bands, one-band-one-page, quantized recenter,
L↔L+1 morph; m3 suite 4 checks fail=0). Leave the remaining M3 bullets (harness, fly-test,
acceptance gate, tuning, manual acceptance) as `[ ]`.

- [ ] **Step 3: Verify the gate counts quoted are real (fresh evidence)**

Run and copy the ACTUAL numbers into the docs (do not guess):
```powershell
$env:CARGO_TARGET_DIR=$null; Push-Location wg-10/rust; cargo test 2>&1 | Select-String "test result"; Pop-Location
python tools/gate.py --suite m3
```
Expected: a `test result: ok. N passed` line and `suite=m3 checks=4 fail=0`. Use those exact
numbers in STATUS/ROADMAP.

- [ ] **Step 4: Commit**

```powershell
git add docs/plans/STATUS.md docs/plans/ROADMAP.md
git commit -m "docs(m3): slice 4 (clipmap rings) done — STATUS + ROADMAP

Wg10ClipmapRings + ring_geometry + geomorph + m3_rings gate landed; concentric hollow
ring-bands, one-band-one-page binding, quantized recenter (no rebuild), L<->L+1 morph
(crack-free seams). m3 suite 4 checks fail=0; fast 5, gpu 2 unchanged; cargo green.
clipmap_rings roadmap item flipped to done; next M3 slice is the fly-test harness +
diagnostics overlay + the p99<6ms acceptance gate.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage:**
- Spec §1 scope (rings/binding/recenter/morph in; fly-cam/overlay/perf deferred) → Tasks 1–7; deferral recorded in Task 7 docs. ✓
- Spec §2.1 hollow ring-band geometry (level L side `2^L`, hole = inner level span) → Task 1 (`RingLayout`) + Task 2 (`band_mesh` hollow center). ✓
- Spec §2.2 one-band-one-page binding (band span = page span, reuse slice-1 sample) → Task 4 `bind_page` + Task 6 gate binds one page/level; shader samples by local UV (Task 5). ✓
- Spec §2.3 rings consume pool textures, own no RIDs → Task 3/4 (`Wg10ClipmapRings` holds MeshInstance3D/materials only; binds textures, never creates/frees RIDs). ✓
- Spec §2.4 `Wg10ClipmapRings` owner (configure/recenter/stats) → Tasks 3–4 (`configure`/`recenter`/`bind_page`/`level_count`/`total_vertex_count`). *Note: spec said a `stats()`; the plan exposes `level_count`/`total_vertex_count` which serve the gate's needs — `stats()` as a Dictionary is deferred to the harness slice where the overlay consumes it (YAGNI now). Recorded below.* ✓ (with noted refinement)
- Spec §3.1 quantized recenter, no rebuild → Task 4 `recenter` + Task 6 assertion (5). ✓
- Spec §3.2 geomorph (transition region, mix toward coarser, t=1 at seam) → Task 5 shader. ✓
- Spec §3.3 shader backward-compat (no-morph default) → Task 5 notes + Task 6 re-runs full m3 suite. ✓
- Spec §3.4 config no-magic-numbers → all tunables are configure/bind args (Tasks 3–4). ✓
- Spec §4 gate (no holes / relief / seam continuity / morph math / recenter-no-rebuild + PNG) → Task 6 assertions 1–5 + `save_png`. ✓
- Spec §5 files → match (ring_geometry.rs, clipmap_rings.rs, m3_rings_check.gd new; ring_displace.gdshader, lib.rs, gate.py modified). `ring_mesh.rs` split listed as optional in spec; plan keeps band_mesh in ring_geometry.rs (under cap) — consistent with "split only if needed." ✓
- Spec §6 done + §7 risks → Task 6 (full-suite regression for shader compat; morph-math + seam checks are the two independent crack checks) + Task 7 docs. ✓

**Refinements from grounding (within spec intent, no spec change needed):**
- The spec's §4 "morph math (CPU-side)" assertion: the pool pages are GPU R32F textures with no CPU read path exposed (no readback API on Wg10PagePool — by design, no-readback). So the plan implements the morph check as a *rendered* seam-continuity assertion (no hard color jump across the boundary, color being monotonic in height) PLUS the seam-no-black-gap check — two independent rendered checks of the same seam. This still proves "the morph closes the seam" without adding a readback path that the no-readback pillar forbids. Documented in Task 6. A true CPU morph-math unit test would require a readback the architecture deliberately doesn't have.
- `Wg10ClipmapRings` is `Node3D` (spec §2.4) — Task 3 probes the gdext Node3D pattern first since it's the first non-RefCounted class in the crate.
- `stats()` Dict deferred to the harness slice (YAGNI); `level_count`/`total_vertex_count` cover this slice's gate needs.

**2. Placeholder scan:** No TBD/TODO/"handle edge cases"/"similar to Task N". Every code step has complete code. The gate's geometric specifics carry an explicit "validate by running windowed + adjust framing, do NOT weaken thresholds" instruction (the honest equivalent of "tune the capture", not a placeholder). ✓

**3. Type consistency:** `RingLayout` (`new`/`level_span`/`inner_hole_span`/`num_levels`) used identically across Tasks 1–3. `band_mesh(&RingLayout, level: i32, grid_res: i32) -> RingMesh { positions: Vec<Vert3>, indices: Vec<u32> }` consistent in Tasks 2–3. `Wg10ClipmapRings` funcs (`configure(num_levels, base_span, grid_res, shader_path)`, `bind_page(level, height_tex, coarse_tex, level_span, coarse_span, height_scale, morph_region, relief_ref)`, `recenter(camera_x, camera_z)`, `level_count`, `total_vertex_count`) consistent across Tasks 3–4 and the Task 6 gate calls. Shader uniforms (`height_tex`, `coarse_height_tex`, `world_span`, `coarse_span`, `height_scale`, `morph_region`, `relief_ref`) match between Task 5 (shader) and Task 4 (`bind_page` set_shader_parameter calls) and Task 6 (gate). ✓
