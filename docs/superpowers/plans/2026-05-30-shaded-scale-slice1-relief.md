# Shaded-Scale Slice 1 — relief_scale knob (Implementation Plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add one authoritative `relief_scale` config knob that multiplies the base height field, applied identically on BOTH the render path (shader) AND the facts/collision path (Rust), so visible terrain relief shrinks to a sane (WG9-class) range WITHOUT breaking visible==collision parity and WITHOUT touching the raw `height()` parity formula.

**Architecture:** `relief_scale` is applied AFTER `height::height()` as a uniform multiplier. In Rust `Wg10Facts`, a single helper computes `scaled_base = height::height(...) * relief_scale` used at all 3 consume points (get_height, get_collision_field, bake_collision_region). In the render shader `ring_displace.gdshader`, the sampled page height is multiplied by a `relief_scale` uniform. Both use the SAME value → visible==collision holds. The existing render `height_scale` (0.35) is folded into `relief_scale` (one authoritative knob, no two fighting scales).

**Tech Stack:** Rust GDExtension (`Wg10Facts` in `facts_api.rs`), Godot spatial shader (`ring_displace.gdshader`), GDScript gates (`facts_check`, `facts_collision_parity_check`). The raw `height::height` formula and its M2 GPU parity gate are UNCHANGED.

---

## File structure

- **Modify:** `wg-10/rust/src/facts_api.rs` — add a `relief_scale: f64` field + `configure` param; a private `scaled_base()` helper; apply it at the 3 `height::height` consume points (get_height:67, get_collision_field closure:136, bake_collision_region:196). One responsibility: the authoritative scaled height field.
- **Modify:** `wg-10/worldgen_terrain/shaders/ring_displace.gdshader` — add a `relief_scale` uniform; multiply the sampled height by it; the existing `height_scale` uniform is REPLACED by `relief_scale` (the render multiplier IS the relief scale now).
- **Modify:** `wg-10/rust/src/terrain_view.rs` — `Wg10TerrainView::configure` takes `relief_scale` instead of `height_scale`; passes it to the rings' material via `bind_tile` (the `height_scale` slot becomes `relief_scale`).
- **Modify:** `wg-10/rust/src/clipmap_rings.rs` — `bind_tile` sets the `relief_scale` shader uniform (rename the existing `height_scale` param/uniform set).
- **Modify (callers):** `wg-10/worldgen_terrain/harness/m3_review.gd` + any gate that calls `Wg10TerrainView.configure` / `Wg10Facts.configure` — pass `relief_scale` (replacing `HEIGHT_SCALE`).
- **Modify (gate):** `wg-10/worldgen_terrain/tests/facts_collision_parity_check.gd` — assert visible==collision parity WITH a non-1.0 relief_scale set (proves both sides scale identically).

> **The 3 facts consume points (verified):** `facts_api.rs` calls `height::height(...)` then `facts::composed_height(base, delta, floor, ceil)` at: get_height (line ~67), the get_collision_field closure (~136), and bake_collision_region composes the GPU `base` (~196). ALL THREE must scale the base. The edit `delta` and `floor`/`ceil` clamp are NOT scaled (an edit is an absolute metre dig; clamps are absolute bedrock) — only the procedural `base` scales.

> **Rust rebuild required this slice** (Rust changes). Use `tools/build_rust.ps1` (do NOT kill the editor; alt-tab + retry on a locked DLL, or ask the owner to close the editor). GDScript/shader changes hot-reload.

---

## Task 1: Add `relief_scale` to Wg10Facts (Rust) — scaled base at all 3 consume points

**Files:**
- Modify: `wg-10/rust/src/facts_api.rs`
- Test: `wg-10/worldgen_terrain/tests/facts_check.gd` (extend)

- [ ] **Step 1: Write the failing test (extend facts_check.gd)**

In `wg-10/worldgen_terrain/tests/facts_check.gd`, after the existing no-edit-parity assertions, add a relief_scale sub-test. Find where the test instantiates + configures `Wg10Facts` (it calls `facts.configure(dir, file, seed)`), and ADD a second facts instance configured with a relief_scale, asserting its height is exactly `relief_scale ×` the unscaled height:

```gdscript
	# relief_scale: a facts configured with relief_scale=R returns R× the unscaled base height
	# at every point (the authoritative world-relief knob; render + collision both honor it).
	var facts_scaled: Object = ClassDB.instantiate("Wg10Facts")
	var rs := 0.25
	var e2: String = str(facts_scaled.call("configure_scaled", pack_os, PACK_FILE, SEED, rs))
	if e2 != "":
		push_error("[facts] configure_scaled failed: %s" % e2); return 1
	var max_rel_err := 0.0
	for t in test_points:   # reuse the same coords the no-edit parity loop uses
		var h_unscaled: float = facts.call("get_height", t.x, t.y)
		var h_scaled: float = facts_scaled.call("get_height", t.x, t.y)
		var expected: float = h_unscaled * rs
		max_rel_err = max(max_rel_err, abs(h_scaled - expected))
	if max_rel_err > 1e-6:
		push_error("[facts] relief_scale mismatch: max|scaled - R*unscaled|=%.6g > 1e-6" % max_rel_err)
		return 1
	print("[facts] relief_scale ok (max_err=%.2g at R=%.2f)" % [max_rel_err, rs])
```

(NOTE: adapt `test_points` / coord access to the actual variable names in facts_check.gd — read the file first. If the existing test uses a different loop structure, mirror it. The assertion is: a relief_scale=0.25 facts returns exactly 0.25× the default facts height at the same coords, within 1e-6.)

- [ ] **Step 2: Run the test to verify it FAILS**

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools/gate.py --suite fast
```
Expected: FAIL — `Wg10Facts` has no `configure_scaled` method (the call errors / returns nothing), so the relief_scale assertion fails or the script errors. (This proves the test exercises the new path.)

- [ ] **Step 3: Add the relief_scale field + helper + configure_scaled (minimal impl)**

In `wg-10/rust/src/facts_api.rs`:

(a) Add the field to the struct (after `seed: i64,`):
```rust
    relief_scale: f64,   // authoritative world-relief multiplier on the base height field;
                         // render + facts/collision both apply it -> visible==collision held.
```

(b) Initialize it in `init` (after `seed: 0,`):
```rust
            relief_scale: 1.0,
```

(c) Add a free helper near the top of the file (after the `use` lines), so both get_height and the get_collision_field closure can call it without borrow conflicts:
```rust
/// The authoritative scaled base height: the parity-gated procedural height times the world-relief
/// multiplier. relief_scale is applied AFTER height::height (the formula is untouched -> M2 parity
/// holds); both the render shader and this facts path apply the SAME multiplier -> visible==collision.
#[inline]
fn scaled_base(x: f64, z: f64, seed: i64, pack: &Pack, relief_scale: f64) -> f64 {
    height::height(x, z, seed, pack) * relief_scale
}
```

(d) Add a `configure_scaled` method (keep the existing `configure` delegating to it with relief_scale=1.0, so existing callers are unaffected). In the `#[godot_api] impl Wg10Facts` block, replace the existing `configure` with:
```rust
    /// Load + validate the pack, set seed + relief_scale. relief_scale multiplies the base height
    /// field (default 1.0 = unscaled). Returns "" on success or the error message.
    #[func]
    fn configure_scaled(&mut self, dir: GString, file: GString, seed: i64, relief_scale: f64) -> GString {
        match pack::load_pack_dir(Path::new(&dir.to_string()), &file.to_string()) {
            Ok(p) => {
                self.pack = Some(p);
                self.seed = seed;
                self.relief_scale = relief_scale;
                GString::new()
            }
            Err(e) => GString::from(&e),
        }
    }

    /// Back-compat: configure with relief_scale = 1.0 (unscaled).
    #[func]
    fn configure(&mut self, dir: GString, file: GString, seed: i64) -> GString {
        self.configure_scaled(dir, file, seed, 1.0)
    }
```

(e) Apply `scaled_base` at the 3 consume points:
- In `get_height` (~line 67) replace `let base = height::height(x, z, self.seed, p);` with:
  ```rust
        let base = scaled_base(x, z, self.seed, p, self.relief_scale);
  ```
- In `get_collision_field` (~line 136), the closure: capture `relief_scale` and use the helper. Before the `facts::collision_field` call add `let relief_scale = self.relief_scale;`, then in the closure replace `let base = height::height(x, z, seed, p);` with:
  ```rust
                let base = scaled_base(x, z, seed, p, relief_scale);
  ```
