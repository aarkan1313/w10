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

Current source-size check also retires the old 3.6k-line source finding. The former
`biome_page_compute.rs` hotspot is now split, and the first static-reference split reduces
`wg-10/rust/src/page_pool/static_reference.rs` to runtime sampling/texture upload while moving
payload work into `static_reference/payload.rs`. The remaining refactor pressure is not "split a
giant biome file"; it is separation of producer facts, page-pool routing, renderer presentation,
and review artifacts.

Owner-visual fixes landed in the review path:

- per-mode color normalization follows displayed relief instead of one fixed 2000 m palette ref;
- the owner fly starts from an accepted-reference camera frame and `G` reframes to it;
- review fog/far uses the accepted 76.8 km visual footprint while streaming still loads farther;
- REFERENCE material pages blend into terrain shading rather than replacing it;
- the owner-scene smoke test proves static material page textures are bound.

Static-reference separation is now underway: payload loading/validation has been split out first.
Remaining static-reference seams are page sampling, material-code presentation, and
report/diagnostic surfaces. After that, move producer selection out of `Wg10PagePool` into an
explicit producer interface so REFERENCE, MOUNTAIN, WORLD, and LEGACY are not routed by one pool
implementation.

### Static-Reference Split Checkpoint - 2026-06-04

The first static-reference separation pass is complete. `static_reference.rs` is reduced from the
former payload/runtime/report mix to runtime sampling and page texture upload, while
`static_reference/payload.rs` now owns the JSON schema, contract validation, chunk stitching,
material-hint validation, conditioning-stat validation, and payload-focused tests.

Current proof after the split:

- `cargo test -p wg10_terrain --lib` = 227 passed / 0 failed.
- `powershell -ExecutionPolicy Bypass -File tools\build_rust.ps1` builds the Godot extension.
- `python tools\gate.py --suite fast` = 8/8.
- `python tools\gate.py --suite review_runtime` = 2/2.
- `python tools\gate.py --suite review_runtime_modes` = 2/2. Latest render p99:
  REFERENCE 0.415 ms, MOUNTAIN 0.247 ms, WORLD 0.474 ms, with zero hide/show churn.
- `python tools\gate.py --suite review_runtime_visual` = 1/1.

Visual read after the proof: runtime lag/page visibility is no longer the primary measured
failure. The remaining owner-visible problems are architectural:

- Mode 1 / `REFERENCE` is the accepted static bridge and still the visual baseline.
- Mode 2 / `MOUNTAIN` is a raw seam-safe GPU recipe candidate. It does not yet port the accepted
  pass-network, route carving, page-stable conditioning, material hints as live facts, or final
  dressing.
- Mode 3 / `WORLD` is still a composed prototype and currently exposes page-scale composition/LOD
  boundaries in capture.

WORLD follow-up audit: the page-scale artifact is caused by the owner fly's bounded WORLD preview
using one active biome per page. Switching the review helper to top-2 or unbounded height compose
is not currently viable on the synchronous fly stream: `review_runtime_modes` measured WORLD
`cpu_max` around 1900-1950 ms and failed the 50 ms max-update gate, even with WORLD flow disabled.
Restoring the one-biome diagnostic cap returns the same gate to pass with latest WORLD
`cpu_p99=7.686 ms`, `cpu_max=10.050 ms`, zero hide/show, and render p99 `0.505 ms`.

Conclusion: mode 3 is a routing/material diagnostic until multi-biome WORLD height composition is
moved to a background/cache path or replaced by a cheaper preview contract. The next visual target
remains live `MOUNTAIN` against the accepted mountain-world-layer contract; WORLD composition should
not block that path or be presented as accepted terrain.

### Page-Pool Producer Split Checkpoint - 2026-06-04

The next producer separation pass is complete. Active producer classification and page dispatch now
live in `page_pool/producer.rs`, with an explicit `ProducerKind` for `Legacy`, `SingleBiome`,
`World`, and `StaticReference`. `acquire.rs` is reduced to page policy/slot acquisition/rollback,
and the redundant `use_biome_path` field is gone; public `uses_biome_path()` now derives from the
active producer kind.

Current proof after the split:

- `cargo test -p wg10_terrain --lib` = 227 passed / 0 failed.
- `powershell -ExecutionPolicy Bypass -File tools\build_rust.ps1` builds the Godot extension.
- `python tools\gate.py --suite fast` = 8/8.
- `python tools\gate.py --suite review_runtime` = 2/2.
- `python tools\gate.py --suite review_runtime_modes` = 2/2. Latest WORLD numbers:
  `cpu_p99=8.723 ms`, `cpu_max=10.612 ms`, render p99 `0.216 ms`, zero hide/show.
- `python tools\gate.py --suite biome_world` = 1/1.

Next refactor target: split static-reference sampling/material presentation from report surfaces,
then keep moving producer details out of `Wg10PagePool` storage/configuration toward explicit
producer implementations. Next visual target: make live `MOUNTAIN` reproduce the accepted
mountain-world-layer contract before trying to tune biome palettes or add more biomes.

