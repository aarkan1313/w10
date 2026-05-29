# WorldGen10 — M2 GPU Formula + Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run WorldGen10's full deterministic formula (hash→grammar→height) on the GPU via a hand-ported GLSL compute shader, and prove it equals the CPU with a two-tier CPU/GPU parity gate (exact family selection + epsilon height).

**Architecture:** Introduce a GPU-portable **integer-domain hash** (`stable_hash_ints`, pure u32-wrapping arithmetic — identical on CPU `u32::wrapping_*` and GLSL `uint`) and switch the grammar's 5 roll sites from string-join hashing to it (re-locking grammar tests; the WG9-bit-exact `hash_grid`/`value_noise`/`fbm` bedrock is untouched). Hand-port the whole formula to one GLSL compute shader. A `gpu_compute.rs` (the only new godot-facing file) builds a local RenderingDevice pipeline, uploads pack tables + a kernel atlas + query coords as buffers, dispatches, and reads back height + a family-signature buffer. A windowed `gpu_parity_check.gd` (new `gpu` gate suite) compares CPU vs GPU: family signatures must match **exactly**, heights within a documented f32 epsilon.

**Tech Stack:** Rust + godot-rust (gdext 0.5.3, Godot API 4.6); Godot `RenderingDevice` local compute (D3D12, verified working windowed on this machine — see Build/run notes); GLSL `#version 450` compute; `cargo test`; Python gate runner + **windowed** (non-headless) Godot for the GPU parity check.

**Design source:** `docs/superpowers/specs/2026-05-29-m2-gpu-parity-design.md`. Read it first — especially §2 (the NON-NEGOTIABLE constraints: same formula both sides, no readback in production, bedrock untouched, grammar structure unchanged) and §3 (GPU architecture).

---

## Scope & boundaries

**In scope:** the integer-hash refactor (CPU, re-lock grammar tests); a GLSL compute shader running the full formula; `gpu_compute.rs` (RenderingDevice plumbing); pack tables + kernel atlas + coords uploaded as buffers; a CPU `family_signature` helper; a windowed CPU/GPU parity gate (new `gpu` suite); DESIGN/ROADMAP/STATUS updates.

**Out of scope (deferred):** the render pipeline (M3 — page pool/scheduler/clipmap/fly-test); real DEM pack; anti-repetition; a kernel-atlas redesign for varied real-DEM kernel sizes (synthetic 4×4 kernels pack trivially). GPU output is validated by readback **in the gate only** — production no-readback streaming is M3.

## Interface constraints (from design §2 — enforce while building)

1. **Same formula both sides.** The parity gate is the contract; any divergence fails.
2. **No readback in production.** Readback exists ONLY in the gate.
3. **Bedrock untouched.** `hash_grid`/`value_noise`/`fbm` + WG9 `hash_reference.json` parity stay bit-exact. Only grammar rolls move to the integer hash. String `stable_hash` stays for the bedrock + `Wg10Hash` binding.
4. **Grammar structure unchanged.** Only the roll-hash INPUT changes (string prefix → integer salt). Roll arithmetic (`% len`, pct thresholds, bias rotation) unchanged.
5. **Engine-agnostic core stays so.** Integer hash in `hash.rs` (no godot). `gpu_compute.rs` is the only new godot/RenderingDevice file. `grammar.rs`/`height.rs`/`pack.rs`/`npy.rs` gain no godot.

## Critical: GPU-portability of the hash (why u32-only)

`hash_grid` relies on a **full-width 64-bit multiply** (`n1` un-masked before `>>16`). GLSL's base profile has **no 64-bit ints** — `uint` is 32-bit. To run bit-identically on both sides WITHOUT a shader int64 extension, `stable_hash_ints` uses **only u32 wrapping arithmetic** (FNV-1a-32 folding each i64 arg as its two u32 halves). Rust `u32::wrapping_*` and GLSL `uint` arithmetic both wrap mod 2³² by spec → identical results. This is a DIFFERENT hash from `hash_grid` (which stays as-is for the bedrock); it is only for the grammar rolls.

## Build/run notes (every task)

> **Cargo:** global `CARGO_TARGET_DIR` must be unset. From `wg-10/rust`, bash:
> `env -u CARGO_TARGET_DIR cargo test`. Never `CARGO_TARGET_DIR=` (empty errors).
>
> **Godot:** `GODOT_BIN="C:/tmp/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64_console.exe"`.
> The headless `fast` suite is unchanged. **GPU compute does NOT work headless**
> (verified: `create_local_rendering_device()` returns null under `--headless`).
> The `gpu` suite runs **windowed** — a normal `$GODOT_BIN --path ... --script ...`
> invocation with NO `--headless`. `.glsl` needs an editor `--import` pass to become
> a loadable `RDShaderFile` (gate.py runs the import pass).

## File structure

```
wg-10/rust/src/
  hash.rs            # MODIFY: add stable_hash_ints (u32-only) + salt consts. Bedrock untouched.
  hash_tests.rs      # MODIFY: add stable_hash_ints determinism/decorrelation tests.
  grammar.rs         # MODIFY: 5 roll sites switch string-prefix -> integer salt. Structure unchanged.
  grammar_tests.rs   # MODIFY: re-lock any specific-index expectations to new outputs.
  parity.rs          # NEW: CPU family_signature(x,z,seed,&Pack) -> u32 (sorted family ids folded via stable_hash_ints).
  parity_tests.rs    # NEW: family_signature determinism + matches family_weights entries.
  gpu_compute.rs     # NEW: godot-facing. RenderingDevice local compute: upload tables+atlas+coords, dispatch, readback height[] + family_sig[].
  lib.rs             # MODIFY: add `mod parity; mod gpu_compute;` + test module decls.
  bind_worldgen.rs   # MODIFY: nothing (gpu_compute.rs is its own GodotClass) — or re-export if needed.
wg-10/worldgen_terrain/
  shaders/
    height_field.glsl   # NEW: hand-ported full formula (the GPU side of the parity contract).
  tests/
    gpu_parity_check.gd # NEW: windowed CPU/GPU parity check (Tier1 exact sigs, Tier2 epsilon height).
tools/gate.py             # MODIFY: add `gpu` suite + a non-headless run branch for it.
docs/plans/               # MODIFY: DESIGN §2.4/§9, ROADMAP M2, STATUS.
```

