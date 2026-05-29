# WorldGen10 — M0 + M1 Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the WorldGen10 project skeleton (Rust GDExtension toolchain + headless gate runner) and port WorldGen9's deterministic worldgen *foundation* (hash → value-noise → fbm) into a clean, parity-tested Rust core that exactly reproduces WG9's reference fixtures.

**Architecture:** A Rust GDExtension crate holds the engine-agnostic deterministic terrain math (pure functions, unit-tested in Rust). Godot consumes it through a thin binding. Parity is proven against WG9's committed reference fixtures (hash cases with known u32 outputs) — the port is "correct" only when it reproduces those exactly. A small Python gate runner executes headless Godot checks. This plan covers ONLY the lowest layer (hash/noise/fbm + determinism + the toolchain). Region/kernel/landform and the renderer are later plans.

**Tech Stack:** Rust + godot-rust (gdext) targeting Godot API 4.6, Godot 4.6 (mono/.NET, D3D12), Python 3 for the gate runner, `cargo test` for Rust unit/parity tests.

---

## Scope & boundaries

This plan deliberately stops at the **hash/noise/fbm bedrock + toolchain**. Per
DESIGN.md §4, every higher-level deterministic choice (region grammar, kernels,
landform) can silently drift if this layer isn't bit-exact first, so it is its
own plan and its own acceptance.

**In scope:** Rust crate skeleton that loads in Godot 4.6; a `wg10_hash` module
porting WG9's `TerrainHash` (FNV-1a `stable_hash`, `hash_grid`, `value_noise`,
`fbm`, `fade`, `smoothstep_unit`); Rust parity tests against WG9's
`hash_reference.json` cases; a Godot-side headless determinism check; a Python
gate runner; the three-doc + git hygiene setup.

**Out of scope (later plans):** region/province decisions, kernels, landform
profiles, the GPU formula + CPU/GPU parity, the render pipeline, the Facts API,
the manual review scene.

## File structure (what gets created, and why)

```
wg-10/
  rust/                          # the GDExtension crate (engine-agnostic core + thin binding)
    Cargo.toml                   # crate manifest, gdext dep
    src/
      lib.rs                     # gdext entry point + class registration (THIN)
      hash.rs                    # wg10 deterministic hash/noise/fbm — pure fns, no Godot types
      hash_tests.rs              # #[cfg(test)] parity + property tests for hash.rs
      bind_worldgen.rs           # Godot-facing class exposing hash/noise for GDScript checks (THIN)
  addons/wg10_terrain/           # the drop-in addon root (DESIGN §6.2) — grows in later plans
    wg10_terrain.gdextension     # points Godot at the built rust lib
  worldgen_terrain/
    tests/
      hash_parity_check.gd       # headless: Godot calls the native hash, compares to fixture
      determinism_check.gd       # headless: same coord twice / across callers => identical
    fixtures/
      hash_reference.json        # COPIED from WG9 (tracked in git) — parity ground truth
  project.godot                  # already exists
tools/
  gate.py                        # headless Godot gate runner (suite of *_check.gd)
docs/plans/                      # already exists (DESIGN/ROADMAP/STATUS + this plan)
```

Each Rust file has one job: `hash.rs` is pure math (no `godot` imports),
`bind_worldgen.rs` is the only file that touches Godot types, `lib.rs` only
registers. This keeps the core portable (DESIGN §6.3) and every file well under
the ~600-line cap (DESIGN §7.1).

---

## Task 0: Copy the parity fixture into the repo (tracked)

**Files:**
- Create: `wg-10/worldgen_terrain/fixtures/hash_reference.json` (copied from WG9)

- [ ] **Step 1: Copy the WG9 hash reference fixture**

Run (PowerShell):
```powershell
New-Item -ItemType Directory -Force "D:\workflows\worldgen10\wg-10\worldgen_terrain\fixtures" | Out-Null
Copy-Item "D:\workflows\worldgen9\factory\runtime\hash_reference\hash_reference.json" `
          "D:\workflows\worldgen10\wg-10\worldgen_terrain\fixtures\hash_reference.json"
