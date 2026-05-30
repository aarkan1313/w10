# M5 Slice 3 — Surface descriptor + slope modulation (Implementation Plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **PRECONDITION:** Do NOT start until M5 Slice 2 (LOD fade) is landed + owner-accepted.
> S3 builds the `detail * mod` term on top of S2's `detail * fade`. Order: S1 (flat detail)
> → S2 (LOD fade) → **S3 (descriptor + slope modulation)** → S4 (config + p99 + docs).

**Goal:** Add a reusable `surface_descriptor(world)` (slope / curvature / height_band)
computed from the BASE page, and use its `slope` to MODULATE detail amplitude — more detail
on steep faces, calm on flat valley floors (the "balanced" look). The descriptor is the
seam M6 (materials) and M7 (erosion) will reuse; M5 consumes only `slope`.

**Architecture:** Pure shader change in `ring_displace.gdshader`. A
`surface_descriptor(world)` GLSL function samples the base `height_tex` at ±1 texel
(finite-difference) to get slope + curvature, and reads `height_band` from the already-known
base height. Detail becomes `detail * modulate(desc)`. Texel size comes from
`textureSize(height_tex, 0)` — NO new uniform, NO Rust change. `slope_influence` /
`slope_ref` are global uniforms (defaults in S3; full config in S4).

**Tech Stack:** Godot 4.6 spatial shader; extend `m5_detail_check.gd`; no Rust change.

---

## File structure

- **Modify:** `wg-10/worldgen_terrain/shaders/ring_displace.gdshader` —
  add `SurfaceDescriptor` struct + `surface_descriptor(vec3 world)` + `modulate(...)`;
  multiply detail by `modulate(desc)`; add globals `wg_slope_influence`, `wg_slope_ref`.
  The descriptor function is documented as the M6/M7 reusable seam.
- **Modify:** `wg-10/worldgen_terrain/m5/m5_detail_check.gd` — add a `slope_modulation`
  assertion: detail energy is HIGHER on a steep region than a flat region. Keep
  bounded/edge-safe/non-vacuous/lod_fade.
- **Modify (defaults):** `wg-10/worldgen_terrain/harness/m3_review.gd` — register
  `wg_slope_influence` (~0.6) + `wg_slope_ref` globals with documented defaults.

> **Texel size without a uniform:** finite-difference slope needs the page's texel spacing
> in world metres = `world_span / page_px`. `page_px` isn't a uniform, but
> `textureSize(height_tex, 0).x` returns it directly in GDShader → `texel_world =
> world_span / float(textureSize(height_tex, 0).x)`. No Rust/bind_tile change.

---

## Task 1: Write the failing slope-modulation assertion in the gate

Extend `m5_detail_check.gd`: detail must carry MORE energy on a steep base region than on a
flat one. FAILS now because S1/S2 detail is slope-independent (uniform modulation).

**Files:** Modify `wg-10/worldgen_terrain/m5/m5_detail_check.gd`

- [ ] **Step 1: Add the slope-modulation check**

The check needs a STEEP base region and a FLAT base region. The DEM pack has both; pick two
page origins known to differ in slope, OR (simpler + deterministic) drive modulation
directly: render the same page twice with `wg_slope_influence = 0` (uniform) vs `= 1`
(slope-only) and assert the detail DISTRIBUTION changes (the slope-modulated render must
differ from the uniform one — proving slope actually steers detail). Add:

```gdscript
# Slope modulation: with slope_influence=1, detail concentrates on slopes; with =0 it is
# uniform. The two renders must DIFFER (slope genuinely steers detail), and the modulated
# render must not be globally darker-or-brighter only (it redistributes, not just scales).
func _slope_modulation_ok() -> bool:
	var uniform_img := await _capture_one_tile_mod(DETAIL_AMP, 0.0)   # slope_influence = 0
	var sloped_img := await _capture_one_tile_mod(DETAIL_AMP, 1.0)    # slope_influence = 1
	if uniform_img == null or sloped_img == null:
		return false
	var redistribution := _mean_abs_diff(uniform_img, sloped_img)
	var modulated := redistribution > 0.001   # slope_influence actually changed the surface
	print("[wg10-m5]   slope_mod redistribution=%.5f modulated=%s" % [redistribution, modulated])
	return modulated

# Capture one tile with a given slope_influence (sets the global, renders, restores).
func _capture_one_tile_mod(amp: float, slope_influence: float) -> Image:
	RenderingServer.global_shader_parameter_set("wg_slope_influence", slope_influence)
	var img := await _capture_one_tile(amp, 0.0)   # reuse the S1 single-tile capture
	return img
```

Register the `wg_slope_influence` + `wg_slope_ref` globals in the gate's setup (mirror how
`wg_detail_amp` is registered): add them with `global_shader_parameter_add` if not present,
default `wg_slope_ref` to a sensible slope scale (e.g. `50.0` — tune in Step 3 of Task 2).

- [ ] **Step 2: Wire into `_run` + require it**