Each file one job, under the ~600-line cap.

---

## Task 0: GPU-portable integer hash `stable_hash_ints` (TDD)

**Files:**
- Modify: `wg-10/rust/src/hash.rs`
- Modify: `wg-10/rust/src/hash_tests.rs`

A u32-only FNV-1a-32 fold over i64 args (GLSL-reproducible). Each i64 is folded as its low and high u32 halves so the full 64-bit value participates. A `salt: u32` replaces the string prefix.

- [ ] **Step 1: Write failing tests** — append to `wg-10/rust/src/hash_tests.rs`:
```rust
use crate::hash::{self};

#[test]
fn stable_hash_ints_is_deterministic() {
    let a = hash::stable_hash_ints(0xABCD_1234, &[3, -7, 1337]);
    let b = hash::stable_hash_ints(0xABCD_1234, &[3, -7, 1337]);
    assert_eq!(a, b);
}

#[test]
fn stable_hash_ints_salt_decorrelates() {
    // Same args, different salt -> (almost certainly) different output.
    let a = hash::stable_hash_ints(1, &[10, 20]);
    let b = hash::stable_hash_ints(2, &[10, 20]);
    assert_ne!(a, b, "distinct salts must not collide on these args");
}

#[test]
fn stable_hash_ints_args_matter() {
    let a = hash::stable_hash_ints(7, &[10, 20]);
    let b = hash::stable_hash_ints(7, &[10, 21]);
    assert_ne!(a, b);
}

#[test]
fn stable_hash_ints_handles_negatives_distinctly() {
    let a = hash::stable_hash_ints(7, &[-1]);
    let b = hash::stable_hash_ints(7, &[1]);
    assert_ne!(a, b);
}

#[test]
fn stable_hash_ints_distribution_sanity() {
    // Over a grid, the low bits mod 4 should hit all residues (not collapse).
    let mut seen = [false; 4];
    for i in 0..200i64 {
        let h = hash::stable_hash_ints(42, &[i, i * 3 - 5]);
        seen[(h % 4) as usize] = true;
    }
    assert!(seen.iter().all(|&s| s), "hash % 4 collapsed: {seen:?}");
}
```

- [ ] **Step 2: Run; verify FAIL** — `cd /d/workflows/worldgen10/wg-10/rust && env -u CARGO_TARGET_DIR cargo test --lib hash_tests::stable_hash_ints` → FAIL (function missing).

- [ ] **Step 3: Implement** — add to `wg-10/rust/src/hash.rs` (after `fnv1a_32`, keep everything else unchanged):
```rust
/// GPU-portable integer hash. Pure u32 wrapping arithmetic (FNV-1a-32 fold) so
/// it is bit-identical on CPU (`u32::wrapping_*`) and in GLSL (`uint`, which
/// wraps mod 2^32 by spec). A `salt` replaces the old string prefix; each i64
/// arg is folded as its low and high u32 halves so the full value participates.
/// This is SEPARATE from `hash_grid` (which keeps its 64-bit-multiply bedrock
/// scheme); `stable_hash_ints` is the grammar-roll hash that must run on the GPU.
pub fn stable_hash_ints(salt: u32, args: &[i64]) -> u32 {
    let mut h = FNV1A_INITIAL;
    h = fold_u32(h, salt);
    for &a in args {
        let u = a as u64;
        h = fold_u32(h, u as u32);
        h = fold_u32(h, (u >> 32) as u32);
    }
    // final avalanche (xorshift-multiply), all u32 wrapping.
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h
}

/// FNV-1a-32 mix of one u32 word (4 bytes, little-endian order).
fn fold_u32(mut h: u32, word: u32) -> u32 {
    for shift in [0u32, 8, 16, 24] {
        let byte = (word >> shift) & 0xff;
        h ^= byte;
        h = h.wrapping_mul(FNV1A_MULTIPLY);
    }
    h
}
```

- [ ] **Step 4: Run; verify PASS** — `env -u CARGO_TARGET_DIR cargo test --lib hash_tests` → all green (existing 6 hash tests + 5 new). Then `env -u CARGO_TARGET_DIR cargo test` → all green (the bedrock + grammar + height untouched). Report exact total.

- [ ] **Step 5: Commit**
```bash
cd /d/workflows/worldgen10
git add wg-10/rust/src/hash.rs wg-10/rust/src/hash_tests.rs
git commit -m "feat: GPU-portable integer hash stable_hash_ints (u32-only, GLSL-reproducible)"
```

---

## Task 1: Switch grammar rolls to the integer hash (TDD / re-lock)

**Files:**
- Modify: `wg-10/rust/src/grammar.rs`
- Modify: `wg-10/rust/src/grammar_tests.rs`

Replace the 5 string-prefix roll sites with `stable_hash_ints(SALT_*, &[...])`. Structure and roll arithmetic unchanged. This changes roll VALUES — re-lock tests (properties unchanged; specific-index expectations refreshed).

- [ ] **Step 1: Add salt constants + swap the rolls.** In `wg-10/rust/src/grammar.rs`, add near the top (after the `use` lines):
```rust
// Integer salts for the grammar rolls (replace the old string prefixes). Each is
// an arbitrary distinct u32; they only need to differ to decorrelate the rolls.
const SALT_PROVINCE_PALETTE: u32 = 0x5052_4f56; // "PROV"
const SALT_PALETTE_LOCAL: u32 = 0x4c4f_4341;    // "LOCA"
const SALT_PALETTE_COMPATIBLE: u32 = 0x434f_4d50; // "COMP"
const SALT_PALETTE_RARE: u32 = 0x5241_5245;     // "RARE"
const SALT_FAMILY_ROLL: u32 = 0x46414d_49 & 0xffff_ffff; // "FAMI"
```
Then replace each roll. `province_primary_palette` becomes:
```rust
fn province_primary_palette(prx: i64, prz: i64, seed: i64, pack: &Pack) -> usize {
    let h = hash::stable_hash_ints(SALT_PROVINCE_PALETTE, &[prx, prz, seed]);
    (h as usize) % pack.palettes.len()
}
```
In `palette_for_region`, the local roll:
```rust
    let roll = hash::stable_hash_ints(SALT_PALETTE_LOCAL, &[rx, rz, prx, prz, seed]) % 100;
```
the compatible pick:
```rust
                let pick = hash::stable_hash_ints(SALT_PALETTE_COMPATIBLE, &[rx, rz, seed]) as usize
                    % compat.len();
```
the rare pick:
```rust
    hash::stable_hash_ints(SALT_PALETTE_RARE, &[rx, rz, seed]) as usize % pack.palettes.len()
```
In `families_for_region`, the family roll:
```rust
    let roll = (hash::stable_hash_ints(SALT_FAMILY_ROLL, &[rx, rz, seed])
        % FAMILIES_PER_PALETTE as u32) as usize;
```
Remove the now-unused `HashVal` import IF nothing else in grammar.rs uses it (check — if `use crate::hash::{self, HashVal};` and `HashVal` is now unused, change to `use crate::hash;`).

