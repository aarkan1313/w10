# WorldGen10 M3 — Slice 3: Stream-Ahead Scheduler Design

**Date:** 2026-05-29
**Status:** Approved (brainstorming) — pending writing-plans
**Layer:** Render pipeline (M3) — slice 3: velocity-aware stream-ahead scheduler (bounded computes/frame, coarser fallback, never black / never stall)
**Builds on:** M3 slice 1 (Wg10PageCompute → Texture2DRD render path), M3 slice 2 (Wg10PagePool single RID owner + PagePolicy)
**Followed by:** M3 slice 4+ (clipmap rings → harness components → fly-test scene → acceptance gate)

---

## 0. Framing

DESIGN §5.3 names the stream-ahead scheduler as **the unit WG9 lacked**. WG9's
128 ms-per-chunk black-slab disease came from creating a GPU height page *inside*
the build-during-motion path — synchronously, per chunk, with no rate limit and
no fallback. §5.3 fixes that structurally with three guarantees:

1. **Bounded work per frame** — dispatch ≤ N page computes per update, no matter
   how fast the camera moves. Per-frame production cost is capped by construction.
2. **Stream ahead** — given camera position *and velocity*, compute the pages the
   rings will need *soon* (current coverage + a velocity-biased lead margin), so
   pages arrive before they are sampled.
3. **Coarser fallback** — if a needed page is not yet resident, the ring samples
   the best available **coarser** resident page. Briefly lower-detail but correct
   terrain. **Never black, never a stall.**

Slice 2 built the bounded pool that owns all page RIDs and answers
acquire/release/Full. Slice 3 builds the brain that *drives* that pool under
motion: it decides which pages to acquire each frame, in what priority, within a
per-frame budget, and which coarser page to fall back to when a fine page is not
ready. This is **the first slice with a live frame loop** and the first that uses
the pool's `Full` path under motion.

It is a **correctness-critical** slice for the project's headline pillar: the
"never black, never stall" guarantee is decided here. The danger is a coverage
gap with no valid fallback (→ black) or unbounded acquires (→ stall).

---

## 1. Scope

**In scope (slice 3):**

- **`SchedulePolicy`** — pure-Rust (no godot) scheduling math: given camera
  position + velocity + the resident page set, compute (a) the multi-level
  coverage the rings need, (b) a bounded, prioritized per-frame acquire/release
  plan, and (c) a coarser-page fallback for any not-yet-resident page. Returns
  PLANS and DECISIONS; owns no RIDs and dispatches nothing. Exhaustively headless
  `cargo test`-ed (the never-black / bounded-work invariants).
- **`Wg10Streamer`** (godot) — the frame-loop driver. Holds a `Wg10PagePool` and a
  `SchedulePolicy` config; on each `update(camera_x, camera_z, vel_x, vel_z)` it
  runs the §5.4 loop: ask the policy for the frame plan, release departing pages,
  then acquire ≤ N pages (synchronously this slice — see §1.1). Exposes `stats()`
  for the gate and the future diagnostics overlay.
- **`Wg10PagePool::resident_keys()`** accessor — the policy needs the current
  resident set to diff against coverage. This is the **only** change to the pool.
- **`m3_stream_check.gd`** (`m3` suite, WINDOWED) — drives the streamer over a
  synthetic straight-line camera sweep at high speed and asserts the invariants:
  bounded acquires/frame, full coverage-or-coarser every frame (never black),
  pool budget never exceeded, deterministic.

**Out of scope (later slices, explicitly NOT built here):**

- **Async/background page production.** This slice produces pages **synchronously**
  inside `update` (≤ N per frame, so still bounded). The scheduler↔pool seam is
  designed **async-ready** (§1.1) so background production drops in later behind
  `acquire_page` with **zero scheduler change**. Trigger to actually build it:
  when a single page compute becomes heavy enough that N synchronous computes
  blow the frame budget — i.e. multi-pass pages (M5 detail/normals, M6 biome
  masks, M7 erosion/hydrology). Tracked as a deferred pool-layer follow-up.
