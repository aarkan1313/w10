# WG10 Spec Audit And Separation Plan - 2026-06-03

This audit is based on the current worktree in `D:\workflows\worldgen10` on
branch `slice4-gpu-page-integration`. It treats untracked files as part of the
current state, because much of the active WG10 work is presently untracked.

## Addendum - 2026-06-04 Stabilization

### Current Owner-Visual Checkpoint - 2026-06-04

The windowed scale-invariance and owner-runtime gates have now run on hardware. Current proof:

- `cargo test -p wg10_terrain --lib` = 227 passed / 0 failed.
- `python tools\gate.py --suite fast` = 8/8.
- `python tools\gate.py --suite review_runtime` = 2/2.
- `python tools\gate.py --suite review_runtime_modes` = 2/2.
- `python tools\gate.py --suite review_runtime_visual` = 1/1.
- `python tools\gate.py --suite biome_fly` = 4/4, including cross-level macro ratio
  0.066665 <= 0.08.

This retires the old F1 "missing cross-level gate" finding as an implementation/gate
blocker. It does not mean visual acceptance is complete: live `MOUNTAIN` still lacks
the accepted pass-network, route-carving, page-stable conditioning, and facts/material
world-layer contract. `REFERENCE` remains the accepted static baseline bridge; `MOUNTAIN`
and `WORLD` remain explicit candidates/prototypes.

Current source-size check also retires the old 3.6k-line source finding. The largest tracked
source hotspot is now `wg-10/rust/src/page_pool/static_reference.rs` at about 778 lines, while
files above 1000 lines are docs/history. The next refactor pressure is not "split a giant
biome file"; it is separation of producer facts, page-pool routing, renderer presentation, and
review artifacts.

Owner-visual fixes landed in the review path:

- per-mode color normalization follows displayed relief instead of one fixed 2000 m palette ref;
- the owner fly starts from an accepted-reference camera frame and `G` reframes to it;
- review fog/far uses the accepted 76.8 km visual footprint while streaming still loads farther;
- REFERENCE material pages blend into terrain shading rather than replacing it;
- the owner-scene smoke test proves static material page textures are bound.

Next refactor target: split the static-reference bridge into payload loading/validation, page
sampling, material-code presentation, and report/diagnostic surfaces. After that, move producer
selection out of `Wg10PagePool` into an explicit producer interface so REFERENCE, MOUNTAIN, WORLD,
and LEGACY are not routed by one pool implementation.

The highest-priority audit finding, F1 (missing scale-invariant cross-level macro gate), is now
implemented in source: `Wg10BiomePageCompute::generate_runtime_page_flow(..., flow_on)` exposes the
readback-only macro path, `wg-10/worldgen_terrain/tests/biome_crosslevel_check.gd` compares level 0
and level 1 macro pages over identical world XZ points, and `tools/gate.py` wires it into
`biome_fly` after the 576 parity gate. This does **not** mean the scale-invariant producer is
accepted yet; the new gate still needs the editor-closed/windowed GPU run, followed by owner re-fly.

The source-size finding is also partially retired by subsequent refactor commits. The former
3.6k-line `biome_page_compute.rs` is now a module facade with focused children. Remaining refactor
pressure is architectural rather than raw line count: `page_pool` still routes producer choice, the
shader ABI is still manually mirrored, and the render/facts split remains unresolved for 4c.

## Evidence Checked

- Branch/status: `git status --short -b`
- File inventory and line counts: `rg --files`, excluding packs, png review
  output, and pycache for the source-size pass
- Living docs: `docs/plans/DESIGN.md`, `ROADMAP.md`, `STATUS.md`
- Current slice specs/plans:
  - `docs/superpowers/specs/2026-05-30-worldgen10-north-star-vision.md`
  - `docs/superpowers/specs/2026-05-30-worldgen-core-design.md`
  - `docs/superpowers/specs/2026-06-02-worldgen-slice4-gpu-page-integration-design.md`
  - `docs/superpowers/specs/2026-06-02-worldgen-runtime-drainage-design.md`
  - `docs/superpowers/specs/2026-06-03-worldgen-scale-invariant-biome-producer-design.md`
  - `docs/superpowers/plans/2026-06-03-scale-invariant-biome-producer.md`