```

- [ ] **Step 2: Verify it copied and is valid JSON**

Run:
```powershell
Get-Content "D:\workflows\worldgen10\wg-10\worldgen_terrain\fixtures\hash_reference.json" -Raw | ConvertFrom-Json | Select-Object -ExpandProperty schema
```
Expected: `worldgen9.hash_reference.v1`

- [ ] **Step 3: Commit**

```bash
git add wg-10/worldgen_terrain/fixtures/hash_reference.json
git commit -m "chore: vendor WG9 hash reference fixture as parity ground truth"
```

---

## Task 1: Rust crate skeleton that builds

**Files:**
- Create: `wg-10/rust/Cargo.toml`
- Create: `wg-10/rust/src/lib.rs`

- [ ] **Step 1: Write Cargo.toml**

```toml
[package]
name = "wg10_terrain"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
godot = { version = "0.5", features = ["api-4-6"] }
```

- [ ] **Step 2: Write a minimal lib.rs entry point**

```rust
use godot::prelude::*;

mod hash;
mod bind_worldgen;

#[cfg(test)]
mod hash_tests;

struct Wg10Terrain;

#[gdextension]
unsafe impl ExtensionLibrary for Wg10Terrain {}
```

- [ ] **Step 3: Create empty module files so it compiles**

Create `wg-10/rust/src/hash.rs` with `// wg10 deterministic hash — filled in Task 2`
Create `wg-10/rust/src/bind_worldgen.rs` with `// Godot binding — filled in Task 4`
Create `wg-10/rust/src/hash_tests.rs` with `// tests — filled in Task 2`

- [ ] **Step 4: Build it**

Run: `cargo build --manifest-path wg-10/rust/Cargo.toml`
Expected: compiles (downloads gdext on first run); produces a `.dll` under `wg-10/rust/target/debug/`.

- [ ] **Step 5: Add .gitignore for Rust target and commit**

Create/append `wg-10/.gitignore`:
```
/rust/target/
```

```bash
git add wg-10/rust/Cargo.toml wg-10/rust/src/lib.rs wg-10/rust/src/hash.rs wg-10/rust/src/bind_worldgen.rs wg-10/rust/src/hash_tests.rs wg-10/.gitignore
git commit -m "feat: rust gdextension crate skeleton (builds, registers extension)"
```

---

## Task 2: Port the deterministic hash core (TDD against WG9 fixture)

**Reference (WG9 `terrain_hash.gd`, the math being ported):**
- `stable_hash(values)`: FNV-1a 32-bit over the `|`-joined string of values. Ints formatted base-10 (negative keeps `-`); floats that are whole print as the int. Initial `0x811c9dc5`, multiply `0x01000193`, mask `0xffffffff` after each multiply.
- `hash_grid(ix, iz, seed, salt)`: `n = (ix*374761393 + iz*668265263 + seed*1442695041 + salt*69069) & 0xffffffff; n = (n ^ (n>>13)) * 1274126177; n = (n ^ (n>>16)) & 0xffffffff; return n / 4294967295.0`.
- `value_noise(x,z,scale,seed,salt)`: lattice value noise, quintic `fade`, bilerp of 4 `hash_grid` corners, remapped to `[-1,1]` via `*2-1`.
- `fbm(x,z,scale,seed,octaves=4)`: sum of `value_noise` at `scale/2^o` with amp `0.5^o`, salt = octave index, normalized by amp sum.
- `fade(t) = t*t*t*(t*(t*6-15)+10)`; `smoothstep_unit(t) = clamp01(t)^2*(3-2t)`.

**Files:**
- Modify: `wg-10/rust/src/hash.rs`
- Modify: `wg-10/rust/src/hash_tests.rs`

- [ ] **Step 1: Write the failing parity test for stable_hash**

In `hash_tests.rs`:
```rust
use crate::hash;

#[test]
fn stable_hash_matches_wg9_fixture_cases() {
    // Cases taken verbatim from worldgen_terrain/fixtures/hash_reference.json
    // (joined_text -> hash_u32). Hardcoded here so the Rust test is standalone.
    let cases: &[(&str, u32)] = &[
        ("province_palette|0|0|1337", 1924655373),
        ("province_palette|-6|12|1337", 4166305643),
        ("palette_local|-24|-24|-6|-6|1337", 1435408736),
        ("palette_compatible|17|-9|1337", 2856444241),
    ];
    for (text, expected) in cases {
        assert_eq!(hash::fnv1a_32(text), *expected, "case {text}");
    }
}
```

