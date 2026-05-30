# M5 Slice 1 — fBm + uniform detail (Implementation Plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add bounded, world-space, edge-safe procedural fBm detail to the terrain in
the vertex shader at a flat (un-modulated, un-faded) amplitude — and prove with a gate
that it is bounded, that abutting tiles still bit-agree on shared edges WITH detail on,
and that the base/collision height is untouched.

**Architecture:** Detail is computed entirely in `ring_displace.gdshader`'s `vertex()`
as fBm of a value-noise evaluated from **world XZ** (pure function of world position →
edge-safe by construction), added to `VERTEX.y` *after* the base height `h_base` is
formed (so `h_base` — what facts/collision read — is untouched). Detail amplitude is a
**global shader parameter** (`wg_detail_amp`), like the existing `wg_dbg_mode`, so no
per-tile `bind_tile` arg is needed (that function is already at gdext's 15-arg cap).

**Tech Stack:** Godot 4.6 spatial shader (GDShader, not GLSL compute), the existing
`Wg10PagePool`/`ring_displace.gdshader` render path, a windowed GDScript gate
(`tools/gate.py --suite m3`), no Rust core change.

**Why this slice first:** it retires the single scariest M5 risk (reopening the tile
seams M3 bled over) before any modulation/fade complexity is added. See spec
`docs/superpowers/specs/2026-05-30-m5-detail-masks-design.md` §9.

---

## File structure

- **Modify:** `wg-10/worldgen_terrain/shaders/ring_displace.gdshader`
  — add a global `wg_detail_amp` uniform + a self-contained value-noise/fBm block +
  one line adding detail to `VERTEX.y`. Responsibility: the only place detail is
  computed; base-height formation is unchanged above it.
- **Create:** `wg-10/worldgen_terrain/m5/m5_detail_check.gd`
  — windowed gate (SubViewport render + page readback) asserting bounded + edge-safe +
  base-unchanged. Responsibility: the M5 Slice-1 invariant proof.
- **Modify:** `tools/gate.py`
  — register `m5_detail_check.gd` in the `m3` suite so it runs windowed with the others.
- **Modify (harness toggle, end of slice):** `wg-10/worldgen_terrain/harness/m3_review.gd`
  — register the `wg_detail_amp` global + an `N` key to toggle detail on/off for the
  owner's live A/B fly. Responsibility: owner acceptance ergonomics only.

> **gdext arity note:** `Wg10ClipmapRings::bind_tile` already packs args into `Vector2`
> to stay under gdext's 15-`#[func]`-arg cap. Detail params are scene-global in this
> slice (no per-tile/per-level variation until S2), so they are set as GLOBAL shader
> parameters — NOT new `bind_tile` args. This keeps Rust untouched and is the correct
> design for global config.

---

## Task 1: Add the global detail-amplitude uniform (no detail yet)

Establish the plumbing — a global uniform the shader reads — and prove the existing
gates still pass byte-identically when `wg_detail_amp` is registered but zero. This
isolates "did adding the uniform break anything" from "did detail break anything."

**Files:**
- Modify: `wg-10/worldgen_terrain/shaders/ring_displace.gdshader`

- [ ] **Step 1: Add the global uniform declaration**

In `ring_displace.gdshader`, immediately AFTER the existing
`global uniform float wg_dbg_mode;` line (currently ~line 25), add:

```glsl
// M5 detail: GLOBAL amplitude knob (one RenderingServer.global_shader_parameter for the
// whole scene, set by the harness / gate). 0.0 = no detail (byte-identical to pre-M5).
// Detail is added to VERTEX.y AFTER h_base is formed, so the base/collision height is
// untouched (facts read h_base). See spec §4 invariant 1.
global uniform float wg_detail_amp;
```

- [ ] **Step 2: Verify the shader still compiles and gates pass (detail amp defaults to 0, unused)**

Run (PowerShell, from repo root — sets the Godot bin, then the m3 suite):

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools/gate.py --suite m3
```

Expected: `[gate] suite=m3 checks=6 fail=0 skip=0` (unchanged — the uniform is declared
but not yet read, so rendering is byte-identical).

- [ ] **Step 3: Commit**

```bash
git add wg-10/worldgen_terrain/shaders/ring_displace.gdshader
git commit -m "M5 s1: declare global wg_detail_amp uniform (unused, gates byte-identical)"
```

---

## Task 2: Write the failing edge-safety + bounded gate (`m5_detail_check.gd`)

Write the gate BEFORE the noise exists. It renders two horizontally-abutting tiles
through the real `ring_displace` shader path with detail ON, reads back their displaced
heights, and asserts: (a) the shared edge bit-agrees (seam=0), (b) detail stays within
the bounded ceiling, (c) the same render with detail OFF differs (non-vacuous — detail
actually did something). It will FAIL now because the noise block doesn't exist yet
(detail-on == detail-off → the non-vacuous check fails).

**Files:**
- Create: `wg-10/worldgen_terrain/m5/m5_detail_check.gd`

- [ ] **Step 1: Create the gate script**

Create `wg-10/worldgen_terrain/m5/m5_detail_check.gd` with EXACTLY this content:

```gdscript
extends SceneTree

# M5 Slice 1 gate — fBm uniform detail: bounded + edge-safe-with-detail + base-untouched.
# WINDOWED ONLY (global RenderingDevice is null headless on this D3D12 box).
#
# Three invariants, all observed from the rendered output (GDScript can't run the shader's
# fbm directly, so we prove them at the render boundary, not by mirroring the noise in CPU):
#   (1) BOUNDED     — detail-on does not blow the surface past the height-color range
#                     (saturated-pixel fraction stays low). |detail| <= wg_detail_amp by
#                     construction (fbm normalized to [-1,1]); the capture confirms no blowup.
#   (2) EDGE-SAFE   — two ABUTTING tiles (page at (0,0) and (SPAN,0)), each with its correct
#                     page_origin, agree along the shared world seam (x==SPAN) within a tight
#                     luma epsilon — because detail is a pure function of world XZ. If detail
#                     were page-local, the seam columns would diverge. (The M3 seam contract
#                     must survive M5.)
#   (3) NON-VACUOUS — detail-on differs from detail-off (detail genuinely displaces; the gate
#                     can't pass on a no-op).

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const WORLD_SPAN := 8192.0
const PAGE_PX := 256
const GRID_RES := 128
const HEIGHT_SCALE := 0.35
const RELIEF_REF := 2000.0
const SEED := 1337
const DETAIL_AMP := 60.0           # metres of peak detail for the test (visible, bounded)
const VIEW_SIZE := Vector2i(512, 512)

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10PagePool"):
		push_error("[wg10-m5] Wg10PagePool not registered — run WINDOWED (not headless)")
		return 1
	if RenderingServer.get_rendering_device() == null:
		push_error("[wg10-m5] no RenderingDevice — run WINDOWED")
		return 2

	# register the global detail-amp param so set() below works (harness does the same).
	if not RenderingServer.global_shader_parameter_get_list().has("wg_detail_amp"):
		RenderingServer.global_shader_parameter_add("wg_detail_amp", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, 0.0)
	if not RenderingServer.global_shader_parameter_get_list().has("wg_dbg_mode"):
		RenderingServer.global_shader_parameter_add("wg_dbg_mode", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, 0.0)

	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	var err := str(pool.call("configure", pack_os, PACK_FILE, glsl_os, 4, PAGE_PX, WORLD_SPAN, SEED))
	if err != "":
		push_error("[wg10-m5] pool configure failed: %s" % err)
		return 1
	var tex = pool.call("acquire_page", 0, 0.0, 0.0)
	if tex == null:
		push_error("[wg10-m5] acquire_page failed")
		return 1

	# Capture the displaced surface twice (detail OFF, detail ON) with a top-down ORTHO
	# camera over one page, reading VERTEX.y via a high-res displaced mesh -> the captured
	# luminance encodes height (the shader's height-color path). We compare statistics.
	var off_img := _capture(tex, 0.0)
	var on_img := _capture(tex, DETAIL_AMP)
	if off_img == null or on_img == null:
		push_error("[wg10-m5] capture failed")
		return 1

	# (3) NON-VACUOUS: detail-on must differ from detail-off (detail genuinely displaced).
	var diff := _mean_abs_diff(off_img, on_img)
	var non_vacuous := diff > 0.002    # >0.2% mean luminance change
	# (1) BOUNDED: no pixel saturates to pure white/black beyond the height-color range, i.e.
	# detail did not blow the surface past the relief_ref color clamp into a flat saturated
	# band. Proxy: the fraction of fully-saturated (==1.0 or ==0.0 luma) pixels stays low.
	var sat := _saturated_frac(on_img)
	var bounded := sat < 0.20
	# (2) EDGE-SAFE is asserted in CPU space below (the strong test), not from the capture.
	var edge_safe := _edge_safe(DETAIL_AMP)

	var ok := non_vacuous and bounded and edge_safe
	print("[wg10-m5] non_vacuous=%s (diff=%.4f) bounded=%s (sat=%.3f) edge_safe=%s -> %s" % [
		non_vacuous, diff, bounded, sat, edge_safe, "PASS" if ok else "FAIL"])
	return 0 if ok else 1

# Render one page top-down ortho at the given global detail amp; return the captured Image.
func _capture(tex, amp: float) -> Image:
	RenderingServer.global_shader_parameter_set("wg_detail_amp", amp)
	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	var world := World3D.new()
	vp.world_3d = world
	var envh := Environment.new()
	envh.background_mode = Environment.BG_COLOR
	envh.background_color = Color.BLACK
	var cam := Camera3D.new()
	cam.projection = Camera3D.PROJECTION_ORTHOGONAL
	cam.size = WORLD_SPAN
	cam.position = Vector3(WORLD_SPAN * 0.5, 5000.0, WORLD_SPAN * 0.5)
	cam.rotation_degrees = Vector3(-90.0, 0.0, 0.0)
	cam.far = 20000.0
	cam.environment = envh
	vp.add_child(cam)
	# flat grid mesh covering [0,SPAN]^2, displaced by the shader.
	var mesh := PlaneMesh.new()
	mesh.size = Vector2(WORLD_SPAN, WORLD_SPAN)
	mesh.subdivide_width = GRID_RES
	mesh.subdivide_depth = GRID_RES
	var mi := MeshInstance3D.new()
	mi.mesh = mesh
	mi.position = Vector3(WORLD_SPAN * 0.5, 0.0, WORLD_SPAN * 0.5)
	var mat := ShaderMaterial.new()
	mat.shader = load(SHADER)
	mat.set_shader_parameter("height_tex", tex)
	mat.set_shader_parameter("coarse_height_tex", tex)
	mat.set_shader_parameter("world_span", WORLD_SPAN)
	mat.set_shader_parameter("coarse_span", WORLD_SPAN)
	mat.set_shader_parameter("height_scale", HEIGHT_SCALE)
	mat.set_shader_parameter("morph_region", 0.0)
	mat.set_shader_parameter("relief_ref", RELIEF_REF)
	mat.set_shader_parameter("page_origin", Vector2(0.0, 0.0))
	mat.set_shader_parameter("coarse_origin", Vector2(0.0, 0.0))
	mat.set_shader_parameter("level_center", Vector2(WORLD_SPAN * 0.5, WORLD_SPAN * 0.5))
	mat.set_shader_parameter("level_half_extent", WORLD_SPAN * 1.5)
	mi.material_override = mat
	vp.add_child(mi)
	get_root().add_child(vp)
	await process_frame
	await process_frame
	RenderingServer.force_draw()
	var img := vp.get_texture().get_image()
	vp.queue_free()
	return img

func _mean_abs_diff(a: Image, b: Image) -> float:
	var n := 0
	var s := 0.0
	var y := 0
	while y < a.get_height():
		var x := 0
		while x < a.get_width():
			s += absf(a.get_pixel(x, y).v - b.get_pixel(x, y).v)
			n += 1
			x += 8
		y += 8
	return s / float(maxi(n, 1))

func _saturated_frac(img: Image) -> float:
	var n := 0
	var sat := 0
	var y := 0
	while y < img.get_height():
		var x := 0
		while x < img.get_width():
			var v := img.get_pixel(x, y).v
			if v >= 0.999 or v <= 0.001:
				sat += 1
			n += 1
			x += 8
		y += 8
	return float(sat) / float(maxi(n, 1))

# EDGE-SAFE (the strong test): the fbm is a pure function of WORLD XZ, so two abutting
# tiles evaluate the IDENTICAL value on their shared world edge. We don't have the shader's
# fbm in GDScript, but we can prove the CONTRACT that makes it edge-safe: the shader reads
# detail from world.xz only (no VERTEX-local, no page-local term). We assert that by
# rendering page B placed at origin (SPAN,0) and page A at (0,0) and comparing the displaced
# luminance along the shared seam column — they must match within a tight epsilon.
func _edge_safe(amp: float) -> bool:
	RenderingServer.global_shader_parameter_set("wg_detail_amp", amp)
	# Render a 2-page-wide strip: A at x in [0,SPAN], B at x in [SPAN,2*SPAN], both sampling
	# the SAME page texture but with their correct page_origin, so the only thing aligning
	# the seam is world-XZ-pure detail. Compare the pixel column at the shared seam.
	# (Reuse _capture machinery inline would duplicate; we accept the capture-based proxy
	# here: if detail were page-local, the seam column of A and B would diverge.)
	# For Slice 1 we assert via the strip capture in _capture_strip.
	var seam_max := _capture_strip(amp)
	print("[wg10-m5]   edge seam_max_luma_delta=%.5f" % seam_max)
	return seam_max < 0.01

# Render two abutting pages and return the max luminance delta along the shared seam column.
func _capture_strip(amp: float) -> float:
	RenderingServer.global_shader_parameter_set("wg_detail_amp", amp)
	var pool2: Object = ClassDB.instantiate("Wg10PagePool")
	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	pool2.call("configure", pack_os, PACK_FILE, glsl_os, 4, PAGE_PX, WORLD_SPAN, SEED)
	var ta = pool2.call("acquire_page", 0, 0.0, 0.0)
	var tb = pool2.call("acquire_page", 0, WORLD_SPAN, 0.0)
	if ta == null or tb == null:
		return 999.0
	var vp := SubViewport.new()
	vp.size = Vector2i(1024, 512)
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	vp.world_3d = World3D.new()
	var envh := Environment.new()
	envh.background_mode = Environment.BG_COLOR
	envh.background_color = Color.BLACK
	var cam := Camera3D.new()
	cam.projection = Camera3D.PROJECTION_ORTHOGONAL
	cam.size = WORLD_SPAN * 2.0
	cam.position = Vector3(WORLD_SPAN, 5000.0, WORLD_SPAN * 0.5)
	cam.rotation_degrees = Vector3(-90.0, 0.0, 0.0)
	cam.far = 20000.0
	cam.environment = envh
	vp.add_child(cam)
	_add_strip_tile(vp, ta, 0.0)
	_add_strip_tile(vp, tb, WORLD_SPAN)
	get_root().add_child(vp)
	await process_frame
	await process_frame
	RenderingServer.force_draw()
	var img := vp.get_texture().get_image()
	# The shared seam is world x == SPAN -> screen center column.
	var col := img.get_width() / 2
	var m := 0.0
	var y := 0
	while y < img.get_height():
		var l := img.get_pixel(col - 1, y).v
		var r := img.get_pixel(col + 1, y).v
		m = maxf(m, absf(l - r))
		y += 1
	vp.queue_free()
	return m

func _add_strip_tile(vp: SubViewport, tex, origin_x: float) -> void:
	var mesh := PlaneMesh.new()
	mesh.size = Vector2(WORLD_SPAN, WORLD_SPAN)
	mesh.subdivide_width = GRID_RES
	mesh.subdivide_depth = GRID_RES
	var mi := MeshInstance3D.new()
	mi.mesh = mesh
	mi.position = Vector3(origin_x + WORLD_SPAN * 0.5, 0.0, WORLD_SPAN * 0.5)
	var mat := ShaderMaterial.new()
	mat.shader = load(SHADER)
	mat.set_shader_parameter("height_tex", tex)
	mat.set_shader_parameter("coarse_height_tex", tex)
	mat.set_shader_parameter("world_span", WORLD_SPAN)
	mat.set_shader_parameter("coarse_span", WORLD_SPAN)
	mat.set_shader_parameter("height_scale", HEIGHT_SCALE)
	mat.set_shader_parameter("morph_region", 0.0)
	mat.set_shader_parameter("relief_ref", RELIEF_REF)
	mat.set_shader_parameter("page_origin", Vector2(origin_x, 0.0))
	mat.set_shader_parameter("coarse_origin", Vector2(origin_x, 0.0))
	mat.set_shader_parameter("level_center", Vector2(origin_x + WORLD_SPAN * 0.5, WORLD_SPAN * 0.5))
	mat.set_shader_parameter("level_half_extent", WORLD_SPAN * 1.5)
	mi.material_override = mat
	vp.add_child(mi)
```

- [ ] **Step 2: Run the gate to verify it FAILS (no noise yet → detail-on == detail-off)**

Run:

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
& $env:GODOT_BIN --path wg-10 --import
& $env:GODOT_BIN --path wg-10 --script res://worldgen_terrain/m5/m5_detail_check.gd
```

Expected: prints `non_vacuous=false (diff=0.0000) ... -> FAIL`, exit code 1. (Detail amp
is set but the shader ignores it — there is no noise yet — so detail-on and detail-off
render identically and the non-vacuous check fails.)

- [ ] **Step 3: Commit the failing gate**

```bash
git add wg-10/worldgen_terrain/m5/m5_detail_check.gd
git commit -m "M5 s1: add m5_detail_check gate (RED — no noise yet, non_vacuous fails)"
```

---

## Task 3: Implement the fBm detail in the shader (make the gate pass)

Add the value-noise + fBm block and wire it into `VERTEX.y` after `h_base`. This makes
detail-on differ from detail-off (non-vacuous), stays bounded (the fBm sum has a closed
ceiling and we scale by a fixed amp), and is edge-safe (world-XZ-pure → seam matches).

**Files:**
- Modify: `wg-10/worldgen_terrain/shaders/ring_displace.gdshader`

- [ ] **Step 1: Add the value-noise + fBm functions**

In `ring_displace.gdshader`, AFTER the `global uniform float wg_detail_amp;` line and
BEFORE `void vertex()`, add this self-contained block:

```glsl
// ---- M5 detail: value-noise fBm, pure function of world XZ (edge-safe by construction) ----
// Integer hash (no sin-banding): 2D -> [0,1). Cheap, no texture, no streaming.
float wg_hash2(vec2 p) {
	// fract-of-large-product hash with two mixing steps; stable + bandless enough for detail.
	vec3 p3 = fract(vec3(p.xyx) * 0.1031);
	p3 += dot(p3, p3.yzx + 33.33);
	return fract((p3.x + p3.y) * p3.z);
}

// value noise: bilinear blend of 4 lattice hashes with a smootherstep weight.
float wg_value_noise(vec2 x) {
	vec2 i = floor(x);
	vec2 f = fract(x);
	float a = wg_hash2(i);
	float b = wg_hash2(i + vec2(1.0, 0.0));
	float c = wg_hash2(i + vec2(0.0, 1.0));
	float d = wg_hash2(i + vec2(1.0, 1.0));
	vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);  // in [0,1]
}

// fBm: DETAIL_OCTAVES of value noise, centered to [-1,1]*ceiling. Fixed octaves/gain for
// Slice 1 (they become config in Slice 4). Returns a value whose |.| <= 1.0 (the sum of a
// geometric series with gain 0.5 over 5 octaves, normalized) so wg_detail_amp is the metre
// ceiling directly.
const int   WG_DETAIL_OCTAVES = 5;
const float WG_DETAIL_GAIN    = 0.5;
const float WG_DETAIL_FREQ    = 0.0009;   // ~1/1100 m base wavelength; tuned to GRID_RES in S4
float wg_fbm_detail(vec2 world_xz) {
	float sum = 0.0;
	float amp = 1.0;
	float norm = 0.0;
	float freq = WG_DETAIL_FREQ;
	for (int i = 0; i < WG_DETAIL_OCTAVES; i++) {
		sum  += amp * (wg_value_noise(world_xz * freq) * 2.0 - 1.0); // center to [-1,1]
		norm += amp;
		freq *= 2.0;
		amp  *= WG_DETAIL_GAIN;
	}
	return sum / max(norm, 1e-6);   // normalized to [-1,1]
}
```

- [ ] **Step 2: Add detail to VERTEX.y after h_base**

In `ring_displace.gdshader`'s `vertex()`, find the existing lines (~53-56):

```glsl
	float h = mix(h_fine, h_coarse, t);
	v_height = h;
	v_morph = t;
	VERTEX.y = h * height_scale;
```

Replace them with:

```glsl
	float h = mix(h_fine, h_coarse, t);   // h_base: THE facts/collision height (untouched)
	v_height = h;
	v_morph = t;
	// M5 detail: bounded world-space fBm added ON TOP of the base height. Pure function of
	// world.xz -> abutting tiles agree on shared edges (edge-safe). h (above) is unchanged,
	// so facts/collision parity is preserved. Slice 1 = flat amp (no modulation/fade yet).
	float detail = wg_fbm_detail(world.xz) * wg_detail_amp;
	VERTEX.y = (h + detail) * height_scale;
```

- [ ] **Step 3: Run the gate to verify it PASSES**

Run:

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
& $env:GODOT_BIN --path wg-10 --import
& $env:GODOT_BIN --path wg-10 --script res://worldgen_terrain/m5/m5_detail_check.gd
```

Expected: prints `non_vacuous=true (diff=...) bounded=true (sat=...) edge_safe=true ... -> PASS`,
exit code 0. (Detail now displaces the surface, stays within the color range, and the
seam column matches because detail is world-XZ-pure.)

- [ ] **Step 4: Commit**

```bash
git add wg-10/worldgen_terrain/shaders/ring_displace.gdshader
git commit -m "M5 s1: fBm uniform detail in vertex() — bounded, edge-safe, base untouched (gate GREEN)"
```

---

## Task 4: Register the gate in the m3 suite + prove base/collision parity unchanged

Wire the new gate into `tools/gate.py` so it runs with the suite, and confirm the
existing parity gates still pass (proving detail did NOT touch the base height — the
facts contract). `wg_detail_amp` defaults to 0 in the parity gates' render path, but the
key proof is that `facts_collision_parity_check` (which reads `h_base` via `get_height`,
not the shader) is mathematically independent of the shader detail — we assert it still
passes to lock the contract in CI.

**Files:**
- Modify: `tools/gate.py`

- [ ] **Step 1: Find the m3 suite check list in gate.py**

Run:

```bash
grep -n "m3_" tools/gate.py
```

Expected: a list/array of m3 check script names (e.g. `m3_slice1_check.gd`,
`m3_pool_check.gd`, ...). Note the exact structure (list of filenames in the `m3` suite).

- [ ] **Step 2: Add `m5_detail_check.gd` to the m3 suite**

In `tools/gate.py`, in the `m3` suite's list of check scripts, add the new gate path
alongside the existing ones. The check lives at
`worldgen_terrain/m5/m5_detail_check.gd` (res-relative, matching how the others are
referenced). Add it as the last entry of the m3 list, mirroring the exact path style
already used (if others are listed as `"m3/m3_slice1_check.gd"`, use
`"m5/m5_detail_check.gd"`; if they use the full `res://worldgen_terrain/...` form, match
that).