- **Clipmap rings / real meshes.** Slice 3 has no ring meshes and no L↔L+1 morph.
  "Coverage" is computed abstractly (the page keys rings *would* need); the gate
  verifies the key-level invariants, not pixels. Rings are slice 4.
- **Harness / camera / movement / UI overlay.** The gate drives a *synthetic*
  camera path in code. The real WASD/mouse fly camera and diagnostics overlay are
  later M3 slices (DESIGN §6.4).
- **The acceptance gate (p99 < 6 ms + manual fly).** Slice 3's gate proves the
  scheduling invariants, not the frame-time budget. Perf acceptance needs real
  rings + real movement and is the M3 milestone gate (DESIGN §7.3).

### 1.1 The async-ready seam (the "don't hamstring us later" decision)

The scheduler **never assumes a page is resident the same frame it was acquired.**
Its loop is: plan from *this frame's* resident set → request acquires → render
from *whatever is currently resident*, using coarser fallback for the rest. That
is exactly the contract a background producer needs. Concretely:

- `SchedulePolicy::plan_frame` takes the resident set as an **input** and emits an
  `acquire` list — it does not call the pool, and it does not mark requested pages
  resident. Residency is observed next frame via `resident_keys()`.
- This slice's `Wg10Streamer` happens to fulfil each acquire synchronously before
  returning, so by next frame they *are* resident. A future async streamer fulfils
  them on a background queue; the policy is identical because it only ever reads
  the *observed* resident set and always has a coarser fallback for the gap.

So async production is a **pool/streamer-layer** change later, not a scheduler
redesign. The coarser-fallback guarantee is what makes the seam safe: a page that
is requested-but-not-yet-resident is, to the renderer, just a coverage gap — and
every coverage gap has a coarser resident ancestor by construction (§2.4).

---

## 2. `SchedulePolicy` (pure Rust, no godot)

A pure module — same discipline as `PagePolicy`/`grammar`/`height`: no `godot`
imports, fully unit-testable headless, deterministic. It computes scheduling math
over **page keys**; it never touches RIDs, textures, or RenderingDevice.

### 2.1 Types & config

```rust
/// A page key shared with the pool: which clipmap level, and the page's
/// floor-quantized origin in page-grid units at that level.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PageKey { pub level: u8, pub ox: i32, pub oz: i32 }

pub struct ScheduleConfig {
    pub num_levels:   u8,    // clipmap levels 0 (finest) .. num_levels-1 (coarsest)
    pub base_span:    f64,   // world-space span (metres) of one level-0 page
    pub radius_pages: i32,   // ring half-extent in pages, per level (e.g. 1 -> 3x3)
    pub lead_frames:  f64,   // velocity lead: bias coverage centre this many frames ahead
    pub max_per_frame: u32,  // hard cap on acquires dispatched per update (e.g. 2)
}
```

`PageKey` is the same key the pool maps to slots (`(level, origin)`); slice 2's
pool already keys on level+origin, so this is the shared vocabulary, not a new one.
At level `L` a page spans `base_span * 2^L` metres — coarser levels cover more
ground per page (standard clipmap doubling).

### 2.2 `coverage` — what the rings need this frame

```rust
pub fn coverage(&self, pos: Vec2, vel: Vec2) -> Vec<PageKey>
```

