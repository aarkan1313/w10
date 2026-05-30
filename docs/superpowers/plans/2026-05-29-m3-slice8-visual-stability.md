# M3 Slice 8 — Visual Stability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the inter-tile seam and the per-tile geomorph "switching" the owner's fly found, and lock both with a windowed visual-continuity gate.

**Architecture:** Three coordinated sampling fixes — (1) `height_page.glsl` switches to a texel-corner pixel→world mapping so abutting pages share boundary samples; (2) `ring_displace.gdshader` samples the fine page by true world UV (new `page_origin` uniform) and computes the geomorph from distance to the level *neighborhood* center (new `level_center` + `level_half_extent` uniforms); (3) the Rust view/rings pass the new uniforms. A new `m3_continuity_check.gd` proves shared-edge page data matches to the bit and morph banding is gone.

**Tech Stack:** Godot 4.6 (.NET wg10, D3D12, Forward+), Rust GDExtension (gdext 0.5.3), GLSL compute + spatial shader, Python gate runner. Windowed-only for RenderingDevice.

**Pre-flight constraints (do not violate):**
- Cannot `git push` — the USER pushes `origin main`. Commit on `main`.
- `worldgen9/` is READ-ONLY (knowledge only).
- Commit trailer: `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`
- Build env (memory `worldgen10-build-gotchas`): unset `CARGO_TARGET_DIR` (`env -u CARGO_TARGET_DIR`), set `GODOT_BIN`. GPU/m3 suites are WINDOWED-only.
- `height_at()` in `height_page.glsl` must stay bit-synced with `height_field.glsl` — this slice changes ONLY `main()`'s pixel→world mapping, never `height_at`.

---

### Task 1: Texel-corner convention in the page generation shader

**Files:**
- Modify: `wg-10/worldgen_terrain/shaders/height_page.glsl:178-186` (the `main()` body)

- [ ] **Step 1: Change the pixel→world mapping to texel-corner**

In `main()`, replace the texel-center mapping with texel-corner so texel 0 lands on the page
origin and texel N-1 on `origin+span` (abutting pages then share boundary samples). Guard the
`page_px == 1` degenerate (avoid divide-by-zero; never used, but no UB).

```glsl
void main() {
	ivec2 px = ivec2(gl_GlobalInvocationID.xy);
	if (px.x >= P.page_px || px.y >= P.page_px) return;
	// Texel-CORNER convention (slice 8): texel 0 -> origin, texel N-1 -> origin+span, so
	// abutting pages (exactly `span` apart) SHARE their boundary row/column samples and the
	// ring shader's world-UV fine sample is bit-identical across a tile seam. (Was texel-center
	// (px+0.5)/page_px, which left abutting pages' boundary samples one texel apart -> a seam.)
	float denom = float(max(P.page_px - 1, 1));
	float u = float(px.x) / denom;
	float v = float(px.y) / denom;
	float wx = P.origin_x + u * P.world_span;
	float wz = P.origin_z + v * P.world_span;
	imageStore(height_img, px, vec4(height_at(wx, wz), 0.0, 0.0, 1.0));
}
```

- [ ] **Step 2: Verify the parity gates are untouched (read-only check, no code change)**

Confirm by inspection that `gpu_parity_check.gd` and `gpu_parity_dem_check.gd` call
`gpu.heights(xs,zs)` (which uses `height_field.glsl`), NOT `height_page.glsl`'s mapping. They do
— so this change cannot affect them. (No edit; this is a guard against breaking M2.)

- [ ] **Step 3: Commit**

