# WorldGen10 — Status

What is actually true right now. Update this whenever reality changes. If a
manual fly contradicts a claim here, fix this file immediately. (Separating
"what passed a counter gate" from "what is actually accepted" is the whole
point — see DESIGN §7.3.)

Last updated: 2026-05-29 (M2 GPU formula + CPU/GPU parity gate green; integer hash; 67 Rust tests green)

---

## Current state

**Phase:** M0 toolchain green + M1 deterministic bedrock ported + grammar + height + M2 GPU formula + parity gate green. No renderer yet.

- Godot 4.6 project at `wg-10/` (Forward+, D3D12, Jolt, .NET `wg10`).
- Native `wg10_terrain` Rust GDExtension **builds and loads in Godot 4.6**.
  `Wg10Hash` (RefCounted) exposes `stable_hash_ints`, `hash_grid`, `value_noise`,
  `fbm`. `Wg10Grammar` (RefCounted) exposes `load_pack_json` + `family_ids` /
  `weight_values` (parallel packed arrays). `Wg10Height` (RefCounted) exposes
  `load_pack_dir` + `height` + `family_signature` queries.
- Deterministic core ported from WG9 into `wg-10/rust/src/hash.rs` (pure, no
  `godot` imports): FNV-1a `stable_hash`, `hash_grid`, `value_noise`, `fbm`,
  `fade`, `smoothstep_unit`. **Bit-exact vs WG9 `hash_reference.json`** (the
  fixture is vendored at `wg-10/worldgen_terrain/fixtures/`).
- **GPU-portable integer hash** `hash::stable_hash_ints(salt: u32, &[i64]) -> u32`
  (`hash.rs`): pure u32-wrapping FNV-1a fold, bit-identical on CPU and GLSL `uint`.
  Golden-value locked. Separate from the bedrock `hash_grid` (64-bit-multiply
  scheme, untouched).
- **Grammar rolls refactored** (`grammar.rs`): the 5 roll sites switched from
  string-join hashing to `stable_hash_ints` with distinct integer salts. New
  seed-space (accepted; WG10 grammar was never a WG9 parity contract). All grammar
  property tests pass unchanged; WG9-bit-exact bedrock untouched.
- **Terrain-pack v1 loader/validation** (`wg-10/rust/src/pack.rs`): schema
  `worldgen10.terrain_pack.v1`, validated on load, rejects malformed packs with
  descriptive errors, never silent defaults. `FAMILIES_PER_PALETTE = 3` fixed.
  `Pack` carries `family_kernels: BTreeMap<String, FamilyKernel>` via loaders
  `load_pack_with_base`/`load_pack_dir`.
- **Pure-Rust NumPy-v1.0 `.npy` reader** (`wg-10/rust/src/npy.rs`): parses
  C-order `<f4`/`<f8` 2-D arrays; rejects bad magic, version≠1, non-float dtype,
  Fortran order, non-2D shape, zero dims, overflowing shape. Descriptive errors,
  no silent defaults.
- **Grammar core** (`wg-10/rust/src/grammar.rs`): region/province locate (floor
  semantics), palette decision, `family_weights` corner blend — bounded, no heap
  allocation, normalized, deterministic, seam-continuous. Produces WEIGHTS ONLY —
  never reads kernel data.
- **Height core** (`wg-10/rust/src/height.rs`, pure, no godot): `sample_kernel`
  (tiled bilinear, scaled to `relief_m` — C0 across footprint seams; visible
  creases at footprint repeats are EXPECTED for naive tiling);
  `moderation` amplitude-only; `height(x,z,seed,&Pack)` = blend each
  grammar-selected family's moderated kernel sample by its weight.
  **SYNTHETIC KERNELS ONLY** (flat/ramp toy fixtures). NO real DEM pack, NO
  visual tuning, nothing rendered.
- **GPU compute shader** `height_field.glsl` (`wg-10/worldgen_terrain/shaders/`):
  hand-ported GLSL compute shader implementing hash→grammar→height end-to-end.
  Dispatched by `Wg10GpuCompute` (`gpu_compute.rs`), the only new
  RenderingDevice file; packs kernel atlas + coords as storage buffers, reads
  back height + family-signature buffers. **Runs WINDOWED** (headless
  RenderingDevice returns null on this D3D12 setup).
- **CPU/GPU parity gate** (`gpu_parity_check.gd`, `gpu` suite): verified on
  D3D12/RTX 5090 Laptop GPU over 576 coords. Tier 1: family-selection signatures
  EXACT (bit-exact). Tier 2: height within f32 epsilon (ABS_EPS=1e-2 m, observed
  max delta 7.67e-5 m — 130× headroom). `parity::family_signature` on CPU mirrors
  the GPU's signature; `Wg10Height::family_signature` exposes it.
- Gate runner: `python tools/gate.py --suite fast` → `[gate] suite=fast checks=4
  fail=0` (headless). `--suite gpu` → `[gate] suite=gpu checks=1 fail=0 skip=0`
  (windowed; returns SKIP code 2 on no-GPU/headless box).
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
  range, and variety across a grid (no flat-collapse).
- **Fast suite: 4 checks, fail=0** (headless).
- **GPU parity gate** (`gpu_parity_check.gd`, `gpu` suite): family selection EXACT
  + height within f32 epsilon on D3D12/RTX 5090; 1 check, fail=0. Runs windowed
  (not headless). Returns SKIP code 2 on no-GPU/headless box.
- **67 Rust unit/property tests green** (hash + npy + pack + grammar + height +
  parity). One exact-value anchor: all-flat pack yields `height == 500.0` at any
  coord (roll-independent).
- Nothing rendered yet. GPU output is validated by gate readback only — not yet
  streamed. (Honest baseline — the renderer is M3.)

## What's next

1. **M3 — Render pipeline (the hard part):** page pool + stream-ahead scheduler
   + clipmap rings + manual review scene + diagnostics overlay. This is where
   GPU height pages are consumed with NO readback in production (DESIGN §2.4).
   Acceptance: fly ~1000 m/s, no stalls, no black/holes, frame p99 < 6 ms,
   manually confirmed.
2. **Real DEM pack wiring** (OpenTopo kernels): only synthetic flat/ramp toy
   fixtures exist; still deferred.
3. **Anti-repetition / kernel variety tuning**: naive single-kernel tiling
   visibly creases at footprint seam boundaries (C0 not C1); deferred until the
   renderer can show it.

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
- **GPU kernel-atlas for varied sizes (DESIGN §9):** current atlas packs uniform
  4×4 synthetic kernels trivially; real OpenTopo DEM varied sizes may need an
  atlas redesign. Revisit with the real DEM pack.
- **GPU compute is windowed-only:** `Wg10GpuCompute` / `gpu` gate require a
  windowed run; headless returns null local RenderingDevice on this D3D12 setup.
  SKIP code 2 is returned on no-GPU/headless box — never miscounted as a pass.

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
