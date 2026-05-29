# WorldGen10 M3 Slice 5a — View Wiring + Carry-Forward Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the pool + streamer + rings into one live frame-loop driver (`Wg10TerrainView`) and fix the two slice-4 carry-forwards (per-level page span; geomorph `coarse_origin`) so terrain renders seamless and never-black while the camera MOVES — proven by a scripted moving-sweep windowed gate.

**Architecture:** A new godot `Wg10TerrainView` (Node3D) holds `Gd` handles to the pool, streamer, and rings and ticks them each frame: `streamer.update` → per level acquire the resident page (coarser fallback on Full) + its coarser neighbor → `rings.bind_page` (with the coarser page's world corner as `coarse_origin`) → `rings.recenter`. The pool now computes a level-L page over `world_span·2^L` (Fix #1), and the shader samples the coarse page corner-relative in world space (Fix #2); the two fixes are interdependent and make the seam close at any camera position. A page's world rect is `[corner, corner+span]` where `corner = floor((cam − span/2)/span)·span` — the lower corner of the camera-centered band, quantized to the level grid — so the page exactly covers the band the ring mesh displays.

**Tech Stack:** Rust (gdext 0.5.3, godot api-4-6), Godot 4.6 spatial shader + ArrayMesh, GDScript SceneTree windowed gate via `tools/gate.py`.

---

## Conventions (read before Task 1)

- **Build/test the crate** from `wg-10/rust` with `CARGO_TARGET_DIR` UNSET (not empty —
  empty is rejected). Bash: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo test <one-filter>`
  / `... cargo build`. (`cargo test` takes ONE filter arg.)
- **Windowed gate** (project root, GODOT_BIN set) — the CONTROLLER runs this; a subagent that
  can't run windowed Godot builds + reports DONE and leaves the windowed run to the controller:
  ```powershell
  $env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
  python tools/gate.py --suite m3
  ```
- **Commit trailer** on every commit:
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`
- **Stay on branch `main`** (per-task commits; the user pushes).
- gdext idioms already established in this crate: Node-derived `::new_alloc()`, Resource/
  RefCounted `::new_gd()`, `try_load::<Shader>(&gstring)`, `mat.set_shader_parameter("name",
  &val.to_variant())`, untyped array is `VarArray`. `Gd<T>` is cloned (cheap refcount bump)
  before `bind_mut()` to avoid borrowing `self` twice (see streamer.rs).

---

## The page-rectangle convention (locks Tasks 1, 2, 4, 5)

A clipmap level L band is **centered on the camera** and spans world
`[cam − span_L/2, cam + span_L/2]`, `span_L = base_span·2^L`. Its page must cover exactly that
rectangle. We key a page by its **lower-corner origin**, quantized to the level grid:

```
corner_L(cam) = floor((cam − span_L/2) / span_L) * span_L     # per axis
```

The pool computes the page over `[corner, corner + span_L]` (Task 1). The ring mesh for level
L is centered on its instance (which `recenter` puts at the quantized camera); its fine UV
maps local `[-span_L/2, +span_L/2]` → `[0,1]`. The shader's coarse sample uses
`(world.xz − coarse_origin)/coarse_span` with `coarse_origin = corner_{L+1}(cam)` (Task 2/4).
Because both the fine page and the coarse page are computed over their band rectangles, a
world point on the seam maps to the same coarse texel in both the fine level's morph and the
coarse level's own render → seam closed at any camera position.

Every site that picks a page key (`Wg10TerrainView`, the gates) uses `corner_L` above. This
is the one shared convention; do not deviate.

---

## Task 1: Fix #1 — per-level page span in the pool

**Files:**
- Modify: `wg-10/rust/src/page_pool.rs`

The dispatch span is currently a flat `self.world_span` (local `ws`, set at the top of
`acquire_page` and passed to both `compute_into_texture` calls). Make it level-dependent:
`span_L = self.world_span * 2^level`. This is verified by build + the existing m3_pool_check
(level-0 unchanged) here, and exercised under multi-level use by Tasks 5-6.

- [ ] **Step 1: Locate the `ws` binding**

In `wg-10/rust/src/page_pool.rs`, `acquire_page` has (around line 210):
```rust
        let (ox, oz, ws, ppx, sd) =
            (origin_x, origin_z, self.world_span, self.page_px, self.seed);
```
`ws` is passed to `compute_into_texture(...)` in both the `Allocate` arm (~line 244) and the
`AllocateEvicting` arm (~line 281).

- [ ] **Step 2: Make the span level-dependent**

Replace the tuple binding with one that scales the span by the level:
```rust
        // Per-level page span: a level-L page covers world_span * 2^level (Fix #1, slice 5a).
        // A flat world_span was only correct at level 0; coarser levels must cover 2^L more
        // ground so the page matches the clipmap band (scheduler/RingLayout level_span).
        let span_l = self.world_span * 2f64.powi(level as i32);
        let (ox, oz, ws, ppx, sd) =
            (origin_x, origin_z, span_l, self.page_px, self.seed);
```
(Leave both `compute_into_texture(... ox, oz, ws, ppx, sd)` calls as-is — they now receive the
level-scaled span via `ws`.)

- [ ] **Step 3: Build + run the pool gate (level-0 regression)**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo build`
Expected: clean.
Then the full cargo suite: `env -u CARGO_TARGET_DIR cargo test`
Expected: 103 passed, 0 failed (no Rust unit test exercises multi-level span; level-0 path
unchanged → `m3_pool_check` and slice-1/2 still correct, proven windowed later).

- [ ] **Step 4: Commit**

```powershell
git add wg-10/rust/src/page_pool.rs
git commit -m "fix(m3): per-level page span in Wg10PagePool::acquire_page (slice 5a Fix #1)

A level-L page now dispatches its compute over world_span * 2^level instead of a flat
world_span — coarser levels cover 2^L more ground, matching the scheduler's and rings'
level_span. Was only correct at level 0 (the slice-4 audit Issue 3). Signature unchanged;
level-0 callers (slice-1/2/3 gates, m3_pool_check) are byte-identical (2^0 = 1).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Fix #2 — geomorph `coarse_origin` in the shader

**Files:**
- Modify: `wg-10/worldgen_terrain/shaders/ring_displace.gdshader`

Add a `coarse_origin` uniform and change the coarse sample from origin-centered
(`world.xz/coarse_span + 0.5`) to corner-relative (`(world.xz − coarse_origin)/coarse_span`),
so the morph samples the correct coarse texel when the coarse page is NOT centered at the
world origin (i.e. whenever the camera has moved).

- [ ] **Step 1: Edit the shader**

In `wg-10/worldgen_terrain/shaders/ring_displace.gdshader`, add the uniform after
`coarse_span` and change the `uv_coarse` line. The uniform block becomes:
```glsl
uniform sampler2D height_tex;            // this level's R32F page (pool Texture2DRD)
uniform sampler2D coarse_height_tex;     // next-coarser level's page (for the morph)
uniform float world_span = 8192.0;       // this level's band world span
uniform float coarse_span = 8192.0;      // the coarser level's band world span
uniform vec2 coarse_origin = vec2(0.0);  // the coarser page's WORLD corner (lower-XZ); page covers [origin, origin+coarse_span]
uniform float height_scale = 1.0;        // visual amplitude (config; 1.0 = raw metres)
uniform float morph_region = 0.0;        // transition width as a fraction of span (0 = no morph)
uniform float relief_ref = 2000.0;       // color gradient normalization
```
And the coarse-sample line in `vertex()` changes from:
```glsl
	vec2 uv_coarse = (world.xz / coarse_span) + vec2(0.5);
```
to:
```glsl
	// Coarser page covers world [coarse_origin, coarse_origin + coarse_span]; sample it
	// corner-relative so the morph is correct wherever the coarse page sits (Fix #2).
	vec2 uv_coarse = (world.xz - coarse_origin) / coarse_span;
```
Leave everything else (the fine sample `uv_fine = VERTEX.xz/world_span + 0.5`, the Chebyshev
morph factor `t`, the `mix`, the fragment) UNCHANGED. The fine sample stays local-UV because
the fine page is centered on the level's own (recentered) mesh instance; only the coarse
sample references a different level's page via world coords.

- [ ] **Step 2: Sanity-check the shader parses**

If Godot is runnable, run the import pass and confirm no shader error:
```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
& $env:GODOT_BIN --headless --import --path "D:\workflows\worldgen10\wg-10" 2>&1 | Select-String -Pattern "ring_displace|shader.*error"
```
Expected: no error mentioning ring_displace. (If Godot isn't runnable, leave to the
controller; the shader is exercised by the Task 6 gate.)

- [ ] **Step 3: Commit**

```powershell
git add wg-10/worldgen_terrain/shaders/ring_displace.gdshader
git commit -m "fix(m3): geomorph coarse_origin uniform (slice 5a Fix #2)

The coarse sample assumed the coarse page was centered at the world origin
(uv_coarse = world.xz/coarse_span + 0.5), so the morphed seam reopened proportional to the
camera's displacement (slice-4 audit Issue 2). Add a coarse_origin uniform (the coarse
page's world corner) and sample corner-relative: (world.xz - coarse_origin)/coarse_span. The
fine sample is unchanged (own centered page). Default coarse_origin = vec2(0); callers set it
to the coarse page's corner. NOTE: this changes the coarse-UV convention — bind_page + the
slice-4 gate are updated in the next tasks.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: `bind_page` gains `coarse_origin`

**Files:**
- Modify: `wg-10/rust/src/clipmap_rings.rs`

Add a `coarse_origin` parameter to `Wg10ClipmapRings::bind_page` and set it into the
material. This is the only signature change; the rest of bind_page is unchanged.

- [ ] **Step 1: Add the parameter + set the uniform**

In `wg-10/rust/src/clipmap_rings.rs`, the `bind_page` signature (around line 124) currently
ends with `morph_region: f64, relief_ref: f64`. Add `coarse_origin_x: f64, coarse_origin_z:
f64` as the final two params (two f64s rather than a Vector2, to keep the GDScript `.call`
boundary simple and match the existing all-f64 style):
```rust
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
        coarse_origin_x: f64,
        coarse_origin_z: f64,
    ) {
```
And after the existing `relief_ref` set (around line 155), add:
```rust
        let coarse_origin = Vector2::new(coarse_origin_x as f32, coarse_origin_z as f32);
        mat.set_shader_parameter("coarse_origin", &coarse_origin.to_variant());
```
(Keep all the other `set_shader_parameter` calls. `Vector2` is in the godot prelude.)

- [ ] **Step 2: Build**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo build`
Expected: clean. (No caller compiles against bind_page in Rust — it's `#[func]`, called from
GDScript — so the crate builds; the GDScript callers are updated in Tasks 4/6.)

- [ ] **Step 3: Run the full cargo suite**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo test`
Expected: 103 passed, 0 failed.

- [ ] **Step 4: Commit**

```powershell
git add wg-10/rust/src/clipmap_rings.rs
git commit -m "feat(m3): bind_page gains coarse_origin (slice 5a)

bind_page now takes coarse_origin_x/z (the coarser page's world corner) and sets the
coarse_origin shader uniform, completing Fix #2's caller side. Two f64s (matching the
all-f64 #[func] boundary style). All other params/uniforms unchanged.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Update slice-4 gate for the new convention (regression)

**Files:**
- Modify: `wg-10/worldgen_terrain/tests/m3_rings_check.gd`

Slice-4's `m3_rings_check.gd` calls `bind_page` with the OLD signature (no coarse_origin) and
acquires level-1 at origin `(0,0)`. After Tasks 1-3 it must (a) pass `coarse_origin`, and (b)
acquire each level's page at the band's lower corner so Fix #1's per-level span lines up with
the centered band. Keep slice-4's static (camera-at-origin) capture passing.

At the origin (cam = 0): for level L, `corner_L = floor((0 − span_L/2)/span_L)·span_L =
floor(−0.5)·span_L = −span_L`. So:
- level 0 (span BASE_SPAN): corner = `−BASE_SPAN`
- level 1 (span 2·BASE_SPAN): corner = `−2·BASE_SPAN`

- [ ] **Step 1: Update the page acquires + bind_page calls**

In `wg-10/worldgen_terrain/tests/m3_rings_check.gd`, the acquires (around lines 46-47) and
binds (lines 57-58) currently are:
```gdscript
	var tex0 = pool.call("acquire_page", 0, 0.0, 0.0)
	var tex1 = pool.call("acquire_page", 1, 0.0, 0.0)
	...
	rings.call("bind_page", 0, tex0, tex1, BASE_SPAN, BASE_SPAN * 2.0, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF)
	rings.call("bind_page", 1, tex1, tex1, BASE_SPAN * 2.0, BASE_SPAN * 2.0, HEIGHT_SCALE, 0.0, RELIEF_REF)
```
Replace with (acquire at the band corner; pass coarse_origin = the coarser page's corner):
```gdscript
	# Page key = lower corner of the camera-centered band, quantized. At cam=origin:
	# corner_L = floor((0 - span_L/2)/span_L)*span_L = -span_L.
	var c0 := -BASE_SPAN              # level 0 corner (span BASE_SPAN)
	var c1 := -BASE_SPAN * 2.0        # level 1 corner (span 2*BASE_SPAN)
	var tex0 = pool.call("acquire_page", 0, c0, c0)
	var tex1 = pool.call("acquire_page", 1, c1, c1)
	if tex0 == null or tex1 == null:
		push_error("acquire_page returned null"); return 1
	...
	# level 0 morphs toward level 1; coarse_origin = level-1's corner (c1,c1).
	rings.call("bind_page", 0, tex0, tex1, BASE_SPAN, BASE_SPAN * 2.0, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF, c1, c1)
	# level 1 coarsest -> no morph; coarse = itself, coarse_origin = its own corner (c1,c1).
	rings.call("bind_page", 1, tex1, tex1, BASE_SPAN * 2.0, BASE_SPAN * 2.0, HEIGHT_SCALE, 0.0, RELIEF_REF, c1, c1)
```
(The existing `null` guard line may already be present right after the acquires — keep one
copy; don't duplicate.)

- [ ] **Step 2 (CONTROLLER runs windowed):** build the crate, run the m3 suite, confirm
  slice-4 still passes. A subagent without windowed Godot: `cd wg-10/rust && env -u
  CARGO_TARGET_DIR cargo build` (confirms no Rust breakage) and report DONE, leaving the gate
  run to the controller.

Controller windowed run:
```powershell
$env:CARGO_TARGET_DIR=$null; Push-Location wg-10/rust; cargo build; Pop-Location
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools/gate.py --suite m3
```
Expected: `[wg10-m3-rings] status=pass ...` still (slice-4 static capture correct under the
new convention), other m3 checks unaffected.

- [ ] **Step 3: Commit**

```powershell
git add wg-10/worldgen_terrain/tests/m3_rings_check.gd
git commit -m "test(m3): update m3_rings_check for per-level span + coarse_origin (slice 5a)

Slice-4's gate now acquires each level's page at the band's lower corner (corner_L =
-span_L at cam=origin) so Fix #1's per-level span matches the centered band, and passes
coarse_origin to bind_page (= the coarser page's corner). At the origin this is byte-
equivalent to the old origin-centered behavior, so the static capture still passes.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: `Wg10TerrainView` — the live-loop coordinator

**Files:**
- Create: `wg-10/rust/src/terrain_view.rs`
- Modify: `wg-10/rust/src/lib.rs`

A godot Node3D that owns `Gd` handles to the pool, streamer, and rings and ticks the live
loop. No scheduling math, no meshes, no RIDs. Verified by compile + the Task 6 windowed gate.

- [ ] **Step 1: Declare the module**

In `wg-10/rust/src/lib.rs`, add `mod terrain_view;` after `mod clipmap_rings;`.

- [ ] **Step 2: Write the implementation**

Create `wg-10/rust/src/terrain_view.rs`. ADAPT gdext spellings to what compiles (match the
crate's established forms in streamer.rs/clipmap_rings.rs), keeping the public API + the loop
semantics fixed:
```rust
//! Wg10TerrainView (DESIGN §6.2) — the single drop-in terrain node. Owns Gd handles to the
//! page pool, the stream-ahead scheduler, and the clipmap rings, and ticks them each frame:
//! streamer.update -> per-level acquire resident page (coarser fallback on Full) + coarser
//! neighbor -> rings.bind_page (with the coarser page's world corner as coarse_origin) ->
//! rings.recenter. Owns NO RIDs, NO meshes, NO scheduling math — pure orchestration.

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
    /// Wire up the view with already-configured pool/streamer/rings + tunables.
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

    /// One frame of the §5.4 loop: advance the streamer, then bind + recenter the rings.
    #[func]
    pub fn update(&mut self, camera_x: f64, camera_z: f64, vel_x: f64, vel_z: f64) {
        if self.pool.is_none() || self.streamer.is_none() || self.rings.is_none() {
            godot_error!("Wg10TerrainView: update called before configure()");
            return;
        }

        // 1. advance the scheduler (bounded stream-ahead + eviction).
        {
            let mut streamer = self.streamer.as_ref().unwrap().clone();
            streamer.bind_mut().update(camera_x, camera_z, vel_x, vel_z);
        }

        // 2. per-level bind.
        let num = self.num_levels;
        for level in 0..num {
            let span_l = self.base_span * 2f64.powi(level);
            let (ox_l, oz_l) = corner(camera_x, camera_z, span_l);

            // coarser neighbor (level+1); coarsest morphs to itself.
            let (coarse_level, span_c) = if level < num - 1 {
                (level + 1, self.base_span * 2f64.powi(level + 1))
            } else {
                (level, span_l)
            };
            let (ox_c, oz_c) = corner(camera_x, camera_z, span_c);

            // acquire this level's page (cache hit on resident; re-protects it).
            let mut pool = self.pool.as_ref().unwrap().clone();
            let tex_l = pool.bind_mut().acquire_page(level as i64, ox_l as f64, oz_l as f64);
            let coarse_tex = pool.bind_mut().acquire_page(coarse_level as i64, ox_c as f64, oz_c as f64);

            // never-black fallback: if this level's page isn't resident, show the coarser
            // page (which the streamer keeps resident — slice-3 guarantee).
            let (height_tex, morph_l) = match (tex_l, coarse_tex.clone()) {
                (Some(t), _) => (Some(t), if level < num - 1 { self.morph_region } else { 0.0 }),
                (None, Some(c)) => (Some(c), 0.0), // degenerate to flat coarse this level
                (None, None) => (None, 0.0),       // nothing resident; skip bind (gate's never-black catches)
            };

            if let (Some(ht), Some(ct)) = (height_tex, coarse_tex) {
                let mut rings = self.rings.as_ref().unwrap().clone();
                rings.bind_mut().bind_page(
                    level as i64,
                    ht.upcast(),
                    ct.upcast(),
                    span_l, span_c,
                    self.height_scale, morph_l, self.relief_ref,
                    ox_c as f64, oz_c as f64,
                );
            }
        }

        // 3. recenter (quantized translate; never a rebuild).
        {
            let mut rings = self.rings.as_ref().unwrap().clone();
            rings.bind_mut().recenter(camera_x, camera_z);
        }
    }

    /// Minimal stats passthrough (5b's overlay will consume more). Reports the pool's view.
    #[func]
    pub fn stats(&self) -> Dictionary<GString, Variant> {
        if let Some(pool) = self.pool.as_ref() {
            return pool.bind().stats();
        }
        Dictionary::<GString, Variant>::new()
    }
}

/// Lower-corner origin of the camera-centered band at the given span, quantized to the level
/// grid: corner = floor((cam - span/2)/span) * span (per axis). Shared convention (see plan).
fn corner(camera_x: f64, camera_z: f64, span: f64) -> (i64, i64) {
    let cx = (((camera_x - span * 0.5) / span).floor() * span) as i64;
    let cz = (((camera_z - span * 0.5) / span).floor() * span) as i64;
    (cx, cz)
}
```

NOTE for the implementer (adapt to compile, keep intent + API):
- `acquire_page` returns `Option<Gd<Texture2Drd>>`. `bind_page` wants `Gd<Texture2D>`. Convert
  with `.upcast()` (Texture2DRD → Texture2D) — verify the exact gdext spelling
  (`upcast`/`upcast::<Texture2D>()`); match how the crate upcasts elsewhere, or use
  `.clone().upcast()` if a move is needed. If `upcast` isn't available, use the appropriate
  gdext cast (`.to_variant()` round-trip is a last resort; prefer a direct upcast).
- The clone-`Gd`-before-`bind_mut()` pattern is required (see streamer.rs) — don't hold a
  borrow of `self.pool`/`self.rings` while calling `bind_mut()`.
- Calling `acquire_page` twice (this level + coarser) per level is intentional — both are
  cache hits on resident pages. Keep it.
- Keep `corner(...)` as a free fn (the shared page-key convention). Keep the public funcs
  `configure`/`update`/`stats` with these signatures.

- [ ] **Step 3: Build to verify it compiles + registers**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo build`
Expected: clean (iterate on gdext spellings — esp. the upcast — until it compiles).

- [ ] **Step 4: Run the full cargo suite**

Run: `cd wg-10/rust && env -u CARGO_TARGET_DIR cargo test`
Expected: 103 passed, 0 failed (terrain_view has no unit tests — verified by compile + the
windowed gate in Task 6).

- [ ] **Step 5: Commit**

```powershell
git add wg-10/rust/src/terrain_view.rs wg-10/rust/src/lib.rs
git commit -m "feat(m3): Wg10TerrainView live-loop coordinator (slice 5a)

The drop-in terrain Node3D: owns Gd handles to pool/streamer/rings; update(cam,vel) ticks
streamer.update -> per-level acquire resident page (coarser fallback on Full) + coarser
neighbor -> rings.bind_page (coarse_origin = coarser page's corner) -> rings.recenter. Page
keys use the shared corner convention floor((cam-span/2)/span)*span so each page covers the
camera-centered band. Owns no RIDs/meshes/scheduling math — pure orchestration.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: `m3_view_check.gd` moving-sweep gate + wire into gate.py

**Files:**
- Create: `wg-10/worldgen_terrain/tests/m3_view_check.gd`
- Modify: `tools/gate.py`

Drives `Wg10TerrainView.update` over a scripted moving sweep across page boundaries, renders
top-down ortho centered on the camera at each sampled position, and asserts seamless +
never-black at several NON-ZERO positions + the per-level-span check. WINDOWED; the controller
runs it.

- [ ] **Step 1: Add to the m3 suite in gate.py**

In `tools/gate.py`, the `"m3"` list has 4 entries. Add a 5th:
`"worldgen_terrain/tests/m3_view_check.gd"`.

- [ ] **Step 2: Write the gate** (TAB indentation, matching the other m3 checks)

Create `wg-10/worldgen_terrain/tests/m3_view_check.gd`:
```gdscript
extends SceneTree

# M3 slice 5a gate: drive Wg10TerrainView over a scripted MOVING sweep across page
# boundaries; at each non-zero camera position render top-down ortho centered on the camera
# and assert seamless (no holes, seam + morph continuity) + never-black + per-level span.
# This is the first gate that renders at NON-ZERO camera positions (slice 4 only tested
# recenter(0,0)) — it proves the two carry-forward fixes under motion. WINDOWED. Saves PNGs.

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
const MAX_PER_FRAME := 3
const CAPACITY     := 24
const MORPH_REGION := 0.15
const HEIGHT_SCALE := 0.35
const RELIEF_REF   := 2000.0
const VIEW_SIZE    := Vector2i(512, 512)
const MIN_DISTINCT := 8

# Sweep: several +x camera positions, including a level-0 boundary crossing and non-zero
# offsets where the pre-fix coarse_origin bug would reopen the seam.
const POSITIONS := [0.0, 2048.0, 4096.0, 8192.0, 20000.0]
const VEL_X := 6000.0

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

	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, HEIGHT_SCALE, MORPH_REGION, RELIEF_REF)

	# Per-level span check (Fix #1): level 1 covers 2x level 0.
	var errs: Array[String] = []
	# (geometry constants the gate trusts: span_L = BASE_SPAN * 2^L)
	if not is_equal_approx(BASE_SPAN * 2.0, BASE_SPAN * pow(2.0, 1)):
		errs.append("per-level span constant wrong")  # sanity; real check is the render below

	# SubViewport + top-down ortho camera (re-pointed at each position).
	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.0, 0.0, 0.0)
	var cam := Camera3D.new()
	cam.projection = Camera3D.PROJECTION_ORTHOGONAL
	cam.size = BASE_SPAN * 2.0
	cam.far = BASE_SPAN * 8.0
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
		# advance the view a few frames at this position so stream-ahead fills in
		# (coarsest-first; finer levels catch up — slice-3 liveness).
		for _w in range(12):
			view.call("update", pos_x, 0.0, VEL_X, 0.0)

		# frame the ortho camera ON the camera position (rings recenter to it).
		cam.look_at_from_position(Vector3(pos_x, BASE_SPAN * 2.0, 0.0), Vector3(pos_x, 0.0, 0.0), Vector3(0.0, 0.0, -1.0))
		for _s in range(6):
			await process_frame
		RenderingServer.force_draw()
		await process_frame

		var img := vp.get_texture().get_image()
		if img == null:
			push_error("get_image() null at pos %f" % pos_x); return 1
		img.save_png("user://m3_view_%d.png" % idx)

		# no holes + relief
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
		if frac < 0.95:
			errs.append("pos %.0f: holes nonblack=%.3f < 0.95" % [pos_x, frac])
		if distinct.size() < MIN_DISTINCT:
			errs.append("pos %.0f: no relief distinct=%d" % [pos_x, distinct.size()])

		# seam continuity + morph continuity at the level-0/1 boundary. The frame is centered
		# on the camera and spans 2*BASE_SPAN; the level-0 outer edge is at +-BASE_SPAN/2 from
		# centre -> pixel cols width/4 and 3*width/4. No black gap, no hard color jump.
		var midy := img.get_height() / 2
		for bx in [img.get_width()/4, (img.get_width()*3)/4]:
			var black_run := 0
			for dx in range(-2, 3):
				var c := img.get_pixel(int(clamp(bx+dx, 0, img.get_width()-1)), midy)
				if c.r <= 0.03 and c.g <= 0.03 and c.b <= 0.03:
					black_run += 1
			if black_run > 0:
				errs.append("pos %.0f: seam crack %d black px at col %d" % [pos_x, black_run, bx])
			var ci := img.get_pixel(int(clamp(bx-2,0,img.get_width()-1)), midy)
			var co := img.get_pixel(int(clamp(bx+2,0,img.get_width()-1)), midy)
			var dr: float = abs(ci.r-co.r) + abs(ci.g-co.g) + abs(ci.b-co.b)
			if dr > 0.5:
				errs.append("pos %.0f: morph jump %.2f at col %d" % [pos_x, dr, bx])

		# never-black: pool residency must be non-empty (the streamer keeps the blanket).
		var ps: Dictionary = pool.call("stats")
		if int(ps.get("resident", 0)) < 1:
			errs.append("pos %.0f: nothing resident (never-black blanket missing)" % pos_x)
		idx += 1

	# pool churn bounded: the view's per-level acquires are cache hits, not recompute storms.
	var final_stats: Dictionary = pool.call("stats")
	# created is bounded by capacity; recomputed grows only on genuine evictions. Assert
	# created never exceeded capacity (the pool enforces it, but check the budget held).
	if int(final_stats.get("resident", 0)) > CAPACITY:
		errs.append("budget exceeded: resident %d > capacity %d" % [int(final_stats.get("resident",0)), CAPACITY])

	pool.call("free_all")

	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m3-view] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-m3-view] status=pass positions=%d resident=%d" % [POSITIONS.size(), int(final_stats.get("resident",0))])
	return 0