For each level `L` in `0..num_levels`:
- `span = base_span * 2^L`.
- **Velocity-biased centre:** `centre = pos + vel * lead_frames` (look ahead in the
  direction of travel, so pages stream in before they're sampled). Lead distance
  scales with speed automatically — fast camera ⇒ look further ahead.
- Floor-quantize `centre` to the level's page grid: `cx = floor(centre.x / span)`,
  `cz = floor(centre.z / span)` (same floor semantics as grammar/height — seam-
  exact, no off-by-one at axis crossings).
- Emit the `(2*radius_pages + 1)^2` ring of keys around `(cx, cz)` at level `L`.

Union across all levels = the full set of pages the rings would sample this frame.
Coarser levels naturally overlap finer ones in world space — that overlap is what
makes the coarser fallback always available (§2.4).

### 2.3 `plan_frame` — bounded, prioritized acquire/release

```rust
pub struct FramePlan { pub acquire: Vec<PageKey>, pub release: Vec<PageKey> }

pub fn plan_frame(&self, pos: Vec2, vel: Vec2, resident: &HashSet<PageKey>) -> FramePlan
```

1. `needed = coverage(pos, vel)` (as a set).
2. `release = resident - needed` — resident pages no longer in coverage. **Not**
   capped: releasing is just unprotect/LRU bookkeeping in the pool, cheap, and we
   must free slots so acquires can succeed. (The pool still won't evict a page
   that's somehow still protected; release is the scheduler relinquishing its
   claim.)
3. `missing = needed - resident`.
4. **Prioritize `missing`** ascending by a cost key, then truncate to
   `max_per_frame`:
   - primary: **finest first** (lower `level` = higher detail = more visible) —
     coarser gaps are covered by even-coarser pages, finer gaps are not;
   - secondary: **nearest-and-most-ahead-of-motion first** — distance from `centre`
     (the velocity-biased lead point, §2.2), so the pages we're about to fly into
     win ties.
5. `acquire = prioritized_missing[..max_per_frame]`.

Bounded-work invariant: `acquire.len() <= max_per_frame` **always**, regardless of
how large `missing` is (i.e. regardless of camera speed). This is the structural
fix for WG9's unbounded per-frame production.

### 2.4 `coarser_fallback` — the never-black guarantee

```rust
pub fn coarser_fallback(&self, missing: PageKey, resident: &HashSet<PageKey>) -> Option<PageKey>
```

For a needed-but-not-resident page, walk **up** the levels (coarser): for
`L' in (missing.level+1)..num_levels`, compute the coarser page whose world-space
extent contains `missing`'s centre (origin re-quantized to `span' = base_span *
2^L'`). Return the first such ancestor that **is** in `resident`. `None` only if no
coarser resident page covers the area at all.

**Never-black invariant (the one the gate enforces):** during a coverage sweep, for
**every** page in `coverage()` that is not resident, `coarser_fallback` returns
`Some`. Established by construction + maintained by the policy:
- the **coarsest** level (`num_levels-1`) has the largest pages and the widest
  `radius_pages` world footprint; the scheduler keeps coarsest-level coverage
  resident first-class (it's in `coverage()` like any level, and finest-first
  prioritization in §2.3 means coarse pages, being few and rarely missing once
  warm, stay resident);
- because coarser pages span `2^(L'-L)`× more ground, a single resident coarse page
  is the fallback for many missing fine pages — so the coarse ring being resident
  blankets the whole fine region.
- The gate asserts this holds **even on frame 0** (cold start) by requiring the
  warm-up to acquire coarsest-first, and **every frame mid-sweep** at high speed.

This is the contract that also makes the async seam (§1.1) safe: a
requested-but-not-yet-produced page is just a coverage gap, and every gap has a
coarser resident ancestor.

### 2.5 Determinism & purity

- No `Date`/`Math.random`/wall-clock; output is a pure function of
  `(config, pos, vel, resident)`. Same inputs ⇒ same plan (asserted).
- No heap churn beyond the returned `Vec`s; no `godot` import; no I/O.
- Floor-quantization uses the same `floor` semantics as `grammar`/`height` so page
  origins are seam-exact and consistent with the rest of the pipeline.

---

## 3. `Wg10Streamer` (godot binding — the frame-loop driver)

The thin Godot class that owns the live loop. Holds one `Wg10PagePool` and one
`SchedulePolicy` config. No scheduling math lives here — it only translates
between the pool and the policy and dispatches the bounded acquires.

### 3.1 API

```
configure(pool: Wg10PagePool, num_levels, base_span, radius_pages, lead_frames, max_per_frame)
update(camera_x: float, camera_z: float, vel_x: float, vel_z: float) -> void
stats() -> Dictionary   # { acquired_this_frame, released_this_frame, resident,
                        #   coverage_size, fallback_used, full_events, frame }
```

### 3.2 `update` — the §5.4 frame loop (this slice's synchronous form)

```
plan = policy.plan_frame(pos, vel, pool.resident_keys())
for key in plan.release:  pool.release_page(key)        # relinquish claim; cheap
for key in plan.acquire[..max_per_frame]:               # bounded by construction
    rid = pool.acquire_page(key, ...)                   # synchronous produce this slice
    if rid is null: record full_event                   # pool said Full — leave gap; ring uses coarser
# record stats; the "render" step (sample resident, else coarser_fallback) is the
# gate's job this slice (no real rings yet) — the streamer exposes resident_keys +
# coverage so the gate can assert the never-black invariant.
```

- **Release before acquire** so freed slots are available to the acquires in the
  same frame (maximizes the chance acquires succeed within budget).
- Acquires are **already** truncated to `max_per_frame` by the policy; the streamer
  re-asserts the cap defensively (never dispatch more than N).
- A `null` from `acquire_page` (pool `Full`) is **not** an error — it means the
  page must be served by coarser fallback this frame. Recorded in `full_events`.
- `stats()` is the gate's and the future overlay's window into the loop. No
  scheduling decision is made from outside.

### 3.3 What the streamer does NOT do

- It does not create or free RIDs (the pool is the sole owner — slice 2 rule).
- It does not contain coverage/priority/fallback math (the policy does).
- It does not render or hold meshes (rings are slice 4).
- It does not block on background work (there is none yet; the seam is ready).

---

## 4. Gates

### 4.1 `m3_stream_check.gd` (`m3` suite, WINDOWED)

The pool needs the global RenderingDevice (windowed), so this check lives in the
`m3` suite alongside slice 1/2 and returns SKIP code 2 on a headless/no-GPU box.

Drives a **synthetic straight-line sweep**: fixed start, constant high velocity
(fast enough that `missing` each frame far exceeds `max_per_frame`, exercising the
bound), over ~M frames. Each frame calls `update(...)` then reads `stats()`.
Asserts:

1. **Bounded work:** `acquired_this_frame <= max_per_frame` every frame.
2. **Budget:** `resident <= pool capacity` every frame (slice-2 invariant holds
   under motion).
3. **Never black:** every page in `coverage()` is either resident **or** has a
   coarser resident fallback — checked every frame, including frame 0 (cold start).
4. **Determinism:** running the same sweep twice yields identical per-frame
   `(acquire, release)` sequences.
5. **Liveness / progress:** finest-level coverage strictly improves over the warm-up
   (the sweep doesn't get stuck perpetually coarse — stream-ahead actually catches
   up when the camera holds steady), guarding against a vacuous "always fall back"
   pass.

Non-vacuous by construction: the speed guarantees `missing > max_per_frame`, so the
bound and the fallback are both genuinely exercised (a too-slow sweep that never
misses would make assertions 1/3 trivially true — the check picks speed to avoid
that and asserts `full_events > 0` **or** `fallback_used > 0` at least once).

### 4.2 `SchedulePolicy` cargo tests (headless, pure)

Exhaustive unit tests on the pure module (the WG9-killer invariants, provable
without a GPU):

- `coverage` size = `num_levels * (2*radius+1)^2`; velocity bias shifts the centre
  in the travel direction; floor-quantization is seam-exact at axis crossings.
- `plan_frame`: `acquire.len() <= max_per_frame` for arbitrarily large `missing`;
  `release == resident - needed`; finest-first + nearest-first priority order;
  empty plan when fully resident and stationary.
- `coarser_fallback`: returns a resident coarser ancestor when one exists; `None`
  only when no coarser resident page covers the area; **never-black property test** —
  over a randomized sweep with the coarsest ring kept resident, every missing fine
  page has `Some` fallback.
- Determinism: identical `(config,pos,vel,resident)` ⇒ identical output.

### 4.3 Regression

`fast`/`gpu` suites unchanged. `m3` suite grows to **3** checks (slice1 + pool +
stream), all `fail=0`. Cargo test count grows by the `SchedulePolicy` suite.

---

## 5. Files

**New:**
- `wg-10/rust/src/schedule_policy.rs` — `PageKey`, `ScheduleConfig`,
  `SchedulePolicy` (`coverage` / `plan_frame` / `coarser_fallback`). Pure, no godot.
- `wg-10/rust/src/schedule_policy_tests.rs` (or `#[cfg(test)]` in the module) —
  the §4.2 unit/property tests.
- `wg-10/rust/src/streamer.rs` — `Wg10Streamer` godot binding (the §3 loop driver).
- `wg-10/worldgen_terrain/tests/m3_stream_check.gd` — the §4.1 gate.

**Modified:**
- `wg-10/rust/src/page_pool.rs` — add `resident_keys()` accessor (the only pool
  change; reads the existing `(level,origin)→slot` map, returns the keys).
- `wg-10/rust/src/lib.rs` — register `schedule_policy` + `streamer` modules and the
  `Wg10Streamer` class.
- `tools/gate.py` — add `m3_stream_check.gd` to the `m3` suite list.

**Soft cap:** all new files stay under the DESIGN §7 ~600-line cap; `SchedulePolicy`
math and its tests split into module + tests file if either approaches it.

---

## 6. Definition of done

- `SchedulePolicy` pure module + tests: all cargo tests green (bounded-work,
  never-black property, determinism, priority order).
- `Wg10Streamer` drives the pool over the synthetic sweep; `m3_stream_check.gd`
  passes (bounded, budget-safe, never-black, deterministic, non-vacuous).
- `m3` suite = 3 checks `fail=0` (windowed); `fast`/`gpu` unchanged; cargo green.
- STATUS + ROADMAP updated (slice 3 done; the async-production follow-up tracked as
  a deferred pool-layer item with its trigger written down).
- Each task committed separately (TDD shape: failing test → minimal impl → pass →
  commit). Per DESIGN §7.3, the perf+visual+manual acceptance gate is the **M3
  milestone** gate, not slice 3's — slice 3's done is the scheduling-invariant gate.

---

## 7. Risks & mitigations

- **A coverage gap with no coarser fallback ⇒ black.** This is *the* failure mode
  the slice exists to prevent. Mitigated by the never-black invariant (§2.4) being
  a property test in cargo **and** a per-frame assertion in the windowed gate, and
  by warm-up acquiring coarsest-first so the coarse blanket is resident before fine
  detail streams in.
- **`max_per_frame` too low ⇒ finest detail never catches up at speed.** Acceptable
  this slice (correct-but-coarse is the guarantee, not full detail at 1000 m/s); the
  liveness assertion (§4.1.5) ensures it *does* catch up when the camera steadies.
  Tuning `max_per_frame`/`lead_frames` against real frame cost is M3-milestone work
  once rings + real movement exist.
- **Synchronous production in `update` blows the frame budget when pages get
  heavy.** Real, and deferred on purpose (§1.1). The async seam means the fix is a
  pool/streamer change with zero scheduler change; the trigger (multi-pass pages,
  M5–M7) is written into ROADMAP so it isn't forgotten.
- **`resident_keys()` exposing pool internals.** Kept to a read-only key snapshot
  (no RIDs, no slots) so the single-owner rule (slice 2) is intact — the pool still
  solely creates/frees; the streamer only *reads* what's resident.
