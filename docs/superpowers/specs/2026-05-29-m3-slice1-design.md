# WorldGen10 M3 — Slice 1: First Rendered Page Design

**Date:** 2026-05-29
**Status:** Approved (brainstorming) — pending writing-plans
**Layer:** Render pipeline (M3) — first vertical slice: one GPU height page → displaced ring mesh on screen
**Builds on:** M2 GPU formula/parity + the real DEM pack (`packs/dem_v1`)
**Followed by:** M3 slice 2+ (page pool → scheduler → multi-ring → harness → fly-test acceptance)

---

## 0. Framing

M3 is the render pipeline — the milestone WG9 died on (128 ms/chunk synchronous
page builds → black slabs at speed). DESIGN §5 is the locked anti-WG9 architecture
(fixed clipmap rings, bounded page pool, stream-ahead scheduler, never-black/
never-stall). M3 is too big for one spec and ends in a **manual visual fly-test**
(§7.3) — a verification shape unlike every prior headless milestone. So M3 is
decomposed into **vertical slices**, smallest-renderable-thing first.

**This spec = Slice 1 only:** the smallest thing that puts real DEM terrain on
screen — ONE clipmap ring mesh displaced by ONE GPU height page written to a
texture (no readback) — captured to a PNG. It proves the single scariest unknown
(*does a GPU-computed height page become correct visible terrain?*) before any of
the streaming machinery is built on top of it.

---

## 1. Scope

**In scope (Slice 1):**
- **Render-mode probe (step 1):** does a 3D viewport render to a non-black PNG
  under `--headless` on this machine? (RenderingDevice *compute* is null headless
  here — render may be too.) Result picks the slice gate's run-mode: headless if
  it works, else windowed (like the `gpu` suite). Empirical, not assumed.
- **Compute → height texture (no readback):** a `Wg10PageCompute` Godot class
  writes `height(x,z)` into an **R32F `Texture2DRD`** for one page region, using
  the SAME hash→grammar→height formula as M2's `height_field.glsl` (only the
  output binding changes: `imageStore` to a texture, not a readback buffer).
- **Ring mesh + displacement shader:** one flat grid mesh (single clipmap level)
  with a **spatial shader** whose `vertex()` samples the height texture to set
  `VERTEX.y` — the real no-readback render path. Fragment colors by height/flat-
  shade so relief is *legible* in the PNG (pretty lighting/normals are later).
- **Slice scene + screenshot gate:** `m3_slice1.tscn`/`.gd` dispatches the page,
  builds the displaced ring, places a fixed camera, renders, saves `m3_slice1.png`.
  `m3_slice1_check.gd` asserts the frame has real height variance (non-flat,
  non-black, finite — no NaN garbage). A new `m3` gate suite (run-mode per probe).
- **Human inspection:** the agent reads `m3_slice1.png` to confirm it reads as
  DEM terrain (the honest "looks right" check); the owner can glance later.

**Out of scope (later M3 slices — do NOT build):** page pool (LRU/eviction/
single-RID-owner), stream-ahead scheduler (velocity, bounded computes/frame,
coarser-fallback), multiple concentric rings + L↔L+1 morph, recenter-on-move,
the modular harness (camera/movement/diagnostics/profiling/UI components), the
manual fly review scene, the p99<6 ms perf gate. **Slice 1 is STATIC** — one
page, one ring, one frame. No movement, no streaming, no perf claim.

---

## 2. Pillar alignment (DESIGN §1 — the acceptance lens)

This slice is checked against all four pillars, because "are we following the
pillars" is the real soundness test:

1. **Adaptable/tunable (top):** the page's origin/world-span/resolution and the
   ring grid resolution are **config-style args, never hardcoded magic numbers**
   (the §6.1 "config is the only source" rule applies even to the slice). The
   slice reuses the data-driven DEM pack; it bakes in no world-specific values.
2. **Performance (GPU-shaped):** the slice IS the **no-readback** path
   (compute→texture→vertex-sample) — the anti-WG9 performance foundation, correct
   in *shape* from the first slice even though the perf *number* (p99<6 ms) is a
   later slice's gate. No synchronous per-chunk work.