### Static-Reference Sampling/Presentation Split Checkpoint - 2026-06-04

The follow-up static-reference split is complete. Runtime sampling now lives in
`page_pool/static_reference/sampling.rs`, and the temporary renderer-facing material-code
projection now lives in `page_pool/static_reference/presentation.rs`. The root
`static_reference.rs` file is down to the runtime holder, JSON entrypoint, and height texture upload
path (115 lines). Current split sizes:

- `static_reference.rs` = 115 lines.
- `static_reference/payload.rs` = 556 lines.
- `static_reference/sampling.rs` = 126 lines.
- `static_reference/presentation.rs` = 71 lines.

Current proof after the split:

- `cargo test -p wg10_terrain --lib` = 227 passed / 0 failed.
- `powershell -ExecutionPolicy Bypass -File tools\build_rust.ps1` builds the Godot extension.
- `python tools\gate.py --suite fast` = 8/8.
- `python tools\gate.py --suite review_runtime` = 2/2.
- `python tools\gate.py --suite review_runtime_visual` = 1/1.
- `python tools\gate.py --suite review_runtime_modes` = 2/2. Latest scripted motion CPU p99:
  REFERENCE 34.640 ms, MOUNTAIN 9.161 ms, WORLD 8.122 ms, with zero hide/show in all three.
  Latest render p99: REFERENCE 0.234 ms, MOUNTAIN 0.247 ms, WORLD 0.488 ms.

Fresh visual captures still support the same fix order:

- Mode 1 / `REFERENCE` remains the accepted static mountain-network bridge, not the live runtime.
- Mode 2 / `MOUNTAIN` is the correct shippable target and must port the accepted mountain-world-layer
  contract: source/display mapping, macro field, connected pass-network routes, route carving,
  page-stable conditioning, material hints, and facts/collision story.
- Mode 3 / `WORLD` remains diagnostic until multi-biome composition is async/cached or replaced by a
  cheaper preview contract. Do not chase palette tweaks there as the primary fix.

Next refactor target: split static-reference report/diagnostic surfaces from `Wg10PagePool`
state API, then keep moving producer details toward explicit producer implementations. Next visual
target remains the live `MOUNTAIN` mountain-world-layer port.

### Static-Reference Report Split Checkpoint - 2026-06-04

Accepted-baseline report surfaces have now moved out of the generic page-pool state API. The new
`page_pool/static_reports.rs` module owns:

- `mountain_world_layer_contract_report()`.
- `static_reference_report()`.
- `static_reference_page_report(...)`.
- static page corridor/material helper methods consumed by `Wg10TerrainView`.

`state_api.rs` is reduced from 549 to 349 lines and now carries generic pool state/source-transform
APIs plus WORLD route diagnostics, not the accepted static baseline facts.

Current proof after the split:

- `cargo test -p wg10_terrain --lib` = 227 passed / 0 failed.
- `powershell -ExecutionPolicy Bypass -File tools\build_rust.ps1` builds the Godot extension.
- `python tools\gate.py --suite fast` = 8/8.
- `python tools\gate.py --suite review_runtime` = 2/2.
- `python tools\gate.py --suite review_runtime_visual` = 1/1.
- `python tools\gate.py --suite review_runtime_modes` = 2/2. Latest scripted motion CPU p99:
  REFERENCE 34.839 ms, MOUNTAIN 9.369 ms, WORLD 7.792 ms, with zero hide/show in all three.
  Latest render p99: REFERENCE 0.326 ms, MOUNTAIN 0.251 ms, WORLD 0.470 ms.

Next refactor target: split WORLD route diagnostics out of `state_api.rs` if another
behavior-neutral page-pool cleanup is needed. Next visual target is still higher priority:
start porting the accepted mountain-world-layer facts into the live `MOUNTAIN` producer rather
than tuning mode 3.

### Live-MOUNTAIN Fact Bridge Checkpoint - 2026-06-04

The first non-static mountain-world-layer bridge is now in the live `MOUNTAIN/network_ref` path.
`Wg10PagePool` can bind the accepted mountain payload as a separate reference beside the live
single-biome producer, and the contract/report surfaces expose its pass-network, route-carving,
page-stable conditioning, corridor, and material-hint facts. The renderer can also use the bound
reference as a material-page source for live `MOUNTAIN`.

Current proof after the bridge:

- `cargo test -p wg10_terrain --lib` = 227 passed / 0 failed.
- `powershell -ExecutionPolicy Bypass -File tools\build_rust.ps1` builds the Godot extension.
- `python tools\gate.py --suite fast` = 8/8.
- `python tools\gate.py --suite review_runtime` = 2/2.
- `python tools\gate.py --suite review_runtime_visual` = 1/1.
- `python tools\gate.py --suite review_runtime_modes` = 2/2. Latest scripted motion CPU p99/max:
  REFERENCE 31.273/40.549 ms, MOUNTAIN 25.059/46.348 ms, WORLD 9.033/10.894 ms, with zero
  hide/show in all three. Latest render p99: REFERENCE 0.233 ms, MOUNTAIN 0.247 ms, WORLD
  0.216 ms.