- [ ] **Step 3: Run the full m3 suite — all green including the new gate**

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools/gate.py --suite m3
```

Expected: `[gate] suite=m3 checks=7 fail=0 skip=0` (was 6, now 7 with `m5_detail_check`).

- [ ] **Step 4: Run the gpu suite — base/collision parity unchanged (the facts contract)**

```powershell
python tools/gate.py --suite gpu
```

Expected: `[gate] suite=gpu checks=4 fail=0 skip=0` — in particular
`facts_collision_parity_check` still passes (maxd ~0.0009 m). This proves the M5 detail
did NOT alter the base height that facts/collision read (spec §4 invariant 1, §8 check 3).

- [ ] **Step 5: Run the fast suite + cargo — nothing else regressed**

```powershell
python tools/gate.py --suite fast
```

Expected: `[gate] suite=fast checks=6 fail=0`.

```powershell
$env:CARGO_TARGET_DIR=$null
cd wg-10/rust; cargo test --quiet; cd ../..
```

Expected: `115 passed` (Rust core untouched this slice).

- [ ] **Step 6: Commit**

```bash
git add tools/gate.py
git commit -m "M5 s1: register m5_detail_check in m3 suite (m3 7/7; gpu/fast/cargo unchanged)"
```

---

## Task 5: Add the owner A/B detail toggle to m3_review (acceptance ergonomics)

Give the owner an `N` key in the fly scene to toggle detail on/off live, so the
acceptance fly can A/B "blobby vs has-shape." Detail amp defaults to a sensible visible
value; `N` flips it to 0 and back.

**Files:**
- Modify: `wg-10/worldgen_terrain/harness/m3_review.gd`

- [ ] **Step 1: Register the global detail-amp param + a default constant**

In `m3_review.gd`, add a constant near the other tunables (~line 33, after
`const RELIEF_REF := 2000.0`):

```gdscript
const DETAIL_AMP := 60.0     # M5 detail peak (metres); press N to toggle on/off live
```

And add a state var near the other `var _*` declarations (~line 43):

```gdscript
var _detail_on := true
```

In `_ready()`, AFTER the existing
`RenderingServer.global_shader_parameter_add("wg_dbg_mode", ...)` line (~line 103), add:

```gdscript
	# M5: global detail amplitude (read by ring_displace's wg_detail_amp). N toggles it.
	RenderingServer.global_shader_parameter_add("wg_detail_amp", RenderingServer.GLOBAL_VAR_TYPE_FLOAT, DETAIL_AMP)
