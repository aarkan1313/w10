# WorldGen10 — Status

What is actually true right now. Update this whenever reality changes. If a
manual fly contradicts a claim here, fix this file immediately. (Separating
"what passed a counter gate" from "what is actually accepted" is the whole
point — see DESIGN §7.3.)

Last updated: 2026-05-28 (M0 toolchain + M1 hash/noise/fbm foundation green)

---

## Current state

**Phase:** M0 toolchain green + M1 deterministic bedrock ported. No renderer yet.

- Godot 4.6 project at `wg-10/` (Forward+, D3D12, Jolt, .NET `wg10`).
- Native `wg10_terrain` Rust GDExtension **builds and loads in Godot 4.6**.
  `Wg10Hash` (RefCounted) is registered and callable headlessly, exposing
  `stable_hash_ints`, `hash_grid`, `value_noise`, `fbm`.
- Deterministic core ported from WG9 into `wg-10/rust/src/hash.rs` (pure, no
  `godot` imports): FNV-1a `stable_hash`, `hash_grid`, `value_noise`, `fbm`,
  `fade`, `smoothstep_unit`. **Bit-exact vs WG9 `hash_reference.json`** (the
  fixture is vendored at `wg-10/worldgen_terrain/fixtures/`).
- Headless gate runner: `python tools/gate.py --suite fast` → `fail=0`
  (hash parity + determinism through the loaded native lib).
- Three living docs (DESIGN, ROADMAP, STATUS). Architecture locked — see DESIGN.

## What works

- **Deterministic hash/noise bedrock, proven bit-exact** against WG9 — at both
  the Rust unit level (7 tests: stable_hash/hash_grid/value_noise/fbm exact
  cases + determinism + bounds + negative-axis floor continuity) and through
  the Godot native boundary (hash parity + determinism gates).
- Nothing rendered yet. (Honest baseline — the renderer is M3.)

## What's next

1. OpenTopo kernel methodology review is already done (DESIGN §9); next plan is
   **M1 continued**: region/province decisions + kernels + landform +
   terrain-pack format. Write it via the writing-plans skill before executing.
2. Then M2 (GPU formula + CPU/GPU parity), then M3 (render pipeline — the hard
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
