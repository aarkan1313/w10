# WG10 Connected Carve → Live GPU Path — Design

Date: 2026-06-04
Status: design in progress; owner approving section-by-section.
Supersedes the immediate target of the un-intercept ladder's Rung 1 conditioning
fix (conditioning turned out to be ~2 ms / trivial; the real gap is the carve).

## The finding that drove this (measured, not assumed)

The un-intercept ladder (spec `2026-06-04-wg10-unintercept-proving-ladder-design.md`)
proved the live runtime IS on GPU and produces real procedural mountain pages —
but Rung 1 measured them ~2× the relief of the accepted baked "mountain chunk
network" look, with a large gap. Investigation + profiling located the cause
precisely:

- The accepted chunk-network look comes from **connected pass-network carving**
  (`mountain_pass_network.carve_pass_network` → least-cost routing → ramp carve).
- That carve is **pure-Python Dijkstra** (`traverse_corridor._dijkstra_cost_field`,
  a `heapq` binary-heap shortest-path over a 4-connected grid).
- Profiled cost of a single-region bake: **~4 s wall, of which 99.9% is the carve**
  (`carve_pass_network` → `_routes`). Everything else — macro generation,
  `condition_world`, material hints — is collectively **~2 ms**.
- **The carve was NEVER ported to GPU or Rust.** It only ever ran offline, baked
  into `mountain_network_chunks.json`, which the runtime streams. When the project
  pivoted to the seam-safe live recipe, the recipe **dropped carving entirely** —
  so the live path is "GPU but carveless," which is exactly why it diverged from
  the chunk-network look.

**This is the divergence, named:** the single feature that makes the look special
was stranded in 4-second offline Python and never reached the live GPU path.

## North star (owner-set this session)

"The baked look, procedurally." The mountain-chunk-network *look* — broad coherent
massifs with connected valley/pass structure and tamed relief — reproduced by a
system that generates it on demand, on a 6–8 ms frame budget, extensible to a
large planned + unplanned feature set. Pillars: seamless, fast, infinite-procedural,
parity-safe. Owner directive for this work: **"full GPU for what's appropriate."**

## What "full GPU for what's appropriate" means here (the architecture call)

Classify each stage by GPU-appropriateness:

- **Macro / ridges / flow-drainage / conditioning** — pointwise or stencil,
  embarrassingly parallel → **already GPU** (the live recipe). Keep.
- **Conditioning specifically** — `tanh((h-p50)/(p95-p05)*2.10)` after a tiny
  gaussian; ~2 ms; pointwise once the percentiles are known → trivially GPU-able.
  NOT the bottleneck; a later cheap add.
- **Connected carve (Dijkstra least-cost routing)** — inherently sequential,
  data-dependent graph search. **NOT appropriate for naive GPU** (parallel
  shortest-path is research-grade and may not reproduce the exact routes the look
  depends on). It is ALSO not appropriate for 4-second Python. The appropriate
  move: **port Python → Rust** (a coarse-grid binary-heap Dijkstra is
  microseconds-to-low-ms in Rust), and **GPU the parallelizable sub-parts** (the
  slope/cost-field derivation is a parallel stencil; the path-walk itself stays
  CPU/Rust on the coarse grid). The subsequent **ramp carve** of the routes into
  the height field is a parallel write → GPU-appropriate.

So: macro/flow/condition stay GPU; the carve's *routing* goes to Rust; the carve's
*cost-field* and *ramp application* are GPU-appropriate and can move to GPU.

## Scope of THIS spec

Port the connected carve from offline Python to a fast runtime path, and measure
it, so the delivery architecture (how baked region-facts reach the live renderer)
can be sized from the real ported cost instead of a guess. Explicitly:

1. **Port the routing** (`_dijkstra_cost_field` + `_reconstruct_path` + `_step_cost`
   + `_routes`' coarse-grid WE/NS crossing seeding) to Rust, **bit-faithful** to the
   Python (same fixed neighbour order `(-1,0),(1,0),(0,1),(0,-1)`, same
   `(cost, idx)` heap tie-break, same coarse `zoom` downsample) so it reproduces the
   SAME routes → the same look. Parity-gated against the Python routes on a fixture.
2. **Measure** the Rust carve (route + ramp) cost for a representative region.
3. **Decide delivery from the measured number** (see "Delivery fork" below) — this
   spec does NOT pre-commit to heavy async infrastructure.

Out of scope here (follow-on): the full region-fact-cache backbone, multi-feature
tenancy, conditioning port (cheap, later), the un-intercept ladder Rungs 2–5.

## Components

- **`mountain_pass_network.py` (reference)** — the Python carve being ported.
  `_routes` (coarse WE+NS Dijkstra crossings) + `carve_ramp` (ramp the routes into
  height). Kept as the parity oracle; not deleted.
- **`traverse_corridor._dijkstra_cost_field` / `_reconstruct_path` / `_step_cost`
  (reference)** — the exact shortest-path + cost model to mirror.
- **NEW: `wg-10/rust/src/pass_network/` (Rust)** — the ported carve:
  - `dijkstra.rs` — binary-heap Dijkstra over a coarse 4-connected grid, the
    `_step_cost` model, `reconstruct_path`. Pure Rust, no Godot. Unit-testable.
  - `routes.rs` — `_routes` equivalent: coarse downsample + evenly-spaced WE/NS
    crossing seeds + map routes back to full-res index space.
  - `carve.rs` — `carve_ramp` equivalent: ramp the routes into the height field
    (parallel write; GPU-portable later).
  - `mod.rs` — `carve_pass_network(height, span_m, height_scale_m, params) ->
    { delta, routes, carved_frac }`, the public entry mirroring the Python.
- **NEW: parity fixture + gate** — a committed Python-generated routes/delta fixture
  for a known region; a Rust test asserting the Rust carve reproduces it
  (routes identical; delta within f32/f64 epsilon).
- **GPU-appropriate sub-parts (staged, after the Rust port is parity-proven):**
  the slope/cost-field stencil and the ramp-application pass move to GLSL compute,
  reusing the biome_page_compute scratch-buffer + scheduler pattern. The Dijkstra
  walk stays CPU/Rust.

## Delivery fork (decided AFTER measuring the Rust carve)

The measured Rust carve cost determines how a carved region reaches the renderer:

- **If carve ≈ 10–50 ms/region:** synchronous-off-frame bake riding the EXISTING
  page-pool LRU (model: `facts_api.bake_collision_region` — a deliberate off-frame
  one-shot). A region bakes in one brief off-frame step (or a few frames), pages
  sample it like `static_reference` does today. **No async job system needed.**
  PagePolicy (already generic) caches region-facts keyed by region; coarser-fact
  fallback (already in `schedule_policy.coarser_fallback`) covers the not-yet-baked
  case. This is the SMALL backbone.
- **If carve ≈ 100s of ms/region:** a genuine background/async bake (prefetch
  regions ahead of the camera, cold-start fallback to a coarse fact while the real
  bake completes, LRU-evict far regions). This is the LARGE backbone. The
  explorer confirmed NO async infra exists today → this would be net-new.

The spec deliberately defers this choice to a measured number. The Rust port is
valuable either way (it's the prerequisite for both).

## Data flow (target, once carve is on the live path)

```
camera position
  -> region grid quantize (grammar.region_of, already exists)
  -> region resident in fact-cache?
       yes -> pages sample the region's carved+conditioned height (cheap, hot-frame)
       no  -> bake region off-frame:
                macro (GPU) -> condition (GPU, ~free) -> carve routes (Rust)
                -> ramp carve (GPU) -> store region-fact, LRU
              meanwhile pages show coarser-fact fallback (never-black)
  -> page texture = sample(region-fact)  [the existing static_reference sampling path]
```

## Testing

- **Routing parity (hard):** Rust Dijkstra routes == Python routes on a committed
  fixture (identical index paths). This is the "same look" guarantee — if routes
  drift, the carved valleys move.
- **Carve delta parity:** Rust `delta` within epsilon of Python `delta` over the
  fixture region.
- **Cost measurement (non-vacuous):** time the Rust carve on a real region; record
  it; assert it actually carved (non-zero delta, routes connect edge-to-edge — the
  existing `network_crosses` check).
- **Determinism:** same region+seed → identical routes across runs (the Python is
  deterministic via fixed neighbour order + heap tie-break; Rust must match).
- **No regression:** existing `cargo test` (233) stays green; the new
  `pass_network` tests are additive.

## Error handling

- Carve over a region with no crossable route: the Python raises / returns no
  route for that crossing; the Rust mirrors (skip that crossing, carve the ones
  that connect) — never panic on the hot or bake path.
- Region bake failure (e.g. GPU step errors): the region stays un-baked; pages keep
  showing the coarser fallback; log, don't stall (the WG9 hot-path rule).

## Open questions / risks

- **Route parity exactness.** Floating-point cost accumulation order in the heap
  could differ Python↔Rust and flip a near-tie route (cf. the 576 parity residual
  memory — an f32 near-tie flipped a routing). Mitigation: port in f64 (the Python
  is f64), preserve the exact tie-break, and if a near-tie still flips, treat it the
  way the 576 residual was treated (both routes valid; record justification) rather
  than chasing bit-equality as parity-theater.
- **Coarse-grid size.** `pp.coarse_n` controls the routing grid; the Python already
  routes coarse then maps back. Keep the same `coarse_n` so cost AND routes match.
- **GPU ramp-carve staging.** Moving the ramp application to GLSL is a follow-on;
  the first milestone is Rust-CPU carve parity + cost. Don't block the measurement
  on the GPU port.
