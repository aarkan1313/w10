# WorldGen10 M3 — Slice 7: Page-Compute Resource Caching Design

**Date:** 2026-05-29
**Status:** Approved (brainstorming) — pending writing-plans
**Layer:** Render pipeline (M3) — slice 7: cache the per-page-invariant GPU compute resources so page production stops blowing the frame budget. Turns the p99 acceptance gate green.
**Builds on:** slice 6 (the p99 gate that diagnosed this), slice 2 (Wg10PagePool single RID owner), the slice-1/2 `compute_into_texture` producer.
**Followed by:** M3 close-out = the owner's manual fly (once p99 is green). Async/background page production becomes a follow-up ONLY if a residual per-page cost remains after caching (the measurement decides).

---

## 0. Framing — the reframe

The slice-6 p99 gate found a 90 ms spike on the ~4 boundary-crossing frames that compute a
page, while render-only frames are ≤ 2 ms. Reading `compute_into_texture` (`page_compute.rs`)
reframed the cause: the GPU dispatch is **fire-and-forget** (no submit/sync — the engine
auto-submits at draw), so the 90 ms is **NOT GPU execution blocking the CPU**. It is **redundant
CPU-side setup done per page**:

- `shader_compile_spirv_from_source` + `shader_create_from_spirv` — recompiling GLSL→SPIRV
  **every page** (tens of ms).
- `storage_buffer_create` × 6 — re-uploading all six pack buffers, **including the ~25 MB kernel
  atlas**, **every page**.
- `free_rid` × 8 — freeing all of it after each dispatch.