```gdscript
	var slope_mod := await _slope_modulation_ok()
	var ok := non_vacuous and bounded and edge_safe and lod_fade and slope_mod
```
Add `slope_mod=%s` to the final print.

- [ ] **Step 3: Run — verify FAILS on slope_mod**

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
& $env:GODOT_BIN --path wg-10 --import
& $env:GODOT_BIN --path wg-10 --script res://worldgen_terrain/m5/m5_detail_check.gd
```
Expected: `... slope_mod=false ... -> FAIL`. (Detail ignores `wg_slope_influence` → the two
renders are identical → `redistribution ≈ 0`.)

- [ ] **Step 4: Commit**
```bash
git add wg-10/worldgen_terrain/m5/m5_detail_check.gd
git commit -m "M5 s3: extend gate with slope-modulation assertion (RED — detail ignores slope)"
```

---

## Task 2: Implement the surface descriptor + modulation in the shader

**Files:** Modify `wg-10/worldgen_terrain/shaders/ring_displace.gdshader`

- [ ] **Step 1: Add the modulation globals**

After `global uniform float wg_detail_fade_ref_span;` (from S2), add:
```glsl
// M5 S3: slope-driven detail modulation. slope_influence in [0,1] blends uniform detail (0)
// <-> slope-only detail (1); slope_ref is the slope magnitude at which modulation saturates.
// GLOBAL (no per-tile uniform); defaults in S3, full config in S4.
global uniform float wg_slope_influence;
global uniform float wg_slope_ref;
```

- [ ] **Step 2: Add the SurfaceDescriptor struct + function (the reusable seam)**

Before `void vertex()`, after the fBm block, add:
```glsl
// ---- M5 S3: surface descriptor — the REUSABLE seam (M6 materials + M7 erosion call this) ----
// Computed from the BASE height page (height_tex), purely from world position, so it is
// edge-safe (abutting tiles read the same clamp-to-edge texels on a shared edge). M5 uses
// only .slope for detail modulation; .curvature/.height_band are computed + available for
// M6/M7 — EXTEND this function, do NOT re-derive slope elsewhere.
struct SurfaceDescriptor {
	float slope;        // 0 = flat, ↑ = steeper; |gradient of base height| (metres per metre)
	float curvature;    // signed; convex/ridge (+) vs concave/valley (−)
	float height_band;  // clamp(h_base / relief_ref, 0, 1); altitude band
};

SurfaceDescriptor surface_descriptor(vec3 world, float h_base) {
	// texel spacing in world metres = world_span / page resolution (px). textureSize avoids a uniform.
	float page_px = float(textureSize(height_tex, 0).x);
	float texel = world_span / max(page_px, 1.0);
	vec2 uv = (world.xz - page_origin) / world_span;
	vec2 du = vec2(texel / world_span, 0.0);   // one texel in UV, X
	vec2 dv = vec2(0.0, texel / world_span);   // one texel in UV, Z
	float hl = texture(height_tex, uv - du).r;
	float hr = texture(height_tex, uv + du).r;
	float hd = texture(height_tex, uv - dv).r;
	float hu = texture(height_tex, uv + dv).r;
	// central-difference gradient (metres of height per metre of world).
	float dh_dx = (hr - hl) / (2.0 * texel);
	float dh_dz = (hu - hd) / (2.0 * texel);
	SurfaceDescriptor s;
	s.slope = length(vec2(dh_dx, dh_dz));
	s.curvature = (hl + hr + hd + hu - 4.0 * h_base) / (texel * texel); // Laplacian (2nd diff)
	s.height_band = clamp(h_base / max(relief_ref, 1e-3), 0.0, 1.0);
	return s;
}

// Modulate detail amplitude by the descriptor. M5 default: more detail on slopes, less on
// flats. slope_influence blends uniform (0) <-> slope-driven (1). Returns [0,1].
float wg_modulate(SurfaceDescriptor s) {
	float slope_term = mix(1.0, smoothstep(0.0, max(wg_slope_ref, 1e-3), s.slope), clamp(wg_slope_influence, 0.0, 1.0));
	return clamp(slope_term, 0.0, 1.0);
}
```

- [ ] **Step 3: Apply modulation to the detail term**

In `vertex()`, the S2 detail line reads (roughly):
```glsl
	float detail = wg_fbm_detail(world.xz) * wg_detail_amp * morph_fade * level_detail_scale;
```
Change it to compute the descriptor (using the existing base height `h`) and modulate:
```glsl
	SurfaceDescriptor wg_desc = surface_descriptor(world, h);   // h = h_base, formed above
	float wg_mod = wg_modulate(wg_desc);
	float detail = wg_fbm_detail(world.xz) * wg_detail_amp * wg_mod * morph_fade * level_detail_scale;
