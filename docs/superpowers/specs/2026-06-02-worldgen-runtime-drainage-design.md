# WorldGen10 — Runtime Drainage Delivery (on-demand full-res flow bake + fact cache)

**Date:** 2026-06-02
**Milestone:** Slice 4 drainage delivery. Resolves the §3.1 open item ("how is drainage DELIVERED at the 576²
production page?") that the biome-page work (Slice 4a/4b) deferred. Sibling to
`2026-06-02-worldgen-slice4-gpu-page-integration-design.md` (§3.1).
**Status:** design-ready; owner-decided direction (procedural-first, baking-fine). Owner approval of THIS spec
gates the plan.
**Parents:** the slice-4 spec §3.1 + the three probe findings in memory
`worldgen10-flow-convergence-production`, `worldgen10-coarse-drainage-refuted`, and the M3 page-pool streaming
model (`page_pool.rs` acquire/release/evict/LRU).

---

## 1. Purpose

The 11 biome recipes + compose run as GPU page pipelines, parity-exact to their f64 oracle (Slice 4a/4b,
hardware-proven). Each recipe's drainage (the MFD flow-accumulation channels) is computed by a GPU PULL
relaxation. At the REAL 576² production apron that relaxation needs **~192 iters = ~6.45 ms** to converge
(measured, `worldgen10-flow-convergence-production`) — too slow for the per-frame budget (3 ms half-budget).

Three deliveries were probed against the f64 oracle (`worldgen10-coarse-drainage-refuted`):
- **Live full-res per page:** correct, but 6.45 ms/page — too slow on the hot frame.
- **Coarse-relaxation + upsample / coarse-fact cache:** cheap, but a DIFFERENT drainage — valleys misplaced
  by up to **~800 m at 1000 m relief** (100–1000× the parity bar). REFUTED as parity-exact (the cheap path is
  proven-wrong, not just imprecise).
- **Operator-squaring (exact log-step) solver:** mathematically exact (~8 steps), but its GPU realization
  densifies connectivity (growing per-cell ancestor buffers; jump-flooding does NOT apply to a weighted-sum
  DAG) — heavyweight, high build-risk, unproven cheaper.

**Owner priority (2026-06-02):** procedural-infinite is the PRIMARY goal; BAKING IS FINE; fully-bounded /
non-procedural worlds are a future concern, not the current focus.

**Decision (pillars + that priority): compute the CORRECT full-res flow, but OFF the hot frame — bake the
drainage fact per REGION on-demand as the camera approaches, cache it, pages sample it cheaply, evict far
regions.** Rationale: once baking is acceptable and off-frame, the cost argument that made coarse attractive
vanishes — so bake the CORRECT (full-res, oracle-parity) drainage, not the wrong (coarse) one. On-demand bake
+ evict IS procedural-infinite (No Man's Sky-style, not a finite pre-bake). The 6.45 ms becomes a one-time
per-region cost amortized over every frame that region is visible; the hot frame only samples a cached texture.

## 2. Non-Goals

- **No coarse drainage.** The bake is FULL-RES (oracle-parity). Coarse upsampling is refuted (§1).
- **No finite pre-bake.** Regions bake on-demand as approached + evict when far — procedural-infinite preserved.
  (A finite eager bake for future bounded-world games is a trivial later mode reusing the same bake fn — out of
  scope now.)
- **No new streaming engine.** The drainage-fact cache RIDES the existing M3 page-pool pattern
  (acquire/release/evict/LRU, level+origin keyed, off-frame compute dispatch) — a sibling cache of the same
  shape, not parallel machinery.
- **No exact-solver build.** The operator-squaring solver is parked (documented in memory) as a future
  optimization IF per-region bake cost ever becomes the bottleneck; not built now.

## 3. Architecture

### 3.1 The drainage fact
- A **drainage fact** = the converged flow-discharge field for a REGION, at the resolution the recipe's flow
  pass consumes (full-res core for that region), stored as an R32F (or R16F if parity allows — gate decides)
  texture. It is the SAME discharge the per-page flow relaxation would produce at 192 iters — i.e. oracle-parity.
- Keyed by `(region_level, region_origin_x, region_origin_z)`. A region is COARSER/LARGER than a page (one
  region spans many pages) so one bake amortizes over many pages. Region size is a tuned constant (start: a
  region = the apron-padded extent that fully contains a page's flow-source neighborhood, so a page samples its
  drainage entirely from its own region's fact + at most its 8 neighbor regions — see §3.4 seam handling).

### 3.2 The bake (off-frame, on-demand)
- When the camera approaches a region not in the cache, enqueue a **drainage bake**: run the full-res flow
  (pre-blur 1.15 → 192-iter MFD relaxation → log1p-normalize → spread) on that region's surface, off the hot
  frame (a budgeted background dispatch, like a page compute but not blocking the frame). Write the discharge
  into a cached fact texture.
- Bake cadence: at most N region-bakes per frame (budgeted; tuned so the off-frame GPU time stays within the
  frame's spare budget). The flow relaxation is the same proven GLSL (`flow_accum_spike.glsl` body / the
  `flow_discharge` Scheduler path) — NO new flow math, just run at region scale off-frame.

### 3.3 Page sampling (hot frame, cheap)
- A page's flow pass is REPLACED on the runtime path: instead of running the 192-iter relaxation itself, the
  page SAMPLES its region's cached drainage fact (a texture fetch + the recipe's downstream smoothstep/mask
  math, which stays per-page and cheap). The expensive relaxation never runs on the hot frame.
- The recipe's per-biome flow params (width/power/the two-spread for temperate/rainforest, glacial's 1.85
  pre-blur) are baked INTO the fact per biome — OR the fact stores the raw discharge and the page applies the
  cheap per-biome spread/threshold. DECISION (gate-informed): store the discharge at the point the per-page
  work diverges (likely the raw log1p discharge, since the spreads are cheap gaussians the page can do); the
  4b biome schedules already separate `flow_discharge` (expensive) from the spread (cheap) — bake the former,
  keep the latter per-page.

### 3.4 Seam-safety of the cached fact
- The whole biome stack is seam-safe via the per-window apron + fixed-max normalization. The region bake MUST
  preserve this: a region's fact is computed on an APRON-PADDED region grid (same apron discipline), so pages
  near a region boundary sample a consistent fact. Adjacent regions overlap by the apron; a page straddling two
  regions blends/selects per the established seam convention. The fact is computed by the SAME seam-safe flow
  (apron + fixed-max log1p norm) → adjacent region facts agree at their overlap within the visually-seamless
  <1e-3 bar (the same bar the per-window flow holds).

### 3.5 Cache + eviction (rides M3)
- The fact cache mirrors `page_pool`: LRU, capacity-bounded, `acquire_fact(region_key)` (bake-on-miss,
  off-frame), `release_fact`, evict-far. A page's `acquire` ensures its region's fact is present (or enqueues
  the bake + uses a fallback until ready — §6).

## 4. Verification / parity bar

- **Bake parity:** the baked region fact == the per-page 192-iter relaxation discharge for that region, within
  the SAME normalized epsilon the biome parity gate uses (1e-4) — i.e. the runtime drainage IS the proven
  oracle look. Gate: bake a region, sample it where a fixture page sits, compare to that page's full-res flow
  discharge.
- **Seam:** adjacent region facts agree at their apron overlap < 1e-3 (visually-seamless bar).
- **Perf:** hot-frame page sampling adds only a texture fetch + the cheap spread (measure: the page's flow
  pass drops from ~6.45 ms relaxation to ~sub-ms sample). Off-frame bake budget: region bakes/frame × ~6.45 ms
  each stays within the frame's spare GPU time (measured; tuned).
- **Procedural-infinite:** fly a long traverse; regions bake on-demand + evict; no finite pre-bake; memory
  bounded by the cache capacity (did-real-work assertions: baked > 0, evicted > 0, no unbounded growth).

## 5. Boundary / honest risk

- **#1 risk — what a page shows BEFORE its region's fact is baked.** On-demand bake has latency; a newly-
  approached region's fact isn't instant. Options (gate-decided): (a) the page shows NO drainage (un-carved
  height) until the fact arrives, then pops in — visible but bounded; (b) a cheap coarse drainage as a
  TEMPORARY placeholder until the full-res fact bakes (the coarse error is acceptable transiently); (c)
  prefetch regions far enough ahead of the camera that the fact is always ready (the M3 lead-distance pattern).
  (c) is the M3-proven approach (never-black = coarsest-first prefetch); apply the same lead discipline.
- **Region size tuning** trades bake cost (bigger region = fewer bakes but each costs more) vs cache
  granularity. Start from the page-neighborhood-containing size; tune by the perf gate.
- **The bake is still 6.45 ms** — off-frame, but if too many regions bake at once (fast camera) it could spike
  the background budget. The bake cadence cap + lead prefetch bound this; the perf gate verifies under a
  ~1000 m/s traverse.
- **f16 vs f32 fact storage** — f16 halves cache memory but may break the 1e-4 parity; gate decides.

## 6. Out of scope / deferred

- The exact operator-squaring GPU solver (parked; future optimization if bake cost dominates).
- Eager finite pre-bake for bounded-world games (trivial later mode reusing the bake fn).
- Per-point facts/collision drainage (the legacy facts path stays as-is per the slice-4 spec §2/§7).