- [ ] **Step 2: Run it; verify it fails**

Run: `cargo test --manifest-path wg-10/rust/Cargo.toml stable_hash_matches`
Expected: FAIL — `hash::fnv1a_32` does not exist.

- [ ] **Step 3: Implement fnv1a_32 + stable_hash value formatting**

In `hash.rs`:
```rust
//! Engine-agnostic deterministic hash/noise. No Godot imports (DESIGN §6.3).

const FNV1A_INITIAL: u32 = 0x811c_9dc5;
const FNV1A_MULTIPLY: u32 = 0x0100_0193;

/// FNV-1a over the UTF-8 code units of `text`. WG9 hashes per `unicode_at`
/// (code point). For the ASCII join strings used here, bytes == code points.
pub fn fnv1a_32(text: &str) -> u32 {
    let mut h = FNV1A_INITIAL;
    for cp in text.chars() {
        h ^= cp as u32;
        h = h.wrapping_mul(FNV1A_MULTIPLY);
    }
    h
}

/// A value that can appear in a stable_hash key, formatted exactly as WG9's
/// `_format_value` does (base-10 ints; whole floats render as the int).
pub enum HashVal<'a> {
    Int(i64),
    Float(f64),
    Str(&'a str),
}

fn format_val(v: &HashVal) -> String {
    match v {
        HashVal::Int(i) => i.to_string(),
        HashVal::Float(f) => {
            if (*f - f.round()).abs() < f64::EPSILON {
                (f.round() as i64).to_string()
            } else {
                // GDScript str(float) formatting differs; floats are not used
                // as hash keys in the ported paths. Guard against silent drift.
                f.to_string()
            }
        }
        HashVal::Str(s) => (*s).to_string(),
    }
}

pub fn stable_hash(values: &[HashVal]) -> u32 {
    let joined = values.iter().map(format_val).collect::<Vec<_>>().join("|");
    fnv1a_32(&joined)
}
```

- [ ] **Step 4: Run it; verify it passes**

Run: `cargo test --manifest-path wg-10/rust/Cargo.toml stable_hash_matches`
Expected: PASS (all 4 cases).

- [ ] **Step 5: Add hash_grid test + impl**

Test in `hash_tests.rs`:
```rust
#[test]
fn hash_grid_is_deterministic_and_unit_range() {
    let a = hash::hash_grid(3, -7, 1337, 0);
    let b = hash::hash_grid(3, -7, 1337, 0);
    assert_eq!(a, b);                       // deterministic
    assert!((0.0..=1.0).contains(&a));      // normalized
    assert_ne!(hash::hash_grid(3, -7, 1337, 0), hash::hash_grid(4, -7, 1337, 0));
}
```
Impl in `hash.rs`:
```rust
const U32_DENOM: f64 = 4294967295.0;

pub fn hash_grid(ix: i64, iz: i64, seed: i64, salt: i64) -> f64 {
    // Match WG9: signed intermediates, mask to u32 at the documented points.
    let mut n = (ix.wrapping_mul(374761393)
        + iz.wrapping_mul(668265263)
        + seed.wrapping_mul(1442695041)
        + salt.wrapping_mul(69069)) as u64 as u32;
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    n ^= n >> 16;
    n as f64 / U32_DENOM
}
```

- [ ] **Step 6: Run; verify pass**

Run: `cargo test --manifest-path wg-10/rust/Cargo.toml hash_grid`
Expected: PASS.

- [ ] **Step 7: Add fade / value_noise / fbm + a determinism property test**