- In `bake_collision_region` (~line 196), the GPU base must also scale. Replace `let b = base.get(k).unwrap_or(0.0);` with:
  ```rust
            let b = base.get(k).unwrap_or(0.0) * self.relief_scale as f32;
  ```
  (the GPU returned the raw base; scale it on the CPU compose pass, parity-identical to the CPU path.)

- [ ] **Step 4: Rebuild Rust**

```powershell
$env:CARGO_TARGET_DIR=$null
powershell -File tools/build_rust.ps1
```
Expected: builds clean. (If the DLL is locked by a running editor, alt-tab to release focus + retry, or ask the owner to close the editor. Do NOT force-kill it.)

- [ ] **Step 5: Run the test to verify it PASSES**

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools/gate.py --suite fast
```
Expected: `[gate] suite=fast checks=6 fail=0`, and the new `[facts] relief_scale ok` line prints (max_err ~0 at R=0.25). The relief_scale=0.25 facts returns exactly 0.25× the default heights.

- [ ] **Step 6: Run cargo tests (Rust core intact)**

```powershell
$env:CARGO_TARGET_DIR=$null
cd wg-10/rust; cargo test --quiet; cd ../..
```
Expected: `115 passed` (the helper + configure refactor don't change any existing Rust test; `height::height` is untouched).

- [ ] **Step 7: Commit**

```bash
git add wg-10/rust/src/facts_api.rs wg-10/worldgen_terrain/tests/facts_check.gd
git commit -m "shaded-scale s1: Wg10Facts relief_scale knob — scaled_base at all 3 consume points (facts_check green)"
```

---

## Task 2: Apply relief_scale in the render shader (replacing height_scale)

**Files:**
- Modify: `wg-10/worldgen_terrain/shaders/ring_displace.gdshader`

- [ ] **Step 1: Rename the height_scale uniform to relief_scale**

In `ring_displace.gdshader`, find:
```glsl
uniform float height_scale = 1.0;         // visual amplitude (config; 1.0 = raw metres)
```
Replace with:
```glsl
uniform float relief_scale = 1.0;         // authoritative world-relief multiplier (render side). Same
                                          // value as Wg10Facts.relief_scale -> visible==collision. Was
                                          // `height_scale`; folded into one relief knob (spec §5).
```

- [ ] **Step 2: Apply relief_scale to the displaced height**

In `vertex()`, find the final displacement line (it reads roughly):
```glsl
	VERTEX.y = (h + detail) * height_scale;
```
Replace with:
```glsl
	VERTEX.y = (h + detail) * relief_scale;
```
(Detail is part of the displaced surface; scaling the whole displaced height by relief_scale keeps detail proportional to terrain — correct. The base `h` here is the raw page height; relief_scale shrinks it to match the facts-side scaled_base, since facts scales height::height by the same relief_scale.)

- [ ] **Step 3: Verify the shader compiles (run the m5 detail gate, which uses this shader)**

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
& $env:GODOT_BIN --path wg-10 --import
& $env:GODOT_BIN --path wg-10 --script res://worldgen_terrain/m5/m5_detail_check.gd
```
Expected: the m5 gate still PASSES (the gate sets `height_scale`... → see Task 3; for now the shader compiles with the renamed uniform). If the gate sets `height_scale` it will now be a no-op uniform — Task 3 updates the gate. If the m5 gate fails ONLY because it sets the old name, that's expected and fixed in Task 3; confirm the shader itself compiles (no GLSL error).

- [ ] **Step 4: Commit**

```bash
git add wg-10/worldgen_terrain/shaders/ring_displace.gdshader
git commit -m "shaded-scale s1: shader height_scale -> relief_scale (one authoritative relief knob)"
```

---

## Task 3: Thread relief_scale through the render config (view, rings, callers)

**Files:**
- Modify: `wg-10/rust/src/terrain_view.rs`, `wg-10/rust/src/clipmap_rings.rs`
- Modify: callers — `m3_review.gd`, `m5_detail_check.gd`, `m5_perf_hardened_check.gd`, `m3_accept_check.gd`, any `Wg10TerrainView.configure` caller

- [ ] **Step 1: Rename height_scale → relief_scale in terrain_view.rs**

In `wg-10/rust/src/terrain_view.rs`: the struct field `height_scale: f64` → `relief_scale: f64`; the `configure` param `height_scale: f64` → `relief_scale: f64`; the `init` default; and where it's passed to `rings.bind_tile(...)` (the `self.height_scale` arg → `self.relief_scale`). This is a mechanical rename of the existing `height_scale` plumbing — it already flows view→bind_tile→material; we're just renaming it and (Task 2) the shader reads `relief_scale`.