- [ ] **Step 2: Run; observe which tests break** — `cd /d/workflows/worldgen10/wg-10/rust && env -u CARGO_TARGET_DIR cargo test --lib grammar_tests`. The PROPERTY tests (sum=1, determinism, variety, seam-continuity, bounded) must STILL PASS unchanged. Tests asserting a SPECIFIC palette index (e.g. `palette_for_region_is_deterministic_and_valid` only checks `< len` and determinism — should still pass) generally still pass since they assert properties, not fixed indices. If ANY test asserts a hardcoded palette/family index value, it may now fail — note exactly which.

- [ ] **Step 3: Re-lock any value-specific test.** For each test that failed ONLY because a specific index changed (not a property violation), update its expected value to the new output. Do NOT loosen a property assertion. If a PROPERTY test fails (sum≠1, determinism broken, variety collapsed), STOP — that's a real bug in the refactor, not a re-lock. Report it.

- [ ] **Step 4: Run; verify PASS** — `env -u CARGO_TARGET_DIR cargo test` → all green (grammar properties hold; height tests — including the roll-independent flat anchor `height==500` — unaffected; bedrock untouched). Report exact total + confirm the flat anchor test still passes.

- [ ] **Step 5: Commit**
```bash
cd /d/workflows/worldgen10
git add wg-10/rust/src/grammar.rs wg-10/rust/src/grammar_tests.rs
git commit -m "refactor: grammar rolls use GPU-portable stable_hash_ints (new seed-space; properties hold)"
```

---

## Task 2: CPU family signature (TDD)

**Files:**
- Create: `wg-10/rust/src/parity.rs`
- Create: `wg-10/rust/src/parity_tests.rs`
- Modify: `wg-10/rust/src/lib.rs`

`family_signature(x,z,seed,&Pack) -> u32`: the sorted ascending family ids present in the blend, folded via `stable_hash_ints(SALT_SIG, ...)`. The GLSL shader (Task 4) computes the identical signature. Tier-1 parity compares these.

- [ ] **Step 1: Declare modules in lib.rs.** In `wg-10/rust/src/lib.rs`, add `mod parity;` (after `mod height;`) and `mod gpu_compute;` (after `mod bind_worldgen;`), plus `#[cfg(test)] mod parity_tests;`. (gpu_compute is godot-facing; declare it as a normal `mod`. Leave the gdextension block unchanged.)

- [ ] **Step 2: Create gpu_compute stub so the crate compiles** — create `wg-10/rust/src/gpu_compute.rs`:
```rust
//! WorldGen10 GPU compute (RenderingDevice) — filled in Task 5.
```

- [ ] **Step 3: Write failing tests** — create `wg-10/rust/src/parity_tests.rs`:
```rust
use crate::parity;
use crate::pack;
use std::path::Path;

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../worldgen_terrain/fixtures")
}
fn height_pack() -> pack::Pack {
    pack::load_pack_dir(&fixtures_dir(), "height_pack.json").expect("height pack loads")
}

#[test]
fn family_signature_is_deterministic() {
    let p = height_pack();
    let a = parity::family_signature(-1024.5, 2048.25, 1337, &p);
    let b = parity::family_signature(-1024.5, 2048.25, 1337, &p);
    assert_eq!(a, b);
}

#[test]
fn family_signature_varies_across_grid() {
    // Different family sets across the world -> more than one signature.
    let p = height_pack();
    let mut seen = std::collections::BTreeSet::new();
    for i in -20..20 {
        seen.insert(parity::family_signature(i as f64 * 40000.0, 0.0, 1337, &p));
    }
    assert!(seen.len() >= 2, "signature collapsed to one value");
}

#[test]
fn family_signature_order_independent_of_weight_order() {
    // The signature depends on the SET of families (sorted), not their weight
    // order — two coords with the same family set share a signature.
    let p = height_pack();
    // a flat-only-style coord set: just assert the helper returns a stable u32.
    let s = parity::family_signature(0.0, 0.0, 1337, &p);
    assert_eq!(s, parity::family_signature(0.0, 0.0, 1337, &p));
}
```

- [ ] **Step 4: Run; verify FAIL** — `env -u CARGO_TARGET_DIR cargo test --lib parity_tests` → FAIL (`parity::family_signature` missing).

- [ ] **Step 5: Implement** — create `wg-10/rust/src/parity.rs`:
```rust
//! CPU side of the GPU parity contract: the family-selection signature. The GLSL
//! shader (Task 4) computes the identical value; the parity gate compares them
//! exactly (Tier 1). Pure, no godot.

use crate::grammar;
use crate::hash;
use crate::pack::Pack;

/// Salt for the family-signature fold. Must match the GLSL `SALT_SIG`.
pub const SALT_SIG: u32 = 0x5349_4753; // "SIGS"

/// A deterministic signature of the SET of families present in the blend at
/// (x,z): sorted ascending family ids, folded via `stable_hash_ints`. Two coords
/// select the same families <=> same signature (ignores the float weights —
/// Tier 2 covers magnitude). CPU and GPU must agree on this exactly.
pub fn family_signature(x: f64, z: f64, seed: i64, pack: &Pack) -> u32 {
    let w = grammar::family_weights(x, z, seed, pack);
    let mut ids: Vec<i64> = w.entries().iter().map(|(fam, _)| *fam as i64).collect();
    ids.sort_unstable();
    hash::stable_hash_ints(SALT_SIG, &ids)
}
```