- Core source files:
  - `wg-10/rust/src/biome_page_compute.rs`
  - `wg-10/rust/src/page_pool.rs`
  - `wg-10/rust/src/page_compute.rs`
  - `wg-10/rust/src/height.rs`
  - `wg-10/rust/src/lib.rs`
  - `tools/gate.py`
- Verification run in this audit:
  - `python tools\gate.py --suite pytest_fast`: 15 passed
  - `cargo test --lib` with `CARGO_TARGET_DIR` inside the workspace: 217 passed

Windowed Godot gates were not run in this audit. The active plan says those need
the editor closed and should not be forced while the owner may have the editor
open.

## Bottom Line

The main problem is not that "a lot" of source files exceed 1000 lines. The
current source-size hotspot is much more specific:

| Lines | File | Meaning |
|---:|---|---|
| 3693 | `wg-10/rust/src/biome_page_compute.rs` | Too many responsibilities in one file |
| 982 | `wg-10/rust/src/page_pool.rs` | Borderline too large and mixing legacy/biome producer concerns |
| 888 | `wg-10/rust/src/recipes_volcanic.rs` | Large but domain-local recipe code |
| 745 | `tools/dem_pack/export_godot_rough_world_chunks.py` | Large review/export tool |
| 695 | `wg-10/worldgen_terrain/harness/mountain_world_chunks_review.gd` | Large harness |

The destabilizing file is `biome_page_compute.rs`. It has become a combined
shader ABI manifest, gaussian-kernel builder, pass scheduler, per-biome recipe
schedule library, compose engine, runtime producer, readback test bridge,
GDExtension class, and test module. That concentration makes every next change
look risky, even when the math is well tested.

The project is also deliberately mid-migration. The legacy kernel-atlas path is
still live in several render/facts tests, while the new biome producer path is
partly integrated behind `configure_biome`. That is allowed by the Slice 4
spec until 4c, but it is now a major source of confusion and audit noise.

## Spec Audit

| Spec requirement | Current evidence | Status | Risk |
|---|---|---|---|
| North-star: infinite procedural terrain framework, adaptable first, performant, high quality, no shortcuts | Docs still align on this direction. The code is focused on the procedural biome path and clipmap render stack. | On track | Scope is broad, but current slice is narrow enough if kept disciplined. |
| Core design: kernels must not be sampled as runtime terrain; kernels become offline DNA/reference | `height.rs`, `height_page.glsl`, `page_compute.rs`, and several m3/facts gates still use the legacy kernel-atlas path. `height.rs` explicitly marks itself legacy/scaffolding. | Not done, but expected pre-4c | Confusing because old path is still default in many harnesses. Do not call Slice 4 complete until the atlas-removal audit passes. |
| Slice 4a/4b: GPU biome page producer behind flag, parity-gated | Source contains `Wg10BiomePageCompute`, `biome_page.glsl`, per-biome fragments, compose passes, 576 parity harness, and `configure_biome`. Headless Rust tests pass. | Mostly implemented | Windowed parity/perf gates were not rerun in this audit; current proof relies on prior status plus source shape. |
| Slice 4c: flip runtime default, remove 25 MB atlas, audit no active `KData` sampling | `m3_review.gd`, m3 tests, facts parity tests, `height_page.glsl`, `height_field.glsl`, `page_compute.rs`, and `gpu_compute.rs` still reference atlas/KData. | Not done | This is the biggest remaining integration boundary. |
| Runtime drainage spec: on-demand full-res flow bake plus drainage fact cache | Current source still uses inline flow relaxation in the biome page producer, with scale-invariant flow-off for coarse levels. No drainage fact cache subsystem found. | Not implemented | Do not conflate the June 3 flow-off coarse-level mitigation with the runtime-drainage fact-cache design. |
| Scale-invariant producer: world-anchor mountain sigmas, flow-on threshold, Rust/Python parity, cross-level macro gate, owner fly | Rust/Python flow-off parity exists (`mountain_macro_matches_oracle` passes). `page_pool.rs` has `flow_max_level`; GDScript configure calls pass `2`. `compute_biome_page_cached` rebuilds anchored kernels per spacing and gates `flow_on`. | Partially implemented | Required `biome_crosslevel_check.gd` is absent and not wired in `tools/gate.py`. Owner re-fly/docs are pending. |
| Design contract: render path never blocks, never shows black, degrades to coarser valid terrain | M3 policy/pool/view tests exist; headless policy tests pass. Biome fly perf gate exists and checks real work. | Legacy render path proven; biome path needs current windowed proof | Need rerun `biome_fly` and m3 windowed after DLL rebuild. |
| Design contract: visible surface vs collision/facts parity | Facts path still uses legacy `height.rs`/pack. Slice 4 spec says facts are out of scope for page-path swap. | Explicit exception, not resolved | Once the live render path flips to biome, visible terrain and gameplay facts will diverge unless a new facts story or explicit temporary exception is documented and gated. |
| Owner-judged visual acceptance | Status records prior owner acceptance for earlier slices. June 3 scale-invariant plan requires owner re-fly. | Pending for current scale-invariant work | Gates cannot prove "looks right"; this remains a hard acceptance step. |

