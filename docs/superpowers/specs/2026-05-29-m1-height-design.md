# WorldGen10 — M1-Height Layer Design

**Date:** 2026-05-29
**Status:** Approved (brainstorming) — pending writing-plans
**Layer:** Kernel sampling + slope moderation + family composition → `height(x,z)`
**Builds on:** the M1-grammar layer (`grammar::family_weights`) + the pack v1 loader (`pack.rs`)
**Followed by:** real DEM pack wiring (separate follow-up), then M2 (GPU formula + parity)

---

## 0. Framing: this consumes the grammar, it does not change it

The M1-grammar layer emits a bounded, normalized, deterministic, seam-continuous
blend of terrain families: `family_weights(x,z,seed,pack) -> [(family_id, weight)]`
summing to 1. **This layer turns that blend into elevation.** The grammar is a
finished, frozen dependency — the height layer is its first consumer.

WG9 (`d:/workflows/worldgen9`) is consulted only to understand the problem
(how kernels become height, what "moderation" was) and as a loose sanity oracle.
It is **not** a parity contract. The DEM factory (`tools/dem_factory/dem_factory.py`)
lives in WG9 and writes per-kernel `.npy` artifacts (`height_m.npy`,
`normalized_height.npy`, `residual_m.npy`); those real kernels are **not** wired
in this plan (see §1 out-of-scope).

---

## 1. Scope

**In scope (this plan):** for any `(world_x, world_z, seed, pack)`, deterministically
produce a single `height(x,z): f64` by:
- loading terrain-family **kernels** (processed `.npy` height arrays) + per-family
  relief/footprint params into the in-memory `Pack` (extends the v1 loader);
- **tiled/wrapped sampling** of a kernel at a world coordinate (world→kernel-local,
  bilinear, wrap/mirror at edges) scaled to the family's relief;
- **slope-derived moderation** (amplitude-only damping of steep contributions —
  the carried-forward named risk, resolved here);
- **composition**: `height = Σ family_weight · moderated_contribution`,
  amplitude-bounded by the families' relief;
- a small `.npy` reader (pure Rust, in the pack layer);
- **synthetic golden kernels** (known values) as deterministic test ground truth;
- a thin `Wg10Height` Godot binding; property + exact-value gates.

**Out of scope (explicit, deferred — do NOT build now):**
- **Real DEM pack wiring** (loading the actual OpenTopo `.npy` kernels). Separate
  follow-up once the pipeline is proven against synthetic kernels.
- **Anti-repetition / variety tuning** (kernel rotation/offset/variant selection).
  Naive tiling repeats; that is acceptable for a pipeline-first cut and is tuned
  once the renderer (M3) can show it.
- **Inter-family blend aesthetics**, amplitude tuning to real-world ranges, and
  the **visual gate** — all wait for the renderer. You cannot judge terrain by
  looking at it yet (renderer is M3), so this plan does NOT tune blind.
- **GPU port** — M2.

**Ambition: pipeline-first.** Deliver a correct, deterministic, seam-continuous
`height(x,z)`; gate on **properties + exact synthetic values**, not visual
quality. Mirrors how M1-grammar was scoped (honest baseline, no renderer).

---

## 2. Interface constraints — NON-NEGOTIABLE (inherited from grammar §2)

1. **The grammar is untouched.** Height *consumes* `grammar::family_weights(...)`.
   It must not modify the blend math, the grammar module, or the weights output.
2. **The grammar still never reads kernel data.** Kernel data (`.npy` arrays,
   relief, footprint) enters the system **only** at the height layer. `grammar.rs`
   gains no kernel imports.
