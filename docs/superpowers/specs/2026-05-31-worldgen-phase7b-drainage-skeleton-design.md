# WorldGen10 - Phase 7B Drainage Skeleton Runtime Design

**Date:** 2026-05-31
**Milestone:** Phase 7B / pull-forward escape hatch.
**Status:** design-ready, implementation gated by Phase 5 owner visual acceptance.
**Parents:** `docs/plans/ROADMAP.md` Phase 5 and Phase 7B,
`docs/superpowers/specs/2026-05-31-worldgen-slice2a-geography-engine-design.md`.

---

## 1. Purpose

Phase 5 is trying to reach an 85%-class geography read before any Rust/GLSL port. If the accepted keeper
depends on the current 7B-lite skeleton-first result, WG10 needs a real runtime subsystem for routed coarse
structure. This spec defines that subsystem boundary before anyone tries to flatten the result back into local
noise.

The system's job is to provide deterministic, world-anchored skeleton facts that fine terrain pages can sample:
uplift, routed surface, discharge/accumulation proxy, tributary/channel axis, distance to channel, distance to
crest, and derived regime hints. Those facts organize height. Local noise and material detail remain secondary.

## 2. Non-Goals

- Do not implement this before a specific offline/owner-accepted Phase 5 keeper exists.
- Do not claim exact continental hydrology from bounded windows.
- Do not make the skeleton camera-relative or page-relative.
- Do not let render pages own or mutate drainage state.
- Do not bypass Facts/collision parity. If render sees a skeleton fact, CPU facts must be able to query the
  same fact.
- Do not port the current Python code line-for-line if the accepted keeper changes the fields or thresholds.

## 3. Current Evidence

Offline evidence exists, but it is not enough for runtime acceptance:

- `tools/dem_pack/geography_skeleton.py`: current 7B-lite image prototype used for owner review.
- `tools/dem_pack/geography_skeleton_windows.py`: non-visual window seam spike.
- `tools/dem_pack/analyze_geography_skeleton_windows.py`: emits
  `D:\tmp\wg10_geography_engine\geography_skeleton_window_seams.{csv,md}`.
- `tools/dem_pack/export_godot_rough_world_chunks.py`: bounded 3x3 rough-highlands chunk proof using
  independent world-coordinate skeleton windows, 25.6 km aprons, exact height seams, and exported corridor
  masks.
- `tools/dem_pack/fixtures/rough_highlands_keeper_v1.json`: frozen candidate keeper fixture with fixed sample
  points, skeleton facts, seam/variation summaries, and a reproducible contact-sheet hash.
- Tests: `tools/dem_pack/test_geography_skeleton_windows.py` proves fixed world-origin keys, deterministic
  window fields, apron-cropped core facts, and bounded adjacent-window seams for sampled facts.
- Owner visual seam verdict on the opened chunk review scene: seams look good visually.

What this proves: the windowed field model is plausible, can be gated, and can support a bounded seam-clean
review scene.

What this does not prove: owner-accepted terrain, exact global river networks, Rust parity, GPU parity,
performance, live streaming/cache behavior, travel pacing, or visible/collision parity.

## 4. Data Model

### 4.1 Window Identity

Each skeleton window is keyed by:

```text
seed
core_origin_x = floor(world_x / core_span_m) * core_span_m
core_origin_z = floor(world_z / core_span_m) * core_span_m
core_span_m
spacing_m
apron_m
algorithm_version
```

The key is independent of camera position, clipmap level, render page origin, load order, or cache state.

### 4.2 Window Extent

Each window computes an extended field:

```text
extended_span_m = core_span_m + 2 * apron_m
extended_origin = core_origin - apron_m
```

Only the core is authoritative for neighboring consumers. The apron exists to route and smooth facts near the
edge before cropping.

### 4.3 Stored / Queryable Facts

Minimum facts:

- `uplift`: broad structural high/low potential.
- `routed_surface`: coarse surface used for routing.
- `discharge`: routed accumulation proxy, normalized in a stable, non-local-max way.
- `tributary`: softer lower-order routed field.
- `channel_axis`: channel/corridor influence.
- `crest_dist`: saturated distance to uplift crest.
- `channel_dist`: saturated distance to channel.

Optional derived facts after acceptance:

- `range_weight`
- `foothill_weight`
- `basin_weight`
- `fan_weight`
- `badlands_weight`
- `drainage_density`

Distance facts are not unbounded truth. They are saturated inside the apron-valid band. Beyond that, the fact
means "far enough for this page's local shaping," not "exact nearest channel anywhere in the world."

## 5. Generation Order

For each coarse window:

1. Build world-coordinate uplift and basin/routed-surface fields on the extended grid.
2. Route multiple-flow accumulation over the extended grid.
3. Convert accumulation to stable discharge without per-window max normalization.
4. Build tributary/channel-axis fields from discharge.
5. Build crest/channel masks from thresholded world facts, not local winner-picking that can choose different
   representatives across a seam.
6. Build saturated distance facts.
7. Crop to the authoritative core.
8. Cache by deterministic key.

Fine terrain pages sample these facts by absolute world coordinate. They then apply the accepted height
composition: base uplift/fill, causal incision, regime shaping, local material/detail.

## 6. Seam Strategy

The seam contract is:

- scalar facts (`uplift`, `routed_surface`, `discharge`, `tributary`, `channel_axis`) must be continuous across
  neighboring core edges within explicit tolerances;
- saturated distance facts must agree or both saturate at the boundary;
- the fine page must bilerp facts in world coordinates, not index by local page coordinates;
- no page samples outside the authoritative core unless it explicitly owns the neighboring window key.

Current offline thresholds are intentionally conservative for the spike:

```text
uplift <= 0.001
routed_surface <= 0.001
discharge < 0.020
tributary < 0.035
channel_axis < 0.050
crest_dist_core_frac < 0.001
channel_dist_core_frac < 0.001
```

Runtime thresholds may change after the accepted keeper is frozen, but they must be named and gated.

## 7. Facts, Collision, And Render

There must be one authoritative skeleton query path. Render, CPU facts, and collision cannot each rederive a
different skeleton.

Required API shape before implementation:

```text
SkeletonFacts query_skeleton(seed, world_x, world_z, config)
```

Where `SkeletonFacts` includes the minimum fields above. Height composition then calls:

```text
height = compose_height(seed, world_x, world_z, skeleton_facts, local_material_config)
```

Render pages may batch this query on the GPU, but CPU facts must match sample-for-sample at agreed fixtures.
Collision cannot use a skeleton-free approximation if the render height uses routed structure.

## 8. Cache And Scheduling

Window generation may be expensive enough to cache. The cache contract is:

- key by window identity and algorithm version;
- deterministic output regardless of cache hit/miss or load order;
- bounded memory with LRU or explicit residency;
- no render-thread stalls beyond the accepted budget;
- no mutation of a completed window;
- no page-local edits to skeleton state.

If generation cost is too high for synchronous page production, reuse the existing async-ready terrain pipeline
shape, but the scheduler still requests deterministic windows by key.

## 9. Verification Gates

Python/offline gates:

- deterministic window generation;
- adjacent-window seam bounds;
- saturated distance facts do not expose incomplete-window context;
- no per-window max normalization that changes facts based on arbitrary crop contents;
- report writer for seam deltas across seeds/origins.

Rust CPU gates:

- Python-vs-Rust fixtures for window facts and sampled `SkeletonFacts`;
- deterministic cache keys;
- cross-window seam gate using Rust-generated windows;
- bounded finite output for all facts.

GPU gates:

- CPU-vs-GPU fact parity at sampled world points;
- CPU-vs-GPU composed height parity;
- no per-page ownership/mutation of skeleton state;
- no render path readback except explicit gates.

Render/facts gates:

- visible height equals CPU facts/collision for base terrain within the existing parity envelope;
- no-black/no-stall inherited from M3;
- perf gate with skeleton facts enabled;
- owner visual fly only after the above are green.

## 10. Slice Plan

Do not start these slices until Phase 5 has an accepted keeper that actually needs routed skeleton facts.

1. **Freeze accepted Python keeper.** Record exact fields, thresholds, and composition formula.
2. **Rust skeleton facts core.** Port deterministic window generation and sample queries. No Godot yet.
3. **Rust seam/parity gates.** Fixture Python-vs-Rust sampled facts and adjacent-window seams.
4. **Integrate CPU height composition.** Replace `height.rs` local-only generator path with accepted
   skeleton-aware composition.
5. **GPU skeleton sampling.** Mirror fact sampling/composition in GLSL or precomputed page buffers.
6. **Facts/collision parity.** Ensure `Wg10Facts` and render share skeleton-aware height.
7. **Perf and no-black gates.** Run hardened fast/gpu/m3 gates with skeleton enabled.
8. **Owner fly.** Only now ask whether the runtime look holds live.

## 11. Open Questions

- Does the accepted keeper need exact long rivers or only discharge-shaped coarse corridors?
- What `core_span_m`, `spacing_m`, and `apron_m` survive owner scale review?
- Are saturated distance facts enough, or does the final look need cross-window centerline identity?
- Does cache generation cost force async production immediately, or can the first port stay synchronous?
- Which facts belong in the public framework descriptor versus private height internals?

These are implementation questions, not blockers for the current offline design. The current hard blocker is
still visual acceptance of a specific Phase 5 keeper.
