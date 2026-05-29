# WorldGen10 M3 — Slice 2: Page Pool Design

**Date:** 2026-05-29
**Status:** Approved (brainstorming) — pending writing-plans
**Layer:** Render pipeline (M3) — slice 2: bounded GPU-resident page pool (single RID owner, LRU, protected)
**Builds on:** M3 slice 1 (Wg10PageCompute → Texture2DRD render path)
**Followed by:** M3 slice 3+ (stream-ahead scheduler → clipmap rings → harness → fly-test)

---

## 0. Framing

DESIGN §5.2: a bounded pool of GPU-resident page textures keyed by `(level,
page_origin)`, fixed budget, LRU eviction, pages currently sampled by a ring
**protected from eviction**, and — critically — a **single owner of all page
RIDs** ("one place creates, one place frees"). §5.2 names scattered RID lifecycle
as WG9's black-far-ring bug (one path freed a texture another still referenced).
Slice 2 builds that pool. It is a **correctness-critical** slice: the danger is
freeing an in-use RID.

Slice 1 proved the render path with ONE texture owned by `Wg10PageCompute`. Slice
2 generalizes ownership into a real pool and turns `Wg10PageCompute` into a
stateless producer.

---

## 1. Scope

**In scope (slice 2):**
- **`PagePolicy`** — pure-Rust (no godot) eviction bookkeeping: fixed-capacity
  slots, `(level,origin)→slot` map, LRU order, protected set. Returns DECISIONS
  (reuse / allocate / allocate-evicting / full), owns no RIDs. Exhaustively
  headless `cargo test`-ed (the WG9-killer rules).
- **`Wg10PagePool`** (godot) — THE single owner of all page-texture RIDs. Asks
  `PagePolicy` what to do on `acquire`, performs the actual `texture_create` /
  reuse / fill-via-producer, exposes the page `Texture2DRD`. Budget, LRU,
  protected all enforced through the policy. One place creates, one place frees.
- **Refactor `Wg10PageCompute`** → stateless producer: `compute_page` writes
  height into a **pool-provided** texture RID; it no longer creates or owns a
  texture (`tex_rid`/`tex_wrapper` removed).
- **Slice-1 scene/gate** switch to the pool API; the existing `m3_slice1_check.gd`
  (same rendered terrain, distinct≥8) is the **regression guard**.
- **Windowed `m3_pool_check.gd`** — acquire/release cycles prove RIDs reuse/cycle
  correctly, budget respected, protected survives, a page still renders.
- ROADMAP/STATUS.

**Out of scope (slices 3+ — do NOT build):** clipmap rings (what calls acquire
per region), the stream-ahead scheduler (velocity, prefetch, bounded
computes/frame), the coarser-page-fallback *behavior* (slice 2 returns `Full`;
responding to it is the scheduler/rings), normal pages, L↔L+1 morph, movement,
multi-level streaming (the key carries `level` but slice 2 only exercises level
0). Slice 2 is STATIC — no frame loop; the pool is driven by explicit
acquire/release in tests + the slice scene asking for a few pages. No perf
number, no fly-test. **M3 stays OPEN.**

---

## 2. Interface constraints — NON-NEGOTIABLE

1. **Single RID owner (the §5.2 WG9 fix).** ONLY `page_pool.rs` calls
   `texture_create` / `free_rid` on page textures in the render path. `PagePolicy`
   owns no RIDs (keys/slots only); `Wg10PageCompute` owns no textures (produces
   into a pool RID). Structurally enforced by the file split.
2. **Never free an in-use (protected) RID.** A protected slot is never the
   eviction target. If all slots are protected and a new key is acquired, the
   policy returns `Full` (NOT a panic, NOT a wrong eviction). This is the exact
   anti-WG9 guarantee.
3. **M2 parity + slice-1 render unchanged.** The crate's M2 formula/parity stay
   green; slice-1 still renders the same terrain (its gate is the proof). The
   refactor changes *who owns the texture*, not the height values.
4. **Engine-agnostic core.** `PagePolicy` has no godot import (pure, headless-
   tested). godot/RenderingDevice only in `page_pool.rs` + `page_compute.rs` +
   `bind_worldgen.rs`.
5. **Config, not magic numbers** (pillar 1): pool capacity, page_px, world_span
   come from config/args — not scattered constants.

---

## 3. PagePolicy — pure eviction bookkeeping (`page_policy.rs`)

Pure Rust, no godot. Constructed with `capacity: usize` (slot count, > 0).