## Review Findings

### F1 - Missing Required Scale-Invariant Gate

The June 3 scale-invariant spec requires a new cross-level macro-agreement gate
to prove the LOD warp is fixed. The plan names
`wg-10/worldgen_terrain/tests/biome_crosslevel_check.gd` and says to add it to
the `biome_fly` suite. Current `tools/gate.py` has `biome_fly` with only:

- `biome_page_576_parity_check.gd`
- `biome_fly_perf_check.gd`

No cross-level check file exists in the current tree.

Impact: the code may be correct, but the spec proof is missing. This is the
highest priority before claiming the scale-invariant producer is done.

### F2 - `biome_page_compute.rs` Is Carrying Too Many Contracts

`biome_page_compute.rs` currently owns:

- Rust copies of GLSL pass IDs and per-biome pass IDs
- Gaussian kernel generation and world anchoring
- Push-constant layout
- RenderingDevice uniform helpers
- Scheduler dispatch DSL
- Flow relaxation helpers
- Compose helpers
- All 11 biome schedules
- Apron buffer allocation/free
- Runtime context build/free
- Runtime page compute
- Readback-only GDExtension methods
- Godot class wrapper
- A large unit-test module

Impact: every biome, runtime, parity, resource-lifecycle, and shader ABI change
touches one file. This is exactly the kind of concentration that makes the team
feel lost: the file contains several subsystems that should have separate owners
and tests.

### F3 - `page_pool.rs` Mixes Pool Ownership With Producer Selection

`Wg10PagePool` correctly owns page texture RIDs and PagePolicy state, but it now
also chooses between the legacy kernel producer and the biome producer, stores
both producer configurations, and threads scale-invariant biome options.

Impact: the pool is becoming a producer router, lifecycle manager, and texture
owner at the same time. The existing reset tests help, but the code will get
harder when 4c flips defaults, removes atlas buffers, or adds drainage-fact
sampling.

### F4 - Current Docs Are Not A Single Current Truth

Examples:

- `ROADMAP.md` still has a 2026-05-31 top state.
- `STATUS.md` top state is 2026-06-02, while the repo contains 2026-06-03
  scale-invariant specs/plans and code.
- Several active plan/spec files are untracked.
- `DESIGN.md` is explicitly partially superseded.
- Current branch is ahead of origin by 19 commits and has many untracked files.

Impact: new work has no single authoritative current-state document. This is a
coordination risk, not just documentation polish.

### F5 - Legacy And New Runtime Paths Are Interleaved

The legacy path is intentionally kept for A/B and pre-4c rollback, but references
are spread through harnesses/tests:

- `m3_review.gd`, `proving_ground.gd`, m3 gates, facts gates use `height_page.glsl`
- `mountain_fly_review.gd` can toggle between biome and legacy
- `page_pool.rs` exposes both `configure` and `configure_biome`

Impact: it is easy to run a green gate that proves the old path, not the new one.
The biome perf gate checks `uses_biome_path()`, which is good. The rest of the
gate taxonomy should make old-path vs new-path explicit.

### F6 - Shader ABI Is Duplicated Manually

