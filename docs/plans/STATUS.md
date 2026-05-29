# WorldGen10 — Status

What is actually true right now. Update this whenever reality changes. If a
manual fly contradicts a claim here, fix this file immediately. (Separating
"what passed a counter gate" from "what is actually accepted" is the whole
point — see DESIGN §7.3.)

Last updated: 2026-05-29 (M1 height layer + .npy kernel loading + height property gate green)

---

## Current state

**Phase:** M0 toolchain green + M1 deterministic bedrock ported + grammar layer in + height layer in. No renderer yet.

- Godot 4.6 project at `wg-10/` (Forward+, D3D12, Jolt, .NET `wg10`).
- Native `wg10_terrain` Rust GDExtension **builds and loads in Godot 4.6**.
  `Wg10Hash` (RefCounted) exposes `stable_hash_ints`, `hash_grid`, `value_noise`,
  `fbm`. `Wg10Grammar` (RefCounted) exposes `load_pack_json` + `family_ids` /
  `weight_values` (parallel packed arrays). `Wg10Height` (RefCounted) exposes
  `load_pack_dir` + `height` query for headless checks.
- Deterministic core ported from WG9 into `wg-10/rust/src/hash.rs` (pure, no
  `godot` imports): FNV-1a `stable_hash`, `hash_grid`, `value_noise`, `fbm`,
  `fade`, `smoothstep_unit`. **Bit-exact vs WG9 `hash_reference.json`** (the
  fixture is vendored at `wg-10/worldgen_terrain/fixtures/`).
- **Terrain-pack v1 loader/validation** (`wg-10/rust/src/pack.rs`): schema
  `worldgen10.terrain_pack.v1`, validated on load, rejects malformed packs with
  descriptive errors, never silent defaults. `FAMILIES_PER_PALETTE = 3` fixed.
  `Pack` now carries `family_kernels: BTreeMap<String, FamilyKernel>` (loaded
  array + relief + footprint) via new loaders `load_pack_with_base`/`load_pack_dir`.
- **Pure-Rust NumPy-v1.0 `.npy` reader** (`wg-10/rust/src/npy.rs`): parses
  C-order `<f4`/`<f8` 2-D arrays; rejects bad magic, version≠1, non-float dtype,
  Fortran order, non-2D shape, zero dims, overflowing shape. Descriptive errors,
  no silent defaults.
- **Grammar core** (`wg-10/rust/src/grammar.rs`): region/province locate (floor
  semantics), palette decision, `family_weights` corner blend — bounded, no heap
  allocation, normalized, deterministic, seam-continuous. **Produces WEIGHTS ONLY
  — never reads kernel data.**
- **Height core** (`wg-10/rust/src/height.rs`, pure, no godot): `sample_kernel`
  (tiled bilinear, scaled to `relief_m` — C0 across footprint seams; visible
  creases at footprint repeats are EXPECTED for naive tiling);
  `moderation(slope,min,strength)=clamp(1-strength×slope,min,1)` amplitude-only;
  `local_slope` (central difference); `height(x,z,seed,&Pack)` = blend each
  grammar-selected family's moderated kernel sample by its weight. Single
  elevation out. Grammar↔kernel coupling **RESOLVED**: moderation is
  amplitude-only in the height layer; the grammar still never reads kernel data.
  **SYNTHETIC KERNELS ONLY** (flat/ramp toy fixtures). NO real DEM pack, NO
  visual tuning, nothing rendered.
- Headless gate runner: `python tools/gate.py --suite fast` → `[gate] suite=fast
  checks=4 fail=0` (hash_parity + determinism + grammar + height).
- Three living docs (DESIGN, ROADMAP, STATUS). Architecture locked — see DESIGN.

## What works

- **Deterministic hash/noise bedrock, proven bit-exact** against WG9 — at both
  the Rust unit level and through the Godot native boundary (hash parity +
  determinism gates).