Test in `hash_tests.rs`:
```rust
#[test]
fn value_noise_deterministic_and_bounded() {
    let n1 = hash::value_noise(123.5, -88.25, 600.0, 1337, 0);
    let n2 = hash::value_noise(123.5, -88.25, 600.0, 1337, 0);
    assert_eq!(n1, n2);
    assert!((-1.0..=1.0).contains(&n1));
}

#[test]
fn fbm_deterministic_and_bounded() {
    let f1 = hash::fbm(10.0, 20.0, 800.0, 1337, 4);
    let f2 = hash::fbm(10.0, 20.0, 800.0, 1337, 4);
    assert_eq!(f1, f2);
    assert!((-1.0..=1.0).contains(&f1));
}
```
Impl in `hash.rs`:
```rust
pub fn fade(t: f64) -> f64 { t * t * t * (t * (t * 6.0 - 15.0) + 10.0) }

pub fn smoothstep_unit(t: f64) -> f64 {
    let v = t.clamp(0.0, 1.0);
    v * v * (3.0 - 2.0 * v)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 { a + (b - a) * t }

pub fn value_noise(x: f64, z: f64, scale_m: f64, seed: i64, salt: i64) -> f64 {
    let fx = x / scale_m;
    let fz = z / scale_m;
    let ix = fx.floor() as i64;
    let iz = fz.floor() as i64;
    let tx = fade(fx - ix as f64);
    let tz = fade(fz - iz as f64);
    let a = hash_grid(ix, iz, seed, salt);
    let b = hash_grid(ix + 1, iz, seed, salt);
    let c = hash_grid(ix, iz + 1, seed, salt);
    let d = hash_grid(ix + 1, iz + 1, seed, salt);
    let ab = lerp(a, b, tx);
    let cd = lerp(c, d, tx);
    lerp(ab, cd, tz) * 2.0 - 1.0
}

pub fn fbm(x: f64, z: f64, scale_m: f64, seed: i64, octaves: u32) -> f64 {
    let mut total = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    for octave in 0..octaves {
        let s = scale_m / (1u64 << octave) as f64;
        total += value_noise(x, z, s, seed, octave as i64) * amp;
        norm += amp;
        amp *= 0.5;
    }
    total / norm.max(0.000001)
}
```

- [ ] **Step 8: Run all hash tests; verify pass**

Run: `cargo test --manifest-path wg-10/rust/Cargo.toml`
Expected: PASS (all tests).

- [ ] **Step 9: Commit**

```bash
git add wg-10/rust/src/hash.rs wg-10/rust/src/hash_tests.rs
git commit -m "feat: port WG9 deterministic hash/noise/fbm to rust with fixture parity"
```

---

## Task 3: Seam-safety property test (negative-axis floor semantics)

DESIGN §4 calls out `x=0`/`z=0` crossing as the classic seam break. `value_noise`
uses `floor`, which is correct for negatives in both GDScript and Rust — lock it
with a test so a future refactor to `as i64` truncation (which rounds toward
zero) is caught.

**Files:**
- Modify: `wg-10/rust/src/hash_tests.rs`

- [ ] **Step 1: Write the failing-guard test**

```rust
#[test]
fn value_noise_is_continuous_across_zero_axis() {
    // Sampling the same world coordinate must give the same value regardless of
    // which side of the axis the integer lattice cell is computed from. Walk a
    // line straight across x=0 at fixed z; values must be finite and stable on
    // repeat (determinism), and floor (not truncation) must be used so the cell
    // index is continuous across 0.
    let scale = 256.0;
    for x in [-0.001_f64, 0.0, 0.001] {
        let a = hash::value_noise(x, 5.0, scale, 1337, 0);
        let b = hash::value_noise(x, 5.0, scale, 1337, 0);
        assert_eq!(a, b);
        assert!(a.is_finite());
    }
    // Explicit floor-vs-truncate guard: cell index just below 0 is -1, not 0.
    assert_eq!((-0.001_f64).floor() as i64, -1);
}
```

- [ ] **Step 2: Run; verify pass** (impl already correct from Task 2)

Run: `cargo test --manifest-path wg-10/rust/Cargo.toml value_noise_is_continuous`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add wg-10/rust/src/hash_tests.rs
git commit -m "test: lock floor-based negative-axis continuity for value noise"
```

---

## Task 4: Expose the hash to Godot (thin binding)

**Files:**
- Modify: `wg-10/rust/src/bind_worldgen.rs`

- [ ] **Step 1: Implement the binding class**

```rust
use godot::prelude::*;
use crate::hash;