Pass IDs, pool slots, binding IDs, push constant fields, sigma lists, and
fragment expectations are manually mirrored between Rust and GLSL comments/code.
There are tests for parts of this, but the ABI is still implicit.

Impact: refactors are risky because a Rust constant can drift from GLSL. A small
manifest or generated constants file would reduce this risk.

### F7 - Facts/Collision Story Is About To Diverge From Render

`height.rs` explicitly says it remains the live per-point facts formula until
Slice 4 page-path swap. Slice 4 page integration says facts are out of scope. As
soon as the rendered biome path becomes default, visible terrain can differ from
facts/collision unless that is explicitly managed.

Impact: 4c can make the visual path better while silently breaking gameplay
expectations. Either keep this as a documented temporary exception with a gate
that only asserts old facts did not regress, or plan a follow-up facts producer.

## Recommended Refactor Plan

The refactor should not rewrite algorithms. The safe path is mechanical
separation first, keeping the current gates green and preserving the public
Godot-facing API until the seams are visible.

### Phase 0 - Stabilize Proof And Current State

Do this before structural file splits.

1. Add the missing `biome_crosslevel_check.gd` and wire it into `biome_fly`.
2. Rebuild the DLL with the editor closed and run:
   - `python tools/gate.py --suite biome_page`
   - `python tools/gate.py --suite biome_fly`
   - `python tools/gate.py --suite m3`
3. Update `STATUS.md` with the actual June 3 state:
   - headless cargo/Python pass counts
   - whether cross-level macro agreement passed
   - whether biome fly p99/update improved
   - what remains unproven
4. Make a named untracked-file decision:
   - promote current active specs/plans/tests/fixtures by name, or
   - move stale experiments under an explicit scratch/parking area, or
   - leave them untracked but document that they are not part of the audited build.

Exit criteria: the team can answer "what is current and green?" without reading
five docs and a giant source file.

### Phase 1 - Split `biome_page_compute.rs` Without Behavior Changes

Target module layout:

```text
wg-10/rust/src/biome_page/
  mod.rs                  # public facade; keeps old call sites stable
  abi.rs                  # binding IDs, pass IDs, push layout, pool slots
  kernels.rs              # gaussian kernels, S_REF, sigma anchoring
  scheduler.rs            # Scheduler and dispatch helpers
  flow.rs                 # flow_discharge / flow_channels helpers
  compose.rs              # compose pass helpers and compose constants
  resources.rs            # ApronBuffers, context allocation/free
  runtime.rs              # build/free/compute_biome_page_cached
  readback.rs             # Wg10BiomePageCompute Godot test/readback bridge
  schedules/
    mod.rs
    mountain.rs
    grassland.rs
    desert.rs
    coast.rs
    wetland.rs
    tundra.rs
    glacial.rs
    karst.rs
    temperate.rs
    rainforest.rs
    volcanic.rs
```

Keep `wg-10/rust/src/biome_page_compute.rs` temporarily as a compatibility
facade that re-exports the same names, then delete it once call sites are moved.

Suggested commit slices:

1. Move pure helpers/tests: gaussian kernel, apron/core index, push constant.
2. Move ABI constants and add a single test that asserts Rust ABI constants
   cover GLSL-visible pass ranges.
3. Move `Scheduler`, `flow`, and `compose` helpers.
4. Move `schedule_mountain` only, run cargo.
5. Move remaining schedules one biome at a time.
6. Move resource/context/runtime compute.
7. Move GDExtension readback class.

Rules:

- No algorithm edits in this phase.
- No signature changes unless required by module privacy.
- Preserve all current tests and add only relocation tests.
- Keep each commit small enough that a regression can be bisected to one seam.

Exit criteria: no single `biome_page/*` source file exceeds about 700 lines, and
runtime resource ownership is not in the same file as biome recipe schedules.

### Phase 2 - Split `page_pool.rs` By Producer Boundary

The page pool should own page texture RIDs and PagePolicy. It should not contain
the details of how a page is produced.

Recommended structure:

```text
wg-10/rust/src/page_pool/
  mod.rs              # Wg10PagePool Godot API, policy, slot texture ownership
  state.rs            # configured/unconfigured state and reset tests
  texture.rs          # create/free page texture helpers
  producer.rs         # PageProducer enum/trait-like interface
  legacy_producer.rs  # old atlas producer wrapping page_compute.rs
  biome_producer.rs   # biome producer wrapping biome_page::runtime
```

Use an enum before a trait object if that keeps Rust/Godot ownership simpler:

```rust
enum PageProducer {
    Legacy(LegacyPageProducer),
    Biome(BiomePageProducer),
}
```

`Wg10PagePool::acquire_page` should do only:

1. ask `PagePolicy` for a decision,
2. create or reuse the texture RID,
3. call `producer.compute_into(...)`,
4. update slot/wrapper/stats or rollback.

Exit criteria: adding/removing the legacy path does not require touching the
pool policy or texture lifecycle logic.

### Phase 3 - Make Gate Taxonomy Match The Migration

Rename or document suites so green results cannot be misread:

- `m3_legacy`: old atlas render-stack regression gates
- `biome_page`: primitive/per-biome/compose parity
- `biome_runtime`: biome producer runtime parity, cross-level, no-black, perf
- `facts_legacy`: current facts/collision legacy contract

This can be done by aliases first, without breaking old commands.

Add a simple audit gate for Slice 4c readiness:

- active default harness uses `configure_biome`
- `uses_biome_path() == true`
- no new-path context allocates pack atlas buffers
- no new-path shader samples `KData`
- legacy files are either deleted, parked, or named as legacy-only tests

Exit criteria: each suite name tells the reader what path it proves.

### Phase 4 - 4c Runtime Flip And Atlas Removal

Only after Phases 0-3.

1. Make the biome producer the default for the live mountain review path.
2. Keep legacy A/B available only behind an explicit legacy toggle or parked
   harness.
3. Remove atlas use from the new render path.
4. Run the atlas-removal audit gate.
5. Run `biome_runtime`, `m3_legacy` or equivalent regression suite, and owner
   fly.

Exit criteria:

- New live path is biome path by default.
- Atlas buffers are absent from the new path.
- Any remaining legacy path is explicitly labeled and not part of the current
  runtime default.

### Phase 5 - Facts/Collision Alignment Plan

Do not solve this inside the render refactor, but do not ignore it.

Options:

1. Temporary explicit exception: facts/collision remain legacy until a later
   slice. Add a gate that says this is expected and checks only legacy facts did
   not regress.
2. Sparse biome facts: port the accepted biome producer to sparse CPU facts for
   `get_height` and collision sample fields.
3. Bake-backed facts: if drainage facts become the runtime truth, facts/collision
   read the same cached/drainage data where needed.

Exit criteria: the team has a named facts path before gameplay depends on
biome-rendered terrain.

### Phase 6 - Only Then Consider Higher-Level Abstractions

Once files are separated, consider whether the schedules should remain manual
Rust code or become data-driven DAGs. Do not jump directly to a generic DAG
engine while the current code is still tangled.

Likely safe abstraction:

- keep per-biome schedules as code for now,
- put pass IDs and pool slot declarations in per-biome ABI structs,
- add tests that each schedule only uses declared sigmas/slots/passes.

Riskier abstraction:

- a generic schedule interpreter over a declarative pass graph. This may be
  useful later, but it is too much change before 4c is proven.

## Immediate Next Actions

1. Build the missing cross-level macro gate from the June 3 plan.
2. Run the required windowed gates with the editor closed.
3. Update `STATUS.md` and make the untracked current-state docs/tests explicit.
4. Start Phase 1 with pure helper extraction from `biome_page_compute.rs`.
5. Stop adding new responsibilities to `biome_page_compute.rs` except temporary
   compatibility exports.

## What Not To Do

- Do not "fix" the old `height.rs` kernel path as if it were the future terrain.
  It is documented scaffolding.
- Do not delete legacy files before the 4c audit gate exists; they are still
  useful regression/A-B evidence.
- Do not combine the cross-level fix, runtime flip, and file split in one PR.
- Do not accept a green cargo run as proof of the render path. The key render
  checks are windowed.
- Do not claim the drainage-fact-cache spec is implemented because coarse levels
  can run `flow_on=false`. That is only a mitigation, not the cache subsystem.