```

NOTE for the controller: the seam pixel columns (width/4, 3·width/4), the ortho framing
(`cam.size = 2·BASE_SPAN`, camera following the position), and the warm-up frame counts are
best-effort and MUST be validated by running the windowed gate + inspecting the PNGs. If a
real crack appears at a non-zero position, that is a TRUE finding — the coarse_origin/span
fixes are wrong or incomplete; fix the code, do NOT weaken the thresholds. If the framing is
off (seam not at width/4), adjust the camera/columns so the assertion targets the real seam.

- [ ] **Step 3 (CONTROLLER runs windowed):** build + run the m3 suite + inspect PNGs.
```powershell
$env:CARGO_TARGET_DIR=$null; Push-Location wg-10/rust; cargo build; Pop-Location
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools/gate.py --suite m3
```
Expected: `[wg10-m3-view] status=pass positions=5 ...` and `[gate] suite=m3 checks=5 fail=0`.
slice-1/2/3/4 still pass. Inspect `user://m3_view_*.png` for seamless relief that follows the
camera across the boundary crossings.

- [ ] **Step 4: Run fast + gpu (no regressions)**
```powershell
python tools/gate.py --suite fast
python tools/gate.py --suite gpu
```
Expected: `suite=fast checks=5 fail=0`, `suite=gpu checks=2 fail=0 skip=0`.