All of that is **identical for every page** — only the push-constant `origin_x/z` changes. So
the fix is **caching**, not threading: build the shader + pipeline + the six pack-buffer RIDs
**once**, reuse them for every page. Per-page work then shrinks to: create a small uniform set
(binding the cached buffers + this page's target image) + set the push constant + dispatch.
That eliminates the 90 ms; the p99 gate should go green with **no threading** and none of the
`RenderingDevice`-thread-safety risk async would carry.

This is "diagnose before architecting": the slice-6 finding *assumed* async; the code says
caching. We cache, **re-measure**, and only consider async if a genuinely-expensive residual
per-page cost remains.

---

## 1. Scope

**In scope (slice 7):**
- **`PageComputeContext`** — a cached bundle of GPU RIDs (compiled shader, compute pipeline,
  the six pack-buffer RIDs), built **once** from the pack + GLSL, owned by `Wg10PagePool`.
- **`Wg10PagePool` builds the context at `configure`** and **frees it at `free_all`** (alongside
  the page textures — the pool stays the single owner of ALL its GPU RIDs).
- **`compute_into_texture` refactored** to take the cached context: per call it creates only the
  uniform set (cached buffers + the page's target image), sets the push constant, dispatches,
  and frees only the per-page uniform set. No shader recompile, no buffer re-upload, no per-page
  buffer frees.
- **Strengthen `m3_accept_check`** with a compute-frame cost ceiling so a regression to per-page
  rebuild is caught (and the headline p99 assertion goes green).

**Out of scope:** async/background page production (a follow-up ONLY if caching leaves a residual
spike — the re-measurement decides; expected unnecessary). Any scheduler/view/rings change
(none — this is purely inside the pool's producer path).

---

## 2. `PageComputeContext` — the cached compute resources

A struct (in `page_compute.rs` or `page_pool.rs`) holding the per-page-invariant RIDs:
```
struct PageComputeContext {
    shader:    Rid,          // compiled once from the stripped GLSL
    pipeline:  Rid,          // compute_pipeline_create(shader), once
    palettes:  Rid,          // the 6 pack-buffer RIDs, uploaded once
    compat_off: Rid,
    compat_flat: Rid,
    krec:      Rid,
    kparam:    Rid,
    kdata:     Rid,          // the ~25 MB kernel atlas — uploaded ONCE, not per page
}
```
Built by a `build_page_compute_context(rd, pack, pack_buffers, glsl_source) -> Result<PageComputeContext, String>`:
strip the GLSL annotations, compile SPIRV (check compile error), create the shader + pipeline,
upload the six storage buffers from `PackBuffers` (the same bytes the pool already has). Returns
the RIDs to cache. A `free_page_compute_context(rd, ctx)` frees all 8 RIDs (shader cascade-frees
its uniform sets; free buffers + pipeline + shader). These mirror the existing build/free logic
in `compute_into_texture` — just hoisted to run once.

**Validation (pillar 4 — no shortcuts):** SPIRV compile error and invalid shader/pipeline RIDs
are surfaced as `Err` at context-build time (configure fails with a descriptive message), exactly
as the per-page path did — just once, at the right time.

---

## 3. `Wg10PagePool` — own + use the cached context

### 3.1 configure
After loading the pack + `PackBuffers` + GLSL (already done), call
`build_page_compute_context(...)` once and store `Option<PageComputeContext>` on the pool. If it
errors, `configure` returns the error string (the pool stays not-ready) — same contract as the
existing pack/glsl load failures.

### 3.2 acquire_page (the Allocate / AllocateEvicting arms)
Replace the `compute_into_texture(rd, pack, pack_buffers, target_rid, glsl, ox,oz,ws,ppx,sd)` call
with a cached-context dispatch: `compute_page_cached(rd, &ctx, target_rid, ox, oz, ws, ppx,
grammar_constants, seed, num_palettes)` — which:
1. builds the uniform set: binding 0 = `target_rid` (this page's image) + bindings 3–8 = the
   cached pack-buffer RIDs (NO re-upload — reuse `ctx`'s RIDs);
2. builds the push constant (origin/span/page_px — the only per-page-varying data);
3. `compute_list_begin` → bind pipeline (cached) + uniform set + push constant → dispatch →
   `compute_list_end` (fire-and-forget, as today — no submit/sync on the global RD);
4. frees ONLY the per-page uniform set (or lets it be transient — confirm Godot reclaims it; if
   not, free it explicitly after the compute list ends).
The target texture is still NOT freed here (pool owns it). The push-constant builder
(`build_page_push_constant`) and `make_image_uniform`/`make_storage_uniform` are reused as-is.

### 3.3 free_all
Free the cached `PageComputeContext` (via `free_page_compute_context`) in addition to the page
textures. The pool remains the single owner of all its GPU RIDs: page textures (the 3 documented
sites) AND the compute context (built at configure, freed at free_all). One configure builds, one
free_all frees.

### 3.4 The old per-page `compute_into_texture`
Either delete it (replaced by `compute_page_cached`) or keep it only if another caller exists
(none does — the pool was its only caller). Removing it prevents the slow path from being
reintroduced. `Wg10PageCompute` (the stateless producer class) is unaffected or simplified.

---

## 4. Gate: strengthen `m3_accept_check`

The decisive proof is the existing gate's **p99 < 6 ms** going green (the spike eliminated). Add
one contract guard using the diagnostic split the gate already computes:

- **Compute-frame ceiling:** assert `compute_ms_max < CEIL` (e.g. < 6 ms, or a tighter few-ms
  bound) — the time of the worst frame that actually computed a page. Before caching this was
  90 ms; after caching it should be ~render-cost + a cheap uniform-set + dispatch (low single-digit
  ms). A regression that reintroduces per-page recompile/re-upload blows `compute_ms_max` and fails
  the gate. This locks in the win cheaply, no new test infra, no exposed internals.
- Keep the existing assertions: p99 < 6 ms, no-black, never-stall (max < 33 ms). With caching,
  p99 and max should both drop dramatically (the 90 ms outliers gone).
- Keep printing `p99/mean/max` + the `compute_frames/compute_ms_max/renderonly_ms_max` diagnostic
  line (it's the permanent "where does the time go" readout).

The gate flips from RED to GREEN when this lands. The m3 suite then has 5 checks all passing.

---

## 5. Files

**Modify:**
- `wg-10/rust/src/page_compute.rs` — add `PageComputeContext` + `build_page_compute_context` +
  `free_page_compute_context` + `compute_page_cached` (the per-page dispatch using the cached
  context); the old per-page `compute_into_texture` is removed (or kept only if still referenced).
- `wg-10/rust/src/page_pool.rs` — own `Option<PageComputeContext>`; build it in `configure`, use
  `compute_page_cached` in the Allocate/AllocateEvicting arms, free it in `free_all`.
- `wg-10/worldgen_terrain/tests/m3_accept_check.gd` — add the `compute_ms_max` ceiling assertion.

**Unchanged:** the GLSL shader, the scheduler/view/rings/streamer (zero change — this is inside
the pool's producer path), the push-constant + uniform builders (reused).

**Soft cap:** files stay under the 600-line cap.

---

## 6. Definition of done

- `Wg10PagePool` builds the `PageComputeContext` once at configure, reuses it for every page
  (per-page work = uniform set + push constant + dispatch), frees it at free_all. Single-owner
  discipline intact.
- `m3_accept_check` passes WINDOWED: **p99 < 6 ms**, no-black, never-stall, AND
  `compute_ms_max < CEIL` (the per-page spike eliminated). The gate is GREEN. The printed numbers
  are recorded (the new p99/compute_ms_max).
- Regression-safe: the slice-1/2 render + pool gates still pass (the cached path produces the same
  page content — same shader, same buffers, same push constant; only the build timing changed).
  Cargo green; `fast`/`gpu` unchanged.
- STATUS + ROADMAP updated: caching done; **p99 gate GREEN**; M3 milestone has ONE box left — the
  owner's manual fly of `m3_review.tscn`. The async page-production item is marked
  "not needed (caching resolved the spike)" UNLESS the re-measurement shows a residual, in which
  case it's re-scoped as a follow-up with the new number.
- Each task committed separately.

---

## 7. Risks & mitigations

- **Caching doesn't fully close the spike (a residual per-page cost remains).** The
  re-measurement is the arbiter. If `compute_ms_max` is still > 6 ms after caching, the residual
  is the genuinely-expensive part (likely the dispatch itself or uniform_set_create at 25 MB-bound
  scale) — THEN async/amortization is the follow-up, now with a precise number. Caching is correct
  regardless (it removes provably-redundant work); it just may not be sufficient alone.
- **Cached uniform-set lifetime / per-page leak.** If Godot doesn't auto-reclaim the per-page
  uniform set, free it explicitly after `compute_list_end`. The gate's budget/never-stall +
  watching `resident`/RID counts catches a leak (frame time would climb over the run).
- **Shader cascade-free on free_all.** The existing per-page path relied on the shader free
  cascading its uniform sets. With a cached shader + per-page uniform sets, ensure each per-page
  uniform set is freed per page (not accumulated until free_all). State this in the code.
- **Single-owner discipline.** The compute context RIDs are pool-owned (built configure, freed
  free_all) — same rule as page textures. No other class creates/frees them. A second owner would
  be the scattered-RID smell slice 2 eliminated.
- **Content regression.** The cached path must produce byte-identical pages to the per-page path
  (same shader/buffers/push constant). The slice-1/2 render gates (distinct-color relief) +
  GPU-parity (M2, same formula) guard correctness; re-run the full m3 suite.