```

- [ ] **Step 2: Add the N-key toggle in _input**

In `m3_review.gd`'s `_input()`, after the existing `KEY_M` branch (~line 122), add:

```gdscript
	elif event is InputEventKey and event.pressed and event.keycode == KEY_N:
		_detail_on = not _detail_on
		RenderingServer.global_shader_parameter_set("wg_detail_amp", DETAIL_AMP if _detail_on else 0.0)
```

- [ ] **Step 3: Sanity-check the scene loads (editor hot-reload, no rebuild)**

Run (windowed, briefly — confirms the script parses + the global param registers; close
the window after it opens):

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
& $env:GODOT_BIN --path wg-10 res://worldgen_terrain/harness/m3_review.tscn
```

Expected: the fly scene opens with terrain; no script parse errors in the console. (This
is a GDScript change — it hot-reloads, no Rust rebuild needed.)

- [ ] **Step 4: Commit**

```bash
git add wg-10/worldgen_terrain/harness/m3_review.gd
git commit -m "M5 s1: N-key detail on/off toggle in m3_review for owner A/B acceptance fly"
```

---

## Task 6: Owner acceptance + STATUS update (the real sign-off)

Gate-green is necessary, not sufficient (DESIGN §7.3). The owner flies and judges the
look; then STATUS records the honest result.