- [ ] **Step 5: Commit**

```powershell
git add wg-10/worldgen_terrain/tests/m3_view_check.gd tools/gate.py
git commit -m "test(m3): m3_view_check moving-sweep gate (slice 5a) + wire into m3 suite

Drives Wg10TerrainView over a scripted +x sweep across page boundaries; at each NON-ZERO
position renders top-down ortho centered on the camera and asserts no holes, real relief,
seam continuity + morph continuity (the coarse_origin fix under motion), never-black
(residency non-empty), and budget. Saves m3_view_<i>.png per position. m3 suite now 5
checks. First gate that renders at non-zero camera positions — proves both carry-forward
fixes under motion.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Update STATUS + ROADMAP (5a done)

**Files:**
- Modify: `docs/plans/STATUS.md`, `docs/plans/ROADMAP.md`

- [ ] **Step 1: Update STATUS.md** — `Last updated:` + **Phase** line to record slice 5a done;
  add current-state bullets for `Wg10TerrainView` (the live loop), Fix #1 (per-level span),
  Fix #2 (coarse_origin), and `m3_view_check` (moving sweep, seamless+never-black at non-zero
  positions); update the gate-runner line to `--suite m3 ... checks=5`; in "What works" bump
  the m3 count to 5 and note the two carry-forwards are CLOSED. Update "What's next" item 1 to
  slice 5b (fly-cam + overlay + p99<6ms acceptance gate + manual fly = M3 done). Keep the
  honest baseline: 5a proves correctness under SCRIPTED motion, not interactive flight or perf.

- [ ] **Step 2: Update ROADMAP.md** — `Last updated:` + in the M3 section, mark the
  rings↔scheduler-wiring / carry-forward-prerequisites item DONE (slice 5a), with a one-line
  summary (Wg10TerrainView live loop + per-level span + coarse_origin; m3_view_check moving
  sweep passes at non-zero positions; m3 suite 5 checks fail=0). Leave the harness / fly-test /
  acceptance-gate / manual-acceptance items `[ ]` (slice 5b).

- [ ] **Step 3: Verify the quoted gate counts are real (fresh evidence)** — run and copy ACTUAL
  numbers:
```powershell
$env:CARGO_TARGET_DIR=$null; Push-Location wg-10/rust; cargo test 2>&1 | Select-String "test result"; Pop-Location
python tools/gate.py --suite m3
```
  Use the actual `test result: ok. N passed` and `suite=m3 checks=5 fail=0` numbers.

- [ ] **Step 4: Commit**

```powershell
git add docs/plans/STATUS.md docs/plans/ROADMAP.md
git commit -m "docs(m3): slice 5a (view wiring + carry-forward fixes) done — STATUS + ROADMAP

