# WorldGen10 — Status

What is actually true right now. Update this whenever reality changes. If a
manual fly contradicts a claim here, fix this file immediately. (Separating
"what passed a counter gate" from "what is actually accepted" is the whole
point — see DESIGN §7.3.)

Last updated: 2026-05-29 (M1 grammar + terrain-pack v1 loader/validation + grammar gate green)

---

## Current state

**Phase:** M0 toolchain green + M1 deterministic bedrock ported + grammar layer in. No renderer yet.

- Godot 4.6 project at `wg-10/` (Forward+, D3D12, Jolt, .NET `wg10`).
- Native `wg10_terrain` Rust GDExtension **builds and loads in Godot 4.6**.
  `Wg10Hash` (RefCounted) exposes `stable_hash_ints`, `hash_grid`, `value_noise`,
  `fbm`. `Wg10Grammar` (RefCounted) exposes `load_pack_json` + `family_ids` /
  `weight_values` (parallel packed arrays) for headless checks.
- Deterministic core ported from WG9 into `wg-10/rust/src/hash.rs` (pure, no
  `godot` imports): FNV-1a `stable_hash`, `hash_grid`, `value_noise`, `fbm`,
  `fade`, `smoothstep_unit`. **Bit-exact vs WG9 `hash_reference.json`** (the
  fixture is vendored at `wg-10/worldgen_terrain/fixtures/`).
- **Terrain-pack v1 loader/validation** (`wg-10/rust/src/pack.rs`): schema
  `worldgen10.terrain_pack.v1`, validated on load, rejects malformed packs with
  descriptive errors, never silent defaults. `FAMILIES_PER_PALETTE = 3` fixed.
- **Grammar core** (`wg-10/rust/src/grammar.rs`): region/province locate (floor
  semantics), palette decision (primary/compatible/rare roll), `family_weights`
  corner blend — bounded (MAX 12 inputs: 4 corners × 3 families), no heap
  allocation, normalized (sum exactly 1.0), deterministic, seam-continuous.
  **Produces WEIGHTS ONLY — no height.**
- Headless gate runner: `python tools/gate.py --suite fast` → `[gate] suite=fast
  checks=3 fail=0` (hash_parity + determinism + grammar).
- Three living docs (DESIGN, ROADMAP, STATUS). Architecture locked — see DESIGN.

## What works

- **Deterministic hash/noise bedrock, proven bit-exact** against WG9 — at both
  the Rust unit level and through the Godot native boundary (hash parity +
  determinism gates).
- **Grammar property gate** (`grammar_check.gd`, fast suite): asserts sum=1,
  determinism, id/weight array parallelism, and family variety across a region
  grid (no single-palette collapse). Fast suite: **3 checks, fail=0**.
- **24 Rust unit/property tests green** (7 hash + 7 pack + 10 grammar).
- Nothing rendered yet. (Honest baseline — the renderer is M3.)

## What's next

1. **Kernel sampling + landform composition → `height(x,z)`** (next plan).
   Grammar outputs weights-only; the height layer applies them. Keep the
   grammar↔kernel coupling decision in mind: the grammar must not read kernel
   data — if it ever needs to, the weights/height seam has moved (stop and
   re-cut before continuing).
2. First real DEM pack (OpenTopo kernels) loads with the height plan (only a
   synthetic golden pack exists now).
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