- [ ] **Step 1: Owner flies the A/B**

Ask the owner to launch `m3_review.tscn` (windowed), fly with WASD/Shift/mouse, and
press `N` to toggle detail. Confirm: (a) detail-on visibly adds high-frequency shape
("less blobby"); (b) no new seams/cracks at tile boundaries; (c) no obvious shimmer/crawl
under motion at speed; (d) the HUD p99 stays well under 6 ms with detail on. Record what
the owner reports verbatim.

- [ ] **Step 2: Update STATUS.md with the Slice-1 result**

Add an M5-Slice-1 entry at the top of `docs/plans/STATUS.md` stating: fBm uniform detail
landed in `ring_displace.gdshader`; `m5_detail_check` green (bounded + edge-safe-with-
detail + non-vacuous); base/collision parity unchanged (gpu 4/4, facts maxd 0.0009 m);
m3 7/7; fast 6/6; cargo 115; the owner A/B result (verbatim). Note explicitly that the
gate proves invariants, NOT "looks good" — the look judgment is the owner's. Note S2 (LOD
fade) is next.

- [ ] **Step 3: Commit STATUS**

```bash
git add docs/plans/STATUS.md
git commit -m "M5 s1: STATUS — fBm uniform detail landed, gates green, owner A/B recorded"
```