```bash
git add wg-10/worldgen_terrain/shaders/height_page.glsl
git commit -m "fix(wg10/m3): texel-corner page sampling so abutting pages share boundary samples

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Ring shader — world-UV fine sampling + neighborhood-center geomorph

**Files:**
- Modify: `wg-10/worldgen_terrain/shaders/ring_displace.gdshader` (uniforms + `vertex()`)

- [ ] **Step 1: Add the three new uniforms and rewrite `vertex()`**

Replace the uniform block additions and the `vertex()` body. `page_origin` is the fine page's
world lower-XZ corner; `level_center` is the 3×3 neighborhood's world center; `level_half_extent`
is `1.5*span_l`. Sample the fine page by world UV (matching texel-corner gen), and compute the
morph from Chebyshev distance to the neighborhood center.

```glsl
shader_type spatial;
render_mode unshaded, cull_disabled;

uniform sampler2D height_tex;            // this level's R32F page (pool Texture2DRD)
uniform sampler2D coarse_height_tex;     // next-coarser level's page (for the morph)
uniform float world_span = 8192.0;       // this level's page world span
uniform float coarse_span = 8192.0;      // the coarser level's page world span
uniform vec2 page_origin = vec2(0.0);    // this fine page's WORLD lower-XZ corner; page covers [origin, origin+span]
uniform vec2 coarse_origin = vec2(0.0);  // the coarser page's WORLD lower-XZ corner
uniform vec2 level_center = vec2(0.0);   // world center of this level's 3x3 neighborhood
uniform float level_half_extent = 12288.0; // 1.5 * world_span (half the 3x3 neighborhood width)
uniform float height_scale = 1.0;        // visual amplitude (config; 1.0 = raw metres)
uniform float morph_region = 0.0;        // transition width as a fraction of the half-extent (0 = no morph)
uniform float relief_ref = 2000.0;       // color gradient normalization

varying float v_height;

void vertex() {
	vec3 world = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;

	// Fine sample by TRUE WORLD UV (matches the page's texel-corner generation): uv=0 -> texel 0
	// (= page_origin), uv=1 -> texel N-1 (= page_origin+span). At a tile seam, both abutting
	// tiles map their shared-edge vertex onto the SAME shared boundary texel -> no seam.
	vec2 uv_fine = (world.xz - page_origin) / world_span;
	float h_fine = texture(height_tex, uv_fine).r;

	// Geomorph: blend to the coarser level ONLY at this LEVEL's true outer ring (the outward
	// edge of the 3x3 neighborhood), measured from the NEIGHBORHOOD CENTER — not the tile center.
	// Tile-local distance fired the morph at every one of the 9 tiles' edges (an interior lattice
	// that swept under motion). cheb in [0,1], 1 at the neighborhood outer edge.
	float cheb = max(abs(world.x - level_center.x), abs(world.z - level_center.z)) / level_half_extent;
	float region = max(morph_region, 1e-6);
	float t = clamp((cheb - (1.0 - region)) / region, 0.0, 1.0);

	// Coarser page sampled corner-relative in world (same texel-corner convention).
	vec2 uv_coarse = (world.xz - coarse_origin) / coarse_span;
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

- [ ] **Step 2: Commit**

```bash
git add wg-10/worldgen_terrain/shaders/ring_displace.gdshader
git commit -m "fix(wg10/m3): ring shader world-UV fine sample + neighborhood-center geomorph

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Rings — set the three new uniforms in `bind_tile`

**Files:**
- Modify: `wg-10/rust/src/clipmap_rings.rs` — `bind_tile` signature + body (around lines 136-186)

- [ ] **Step 1: Extend `bind_tile` to accept + set `page_origin`, `level_center`, `level_half_extent`**

Add three parameters and set the new shader uniforms. `page_origin` reuses the existing
`page_origin_x/page_origin_z`; add `level_center_x/level_center_z` and `level_half_extent`. Keep
the transform placement (`page_origin + span_l*0.5`) unchanged.

In the signature (after `coarse_origin_z: f64,`), add:
```rust
        level_center_x: f64,
        level_center_z: f64,
        level_half_extent: f64,
```

In the body, after the existing `coarse_origin` set (line ~184), add the new uniforms and a
`page_origin` set (the fine page's world corner = `page_origin_x/z`):
```rust
        let po = Vector2::new(page_origin_x as f32, page_origin_z as f32);
        mat.set_shader_parameter("page_origin", &po.to_variant());
        let lc = Vector2::new(level_center_x as f32, level_center_z as f32);
        mat.set_shader_parameter("level_center", &lc.to_variant());
        mat.set_shader_parameter("level_half_extent", &level_half_extent.to_variant());
```

- [ ] **Step 2: Build the Rust crate**

```bash
cd wg-10/rust
env -u CARGO_TARGET_DIR cargo build 2>&1 | tail -20
```
Expected: compiles clean (warnings ok). If `bind_tile` callers don't yet pass the new args, the
view (Task 4) is updated next — but build Task 3+4 together if the compiler requires all callers
fixed first (it will: the `#[func]` signature change breaks the GDScript call only at runtime,
but the Rust caller in Task 4 must match). NOTE: `bind_tile` is `#[func]` and called from Rust
(`terrain_view.rs`), so Task 3 and Task 4 must compile together. Do Task 3 edits, then Task 4
edits, THEN build.