Follow-up visual recovery: `MOUNTAIN/network_ref` now writes height from the same bound
mountain-world-layer payload it uses for material/fact pages. Its contract report names this as
`single_mountain_world_layer_reference_bridge`, sets
`height_source=bound_world_layer_reference_payload`, and keeps
`procedural_world_layer_height=false`. The latest capture shows MOUNTAIN/network matching
REFERENCE at the reviewed frame, and the latest gates still pass:

- `cargo test -p wg10_terrain --lib` = 227 passed / 0 failed.
- `powershell -ExecutionPolicy Bypass -File tools\build_rust.ps1` builds the Godot extension.
- `python tools\gate.py --suite fast` = 8/8.
- `python tools\gate.py --suite review_runtime` = 2/2.
- `python tools\gate.py --suite review_runtime_visual` = 1/1.
- `python tools\gate.py --suite review_runtime_modes` = 2/2. Latest scripted motion CPU p99/max:
  REFERENCE 31.577/40.887 ms, MOUNTAIN 35.781/42.245 ms, WORLD 8.431/19.247 ms, with zero
  hide/show in all three. Latest render p99: REFERENCE 0.232 ms, MOUNTAIN 0.367 ms, WORLD
  0.216 ms.
- The visual gate now also locks the recovered bridge: REFERENCE vs
  MOUNTAIN/network sampled image diff is mean `0.000000`, p95 `0.000000` over
  57,600 samples at stride 4, with budgets `0.002500` / `0.020000`.

This intentionally recovers a good reviewed visual before adding more biome complexity. It does
not close final procedural acceptance: close-debug remains the raw live page recipe, and WORLD
remains diagnostic until multi-biome composition is backgrounded/cached or given a cheaper preview
contract.

Next visual/procedural target: turn the reference-backed bridge into a generated world-layer
producer or measured candidate equivalent, then prove its numeric gap against REFERENCE moves down
without reintroducing page/LOD drift.

The highest-priority audit finding, F1 (missing scale-invariant cross-level macro gate), is now
implemented in source: `Wg10BiomePageCompute::generate_runtime_page_flow(..., flow_on)` exposes the
readback-only macro path, `wg-10/worldgen_terrain/tests/biome_crosslevel_check.gd` compares level 0
and level 1 macro pages over identical world XZ points, and `tools/gate.py` wires it into
`biome_fly` after the 576 parity gate. The editor-closed/windowed GPU run has now passed. This does
**not** mean the live producer is visually accepted; it means the former 73% cross-level macro warp
has a passing proof and the remaining defects are content-contract and presentation issues.

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

## Latest Runtime Read - 2026-06-04

Keys `1`, `2`, and `3` in `mountain_fly_review.tscn` are different
architectures, not three equivalent quality views:

- `1` / `REFERENCE` is the accepted static mountain-network payload streamed
  through the live clipmap renderer.
- `2` / `MOUNTAIN` with `network_ref` is the reference-backed bridge. It now
  matches `REFERENCE` in the visual capture gate and in a numeric center-page
  fact guard for corridor/material hints.
- `3` / `WORLD` is still a bounded diagnostic composition path. It is capped to
  one active biome per page because top-2/full WORLD height composition caused
  roughly 1900-1950 ms synchronous page-build hitches in the owner fly stream.
  Page-scale regions and samey/unfinished materials are expected there until
  WORLD composition is async/cached or replaced by a cheaper preview contract.

Latest gates with the editor closed:

- `python tools\gate.py --suite review_runtime` = 2/2.
- `python tools\gate.py --suite review_runtime_modes` = 2/2. Scripted motion
  CPU p99/max: REFERENCE 32.593/41.244 ms, MOUNTAIN 40.265/43.680 ms, WORLD
  8.617/10.980 ms, with zero hide/show in all three. Render p99: REFERENCE
  0.342 ms, MOUNTAIN 0.382 ms, WORLD 0.216 ms.
- `python tools\gate.py --suite review_runtime_visual` = 1/1. REFERENCE and
  MOUNTAIN/network sampled image delta remains mean `0.000000`, p95 `0.000000`
  at 57,600 sampled pixels.

Immediate fix direction: do not tune WORLD as if it were the accepted mountain
look. Keep `REFERENCE`/`MOUNTAIN network_ref` as the mountain acceptance lane,
add manual-path instrumentation if owner fly still shows pop outside the gated
camera path, and move WORLD visual quality work behind a separated
world-selection/producer path instead of the current synchronous page stream.
Implemented pop mitigation: renderer page fade is now wall-clock based
(`0.18 s`) instead of frame-count based, so high-FPS owner review does not
compress REPAGE transitions into a near-hard snap.