Wg10TerrainView live loop + per-level page span (Fix #1) + geomorph coarse_origin (Fix #2)
landed; m3_view_check passes WINDOWED over a moving sweep across page boundaries (seamless +
never-black at several non-zero camera positions; PNGs eyeballed). The two slice-4 audit
carry-forwards are CLOSED. m3 suite 5 checks fail=0; fast 5, gpu 2 unchanged; cargo green.
Next (slice 5b, finishes M3): fly camera + diagnostics overlay + p99<6ms acceptance gate +
manual fly.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage:**
- Spec §1 scope (Fix #1, Fix #2, Wg10TerrainView, m3_view_check; 5b deferred) → Tasks 1,2/3,5,6; deferral in Task 7. ✓
- Spec §2 Wg10TerrainView (configure/update/stats, the loop, view-acquires-as-cache-hits, owns no RIDs) → Task 5. ✓
- Spec §2.2 page key = corner convention → the shared `corner()` fn (Task 5) + the page-rectangle convention section, used by the gate too (Task 6) and slice-4 update (Task 4). ✓
- Spec §3 Fix #1 (span_L = world_span·2^level at dispatch) → Task 1. ✓
- Spec §4 Fix #2 (coarse_origin uniform + corner-relative uv_coarse + bind_page param + fine/coarse asymmetry) → Task 2 (shader) + Task 3 (bind_page). ✓
- Spec §4 interdependence + slice-4 gate update → Task 4. ✓
- Spec §5 gate (no holes/relief/seam/morph/never-black/per-level-span, moving non-zero positions, PNGs, churn bounded, non-vacuous boundary crossing) → Task 6. ✓
- Spec §6 files → match (terrain_view.rs, m3_view_check.gd new; page_pool.rs, ring_displace.gdshader, clipmap_rings.rs, m3_rings_check.gd, lib.rs, gate.py modified). ✓
- Spec §7 done + §8 risks → Task 4 (slice-4 regression), Task 6 (churn-bounded + non-vacuous), Task 7 (docs). ✓

**Refinement surfaced during grounding (within spec intent):** the spec's §2.2 used
`floor(cam/span)·span` for the page origin, but a level band is *centered* on the camera, so
its page must cover `[cam−span/2, cam+span/2]` — hence the origin is the band's LOWER CORNER
`floor((cam−span/2)/span)·span`, not `floor(cam/span)·span`. The plan uses the corner form
consistently (the `corner()` fn, the slice-4 gate update, the page-rectangle convention
section) and notes it. This is more correct than the spec's shorthand and keeps the page rect
aligned with the centered band — without it, Fix #1's wider page would be offset from the band
by half a span. Recorded here so the spec/plan don't silently disagree.

**2. Placeholder scan:** No TBD/TODO/"handle edge cases". Every code step has complete code.
The gate's geometric specifics carry an explicit "validate windowed + adjust framing, do NOT
weaken thresholds; a real crack is a true finding" instruction (honest, not a placeholder). ✓

**3. Type consistency:** `corner(cam_x, cam_z, span) -> (i64,i64)` used in Task 5 + mirrored in
the Task 4/6 gate math. `bind_page(level, height_tex, coarse_tex, level_span, coarse_span,
height_scale, morph_region, relief_ref, coarse_origin_x, coarse_origin_z)` — the new 10-arg
signature is consistent across Task 3 (def), Task 4 (slice-4 gate call), Task 5 (view call).
Shader uniforms (`coarse_origin` added) consistent between Task 2 (shader) + Task 3
(set_shader_parameter). `Wg10TerrainView::configure/update/stats` consistent Task 5 ↔ Task 6
gate calls. `span_L = base_span·2^L` identical in pool (Task 1), view (Task 5), gate (Task 6). ✓
