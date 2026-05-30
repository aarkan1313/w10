# M3 Slice 8 — Visual Stability (seam-free + correct geomorph) Design

**Status:** spec
**Date:** 2026-05-29
**Milestone:** M3 (render pipeline at speed) — the slice the owner's manual fly demanded.

## Why this slice exists

The M3 automated gate (`m3_accept_check`) is GREEN on p99/no-black/never-stall, but the
owner's manual fly of `m3_review.tscn` reported "crazy amounts of switching and stuff
happening" at speed. A code-level trace (generation → sampling → mesh) pinned this to **three
distinct defects in the render-time sampling** — the height *field* is continuous; only the way
the ring shader samples it is wrong. The top-down/scripted gate never saw them because it
measured timing and non-black coverage, not **perceptual surface continuity in a perspective
POV**. That is the gap this slice closes: fix the three sampling defects AND add a windowed
visual-continuity gate so they cannot silently regress.

This keeps M3 correctly OPEN: the owner's fly found a real defect (DESIGN §7.3 — the manual fly
is the final authority; automated-green is necessary, not sufficient).

## The three defects (root cause, code-confirmed)

### How pages are generated (the ground truth)
`height_page.glsl` `main()` currently writes each pixel from the world height field with a
**texel-center** convention:
```
u = (px + 0.5) / page_px ;  wx = origin_x + u * span    // px in [0, page_px)
```
`height_at(wx,wz)` is deterministic and continuous, so two pages that abut evaluate the **same
height at the same world point**. The field has no discontinuity; every artifact below is a
*sampling* defect in `ring_displace.gdshader`.

### Defect 1 — geomorph fires at every tile edge (the dominant "switching")
`ring_displace.gdshader`:
```
float cheb = max(abs(VERTEX.x), abs(VERTEX.z)) / half_span;   // TILE-local: 1 at THIS tile's edge
```
A level is a 3×3 neighborhood of 9 one-page tiles (slice 5b). `VERTEX.xz` is tile-local, so the
morph-to-coarse blend triggers at **all 9 tiles' outer edges** — an interior lattice of blend
transitions that sweeps under motion. The morph must engage **only at the level's true outer
ring** (the outward edge of the 3×3 neighborhood), i.e. as a function of distance from the
**neighborhood center**, normalized to the neighborhood half-extent.

### Defect 2 — fine UV samples texel borders, not centers
```
vec2 uv_fine = (VERTEX.xz / world_span) + vec2(0.5);   // edge vertex -> uv in {0,1} = texture BORDER
```
With the texel-center generation convention, texel 0's *center* is at `uv = 0.5/N`, not `0`. An
edge vertex at `uv=0` (or `1`) reads the border-clamped edge texel — a half-texel
misregistration. At a shared tile edge, tile A reads `uv=1.0` and tile B reads `uv=0.0`, each
offset half a texel the *opposite* way → a visible height step (compounds Defect 3).

### Defect 3 — abutting pages share no boundary sample
Adjacent pages are exactly `span` apart (`origin` is `floor(cam/span)·span`-quantized). Under
the texel-center convention the shared edge world line `X = origin_B` has **no sample on either
page** — page A's nearest sample (texel N-1 center) and page B's nearest (texel 0 center) are
one texel apart, straddling the boundary. So even a perfect half-texel inset leaves a residual
sub-texel step. To make the shared edge **bit-identical from both sides**, abutting pages must
share their boundary samples. (Chosen fix: texel-corner convention — below.)

## The fix (one coherent change across generation + shader + view)

### Decision: texel-corner sampling convention (provably gapless)
Change `height_page.glsl` to a **texel-corner** convention:
```
u = px / float(page_px - 1) ;  wx = origin_x + u * span     // texel 0 at origin, texel N-1 at origin+span
```
Now texel 0 sits exactly at `origin`, texel N-1 exactly at `origin+span`. Page B's origin is
`origin_A + span`, so **B's texel 0 (world origin_A+span) == A's texel N-1 (world origin_A+span)
== identical `height_at` value**. Abutting pages share their boundary row/column by construction
— "gapless by construction" (DESIGN §5.1), seam provably zero, no skirt, no overlap allocation.

The ring shader then samples the fine page by **true world UV** matching this convention:
```
vec2 uv_fine = (world.xz - page_origin) / world_span;       // uv=0 -> texel 0 (=origin), uv=1 -> texel N-1
```
`page_origin` is the new shader uniform (the fine page's world lower-XZ corner — the view
already computes `po_x/po_z`). Under linear filtering an edge vertex maps exactly onto the
shared boundary texel, so adjacent tiles agree to the bit. The coarse sampler (already
world-relative via `coarse_origin`) adopts the same corner convention automatically because its
page is produced by the same `height_page.glsl`.