3. **Moderation modulates AMPLITUDE, not family IDENTITY.** The slope-moderation
   factor scales *how much* a kernel contributes; it never changes *which*
   families appear (that is the grammar's job). **If moderation ever needs to
   influence family selection, the weights/height seam has moved — STOP and
   re-cut the seam before continuing.** (This is the named risk, decided on
   purpose: moderation lives in the height layer.)
4. **GPU-shaped.** `height(x,z)` is pure float math over fixed-arity grammar
   output + bounded kernel lookups — no heap allocation in the hot path beyond
   the (already-loaded) kernel arrays, so M2 can port it to a compute shader.
5. **Engine-agnostic core.** `pack.rs` and `height.rs` import no `godot`. Only
   `bind_worldgen.rs` touches Godot.

---

## 3. Pack schema extension + kernel loading (`pack.rs`)

**Schema stays `worldgen10.terrain_pack.v1`** — additive, no version bump. The
kernel fields were always reserved ("present in the pack but not loaded yet" per
the grammar design §3); this plan loads them.

Each `families` entry MAY carry kernel data (shown JSON-shorthand):
```
"mountain": {
  "kernel":      "kernels/mountain.npy",   # path relative to the pack file's dir
  "relief_m":    1200.0,                    # peak-to-trough amplitude this family contributes
  "footprint_m": 8192.0                     # world distance the kernel tiles over
}
```
- **`.npy` reader** (pure Rust, ~50–80 lines, in `pack.rs`): targets the standard
  **NumPy format v1.0** that `np.save` produces by default — magic `\x93NUMPY`,
  version byte, little-endian header-length, an ASCII header dict with `descr`,
  `fortran_order`, `shape`. Supports **C-order** (`fortran_order: False`) `<f4`
  and `<f8` 2-D arrays (the DEM factory's output shape). Rejects anything else
  (pickled/object arrays, Fortran order, unsupported dtype, bad magic) with a
  descriptive error — never silently defaults.
- **`KernelData { w: usize, h: usize, samples: Vec<f32> }`** — row-major, f32
  (f64 input is narrowed on load; height precision at metre scale does not need
  f64 in the stamp). Stored in `Pack`.
- **`Pack` gains `kernels: BTreeMap<String, KernelData>`** keyed by family id,
  plus per-family `relief_m` / `footprint_m` (a small `FamilyKernel` record).
- **Validation (reject, never default):** referenced `.npy` exists + parses;
  `w > 0 && h > 0`; `relief_m > 0`; `footprint_m > 0`. A pack constant
  `moderation_min` (in `[0,1]`) and `moderation_strength` validated to range.
- **Opt-in per family — back-compat:** a family with `{}` (no kernel) loads fine,
  so the grammar's synthetic golden pack and **all 25 existing grammar tests stay
  green untouched**. Kernel loading happens only for families that declare a
  kernel. The **height layer** requires every family it touches to have kernel
  data and errors clearly if one is missing.

**Module boundary:** `pack.rs` owns ALL loading/validation (JSON + `.npy`) — it is
still the only file that parses bytes. It may approach the 600-line cap; if it
does, split the `.npy` reader into a `npy.rs` helper that `pack.rs` calls (decide
during planning). `grammar.rs`/`height.rs` read the in-memory `Pack`, never files.

---

## 4. The height core (`height.rs`, pure, no godot)

```
height(x, z, seed, &Pack) -> f64
```
1. **Weights (the seam, untouched):** `let w = grammar::family_weights(x, z, seed, pack);`
2. **Per family entry `(family_id, weight)`:**
   - `sample_kernel(family, x, z)`: map world `(x,z)` into kernel-local UV via
     `footprint_m` (`u = (x / footprint_m)`, fract → `[0,1)` → scale to `[0,w)`).
     **Edge policy: wrap (tile).** The kernel repeats every `footprint_m`; the
     bilinear neighbour past the last texel wraps to texel 0 (so the stamp tiles
     seamlessly and `height` stays C0 across footprint boundaries). **Bilinear**
     interpolate the four (wrapped) neighbours; scale the (already per-kernel
     normalized) sample by `relief_m`. (Naive single-kernel tiling visibly
     repeats — anti-repetition is the deferred §1 follow-up, not this plan.)
   - **moderation:** estimate local kernel slope by finite difference of the
     sample (central difference over one kernel texel in each axis); map slope to
     a factor in `[moderation_min, 1.0]` (steeper → smaller, strength controlled
     by the pack constant). **Amplitude only.**
   - contribution = `moderation · scaled_sample`.
3. **Compose:** `height = Σ weight · contribution`. Since `Σ weight = 1`, the
   result is a convex blend bounded by `max(relief_m)` of the present families.

**Determinism:** pure float math over loaded arrays; no RNG here — the grammar
already injected all seed-driven variety upstream. Same `(x,z,seed,pack)` →
identical height.

**Continuity (C0):** `family_weights` is C0-continuous across region AND province
seams (proven in M1-grammar; province delta measured ~1e-13). Bilinear kernel
sampling is C0. A sum of C0 functions with C0 weights is C0 ⇒ `height` is
seam-continuous. The gate asserts this directly.

**Module boundaries:** `pack.rs` = data + all parsing/validation; `grammar.rs` =
families (unchanged); `height.rs` = sampling + moderation + composition;
`bind_worldgen.rs` = the only Godot-facing file. Each one job, under ~600 lines.

---

## 5. Binding + gates + done

**Binding (`bind_worldgen.rs`):** add a thin `Wg10Height` class (the existing
`Wg10Hash`/`Wg10Grammar` stay as-is): `load_pack_json(json) -> error-or-""`,
`height(x, z, seed) -> f64`. No math.

**Gates (property + exact-value, NOT visual quality):**
- **Rust unit/property tests** (`height_tests.rs`) against **synthetic golden
  kernels** (hand-authored `.npy` with known values — e.g. a linear ramp and a
  centred gaussian bump — so tests assert **exact** height at chosen coords:
  true ground truth):
  - `.npy` reader parses the synthetic kernels (dims + a few exact cell values);
  - rejects malformed `.npy` (bad magic, Fortran order, unsupported dtype);
  - `height` is **deterministic** (same inputs → identical output);
  - **finite** (no NaN/inf) at varied coords including negatives and far-field;
  - **bounded** (|height| ≤ `max(relief_m)` over present families — a convex
    blend of per-family contributions each ≤ that family's relief — within
    tolerance);
  - **exact value** at a coord where the kernel sample + weight + moderation are
    hand-computable (the ground-truth anchor);
  - **seam-continuous** across a region boundary AND a province boundary
    (per-coord delta over a tiny step < tolerance — mirrors the grammar tests);
  - height-layer **rejects** a pack whose referenced family lacks kernel data.
- **Headless `height_check.gd`** in the fast suite: load the (synthetic-kernel)
  golden pack through `Wg10Height`, assert determinism, finiteness, and bounded
  range across a coord set + a grid, through the real native boundary.

**Definition of done:** `cargo test` green (existing 25 + new height tests);
`python tools/gate.py --suite fast` → fail=0 (now 4 checks); DESIGN/ROADMAP/STATUS
updated; each task committed separately. (DESIGN §7.3 perf+visual+manual
acceptance applies to the render pipeline, not this pure-math CPU layer.)

---

## 6. Test fixtures (synthetic kernels)

A new synthetic golden pack variant ships in-repo with **tiny** real `.npy`
kernels (e.g. 4×4 or 8×8) authored with known values:
- a **ramp** kernel (linear gradient — easy exact bilinear math);
- a **bump** kernel (single centred peak — exercises slope/moderation);
- written in the exact NumPy v1.0 C-order `<f4` format the reader targets, so the
  reader is tested against genuine `.npy` bytes, not a mock.
Plus an invalid `.npy` (wrong magic or Fortran order) for the reject test.

These are W10-authored toy ground truth. The realistic DEM kernel pack is the
deferred follow-up (it needs the curation work flagged in DESIGN §9 —
uncategorized families, NoData masking — which is out of scope here).

---

## 7. DESIGN.md updates this plan must make

- §3 (terrain packs): record that the kernel fields (`kernel`, `relief_m`,
  `footprint_m`) are now **loaded** (additively, still schema v1), via a pure-Rust
  `.npy` reader; the in-memory `Pack` now carries `KernelData`.
- §9 / architecture notes: record the **grammar↔kernel coupling decision RESOLVED**
  — moderation lives in the height layer (amplitude only); the grammar still never
  reads kernel data; the weights/height seam holds. Note the **real-DEM-pack
  wiring** + **anti-repetition tuning** as the named deferred follow-ups.
- ROADMAP M1: advance "Port the deterministic formula" — kernel → landform now
  DONE (synthetic-kernel pipeline); note real DEM pack is the next step. Keep the
  terrain-pack line `[~]` until the real pack lands.
