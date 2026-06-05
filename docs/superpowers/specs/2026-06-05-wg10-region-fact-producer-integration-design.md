# WG10 — Region-Fact Producer Integration (GPU-macro → CPU-carve → live producer)

**Date:** 2026-06-05
**Branch:** `slice4-gpu-page-integration`
**Status:** design (approved in brainstorm; pending spec review)
**Supersedes nothing; consumes:** `2026-06-05-wg10-bake-region-assembly-design.md` (the
assembled CPU pipeline this wires into the runtime).

## Goal

Put the carved "baked look" on screen in the live runtime. `bake_region.rs` already
assembles the whole offline look-pipeline in Rust (macro → carve → condition) at
end-to-end parity. The remaining work is the **producer integration**: bake regions
inside the pool's normal streaming loop and have pages sample the result. This closes
the un-intercept ladder's Rung-1 gap (the live recipe read ~2× the reference relief
precisely because it lacked carve + condition_world — both now exist in Rust).

## The forced architecture (from measurement)

All-CPU `bake_region` over a `region_size_m=32768` region (~16 pages) measured
**~961 ms (513px) / ~3319 ms (1025px)** — the CPU macro (`mountain_seamsafe`, the GPU
recipe's CPU twin) dominates; carve is ~19 ms, condition ~2 ms. Seconds/region is too
slow for synchronous OR a frame-amortized bake. Per the owner GPU/Rust-first principle:

> **GPU macro (region) → ONE off-frame readback → CPU carve (~19 ms) + condition (~2 ms)
> → RegionFactRuntime (mirror StaticHeightRuntime) → pages sample it.**

## Owner decisions locked in brainstorm

1. **Off-frame mechanism = the real async bake worker now** (not a throwaway load-time
   trigger — "don't waste time on a thing we'll sunset"). A background thread with its own
   `RenderingDevice` runs the bake; the pool checks "is region baked?" each tick, shows the
   existing coarse clipmap fallback for not-yet-baked regions (never-black), and swaps in
   the `RegionFactRuntime` when ready. This is the pillar-aligned, keep-forever mechanism.
2. **Multi-region from the start** — the cross-region condition seam is a first-class
   design problem here, not deferred.
3. **GPU-macro = reuse the proven biome page-compute at region scale** (`compute_biome_page_cached`,
   which already matches `mountain_seamsafe` to 1e-6), sized to a whole region + apron, then
   `texture_get_data`. Parity-correct by construction.
4. **Condition seam = settle by measurement** — the first gate measures the actual per-region
   percentile drift between adjacent real regions; reconcile-vs-coarse is decided with data
   (your `coarse-drainage-refuted` memory: coarse approximations gave a *different* look, so
   we do not assume a coarse percentile field is safe).

## Architecture

```
Camera moves → pool tick detects regions in prefetch radius
   ├─ region R baked?  → ProducerKind::RegionFact: page samples RegionFactRuntime(R)
   └─ not baked?       → enqueue R on the async bake worker;
                         pages render the existing coarse clipmap fallback (never-black)

Async bake worker (own thread, own local RenderingDevice):
   pop region key R →
     GPU macro over (region core + apron)   [reuse compute_biome_page_cached at region scale]
       → texture_get_data                    (off-frame readback — fine, it's a worker thread)
     → CPU carve_routes → carve_ramp_delta → raw+delta             [~21 ms, Rust]
     → condition_world(with seam-reconciled percentiles)
     → RegionFactRuntime { conditioned grid, bounds, region key, percentiles }
   → send back over channel; pool inserts into the region LRU on the main thread
```

The bake body is **exactly `bake_region.rs` with the macro step replaced by the GPU
readback**. The existing parity oracle (`bake_region_matches_python_seamsafe_pipeline`)
stays the correctness reference: feeding the *CPU* macro into the same carve→condition
tail must still reproduce the current bit-exact result.

## Components (dependency order)

Each unit has one purpose, a defined interface, and is independently testable.

### 1. `RegionFactRuntime` — `page_pool/region_fact.rs`
Near-copy of `StaticHeightRuntime`. Holds the conditioned+carved `Vec<f32>` grid +
region bounds (`origin_x/z_m`, `span_x/z_m`, `grid_n`); `sample(x,z)` bilinear;
`write_page_texture(rd, rid, origin, span, page_px)` per-page. Page-sampling math is
**identical to `static_reference/sampling.rs`** (texel-corner convention) so seam-exact
page sampling across abutting pages is inherited, not re-derived.
- Differs from static: grid comes from a bake (not JSON); carries the region key + the
  percentile set used (for the seam reconcile + diagnostics).
- No edge-fade-to-outside behavior is needed for the infinite case (regions tile the
  plane); the static reference's `outside_height`/`edge_fade` fields are dropped.
- Depends on: nothing at runtime (pure data + bilinear). **Tested** purely: known grid →
  expected bilinear samples; abutting-region boundary sample continuity.

### 2. GPU region-macro readback — `region_bake/gpu_macro.rs`
Reuses the `compute_biome_page_cached` seam (the proven seam-safe GPU macro) sized to
`(region_core_px + 2*apron_px)` instead of one 576² page, then `texture_get_data` →
`Vec<f64>` RAW (apron-cropped to the region core). Same call shape and RD lifecycle as
`biome_page_compute::generate_runtime_page_flow`. **Off-frame / worker only.**
- Interface: `fn gpu_macro_region(rd, ctx, region_origin, region_span, core_px, apron_px,
  feature_span_m, seed, flow_on) -> Result<Vec<f64>, String>` (the apron-cropped core).
- The owner's `gpu-readback-bare-pool` rule applies: the worker's RD is a bare local RD,
  no scene/camera/viewport; settle-then-read; `flow` off = `flow_max_level=0` (never
  `flow_iters=0`, which panics the scheduler).
- Depends on: existing biome page-compute context builders. **Tested** windowed (RTX 5090,
  editor closed): the GPU region-macro readback must match the CPU `mountain_seamsafe`
  over the same region grid to the established biome bar (≤ 1e-5).

### 3. Region bake pipeline — `region_bake/mod.rs`
`bake_region.rs` refactored to accept an **injected RAW field** (from the GPU readback)
instead of computing the macro on CPU. The carve → condition tail is unchanged:
`carve_routes` → `carve_ramp_delta` (on RAW) → `raw + delta` → `condition_world`.
- Interface: `fn bake_region_from_raw(raw, n, span_m, height_scale_m, seed, pass, traverse,
  ramp, percentiles: Option<RegionPercentiles>) -> BakeResult`.
- `bake_region.rs`'s current all-CPU entry is retained (it computes the CPU macro then calls
  `bake_region_from_raw`), so the existing end-to-end parity test continues to gate the tail.
