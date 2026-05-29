# WorldGen10 M3 Slice 5b — 3×3 Ring Tiling + Live Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make each clipmap level render a 3×3 page neighborhood that surrounds the camera (N levels × 9 page tiles), wire it to the streamer in a live loop via the read-only `get_resident_page` accessor, and prove coverage-surrounds-camera + seamless + never-black under scripted motion.

**Architecture:** `Wg10ClipmapRings` is rebuilt to hold N×9 persistent `MeshInstance3D` tiles (each a full one-page grid, its own `ShaderMaterial`, `render_priority` by level so finer draws on top); `bind_tile(level,dx,dz,…)` places + binds one tile. `Wg10TerrainView` (recreated) ticks the live loop: `streamer.update` → per level per tile fetch the page via read-only `get_resident_page` (coarser fallback on miss, never computes) → `bind_tile`. Levels overlap (coarse full 3×3, finer on top, geomorph blends at the finer's outer edge) — gapless by construction, bounded fixed overdraw. All four (scheduler/pool/rings/view) share the page-key convention `origin = floor(cam/span)·span`.

**Tech Stack:** Rust (gdext 0.5.3, godot api-4-6), Godot 4.6 spatial shader + ArrayMesh, GDScript SceneTree windowed gate via `tools/gate.py`.

---

## Conventions (read before Task 1)

- **Build/test** from `wg-10/rust` with `CARGO_TARGET_DIR` UNSET: Bash `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo build` / `... cargo test <one-filter>`.
- **Windowed gate** — the CONTROLLER runs it; a subagent that can't run windowed Godot builds + reports DONE. `python tools/gate.py --suite m3` (GODOT_BIN set).
- **Commit trailer:** `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`. Stay on branch `main`.
- gdext idioms in this crate: Node-derived `::new_alloc()`, Resource `::new_gd()`,
  `self.base_mut().add_child(&mi)`, `mi.set_mesh(&mesh)`, `mi.set_material_override(&mat)`,
  `mi.set_transform(t)` / `mi.get_transform()` with `t.origin: Vector3`,
  `mat.set_shader_parameter("name", &val.to_variant())`, `mat.set_render_priority(i32)`,
  untyped array is `VarArray`, `try_load::<Shader>(&gstring)`. `Gd` cloned before `bind_mut()`.
- **Page-key convention (shared by all):** `origin = floor(cam/span)·span` per axis,
  `span_L = base_span·2^L`. A page covers world `[origin, origin+span]`. A tile's centered
  mesh covers that when its instance origin = `origin + span/2`.

---

## File Structure

**Rewrite:** `wg-10/rust/src/clipmap_rings.rs` — N×9 tiles, `bind_tile`, per-level
`render_priority`, accessors (`level_count`, `tile_count`, `total_vertex_count`,
`bound_page_key`). Per-tile meshes (no sharing).
**Recreate:** `wg-10/rust/src/terrain_view.rs` — the 3×3 live loop. `mod terrain_view;` in lib.rs.
**Create:** `wg-10/worldgen_terrain/tests/m3_view_check.gd` — moving-sweep gate.
**Modify:** `tools/gate.py` — m3 suite: +m3_view_check, −m3_rings_check.
**Remove:** `wg-10/worldgen_terrain/tests/m3_rings_check.gd` (+.uid).
**Unchanged (reused from 5a):** `page_pool.rs` (per-level span + `get_resident_page`),
`page_policy.rs` (`slot_of`), `ring_displace.gdshader` (`coarse_origin`).

---

## Task 1: Rebuild `Wg10ClipmapRings` to N×9 tiles + `bind_tile`

**Files:**
- Rewrite: `wg-10/rust/src/clipmap_rings.rs`

Replace the per-level single-mesh structure with N×9 tiles. Verified by compile + the Task 3
windowed gate (no Rust unit tests — it's a godot binding).

- [ ] **Step 1: Rewrite the file**

Replace the ENTIRE contents of `wg-10/rust/src/clipmap_rings.rs` with the following. ADAPT
gdext spellings to what compiles (the crate already uses all of these forms — see the current
file + page_pool.rs), keeping the public API + intent:

```rust
//! Wg10ClipmapRings (DESIGN §5.1) — the godot owner of the clipmap ring meshes. Each level
//! is a 3x3 neighborhood of one-page TILES (9 MeshInstance3D per level) so the level
//! surrounds the camera. Each tile is a full grid spanning one page, with its own
//! ShaderMaterial(ring_displace). Levels OVERLAP: the coarse level keeps its full 3x3, the
//! finer level's 3x3 draws on top (render_priority by level), and the geomorph blends at the
//! finer's outer edge — gapless by construction. `bind_tile` places + binds one tile each
//! frame. Persistent: tiles are created once, never rebuilt (only transform + uniforms
//! change). Owns NO page RIDs — it only samples textures the pool owns.

use godot::prelude::*;
use godot::classes::{
    ArrayMesh, MeshInstance3D, ShaderMaterial, Shader, INode3D,
    mesh::{ArrayType, PrimitiveType},
};
use crate::ring_geometry::{RingLayout, band_mesh, RingMesh};

/// Tiles per level: a 3x3 neighborhood (radius 1).
const TILES_PER_LEVEL: usize = 9;

/// Flat tile index from (level, dx, dz) with dx,dz in {-1,0,+1}.
fn tile_index(level: i32, dx: i32, dz: i32) -> usize {
    (level as usize) * TILES_PER_LEVEL + ((dz + 1) as usize) * 3 + (dx + 1) as usize
}

#[derive(GodotClass)]
#[class(base=Node3D)]
pub struct Wg10ClipmapRings {
    tiles: Vec<Gd<MeshInstance3D>>,     // num_levels * 9, indexed by tile_index
    bound_keys: Vec<(i64, i64)>,        // per-tile last bound page origin (for the gate)
    num_levels: i32,
    base_span: f64,
    grid_res: i32,
    built_vertex_count: i64,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for Wg10ClipmapRings {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            tiles: Vec::new(),
            bound_keys: Vec::new(),
            num_levels: 0,
            base_span: 0.0,
            grid_res: 0,
            built_vertex_count: 0,
            base,
        }
    }
}

#[godot_api]
impl Wg10ClipmapRings {
    /// Build N levels x 9 persistent tile meshes. `shader_path` = res:// to ring_displace.
    /// Each tile is a full grid spanning one page at its level's span. render_priority is
    /// derived from level (finest on top) so the finer level wins in the overlap.
    #[func]
    pub fn configure(&mut self, num_levels: i64, base_span: f64, grid_res: i64, shader_path: GString) {
        if !self.tiles.is_empty() {
            godot_error!("Wg10ClipmapRings::configure called more than once — ignoring");
            return;
        }
        if grid_res < 1 || grid_res % 4 != 0 {
            godot_error!("Wg10ClipmapRings: grid_res must be >= 1 and divisible by 4, got {grid_res}");
            return;
        }
        self.num_levels = num_levels as i32;
        self.base_span = base_span;
        self.grid_res = grid_res as i32;
        self.built_vertex_count = 0;
        let layout = RingLayout::new(self.num_levels, self.base_span);

        let shader: Gd<Shader> = match try_load::<Shader>(&shader_path) {
            Ok(s) => s,
            Err(_) => { godot_error!("Wg10ClipmapRings: failed to load shader {shader_path}"); return; }
        };

        // Pre-size the flat tile + bound-key vectors.
        let total = (self.num_levels as usize) * TILES_PER_LEVEL;
        self.bound_keys = vec![(i64::MIN, i64::MIN); total];

        for level in 0..self.num_levels {
            // Finest (level 0) draws last/on-top -> highest priority. render_priority is i32.
            let priority = (self.num_levels - 1 - level) as i32;
            for dz in -1..=1 {
                for dx in -1..=1 {
                    // A full-grid tile spanning one page at this level. band_mesh with the
                    // level's own index gives a full grid (no hole only at level 0); to get
                    // a FULL grid at any level's SPAN, build it as a level-0 grid scaled to
                    // this level's span via a fresh RingLayout whose base_span = span_L.
                    let span_l = self.base_span * 2f64.powi(level);
                    let tile_layout = RingLayout::new(1, span_l);   // 1 level, full grid at span_l
                    let rm: RingMesh = band_mesh(&tile_layout, 0, self.grid_res);
                    self.built_vertex_count += rm.positions.len() as i64;
                    let mesh = build_array_mesh(&rm);

                    let mut mi = MeshInstance3D::new_alloc();
                    mi.set_mesh(&mesh);
                    let mut mat = ShaderMaterial::new_gd();
                    mat.set_shader(&shader);
                    mat.set_render_priority(priority);
                    mi.set_material_override(&mat);
                    self.base_mut().add_child(&mi);
                    // store at the flat index (build order = tile_index order)
                    self.tiles.push(mi);
                    let _ = (dx, dz); // index is push-order; tile_index mirrors this loop order
                }
            }
        }
    }

    #[func]
    pub fn level_count(&self) -> i64 { self.num_levels as i64 }

    #[func]
    pub fn tile_count(&self) -> i64 { self.tiles.len() as i64 }

    /// Total vertex count across all tiles (recenter-no-rebuild check). Falls back to the
    /// build-time count if surface read-back is unavailable.
    #[func]
    pub fn total_vertex_count(&self) -> i64 {
        let mut total = 0i64;
        let mut read_any = false;
        for mi in &self.tiles {
            if let Some(mesh) = mi.get_mesh() {
                if let Ok(am) = mesh.try_cast::<ArrayMesh>() {
                    if am.get_surface_count() > 0 {
                        let arrays = am.surface_get_arrays(0);
                        let verts: PackedVector3Array = arrays.at(0).to();
                        total += verts.len() as i64;
                        read_any = true;
                    }
                }
            }
        }
        if read_any { total } else { self.built_vertex_count }
    }

    /// The last page origin bound to tile (level,dx,dz), as a Vector2i (for the gate's
    /// tile<->page mapping assertion). Returns (i32::MIN,i32::MIN) cast if never bound.
    #[func]
    pub fn bound_page_key(&self, level: i64, dx: i64, dz: i64) -> Vector2i {
        let idx = tile_index(level as i32, dx as i32, dz as i32);
        if idx >= self.bound_keys.len() {
            return Vector2i::new(i32::MIN, i32::MIN);
        }
        let (ox, oz) = self.bound_keys[idx];
        Vector2i::new(ox as i32, oz as i32)
    }

    /// Place + bind one tile. The tile's centered mesh is translated to `page_origin +
    /// span_l/2` so it covers world [page_origin, page_origin+span_l]; the material uniforms
    /// are set (incl. coarse_origin for the morph). Never rebuilds geometry.
    #[func]
    pub fn bind_tile(
        &mut self,
        level: i64,
        dx: i64,
        dz: i64,
        height_tex: Gd<godot::classes::Texture2D>,
        coarse_tex: Gd<godot::classes::Texture2D>,
        span_l: f64,
        coarse_span: f64,
        height_scale: f64,
        morph_region: f64,
        relief_ref: f64,
        page_origin_x: f64,
        page_origin_z: f64,
        coarse_origin_x: f64,
        coarse_origin_z: f64,
    ) {
        let idx = tile_index(level as i32, dx as i32, dz as i32);
        if idx >= self.tiles.len() {
            godot_error!("Wg10ClipmapRings::bind_tile: ({level},{dx},{dz}) out of range");
            return;
        }
        // place
        {
            let mi = &mut self.tiles[idx];
            let mut t = mi.get_transform();
            t.origin = Vector3::new(
                (page_origin_x + span_l * 0.5) as f32,
                0.0,
                (page_origin_z + span_l * 0.5) as f32,
            );
            mi.set_transform(t);
        }
        // bind uniforms
        let mi = &mut self.tiles[idx];
        let Some(mat_res) = mi.get_material_override() else {
            godot_error!("Wg10ClipmapRings::bind_tile: tile has no material"); return;
        };
        let Ok(mut mat) = mat_res.try_cast::<ShaderMaterial>() else {
            godot_error!("Wg10ClipmapRings::bind_tile: material is not a ShaderMaterial"); return;
        };
        mat.set_shader_parameter("height_tex", &height_tex.to_variant());
        mat.set_shader_parameter("coarse_height_tex", &coarse_tex.to_variant());
        mat.set_shader_parameter("world_span", &span_l.to_variant());
        mat.set_shader_parameter("coarse_span", &coarse_span.to_variant());
        mat.set_shader_parameter("height_scale", &height_scale.to_variant());
        mat.set_shader_parameter("morph_region", &morph_region.to_variant());
        mat.set_shader_parameter("relief_ref", &relief_ref.to_variant());
        let co = Vector2::new(coarse_origin_x as f32, coarse_origin_z as f32);
        mat.set_shader_parameter("coarse_origin", &co.to_variant());
        self.bound_keys[idx] = (page_origin_x as i64, page_origin_z as i64);
    }
}

/// Build a Godot ArrayMesh from ring_geometry's vertex/index lists.
fn build_array_mesh(rm: &RingMesh) -> Gd<ArrayMesh> {
    let mut verts = PackedVector3Array::new();
    for v in &rm.positions {
        verts.push(Vector3::new(v.x as f32, v.y as f32, v.z as f32));
    }
    let mut indices = PackedInt32Array::new();
    for i in &rm.indices {
        indices.push(*i as i32);
    }
    let mut arrays = VarArray::new();
    arrays.resize(ArrayType::MAX.ord() as usize, &Variant::nil());
    arrays.set(ArrayType::VERTEX.ord() as usize, &verts.to_variant());
    arrays.set(ArrayType::INDEX.ord() as usize, &indices.to_variant());
    let mut mesh = ArrayMesh::new_gd();
    mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);
    mesh
}
```

NOTE for the implementer:
- `band_mesh(&RingLayout::new(1, span_l), 0, grid_res)` builds a FULL grid spanning `span_l`
  centered at origin (level 0 of a 1-level layout has no hole). This is the per-tile mesh.
  Confirm `band_mesh` is `pub` and takes `(&RingLayout, level: i32, grid_res: i32)` (it is).
- `mat.set_render_priority(i32)` — verify the gdext spelling (`set_render_priority`); if it
  differs, find the right setter on `ShaderMaterial`/`Material` (`render_priority` is a
  Material property). If it genuinely isn't exposed, fall back to setting it via
  `mi.set_sorting_offset` or material `set("render_priority", ...)` generic property — but
  prefer the typed setter. This MUST work (finer-on-top depends on it).
- The tile push-order in `configure` (level outer, dz, dx inner) MUST match `tile_index`
  (`level*9 + (dz+1)*3 + (dx+1)`) so the flat Vec lines up. Verify: for level L, the loop
  pushes (dz=-1,dx=-1),(dz=-1,dx=0),(dz=-1,dx=1),(dz=0,dx=-1)... = indices L*9+0,1,2,3...
  which matches `(dz+1)*3+(dx+1)`. Correct.
- Owns no RIDs (no texture_create/free_rid).

- [ ] **Step 2: Build to verify it compiles**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo build`
Expected: clean. Iterate on `set_render_priority` / other gdext spellings until it compiles.

