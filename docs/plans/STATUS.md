# WorldGen10 — Status

What is actually true right now. Update this whenever reality changes. If a
manual fly contradicts a claim here, fix this file immediately. (Separating
"what passed a counter gate" from "what is actually accepted" is the whole
point — see DESIGN §7.3.)

Last updated: 2026-05-28

---

## Current state

**Phase:** Design / planning. No terrain code written yet.

- Godot 4.6 project exists at `wg-10/` (Forward+, D3D12, Jolt, .NET `wg10`).
  It is an empty default project — no terrain systems yet.
- Three living docs written (DESIGN, ROADMAP, STATUS).
- Architecture locked: unified GPU clipmap (stream-ahead, never-block,
  coarser-fallback) + sparse CPU-authoritative facts + data-driven terrain
  packs. See DESIGN.md.

## What works

- Nothing rendered yet. (Honest baseline.)

## What's next

1. Finish Milestone 0 skeleton (addon layout, native toolchain, gate runner).
2. OpenTopo kernel methodology review (confirms the starter pack is sound
   before M1 builds on it).
3. Milestone 1: worldgen core + parity/seam gates.

## Decisions locked

- Native backend: **Rust GDExtension** (carried forward from WG9).
- Renderer acceptance budget: **frame p99 < 6 ms at ~1000 m/s**.
- Finest-ring spacing / ring count: **config-driven, value deliberately not
  locked** — tune against real assets later.

## Known risks / watch-items

- The OpenTopo processed kernel cache (~80 GB raw + processed, from WG9) is the
  intended first terrain pack but its extraction methodology has not yet been
  reviewed. Do not treat the DEM pack as trusted until that review passes.
- Finest-ring spacing affects near-detail radius and interacts with future
  asset/texture scale; the owner flagged it needs review once assets exist.

## Reference

- Predecessor: `d:/workflows/worldgen9` — read for knowledge (formulas,
  contracts, lessons); do not copy code. Its render layer is the cautionary
  tale (per-chunk synchronous GPU pages → 128 ms/chunk → black slabs + 5 fps at
  speed).
- Godot binary used for gates:
  `C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe`
