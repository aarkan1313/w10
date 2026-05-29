# WorldGen10 — Roadmap

Ordered milestones. Mark `[x]` only when the item meets the definition of done
in DESIGN.md §7.3 (perf gate + visual gate + manual confirmation, as
applicable). Update this file in place; do not create new plan docs.

Last updated: 2026-05-28

---

## Milestone 0 — Project skeleton & rules

- [x] Godot 4.6 project created (`wg-10/`, Forward+, D3D12, Jolt, .NET `wg10`).
- [x] Three living docs created (DESIGN / ROADMAP / STATUS).
- [ ] Addon/folder layout decided (drop-in boundary): one terrain node + one
      config resource, narrow public API.
- [ ] Native backend toolchain set up (**Rust GDExtension**, carried forward
      from WG9) and loads in Godot 4.6.
- [ ] Test/gate runner skeleton (headless + renderer-backed), so gates exist
      before features.

## Milestone 1 — Worldgen core (CPU) + parity foundation

- [ ] Port the deterministic formula: hash → noise → region/province → kernel →
      landform, as pure engine-agnostic math.
- [ ] Terrain-pack format defined and loadable (first pack = DEM/OpenTopo
      kernels). Core consumes the pack; no source assumptions baked in.
- [ ] Parity fixtures (hash, noise, provider decisions, sample grids) committed
      **to git**.
- [ ] Determinism gate (same coord → same value across callers/runs).
- [ ] Seam gate including **x=0 / z=0 axis-crossing** exact-zero edges.

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

- [ ] **Review OpenTopo kernel-extraction methodology** (see DESIGN §9): read
      WG9 `factory/`+`tools/` pipeline, eyeball sample kernel outputs, confirm
      the processed cache has everything the new generator needs. Conclusion
      recorded as a DESIGN.md update, not a new doc.