### Decision: geomorph from neighborhood-center distance
The view passes the **level neighborhood center** (camera-quantized, the shared center of the
3×3) and the morph normalizes to the neighborhood half-extent `1.5 * span_l` (a 3×3 of
`span_l`-wide tiles spans `3*span_l`; half-extent `1.5*span_l`). Replace the tile-local `cheb`:
```
// uniforms (new): level_center (vec2 world), level_half_extent (float = 1.5*span_l)
float cheb = max(abs(world.x - level_center.x), abs(world.z - level_center.z)) / level_half_extent;
float region = max(morph_region, 1e-6);
float t = clamp((cheb - (1.0 - region)) / region, 0.0, 1.0);  // 0 in interior, 1 at neighborhood outer ring
```
Interior tiles get `t=0` everywhere (pure fine, continuous with neighbors); only the outer ring
of tiles blends, only on their outward side — engaging exactly where the next-coarser level
takes over. The coarsest level passes `morph_region=0` (already does, view line 109).

`level_center` is the same `(center_x, center_z)` the view already computes per level
(`floor(cam/span_l)*span_l`) — but note that is a page-origin corner, not the neighborhood
center. The neighborhood center is `center + (span_l*0.5, span_l*0.5)` shifted to the middle
tile's center; spec'd precisely in the plan. (The neighborhood of 3×3 page tiles with the middle
tile at page-origin `center` has its geometric center at `center + span_l*0.5`.)

## Components changed

| File | Change |
|---|---|
| `wg-10/worldgen_terrain/shaders/height_page.glsl` | `main()` u/v mapping: texel-center → texel-corner. `height_at` UNCHANGED (formula sync with `height_field.glsl` preserved). |
| `wg-10/worldgen_terrain/shaders/ring_displace.gdshader` | `uv_fine` by world (new `page_origin` uniform); morph `cheb` by neighborhood center (new `level_center`, `level_half_extent` uniforms). |
| `wg-10/rust/src/clipmap_rings.rs` | `bind_tile` sets the 3 new uniforms: `page_origin`, `level_center`, `level_half_extent`. |
| `wg-10/rust/src/terrain_view.rs` | compute + pass `level_center = (center + span_l*0.5)` and `level_half_extent = 1.5*span_l` per level; pass `page_origin = (po_x, po_z)`. |
| `wg-10/worldgen_terrain/tests/m3_continuity_check.gd` (NEW) | windowed perspective visual-continuity gate (below). |
| `wg-10/tools/gate.py` | add `m3_continuity_check.gd` to the `m3` suite. |

## The visual-continuity gate (the regression catcher this slice adds)

`m3_continuity_check.gd` — windowed, SubViewport, perspective flight POV (same rig style as
`m3_accept_check`). After warm-up, at a few sampled frames it asserts **surface continuity** two
ways that the timing/non-black gate cannot:

1. **No inter-tile height seam (Defect 2/3).** Drive the camera to a known position, read back
   the bound page textures via the pool, and assert that for each pair of horizontally/vertically
   abutting resident fine pages of the same level, the shared edge column/row matches within an
   epsilon: `|A[:, N-1] - B[:, 0]| < EPS` (EPS in metres, ~1e-2 like parity). This is a *data*
   check on the page textures — deterministic, not pixel-based — and is the precise, non-flaky
   assertion that the texel-corner convention works. Pages are read back with
   `RenderingDevice.texture_get_data` (windowed; readback is allowed in a GATE, never on the
   render path).

2. **Smooth morph under motion (Defect 1).** Across consecutive frames at speed, capture the
   rendered viewport and assert no spurious high-frequency banding sweeping the interior:
   approximate by sampling a horizontal scanline of luminance and asserting the count of
   large frame-to-frame per-pixel jumps stays under a threshold (the morph-per-tile lattice
   produced many; correct morph produces ~none in the interior). Saves a PNG for eyeball.

If readback of (1) proves too device-specific for a hard gate, (1) is the authoritative
assertion and (2) is a softer ceiling + PNG artifact. (1) MUST be a hard assertion: it directly
proves the seam fix.

## What this slice does NOT do (YAGNI)
- No page-skirt/overlap allocation (texel-corner makes it unnecessary).
- No change to `height_at` (formula stays bit-synced with `height_field.glsl`; parity gates
  untouched and stay green — verified: both parity gates sample `height_field.glsl` at explicit
  coords and never exercise the page pixel→world mapping).
- No scheduler/pool RID-ownership change (this is shader + uniforms + one generation-mapping
  line + a gate).

## Pillars check
- **Adaptable:** new uniforms are config-derived from existing values (`span_l`, page origin);
  no magic numbers (`1.5` and `3` are the 3×3 radius-1 geometry, documented).
- **Performance:** shader-only per-vertex change (a few extra ALU ops + 2 uniforms); p99 gate
  must stay green (re-run `m3_accept_check`). Readback is gate-only.
- **Quality:** seam is provably zero by construction (shared boundary samples); morph engages
  only at the true level boundary; the new gate is a hard data assertion that locks it.
- **No shortcuts:** the seam is fixed at the root (shared samples), not hidden by a fudge
  factor; the gate proves the page data matches, not just "looks ok".

## Acceptance
1. `m3_continuity_check`: shared-edge page data matches within EPS (hard); morph-banding under
   ceiling; PNG saved. GREEN.
2. `m3_accept_check` still GREEN (p99 < 6 ms, no-black, never-stall, compute-frame ≤ 6 ms).
3. `gpu` suite still GREEN (parity unaffected).
4. Owner re-flies `m3_review.tscn`: no inter-tile seam, no morph lattice/switching in the
   interior, smooth LOD transition at level boundaries — then M3 closes (DESIGN §7.3).