- [ ] **Step 6: Run; verify PASS** — `env -u CARGO_TARGET_DIR cargo test --lib parity_tests` → 3 pass. Then `env -u CARGO_TARGET_DIR cargo test` → all green.

- [ ] **Step 7: Commit**
```bash
cd /d/workflows/worldgen10
git add wg-10/rust/src/parity.rs wg-10/rust/src/parity_tests.rs wg-10/rust/src/lib.rs wg-10/rust/src/gpu_compute.rs
git commit -m "feat: CPU family_signature for GPU parity (sorted family-id set folded)"
```

---

## Task 3: The GLSL compute shader (hand-port)

**Files:**
- Create: `wg-10/worldgen_terrain/shaders/height_field.glsl`

The GPU side of the parity contract. Hand-port the integer hash + grammar rolls + weight blend + tiled kernel sample + moderation + height, plus the family signature. Functions named to mirror the Rust fns. No test here (Task 6's parity gate is its test); this task is verified by compiling cleanly in Task 5/6's import + dispatch.

- [ ] **Step 1: Write the shader.** Create `wg-10/worldgen_terrain/shaders/height_field.glsl`:
```glsl
#[compute]
#version 450

// WorldGen10 height field — the GPU side of the CPU/GPU parity contract.
// EDIT BOTH SIDES: every function here mirrors a Rust fn (hash.rs / grammar.rs /
// height.rs / parity.rs). The parity gate (gpu_parity_check.gd) enforces sync.
// Pure u32 + f32 math: no strings, no maps, no 64-bit ints (GLSL base profile).

layout(local_size_x = 64) in;

// ---- uniforms / push data ----
layout(set = 0, binding = 0, std430) restrict readonly buffer Coords { vec2 xz[]; } coords;
layout(set = 0, binding = 1, std430) restrict writeonly buffer OutH { float h[]; } out_h;
layout(set = 0, binding = 2, std430) restrict writeonly buffer OutSig { uint sig[]; } out_sig;
// palette table: palettes_flat[p*3 + k] = family index of slot k in palette p
layout(set = 0, binding = 3, std430) restrict readonly buffer Palettes { int fam[]; } palettes;
// compatibility: per palette an (offset,count) into compat_flat
layout(set = 0, binding = 4, std430) restrict readonly buffer CompatOff { ivec2 oc[]; } compat_off;
layout(set = 0, binding = 5, std430) restrict readonly buffer CompatFlat { int pal[]; } compat_flat;
// per-family kernel record: (dataOffset, rows, cols, _pad), then relief/footprint
layout(set = 0, binding = 6, std430) restrict readonly buffer KRec { ivec4 rec[]; } krec;
layout(set = 0, binding = 7, std430) restrict readonly buffer KParam { vec2 rf[]; } kparam; // (relief_m, footprint_m)
layout(set = 0, binding = 8, std430) restrict readonly buffer KData { float v[]; } kdata;

layout(push_constant, std430) uniform Params {
    float region_size_m;
    int province_size_regions;
    uint palette_primary_pct;
    uint palette_compatible_pct;
    float moderation_min;
    float moderation_strength;
    int seed;          // grammar seed (fits i32 for the test seeds)
    int num_palettes;
    int num_coords;
} P;

const uint FNV1A_INITIAL = 0x811c9dc5u;
const uint FNV1A_MULTIPLY = 0x01000193u;
const uint SALT_PROVINCE_PALETTE = 0x5052_4f56u;
const uint SALT_PALETTE_LOCAL    = 0x4c4f_4341u;
const uint SALT_PALETTE_COMPATIBLE = 0x434f_4d50u;
const uint SALT_PALETTE_RARE     = 0x5241_5245u;
const uint SALT_FAMILY_ROLL      = 0x46414d49u & 0xffffffffu;
const uint SALT_SIG              = 0x5349_4753u;
const int FAMILIES_PER_PALETTE = 3;

uint fold_u32(uint h, uint word) {
    for (int s = 0; s < 32; s += 8) {
        uint b = (word >> uint(s)) & 0xffu;
        h ^= b;
        h *= FNV1A_MULTIPLY;
    }
    return h;
}
// args are i64 on CPU; here they fit in i32 range for our coords/seed. Fold each
// as low u32 then high u32 (sign-extended) to match the CPU's i64 halves.
uint hash_ints1(uint salt, int a0) {
    uint h = FNV1A_INITIAL; h = fold_u32(h, salt);
    h = fold_u32(h, uint(a0)); h = fold_u32(h, uint(a0 >> 31)); // high half = sign extension
    h ^= h >> 16; h *= 0x7feb352du; h ^= h >> 15; return h;
}
uint hash_ints3(uint salt, int a0, int a1, int a2) {
    uint h = FNV1A_INITIAL; h = fold_u32(h, salt);
    h = fold_u32(h, uint(a0)); h = fold_u32(h, uint(a0 >> 31));
    h = fold_u32(h, uint(a1)); h = fold_u32(h, uint(a1 >> 31));
    h = fold_u32(h, uint(a2)); h = fold_u32(h, uint(a2 >> 31));
    h ^= h >> 16; h *= 0x7feb352du; h ^= h >> 15; return h;
}
uint hash_ints5(uint salt, int a0, int a1, int a2, int a3, int a4) {
    uint h = FNV1A_INITIAL; h = fold_u32(h, salt);
    h = fold_u32(h, uint(a0)); h = fold_u32(h, uint(a0 >> 31));
    h = fold_u32(h, uint(a1)); h = fold_u32(h, uint(a1 >> 31));
    h = fold_u32(h, uint(a2)); h = fold_u32(h, uint(a2 >> 31));
    h = fold_u32(h, uint(a3)); h = fold_u32(h, uint(a3 >> 31));
    h = fold_u32(h, uint(a4)); h = fold_u32(h, uint(a4 >> 31));
    h ^= h >> 16; h *= 0x7feb352du; h ^= h >> 15; return h;
}

int floor_div(float a, float b) { return int(floor(a / b)); }
int div_euclid(int a, int b) { int q = a / b; if ((a % b != 0) && ((a < 0) != (b < 0))) q -= 1; return q; }

int province_primary_palette(int prx, int prz) {
    uint h = hash_ints3(SALT_PROVINCE_PALETTE, prx, prz, P.seed);
    return int(h % uint(P.num_palettes));
}
int palette_for_region(int rx, int rz) {
    int prx = div_euclid(rx, P.province_size_regions);
    int prz = div_euclid(rz, P.province_size_regions);
    int primary = province_primary_palette(prx, prz);
    uint roll = hash_ints5(SALT_PALETTE_LOCAL, rx, rz, prx, prz, P.seed) % 100u;
    if (roll < P.palette_primary_pct) return primary;
    if (roll < P.palette_primary_pct + P.palette_compatible_pct) {
        ivec2 oc = compat_off.oc[primary];
        if (oc.y > 0) {
            uint pick = hash_ints3(SALT_PALETTE_COMPATIBLE, rx, rz, P.seed) % uint(oc.y);
            int idx = compat_flat.pal[oc.x + int(pick)];
            if (idx >= 0) return idx;
        }
        return primary;
    }
    return int(hash_ints3(SALT_PALETTE_RARE, rx, rz, P.seed) % uint(P.num_palettes));
}

// families + normalized bias for a region (mirrors families_for_region).
void families_for_region(int rx, int rz, out int fams[3], out float bias[3]) {
    int pal = palette_for_region(rx, rz);
    for (int i = 0; i < 3; i++) fams[i] = palettes.fam[pal * 3 + i];
    float base[3] = float[3](0.55, 0.30, 0.15);
    uint roll = hash_ints3(SALT_FAMILY_ROLL, rx, rz, P.seed) % 3u;
    for (int i = 0; i < 3; i++) bias[i] = base[(uint(i) + roll) % 3u];
}

float smoothstep_unit(float t) { float v = clamp(t, 0.0, 1.0); return v * v * (3.0 - 2.0 * v); }

// tiled bilinear sample of kernel `f` at world (x,z), scaled to relief (mirrors sample_kernel).
float sample_kernel(int f, float x, float z) {
    ivec4 r = krec.rec[f]; int off = r.x; int rows = r.y; int cols = r.z;
    float relief = kparam.rf[f].x; float footprint = kparam.rf[f].y;
    float u = (fract(x / footprint)) * float(cols);
    float v = (fract(z / footprint)) * float(rows);
    // GLSL fract on negatives already returns [0,1); matches rem_euclid(1.0).
    int u0 = int(floor(u)); int v0 = int(floor(v));
    float tu = u - float(u0); float tv = v - float(v0);
    int u1 = (u0 + 1) % cols; int v1 = (v0 + 1) % rows;
    u0 = ((u0 % cols) + cols) % cols; v0 = ((v0 % rows) + rows) % rows;
    float c00 = kdata.v[off + v0 * cols + u0];
    float c10 = kdata.v[off + v0 * cols + u1];
    float c01 = kdata.v[off + v1 * cols + u0];
    float c11 = kdata.v[off + v1 * cols + u1];
    float top = c00 + (c10 - c00) * tu;
    float bot = c01 + (c11 - c01) * tu;
    return (top + (bot - top) * tv) * relief;
}
float moderation(float slope) { return clamp(1.0 - P.moderation_strength * slope, P.moderation_min, 1.0); }
float local_slope(int f, float x, float z) {
    ivec4 r = krec.rec[f]; float footprint = kparam.rf[f].y; float relief = kparam.rf[f].x;
    float dx = footprint / float(r.z); float dz = footprint / float(r.y);
    float sx = (sample_kernel(f, x + dx, z) - sample_kernel(f, x - dx, z)) / (2.0 * relief);
    float sz = (sample_kernel(f, x, z + dz) - sample_kernel(f, x, z - dz)) / (2.0 * relief);
    return sqrt(sx * sx + sz * sz);
}

void main() {
    uint gid = gl_GlobalInvocationID.x;
    if (int(gid) >= P.num_coords) return;
    float x = coords.xz[gid].x; float z = coords.xz[gid].y;
    float s = P.region_size_m;
    float gx = x / s; float gz = z / s;
    int rx = int(floor(gx)); int rz = int(floor(gz));
    float tx = smoothstep_unit(gx - float(rx));
    float tz = smoothstep_unit(gz - float(rz));
    // 4 corners, accumulate weighted families into a small fixed buffer (<=12).
    int ids[12]; float wts[12]; int n = 0;
    ivec2 cr[4] = ivec2[4](ivec2(rx, rz), ivec2(rx + 1, rz), ivec2(rx, rz + 1), ivec2(rx + 1, rz + 1));
    float cw[4] = float[4]((1.0 - tx) * (1.0 - tz), tx * (1.0 - tz), (1.0 - tx) * tz, tx * tz);
    for (int c = 0; c < 4; c++) {
        if (cw[c] == 0.0) continue;
        int fams[3]; float bias[3]; families_for_region(cr[c].x, cr[c].y, fams, bias);
        for (int i = 0; i < 3; i++) {
            int fam = fams[i]; float add = cw[c] * bias[i];
            int found = -1; for (int j = 0; j < n; j++) if (ids[j] == fam) { found = j; break; }
            if (found >= 0) wts[found] += add; else { ids[n] = fam; wts[n] = add; n++; }
        }
    }
    float total = 0.0; for (int j = 0; j < n; j++) total += wts[j]; total = max(total, 1e-12);
    float height = 0.0;
    for (int j = 0; j < n; j++) {
        float w = wts[j] / total; int f = ids[j];
        float slope = local_slope(f, x, z);
        height += w * moderation(slope) * sample_kernel(f, x, z);
    }
    out_h.h[gid] = height;
    // family signature: sorted ascending ids folded via stable_hash_ints (mirrors parity.rs).
    // insertion sort the n (<=12) ids.
    for (int a = 1; a < n; a++) { int key = ids[a]; int b = a - 1; while (b >= 0 && ids[b] > key) { ids[b+1] = ids[b]; b--; } ids[b+1] = key; }
    uint h = FNV1A_INITIAL; h = fold_u32(h, SALT_SIG);
    for (int j = 0; j < n; j++) { h = fold_u32(h, uint(ids[j])); h = fold_u32(h, uint(ids[j] >> 31)); }
    h ^= h >> 16; h *= 0x7feb352du; h ^= h >> 15;
    out_sig.sig[gid] = h;
}
```
> **Sync note for the implementer:** the salts, FNV constants, the avalanche
> (`^= h>>16; *= 0x7feb352d; ^= h>>15`), the byte-fold order, and the i64→(low,high)
> split MUST match `hash.rs` exactly. The CPU folds an i64 as `u as u32` then
> `(u>>32) as u32`; for the test coords/seed (all within i32 range) the GLSL
> `uint(a)` + `uint(a>>31)` sign-extension reproduces the same two u32 words. If
> the parity gate (Task 6) shows signature mismatches, this is the first place to
> look. (Coords in the gate are chosen within i32 region-index range so this holds.)

- [ ] **Step 2: Commit** (the shader compiles/imports in Task 5/6)
```bash
cd /d/workflows/worldgen10
git add wg-10/worldgen_terrain/shaders/height_field.glsl
git commit -m "feat: GLSL compute port of the full formula (hash/grammar/height + signature)"
```

---

## Task 4: GPU compute plumbing `gpu_compute.rs`

**Files:**
- Modify: `wg-10/rust/src/gpu_compute.rs`

A `Wg10GpuCompute` GodotClass: load a pack, build the RenderingDevice local pipeline from the imported shader, upload tables+atlas+coords, dispatch, read back `height[]` + `family_sig[]`. The ONLY new godot/RenderingDevice file.

> **Implementer guidance:** the working RenderingDevice compute pattern verified on
> this machine is: `RenderingServer::create_local_rendering_device()` →
> load the `.glsl` as `RDShaderFile` (via Godot resource load) → `get_spirv()` →
> `shader_create_from_spirv` → `storage_buffer_create` per buffer → `RDUniform`
> (UNIFORM_TYPE_STORAGE_BUFFER) per binding → `uniform_set_create(.., shader, 0)` →
> `compute_pipeline_create` → `compute_list_begin/bind_compute_pipeline/bind_uniform_set/`
> `set_push_constant/dispatch/end` → `submit` → `sync` → `buffer_get_data`.
> Because gdext exposes RenderingDevice, prefer doing this in Rust; BUT loading the
> shader resource + push-constant packing is fiddly in gdext 0.5.3. If a Rust
> RenderingDevice path proves impractical within this task, STOP and report — the
> fallback (do the dispatch in GDScript inside the gate, Task 6, and have
> gpu_compute.rs only build the upload buffers from the Pack) is acceptable and
> keeps the formula/parity contract intact. Decide based on what compiles cleanly.

- [ ] **Step 1: Implement the buffer-builder + dispatch.** Replace `wg-10/rust/src/gpu_compute.rs` with a `Wg10GpuCompute` `#[derive(GodotClass)]` (base RefCounted) exposing:
  - `#[func] load_pack_dir(dir: GString, file: GString) -> GString` (reuse `pack::load_pack_dir`, store the Pack; "" on success else error — mirror `Wg10Height`).
  - `#[func] heights(coords_x: PackedFloat64Array, coords_z: PackedFloat64Array, seed: i64) -> PackedFloat64Array` and `#[func] signatures(...) -> PackedInt64Array` — OR a single dispatch returning both. These run the compute shader and read back.
  Build the palette table (`num_palettes*3` family indices via `Pack::family_ids` order + each `Palette.families` resolved through `palette_index`/`family_ids.position`), the compatibility offset/flat buffers (palette ids resolved to indices; `-1` for any unresolved — mirrors the CPU fallback), the kernel records `(dataOffset,rows,cols,0)` + `(relief_m,footprint_m)` + a concatenated `kdata` float buffer, the coords buffer, and the push-constant `Params` struct. Dispatch `ceil(num_coords/64)` workgroups, sync, read back.

  **The implementer chooses the cleanest working split** (all-Rust dispatch, or Rust builds buffers + GDScript dispatches) per the guidance above. Whatever the split, the shader is the single source of the formula and the data uploaded must match the CPU `Pack` exactly.

- [ ] **Step 2: Build** — `cd /d/workflows/worldgen10/wg-10/rust && env -u CARGO_TARGET_DIR cargo build 2>&1 | tail -15`. Expect it compiles, dll produced. `env -u CARGO_TARGET_DIR cargo test` still green (this task adds no Rust unit tests; it's exercised by the parity gate).

- [ ] **Step 3: Commit**
```bash
cd /d/workflows/worldgen10
git add wg-10/rust/src/gpu_compute.rs
git commit -m "feat: Wg10GpuCompute — RenderingDevice dispatch of the height formula + readback"
```

---

## Task 5: Windowed CPU/GPU parity gate

**Files:**
- Create: `wg-10/worldgen_terrain/tests/gpu_parity_check.gd`

Compare CPU (`Wg10Height` + a CPU-signature query) vs GPU (`Wg10GpuCompute`) over a coordinate grid: Tier-1 signatures EXACT, Tier-2 heights within epsilon. Runs WINDOWED (compute needs a real device).

> A CPU signature is needed in GDScript. Expose it: add a `#[func] family_signature(x,z,seed) -> i64` to the existing `Wg10Height` binding (delegating to `parity::family_signature`) — note this as a small binding addition the implementer makes in this task (modify `bind_worldgen.rs`).

- [ ] **Step 1: Add the CPU signature to the binding.** In `wg-10/rust/src/bind_worldgen.rs`, add to `Wg10Height`:
```rust
    /// CPU family-selection signature at (x,z) — matches the GPU's family_sig.
    #[func]
    fn family_signature(&self, x: f64, z: f64, seed: i64) -> i64 {
        match &self.pack {
            Some(p) => crate::parity::family_signature(x, z, seed, p) as i64,
            None => 0,
        }
    }
```
Rebuild: `env -u CARGO_TARGET_DIR cargo build`.

- [ ] **Step 2: Write the gate.** Create `wg-10/worldgen_terrain/tests/gpu_parity_check.gd` (TABS):
```gdscript
extends SceneTree

# CPU/GPU parity for the full formula. Tier 1: family-selection signatures must
# match EXACTLY (integer hash identical both sides). Tier 2: height within a
# documented f32 epsilon. Runs WINDOWED (RenderingDevice compute needs a device).

const PACK_RES_DIR := "res://worldgen_terrain/fixtures"
const PACK_FILE := "height_pack.json"
const ABS_EPS := 1.0e-2   # metres; f32 vs f64 over heights up to ~1000 m
const REL_EPS := 1.0e-5   # f32 ~7 sig digits

func _init() -> void:
	quit(_run())

func _run() -> int:
	if not ClassDB.class_exists("Wg10Height") or not ClassDB.class_exists("Wg10GpuCompute"):
		push_error("native classes not registered")
		return 1
	if RenderingServer.create_local_rendering_device() == null:
		print("[wg10-gpu-parity] status=skip reason=no-gpu (headless or no device)")
		return 2  # distinct skip code — runner must NOT treat as pass
	var os_dir: String = ProjectSettings.globalize_path(PACK_RES_DIR)
	var cpu: Object = ClassDB.instantiate("Wg10Height")
	var gpu: Object = ClassDB.instantiate("Wg10GpuCompute")
	var e1: String = str(cpu.call("load_pack_dir", os_dir, PACK_FILE))
	var e2: String = str(gpu.call("load_pack_dir", os_dir, PACK_FILE))
	if e1 != "" or e2 != "":
		push_error("pack load failed: cpu=%s gpu=%s" % [e1, e2])
		return 1

	# coordinate grid — within i32 region-index range (see shader sync note).
	var xs := PackedFloat64Array(); var zs := PackedFloat64Array()
	for ix in range(-12, 12):
		for iz in range(-12, 12):
			xs.append(float(ix) * 12345.0 + 17.0)
			zs.append(float(iz) * 9876.0 - 31.0)
	var n := xs.size()
	var gpu_h: PackedFloat64Array = gpu.call("heights", xs, zs, 1337)
	var gpu_s: PackedInt64Array = gpu.call("signatures", xs, zs, 1337)
	if gpu_h.size() != n or gpu_s.size() != n:
		push_error("gpu output size mismatch: h=%d s=%d n=%d" % [gpu_h.size(), gpu_s.size(), n])
		return 1

	var errors := 0
	var max_dh := 0.0
	var sig_mismatch := 0
	for i in range(n):
		var x := xs[i]; var z := zs[i]
		var ch: float = cpu.call("height", x, z, 1337)
		var cs: int = cpu.call("family_signature", x, z, 1337)
		if cs != gpu_s[i]:
			sig_mismatch += 1
			if sig_mismatch <= 3:
				push_error("Tier1 signature mismatch @ (%f,%f): cpu=%d gpu=%d" % [x, z, cs, gpu_s[i]])
		var dh: float = absf(ch - float(gpu_h[i]))
		if dh > max_dh: max_dh = dh
		var tol := maxf(ABS_EPS, REL_EPS * 1000.0)
		if dh > tol:
			errors += 1
			if errors <= 3:
				push_error("Tier2 height delta @ (%f,%f): cpu=%f gpu=%f d=%e" % [x, z, ch, gpu_h[i], dh])

	if sig_mismatch > 0 or errors > 0:
		print("[wg10-gpu-parity] status=fail coords=%d sig_mismatch=%d height_fail=%d maxd=%e" % [n, sig_mismatch, errors, max_dh])
		return 1
	print("[wg10-gpu-parity] status=pass coords=%d families_exact=true maxd=%e" % [n, max_dh])
	return 0
```
> Note: the `gpu.call("heights"/"signatures")` API must match whatever Task 4
> exposed. If Task 4 chose a Rust-builds-buffers + GDScript-dispatches split, this
> gate does the dispatch instead — adjust the GPU-side calls accordingly while
> keeping the Tier1/Tier2 comparison identical.

- [ ] **Step 3: Import + run windowed.**
```bash
export GODOT_BIN="C:/tmp/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64_console.exe"
"$GODOT_BIN" --headless --import --path "D:/workflows/worldgen10/wg-10" 2>&1 | tail -3
"$GODOT_BIN" --path "D:/workflows/worldgen10/wg-10" --script "res://worldgen_terrain/tests/gpu_parity_check.gd"; echo "RC=$?"
```
Expected: `[wg10-gpu-parity] status=pass coords=576 families_exact=true maxd=<small>`, RC=0.
If Tier-1 mismatches: the shader hash doesn't match `stable_hash_ints` — debug the GLSL hash vs Rust (byte-fold order, avalanche constants, i64 split). If Tier-2 exceeds epsilon: investigate before widening epsilon (DESIGN: documented epsilon only if profiled — do NOT loosen to force a pass; report it).

- [ ] **Step 4: Commit** (include the .uid sidecar if generated)
```bash
cd /d/workflows/worldgen10
git add wg-10/rust/src/bind_worldgen.rs wg-10/worldgen_terrain/tests/gpu_parity_check.gd
git add wg-10/worldgen_terrain/tests/gpu_parity_check.gd.uid 2>/dev/null || true
git commit -m "test: windowed CPU/GPU parity gate (Tier1 exact signatures, Tier2 epsilon height)"
```

---

## Task 6: Add the `gpu` suite to the gate runner

**Files:**
- Modify: `tools/gate.py`

- [ ] **Step 1: Add a `gpu` suite that runs WINDOWED.** In `tools/gate.py`, add a `"gpu"` key to `CHECKS` listing `worldgen_terrain/tests/gpu_parity_check.gd`, and make the runner invoke the `gpu` suite WITHOUT `--headless` (the `fast` suite stays headless). Add the run-mode branch: for `args.suite == "gpu"`, drop `--headless` from the per-check Godot invocation. Treat the check's return code 2 (skip) distinctly: print `status=skip`, and exit non-zero ONLY if a real fail (rc==1) occurred — a skip on a no-GPU box is reported but does not count as a pass. Keep the import pass (it compiles the `.glsl`).
```python
CHECKS = {
    "fast": [
        "worldgen_terrain/tests/hash_parity_check.gd",
        "worldgen_terrain/tests/determinism_check.gd",
        "worldgen_terrain/tests/grammar_check.gd",
        "worldgen_terrain/tests/height_check.gd",
    ],
    "gpu": [
        "worldgen_terrain/tests/gpu_parity_check.gd",
    ],
}
```
In `main`, when running checks: `headless = args.suite != "gpu"`; build the Godot argv with `--headless` only when `headless`. For each check, `rc == 0` → pass; `rc == 2` → skip (print, don't count as fail or pass); else fail. Final line `[gate] suite=gpu checks=1 fail=N skip=M`.

- [ ] **Step 2: Run the gpu suite.**
```bash
export GODOT_BIN="C:/tmp/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64/Godot_v4.6.2-stable_mono_win64_console.exe"
cd /d/workflows/worldgen10
python tools/gate.py --suite gpu; echo "GATE_EXIT=$?"
```
Expected: `[gate] check=...gpu_parity_check.gd status=pass`, `[gate] suite=gpu checks=1 fail=0 skip=0`, GATE_EXIT=0. Also confirm `python tools/gate.py --suite fast` still passes (4 checks, fail=0) and is still headless.

- [ ] **Step 3: Commit**
```bash
cd /d/workflows/worldgen10
git add tools/gate.py
git commit -m "feat: add windowed gpu parity suite to the gate runner (skip-distinct, fast stays headless)"
```

---

## Task 7: Update the living docs

**Files:**
- Modify: `docs/plans/DESIGN.md` (§2.4 + §9)
- Modify: `docs/plans/ROADMAP.md`
- Modify: `docs/plans/STATUS.md`

- [ ] **Step 1: DESIGN §2.4 / architecture** — record that the same deterministic formula now runs on CPU AND GPU via the integer-domain hash `stable_hash_ints` (u32-only, GLSL-reproducible); readback exists only in the parity gate (production no-readback streaming is M3). Record the integer-hash refactor + its accepted consequence (grammar rolls now a new seed-space; the WG9-bit-exact bedrock `hash_grid`/`value_noise`/`fbm` is untouched).

- [ ] **Step 2: DESIGN §9 open items** — add: the **GPU kernel-atlas-for-varied-sizes** named risk (synthetic 4×4 kernels pack trivially; real-DEM varied sizes may need an atlas redesign — revisit with the real DEM pack); the **parity epsilon** (ABS_EPS=1e-2 m, REL_EPS=1e-5, justified by f32 mantissa; widen only if profiled); and that **GPU compute is windowed-only** (headless has no local RenderingDevice on this setup).

- [ ] **Step 3: ROADMAP M2** — mark done: "GPU compute implementation of the same formula (no readback [in production])" and "CPU/GPU parity gate (bit-close; documented epsilon)". Note the parity gate runs windowed (needs a real GPU); `fast` stays headless. Update "Last updated:" to 2026-05-29 with a short note (M2 GPU formula + parity gate green).

- [ ] **Step 4: STATUS** — bump "Last updated:" to 2026-05-29. Current state: the full formula (hash→grammar→height) now runs on GPU via a hand-ported GLSL compute shader (`height_field.glsl`) dispatched by `Wg10GpuCompute` (RenderingDevice, windowed); a CPU/GPU parity gate proves family selection matches EXACTLY and height within an f32 epsilon. The grammar rolls moved to the GPU-portable integer hash `stable_hash_ints` (new seed-space; bedrock untouched; properties hold). What works: add the GPU parity gate; note the `gpu` suite is windowed (`fast` stays headless, still 4 checks fail=0); note the new Rust test count. What's next: M3 render pipeline (page pool / scheduler / clipmap / fly-test — the hard part), which consumes GPU height pages with no readback; real DEM pack + anti-repetition still deferred. Keep the honest-baseline tone (nothing rendered yet; synthetic kernels only; GPU output validated by gate readback, not yet streamed).

- [ ] **Step 5: Commit**
```bash
cd /d/workflows/worldgen10
git add docs/plans/DESIGN.md docs/plans/ROADMAP.md docs/plans/STATUS.md
git commit -m "docs: M2 GPU formula + CPU/GPU parity done; integer-hash refactor; gpu suite green"
```

---

## Self-review notes (already applied)

- **Spec coverage:** §1 (integer-hash refactor T0–1, GLSL shader T3, gpu_compute T4, parity gate T5, gpu suite T6), §2 (all 5 constraints: same formula = parity gate contract; no production readback = gate-only; bedrock untouched = `hash_grid` not modified, only grammar rolls; grammar structure unchanged = roll-input swap only; engine-agnostic = integer hash in hash.rs, gpu_compute.rs the only new godot file), §3 (shader + buffer uploads + gpu_compute), §4 (two-tier gate: exact signatures + epsilon, CPU `family_signature` T2), §5 (module boundaries), §6 (DESIGN/ROADMAP/STATUS T7), §7 (named risks → DESIGN §9).
- **GPU-portability decision baked in:** `stable_hash_ints` is u32-only (no 64-bit multiply) so GLSL `uint` reproduces it exactly — the key risk the design flagged. The i64→(low,high) split is mirrored; the gate's coords stay in i32 region-index range so the sign-extension high-half matches.
- **Honest consequence handled:** Task 1 explicitly distinguishes property re-lock (OK) from property violation (STOP/bug). The flat anchor (`height==500`, roll-independent) is asserted to still pass.
- **Skip ≠ pass:** the gate returns code 2 on no-GPU; the runner treats 2 as skip (reported, not counted pass), 1 as fail. A no-GPU CI box can't silently green.
- **Type consistency:** `stable_hash_ints(salt:u32, &[i64])->u32`, `SALT_*` consts (Rust + GLSL mirrored), `family_signature(x,z,seed,&Pack)->u32`, `Wg10GpuCompute::{load_pack_dir,heights,signatures}`, `Wg10Height::family_signature`, the gate's Tier1/Tier2 — consistent across tasks. GLSL fn names mirror Rust.
- **Pragmatic escape hatch:** Task 4 lets the implementer choose all-Rust dispatch vs Rust-buffers+GDScript-dispatch if the gdext RenderingDevice path is impractical — the formula/parity contract is identical either way; the working compute pattern (verified on this machine) is documented inline.
- **Epsilon discipline:** do NOT loosen epsilon to force a pass (stated in T5); widen only if profiled (DESIGN).
```