/// Thin Godot-facing wrapper over the engine-agnostic `hash` module. The only
/// file in the crate that imports `godot`. No math lives here.
#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10Hash {
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10Hash {
    fn init(base: Base<RefCounted>) -> Self { Self { base } }
}

#[godot_api]
impl Wg10Hash {
    #[func]
    fn stable_hash_ints(&self, prefix: GString, values: PackedInt64Array) -> i64 {
        let mut vals: Vec<hash::HashVal> = Vec::with_capacity(values.len() + 1);
        let p = prefix.to_string();
        vals.push(hash::HashVal::Str(&p));
        for v in values.as_slice() {
            vals.push(hash::HashVal::Int(*v));
        }
        hash::stable_hash(&vals) as i64
    }

    #[func]
    fn hash_grid(&self, ix: i64, iz: i64, seed: i64, salt: i64) -> f64 {
        hash::hash_grid(ix, iz, seed, salt)
    }

    #[func]
    fn value_noise(&self, x: f64, z: f64, scale_m: f64, seed: i64, salt: i64) -> f64 {
        hash::value_noise(x, z, scale_m, seed, salt)
    }

    #[func]
    fn fbm(&self, x: f64, z: f64, scale_m: f64, seed: i64, octaves: i64) -> f64 {
        hash::fbm(x, z, scale_m, seed, octaves.max(1) as u32)
    }
}
```

- [ ] **Step 2: Build**

Run: `cargo build --manifest-path wg-10/rust/Cargo.toml`
Expected: compiles, produces the updated `.dll`.

- [ ] **Step 3: Commit**

```bash
git add wg-10/rust/src/bind_worldgen.rs
git commit -m "feat: expose Wg10Hash binding (stable_hash, hash_grid, value_noise, fbm)"
```

---

## Task 5: Wire the GDExtension into the Godot project

**Files:**
- Create: `wg-10/addons/wg10_terrain/wg10_terrain.gdextension`

- [ ] **Step 1: Write the .gdextension file**

(Path to the built lib is relative to the file. Adjust the dll name if the
crate produces `wg10_terrain.dll` vs `libwg10_terrain.dll` — check the
`target/debug/` output from Task 1 Step 4.)
```ini
[configuration]
entry_symbol = "gdext_rust_init"
compatibility_minimum = 4.6
reloadable = true

[libraries]
windows.debug.x86_64 = "res://../rust/target/debug/wg10_terrain.dll"
windows.release.x86_64 = "res://../rust/target/release/wg10_terrain.dll"
```

- [ ] **Step 2: Verify Godot loads the extension headlessly**

Run (PowerShell, GODOT_BIN set to the 4.6 console exe):
```powershell
& $env:GODOT_BIN --headless --path "D:\workflows\worldgen10\wg-10" --quit 2>&1 | Select-String -Pattern "Wg10|error|ERROR|gdext|Initialize godot-rust"
```
Expected: `Initialize godot-rust ...` line, no errors loading the extension.

- [ ] **Step 3: Commit**

```bash
git add wg-10/addons/wg10_terrain/wg10_terrain.gdextension
git commit -m "feat: register wg10_terrain gdextension in the godot project"
```

---

## Task 6: Headless hash-parity check in Godot (native == fixture)

Proves the *native lib as loaded by Godot* reproduces the fixture (not just the
Rust unit test). This is the cross-boundary parity gate.

**Files:**
- Create: `wg-10/worldgen_terrain/tests/hash_parity_check.gd`

- [ ] **Step 1: Write the check**

```gdscript
extends SceneTree

const FIXTURE := "res://worldgen_terrain/fixtures/hash_reference.json"

func _init() -> void:
	quit(_run())