3. **Quality (bounded, correct):** value-correctness leans on M2's bit-exact
   CPU/GPU parity (same formula); the new gate proves the render path; the PNG is
   inspected. No collapse, no silent defaults.
4. **No shortcuts (validate, don't assume):** the ONE sanctioned fallback (§3) is
   strictly disciplined (see below) so an expedient choice cannot ossify into the
   foundation.

### The fallback-discipline rule (load-bearing, "no shortcuts")

The real path (compute→`Texture2DRD`→spatial-material vertex sample) is the
genuine gdext/Godot-interop unknown this slice exists to surface. **We try the
real path first.** If `Texture2DRD`→spatial-vertex sampling proves impractical in
gdext 0.5.3 within reasonable effort, a **slice-1-ONLY** fallback is permitted:
CPU-fill an `ImageTexture` once from `height(x,z)` and sample THAT. But:
- it is a **throwaway** unblock to prove the texture→mesh→screen render path, NOT
  the M3 model (it is readback-shaped, violates no-readback);
- it must be **flagged loudly** in STATUS + a code comment as "NOT the real path —
  slice 2 replaces this";
- **slice 2 MUST replace it** with the true no-readback texture path before any
  streaming is built. It can never silently become the foundation.

If the real path works (expected/hoped), no fallback is used and this rule is moot.

---

## 3. Compute → height texture (`page_compute.rs` + `height_page.glsl`)

- New `Wg10PageCompute` (`#[derive(GodotClass)]`, RefCounted), sibling to M2's
  `Wg10GpuCompute` (which stays untouched so parity holds). It loads the DEM pack
  (reuse `pack::load_pack_dir`), builds the same pack buffers (reuse M2's
  buffer-builder — or factor it shared), and dispatches a compute shader that
  writes height into an **R32F image** for a page region
  `(origin_x, origin_z, world_span, page_px)`.
- `height_page.glsl`: a compute shader with the **identical** hash→grammar→height
  math as `height_field.glsl`, but output is `imageStore(height_img, ivec2(gid),
  vec4(h))` into a bound R32F `image2D` instead of a storage buffer. **Formula
  sync:** the shared math (integer hash, grammar rolls, sample_kernel, moderation,
  composition) must stay identical to `height_field.glsl` and the Rust. The plan
  decides: either factor the shared formula into a `.glsl` include both shaders
  pull, OR duplicate with an "EDIT BOTH SIDES" header + a parity check. Prefer the
  include (one source) if gdext's shader compile supports it; else duplicate +
  gate. (M2's `gpu_parity` already proves the formula values; this slice does not
  re-derive them.)
- `Wg10PageCompute` exposes the height texture as a `Texture2DRD` the renderer
  binds. **No `buffer_get_data` in this path** — the texture is written by compute
  and read by the vertex shader, never copied to the CPU.

## 4. Ring mesh + displacement shader (`ring_displace.gdshader`)

- One flat grid mesh (generated grid or `PlaneMesh` with subdivisions,
  resolution from config) in the XZ plane, sized to the page's world span.
- A **spatial shader**: `vertex()` computes the page UV from the vertex's local
  XZ (`uv = (local_xz / world_span) + 0.5`), samples the bound height texture
  (`texture(height_tex, uv).r`), sets `VERTEX.y = sampled_height`. `fragment()`:
  color by height (a simple gradient) and/or flat normal so relief is legible in
  the PNG. Full normal pages + lighting are a later slice — slice 1 needs
  *correct and legible*, not pretty.
- The vertex world→UV mapping matches the compute's page mapping exactly, so the
  displaced surface equals `height(x,z)` over the page.

## 5. Slice scene + screenshot gate

- `wg-10/worldgen_terrain/m3/m3_slice1.tscn` + `m3_slice1.gd`: instantiate
  `Wg10PageCompute`, load `packs/dem_v1`, dispatch the page (config origin/span/
  px), create the ring mesh + a material binding the height texture, place a
  fixed camera at a vantage looking across the terrain, render a small number of
  settle frames, then capture.
- **Capture:** read back the rendered *colour frame* (the final screen image) via
  `get_texture().get_image().save_png("res://.../m3_slice1.png")`. NOTE: capturing
  the colour frame for a gate PNG is NOT a violation of the no-readback rule — the
  no-readback rule is about the HEIGHT TEXTURE never being copied to the CPU in the
  render path (it isn't; compute writes it, the vertex shader reads it on-GPU). A
  one-off frame screenshot for verification is a gate concern, exactly like M2's
  one-off parity readback. (path resolves under the project; committed so the
  owner/agent can view it.)
- **Gate `m3_slice1_check.gd`:** load the PNG; assert (a) it loads, (b) pixel
  value variance exceeds a floor (real relief → not a flat/single-color frame),
  (c) all finite/in-range (no NaN/garbage). Print
  `[wg10-m3-slice1] status=pass var=<variance> ...`; return 0 (pass) / 1 (fail) /
  2 (skip, no render device). Mirrors the existing gate idioms.
- **Run mode:** per the §1 probe — headless if a headless 3D render produces a
  non-black PNG here, else windowed (new `m3` suite branch in `gate.py`, like the
  `gpu` suite's windowed branch).
- **Agent inspects `m3_slice1.png`** and reports whether it reads as DEM terrain.

## 6. Module boundaries & files
```
wg-10/rust/src/
  page_compute.rs        # NEW: Wg10PageCompute — compute -> R32F Texture2DRD (no readback). Only new Rust.
  lib.rs                 # MODIFY: mod page_compute;
  (gpu_compute/grammar/height/pack/hash/npy/parity — UNCHANGED; M2 parity intact)
wg-10/worldgen_terrain/shaders/
  height_page.glsl       # NEW: compute, imageStore height into R32F (same formula as height_field.glsl)
  ring_displace.gdshader # NEW: spatial shader; vertex samples height tex -> VERTEX.y
wg-10/worldgen_terrain/m3/
  m3_slice1.tscn         # NEW: slice scene
  m3_slice1.gd           # NEW: assemble compute + ring + camera + capture
wg-10/worldgen_terrain/tests/
  m3_slice1_check.gd     # NEW: screenshot gate (variance / finite / non-black)
  m3_slice1.png          # NEW (generated, committed): the captured frame
tools/gate.py            # MODIFY: add `m3` suite (run-mode per probe)
docs/plans/              # MODIFY: ROADMAP (M3 started, slice 1), STATUS
```
Each file one job. `page_compute.rs` is the only new Rust; the M2 crate stays
untouched so parity holds. Config values (page origin/span/px, ring grid res,
camera vantage) live in the slice scene's exported vars / a small config block —
never hardcoded constants scattered in code (pillar 1).

## 7. Done + docs
- **Done (slice 1):** the probe resolves run-mode; the real `Texture2DRD` path
  works OR the disciplined fallback is used + flagged; `m3_slice1_check.gd` passes
  (PNG has relief, finite, non-black); the agent has inspected the PNG and it
  reads as terrain; `cargo test` still green (page_compute compiles, M2 parity
  unaffected); the `m3` suite is in `gate.py`; ROADMAP/STATUS note "M3 slice 1:
  first rendered page — done (static, one page, one ring)"; each piece committed.
- **NOT claimed:** no streaming, no movement, no multi-ring, no perf number, no
  manual-fly acceptance. The M3 milestone stays OPEN. Slices 2+ (pool → scheduler
  → rings → harness → fly-test) follow, each its own spec→plan→execute cycle.

## 8. Named risks (do not solve now)
- **`Texture2DRD` → spatial-vertex interop** (§3): the genuine unknown; slice
  exists to surface it; disciplined CPU-fill fallback (§2) if it fights gdext.
- **Headless render** may be null like compute (§1 probe decides; windowed
  fallback known-good).
- **Formula duplication** between `height_page.glsl` and `height_field.glsl`: the
  plan picks include-share vs duplicate+gate; M2 parity already guards the values.
- **Page world-span vs game scale:** the slice uses the DEM footprint as the page
  span; whether that *feels* right is an M3-fly-test/visual-tuning question
  (`footprint_scale`), deferred — slice 1 only needs *correct + legible*.