- Depends on: `pass_network`, `condition_world`. **Tested** via the existing parity oracle
  (unchanged) + a new test that GPU-macro RAW → tail produces a field within the biome bar
  of the CPU-macro RAW → tail.

### 4. Async bake worker — `region_bake/worker.rs`
A `std::thread` owning its own `RenderingServer::create_local_rendering_device()`.
- **Input:** an mpsc channel of region keys `(rx, rz)` + bake params (seed, sizes,
  percentile fact).
- **Output:** an mpsc channel of finished `RegionFactRuntime` (boxed) keyed by region.
- The worker's RD is per-thread and **never** touches the pool's RD — no shared GPU state.
  The worker builds its biome page context once and reuses it across regions (context
  build is the expensive GLSL compile; amortize it).
- Lifecycle: created when the region-fact producer is configured; joined on pool drop.
- Depends on: components 2 + 3. **Tested:** channel round-trip — enqueue a region key,
  receive a `RegionFactRuntime` whose sampled height matches the synchronous bake of the
  same region (worker correctness, not just "it returned something").

### 5. Region LRU + producer arm — `page_pool/producer.rs` + region cache
New `ProducerKind::RegionFact`. A region cache keyed by `grammar::region_of` (floor-divide
by `region_size_m`), reusing the generic `PagePolicy` LRU (already region-key-shaped).
Dispatch in `dispatch_page_compute`:
- region baked + resident → `region_fact.write_page_texture(...)`.
- not baked → ensure it's enqueued on the worker (once), and fall through to the existing
  coarse clipmap fallback producer for this page so the screen is never black.
- The pool drains the worker's output channel each tick (main thread) and inserts finished
  regions into the LRU; eviction frees the oldest region grid.
- Depends on: components 1 + 4. **Tested:** dispatch routing — a baked region routes to
  `RegionFact`; an unbaked region routes to the fallback and enqueues exactly once.

### 6. Seam-reconciled conditioning — `condition_world` extension
Factor `condition_world(z, n)` into `condition_world_with_percentiles(z, n, p05, p50, p95)`;
the existing `condition_world` computes its own percentiles then calls it (so the current
parity test is unchanged). The worker passes **border-reconciled** percentiles per the
measurement gate's verdict (Section "Condition seam" below).
- Interface addition only; no formula change to the interior.
- **Tested:** `condition_world(z,n)` == `condition_world_with_percentiles(z,n, self-computed)`
  bit-for-bit (refactor safety), plus the seam gate below.

## Condition seam — settle by measurement (first gate)

`condition_world` normalizes by per-region percentiles, so two adjacent baked regions
condition their shared border differently → a potential seam in the conditioned height.

**Gate G-seam (do FIRST, before wiring the producer arm):** bake two adjacent real regions,
measure (a) the percentile drift `|p05_A − p05_B|`, etc., and (b) the actual conditioned-height
delta along the shared border column. Then:
- **Drift tiny** (border height delta under the parity bar, e.g. ≲ the 0.09 m condition
  residual already accepted): a simple deterministic border reconcile suffices — e.g. both
  regions use `percentiles(region) = quantize(blend of the region and its neighbor toward the
  shared edge)`, deterministic from the region keys so both sides agree by construction.
- **Drift large:** decide reconcile-vs-coarse with the data in hand. A shared coarse-global
  percentile field is the fallback **only if** a look-parity gate shows the coarse-conditioned
  field still matches the accepted look (guarding the `coarse-drainage-refuted` failure mode).