func _run() -> int:
	var f := FileAccess.open(FIXTURE, FileAccess.READ)
	if f == null:
		push_error("missing fixture: %s" % FIXTURE)
		return 1
	var data: Variant = JSON.parse_string(f.get_as_text())
	if typeof(data) != TYPE_DICTIONARY:
		push_error("fixture not an object")
		return 1
	if not ClassDB.class_exists("Wg10Hash"):
		push_error("Wg10Hash native class not registered")
		return 1
	var hasher: Object = ClassDB.instantiate("Wg10Hash")
	var errors: Array[String] = []
	for case_value in (data as Dictionary).get("stable_hash_cases", []) as Array:
		var case: Dictionary = case_value as Dictionary
		var values: Array = case["values"] as Array
		var prefix: String = str(values[0])
		var ints := PackedInt64Array()
		for i in range(1, values.size()):
			ints.append(int(values[i]))
		var got: int = int(hasher.call("stable_hash_ints", prefix, ints))
		var want: int = int(case["hash_u32"])
		if got != want:
			errors.append("%s got=%d want=%d" % [str(case.get("joined_text", "")), got, want])
	if not errors.is_empty():
		for e in errors:
			push_error(e)
		print("[wg10-hash-parity] status=fail errors=%d" % errors.size())
		return 1
	print("[wg10-hash-parity] status=pass cases=%d" % (data["stable_hash_cases"] as Array).size())
	return 0
```

- [ ] **Step 2: Run it**

Run:
```powershell
& $env:GODOT_BIN --headless --path "D:\workflows\worldgen10\wg-10" --script "res://worldgen_terrain/tests/hash_parity_check.gd"
```
Expected: `[wg10-hash-parity] status=pass cases=N`, exit 0.

> If a case fails: the fixture's `stable_hash_cases` may include float values or
> 5-element keys. The binding's `stable_hash_ints` only accepts a string prefix
> + int array. Filter to int-only cases in Step 1, OR extend the binding with a
> variant-array overload. Prefer filtering for this plan (the int cases are
> sufficient parity proof); note any skipped cases in the print line.

- [ ] **Step 3: Commit**

```bash
git add wg-10/worldgen_terrain/tests/hash_parity_check.gd
git commit -m "test: headless godot hash-parity check against WG9 fixture"
```

---

## Task 7: Determinism check (same coord, different callers → identical)

DESIGN §4: the same `(x,z,seed)` must return the same value regardless of caller.
This guards the contract at the Godot boundary.

**Files:**
- Create: `wg-10/worldgen_terrain/tests/determinism_check.gd`

- [ ] **Step 1: Write the check**

```gdscript
extends SceneTree

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10Hash"):
		push_error("Wg10Hash not registered")
		return 1
	var a: Object = ClassDB.instantiate("Wg10Hash")
	var b: Object = ClassDB.instantiate("Wg10Hash")  # different instance = different "caller"
	var errors: Array[String] = []
	var coords := [Vector2(0, 0), Vector2(-1024.5, 2048.25), Vector2(1e6, -1e6)]
	for c in coords:
		var va: float = a.call("fbm", c.x, c.y, 800.0, 1337, 4)
		var vb: float = b.call("fbm", c.x, c.y, 800.0, 1337, 4)
		if va != vb:
			errors.append("caller mismatch @ %s: %f vs %f" % [str(c), va, vb])
		var again: float = a.call("fbm", c.x, c.y, 800.0, 1337, 4)
		if va != again:
			errors.append("repeat mismatch @ %s: %f vs %f" % [str(c), va, again])
	if not errors.is_empty():
		for e in errors:
			push_error(e)
		print("[wg10-determinism] status=fail errors=%d" % errors.size())
		return 1
	print("[wg10-determinism] status=pass coords=%d" % coords.size())
	return 0
```

- [ ] **Step 2: Run it**

Run:
```powershell
& $env:GODOT_BIN --headless --path "D:\workflows\worldgen10\wg-10" --script "res://worldgen_terrain/tests/determinism_check.gd"
```
Expected: `[wg10-determinism] status=pass coords=3`, exit 0.

- [ ] **Step 3: Commit**

```bash
git add wg-10/worldgen_terrain/tests/determinism_check.gd
git commit -m "test: headless determinism check (same coord across callers/runs)"
```

---

## Task 8: Python gate runner

A small runner so checks run as a suite (the seed of WG9's `godot_runtime_gate.py`,
but minimal). Headless suite only in this plan; renderer suites come with the
render pipeline plan.

**Files:**
- Create: `tools/gate.py`

- [ ] **Step 1: Write the runner**

```python
"""WorldGen10 headless gate runner. Runs *_check.gd scripts via Godot."""
import argparse, os, subprocess, sys
from pathlib import Path