State:
- `slots: Vec<Option<PageKey>>` (len = capacity; which key occupies each slot)
- `map: BTreeMap<PageKey, usize>` (key → slot index)
- `lru: ` a recency mechanism (per-slot u64 stamp + a monotonic counter, or an
  explicit order vec — implementer picks; the contract is "least-recently-acquired
  unprotected slot is the eviction target")
- `protected: BTreeSet<usize>` (slot indices currently protected)

`PageKey = (i32 level, i64 origin_x, i64 origin_z)` (Ord-derivable for the map).

`Decision` enum: `Reuse(slot)`, `Allocate(slot)`, `AllocateEvicting{ slot, evicted: PageKey }`, `Full`.

- **`acquire(&mut self, key) -> Decision`:**
  - hit (key in map): touch LRU (MRU), mark protected → `Reuse(slot)`. (Acquiring
    a previously-released-but-still-resident page RE-protects it — `acquire` always
    means "in use now"; symmetric with `release`. A re-acquired page is safe from
    eviction again until re-released.)
  - miss + a free slot exists: occupy it, insert, MRU + protected → `Allocate(slot)`.
  - miss + at capacity: pick the LRU slot whose index is NOT in `protected`; if
    found → evict its key from the map, occupy with the new key, MRU + protected
    → `AllocateEvicting{ slot, evicted }`; if ALL slots protected → `Full`
    (state unchanged).
- **`release(&mut self, key)`:** remove the slot from `protected` (stays
  resident + LRU-eligible). Idempotent (releasing an absent/already-released key
  is a no-op).
- Small accessors for tests/stats: `resident_count()`, `is_protected(key)`,
  `capacity()`.

**Headless tests (the WG9-killer rules — `page_policy_tests.rs`):**
- hit returns the SAME slot as the prior allocate (RID stability across re-acquire).
- miss allocates into a free slot; budget (resident ≤ capacity) never exceeded.
- at capacity, eviction targets the LRU UNPROTECTED slot (not MRU, not protected).
- a protected slot is NEVER evicted; all-protected + new key → `Full` (not a
  panic, not a wrong eviction).
- after `release(key)`, that slot becomes the eviction target if it's now LRU.
- determinism: identical acquire/release sequence → identical decisions.
- `AllocateEvicting.evicted` names the correct (LRU-unprotected) key.

---

## 4. Wg10PagePool — single RID owner (`page_pool.rs`, godot, windowed)

GodotClass, base RefCounted. Holds: `policy: PagePolicy`, `slot_tex: Vec<Option<Rid>>`
(parallel to policy slots, the actual textures — THIS is the single owner),
`compute: ` access to the producer (a `Wg10PageCompute` instance or its dispatch,
held internally), the loaded pack + glsl source, and page dims (`page_px`,
`world_span`) + `capacity` from config. Plus stats counters (`created`, `reused`,
`recomputed`, `full_events`).

- **`#[func] configure(pack_dir, pack_file, glsl_path, capacity, page_px, world_span) -> GString`**
  (or load_pack_dir + setters): load pack, store glsl, size the policy + slot_tex
  to capacity. "" / error.
- **`#[func] acquire_page(level, origin_x, origin_z) -> Gd<Texture2dRd>`:**
  `match policy.acquire(key)`:
  - `Reuse(slot)`: wrap `slot_tex[slot]` (already filled), `reused += 1`, return it.
  - `Allocate(slot)`: `texture_create` a new R32F texture → `slot_tex[slot]`,
    invoke the producer to compute the page into it, `created += 1`, wrap + return.
  - `AllocateEvicting{slot, ..}`: the slot's texture RID already exists (same
    dims) — REUSE it, recompute the page for the new key into it (no free/recreate
    — zero RID churn), `recomputed += 1`, wrap + return.
  - `Full`: `full_events += 1`, `godot_warn!`, return an empty/invalid `Texture2dRd`.
- **`#[func] release_page(level, origin_x, origin_z)`:** `policy.release(key)`.
- **`#[func] stats() -> Dictionary`** (created/reused/recomputed/full_events/resident)
  — lets the gate assert RID cycling without literal free-counting.
