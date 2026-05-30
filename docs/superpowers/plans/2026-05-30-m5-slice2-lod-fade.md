# M5 Slice 2 — LOD fade (Implementation Plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **PRECONDITION:** Do NOT start this slice until M5 Slice 1 is OWNER-ACCEPTED (the
> acceptance fly confirms the detail look). S2 modifies the same detail S1 added; if the
> owner's fly surfaced a look problem, fold that fix in first. STATUS records S1 as
> "gated, not accepted" until the fly.

**Goal:** Make the S1 detail fade to zero where the clipmap blends to a coarser level (the
geomorph band) and on the coarsest level — so detail never fights the LOD morph, never
pops at a level boundary, and never tries to render sub-vertex frequencies on a coarse
mesh that can't carry them.

**Architecture:** Pure shader change in `ring_displace.gdshader`. The shader already
computes the geomorph factor `t` (0 = pure fine … 1 = pure coarse) and receives per-tile
`world_span` (this level's page span). Detail is multiplied by a fade `= (1.0 - t) *
level_detail_scale`, where `level_detail_scale` is derived from the tile's own span (finer
tiles = full detail, coarser = less) — computed IN-SHADER from `world_span` vs a new
global reference, so NO new per-tile uniform is needed (stays under `bind_tile`'s 15-arg
cap; Rust untouched).

**Tech Stack:** Godot 4.6 spatial shader; the existing `m5_detail_check.gd` gate extended
with an LOD-fade assertion; no Rust change.

---

## File structure

- **Modify:** `wg-10/worldgen_terrain/shaders/ring_displace.gdshader` — multiply detail by
  `(1.0 - t) * level_detail_scale`; add a global `wg_detail_fade_ref_span` uniform that
  sets the span at/above which detail fades out. One self-contained change in `vertex()`
  plus one uniform decl.
- **Modify:** `wg-10/worldgen_terrain/m5/m5_detail_check.gd` — add an LOD-fade assertion:
  render a coarse-span tile (or a tile inside the morph band) and assert detail
  contribution → ~0 there, while a fine tile keeps full detail. Extends the existing
  bounded/edge-safe/non-vacuous checks (keep those).
- **Modify (defaults):** `wg-10/worldgen_terrain/harness/m3_review.gd` — register the new
  global `wg_detail_fade_ref_span` with a sensible default (so the owner fly shows the
  fade). No new key.

> **Why span-derived, not a per-level uniform:** `bind_tile` is at gdext's 15-arg cap; the
> view doesn't pass a per-tile "detail scale." But each tile already gets `world_span` (=
> base_span·2^level), which IS the level indicator. `level_detail_scale =
> smoothstep(fade_ref_span*2, fade_ref_span*0.5, world_span)` → fine (small span) = 1,
> coarse (large span) = 0, with a smooth band. Zero Rust change. The exact curve is a
> Slice-4 config knob; S2 uses a fixed, documented default.

---

## Task 1: Write the failing LOD-fade assertion in the gate

Extend `m5_detail_check.gd` with a check that detail vanishes on a coarse-span tile and in
the morph band, while persisting on a fine tile. It FAILS now because S1 detail is flat
(no fade) — a coarse tile still shows full detail.

**Files:** Modify `wg-10/worldgen_terrain/m5/m5_detail_check.gd`

- [ ] **Step 1: Add the LOD-fade check function**

Add this method to `m5_detail_check.gd` (it reuses `_make_tile_material` / `_capture_one_tile`
from S1 — confirm their exact signatures in the file and adapt the calls if needed). The
method renders one tile at a FINE span and one at a COARSE span (same world region, detail
on) and compares how much detail each carries, using the detail-on-minus-detail-off mean
luminance diff as the "detail energy" proxy already used by `_mean_abs_diff`:

```gdscript
# LOD fade: detail must vanish on a COARSE-span tile (and in the morph band) but persist on
# a FINE tile. We measure "detail energy" = mean |detail_on - detail_off| for a tile rendered
# at a given world_span, by overriding the material's world_span/coarse_span to simulate the
# level. A fine tile (small span) keeps energy; a coarse tile (>= fade_ref_span) drops to ~0.
func _lod_fade_ok() -> bool:
	var fine_energy := await _detail_energy_at_span(WORLD_SPAN)            # level 0 span
	var coarse_energy := await _detail_energy_at_span(WORLD_SPAN * 16.0)   # ~level 4 span
	# fine keeps detail; coarse is faded to near-zero. Require coarse << fine.
	var faded := coarse_energy < fine_energy * 0.15
	print("[wg10-m5]   lod_fade fine_energy=%.5f coarse_energy=%.5f faded=%s" % [fine_energy, coarse_energy, faded])
	return faded and fine_energy > 0.0005   # fine must still carry real detail

# Render one tile at the given world_span (detail off then on) and return the mean luma diff.
func _detail_energy_at_span(span: float) -> float:
	var off := await _capture_one_tile_span(0.0, span)
	var on := await _capture_one_tile_span(DETAIL_AMP, span)
	if off == null or on == null:
		return -1.0
	return _mean_abs_diff(off, on)
```

And add a span-parameterized capture (a variant of `_capture_one_tile` that sets
`world_span`/`coarse_span` to `span` and frames the ortho camera to that span). If
`_capture_one_tile` already takes only `(amp, origin_x)`, add `_capture_one_tile_span(amp,
span)` mirroring it but with `cam.size = span`, `mesh.size = Vector2(span, span)`, and the
material's `world_span`/`coarse_span` set to `span`, `level_half_extent = span * 1.5`,
`page_origin = (0,0)`. Acquire a page sized for that level (use `acquire_page(level, 0, 0)`
with the level matching the span, OR reuse the level-0 page — the gate is testing FADE
magnitude vs span, the page content is secondary; document the choice).

- [ ] **Step 2: Wire the check into `_run` and require it**

In `_run()`, after the existing `edge_safe` line, add:
```gdscript
	var lod_fade := await _lod_fade_ok()
```
Change the `ok` line to include it:
```gdscript
	var ok := non_vacuous and bounded and edge_safe and lod_fade
```
And add `lod_fade=%s` to the final print.

- [ ] **Step 3: Run the gate — verify it FAILS on lod_fade**

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
& $env:GODOT_BIN --path wg-10 --import
& $env:GODOT_BIN --path wg-10 --script res://worldgen_terrain/m5/m5_detail_check.gd
```
Expected: `... lod_fade=false ... -> FAIL`, exit 1. (S1 detail is flat, so the coarse tile
carries the SAME detail energy as the fine tile → `coarse_energy` is NOT << `fine_energy`.)

- [ ] **Step 4: Commit the failing extension**

```bash
git add wg-10/worldgen_terrain/m5/m5_detail_check.gd
git commit -m "M5 s2: extend gate with LOD-fade assertion (RED — flat detail doesn't fade on coarse)"
```

---

## Task 2: Implement the LOD fade in the shader

Multiply detail by `(1 - t) * level_detail_scale`. The geomorph `t` zeroes detail in the
morph band; `level_detail_scale` (from span) zeroes it on coarse levels.

**Files:** Modify `wg-10/worldgen_terrain/shaders/ring_displace.gdshader`

- [ ] **Step 1: Add the fade-reference global uniform**

After the existing `global uniform float wg_detail_amp;` line, add:
```glsl
// M5 S2: world_span at/above which detail fades out (coarse levels carry no fine detail —
// they can't render sub-vertex frequencies and you can't see them at distance). GLOBAL so
// no per-tile uniform is needed (bind_tile is at the arg cap). Default set by harness/gate.
global uniform float wg_detail_fade_ref_span;
```

- [ ] **Step 2: Apply the fade to the detail term**

In `vertex()`, find the S1 detail line (it reads roughly):
```glsl
	float detail = wg_fbm_detail(world.xz) * wg_detail_amp;
```
Replace it with:
```glsl
	// M5 S2 LOD fade: detail dies (a) in the geomorph band toward coarse via (1 - t), so it
	// never fights the LOD morph or pops at a level boundary; and (b) on coarse levels via a
	// span-driven scale (fine span -> 1, span >= fade_ref -> 0). fade_ref<=0 disables the
	// span fade (back-compat: behaves like S1 flat detail).
	float morph_fade = 1.0 - t;
	float level_detail_scale = 1.0;
	if (wg_detail_fade_ref_span > 0.0) {
		level_detail_scale = smoothstep(wg_detail_fade_ref_span * 2.0, wg_detail_fade_ref_span * 0.5, world_span);
	}
	float detail = wg_fbm_detail(world.xz) * wg_detail_amp * morph_fade * level_detail_scale;
```

(NOTE: `t` is the existing geomorph factor computed above in `vertex()`; `world_span` is the
existing per-tile uniform = this level's page span. Confirm both names in the file.)

- [ ] **Step 3: Run the gate — verify GREEN**

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
& $env:GODOT_BIN --path wg-10 --import
& $env:GODOT_BIN --path wg-10 --script res://worldgen_terrain/m5/m5_detail_check.gd
```
Expected: `non_vacuous=true ... bounded=true ... edge_safe=true ... lod_fade=true ... -> PASS`,
exit 0. The gate must set `wg_detail_fade_ref_span` to a value that makes the level-16 span
tile fade (e.g. in the gate's setup add
`RenderingServer.global_shader_parameter_set("wg_detail_fade_ref_span", WORLD_SPAN * 4.0)`
so spans >= ~4× base fade out, fine stays). ADD that param registration + set to the gate's
`_run` (mirror how `wg_detail_amp` is registered there), and to `_capture_one_tile_span`'s
material so the fade actually applies.

- [ ] **Step 4: Confirm edge-safety SURVIVES the fade**

The fade is a function of `t` (world-position-derived, symmetric across a seam) and
`world_span` (same for both abutting tiles of the same level), so it stays edge-safe. The
gate's `edge_safe` check (separate-tile seam compare) must still pass — confirm
`seam_max_luma_delta < 0.01` in the GREEN output above. If it regressed, STOP: the fade
introduced a per-tile asymmetry (a real finding) — report it.

- [ ] **Step 5: Commit**

```bash
git add wg-10/worldgen_terrain/shaders/ring_displace.gdshader
git commit -m "M5 s2: LOD fade — detail vanishes into morph band (1-t) + on coarse levels (span scale); edge-safe preserved (gate GREEN)"
```

---

## Task 3: Register the fade default in the harness + run full suites

**Files:** Modify `wg-10/worldgen_terrain/harness/m3_review.gd`

- [ ] **Step 1: Register the global fade-ref default**

In `m3_review.gd`, add a constant near `DETAIL_AMP`:
```gdscript
const DETAIL_FADE_REF_SPAN := 32768.0   # span (m) at which fine detail fades out (= BASE_SPAN*4)
```
In `_ready()`, after the `wg_detail_amp` registration, add:
```gdscript
	RenderingServer.global_shader_parameter_add("wg_detail_fade_ref_span", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, DETAIL_FADE_REF_SPAN)
```
(BASE_SPAN is 8192; ×4 = 32768 → detail full on levels 0-1, fading by level 2-3, gone on the
coarse far levels. This is a documented default; the exact value is a Slice-4 config knob.)

- [ ] **Step 2: Run m3 + gpu + fast suites**

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools/gate.py --suite m3
python tools/gate.py --suite gpu
python tools/gate.py --suite fast
```
Expected: `m3 checks=7 fail=0`, `gpu checks=4 fail=0` (facts parity unchanged — fade is
render-only), `fast checks=6 fail=0`. Report the literal lines.

- [ ] **Step 3: Commit**

```bash
git add wg-10/worldgen_terrain/harness/m3_review.gd
git commit -m "M5 s2: register wg_detail_fade_ref_span default in m3_review (m3 7/7, parity unchanged)"
```

---

## Task 4: Owner acceptance (S2) + STATUS

- [ ] **Step 1: Owner flies the morph/coarse boundaries.** Launch `m3_review.tscn`, fly
  across LOD level boundaries at speed. Confirm: detail is present up close, fades smoothly
  as levels coarsen (no hard pop where detail abruptly appears/disappears), and the morph
  band shows no new shimmer. Press `N` to A/B. Record verbatim.

- [ ] **Step 2: Update STATUS.md** — M5-S2 entry: LOD fade landed (morph-band + span scale),
  gate's lod_fade assertion green, edge-safe preserved (seam still < 0.01), m3 7/7, gpu 4/4,
  facts parity unchanged, owner result. Note S3 (descriptor + slope modulation) next.

- [ ] **Step 3: Commit STATUS.**
```bash
git add docs/plans/STATUS.md
git commit -m "M5 s2: STATUS — LOD fade landed, gates green, owner result recorded"
```

---

## Self-review notes (planner)

- **Spec coverage (S2):** spec §9 S2 = "tie detail to (1−t) × per-level scale; gate detail→0
  on coarsest + morph band; no new LOD pop." Task 2 implements both fade factors; Task 1
  gates the coarse-fade; Task 2 Step 4 + the existing seam test guard edge-safety; Task 4 is
  the owner no-pop judgment.
- **Arity:** fade uses existing `t` + `world_span` + one GLOBAL uniform → no `bind_tile`
  change, Rust untouched (cargo stays 115).
- **Back-compat:** `wg_detail_fade_ref_span <= 0` disables the span fade → S1 behavior, so
  any gate not setting it is unaffected.
- **Risk — morph_fade double-counts with geomorph:** `(1-t)` also scales detail in the
  morph band; since the base height there is already blending fine→coarse, killing detail
  there is correct (don't add fine detail to a surface that's becoming coarse). Confirmed
  intent, not a bug.
- **Gate honesty:** the lod_fade check simulates levels by overriding `world_span` on a
  level-0 page rather than acquiring true coarse pages — documented; it tests the FADE-vs-
  span relationship, which is the S2 invariant. If a reviewer wants true-coarse-page
  realism, that's a strengthening (like the S1 seam-test strengthening), addable in review.
- **Names:** `wg_detail_fade_ref_span`, `level_detail_scale`, `morph_fade`,
  `DETAIL_FADE_REF_SPAN`, `_lod_fade_ok`, `_detail_energy_at_span`, `_capture_one_tile_span`
  used consistently across tasks.