- [ ] **Step 2: Rename in clipmap_rings.rs bind_tile**

In `wg-10/rust/src/clipmap_rings.rs`, `bind_tile`: the `height_scale: f64` param → `relief_scale: f64`, and the shader-parameter set:
```rust
        mat.set_shader_parameter("height_scale", &height_scale.to_variant());
```
→
```rust
        mat.set_shader_parameter("relief_scale", &relief_scale.to_variant());
```

- [ ] **Step 3: Rebuild Rust**

```powershell
$env:CARGO_TARGET_DIR=$null
powershell -File tools/build_rust.ps1
```
Expected: clean build.

- [ ] **Step 4: Update the GDScript callers**

In each of `m3_review.gd`, `m5_detail_check.gd`, `m5_perf_hardened_check.gd`, `m3_accept_check.gd` (and any other that calls `Wg10TerrainView.configure(...)` or sets the shader's `height_scale`): the `HEIGHT_SCALE` constant stays but is now semantically the relief_scale; the `view.configure(...)` call passes it in the same positional slot (the param is just renamed, position unchanged). Where a gate sets the shader uniform directly (`mat.set_shader_parameter("height_scale", ...)`), rename to `"relief_scale"`. For `m3_review.gd`, rename the `HEIGHT_SCALE := 0.35` constant to `RELIEF_SCALE := 0.25` (the new WG9-class default — a STARTING value for live tuning; the owner dials it in the fly).

- [ ] **Step 5: Run the windowed suites — render path intact**

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools/gate.py --suite m3
```
Expected: `[gate] suite=m3 checks=8 fail=0` (m5_detail, m5_perf_hardened, m3_view, etc. all still pass with the renamed uniform).

- [ ] **Step 6: Commit**

```bash
git add wg-10/rust/src/terrain_view.rs wg-10/rust/src/clipmap_rings.rs wg-10/worldgen_terrain/harness/m3_review.gd wg-10/worldgen_terrain/tests/m5_detail_check.gd wg-10/worldgen_terrain/m5/m5_detail_check.gd wg-10/worldgen_terrain/tests/m5_perf_hardened_check.gd wg-10/worldgen_terrain/tests/m3_accept_check.gd
git commit -m "shaded-scale s1: thread relief_scale through view/rings/callers (m3 8/8)"
```
(adjust the `git add` list to the files that actually exist + changed — confirm paths before committing.)

---

## Task 4: Prove visible==collision parity holds WITH relief_scale (the contract gate)

**Files:**
- Modify: `wg-10/worldgen_terrain/tests/facts_collision_parity_check.gd`

- [ ] **Step 1: Read the existing parity gate**

Read `wg-10/worldgen_terrain/tests/facts_collision_parity_check.gd` to see how it compares visible (GPU/render height) vs collision (facts `get_collision_field`). It currently configures facts with `configure(dir, file, seed)` (relief_scale implicitly 1.0) and asserts maxd ~0.0009 m.

- [ ] **Step 2: Add a relief_scale-on variant assertion**

After the existing parity assertion, add a second pass that configures the facts AND the render/GPU height with the SAME non-1.0 relief_scale and re-asserts parity. The key: BOTH the visible-height source and the collision-height source must use relief_scale=R, and their difference must stay within the same ~0.0009 m tolerance (×R, since heights are smaller). Concretely — configure facts via `configure_scaled(dir, file, seed, R)` and scale the visible/GPU comparison heights by R (the GPU parity height × R mirrors the shader's `* relief_scale`):

```gdscript
	# Parity must hold WITH relief_scale: both visible (×R in the shader) and collision (×R in facts)
	# scale identically, so the 0.0009 m contract holds (the relief knob can't open a see/collide gap).
	var R := 0.25
	var facts_r: Object = ClassDB.instantiate("Wg10Facts")
	var er: String = str(facts_r.call("configure_scaled", facts_pack_os, PACK_FILE, SEED, R))
	if er != "":
		push_error("[facts-parity] configure_scaled failed: %s" % er); return 1
	var maxd_r := 0.0
	for idx in sample_count:    # reuse the same sample coords as the base pass
		var x: float = sample_x[idx]
		var z: float = sample_z[idx]
		var visible_r: float = gpu_height[idx] * R          # shader displaces by ×relief_scale
		var collision_r: float = facts_r.call("get_height", x, z)   # facts scaled_base ×R
		maxd_r = max(maxd_r, abs(visible_r - collision_r))
	if maxd_r > 0.01:    # same absolute tolerance bracket as the base parity gate
		push_error("[facts-parity] relief_scale parity broken: maxd=%.6f m > 0.01 at R=%.2f" % [maxd_r, R])
		return 1
	print("[facts-parity] relief_scale parity ok (maxd=%.6f m at R=%.2f)" % [maxd_r, R])
```

(NOTE: adapt `sample_x/sample_z/gpu_height/sample_count/facts_pack_os` to the actual variable names in the gate — read it first. The principle: the visible height (GPU base × R) and the collision height (facts get_height, which is scaled_base × R) must agree within the existing tolerance, proving both honor relief_scale identically.)

- [ ] **Step 3: Run the gpu suite — parity holds with relief_scale**

```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools/gate.py --suite gpu
```
Expected: `[gate] suite=gpu checks=4 fail=0`, with both the base parity (`maxd≈0.0009`) and the new `[facts-parity] relief_scale parity ok` (maxd ~0.0009×0.25 ≈ 0.0002 m) passing. The raw `height()` parity (`gpu_parity_check`) is UNCHANGED (formula untouched).

- [ ] **Step 4: Commit**

```bash
git add wg-10/worldgen_terrain/tests/facts_collision_parity_check.gd
git commit -m "shaded-scale s1: gate visible==collision parity WITH relief_scale (both sides scale identically)"
```

---

## Task 5: Owner A/B fly + STATUS

- [ ] **Step 1: Owner flies the relief change.** Launch `m3_review.tscn`. The terrain now displaces at `relief_scale=0.25` (≈4× shorter than before). Confirm: relief looks saner (not 2.7 km spikes), terrain still streams/renders correctly, no see/collide weirdness. (Optional: the owner can try other relief_scale values by editing `RELIEF_SCALE` in m3_review.gd — it hot-reloads.) Record the value that feels WG9-right.

- [ ] **Step 2: Update STATUS.md.** Add a "Shaded-scale Slice 1" entry: relief_scale knob landed (render + facts in lockstep, one authoritative knob folding in height_scale); facts_check relief_scale assertion green; visible==collision parity holds with relief_scale (maxd ~0.0002 m); raw height() parity untouched; m3 8/8, gpu 4/4, fast 6/6, cargo 115; owner's chosen relief_scale value. Note S2 (normals + lighting) next.

- [ ] **Step 3: Commit STATUS.**
```bash
git add docs/plans/STATUS.md
git commit -m "shaded-scale s1: STATUS — relief_scale knob landed, parity held, owner value recorded"
```

---

## Self-review notes (planner)

- **Spec coverage (Slice 1):** spec §5 (relief multiplier, render+collision lockstep, raw parity untouched, fold in HEIGHT_SCALE) → Tasks 1–3; spec §7 (visible==collision parity gate with relief_scale) → Task 4; owner A/B → Task 5. Mesh density (§4) + normals (§6) are S2/S3, correctly absent here.
- **The 3 consume points** (get_height, get_collision_field closure, bake) all scale via `scaled_base`/`* relief_scale` — verified against facts_api.rs lines 67/136/196. Edit delta + clamps NOT scaled (absolute) — correct.
- **Parity safety:** relief_scale applied AFTER height::height → the M2 raw-formula parity gate (`gpu_parity_check`) is untouched; only the facts-vs-render (composed) parity extends to cover the scaled case.
- **Back-compat:** `configure` delegates to `configure_scaled(.., 1.0)` so any caller not passing relief_scale is unaffected (relief_scale=1.0 = old behavior).
- **Placeholder note:** `RELIEF_SCALE := 0.25` is a STARTING value for owner live-tuning (explicitly), not a guessed final — the owner sets the WG9-feel value in Task 5. Test values (R=0.25) are concrete.
- **Name consistency:** `relief_scale` (Rust field + shader uniform + view/rings param), `scaled_base` (helper), `configure_scaled` (method), `RELIEF_SCALE` (GDScript const) used identically across tasks. The shader uniform name `"relief_scale"` matches the `set_shader_parameter` string in clipmap_rings.rs.
- **Rust rebuild** flagged (Tasks 1, 3) via build_rust.ps1, don't-kill-editor honored.