- [ ] **Step 3: (Deferred build — see Task 4 Step 3.) Commit after Task 4 compiles.**

---

### Task 4: View — compute + pass the neighborhood center and half-extent

**Files:**
- Modify: `wg-10/rust/src/terrain_view.rs` — `update()` loop (lines 78-136)

- [ ] **Step 1: Compute `level_center` and `level_half_extent` per level; pass to `bind_tile`**

Inside `for level in 0..num`, after `span_l` is computed (line 80), add the neighborhood center
(the middle tile's world center = `center + span_l*0.5`) and half-extent (`1.5*span_l`):
```rust
            let level_center_x = center_x + span_l * 0.5;
            let level_center_z = center_z + span_l * 0.5;
            let level_half_extent = 1.5 * span_l;
```
Then extend the `rings.bind_mut().bind_tile(...)` call (lines 117-132) to pass the three new
trailing args after `co_z`:
```rust
                            co_x,
                            co_z,
                            level_center_x,
                            level_center_z,
                            level_half_extent,
                        );
```

- [ ] **Step 2: Build the Rust crate (Task 3 + Task 4 together)**

```bash
cd wg-10/rust
env -u CARGO_TARGET_DIR cargo build 2>&1 | tail -20
```
Expected: clean compile — `bind_tile`'s new params and the view's new call args match.

- [ ] **Step 3: Commit Tasks 3 + 4 together**

```bash
git add wg-10/rust/src/clipmap_rings.rs wg-10/rust/src/terrain_view.rs
git commit -m "feat(wg10/m3): view passes neighborhood center + half-extent + page_origin to ring tiles

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: The windowed visual-continuity gate

**Files:**
- Create: `wg-10/worldgen_terrain/tests/m3_continuity_check.gd`
- Modify: `wg-10/tools/gate.py` (add to `m3` suite list)

- [ ] **Step 1: Write the continuity gate**

Hard assertion (1): read back abutting same-level fine pages and assert their shared edge
column/row matches to EPS — the precise proof that texel-corner makes the seam zero. Soft
ceiling + PNG (2): morph banding under motion. Build pool/streamer/rings/view exactly like
`m3_accept_check.gd`. Read pages back with `RenderingDevice.texture_get_data` (gate-only readback
— never on the render path). Pages are R32F: 4 bytes/pixel, `page_px*page_px` pixels.

```gdscript
extends SceneTree

# M3 visual-continuity gate (slice 8). Proves the two sampling fixes the owner's fly demanded:
#   (1) HARD: abutting same-level fine pages share their boundary samples to EPS (texel-corner
#       convention) -> no inter-tile seam. A data check on the page textures (readback is a
#       GATE-only operation, never on the render path).
#   (2) SOFT: under motion the rendered interior has no high-frequency morph banding (the
#       per-tile morph lattice produced many large frame-to-frame jumps; correct morph ~none).
# WINDOWED (RenderingDevice). Saves a PNG artifact.

const PACK_RES_DIR := "res://worldgen_terrain/packs/dem_v1"
const PACK_FILE := "terrain_pack.gate.json"
const GLSL := "res://worldgen_terrain/shaders/height_page.glsl"
const SHADER := "res://worldgen_terrain/shaders/ring_displace.gdshader"
const PAGE_PX := 256
const SEED := 1337
const NUM_LEVELS := 3
const BASE_SPAN := 8192.0
const GRID_RES := 64
const RADIUS_PAGES := 1
const LEAD_FRAMES := 8.0
const MAX_PER_FRAME := 4
const CAPACITY := 48
const SEAM_EPS := 1.0e-2          # metres; same scale as the parity gates
const VIEW_SIZE := Vector2i(960, 540)

func _init() -> void:
	quit(await _run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10TerrainView"):
		push_error("Wg10TerrainView not registered"); return 1
	var rd := RenderingServer.get_rendering_device()
	if rd == null:
		print("[wg10-m3-continuity] status=skip reason=no-render-device"); return 2

	var pack_os := ProjectSettings.globalize_path(PACK_RES_DIR)
	var glsl_os := ProjectSettings.globalize_path(GLSL)
	var pool: Object = ClassDB.instantiate("Wg10PagePool")
	var err: String = str(pool.call("configure", pack_os, PACK_FILE, glsl_os, CAPACITY, PAGE_PX, BASE_SPAN, SEED))
	if err != "":
		push_error("pool configure failed: %s" % err); return 1
	var streamer: Object = ClassDB.instantiate("Wg10Streamer")
	streamer.call("configure", pool, NUM_LEVELS, BASE_SPAN, RADIUS_PAGES, LEAD_FRAMES, MAX_PER_FRAME)
	var rings: Object = ClassDB.instantiate("Wg10ClipmapRings")
	rings.call("configure", NUM_LEVELS, BASE_SPAN, GRID_RES, SHADER)
	var view: Object = ClassDB.instantiate("Wg10TerrainView")
	view.call("configure", pool, streamer, rings, NUM_LEVELS, BASE_SPAN, 0.35, 0.15, 2000.0)

	var errs: Array[String] = []

	# --- Settle the streamer at a fixed position so a 3x3 of level-0 pages is resident. ---
	var px := 40000.0
	var pz := -25000.0
	for f in range(40):
		view.call("update", px, pz, 0.0, 0.0)
		await process_frame

	# (1) HARD seam check: for level 0, take the center page and its +X and +Z neighbors;
	# assert the shared edge samples match to EPS. Page origin convention: origin = floor(cam/span)*span.
	var span0 := BASE_SPAN
	var ox := floor(px / span0) * span0
	var oz := floor(pz / span0) * span0
	var center_data := _read_page(rd, pool, 0, ox, oz)
	var east_data := _read_page(rd, pool, 0, ox + span0, oz)
	var north_data := _read_page(rd, pool, 0, ox, oz + span0)
	if center_data.is_empty():
		errs.append("seam: center page not resident (cannot test)")
	else:
		if not east_data.is_empty():
			# center's last column (x = N-1) vs east's first column (x = 0), all rows.
			var d := _max_col_diff(center_data, PAGE_PX - 1, east_data, 0, PAGE_PX)
			if d > SEAM_EPS:
				errs.append("seam EAST: max shared-column diff %.5f m > %.4f" % [d, SEAM_EPS])
		else:
			errs.append("seam: east page not resident")
		if not north_data.is_empty():
			# center's last row (z = N-1) vs north's first row (z = 0), all cols.
			var d2 := _max_row_diff(center_data, PAGE_PX - 1, north_data, 0, PAGE_PX)
			if d2 > SEAM_EPS:
				errs.append("seam NORTH: max shared-row diff %.5f m > %.4f" % [d2, SEAM_EPS])
		else:
			errs.append("seam: north page not resident")

	# (2) SOFT morph-banding under motion + PNG. Render a flight POV, sweep speed, sample a
	# luminance scanline; count large frame-to-frame interior jumps. Correct morph -> low count.
	var vp := SubViewport.new()
	vp.size = VIEW_SIZE
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.own_world_3d = true
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.45, 0.62, 0.85)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	var cam := Camera3D.new()
	cam.far = BASE_SPAN * 32.0
	cam.environment = env
	vp.add_child(rings); vp.add_child(light); vp.add_child(cam)
	get_root().add_child(vp)

	var pos := Vector2(px, pz)
	var heading := Vector2(0.7, 0.7)
	var dt := 1.0 / 60.0
	var speed := 800.0
	var prev_line: PackedFloat32Array = PackedFloat32Array()
	var big_jumps := 0
	var samples := 0
	for f in range(120):
		var vx := heading.x * speed
		var vz := heading.y * speed
		pos += Vector2(vx, vz) * dt
		view.call("update", pos.x, pos.y, vx, vz)
		var eye := Vector3(pos.x - heading.x * 600.0, 900.0, pos.y - heading.y * 600.0)
		var look := Vector3(pos.x + heading.x * 1200.0, 0.0, pos.y + heading.y * 1200.0)
		cam.look_at_from_position(eye, look, Vector3.UP)
		await process_frame
		RenderingServer.force_draw()
		await process_frame
		if f >= 40 and f % 8 == 0:
			var img: Image = vp.get_texture().get_image()
			if img != null:
				if f == 40:
					img.save_png("user://m3_continuity.png")
				var y := int(img.get_height() * 0.7)   # interior terrain scanline
				var line := PackedFloat32Array()
				for x in range(0, img.get_width(), 4):
					var c := img.get_pixel(x, y)
					line.append(c.r * 0.3 + c.g * 0.6 + c.b * 0.1)
				if prev_line.size() == line.size():
					for i in range(line.size()):
						samples += 1
						if absf(line[i] - prev_line[i]) > 0.35:   # large luminance jump = banding sweep
							big_jumps += 1
				prev_line = line

	var jump_frac := float(big_jumps) / float(max(samples, 1))
	if jump_frac > 0.05:
		errs.append("morph banding: %.3f of interior samples jumped frame-to-frame (>0.05)" % jump_frac)

	pool.call("free_all")

	print("[wg10-m3-continuity] seam_eps=%.4f morph_jump_frac=%.3f" % [SEAM_EPS, jump_frac])
	if not errs.is_empty():
		for e in errs: push_error(e)
		print("[wg10-m3-continuity] status=fail errors=%d" % errs.size())
		return 1
	print("[wg10-m3-continuity] status=pass")
	return 0

# Read an R32F page back as a PackedFloat32Array (row-major, page_px*page_px). Empty if not
# resident or the RID is unavailable. Readback is a GATE-only op (never on the render path).
func _read_page(rd: RenderingDevice, pool: Object, level: int, ox: float, oz: float) -> PackedFloat32Array:
	var tex = pool.call("get_resident_page", level, ox, oz)
	if tex == null:
		return PackedFloat32Array()
	var rid: RID = tex.call("get_texture_rd_rid")
	if not rid.is_valid():
		return PackedFloat32Array()
	var bytes: PackedByteArray = rd.texture_get_data(rid, 0)
	if bytes.size() < PAGE_PX * PAGE_PX * 4:
		return PackedFloat32Array()
	return bytes.to_float32_array()

func _max_col_diff(a: PackedFloat32Array, ax: int, b: PackedFloat32Array, bx: int, n: int) -> float:
	var m := 0.0
	for row in range(n):
		var va := a[row * PAGE_PX + ax]
		var vb := b[row * PAGE_PX + bx]
		m = maxf(m, absf(va - vb))
	return m

func _max_row_diff(a: PackedFloat32Array, az: int, b: PackedFloat32Array, bz: int, n: int) -> float:
	var m := 0.0
	for col in range(n):
		var va := a[az * PAGE_PX + col]
		var vb := b[bz * PAGE_PX + col]
		m = maxf(m, absf(va - vb))
	return m
```

- [ ] **Step 2: Add the gate to the `m3` suite**

In `wg-10/tools/gate.py`, append to the `"m3"` list (after `m3_accept_check.gd`):
```python
        "worldgen_terrain/tests/m3_continuity_check.gd",
```

- [ ] **Step 3: Run the m3 suite (windowed)**

```bash
cd /d/workflows/worldgen10
GODOT_BIN="<path to godot 4.6 console exe>" python tools/gate.py --suite m3 2>&1 | tail -40
```
Expected: all m3 checks `status=pass` — including `m3_continuity` (seam diff < EPS, morph jump
frac < 0.05) AND `m3_accept` still green (p99 < 6 ms). If `m3_continuity` reports "page not
resident", adjust the settle position/frames so a full 3×3 of level-0 pages is resident before
the seam readback (the streamer needs enough frames + a still camera).

- [ ] **Step 4: Commit**

```bash
git add wg-10/worldgen_terrain/tests/m3_continuity_check.gd wg-10/tools/gate.py
git commit -m "test(wg10/m3): windowed visual-continuity gate (shared-edge seam + morph banding)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Docs + memory + re-hand the fly

**Files:**
- Modify: `docs/plans/STATUS.md`, `docs/plans/HANDOFF.md`, `docs/plans/ROADMAP.md` (M3 slice 8 line)
- Modify: memory `worldgen10-m3-perf-async.md` or a new continuity memory + `MEMORY.md`

- [ ] **Step 1: Update the three living docs**

STATUS: M3 slice 8 done — seam + morph fixed, continuity gate green, p99 still green; M3 awaits
the owner's re-fly. HANDOFF: how to run `--suite m3`, what the continuity gate asserts, the
texel-corner convention (and that parity gates are unaffected). ROADMAP: tick slice 8.

- [ ] **Step 2: Update memory**

Record: the three defects + the texel-corner fix + the lesson (timing/non-black gates missed
perceptual continuity; the manual fly caught it; the new gate is a hard *data* assertion on
shared-edge page samples). Update `MEMORY.md` index line.

- [ ] **Step 3: Commit docs + memory**

```bash
git add docs/plans/STATUS.md docs/plans/HANDOFF.md docs/plans/ROADMAP.md
git commit -m "docs(wg10/m3): slice 8 visual stability done; M3 awaits owner re-fly

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 4: Re-hand the fly**

Tell the owner: launch `wg-10/worldgen_terrain/harness/m3_review.tscn` (windowed), fly with
WASD+Shift+mouse+Space/C, and confirm: no inter-tile vertical seam, no interior morph
lattice/switching, smooth LOD blend only at level boundaries. On their sign-off, M3 closes
(DESIGN §7.3) and we move to M4 (Facts API: get_height / Jolt collision).

---

## Self-review notes
- **Spec coverage:** Defect 1 (Task 2 morph) ✓, Defect 2 (Task 2 world-UV) ✓, Defect 3 (Task 1
  texel-corner) ✓, gate (Task 5) ✓, parity-safety (Task 1 Step 2) ✓, p99-still-green (Task 5
  Step 3) ✓.
- **Type consistency:** `bind_tile` gains exactly 3 trailing f64 params; the view passes exactly
  3 matching args; shader uniform names `page_origin`/`level_center`/`level_half_extent` are set
  in `clipmap_rings.rs` with those exact strings.
- **Build coupling:** Task 3 + Task 4 must compile together (the `#[func]` Rust caller changes);
  the plan builds them in one step and commits together.
- **No placeholders** except the `GODOT_BIN` path and the docs prose (intentional — env-specific
  / narrative).
