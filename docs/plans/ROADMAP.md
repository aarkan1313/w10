# WorldGen10 — Roadmap

Ordered milestones. Mark `[x]` only when the item meets the definition of done
in DESIGN.md §7.3 (perf gate + visual gate + manual confirmation, as
applicable). Update this file in place; do not create new plan docs.

Last updated: 2026-05-29 (grammar + terrain-pack v1 loader/validation + grammar
property gate green; 24 Rust tests green)

Legend: `[x]` done · `[~]` partially done (note inline) · `[ ]` not started.

---

## Milestone 0 — Project skeleton & rules

- [x] Godot 4.6 project created (`wg-10/`, Forward+, D3D12, Jolt, .NET `wg10`).
- [x] Three living docs created (DESIGN / ROADMAP / STATUS).
- [ ] Addon/folder layout decided (drop-in boundary): one terrain node + one
      config resource, narrow public API.
- [x] Native backend toolchain set up (**Rust GDExtension**, carried forward
      from WG9) and loads in Godot 4.6. (`wg10_terrain` crate builds; `Wg10Hash`
      registers and is callable headlessly — verified 2026-05-28.)
- [x] Test/gate runner skeleton (headless), so gates exist before features.
      (`tools/gate.py --suite fast`; renderer-backed suites come with M3.)

## Milestone 1 — Worldgen core (CPU) + parity foundation

- [~] Port the deterministic formula: hash → noise → region/province → kernel →
      landform, as pure engine-agnostic math. **DONE: hash → value-noise → fbm →
      region/province + family grammar** (`hash.rs`, `grammar.rs`), bit-exact vs
      WG9 fixture + grammar gates green. kernel → landform are next.
- [~] Terrain-pack format defined and loadable (first pack = DEM/OpenTopo
      kernels). **DONE: format v1 + loader + validation** (`pack.rs`); rejects
      malformed packs; grammar reads in-memory `Pack`. First *real* DEM pack
      (OpenTopo kernels) still comes with the height plan — only a synthetic
      golden pack exists now (kernels not loaded yet). Not `[x]`.
- [~] Parity fixtures (hash, noise, provider decisions, sample grids) committed
      **to git**. **DONE: hash/noise fixture** (`hash_reference.json` vendored);
      provider-decision + sample-grid fixtures come with later layers.
- [x] Determinism gate (same coord → same value across callers/runs).
      (`determinism_check.gd`, in the fast suite.)
- [x] Seam gate including **x=0 / z=0 axis-crossing** exact-zero edges.
      (Rust `value_noise_is_continuous_across_zero_axis` locks floor semantics.)

## Milestone 2 — GPU formula + parity

- [ ] GPU compute implementation of the same formula (no readback).
- [ ] CPU/GPU parity gate (bit-close; documented epsilon only if profiled).

## Milestone 3 — Render pipeline at speed (the hard part)

- [ ] `page_pool`: bounded GPU-resident height/normal page pool, single RID
      owner, LRU + protected keys.
- [ ] `page_scheduler`: velocity-aware stream-ahead, bounded computes/frame,
      coarser-page fallback (never black, never stall).
- [ ] `clipmap_rings`: fixed concentric rings, persistent meshes, recenter on
      move, shader displace + L↔L+1 morph.
- [ ] Manual review scene: WASD + Shift speed + mouse look + Space/C vertical,
      live fps/stats overlay, free-fly (+ optional ground-follow).
- [ ] Renderer-backed acceptance gate: no large black/missing component AND
      **renderer frame p99 < 6 ms**, in motion at ~1000 m/s.
- [ ] Tune finest-ring spacing + ring count against the review scene (config;
      not a locked constant — revisit when real assets exist).
- [ ] **MANUAL ACCEPTANCE:** owner flies it at full speed and confirms no
      stalls and no black/holes. (Gate green is necessary, not sufficient.)

## Milestone 4 — Facts API (authoritative, sparse)

- [ ] `get_height(x, z)` authoritative sparse query.
- [ ] `get_collision_field(area)` + Jolt `HeightMapShape3D` integration.
- [ ] Save/edit layer hook (composition over base height).

## Milestone 5 — Detail & masks (GPU, render-only)

- [ ] Detail/displacement layer (bounded, shader-only, edge-safe).
- [ ] Slope/curvature/debug + world-space masks.

## Milestone 6 — Biomes & textures (data-driven)

- [ ] Stable world-space biome/material masks driven by terrain-family rules.
- [ ] Texture/material packs (swappable, like terrain packs).

## Milestone 7 — Erosion & hydrology

- [ ] River/pass routing facts.
- [ ] Erosion/hydrology, integrated without breaking determinism/parity.

---

## Pre-work follow-up (not blocking M0/M1 doc work)

- [x] **Review OpenTopo kernel-extraction methodology** (done 2026-05-28,
      conclusion in DESIGN §9): methodology sound, cache sufficient. Pack-build
      follow-ups: mask NoData holes; improve family tagging (591/703
      uncategorized).
