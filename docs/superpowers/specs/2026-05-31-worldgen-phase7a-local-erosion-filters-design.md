# WorldGen10 - Phase 7A Local Erosion Filter Design

**Date:** 2026-05-31
**Milestone:** Phase 7A - local drainage-shaped filters.
**Status:** design-ready, implementation gated by Phase 5/6 acceptance and analytic gradient feasibility.
**Parents:** `docs/plans/ROADMAP.md` Phase 7A,
`docs/superpowers/specs/2026-05-31-worldgen-phase6-surfacing-design.md`.

---

## 1. Purpose

Phase 7A adds local erosion-shaped texture to an already accepted height field. Its job is to make slopes,
gullies, and small drainage texture more believable. It is not true hydrology and must not be sold as global
river connectivity.

The distinction matters:

- Phase 7A: local filters over height/slope/gradient. Good for gullies, damping, talus-like texture, and
  erosional roughness.
- Phase 7B: world-anchored routed drainage skeleton. Required for connected discharge facts.

## 2. Preconditions

Do not implement until:

- Phase 5 has an owner-accepted live height core;
- Phase 6 has a shared `SurfaceDescriptor` or equivalent descriptor seam;
- analytic value/gradient noise feasibility is proven across Python/Rust/GLSL;
- CPU/GPU operation order is specified enough for parity fixtures;
- perf budget has room for the extra filter work.

## 3. Non-Goals

- No globally connected rivers.
- No upstream-area/discharge claims.
- No dead-ending gullies hidden as "hydrology."
- No runtime neural/CNN/stencil page path without apron, seam, parity, and collision plans.
- No owner acceptance by metrics alone.

## 4. Candidate Operators

Local operators may include:

- gradient damping on steep convex slopes;
- slope-aligned gully masks from analytic gradient flow direction;
- ridged/detail noise modulated by slope and curvature;
- talus/sediment smoothing near slope breaks;
- badlands-style dense incision texture when regime/substrate says it is plausible.

Every operator must be deterministic, bounded, edge-safe, and controlled by config.

## 5. Descriptor Inputs

Phase 7A reads the shared descriptor:

```text
height_m
normal
slope
curvature
height_band
biome/regime weights
optional skeleton facts from Phase 7B
```

It must not independently rederive a different slope or curvature convention.

## 6. Gates

Non-visual gates:

- Python reference operator deterministic and bounded;
- edge-safe across adjacent pages;
- CPU-vs-GPU parity for the filtered height/detail term;
- no base height drift when filter disabled;
- config validation for all operator weights/frequencies;
- perf gate with filters enabled.

Visual gates:

- owner A/B fly with filter off/on;
- no shimmer, seams, repeated gully motifs, or directional raster scars;
- look improves accepted terrain instead of changing the terrain class.

## 7. Slice Plan

1. **Analytic gradient feasibility.** Prove a single gradient convention in Python/Rust/GLSL.
2. **Python operator sandbox.** Try local filters over accepted height screenshots/data; render A/B sheets.
3. **Owner reject/keep.** Owner decides if local filters help before runtime work.
4. **Rust scalar operator.** Deterministic CPU implementation with fixture tests.
5. **GPU operator.** Mirror in page generation or shader detail path with parity gate.
6. **Perf and owner fly.** Only after invariants are green.

## 8. Boundary

Phase 7A is useful polish after terrain reads correctly. If the owner still misses connected river logic,
Phase 7A is the wrong tool; use Phase 7B instead.