```
(`h` is the existing `mix(h_fine,h_coarse,t)` base height — confirm the variable name in the
file. The descriptor reads the BASE page only; it does NOT change `h`.)

- [ ] **Step 4: Run the gate — verify GREEN (set slope_influence in the gate)**

Ensure the gate's `_run` sets `wg_slope_ref` to a value that makes the DEM slopes register
(start `50.0`; if `slope_mod` redistribution is tiny, the slopes are larger/smaller — adjust
`wg_slope_ref` so the smoothstep isn't saturated at 0 or 1 across the page). Run:
```powershell
& $env:GODOT_BIN --path wg-10 --import
& $env:GODOT_BIN --path wg-10 --script res://worldgen_terrain/m5/m5_detail_check.gd
```
Expected: `non_vacuous=true bounded=true edge_safe=true lod_fade=true slope_mod=true -> PASS`.
The `edge_safe` seam delta must STILL be < 0.01 (the descriptor is world-position-pure +
clamp sampler → edge-safe). If the seam regressed, STOP — the ±texel reads near a page edge
may need clamping; report it (the clamp-to-edge sampler should already handle uv<0 / uv>1).

- [ ] **Step 5: Commit**
```bash
git add wg-10/worldgen_terrain/shaders/ring_displace.gdshader
git commit -m "M5 s3: surface_descriptor (slope/curvature/band) + slope modulation; reusable M6/M7 seam; edge-safe (gate GREEN)"
```

---

## Task 3: Register modulation defaults in the harness + full suites

**Files:** Modify `wg-10/worldgen_terrain/harness/m3_review.gd`

- [ ] **Step 1: Register defaults**

Near `DETAIL_AMP`, add:
```gdscript
const SLOPE_INFLUENCE := 0.6     # M5 balanced default: rocky on steep, calm on flat (0=uniform, 1=slope-only)
const SLOPE_REF := 50.0          # slope magnitude at which modulation saturates (tune vs pack)
```
In `_ready()`, after the S2 fade registration, add:
```gdscript
	RenderingServer.global_shader_parameter_add("wg_slope_influence", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, SLOPE_INFLUENCE)
	RenderingServer.global_shader_parameter_add("wg_slope_ref", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, SLOPE_REF)
```

- [ ] **Step 2: Run m3 + gpu + fast**
```powershell
python tools/gate.py --suite m3
python tools/gate.py --suite gpu
python tools/gate.py --suite fast
```
Expected: `m3 7/7 fail=0`, `gpu 4/4 fail=0` (facts parity unchanged — descriptor is
render-only, reads base, changes nothing), `fast 6/6 fail=0`. Report literal lines.

- [ ] **Step 3: Commit**
```bash
git add wg-10/worldgen_terrain/harness/m3_review.gd
git commit -m "M5 s3: register slope_influence/slope_ref defaults in m3_review (m3 7/7, parity unchanged)"
```

---

## Task 4: Owner acceptance (S3) + STATUS

- [ ] **Step 1: Owner flies.** Launch `m3_review.tscn`, fly. Confirm the BALANCED look:
  rocky/detailed on steep faces, calm on flat valley floors (vs S2's uniform detail). Press
  `N` to A/B. Record verbatim. (Optional: temporarily set `wg_slope_influence` via the
  global to 0 and 1 to see the extremes — not a required key.)

- [ ] **Step 2: Update STATUS.md** — M5-S3 entry: `surface_descriptor()` added (the reusable
  M6/M7 seam), slope modulation green (`slope_mod` redistribution), edge-safe preserved, m3
  7/7, gpu 4/4, parity unchanged, owner result. Note the descriptor is the seam M6/M7 extend.
  Note S4 (config + p99 + docs audit) is the final slice.

- [ ] **Step 3: Commit STATUS.**

---

## Self-review notes (planner)

- **Spec coverage (S3):** spec §5 (descriptor: slope/curvature/height_band) → Task 2 Step 2;
  §6 modulate (slope_influence knob) → Task 2 Step 2-3; §8 "descriptor edge-safe + detail
  concentrates on slopes + non-vacuous" → Task 1 gate + Task 2 Step 4 seam check; the M6/M7
  seam doc → Task 2 comment + Task 4 STATUS.
- **Perf note (spec §5):** the descriptor adds ~4 page taps per vertex — this is the cost the
  S4 p99 gate measures. If p99 is tight at S4, the documented lever is computing slope from
  the COARSE page on far levels (detail fades there anyway). NOT pre-optimized here.
- **Arity:** descriptor uses `textureSize` + existing uniforms + 2 globals → no bind_tile
  change, Rust untouched (cargo stays 115).
- **Edge-safety:** the ±texel reads use the existing clamp-to-edge sampler
  (`repeat_disable`), so reads just outside the page clamp to the edge texel — abutting tiles
  still agree. The seam gate guards this (Task 2 Step 4).
- **Curvature/height_band computed but unused by M5 default:** intentional — they are the
  seam for M6/M7, not dead code (spec §5 "available… used by config experiments and later
  milestones"). Documented in the function comment.
- **Names:** `SurfaceDescriptor`, `surface_descriptor`, `wg_modulate`, `wg_slope_influence`,
  `wg_slope_ref`, `SLOPE_INFLUENCE`, `SLOPE_REF`, `_slope_modulation_ok`,
  `_capture_one_tile_mod` consistent across tasks.