- **Single owner / teardown:** the ONLY `texture_create` + `free_rid(texture)` for
  pages live here. A `free_all()` / Drop frees every `slot_tex` RID at teardown.
  Mid-run there is **no free** (eviction reuses the slot's texture) — steady-state
  zero churn, which is what the 1000 m/s target needs.

**RID lifecycle honesty:** because all pages are same-dims, eviction recomputes
into the evicted slot's existing texture rather than free+recreate. So "evicted"
RIDs are reused, not freed mid-run; explicit free is teardown-only. The gate
asserts the right thing — slot REUSE + content replacement + no slot-count growth
— not a literal free count.

---

## 5. Refactor Wg10PageCompute → stateless producer (`page_compute.rs`)

- `compute_page` signature changes: it takes a **target texture RID** (the
  pool-allocated texture) + page params (`glsl_path`, origin_x, origin_z,
  world_span, page_px, seed), dispatches `height_page.glsl` writing into that RID,
  returns `bool`/error. It no longer `texture_create`s or stores `tex_rid`/
  `tex_wrapper` (those fields removed).
- The pool owns the texture and calls the producer. (Internally, the pool may hold
  a `Wg10PageCompute` and call a `pub(crate)` produce-into-rid method, or the
  produce logic moves into a shared helper both can call — implementer picks the
  cleanest; the contract is: producer writes into a pool RID, never owns it.)
- `load_pack_dir` + the pack-buffer build may stay on `Wg10PageCompute` or move to
  a shared spot the pool uses — keep M2 parity untouched.

---

## 6. Gates

- **Headless `cargo test` (`page_policy_tests.rs`):** the §3 WG9-killer rules.
  This is the bulk of the correctness proof; runs in the cargo suite (no GPU).
- **`m3_slice1_check.gd` (MODIFIED, regression guard):** slice 1 now acquires its
  page via `Wg10PagePool` instead of direct `compute_page`; must STILL render the
  same terrain (status=pass, distinct≥8). This proves the refactor didn't break
  the proven render path.
- **`m3_pool_check.gd` (NEW, windowed, in the `m3` suite):** drive an
  acquire/release sequence on a tiny-budget pool (capacity 2 or 3) over several
  `(level,origin)` keys and assert via `stats()`:
  - re-acquiring a resident key does NOT create a new texture (`reused` increments,
    `created` does not) — RID stability on cache hit.
  - exceeding budget triggers eviction (resident never > capacity; `recomputed`
    increments) — the slot is reused, not leaked.
  - a PROTECTED page (acquired, not released) survives an over-budget acquire of
    other keys (it's still resident + its texture unchanged); acquiring beyond
    capacity while all are protected yields a `Full` event (`full_events` > 0), no
    crash, no wrong eviction.
  - after release + over-budget acquire, the released page's slot is reused.
  - a page still RENDERS: acquire one page, bind its `Texture2DRD`, capture, assert
    relief (reuse slice-1's distinct≥8 check) — the pool feeds the renderer.
  - run-mode WINDOWED (RIDs + render need a device; same as the gpu/m3 suites).

---

## 7. Files & boundaries
```
wg-10/rust/src/
  page_policy.rs        # NEW: pure PagePolicy + Decision (no godot) — eviction bookkeeping
  page_policy_tests.rs  # NEW: #[cfg(test)] exhaustive WG9-killer-rule tests (headless)
  page_pool.rs          # NEW: Wg10PagePool (godot) — single RID owner; asks PagePolicy; texture_create/free; calls producer
  page_compute.rs       # MODIFY: compute_page writes into a pool-provided RID; stateless producer (no tex ownership)
  lib.rs                # MODIFY: mod page_policy; mod page_pool; + #[cfg(test)] mod page_policy_tests;
wg-10/worldgen_terrain/m3/
  m3_slice1.gd          # MODIFY: acquire page via Wg10PagePool (was direct compute_page)
wg-10/worldgen_terrain/tests/
  m3_slice1_check.gd    # MODIFY: same (regression guard, stays green: distinct>=8)
  m3_pool_check.gd      # NEW: windowed acquire/release + RID-cycle + still-renders gate
tools/gate.py           # MODIFY: add m3_pool_check.gd to the m3 suite
docs/plans/             # MODIFY: ROADMAP (slice 2 done), STATUS
```
Each file one job: `page_policy.rs` = pure decisions (headless-tested), `page_pool.rs` = RID ownership (the §5.2 single owner), `page_compute.rs` = produce. The single-owner rule is structurally enforced — only `page_pool.rs` touches page texture RIDs.

## 8. Done + scope honesty
- **Done (slice 2):** PagePolicy headless tests green (WG9-killer rules); `m3_pool_check.gd`
  green (RIDs cycle correctly + a page renders); `m3_slice1_check.gd` still green
  (refactor preserved slice 1); `cargo test` + `fast`/`gpu`/`m3` suites green;
  ROADMAP/STATUS updated; committed.
- **NOT claimed:** no rings, no scheduler, no streaming loop, no movement, no
  prefetch, no coarser-fallback behavior (pool returns `Full`; the response is
  slice 3), no perf number, no fly-test. The pool is driven by explicit
  acquire/release, not a live frame loop. M3 stays OPEN.

## 9. Named risks (do not solve now)
- **`Full` handling** is slice 3's job (coarser fallback). Slice 2 only guarantees
  no free-in-use + no panic when Full.
- **Eviction reuses slot textures** (same dims) — correct + zero-churn now; if a
  future need for variable page dims arises, the slot-reuse assumption is revisited
  (free+recreate). Not now.
- **Single-level only exercised** — the policy keys on `level` but slice 2 tests
  level 0; multi-level streaming is slice 3+.