The reconcile rule chosen by G-seam feeds component 6. The interior of every region keeps its
true per-region (or reconciled) conditioning; only the thin border zone reconciles. **The macro
and carve are already seam-exact; only condition normalization varies by region** — so this gate
isolates the one remaining seam source.

## Data flow (one region, end to end)

1. Pool tick: camera enters prefetch radius of region `R=(rx,rz)`; `R` not in LRU.
2. Pool enqueues `R` (key + seed + region origin/span + percentile fact) on the worker; marks
   `R` "baking" (so it's not enqueued twice). Pages in `R` render the coarse fallback this frame.
3. Worker: GPU macro over `R`+apron → readback RAW (apron-cropped) → carve → condition (reconciled
   percentiles) → `RegionFactRuntime`. Sends it back.
4. Pool tick (later): drains the channel, inserts `RegionFactRuntime(R)` into the LRU, clears
   "baking". Pages in `R` now route to `ProducerKind::RegionFact` and sample the carved height.
5. Eviction: when the LRU is full, the oldest region grid is dropped (re-baked on re-entry).

## Error handling

- **Worker bake failure** (GPU error / readback size mismatch): the worker sends an error
  result for that region; the pool logs it, leaves the region on the coarse fallback, and does
  NOT retry in a tight loop (mark "failed", retry on a later re-entry). Never silently shows
  garbage.
- **RD unavailable** (headless): the worker construction fails loudly at configure time; the
  region-fact producer is simply not selected (the pool keeps its existing producer). Matches
  `gpu-compute-env` (compute works windowed only).
- **Channel disconnect on pool drop:** the worker observes the closed channel and exits; the pool
  joins it. No detached threads.
- **NaN in field:** `condition_world` already panics on NaN; the worker catches the panic boundary
  (`std::panic::catch_unwind`) and converts it to an error result rather than killing the thread.

## Testing strategy (fixture → Rust → parity-gate per piece)

| Piece | Gate | Where |
|---|---|---|
| RegionFactRuntime sampling | known grid → expected bilinear; abutting-boundary continuity | cargo lib (pure) |
| condition_world refactor | `with_percentiles(self-computed)` == original, bit-exact | cargo lib |
| bake_region_from_raw tail | existing `bake_region_matches_python_seamsafe_pipeline` still green | cargo lib |
| GPU region-macro readback | matches CPU `mountain_seamsafe` over region grid ≤ 1e-5 | windowed gate (RTX 5090) |
| GPU-RAW → tail vs CPU-RAW → tail | within biome bar | windowed gate |
| Worker round-trip | enqueue key → sampled height == synchronous bake | windowed gate |
| Producer dispatch routing | baked→RegionFact; unbaked→fallback + enqueue-once | cargo lib (mock) |
| **G-seam** | adjacent-region border drift + conditioned-height delta measured; reconcile verified seamless | windowed gate |
| On-screen Rung-1 | flown/read-back region height matches `bake_region` CPU oracle (carved look on screen) | windowed gate |

Windowed gates run with the editor closed (RTX 5090); cargo lib gates run isolated
(`CARGO_TARGET_DIR=D:/tmp/wg10_check_target`).

## Explicitly NOT in scope (YAGNI)

- No new async *job framework* — a single dedicated bake thread + two channels. Generalize only
  if a second consumer appears.
- No final terrain textures (still out of scope; the bar is geometry + the carved look + facts).
- No per-biome carve (the carve is mountain-only/world-layer; biome parity is already complete).
- Not porting the full-field `mountain.generate` branch (the live path is seam-safe).
- No prefetch-radius *tuning* in this slice — use the pool's existing radius; tune later if the
  worker can't keep ahead at speed (that's a measurement follow-on, not this design).

## Open risks to watch

- **Worker throughput at speed:** one thread baking ~tens-of-ms/region must stay ahead of the
  camera at ~1000 m/s. If it can't, the fix is a small thread pool (same channel shape) or a
  larger prefetch radius — both additive, not redesigns. Measure in the on-screen gate.
- **Context reuse vs RD state:** building the biome page context once and reusing it across regions
  must not accumulate RD state that corrupts later bakes. The worker round-trip gate bakes ≥ 2
  regions back-to-back to catch this.
- **Region apron size:** the macro apron needed for seam-safety at region scale must be confirmed
  (the page-scale apron may differ); the GPU-macro readback gate uses the same apron the CPU
  oracle uses.
- **Reconcile-vs-neighbor ordering:** if G-seam picks a reconcile rule where region `A`'s border
  percentiles depend on neighbor `B`'s *baked* field, `B` may not exist when `A` bakes. The design
  AVOIDS this by requiring the reconcile rule to be a **deterministic function of region keys +
  each region's own field** (e.g. quantize each region's percentiles to a shared grid, or derive
  border percentiles from a cheap deterministic function both sides compute identically) — so no
  region bake blocks on another. If G-seam can only achieve seamlessness with cross-region field
  data, that's a finding to surface before building component 6, not to paper over.
```