---

## Self-review notes (done by the planner)

- **Spec coverage (Slice 1 scope only):** spec §9 S1 = "fBm + uniform detail; gate
  bounded + edge-safe-with-detail + base parity." Tasks 2–4 cover all three. §3 "fBm in
  GDShader, world-space, base untouched" → Task 3. §7 config is S4 (not this slice) — S1
  uses fixed octaves/gain consts + a global amp, explicitly deferred to S4. §8 owner A/B
  → Tasks 5–6.
- **Edge-safety test honesty:** the gate proves edge-safety via the abutting-strip seam
  capture (`_capture_strip`) — a render-level proxy, since GDScript can't run the shader's
  fBm directly. If the owner/reviewer wants a stronger CPU-exact seam assertion, S4 can
  add a readback-of-displaced-vertices test; for S1 the strip-seam delta < 0.01 luma plus
  the world-XZ-pure construction is the proof. This limitation is stated in the gate's
  header comment.
- **Arity:** detail amp is a GLOBAL shader param (not a `bind_tile` arg), so Rust is
  untouched — confirmed against the 15-arg cap note in `clipmap_rings.rs`.
- **Type/name consistency:** `wg_detail_amp` (global uniform), `wg_fbm_detail`,
  `wg_value_noise`, `wg_hash2`, `DETAIL_AMP` (gate + harness constant) used identically
  across Tasks 1, 3, 5. `m5_detail_check.gd` path identical in Tasks 2 and 4.
- **Check counts** assume current m3=6, gpu=4, fast=6, cargo=115 (per HANDOFF). If the
  live counts differ, the implementer uses the actual `--import`-then-suite output and
  adjusts the expected numbers (the assertion is "fail=0 and the new gate is counted,"
  not a hardcoded total).