- **Grammar property gate** (`grammar_check.gd`, fast suite): asserts sum=1,
  determinism, id/weight array parallelism, and family variety across a region
  grid (no single-palette collapse).
- **Height property gate** (`height_check.gd`, fast suite): asserts finite output,
  determinism across two independent calls, bounded output within pack relief
  range, and variety across a grid (no flat-collapse). Fast suite: **4 checks,
  fail=0**.
- **46 Rust unit/property tests green** (6 hash + 5 npy + 14 pack + 11 grammar +
  10 height). One exact-value ground-truth anchor: an all-flat pack yields
  `height == 500.0` at any coord (roll-independent).
- Nothing rendered yet. (Honest baseline — the renderer is M3.)

## What's next

1. **Real DEM pack wiring** (OpenTopo kernels): only synthetic flat/ramp toy
   fixtures exist; wiring and loading the actual OpenTopo kernel cache is the
   first height-layer follow-up.
2. **Anti-repetition / kernel variety tuning**: naive single-kernel tiling
   visibly creases at footprint seam boundaries (C0 not C1); deferred until the
   renderer can show it.
3. Then M2 (GPU formula + CPU/GPU parity), then M3 (render pipeline — the hard
   part).

## Decisions locked

- Native backend: **Rust GDExtension** (carried forward from WG9).
- Renderer acceptance budget: **frame p99 < 6 ms at ~1000 m/s**.
- Finest-ring spacing / ring count: **config-driven, value deliberately not
  locked** — tune against real assets later.

## Known risks / watch-items

- OpenTopo kernel methodology REVIEWED 2026-05-28 (see DESIGN §9): sound, cache
  is sufficient, no blocking issues. Two follow-ups for the pack build: mask
  NoData holes properly (only 2/703 accepted kernels affected), and improve
  family tagging (591/703 are `uncategorized`; some biomes thin).
- Grammar↔kernel coupling RESOLVED 2026-05-29 (see DESIGN §9): moderation is
  amplitude-only in the height layer; grammar never reads kernel data.
- Naive kernel tiling creases at footprint seam boundaries (C0, not C1) — expected
  behavior; deferred until the renderer can show it.
- Finest-ring spacing affects near-detail radius and interacts with future
  asset/texture scale; the owner flagged it needs review once assets exist.

## Build / run gotchas (learned 2026-05-28 wiring the toolchain)

- **`CARGO_TARGET_DIR` is set globally on this machine** (to
  `D:\cargo-target-kalshi`). It OVERRIDES `wg-10/rust/.cargo/config.toml`'s
  `target-dir`, so `cargo build`/`cargo test` send output to the global dir and
  the `.gdextension` can't find the dll. **Unset it per-invocation** when
  building/testing this crate: `$env:CARGO_TARGET_DIR=$null; cargo build`. The
  committed `.cargo/config.toml` makes the local layout correct on a clean
  machine (no global var) — it's only this machine that needs the unset.
- **`.gdextension` library path is `res://rust/target/debug/wg10_terrain.dll`** —
  resolved from the PROJECT ROOT, not relative to the `.gdextension` file.
  Godot `res://` cannot escape the project root with `..`.
- **GDExtension only loads after an editor import pass** writes
  `.godot/extension_list.cfg`. A bare `--headless --script` run on a clean
  checkout will NOT register `Wg10Hash`. `tools/gate.py` runs
  `--headless --import` first to handle this; do the same for any new check.
- **`--quit` without a main scene pops a blocking ALERT dialog** (even headless).
  Use `--script` (SceneTree) for checks, never `--quit`, in automated runs.
- Headless is fine for this pure-CPU layer; GPU work (M2+) won't run headless.

## Reference

- Predecessor: `d:/workflows/worldgen9` — read for knowledge (formulas,
  contracts, lessons); do not copy code. Its render layer is the cautionary
  tale (per-chunk synchronous GPU pages → 128 ms/chunk → black slabs + 5 fps at
  speed).
- Godot binary used for gates:
  `C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe`