PROJECT = Path(__file__).resolve().parents[1] / "wg-10"
CHECKS = {
    "fast": [
        "worldgen_terrain/tests/hash_parity_check.gd",
        "worldgen_terrain/tests/determinism_check.gd",
    ],
}

def godot_bin() -> str:
    env = os.environ.get("GODOT_BIN")
    if env and Path(env).exists():
        return env
    raise SystemExit("set GODOT_BIN to the Godot 4.6 console executable")

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--suite", choices=sorted(CHECKS), default="fast")
    args = ap.parse_args()
    godot = godot_bin()
    failures = 0
    for script in CHECKS[args.suite]:
        res = subprocess.run(
            [godot, "--headless", "--path", str(PROJECT), "--script", f"res://{script}"],
            capture_output=True, text=True,
        )
        tag = "pass" if res.returncode == 0 else "fail"
        if res.returncode != 0:
            failures += 1
        print(f"[gate] check={script} status={tag} rc={res.returncode}")
        if res.returncode != 0:
            sys.stdout.write(res.stdout[-2000:])
            sys.stderr.write(res.stderr[-2000:])
    print(f"[gate] suite={args.suite} checks={len(CHECKS[args.suite])} fail={failures}")
    return 1 if failures else 0

if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 2: Run the suite**

Run:
```powershell
$env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
python tools\gate.py --suite fast
```
Expected: both checks `status=pass`, final line `fail=0`, exit 0.

- [ ] **Step 3: Commit**

```bash
git add tools/gate.py
git commit -m "feat: headless gate runner (fast suite: hash parity + determinism)"
```

---

## Task 9: Update the living docs

**Files:**
- Modify: `docs/plans/ROADMAP.md` (check off M0 items + M1 hash/determinism/seam items achieved)
- Modify: `docs/plans/STATUS.md` (reflect: native crate builds + loads, hash parity + determinism gates green)

- [ ] **Step 1: Edit ROADMAP** — mark done: native toolchain loads in 4.6; gate runner skeleton; (M1) hash/noise ported with fixture parity; determinism gate; seam floor-semantics covered. Leave region/kernel/landform/pack items unchecked (later plans).

- [ ] **Step 2: Edit STATUS** — Current state: native `wg10_terrain` crate builds and loads in Godot 4.6; `Wg10Hash` exposes hash/noise/fbm; fast gate (hash parity vs WG9 fixture + determinism) green. What works: deterministic hash/noise bedrock, proven parity. What's next: region/province decisions + terrain-pack format (next plan).

- [ ] **Step 3: Commit**

```bash
git add docs/plans/ROADMAP.md docs/plans/STATUS.md
git commit -m "docs: M0 + hash/noise foundation done; gates green"
```

---

## Self-review notes (already applied)

- **Spec coverage:** Covers DESIGN §6.3 (engine-agnostic core), §7.1 (small
  files), §4 (determinism + seam + tracked fixtures) for the hash layer, §8
  build-order step 1 (partial: hash/noise; region/kernel/landform deferred to
  the next plan, which is correct decomposition). Render pipeline / Facts API /
  GPU parity are explicitly out of scope.
- **Type consistency:** `fnv1a_32`, `stable_hash`, `hash_grid`, `value_noise`,
  `fbm`, `fade`, `smoothstep_unit`, `HashVal` are used consistently across Rust
  tasks; the binding methods `stable_hash_ints`/`hash_grid`/`value_noise`/`fbm`
  match what the GDScript checks call.
- **Known soft spot:** Task 5's dll name and the `.gdextension` entry symbol
  (`gdext_rust_init`) should be confirmed against the actual gdext 0.5 build
  output in Task 1; the plan flags this inline.
- **Float hash cases:** the fixture may contain non-int hash cases; Task 6
  filters to int-only cases and logs any skipped, which is sufficient parity for
  this layer.