- [ ] **Step 3: Run the full cargo suite**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo test`
Expected: 103 passed, 0 failed (no new unit tests; rings verified by compile + the windowed gate).

- [ ] **Step 4: Commit**

```powershell
git add wg-10/rust/src/clipmap_rings.rs
git commit -m "feat(m3): rebuild Wg10ClipmapRings to N x 9 page tiles (slice 5b)

Each clipmap level is now a 3x3 neighborhood of one-page tiles (9 MeshInstance3D/level) so it
surrounds the camera. Each tile is a full grid spanning one page with its own ShaderMaterial;
render_priority derived from level (finest on top) for the overlap. bind_tile(level,dx,dz,...)
places the tile at page_origin+span/2 and sets the uniforms (incl. coarse_origin). Per-tile
meshes (no sharing). Accessors: level_count/tile_count/total_vertex_count/bound_page_key. Owns
no RIDs. Replaces the one-page-per-level structure (which didn't surround the camera).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Recreate `Wg10TerrainView` for the 3×3 live loop

**Files:**
- Create: `wg-10/rust/src/terrain_view.rs`
- Modify: `wg-10/rust/src/lib.rs`

`Wg10TerrainView` (Node3D) ticks the live loop, driving the 27 tiles via read-only
`get_resident_page`. Verified by compile + the Task 3 gate.

- [ ] **Step 1: Declare the module**

In `wg-10/rust/src/lib.rs`, add `mod terrain_view;` after `mod clipmap_rings;`.

- [ ] **Step 2: Write the implementation**

Create `wg-10/rust/src/terrain_view.rs` (ADAPT gdext spellings; keep API + semantics):

```rust
//! Wg10TerrainView (DESIGN §6.2) — the drop-in terrain Node3D. Owns Gd handles to the page
//! pool, the stream-ahead scheduler, and the 3x3 clipmap rings, and ticks the live loop:
//! streamer.update -> per level per tile (3x3) fetch the resident page via the READ-ONLY
//! get_resident_page (NEVER computes on the render path), coarser fallback on a miss ->
//! rings.bind_tile. Owns no RIDs, no meshes, no scheduling math.

use godot::prelude::*;
use crate::page_pool::Wg10PagePool;
use crate::streamer::Wg10Streamer;
use crate::clipmap_rings::Wg10ClipmapRings;

#[derive(GodotClass)]
#[class(base=Node3D)]
pub struct Wg10TerrainView {
    pool: Option<Gd<Wg10PagePool>>,
    streamer: Option<Gd<Wg10Streamer>>,
    rings: Option<Gd<Wg10ClipmapRings>>,
    num_levels: i32,
    base_span: f64,
    height_scale: f64,
    morph_region: f64,
    relief_ref: f64,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for Wg10TerrainView {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            pool: None, streamer: None, rings: None,
            num_levels: 0, base_span: 0.0, height_scale: 1.0,
            morph_region: 0.0, relief_ref: 2000.0, base,
        }
    }
}

#[godot_api]
impl Wg10TerrainView {
    #[func]
    pub fn configure(
        &mut self,
        pool: Gd<Wg10PagePool>,
        streamer: Gd<Wg10Streamer>,
        rings: Gd<Wg10ClipmapRings>,
        num_levels: i64,
        base_span: f64,
        height_scale: f64,
        morph_region: f64,
        relief_ref: f64,
    ) {
        self.pool = Some(pool);
        self.streamer = Some(streamer);
        self.rings = Some(rings);
        self.num_levels = num_levels as i32;
        self.base_span = base_span;
        self.height_scale = height_scale;
        self.morph_region = morph_region;
        self.relief_ref = relief_ref;
    }

    /// One frame: advance the scheduler, then place+bind every tile read-only.
    #[func]
    pub fn update(&mut self, camera_x: f64, camera_z: f64, vel_x: f64, vel_z: f64) {
        if self.pool.is_none() || self.streamer.is_none() || self.rings.is_none() {
            godot_error!("Wg10TerrainView: update called before configure()");
            return;
        }
        // 1. scheduler (sole producer).
        {
            let mut streamer = self.streamer.as_ref().unwrap().clone();
            streamer.bind_mut().update(camera_x, camera_z, vel_x, vel_z);
        }

        let num = self.num_levels;
        for level in 0..num {
            let span_l = self.base_span * 2f64.powi(level);
            let center_x = (camera_x / span_l).floor() * span_l;
            let center_z = (camera_z / span_l).floor() * span_l;

            let span_c = if level < num - 1 { self.base_span * 2f64.powi(level + 1) } else { span_l };
            let coarse_level = if level < num - 1 { level + 1 } else { level };

            for dz in -1..=1 {
                for dx in -1..=1 {
                    let po_x = center_x + dx as f64 * span_l;
                    let po_z = center_z + dz as f64 * span_l;

                    // coarser page containing this tile's centre
                    let tc_x = po_x + span_l * 0.5;
                    let tc_z = po_z + span_l * 0.5;
                    let co_x = (tc_x / span_c).floor() * span_c;
                    let co_z = (tc_z / span_c).floor() * span_c;

                    // READ-ONLY fetch — never computes.
                    let (tex, coarse_tex) = {
                        let pool = self.pool.as_ref().unwrap().bind();
                        let tex = pool.get_resident_page(level as i64, po_x, po_z);
                        let coarse_tex = pool.get_resident_page(coarse_level as i64, co_x, co_z);
                        (tex, coarse_tex)
                    };

                    // never-black fallback: tex None -> coarse with morph 0; both None -> skip
                    // (tile keeps its previous binding; the gate's never-black catches a true gap).
                    let (height_tex, morph) = if tex.is_some() {
                        let m = if level < num - 1 { self.morph_region } else { 0.0 };
                        (tex, m)
                    } else {
                        (coarse_tex.clone(), 0.0)
                    };

                    if let (Some(ht), Some(ct)) = (height_tex, coarse_tex) {
                        let mut rings = self.rings.as_ref().unwrap().clone();
                        rings.bind_mut().bind_tile(
                            level as i64, dx as i64, dz as i64,
                            ht.upcast::<godot::classes::Texture2D>(),
                            ct.upcast::<godot::classes::Texture2D>(),
                            span_l, span_c,
                            self.height_scale, morph, self.relief_ref,
                            po_x, po_z, co_x, co_z,
                        );
                    }
                }
            }
        }
    }

    /// Pass-through of the pool's stats (the gate's window in).
    #[func]
    pub fn stats(&self) -> Dictionary<GString, Variant> {
        match self.pool.as_ref() {
            Some(pool) => pool.bind().stats(),
            None => Dictionary::<GString, Variant>::new(),
        }
    }
}
```

NOTE: `get_resident_page(level, ox, oz) -> Option<Gd<Texture2Drd>>` (read-only, landed in 5a).
`.upcast::<godot::classes::Texture2D>()` (Texture2Drd -> Texture2D; the turbofish form was used
in 5a and compiled). Clone-Gd-before-bind_mut for streamer/rings. `pool.bind()` (immutable) for
the read-only fetches. The fallback clones `coarse_tex` (consumed twice). Keep `configure`/
`update`/`stats` signatures.

- [ ] **Step 3: Build + full cargo suite**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo build` (clean), then `... cargo test`
(103 passed). Iterate on gdext spellings until clean.

- [ ] **Step 4: Commit**

```powershell
git add wg-10/rust/src/terrain_view.rs wg-10/rust/src/lib.rs
git commit -m "feat(m3): Wg10TerrainView 3x3 live loop (slice 5b)

Recreated for 3x3 tiling: update(cam,vel) runs streamer.update then, per level per tile
(3x3), fetches the page via the read-only get_resident_page (NEVER computes — the anti-WG9
render-path rule) with coarser fallback on a miss, and calls rings.bind_tile. Page key =
floor(cam/span)*span + (dx,dz)*span (= the scheduler's coverage at radius_pages=1), so the
view's lookups hit exactly what the streamer made resident. Owns no RIDs/meshes/scheduling.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: `m3_view_check.gd` moving-sweep gate + gate.py (retire m3_rings_check)

**Files:**
- Create: `wg-10/worldgen_terrain/tests/m3_view_check.gd`
- Modify: `tools/gate.py`
- Remove: `wg-10/worldgen_terrain/tests/m3_rings_check.gd` (+ `.uid`)

The CONTROLLER runs the windowed gate + inspects PNGs; a subagent writes the files + cargo
build and reports DONE.

- [ ] **Step 1: gate.py — swap the m3 entries**

In `tools/gate.py`, the `"m3"` list currently ends with `m3_rings_check.gd`. Remove that line
and add `m3_view_check.gd`:
```python
    "m3": [
        "worldgen_terrain/tests/m3_slice1_check.gd",
        "worldgen_terrain/tests/m3_pool_check.gd",
        "worldgen_terrain/tests/m3_stream_check.gd",
        "worldgen_terrain/tests/m3_view_check.gd",
    ],
```

- [ ] **Step 2: remove the retired gate**

Delete `wg-10/worldgen_terrain/tests/m3_rings_check.gd` and `wg-10/worldgen_terrain/tests/m3_rings_check.gd.uid` (use `git rm`).

- [ ] **Step 3: write the gate** (TAB indentation, matching the other m3 checks)

Create `wg-10/worldgen_terrain/tests/m3_view_check.gd`:
```gdscript
extends SceneTree

# M3 slice 5b gate: drive Wg10TerrainView (3x3 tiling) over a scripted MOVING +x sweep across
# page boundaries; at each non-zero camera position render top-down ortho CENTERED on the
# camera and assert the 3x3 SURROUNDS the camera (full coverage) + seamless + never-black +
# the view triggers ZERO compute (read-only). Proves the slice-5a finding (one page doesn't
# surround) is fixed. WINDOWED. Saves PNGs.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE    := "terrain_pack.gate.json"
const GLSL         := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER       := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const PAGE_PX      := 256
const SEED         := 1337
const NUM_LEVELS   := 3
const BASE_SPAN    := 8192.0
const GRID_RES     := 64
const RADIUS_PAGES := 1
const LEAD_FRAMES  := 8.0
const MAX_PER_FRAME := 4
const CAPACITY     := 48        # >= per-level coverage (3 levels x 9 = 27) + stream-ahead headroom
const MORPH_REGION := 0.15
const HEIGHT_SCALE := 0.35
const RELIEF_REF   := 2000.0
const VIEW_SIZE    := Vector2i(512, 512)
const MIN_DISTINCT := 8

# +x sweep incl. a level-0 boundary crossing and non-zero offsets.
const POSITIONS := [0.0, 2048.0, 4096.0, 8192.0, 20000.0]
const VEL_X := 6000.0
const WARM_FRAMES := 24         # let stream-ahead fill the 3x3 of every level before measuring

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10TerrainView"):
		push_error("Wg10TerrainView not registered"); return 1
	if RenderingServer.get_rendering_device() == null:
		print("[wg10-m3-view] status=skip reason=no-render-device"); return 2

	var pack_os: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os: String = ProjectSettings.globalize_path(GLSL)

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var cfg_err: String = str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if cfg_err != "":
		push_error("pool configure failed: %s" % cfg_err); return 1

	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_FRAMES, MAX_PER_FRAME)

	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	if int(rings.call("tile_count")) != NUM_LEVELS * 9:
		push_error("expected %d tiles, got %s" % [NUM_LEVELS*9, str(rings.call("tile_count"))]); return 1

	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF)

	var errs: Array[String] = []

	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.0, 0.0, 0.0)
	var cam := Camera3D.new()
	cam.projection = Camera3D.PROJECTION_ORTHOGONAL
	cam.size = BASE_SPAN * 1.5     # frame ~1.5 level-0 spans: the level-0 3x3 (3*span) more than fills it
	cam.far = BASE_SPAN * 16.0
	cam.environment = env
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-90.0, 0.0, 0.0)
	light.light_energy = 1.2
	vp.add_child(rings)
	vp.add_child(cam)
	vp.add_child(light)
	get_root().add_child(vp)

	var idx := 0
	for pos_x in POSITIONS:
		for _w in range(WARM_FRAMES):
			view.call("update", pos_x, 0.0, VEL_X, 0.0)

		# (zero-compute) hold static and assert the VIEW triggers no compute (read-only).
		var ps0: Dictionary = pool.call("stats")
		var c0 := int(ps0.get("created", 0)) + int(ps0.get("recomputed", 0))
		for _h in range(4):
			view.call("update", pos_x, 0.0, 0.0, 0.0)
		var ps1: Dictionary = pool.call("stats")
		var c1 := int(ps1.get("created", 0)) + int(ps1.get("recomputed", 0))
		if c1 != c0:
			errs.append("pos %.0f: view triggered compute while static (%d->%d) — render-path compute (WG9)" % [pos_x, c0, c1])

		cam.look_at_from_position(Vector3(pos_x, BASE_SPAN * 4.0, 0.0), Vector3(pos_x, 0.0, 0.0), Vector3(0.0, 0.0, -1.0))
		for _s in range(6):
			await process_frame
		RenderingServer.force_draw()
		await process_frame

		var img := vp.get_texture().get_image()
		if img == null:
			push_error("get_image null at pos %f" % pos_x); return 1
		img.save_png("user://m3_view_%d.png" % idx)

		# (1) full coverage — the headline fix: the 3x3 SURROUNDS the camera.
		var distinct := {}
		var nonblack := 0
		var total := img.get_width() * img.get_height()
		for y in range(img.get_height()):
			for x in range(img.get_width()):
				var c := img.get_pixel(x, y)
				if not (is_finite(c.r) and is_finite(c.g) and is_finite(c.b)):
					push_error("non-finite pixel @ %d,%d pos %f" % [x,y,pos_x]); return 1
				if c.r > 0.03 or c.g > 0.03 or c.b > 0.03:
					nonblack += 1
				distinct[Vector3i(int(c.r*16), int(c.g*16), int(c.b*16))] = true
		var frac := float(nonblack) / float(total)
		if frac < 0.98:
			errs.append("pos %.0f: NOT surrounded — nonblack=%.3f < 0.98 (3x3 should fill the frame)" % [pos_x, frac])
		if distinct.size() < MIN_DISTINCT:
			errs.append("pos %.0f: no relief distinct=%d" % [pos_x, distinct.size()])

		# (4) no z-fight in the overlap: compare two settled captures at the same pose — the
		# overlap region must be pixel-STABLE (flicker = z-fight). Re-capture and diff center band.
		RenderingServer.force_draw()
		await process_frame
		var img2 := vp.get_texture().get_image()
		var diff := 0
		for y in range(img.get_height()):
			for x in range(img.get_width()):
				var a := img.get_pixel(x, y)
				var b := img2.get_pixel(x, y)
				if abs(a.r-b.r) + abs(a.g-b.g) + abs(a.b-b.b) > 0.05:
					diff += 1
		if diff > total / 50:    # >2% of pixels changed between two settled frames -> flicker
			errs.append("pos %.0f: overlap z-fight/flicker — %d px unstable between settled frames" % [pos_x, diff])

		# (5) never-black + budget
		var ps: Dictionary = pool.call("stats")
		if int(ps.get("resident", 0)) < 1:
			errs.append("pos %.0f: nothing resident" % pos_x)
		if int(ps.get("resident", 0)) > CAPACITY:
			errs.append("pos %.0f: budget exceeded resident %d > %d" % [pos_x, int(ps.get("resident",0)), CAPACITY])
		idx += 1

	# (7) tile<->page mapping (CPU): at cam=0, level 0, tile (1,0) should map to page origin
	# (BASE_SPAN, 0) (center page origin 0 + dx*span).
	view.call("update", 0.0, 0.0, 0.0, 0.0)
	var key: Vector2i = rings.call("bound_page_key", 0, 1, 0)
	if key != Vector2i(int(BASE_SPAN), 0):
		errs.append("tile<->page mapping wrong: level0 tile(1,0) -> %s, expected (%d,0)" % [str(key), int(BASE_SPAN)])

	pool.call("free_all")

	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m3-view] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-m3-view] status=pass positions=%d tiles=%d" % [POSITIONS.size(), NUM_LEVELS*9])
	return 0
```

NOTE for the controller: the ortho `cam.size`, `WARM_FRAMES`, `CAPACITY`, `MAX_PER_FRAME`, and
the boundary/overlap sample regions are best-effort and MUST be validated windowed + by
inspecting the PNGs. If coverage isn't ~1.0, the 3x3 isn't surrounding (real bug — fix the
tiling/keys, do NOT lower the 0.98 threshold). If the static-compute assertion trips, the view
is computing (real WG9 regression). Tune CAPACITY/WARM_FRAMES so the streamer can fill 3 levels
× 9 pages before measuring (27 pages + headroom; CAPACITY=48 and bounded MAX_PER_FRAME means
warm-up needs enough frames — increase WARM_FRAMES if residency hasn't filled).

- [ ] **Step 4 (CONTROLLER runs windowed):** build + run m3 + inspect PNGs.
```powershell
$env:CARGO_TARGET_DIR=$null; Push-Location wg-10/rust; cargo build; Pop-Location
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools/gate.py --suite m3
```
Expected: `[wg10-m3-view] status=pass positions=5 tiles=27` and `[gate] suite=m3 checks=4 fail=0`.
slice-1/2/3 still pass. Inspect `user://m3_view_*.png`: terrain fills the frame and follows the
camera across boundary crossings.

- [ ] **Step 5: Run fast + gpu (no regressions)**
```powershell
python tools/gate.py --suite fast
python tools/gate.py --suite gpu
```
Expected: `suite=fast checks=5 fail=0`, `suite=gpu checks=2 fail=0 skip=0`.

- [ ] **Step 6: Commit**

```powershell
git add wg-10/worldgen_terrain/tests/m3_view_check.gd tools/gate.py
git rm wg-10/worldgen_terrain/tests/m3_rings_check.gd wg-10/worldgen_terrain/tests/m3_rings_check.gd.uid
git commit -m "test(m3): m3_view_check 3x3 moving-sweep gate; retire m3_rings_check (slice 5b)

Drives Wg10TerrainView (3x3 tiling) over a +x sweep across page boundaries; at each NON-ZERO
position renders top-down ortho centered on the camera and asserts: full coverage (nonblack
>= 0.98 — the 3x3 SURROUNDS the camera, the slice-5a fix), real relief, no z-fight (two
settled captures pixel-stable in the overlap), never-black + budget, view-zero-compute
(static created+recomputed flat), tile<->page mapping (CPU). Saves PNGs. Retires the one-page
m3_rings_check (its geometry is gone; this supersedes it). m3 suite stays 4 checks.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Update STATUS + ROADMAP (5b done)

**Files:**
- Modify: `docs/plans/STATUS.md`, `docs/plans/ROADMAP.md`

- [ ] **Step 1: STATUS.md** — `Last updated:` + Phase line: 3×3 tiling + rings↔view wiring DONE.
  Add current-state bullets: `Wg10ClipmapRings` (N×9 tiles, render_priority overlap, bind_tile),
  `Wg10TerrainView` (3×3 live loop, read-only fetch), `m3_view_check` (moving sweep, coverage
  ~1.0 at non-zero positions, no z-fight, zero view-compute). Update gate-runner line (m3 = 4
  checks; m3_rings retired, m3_view added). In "What works" note the 3×3 surrounds the camera
  under motion. Update "What's next" item 1 to the M3-closing slice (fly-cam + overlay + p99
  gate + manual fly). Record the **overlap overdraw as an explicit input to the p99 gate**.
  Keep the honest baseline: 5b proves coverage+seamless+never-black under SCRIPTED motion; perf
  (p99) + interactive flight are the closing slice.

- [ ] **Step 2: ROADMAP.md** — `Last updated:` + flip the "3×3 ring tiling + rings↔streamer
  wiring" item to DONE (Wg10ClipmapRings N×9 + Wg10TerrainView 3×3 loop + m3_view_check; coverage
  surrounds camera under motion; m3 4 checks fail=0). Leave harness/fly-test/acceptance-gate/
  manual-acceptance `[ ]`. Add a note under the acceptance-gate item: "overlap overdraw (finer
  3×3 over coarse center, fixed/bounded) is an input to this p99 measurement."

- [ ] **Step 3: fresh evidence** — copy ACTUAL numbers:
```powershell
$env:CARGO_TARGET_DIR=$null; Push-Location wg-10/rust; cargo test 2>&1 | Select-String "test result"; Pop-Location
python tools/gate.py --suite m3
```
Use the real `test result: ok. N passed` + `suite=m3 checks=4 fail=0`.

- [ ] **Step 4: Commit**

```powershell
git add docs/plans/STATUS.md docs/plans/ROADMAP.md
git commit -m "docs(m3): slice 5b (3x3 tiling + wiring) done — STATUS + ROADMAP

Wg10ClipmapRings (N x 9 tiles, render_priority overlap) + Wg10TerrainView (3x3 read-only live
loop) landed; m3_view_check passes WINDOWED — full coverage (3x3 surrounds the camera) +
seamless + no z-fight + never-black + zero view-compute at several non-zero positions; PNGs
eyeballed. m3 suite 4 checks fail=0 (m3_rings retired, m3_view added); fast 5, gpu 2 unchanged;
cargo green. Overlap overdraw recorded as a p99-gate input. Next (M3 close-out): fly camera +
diagnostics overlay + p99<6ms acceptance gate + manual fly.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage:**
- Spec §1 scope (rings N×9, bind_tile, view 3×3, gate, retire rings_check) → Tasks 1,2,3. ✓
- Spec §2 tile geometry (full one-page grid via band_mesh level-0 at span_l; per-tile meshes no-share; overlap; render_priority by level; accessors) → Task 1. ✓
- Spec §2.4 bind_tile (place at origin+span/2, set uniforms incl coarse_origin, record bound key) → Task 1 `bind_tile`. ✓
- Spec §3 view loop (streamer.update; per level center=floor(cam/span)*span; per tile po, coarse co, read-only get_resident_page, fallback, bind_tile; no separate recenter) → Task 2. ✓
- Spec §4 gate (coverage~1, relief, seam, no-z-fight via 2-capture stability, never-black, zero-compute, tile↔page mapping; moving non-zero positions; PNGs) → Task 3. ✓ (seam continuity is implicitly covered by coverage~1 + no-z-fight; the explicit per-boundary color-jump check from prior gates is folded into the no-z-fight stability + coverage checks — NOTE this simplification below.)
- Spec §5 files → match (rewrite clipmap_rings, recreate terrain_view + lib.rs, create m3_view_check, modify gate.py, remove m3_rings_check). ✓
- Spec §6 done + §7 risks → Task 3 (z-fight stability, never-black, zero-compute) + Task 4 (overdraw as p99 input, honest baseline). ✓

**Refinement note (within spec intent):** The spec §4 listed seam continuity (no black gap /
color jump at the boundary) AND no-z-fight as separate assertions. The plan's gate folds seam
evidence into (1) full-coverage `nonblack ≥ 0.98` (a seam gap would show as black → drops
coverage) + (4) the two-capture stability check (a flickering/!misaligned seam shows as
instability) + (7) the tile↔page mapping (wrong keys → seams/holes). A dedicated per-boundary
color-jump probe like slice-4's is omitted because with the 3×3 overlap the level boundary is
no longer at a fixed frame column (it depends on camera-vs-page phase), making a hardcoded
boundary-column probe fragile. If the controller's PNG inspection shows a visible seam that the
coverage/stability checks don't catch, ADD a targeted seam probe then (a real finding). Recorded
so the spec/plan don't silently diverge.

**2. Placeholder scan:** No TBD/TODO/"handle edge cases". Complete code in every code step. The
gate's tuning constants carry an explicit "validate windowed, don't lower thresholds, a real
bug is a real finding" instruction (honest, not a placeholder). ✓

**3. Type consistency:** `tile_index(level,dx,dz) = level*9 + (dz+1)*3 + (dx+1)` defined Task 1,
matched by the configure push-order (Task 1) and the gate's mapping check (Task 3).
`bind_tile(level,dx,dz, height_tex, coarse_tex, span_l, coarse_span, height_scale, morph_region,
relief_ref, page_origin_x, page_origin_z, coarse_origin_x, coarse_origin_z)` — 14 args,
identical between Task 1 (def) and Task 2 (caller). `get_resident_page(level,ox,oz) ->
Option<Gd<Texture2Drd>>` (5a) used in Task 2. `span_l = base_span·2^level`, `center =
floor(cam/span)*span` identical in Task 2 (view) and Task 3 (gate mapping check). Shader uniforms
(coarse_origin etc.) match the 5a shader. ✓
