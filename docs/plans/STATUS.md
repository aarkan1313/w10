# WorldGen10 — Status

> **NEXT-SESSION PICKUP: `docs/plans/WG10_HANDOFF_2026-06-05_CARVE_PORTED.md`** (the
> live handoff; supersedes WG10_FINAL_HANDOFF_2026-06-05.md).

> **✅ CARVED LOOK ON SCREEN — REGION-FACT PRODUCER WIRED + SEAM-EXACT (2026-06-05).** The
> region-fact producer integration is DONE: the carved "baked look" now reaches the screen
> through the live `Wg10PagePool`, via an off-frame async super-region bake. The Rung-1
> un-intercept gap is CLOSED. Verified on hardware (RTX 5090, editor-closed): cargo lib
> **265/265**; windowed gates `region_macro` ✅, `bake_worker` ✅, `region_rung1` ✅ (the
> on-screen gate: a baked region page upgrades past the never-black fallback, is finite +
> non-degenerate, AND two adjacent region pages AGREE to ~1 mm at their shared **INTERNAL**
> super-region border = internal-seam-exact on screen. NOTE: both test regions map to the same
> super-key, so this gate proves the by-construction INTERNAL seam only — the super-region OUTER
> border is NOT yet covered on screen, see deferred item below); regression `gpu` 4/4 ✅ + `biome_page` 3/3 ✅ (no GPU-parity
> disturbance). Branch `slice4-gpu-page-integration`, pushed.
>
> **Architecture shipped (GPU/Rust-first, engine-modular):**
> - **Off-frame async bake worker** (`region_bake/worker.rs`): a dedicated thread with its OWN
>   RenderingDevice (per-thread; never shares the pool's RD) — GPU super-macro readback → CPU
>   carve+condition → sliced region facts, returned over a channel. Never-black coarse fallback
>   while a region bakes. (Drop closes tx-then-joins; no deadlock, no detached thread.)
> - **Seam-exactness in THREE layers, each proven (the measured ~1090 m seam decomposed):**
>   1. **Percentiles** — `SmoothFieldPercentiles` (`region_bake/percentile_provider.rs`): a
>      smooth, world-position-keyed percentile FIELD behind a swappable `PercentileProvider`
>      interface (engine modularity). **0-ULP seam-exact** at the normalization layer (scalar
>      per-region drift ~442–533 m → 0.0 m).
>   2. **Carve + conditioning gaussian** — `bake_super_region` (`region_bake/mod.rs`): bake a
>      k×k SUPER-region as ONE field (carve+condition over the whole super-field), then SLICE
>      into per-region facts. Internal super-region borders are **0-ULP seam-exact BY
>      CONSTRUCTION** (the global Dijkstra carve runs once over the super-field, not per-region;
>      the edge-clamped gaussian likewise runs over the whole field). `k` is a modular knob
>      (k=1 = single region). This is the proven carve-big-then-slice model.
>   - Only super-region OUTER borders can still seam (k× rarer; percentiles stay exact there).
> - **`condition_world` is now a pure transform** taking per-cell percentile fields (length-1 =
>   scalar broadcast, bit-exact to the old path — all existing Python-parity + bake_region gates
>   stay green). The percentile SOURCE is the swappable provider.
>
> **KEY FINDING this session (corrects a prior spec assumption):** the bake-region assembly spec
> claimed "the seam-safe macro + carve are seam-exact." Only the MACRO is — the carve runs a
> GLOBAL edge-to-edge Dijkstra, so INDEPENDENT per-region carves seam by ~3500 m (measured, the
> dominant component of the old ~1090 m condition-seam number, which was actually carve+gaussian+
> percentile combined). Fixed by super-region bake-then-slice (owner: "pillars" → the proven
> seam-exact model, not the deferred scale-contract-coupled core-local-anchored carve).
> Memory: `worldgen10-condition-seam-measured`, `worldgen10-standing-build-directives`.
>
> **Specs/plans (committed):** `docs/superpowers/specs/2026-06-05-wg10-region-fact-producer-integration-design.md`,
> `...specs/2026-06-05-wg10-seam-exact-smooth-percentile-conditioning-design.md`,
> `...plans/2026-06-05-wg10-region-fact-producer-integration.md` (Tasks 1-5,7-9),
> `...plans/2026-06-05-wg10-seam-exact-smooth-percentile-conditioning.md` (revised Task 6a-e).
>
> **DEFERRED (owner-gated, flagged honestly):** (1) OWNER VISUAL A/B — smooth-field vs
> per-region conditioned LOOK — before the carved look is declared "shipped" (numeric gates
> green now; the eye-check is the build-batch-then-visual-approve rhythm). (2) Super-region
> OUTER-border carve/gaussian seam (k× rarer; raise k or, long-term, the core-local-anchored
> carve once the player-to-world SCALE CONTRACT is settled). (3) Worker GPU-context reuse —
> currently rebuilds the RD+context per super-bake (correct + isolated; cache if super-bakes
> become frequent on the live path).

> **PRODUCER-WIRING DESIGN FORCED BY MEASUREMENT (2026-06-05).** Goal: wire
> `bake_region` into the live producer so the carved look reaches the screen (closes
> the un-intercept Rung-1 gap). The simple "all-CPU bake off-frame, cache on LRU, pages
> sample" design was MEASURED and does NOT fit: all-CPU `bake_region` over a
> `region_size_m=32768` region (~16 pages) = **~961 ms (513px) / ~3319 ms (1025px)** —
> the CPU macro (`mountain_seamsafe`, the GPU recipe's CPU twin) dominates; carve is only
> ~19 ms, condition ~2 ms. Seconds/region is too slow for synchronous OR background.
> **FORCED ARCHITECTURE (matches GPU/Rust-first principle): GPU macro (region) → ONE
> off-frame readback (bake_collision_region model) → CPU carve (~19 ms) + condition
> (~2 ms) → RegionFactRuntime (mirror StaticHeightRuntime) → pages sample.** This likely
> drops a region bake from ~3 s to tens of ms. Next session builds it (see handoff for
> concrete steps + the cross-region condition-seam boundary to validate). OWNER PRINCIPLE
> reaffirmed: anything still CPU-bound that can move to GPU/Rust appropriately MUST — the
> ~3 s all-CPU bake is the live example (macro is GPU-appropriate, was slow only because
> bake_region ran it on CPU).

> **WHOLE OFFLINE "BAKED LOOK" PIPELINE NOW IN RUST, END-TO-END PARITY (2026-06-05).**
> `wg-10/rust/src/bake_region.rs` assembles the seam-safe pipeline — macro
> (`recipes::mountain_seamsafe`) → carve (`carve_routes`→`carve_ramp_delta`, on the
> RAW field) → `raw+delta` → `condition_world` — the exact composition + ORDER of the
> accepted chunk-network look. End-to-end parity gate vs the Python seam-safe oracle
> (`bake_region_fixture.json`): **RAW bit-exact (2.2e-16), carve_delta bit-exact (0.0),
> condition stats bit-exact (0.0), final height mean 0.0035 m / p99 0.092 m** (the only
> residual is condition_world's documented reflect-border ring; interior bit-exact).
> So everything that made the mountain chunk networks look good — macro + connected
> carve + conditioning — now runs in Rust and matches Python.
> **The assembly gate also FIXED a latent bug in the already-shipped carve_ramp:** it
> used `gaussian_filter_nearest` but Python `carve_ramp` uses scipy `mode='reflect'`.
> The narrow carve_ramp fixture (half-width 1200 m) never exercised the border cells;
> the WIDE bake ramp (span-relative half=5400 m, flat=1620 m — itself a Task-1 catch:
> `carve_pass_network` uses SPAN-RELATIVE ramp widths, NOT CorridorParams defaults)
> exposed it. Added `array_ops::gaussian_filter_reflect`; carve_ramp is now FULLY
> bit-exact (the prior "0.48% EDT-tie residual" was the gaussian border mode all along).
> cargo lib **251/251** green. Spec/plan
> `docs/superpowers/specs/2026-06-05-wg10-bake-region-assembly-design.md` +
> `...plans/2026-06-05-wg10-bake-region-assembly-plan.md`. Pushed.
> **NEXT (the remaining integration, next session):** wire `bake_region` into the live
> producer (`Wg10PagePool`) as an off-frame region-fact bake + region-fact LRU + page
> sampling, with GPU-macro/CPU-carve coordination — this puts the carved look ON SCREEN
> and closes the un-intercept Rung-1 gap. KNOWN BOUNDARY to validate then: condition_world's
> per-region percentiles can differ across adjacent regions → potential cross-region
> conditioned-height seam (the macro+carve are seam-exact; only condition normalization
> varies by region).

> **ALL-11 BIOME PARITY CONFIRMED ON HARDWARE (2026-06-05).** "Biome-to-biome on
> parity with mountains" is now proven top-to-bottom, RUN GREEN this session, not
> just claimed:
> - **CPU recipe (Rust vs Python):** all 11 `<biome>_seamsafe_matches_python_oracle`
>   pass at eps=1e-9 (verified 12/12 incl. mountain-576). Landed in Slice-3.
> - **GPU (GLSL fragment vs f64 oracle):** `biome_page` suite ran green on RTX 5090
>   for ALL 11 biomes — maxd mountain 1.9e-6, grassland 6.8e-7, desert 1.3e-5,
>   coast 5.6e-6, wetland 2.5e-6, tundra 3.1e-7, glacial 1.8e-6, karst 2.0e-6,
>   temperate 1.7e-6, rainforest 3.2e-6, volcanic 3.0e-6 (all << eps=1e-4). Plus
>   primitive parity 48/48 (maxd 1.86e-4) and compose parity 12/12 (eps=1e-4).
> The carve (pass_network/carve_ramp) is mountain-only/world-layer, NOT a per-biome
> feature — so there is NO per-biome carve-port work; biome parity is COMPLETE at
> recipe + GPU + compose. Remaining is INTEGRATION (wire carve + condition_world
> into a region-fact bake feeding the live producer), not more parity.

> **CARVE FULLY PORTED TO RUST (2026-06-05) - BOTH HALVES PARITY-VERIFIED.**
> The connected pass-network carve — the feature that made the accepted mountain
> chunk-network LOOK, which had ALWAYS lived only in ~4s offline pure-Python and
> was never on the live path (THE divergence: the live seam-safe recipe dropped
> carving → "GPU but carveless") — is now fully in Rust under
> `wg-10/rust/src/pass_network/`:
> - **Routing** (Dijkstra least-cost paths): bit-exact vs Python, tamper-tested,
>   ~19 ms (206× faster than Python). Commits c694f16..f9cfd11.
> - **carve_ramp** (routes → walkable-valley height delta) + **EDT**
>   (`edt.rs`, exact separable Felzenszwalb-Huttenlocher with nearest-index,
>   brute-force-verified): tolerance-gated (owner-chose tolerance since the output
>   is Gaussian-smoothed + clamped) vs the Python `carve_ramp` oracle —
>   **mean 0.14 m, p99 0.0 m (99% of cells bit-identical)** on the 1700 m-relief
>   field; the 0.48% residual is provably EDT distance-TIE cells, not a bug (gate
>   fails at 2436 m with a stub). Reuses `array_ops::gaussian_filter_nearest`.
>   Commits 3d619bf..f9872e0. cargo lib **248/248** green.
> Specs/plans: `docs/superpowers/specs/2026-06-04-wg10-connected-carve-to-live-path-design.md`,
> `docs/superpowers/plans/2026-06-04-wg10-connected-carve-rust-port-plan.md`,
> `docs/superpowers/plans/2026-06-05-wg10-carve-ramp-rust-port-plan.md`.
> Branch `slice4-gpu-page-integration`, pushed to origin.
> **NEXT:** (a) bring ALL biomes to this same parity bar (each biome's recipe/carve
> ported to Rust + parity-gated, "biome-to-biome on parity with mountains"); then
> (b) wire a region-fact bake (macro GPU + condition_world + carve routing + ramp,
> all CPU off-frame, well under budget) into the live producer so the carved look
> reaches the screen + closes the un-intercept-ladder Rung-1 gap (live recipe was
> ~2× reference relief because it lacked carve + condition_world). condition_world
> port is cheap (~2 ms, per-region percentiles).

> **CARVE PORT TASK 5 (2026-06-04) - RUST CARVE COST MEASURED -> DELIVERY DECIDED.**
> The connected pass-network carve's routing (the ~4000 ms offline pure-Python
> Dijkstra) is now ported to Rust (`wg-10/rust/src/pass_network/`) and is
> parity-verified end-to-end (Task 4 GREEN: Rust routes == Python routes
> bit-faithfully, adversarially tamper-tested). Task 5 timed the ported routing
> in `--release`: at n=193 / coarse_n=193 (zoom-identity, so this is pure routing
> work — 8 routes, 2103 path points, non-vacuous) the measured Rust cost is
> **best 19.052 ms / median 19.443 ms** — vs ~4000 ms in Python, an **~206x
> speedup**. (Test: `pass_network::tests::measure_carve_cost_production_scale`;
> full lib suite 241/241 green.)
> **Delivery decision:** median (~19 ms) is comfortably under the ~50 ms bar, so
> the delivery backbone is a **synchronous off-frame region bake riding the
> existing page-pool LRU** (model: `facts_api.bake_collision_region`) — **no
> async job system needed**. Follow-on: port `carve_ramp`, wire the region-fact
> bake into the live producer.

> **CURRENT (2026-06-04) - SLICE 4 STABILIZATION / OWNER VISUAL + ARCHITECTURE DEBT.**
> Branch `slice4-gpu-page-integration`, with backup ref
> `backup-slice4-stabilize-before-crosslevel-20260604-0b0d8a0` created before this pass.
> **Final handoff for the next chat:** read
> `docs/plans/WG10_FINAL_HANDOFF_2026-06-05.md` first.
> **Latest owner-report audit:** see
> `docs/plans/WG10_IMPLEMENTATION_SPEC_AUDIT_AND_VALIDATION_PLAN_2026-06-04.md`.
> Current texture scope: final terrain textures have not been started. The
> accepted runtime bridge has a simple height/slope palette, debug modes, and
> low-resolution material fact masks for readability only. Do not treat palette
> polish as the next acceptance target; pass-network facts, generated mountain
> world-layer content, and facts/collision parity come first.
> Latest hitch fix: accepted/reference-backed material fact pages now stream at
> quarter height resolution (`page_px / 4`) instead of half resolution. Height
> pages remain full resolution; only the low-frequency RGBA material masks are
> cheaper. This removed the strict manual-stress failure where REFERENCE
> morph-off hit `cpu_max=22.436 ms`; the rebuilt run passes
> `review_runtime_stress` with all six REFERENCE/MOUNTAIN/WORLD morph off/on
> cases under the `16.7 ms` CPU p99/max and GPU p99 budgets, with zero
> hide/show/full events and exact bridge captures where modes are supposed to
> match. Serial post-fix gates: `review_runtime_modes` = 2/2,
> `review_runtime_visual` = 2/2, `review_progression` = 3/3, and targeted Rust
> `cargo test static_reference` = 10/10.
> Latest fix in this pass: the live clipmap now uses toroidal page slots in
> `terrain_view.rs`, so already-visible pages keep their mesh/material slot when
> the camera crosses page boundaries. This directly addresses the owner report
> that modes 1/2/3 felt slow and popped while moving. New progression proof first
> failed with `repage_frame_max=18` in all four steps; after the fix it passes
> with `repage_frame_max=8`, total visible repages down from `72` to `26`, zero
> hide/show, zero full events, and CPU p99 <= `13.665 ms` across REFERENCE,
> MOUNTAIN/network, MOUNTAIN/close-debug, and WORLD preview. The earlier shared
> fly-camera `sync_mouse_from_rotation()` method remains part of this recovery
> pass and prevents review-camera reframing from leaving mouse-look yaw/pitch
> stale.
> **Progression harness follow-up:** `wg10_progression_review.tscn` now exists
> with an explicit four-step current ladder: accepted REFERENCE baseline,
> reference-backed MOUNTAIN bridge, raw MOUNTAIN close-debug candidate, and
> bounded WORLD reference preview. `review_progression` = 3/3 now proves those
> steps report their expected runtime modes/contract kinds, survive scripted
> motion through page boundaries with bounded repage bursts, and pass a
> fixed-camera pixel-delta guard at L0/L1/L2 page-boundary crosses. Latest
> follow-up turns the scene into a machine-readable handoff and implements the
> first three review features: every active step emits a
> `source_display_report`, `material_fact_report`, and `pass_network_report`;
> the scene draws gated source/display, material-fact, and pass-network
> overlays; and the remaining procedural/facts-collision planned steps declare
> their labels, added contracts, proving gates, acceptance rules, and blocking
> promotion gaps.
> Latest post-fix proof: `review_progression` = 3/3, `review_runtime` = 2/2,
> `review_runtime_modes` = 2/2, `review_runtime_visual` = 2/2, and
> `review_runtime_stress` = 1/1 with CPU p99/max and GPU p99 capped at
> `16.7 ms`. The visual repage gate checked 12 before/after boundary pairs with
> worst mean/p95/p99 RGB delta `0.000831/0.002614/0.020915`. The visual bridge gate confirms
> REFERENCE, MOUNTAIN/network, and WORLD preview match where they are supposed
> to match; raw `MOUNTAIN/close_debug` and route/debug coloring remain
> prototype/diagnostic and are not accepted terrain.
> **Read this first:** latest scoped runtime-artifact checkpoint now has the
> owner fly binding the JSON-ready accepted mountain world-layer tile payload in
> `REFERENCE`, `MOUNTAIN/network_ref`, and `WORLD` preview. This replaces the
> runtime bridge's direct dependency on the review chunk JSON shape while
> keeping `mountain_network_chunks_review.tscn` as the visual comparison
> baseline. It builds on
> `2af7df4 fix(slice4): remove owner fly page settle`, tagged
> `backup-slice4-no-page-settle-20260604-2af7df4`, and
> `067b14b refactor(slice4): expose mountain world-layer runtime tile`, tagged
> `backup-slice4-runtime-world-layer-tile-20260604-067b14b`. The new tile
> payload/exporter separates the future Rust/Godot runtime producer/cache
> contract from the review chunk JSON shape; it writes flat row-major height,
> corridor, low-pass/floor/rock/snow fields with source/display mapping,
> pass-network facts, conditioning stats, and material summaries. The no-settle
> checkpoint disables the parent-to-fine newly-bound-page height fade that read
> as terrain lag/popping during owner fly movement. `REFERENCE` remains the
> accepted static mountain-network baseline streamed through the runtime page
> pool. `MOUNTAIN/network_ref` now matches that baseline through a
> reference-backed height/material/fact bridge
> (`single_mountain_world_layer_reference_bridge`,
> `height_source=bound_world_layer_reference_payload`,
> `procedural_world_layer_height=false`). This recovers the owner-visible
> mountain network look but does not complete final procedural biome synthesis.
> `WORLD` remains diagnostic until multi-biome composition is async/cached or
> given a cheaper preview contract.
> Latest checkpoint adds two stabilizers for the current owner report that
> modes 1/2/3 looked slow and wrong: bound world-layer references are now a
> distinct page-pool state wrapper rather than raw static-baseline state, and
> runtime review presentation is color-gated against the old static
> `mountain_network_chunks_review.tscn` focus view. The new visual guard reports
> `static_frac=0.789`, `runtime_frac=0.776`, `iou=0.984`, and
> `mean_color_delta=0.076` (budget `0.130`). Latest gates after the presentation
> fix: `fast` = 8/8, `review_runtime_modes` = 2/2,
> `review_runtime_visual` = 2/2, `review_runtime_stress` = 1/1, and focused
> `ring_material_tint_check.gd` passes. Scripted modes 1/2/3 still report zero
> hide/show/full events; latest motion CPU p99/max is REFERENCE 9.707/9.825 ms,
> MOUNTAIN 10.024/10.115 ms, WORLD 9.824/16.527 ms, and render GPU p99 is
> REFERENCE 0.750 ms, MOUNTAIN 0.750 ms, WORLD 0.743 ms. This fixes a concrete
> presentation/gate hole; it does not make raw procedural MOUNTAIN or full WORLD
> composition accepted content.
> Follow-up architecture split on 2026-06-04: the Godot-callable page-pool
> producer configuration API moved out of core `page_pool.rs` into
> `page_pool/config_api.rs`. Core `page_pool.rs` now keeps pool state, owner
> slots, and `free_all`; `config_api.rs` owns `configure`, `configure_biome`,
> `configure_biome_world`, and `configure_static_reference`. This is a
> behavior-preserving separation checkpoint for the current owner report that
> modes 1/2/3 still feel slow or visually wrong, not a final procedural-content
> fix. Proof after the split: `cargo fmt -p wg10_terrain -- --check`, focused
> `page_pool` tests = 18/18, full Rust lib tests = 233/233, `tools/build_rust.ps1`
> builds, `review_runtime` = 2/2, `review_runtime_modes` = 2/2, and
> `review_runtime_visual` = 2/2. Latest scripted mode numbers remain zero
> hide/show/full events with CPU p99 around 9.8 ms and render GPU p99 around
> 0.75 ms, so the next visual/perf step must reproduce the manual flight path
> rather than retuning a green scripted path.
> Follow-up manual-path gate tightening on 2026-06-04: `review_runtime_stress`
> now also compares the final rendered evidence frames for the same hand-style
> speed-pulse/stop/turn path. For both morph off and morph on, it asserts
> `REFERENCE` vs `MOUNTAIN/network_ref` and `REFERENCE` vs `WORLD` stay within
> the same sampled RGB bridge budgets used by the visual gate. Latest proof:
> `review_runtime_stress` = 1/1 with zero hide/show/full events across all six
> mode/morph cases, CPU p99 below 10 ms, GPU p99 below 0.64 ms, and bridge
> deltas `0.000000/0.000000` for MOUNTAIN and WORLD with morph off/on. This
> improves coverage for the manual "modes 1/2/3 feel slow/weird" report; it
> still does not make raw procedural MOUNTAIN or full WORLD composition accepted.
> Follow-up strict owner-spike budget on 2026-06-04: the same stress gate now
> fails any CPU p99/max or GPU p99 over `16.7 ms`. Latest pass keeps all six
> REFERENCE/MOUNTAIN/WORLD morph off/on cases inside that one-frame budget while
> preserving zero hide/show/full events and exact bridge captures where modes
> are supposed to match.
> Follow-up WORLD ownership split on 2026-06-04: `BiomeWorldRuntime` moved out
> of core `page_pool.rs` into `page_pool/world_runtime.rs`, with WORLD context
> construction and teardown owned beside the WORLD producer path. Current source
> size audit: `page_pool.rs` is 172 lines, `world_runtime.rs` is 35 lines, and
> the largest active Rust terrain source file is 566 lines, so the remaining
> architecture risk is no longer "1000-line files" in this slice; it is whether
> producer ownership, WORLD preview/full-compose taxonomy, and mountain
> fact/collision contracts stay explicit. Proof after the split:
> `cargo fmt -p wg10_terrain -- --check`, focused `page_pool` tests = 18/18,
> full Rust lib tests = 233/233, `tools/build_rust.ps1` builds, and
> `review_runtime` = 2/2.
> Follow-up report ownership split on 2026-06-04: page-pool reference binding,
> mountain-world contract taxonomy, and sampled static/reference reports are no
> longer bundled in one `static_reports.rs`. `world_layer_bindings.rs` now owns
> `bind_mountain_world_layer_reference(...)`, `bind_world_preview_reference(...)`,
> and the WORLD-preview reference predicate; `world_layer_contract.rs` owns
> `mountain_world_layer_contract_report()`; `static_reports.rs` now only owns
> sampled static/reference reports and helper sampling accessors. Current line
> counts: `static_reports.rs` 241, `world_layer_contract.rs` 152,
> `world_layer_bindings.rs` 58. Proof after the split:
> `cargo fmt -p wg10_terrain -- --check`, focused `page_pool` tests = 18/18,
> full Rust lib tests = 233/233, `tools/build_rust.ps1` builds, and
> `review_runtime` = 2/2.
> Current proof after the runtime-tile binding fix: `cargo test -p
> wg10_terrain --lib page_pool::static_reference::payload -- --nocapture` =
> 8/0, `cargo test -p wg10_terrain --lib` = 233/0,
> `tools\build_rust.ps1` builds, `fast` = 8/8, `review_runtime` = 2/2,
> `review_runtime_modes` = 2/2, `review_runtime_visual` = 2/2, and
> `review_runtime_stress` = 1/1. Latest mode gate reports zero
> hide/show/full events in REFERENCE, MOUNTAIN, and WORLD; scripted motion CPU
> p95/p99/max is REFERENCE 9.026/9.615/10.012 ms, MOUNTAIN
> 9.310/9.833/9.956 ms, WORLD 9.166/9.748/24.355 ms, with `acquired_max=1`
> and `full_events=0` in all three. Latest render p99 is REFERENCE 0.748 ms,
> MOUNTAIN 0.748 ms, WORLD 0.746 ms. The manual stress gate
> also passes six REFERENCE/MOUNTAIN/WORLD morph on/off movement cases with zero
> hide/show/full events and GPU p99 below 0.65 ms. The REFERENCE vs
> MOUNTAIN/network visual bridge still has sampled mean/p95 RGB delta
> 0.000000/0.000000 at the captured review frame. The visual gate also
> compares the same bridge along an 8000 m/s page-boundary path at frames
> 80/160/240, all now `0.000000/0.000000` mean/p95. The static-reference material page now
> preserves the accepted facts as RGBA channels (`low_pass/corridor`, `floor`,
> `rock`, `snow`) instead of collapsing them to a scalar class code. The material
> fact texture is intentionally lower resolution than height (`page_px / 4`) to
> keep synchronous owner-fly page misses under frame budget while preserving the
> low-frequency material story. The legacy
> `m3_accept` wall-time gate now initializes the shader globals it renders with,
> and passes at p99 2.41 ms in the full `m3` suite. The page transition fade is
> disabled for owner review, because the previous settle window read as terrain
> lag/settle during owner motion even when page residency was clean. The owner fly also now starts
> with procedural display detail disabled; `N` remains the explicit detail
> toggle. Modes 1/2/3 therefore open on the accepted reference presentation
> instead of all sharing the same synthetic close-surface noise layer.
> Follow-up review-control fix: `B` now cycles only the accepted owner-review
> lane (`REFERENCE` <-> `MOUNTAIN/network_ref`). `WORLD` and `LEGACY` remain
> direct-key diagnostics through `3` and `4`, so their known page-scale/legacy
> artifacts are no longer presented as part of the target visual loop.
> `review_runtime` now also gates the owner-mode taxonomy explicitly:
> `REFERENCE` is `accepted_visual_baseline`, `MOUNTAIN/network_ref` is
> `accepted_visual_bridge_not_final_procedural`, `WORLD` is
> `diagnostic_not_owner_accepted`, and `LEGACY` is
> `legacy_regression_not_accepted`. The WORLD diagnostic cap remains
> one active biome per page and is now visible in the owner-scene snapshot.
> Follow-up WORLD diagnostic guard: the owner snapshot now also exposes the
> live pool's center-page WORLD route report and sampled weight-field report.
> `review_runtime` proves the actual composed WORLD preview field is capped to
> one active biome over a 17x17 page sample (`active_biomes=1`,
> `max_texel_active_count=1`, normalized weights), so mode 3 cannot silently
> switch back to full multi-biome compose while still passing only taxonomy
> checks. This reinforces that WORLD remains diagnostic until compose is
> backgrounded/cached or given a cheaper preview contract.
> Follow-up source/display mapping gate: the Rust static-reference runtime now
> stores and reports the accepted runtime tile's explicit display/source
> mapping instead of only claiming `has_source_display_mapping=true`. The Godot
> smoke gate checks display origin `-38400,-38400`, display span `76800`, source
> origin `72000,41000`, source span `270000`, and
> `source_scene_ratio=3.515625` for the accepted bridge. Current proof:
> `cargo fmt -p wg10_terrain -- --check` passes,
> `cargo test -p wg10_terrain --lib page_pool::static_reference::payload -- --nocapture`
> = 8/8, `cargo test -p wg10_terrain --lib` = 233/233,
> `tools\build_rust.ps1` builds, `review_runtime` = 2/2,
> `review_runtime_modes` = 2/2, and `review_runtime_visual` = 2/2. Latest
> scripted motion has zero hide/show/full events in REFERENCE, MOUNTAIN, and
> WORLD; CPU p99/max is REFERENCE 9.729/10.079 ms, MOUNTAIN 10.038/10.133 ms,
> WORLD 9.817/16.880 ms, and render GPU p99 is REFERENCE 0.752 ms, MOUNTAIN
> 0.750 ms, WORLD 0.750 ms. The visual gate again proves REFERENCE vs
> MOUNTAIN/network and REFERENCE vs WORLD preview at mean/p95 RGB delta
> `0.000000/0.000000`.
> Follow-up transform ownership fix: `MOUNTAIN/network_ref` no longer carries
> duplicated source-scale/source-offset constants in the GDScript producer
> helper. `bind_mountain_world_layer_reference(...)` now derives
> `source = display * ratio + offset` from the accepted runtime tile mapping
> when the reference is bound. Current proof: Rust fmt check passes, payload
> Rust tests = 8/8, full Rust lib = 233/233,
> `tools\build_rust.ps1` builds, `fast` = 8/8, `review_runtime` = 2/2,
> `review_runtime_modes` = 2/2, and `review_runtime_visual` = 2/2. The latest
> scripted mode run still has zero hide/show/full events in REFERENCE,
> MOUNTAIN, and WORLD; CPU p99/max is REFERENCE 9.882/10.427 ms, MOUNTAIN
> 10.029/10.221 ms, WORLD 9.978/16.805 ms, and render GPU p99 is REFERENCE
> 0.751 ms, MOUNTAIN 0.748 ms, WORLD 0.748 ms. Visual bridge deltas remain
> `0.000000/0.000000`.
> Follow-up harness separation: the owner-fly runtime snapshot/report builder
> now lives in `wg-10/worldgen_terrain/harness/mountain_fly_snapshot.gd`.
> `mountain_fly_review.gd` still exposes the same `debug_runtime_snapshot()`
> API for gates, but the scene has moved closer to the DESIGN §6.4 rule that
> review scenes assemble components instead of containing diagnostic/report
> logic. This is behavior-preserving cleanup for the current slow/weird visual
> triage; it does not change any producer, shader, or acceptance result.
> Follow-up renderer-presentation fix: `ring_displace.gdshader` now applies
> accepted static-reference material pages as a softer presentation blend
> instead of a hard floor/rock/snow class replacement, and its manual
> directional/slope lighting is less contrasty. This targets the owner report
> that modes 1/2 had the right mountain footprint but still looked masky and
> faceted compared with the old `mountain_network_chunks_review.tscn`. Proof:
> `m3` = 10/10, `review_runtime_visual` = 2/2, and
> `review_runtime_modes` = 2/2. Latest scripted mode run still has zero
> hide/show in REFERENCE, MOUNTAIN, and WORLD with render p99 below 0.5 ms.
> Follow-up accepted-material fact-channel fix: the temporary scalar material
> code page has been replaced with a renderer-facing RGBA32F fact page:
> R=low-pass/corridor, G=floor, B=rock, A=snow. The shader samples those
> channels directly and blends separate terrain targets instead of decoding
> nearest class codes. The current owner-hitch recovery keeps the material
> fact texture at `page_px / 4` while height remains full resolution; current
> proof is listed at the top of this file. Original RGBA channel proof:
> `cargo test -p wg10_terrain --lib` = 231/0, `tools\build_rust.ps1` builds,
> `m3` = 10/10, `review_runtime_visual` = 2/2, and
> `review_runtime_modes` = 2/2. This dropped the failing scripted mode CPU p95
> from 17-18 ms to about 9.5 ms with zero hide/show and zero full events in
> modes 1/2/3.
> Follow-up WORLD owner-preview fix: `WORLD/network_ref` now keeps the
> `configure_biome_world` route/weight diagnostics live, but binds the accepted
> mountain reference payload for normal owner-facing height and material
> presentation. `terrain_view.rs` suppresses the normal WORLD route tint when
> that preview reference is bound; route colors remain available only through
> the explicit route-debug capture. This means modes 1/2/3 now share the same
> accepted reference presentation in normal review, while raw procedural WORLD
> compose remains separately gated by `biome_world` and is still not accepted
> terrain. Current proof after this checkpoint: `cargo test -p wg10_terrain
> --lib` = 227/0, `tools\build_rust.ps1` builds, `fast` = 8/8, `m3` = 10/10,
> `review_runtime` = 2/2, `review_runtime_modes` = 2/2,
> `review_runtime_visual` = 2/2, and `biome_world` = 1/1. Latest scripted
> motion has zero hide/show in REFERENCE, MOUNTAIN, and WORLD; CPU p99/max is
> REFERENCE 12.054/12.603 ms, MOUNTAIN 12.732/14.281 ms, WORLD
> 12.155/12.485 ms, with `acquired_max=1` and `full_events=0` in all three.
> Latest render p99 is REFERENCE 0.493 ms, MOUNTAIN 0.411 ms, WORLD
> 0.884 ms. `review_runtime_visual` proves REFERENCE vs MOUNTAIN/network at
> mean/p95 RGB delta 0.000000/0.000000 and now also proves REFERENCE vs
> WORLD/network preview at 0.000000/0.000000. Remaining owner reports of manual
> flight popping/lag need a capture that follows the exact manual path; the
> current scripted path does not reproduce hide/show or full-page stalls.
> Follow-up manual-flight checkpoint: `review_runtime_stress` now runs a heavier
> owner-flight path across REFERENCE, MOUNTAIN, and WORLD with morph off/on,
> speed pulses, stops, diagonal turns, viewport rendering, CPU/GPU timing,
> visible-tile churn checks, pool-full checks, terrain-fraction checks, and
> evidence PNGs under `D:/tmp/wg10_biome_compose`. The first stress captures
> exposed a real visual bug: the finite accepted static-reference payload was
> clamped outside its 76.8 km domain, so coarse clipmap pages smeared the last
> row/column into the horizon. `StaticHeightRuntime` now fades out-of-domain
> height samples to a low neutral floor and treats out-of-domain corridor/material
> hints as empty, so the finite reference no longer pretends to be infinite
> terrain. The owner fly review mesh is now 256 subdivisions per page; display
> detail is now opt-in through `N` for manual review. Proof after this checkpoint: `cargo test
> -p wg10_terrain --lib` = 229/0, `tools\build_rust.ps1` builds,
> `review_runtime` = 2/2, `review_runtime_modes` = 2/2,
> `review_runtime_visual` = 2/2, and `review_runtime_stress` = 1/1. Latest
> stress run still has zero hide/show, zero full events, `visible0=45/45`, CPU
> p99 about 12.1-12.4 ms, CPU max <= 13.3 ms, and GPU p99 about 0.62 ms across
> all six cases. Remaining visual debt is close-range terrain/content quality:
> the edge smear and measured stream pop are fixed, but the raw procedural
> mountain layer still needs the accepted pass-network/conditioning/material
> contract rather than more ad hoc renderer tuning.
> Follow-up owner-view reconfigure fix: rebuilding the owner fly producer after
> preset or relief changes now also reconfigures the live `Wg10TerrainView`.
> Before this, the page pool could be rebuilt for `network_ref` or
> `close_debug` while the actual clipmap view kept stale relief/morph settings.
> The owner snapshot now records `view.config_report()`, and
> `review_runtime` proves the actual view config for default REFERENCE,
> `MOUNTAIN/network_ref`, raw `MOUNTAIN/close_debug` (`relief_scale=0.25`,
> `relief_ref=425`), WORLD, LEGACY, and the restored network preset. Current
> post-fix proof: `tools\build_rust.ps1` builds, `review_runtime` = 2/2,
> `review_runtime_modes` = 2/2, and `review_runtime_stress` = 1/1. Latest mode
> and stress gates still report zero hide/show, zero full events, and
> `acquired_max=1` across modes 1/2/3. This fixes a harness/runtime state bug;
> it does not make the raw procedural mountain candidate accepted content.
> Follow-up review-state reset fix: mode/preset rebuilds now reset visual
> diagnostics back to normal material review state: debug mode `0`, culling
> enabled, display detail disabled, and default morph state except the explicit
> LEGACY diagnostic. The smoke gate deliberately dirties the scene into morph
> heatmap plus cull-disabled, switches mode, and proves the reset. Current
> proof: `review_runtime` = 2/2 and `review_runtime_modes` = 2/2; scripted
> modes 1/2/3 still have zero hide/show, zero full events, `acquired_max=1`,
> CPU max below 14 ms, and render p99 about 0.746 ms. This targets the class of
> owner reports where blue/yellow debug heatmaps or cull experiments made all
> modes look wrong.
> Superseded accepted-material presentation checkpoint: the interim soft scalar
> code page reduced blocky class slabs, but it has now been replaced by the RGBA
> fact-channel page described above. Keep the old result only as history: it was
> a renderer-presentation cleanup, not final procedural mountain synthesis.
> Current source-size audit: no Rust/GDScript/GLSL/Python source file under
> `wg-10/rust/src`, `wg-10/worldgen_terrain/harness`,
> `wg-10/worldgen_terrain/tests`, or `tools/dem_pack` is at or above 800 lines. The
> largest current source files found were `tools/dem_pack/export_godot_rough_world_chunks.py`
> at 745 lines, `mountain_world_chunks_review.gd` at 695 lines, and
> `tools/dem_pack/mountain_world_layer.py` at 602 lines. The remaining architecture
> risk is producer ownership, WORLD preview/compose taxonomy, and fact/collision
> alignment, not a single still-overgrown runtime file.
> Latest behavior-preserving page-pool split: WORLD active-limit and route/weight
> diagnostic reports moved from generic `state_api.rs` into
> `wg-10/rust/src/page_pool/world_reports.rs`. `state_api.rs` is now generic
> pool/source/page-state API at 169 lines; `world_reports.rs` owns the
> Godot-visible WORLD preview reports at 160 lines. Proof: `cargo test -p
> wg10_terrain --lib` = 227/0, `tools\build_rust.ps1` builds, `biome_world` =
> 1/1, and `review_runtime` = 2/2.
> Follow-up behavior-preserving page-pool split: WORLD route and weight-field
> producer adaptation moved from `wg-10/rust/src/page_pool/producer.rs` into
> `wg-10/rust/src/page_pool/world_producer.rs`. `producer.rs` now owns active
> producer classification, page dispatch, and static material refresh only
> (203 lines); `world_producer.rs` owns the configured WORLD runtime bridge to
> pure `world_route` math (82 lines). Public Godot methods and runtime behavior
> are unchanged. Proof: `cargo test -p wg10_terrain --lib` = 227/0,
> `tools\build_rust.ps1` builds, `biome_world` = 1/1, and `review_runtime` =
> 2/2.
> Latest owner-visible repage presentation fix: `clipmap_rings.rs` now disables
> the newly-bound-page height settle window for the owner fly. The streamer
> already preloads ahead, and the old parent-to-fine fade read as terrain
> lagging/popping while moving even when hide/show/full-event counters were
> clean. Proof after the no-settle patch: `review_runtime_modes` = 2/2 with
> zero hide/show/full events in REFERENCE, MOUNTAIN, and WORLD; latest CPU p99
> is REFERENCE 10.046 ms, MOUNTAIN 9.921 ms, WORLD 9.952 ms. Render p99 remains
> below 0.75 ms in all three modes. `review_runtime_stress` = 1/1 across six
> morph on/off movement cases with zero hide/show/full events, and
> `review_runtime_visual` = 2/2 with REFERENCE vs MOUNTAIN/WORLD preview still
> pixel-identical in the owner visual bridge.
> Follow-up visual bridge guard: `review_runtime_visual` now also runs
> `mountain_runtime_reference_static_compare.gd`, which captures the old static
> `mountain_network_chunks_review.tscn` focus view and the runtime REFERENCE
> bridge under matching focus framing. It compares terrain masks rather than
> colors because the renderers/materials differ. Latest proof: static
> terrain_frac `0.789`, runtime terrain_frac `0.778`, mask IoU `0.987`. This
> proves the live REFERENCE bridge preserves the owner-liked accepted footprint;
> remaining visual complaints are renderer/material/procedural-content quality,
> not a wrong reference scale or camera/window.
> Follow-up owner-visual fix on 2026-06-04: `mountain_fly_review.tscn` now starts from
> an accepted-reference camera frame instead of near-surface origin, and `G` reframes to
> that view during review. Runtime color normalization is now producer-owned:
> REFERENCE normalizes material color against displayed 1700 m relief,
> MOUNTAIN/network against displayed 1700 m, and WORLD/close-debug against 425 m, so low-relief
> modes no longer collapse into one washed-out palette. The review camera/fog now uses
> the accepted mountain-network 76.8 km visual footprint while the streamer still keeps
> the larger 196.608 km loaded edge for fallback coverage; this avoids showing
> static-reference samples beyond the accepted payload as horizon artifacts. REFERENCE
> static material pages are blended into terrain shading (`0.58`) instead of replacing
> the palette outright, and `mountain_fly_review_smoke_check.gd` now proves the owner
> scene has bound those material page textures.
> Follow-up refactor/proof on 2026-06-04: the first static-reference separation pass
> moved JSON payload schema, validation, stitching, material-hint validation, and payload
> tests into `wg-10/rust/src/page_pool/static_reference/payload.rs`. The remaining
> `static_reference.rs` now owns runtime sampling and page texture upload. Current proof:
> `cargo test -p wg10_terrain --lib` = 227/0, `tools\build_rust.ps1` builds the
> Godot-facing extension, `fast` = 8/8, `review_runtime` = 2/2,
> `review_runtime_modes` = 2/2, and `review_runtime_visual` = 1/1.
> The fresh visual captures confirm the owner concern: performance/churn gates are green,
> but mode 2 (`MOUNTAIN`) is still a raw live recipe candidate without the accepted
> pass-network/material/facts world-layer contract, and mode 3 (`WORLD`) still exposes
> page-scale composition/LOD boundaries. Those are architecture/content-port issues, not
> page-pool speed failures.
> Follow-up WORLD diagnostic audit: the owner fly WORLD mode is intentionally capped at
> one active biome per page until multi-biome height composition moves off the synchronous
> streamer path. Testing top-2/full WORLD compose in the owner fly removed the one-biome
> shortcut but produced ~1.9 s `review_runtime_modes` update hitches (`WORLD cpu_max`
> about 1900-1950 ms), even with WORLD flow disabled. Restored bounded WORLD diagnostic
> mode passes again: latest `review_runtime_modes` = 2/2 with WORLD `cpu_p99=7.686 ms`,
> `cpu_max=10.050 ms`, zero hide/show, and render p99 `0.505 ms`. Do not treat mode 3
> as accepted visual terrain until WORLD composition is backgrounded, cached, or replaced
> by a cheaper preview contract.
> Follow-up page-pool separation on 2026-06-04: active producer classification and page
> dispatch moved into `wg-10/rust/src/page_pool/producer.rs`. `acquire.rs` now owns
> page policy/slot acquisition/rollback only, and the redundant `use_biome_path` field was
> removed; `uses_biome_path()` is derived from the active producer kind. Current proof:
> `cargo test -p wg10_terrain --lib` = 227/0, `tools\build_rust.ps1` builds,
> `fast` = 8/8, `review_runtime` = 2/2, `review_runtime_modes` = 2/2, and
> `biome_world` = 1/1. Latest mode gate still reports zero hide/show; WORLD stays bounded
> diagnostic with `cpu_p99=8.723 ms`, `cpu_max=10.612 ms`, render p99 `0.216 ms`.
> Follow-up static-reference separation on 2026-06-04: runtime sampling moved into
> `wg-10/rust/src/page_pool/static_reference/sampling.rs`, and renderer-facing material
> code projection moved into `wg-10/rust/src/page_pool/static_reference/presentation.rs`.
> `static_reference.rs` is now a 115-line facade/runtime holder instead of a mixed
> payload/sampling/presentation file. Current proof: `cargo test -p wg10_terrain --lib`
> = 227/0, `tools\build_rust.ps1` builds, `fast` = 8/8, `review_runtime` = 2/2,
> `review_runtime_visual` = 1/1, and `review_runtime_modes` = 2/2. Latest scripted
> motion numbers: REFERENCE `cpu_p99=34.640 ms`, MOUNTAIN `cpu_p99=9.161 ms`,
> WORLD `cpu_p99=8.122 ms`, zero hide/show in all three; render p99 is
> REFERENCE `0.234 ms`, MOUNTAIN `0.247 ms`, WORLD `0.488 ms`.
> Fresh captures still match the owner report: mode 1 is an accepted static bridge,
> mode 2 is the live producer to fix, and mode 3 remains a diagnostic WORLD preview
> until multi-biome composition is made async/cached or given a cheaper preview
> contract.
> Follow-up static-reference payload split on 2026-06-04: the runtime-tile schema
> and loader moved into
> `wg-10/rust/src/page_pool/static_reference/payload/runtime_tile.rs`, and the
> payload-focused tests moved into
> `wg-10/rust/src/page_pool/static_reference/payload/tests.rs`. The remaining
> `payload.rs` owns the accepted review-payload schema, validation helpers, and
> old chunk stitching. Current split sizes: `payload.rs` 439 lines,
> `payload/runtime_tile.rs` 279 lines, and `payload/tests.rs` 276 lines. Current
> proof: `cargo test -p wg10_terrain --lib page_pool::static_reference::payload
> -- --nocapture` = 8/0 and `cargo test -p wg10_terrain --lib` = 233/0.
> Follow-up report separation on 2026-06-04: accepted-baseline report surfaces moved
> into `wg-10/rust/src/page_pool/static_reports.rs`. This owns
> `mountain_world_layer_contract_report()`, `static_reference_report()`,
> `static_reference_page_report(...)`, and the static page-fact helpers consumed by
> the terrain view. `state_api.rs` dropped from 549 to 349 lines and now carries
> generic pool state plus WORLD route diagnostics instead of the accepted static
> baseline facts. Current proof: `cargo test -p wg10_terrain --lib` = 227/0,
> `tools\build_rust.ps1` builds, `fast` = 8/8, `review_runtime` = 2/2,
> `review_runtime_visual` = 1/1, and `review_runtime_modes` = 2/2. Latest scripted
> motion numbers: REFERENCE `cpu_p99=34.839 ms`, MOUNTAIN `cpu_p99=9.369 ms`,
> WORLD `cpu_p99=7.792 ms`, zero hide/show in all three; render p99 is
> REFERENCE `0.326 ms`, MOUNTAIN `0.251 ms`, WORLD `0.470 ms`.
> Follow-up WORLD report separation on 2026-06-04: WORLD active-limit and
> route/weight diagnostic reports moved into
> `wg-10/rust/src/page_pool/world_reports.rs`. Public Godot method names are
> unchanged (`set_biome_world_active_limit`, `debug_world_biome_for_page`,
> `debug_world_biome_report_for_page`,
> `debug_world_biome_weight_field_report_for_page`), but generic pool
> `state_api.rs` no longer owns WORLD-only preview diagnostics. Current proof:
> `cargo test -p wg10_terrain --lib` = 227/0,
> `tools\build_rust.ps1` builds, `biome_world` = 1/1, and
> `review_runtime` = 2/2.
> Follow-up live-MOUNTAIN fact bridge on 2026-06-04: `MOUNTAIN/network_ref`
> now binds the accepted mountain world-layer payload as a separate
> fact/material reference beside the live single-biome producer. The bridge
> exposes pass-network, carving, page-stable conditioning, corridor, and
> material-hint facts in `mountain_world_layer_contract_report()` plus
> page-sampled reference reports, and the live renderer can consume those bound
> material pages. This is not yet the final visual fix: live GPU height still
> comes from the seam-safe page recipe and `height_consumes_world_layer_facts`
> remains false, so the contract still reports
> `satisfies_mountain_world_layer_contract=false`. Current proof:
> `cargo test -p wg10_terrain --lib` = 227/0, `tools\build_rust.ps1` builds,
> `fast` = 8/8, `review_runtime` = 2/2, `review_runtime_visual` = 1/1, and
> `review_runtime_modes` = 2/2. Latest scripted motion: REFERENCE
> `cpu_p99=31.273 ms`, `cpu_max=40.549 ms`; MOUNTAIN `cpu_p99=25.059 ms`,
> `cpu_max=46.348 ms`; WORLD `cpu_p99=9.033 ms`, `cpu_max=10.894 ms`; zero
> hide/show in all three. Latest render p99 is REFERENCE `0.233 ms`, MOUNTAIN
> `0.247 ms`, WORLD `0.216 ms`.
> Follow-up visual recovery on 2026-06-04: `MOUNTAIN/network_ref` now uses the
> bound mountain world-layer payload for height as well as material/fact pages,
> while still reporting runtime=`single` and `biome_path=true`. Its contract kind
> is `single_mountain_world_layer_reference_bridge`, with
> `height_source=bound_world_layer_reference_payload` and
> `procedural_world_layer_height=false`; do not count this as final procedural
> biome synthesis. The latest capture shows MOUNTAIN/network matching the
> REFERENCE view at the reviewed frame. Current proof: `cargo test -p
> wg10_terrain --lib` = 227/0, `tools\build_rust.ps1` builds, `fast` = 8/8,
> `review_runtime` = 2/2, `review_runtime_visual` = 1/1, and
> `review_runtime_modes` = 2/2. Latest scripted motion: REFERENCE
> `cpu_p99=31.577 ms`, `cpu_max=40.887 ms`; MOUNTAIN `cpu_p99=35.781 ms`,
> `cpu_max=42.245 ms`; WORLD `cpu_p99=8.431 ms`, `cpu_max=19.247 ms`; zero
> hide/show in all three. Latest render p99 is REFERENCE `0.232 ms`, MOUNTAIN
> `0.367 ms`, WORLD `0.216 ms`.
> Follow-up guard on 2026-06-04: `review_runtime_visual` now compares the
> REFERENCE capture against the reference-backed `MOUNTAIN/network_ref` capture
> and fails on drift. Latest proof: 57,600 sampled pixels at stride 4,
> mean RGB delta `0.000000`, p95 RGB delta `0.000000` against budgets
> `0.002500` / `0.020000`.
> Architecture baseline note: `docs/plans/WG10_ARCHITECTURE_BASELINE_AUDIT_2026-06-04.md`
> records the current split between the owner-liked static mountain network chunk review
> (`mountain_network_chunks_review.tscn`) and the current live GPU biome fly
> (`mountain_fly_review.tscn`). Treat the former as the mountain visual/content baseline and
> the latter as the streaming producer/renderer proving scene until the live runtime is
> configured to reproduce the same world/scale assumptions.
> Runtime mountain-world target: `docs/plans/MOUNTAIN_WORLD_LAYER_RUNTIME_CONTRACT_2026-06-04.md`
> defines the required live producer facts: explicit source/display mapping, mountain
> macro field, connected pass-network routes, route carving before conditioning,
> page-stable conditioning, material/dressing hints, and a facts/collision story.
> The first numeric mountain-layer gap probe now exists:
> `tools/dem_pack/test_mountain_world_layer_contract.py`. It proves the accepted
> network payload contract through the tracked
> `tools/dem_pack/mountain_world_layer.py` source module. When the generated
> local review payload is present, it measures the current live seam-safe
> producer against that accepted layer over the same mapped page:
> `mean_abs=1.211743`, `p95_abs=2.276974`, `peak_abs=3.200543`,
> `corr=-0.048456`. This confirms the remaining mismatch is producer-contract
> work, not a stale DLL, wrong command, or relief scalar issue.
> `export_godot_mountain_network_chunks.py` is now a thin writer around that
> module, removing the previous hidden dependency on the untracked
> `export_godot_mountain_world_chunks.py` helper.
> Runtime bridge follow-up: `Wg10PagePool.static_reference_report()` now parses
> and exposes the accepted payload's generator version, source scope, height
> scale, feature span, corridor coverage, and pass-network route/carve summary.
> `mountain_fly_review_smoke_check.gd` gates those facts when switching to
> REFERENCE mode, so the accepted bridge is now a named mountain-world-layer
> contract rather than an anonymous static height texture. Follow-up
> `Wg10PagePool.static_reference_page_report(...)` samples corridor coverage
> over a runtime page, the REFERENCE renderer applies a restrained corridor
> tint/material mix from that page-level fact, and the smoke gate verifies the
> page report exists. The accepted Python world-layer builder also now emits
> four page-stable material hint fields per chunk (`low_pass_hint`,
> `floor_hint`, `rock_hint`, `snow_hint`) plus a world summary; these are
> contract/fact fields for the runtime port, not final per-pixel materials.
> Follow-up bridge on 2026-06-04: the Rust static-reference loader validates
> those four hint arrays as an all-or-none payload contract, exposes whole-payload
> and page-sampled hint coverage through `static_reference_report()` /
> `static_reference_page_report(...)`, and REFERENCE rendering uses page-level
> hints for material color/mix instead of collapsing the accepted payload to a
> corridor-only tint.
> Follow-up contract audit: `Wg10PagePool.mountain_world_layer_contract_report()`
> now exposes the active producer's mountain-world-layer facts in one place.
> `review_runtime` gates that `REFERENCE` is the accepted static visual baseline,
> live `MOUNTAIN` is only the explicit seam-safe page-recipe candidate, WORLD and
> LEGACY are not mountain-network producers, and no current mode claims full live
> mountain-world-layer contract satisfaction.
> Follow-up world-layer seam: `tools/dem_pack/mountain_world_layer.py` now owns
> the accepted source/display mapping and runtime-page sampler
> (`source_origin_for_display`, `sample_world_page`, `sample_payload_page`).
> The focused contract test proves display `0,0` maps to source
> `207000,176000`, accepted height/material fields sample through the shared
> seam, and the live seam-safe page still has the same measured gap. This is the
> first concrete CPU/generated world-tile seam for the later Rust/GPU port.
> The June 3 scale-invariance chain is implemented through the GPU producer plumbing: Python
> oracle world-anchoring + regenerated fixtures, Rust parity, flow-off macro oracle, per-level
> runtime kernel anchoring, and `flow_max_level` are committed. Latest Rust proof:
> `cargo test -p wg10_terrain --lib`
> = **227 passed / 0 failed** after the owner-visual review fix.
>
> Editor-closed/windowed hardware gates on 2026-06-04:
> `review_static` = **1/1 pass** (the accepted `mountain_network_chunks_review.tscn` baseline
> loads), `review_static_visual` = **1/1 pass** (captures the accepted static
> baseline PNGs), `review_runtime` = **2/2 pass** (instantiates the owner
> `mountain_fly_review.tscn` path, verifies accepted `REFERENCE` startup plus
> the explicit `MOUNTAIN/network_ref` candidate, and runs the sprint-speed
> visibility churn gate),
> `review_runtime_visual` = **2/2 pass** (captures REFERENCE/static-payload,
> MOUNTAIN/network, MOUNTAIN/close, WORLD/material, and WORLD/route PNGs through
> the shared producer helper and compares the static accepted focus mask against
> runtime REFERENCE),
> `m3` = **10/10 pass** after the display/prefetch scheduler split
> (`m3_accept` p99 5.25 ms / 6.0 ms budget), `review_runtime_modes` = **2/2 pass**
> after the owner-visual fix (REFERENCE/MOUNTAIN/WORLD zero hide/show; render p99
> REFERENCE 0.358 ms, MOUNTAIN 0.248 ms, WORLD 0.492 ms at 1280x720), and
> `biome_fly` = **4/4 pass**
> (macro 576 maxd 2.3156e-5 <= 5e-4, full 576 maxd 0.001471 <= 0.002,
> cross-level macro ratio 0.066665 <= 0.08, latest fly GPU p99 0.177 ms).
> Correct command sequence for Godot-facing Rust rebuilds is
> `powershell -ExecutionPolicy Bypass -File tools\build_rust.ps1` from the repo
> root, then set `GODOT_BIN` to the Godot 4.6.2 console executable and run
> `python tools\gate.py --suite biome_fly`,
> `python tools\gate.py --suite review_runtime`, and
> `python tools\gate.py --suite review_runtime_visual` from the repo root. Avoid
> raw Cargo target-dir overrides for Godot review; they can build a DLL outside
> the `.gdextension` load path.
> `mountain_fly_review.tscn` now starts in `REFERENCE` mode so owner review opens
> on the accepted mountain-network payload. The explicit `MOUNTAIN/network_ref`
> candidate remains available through `2`/`B` on the accepted `network_ref` scale
> (`feature_span_m=90000`) and exposes `P` to toggle the old `close_debug` scale
> (`feature_span_m=3500`). That candidate uses the accepted mountain-network seed
> family (`runtime_seed=177`), `relief_m=1700`, a MOUNTAIN/network-only view
> relief scale of `1.0`, and the accepted source-window transform
> (`source_scale=3.515625`, source center `207000,176000`); `review_runtime`
> proves runtime=`single`, biome_path=`true` after switching to MOUNTAIN.
> `WORLD` remains available through direct key `3` as the biome-composition
> diagnostic path, not through the owner-review `B` cycle.
>
> Follow-up runtime architecture fix: `mountain_fly_review.tscn` now has four
> explicit producer modes: `REFERENCE` (default), `MOUNTAIN`, `LEGACY`, and
> `WORLD`; `B` cycles only the accepted `REFERENCE`/`MOUNTAIN` review lane, and
> direct keys expose every mode while the HUD/log prints the active mode. `REFERENCE`
> calls `configure_static_reference(...)`, stitches
> `mountain_network_chunks.json` into a 1153x1153 accepted height field, and
> uploads sampled R32F pages through the same `Wg10PagePool`/clipmap renderer
> with view relief scale 1.0. This is a renderer/content-baseline bridge, not a
> replacement for the live biome recipe. `WORLD` calls
> `configure_biome_world(...)`, loads the pack grammar without resolving the
> legacy kernel atlas, builds cached GPU contexts for the 11 currently ported
> biome fragments plus a cached compose context, generates a texel-corner
> runtime-biome weight field per page, dispatches each active GPU biome recipe,
> and folds the resulting core fields through the GPU compose passes before
> writing the page texture. This removes the old whole-page dominant-biome
> selector from the live WORLD fly, but it is still
> not badlands-native (badlands falls back to desert), not per-biome material
> complete, and not the Slice 4c atlas-removal/runtime-flip acceptance.
> `tools\build_rust.ps1` is the Godot-facing DLL build command and passes. The
> new `biome_world` windowed gate is **1/1 pass** when run outside
> the sandbox (`python tools\gate.py --suite biome_world`): runtime=`world`,
> `biome_path=true`, route diversity across the sampled page window includes
> rainforest/wetland/tundra/volcanic/temperate/desert/grassland/coast/mountain/glacial/karst,
> nonzero=65536, min=-1633.198242, max=842.125427. Important run rule: Godot
> gates must run outside the filesystem sandbox because the sandbox cannot write
> `user://` AppData logs and Godot crashes before scripts run.
>
> Visual renderer fix in this pass: the live streaming shader no longer uses the
> blue/yellow height-debug ramp as the normal material. `ring_displace.gdshader`
> now derives a lit terrain palette from displayed height, slope, and world-space
> detail, while `M` cycles material -> morph heatmap -> WORLD route-color
> diagnostic. WORLD pages now also get a restrained route-color material tint in
> normal mode (`biome_material_mix=0.34`, gated by `ring_material_tint_check.gd`),
> so composed WORLD no longer reads as one undifferentiated mountain palette. This
> is a visual-readability bridge, not final per-pixel biome material blending.
> Historical note, superseded by
> `7e0fb98 fix(slice4): recover mountain network visual bridge`: the following
> live-MOUNTAIN mismatch described the raw recipe before MOUNTAIN/network_ref
> became reference-backed for height/material/facts.
> Follow-up shader fix: the palette now uses the same displayed height as
> `VERTEX.y` (`(h + detail) * relief_scale`) instead of coloring against unscaled
> page metres. This removed the misleading snow/gray wash. The follow-up producer
> calibration also moved live MOUNTAIN/network to the accepted seed/relief family,
> and the source-transform fix now makes it synthesize from the accepted 270 km
> source window while displaying over the normal 76.8 km review footprint. The
> capture now shows dense mountain-scale relief instead of the earlier flat carpet.
> It still reads as raw/faceted live page content and does not reproduce the
> accepted static pass-network artifact. Presentation relief experiments
> (`RELIEF_SCALE=0.5` and `1.0`) remain rejected because global display-scale tuning
> breaks close-debug/WORLD captures by driving the camera into terrain or creating
> foreground spikes.
> Follow-up baseline bridge: the new REFERENCE runtime capture proves the
> runtime renderer can show the accepted mountain-network geometry when fed the
> accepted payload. The explicit live MOUNTAIN capture now uses the same seed,
> relief family, and source-window scale as that payload, so the remaining mountain
> mismatch is isolated to the producer contract: the accepted payload was generated
> by the old full-field diagnostic branch (`apron_px=0`, field-level zscore/norm,
> connected pass-network carving, and whole-field percentile/tanh conditioning)
> before slicing, while the live runtime uses the seam-safe page branch with fixed
> affine constants, scale-anchored kernels, flow-level gating, and no pass-network
> fact. This is not a command invocation or relief scalar issue.
> The accepted world-layer builder now carries the next material/fact seam:
> `tools/dem_pack/mountain_world_layer.py` derives low/pass corridor, floor,
> rock/slope, and snow/high hint fields over the coherent conditioned field
> before slicing. Focused proof:
> `python -m pytest tools\dem_pack\test_mountain_world_layer_contract.py -q -s -p no:cacheprovider`
> = **5 passed** and still records the live seam-safe gap
> `mean_abs=1.211743`, `p95_abs=2.276974`, `peak_abs=3.200543`,
> `corr=-0.048456`.
>
> Runtime motion fix: the scheduler now maintains a camera-centred display ring
> plus a velocity-led prefetch ring, and `Wg10TerrainView` displays only the
> camera-centred ring. This turns stream-ahead into actual prefetch instead of
> exposing the led ring as soon as it crosses a page boundary. `Wg10Streamer`
> now exposes `display_keys(...)` so gates assert never-black on visible pages
> while allowing prefetch pages to be missing briefly. New gate
> `mountain_fly_visibility_churn_check.gd` is wired into `review_runtime` and
> passed on hardware: `frames=360 speed=8000 stream_events=24 resident=69
> repage=72 hide=0 show=0 hidden_frames=0 max_hidden=0`. This automates the
> forward-motion hide/show pop proof; owner re-fly is still required for visual
> acceptance and content quality.
>
> Pop-in audit evidence: `biome_world` still reports child/parent route disagreement
> for the old page-center route diagnostic. Current windowed result: `lod_route_mismatch=183/867`
> (`ratio=0.211073`). That means about 21% of sampled fine pages route to a different
> biome than at least one coarser fallback/morph parent under a single-biome selector.
> The live WORLD producer now composes per-page weight fields instead of using that
> selector, but `mountain_fly_review.tscn`
> now prints the routed biome per clipmap level in the yellow debug HUD and can show
> route colors with `M`, so the owner fly can correlate any remaining visible pops
> with route changes and streaming state.
> This is evidence for why the selector was wrong, not proof that the composed live
> path is visually accepted.
> The WORLD routing helper is now split into `page_pool/world_route.rs` and
> `biome_world` also reports page-center route-weight loss: current windowed result
> `route_weights samples=289 multi_active=201 ambiguous=0 max_active=4 mean_top=0.966506
> weakest_top=0.915909 mean_runner_up=0.031516 max_runner_up=0.084091`. So most sampled
> pages have more than one active runtime-biome weight, but the current grammar grid
> still has a very strong dominant biome at each page center. This is retained as
> evidence for why the previous selector discarded valid active weights; the live
> WORLD producer now consumes the weight field through compose.
> Follow-up in-page route probe: current windowed `biome_world` reports
> `route_inpage corner_mixed=0 max_corner_mismatches=0 max_probe_active=4
> weakest_probe_top=0.711914 max_probe_runner_up=0.288086`. In this sampled window,
> page footprints are not crossing dominant-route boundaries at their corners, but
> corners can have material runner-up weights. This further narrows the current
> forward-motion artifact: the hard evidence points first at cross-LOD route changes,
> and the composed runtime needs a fresh owner fly to determine what remains visible.
> New parent/child route breakdown from `biome_world`: `lod_route_by_parent
> L1=0/289(0.000000) L2=63/289(0.217993) L3=120/289(0.415225)
> stable_child_mismatch=183`. Complete child scans inside sampled parents report
> `route_parent_child parents=243 mixed=153 child_mismatch=2472/6804
> parent_absent=0 max_child_routes=6`, with L3 at `mixed:79/81`. So the pop-in
> mechanism is now precise: level-1 parent routing is stable in this sample, but
> coarser parents often contain multiple legitimate fine-page biome routes. A
> single selected parent biome cannot represent those children without throwing
> away real world variation; the aligned fix is runtime per-pixel grammar weights
> feeding compose, not forcing children to inherit a coarser page route.
> Runtime compose bridge now exists in Rust: `page_pool/world_route.rs` generates
> a texel-corner runtime-biome weight field from `Pack + supported biome predicate`,
> and `compute_biome_world_page_composed(...)` copies recipe core buffers into a
> cached compose context before cropping the composed height to the live page
> texture. Current `biome_world` proof line: `route_weight_field samples=289
> active_biomes=2 max_texel_active=2 min_sum=1.000000 max_sum=1.000000
> max_sum_delta=0.000000`, followed by `status=pass runtime=world biome_path=true
> nonzero=65536`.
> The accepted static network baseline now has a direct visual capture harness:
> `worldgen_terrain/tests/mountain_network_visual_capture.gd`. It runs the static
> `mountain_network_chunks_review.tscn` in a windowed `SubViewport` and writes
> `D:/tmp/wg10_biome_compose/mountain_network_static_focus_capture.png` plus
> `D:/tmp/wg10_biome_compose/mountain_network_static_overview_capture.png`
> (`chunks=9`, `feature_span_m=90000`, `1280x720`). It is now wired as the
> `review_static_visual` gate suite. These are comparison evidence for the
> owner-liked offline artifact, not proof that the live runtime matches it.
> The standalone visual capture now writes five runtime artifacts:
> `D:/tmp/wg10_biome_compose/biome_mountain_reference_fly_capture.png`,
> `D:/tmp/wg10_biome_compose/biome_mountain_network_fly_capture.png`,
> `D:/tmp/wg10_biome_compose/biome_mountain_close_fly_capture.png`,
> `D:/tmp/wg10_biome_compose/biome_world_fly_capture.png`, and
> `D:/tmp/wg10_biome_compose/biome_world_fly_capture_routes.png`. Current evidence:
> REFERENCE streams 45 pages from `mountain_network_chunks.json` and visibly
> restores the accepted mountain massifs through the runtime renderer.
> MOUNTAIN/network_ref now streams the same accepted mountain world-layer payload
> through the live single-producer mode as a reference-backed height/material/fact
> bridge; the latest capture matches REFERENCE at the reviewed frame and the
> contract reports `procedural_world_layer_height=false` so it cannot be mistaken
> for final procedural synthesis.
> MOUNTAIN/close_debug streams 45 pages but is visibly faceted/lumpy at close range,
> so it remains a diagnostic scale, not an acceptance target.
> WORLD streams 45 composed pages and route colors are visible; the normal WORLD
> material now carries a route-color tint, but final per-pixel materials/content
> remain open. Treat the capture as evidence of a less samey live read, not as
> proof of accepted biome visuals.
>
> **Still not accepted / do not claim done:** T7 owner re-fly of `mountain_fly_review.tscn`
> is pending after the reference-backed visual bridge. The forward-motion hide/show pop now has
> an automated zero-hide runtime gate, and the latest capture shows the reviewed
> MOUNTAIN/network bridge matching REFERENCE, but the owner still needs to fly
> the scene to judge remaining visual quality, terrain content, and any non-hide
> artifacts. Per-biome materials/content still need review. Slice 4c is also still open: runtime default flip,
> atlas-removal audit, hardened perf gate, and owner acceptance are pending.
> Facts/collision still rely on the legacy `height.rs` path until a follow-up facts story is
> designed or explicitly exempted.
>
> **Refactor state:** the former 3.6k-line `biome_page_compute.rs` has been split into
> focused modules; current Rust hotspots are now mostly recipe-local (`recipes_glacial.rs` 452,
> `biome_page_compute/local_compose.rs` 439, `recipes_desert.rs` 434, `recipes_karst.rs` 428).
> The live fly harness has started that separation: `mountain_fly_review.gd` now delegates
> producer modes, scale presets, relief, and pool configure calls to
> `mountain_fly_producers.gd`, with `mountain_fly_producers_check.gd` in the `fast`
> suite to lock B/P/R state transitions. `review_runtime` now gates the actual owner
> scene startup too, catching GDScript/Rust call-signature drift and default-preset drift.
> `biome_fly_capture.gd` also uses `mountain_fly_producers.gd`, so runtime visual
> evidence can no longer drift from the owner scene's producer constants/configure calls.
> Runtime renderer constants are now split into `mountain_fly_runtime_config.gd` and
> locked by `mountain_fly_runtime_config_check.gd`; the owner scene and runtime visual
> capture share levels, span, lead, morph/detail defaults, fog/loaded edge, shader path,
> and view configuration. The owner scene now also has direct architecture-mode
> keys: `1` REFERENCE accepted payload, `2` MOUNTAIN/network reference-backed
> visual bridge with close-debug raw recipe preset available, `3` WORLD compose,
> and `4` LEGACY atlas; `B` cycles only REFERENCE and MOUNTAIN/network. Follow-up hardening: the
> producer helper exposes
> `runtime_seed()` instead of `seed()` to avoid the GDScript built-in RNG seeder, and
> `mountain_fly_review.gd` exposes `debug_runtime_snapshot()` so `review_runtime`
> validates the actual owner scene through a stable debug surface instead of private
> field reads. The live page pool now also has an identity-default biome source
> transform seam so review presets can separate display coordinates from source
> synthesis coordinates without touching the renderer. Current `fast` = **8/8 pass**.
> Current `review_runtime` also proves direct scene reconfiguration through
> MOUNTAIN -> REFERENCE and direct-key WORLD/LEGACY diagnostics, then runs the
> sprint churn gate. This locks the accepted owner cycle separately from the
> diagnostic live architectures instead of hiding them in one toggle.
> Continue refactor only at clear ownership boundaries: renderer streaming/pop-in, producer
> routing/page compute, biome grammar/composition, and review harness taxonomy. Do not treat
> the live WORLD fly as accepted just because it now composes biome recipe heights; owner visual
> acceptance and the Slice 4c runtime/facts story remain open.

What is actually true right now. Update this whenever reality changes. If a
manual fly contradicts a claim here, fix this file immediately. (Separating
"what passed a counter gate" from "what is actually accepted" is the whole
point — see DESIGN §7.3.)

> **▶ CURRENT (2026-06-02 PM) — SLICE 4: all 11 biomes + compose GPU-proven; runtime-drainage DECIDED; PART B +
> drainage build + 4c remain.** Branch `slice4-gpu-page-integration` (pushed, tip `6bb0771`). **cargo 210/0.**
> Work this session (all on the branch, NOT merged to main; legacy `height_page.glsl`+atlas still the runtime
> default — nothing flipped):
>
> 1. **ALL 11 BIOMES run as GPU page pipelines, each hardware-parity-proven** (RTX 5090/D3D12, `biome_page` suite,
>    NORM_EPS=1e-4): mountain 1.89e-6 · grassland 6.84e-7 · desert 1.30e-5 · coast 5.62e-6 · wetland 2.46e-6 ·
>    tundra 3.11e-7 · glacial 1.81e-6 · karst 2.00e-6 · temperate 1.69e-6 · rainforest 3.24e-6 · volcanic 3.01e-6.
>    Architecture: CONCAT-SELECTION (`biome_page.glsl` generic pass-MACHINE + per-biome `biome_<name>.glsl` FRAGMENT,
>    concatenated+compiled per biome) + a `Scheduler` seam (`schedule_<biome>()`) + a 16-slot generic scratch POOL +
>    additive hooks `flow_channels_ex` (glacial's 1.85 pre-blur), `flow_discharge` (raw discharge for temperate/
>    rainforest dual-spread), a vent SSBO @binding 40 (volcanic's PCG64 vents computed CPU-side — RNG never entered
>    GLSL). Each biome = fragment + schedule + `<biome>_sigmas()` + a BIOMES parity row + fixture.
> 2. **COMPOSE layer (4b.11 PART A) GPU-proven** — blend_field / blend_height_favored / compose_biomes fold, parity
>    vs the f64 fixture. The windowed gate CAUGHT+FIXED a real flat-field f32 bug (`gaussian(constant)!=constant`
>    spurious relief amplified to 2.76%/13m → relief dead-zone snap → 5.6e-8). Compose passes 60-66 in the machine,
>    cfg via spare push pads (11 biomes byte-identical).
> 3. **RUNTIME-DRAINAGE DECISION (data-grounded, owner-priority).** §3.1 said live flow too slow; MEASURED on
>    hardware: the 576² production page needs ~192 relax iters = 6.45ms (the small 344² fixture's ~64-iter
>    convergence was a small-grid artifact — `flow_iters` is now a swept knob via `generate_core_page_iters` /
>    `flow_converge` suite). PROBED the fixes vs the f64 oracle: coarse-upsample/coarse-cache = ~800m valley-
>    misplacement (REFUTED parity-exact); operator-squaring solver = exact but GPU-heavyweight (parked). Owner
>    priority: procedural-first, baking-fine. **DECISION: on-demand FULL-RES flow bake (proven look) OFF the hot
>    frame, per-region drainage-fact cache riding the M3 page-pool LRU, pages sample it, evict far.** Spec written
>    + owner-review-pending: `docs/superpowers/specs/2026-06-02-worldgen-runtime-drainage-design.md`.
>
> **NEXT (new session):** (a) owner reviews the drainage spec; (b) **4b.11 PART B** — port the grammar region/
> palette/family selection so a page picks its ACTIVE biomes + partition-of-unity weights, then composes them
> (the compose MATH is done; this is the integration that makes a page show real multi-biome terrain — recommended
> FIRST, it's smaller + unblocks the look); (c) build the drainage subsystem per the approved spec; (d) Slice 4c
> (flip runtime to the biome path + remove the 25MB atlas + hardened perf gate + owner fly). Plans:
> `docs/superpowers/plans/2026-06-02-slice4-gpu-page-integration.md` (4b/4c). Memories: `worldgen10-slice4a-proven`,
> `worldgen10-flow-convergence-production`, `worldgen10-coarse-drainage-refuted`, `worldgen10-godot46-string-format`.

> **▶ SLICE 4a DONE + PROVEN ON HARDWARE — mountain terrain runs on the GPU, parity-exact, behind a flag
> (2026-06-02, RTX 5090/D3D12 windowed).** The GPU page integration's first biome works: the accepted MOUNTAIN
> recipe runs as a 25-pass GLSL compute pipeline on the apron grid and **matches the f64 oracle fixture to
> overall_maxd = 1.89e-6** (~5000x under the tightened 1e-4 epsilon; `biome_page` suite GREEN). Primitive parity
> also green on real D3D12 (maxd 1.86e-4, the i64-emulated lattice hash holds on the actual driver). The windowed
> run found+fixed a real integration bug the no-GPU validation structurally couldn't catch: the GLSL concat placed
> `#version` mid-file (helpers fragment is concatenated first) → both shaders failed to compile; fixed with a shared,
> unit-tested `concat_glsl_hoist_version` helper. **§3.1 PIPELINE DECISION (MEASURED): `coarse-drainage-fact-fallback`**
> — per-page LIVE flow at the real 576² production apron costs 4.30 ms marginal > the 3.0 ms half-budget, so
> live-per-page drainage is TOO SLOW; the coarse cached drainage-fact is the runtime path (now a REQUIRED 4b/4c
> design item). The recipe MATH is proven correct regardless of how drainage is delivered. The accepted MOUNTAIN
> recipe now runs as a multi-pass GLSL compute pipeline on the apron
> grid, parity-gated against the committed f64 fixture. DONE: **4a.1** per-page
> cost measurement spike (`Wg10PageMeasure`, commit c50f19d); **4a.2** measurement gate + `page_measure` suite
> (3c017a0); **4a.3** GLSL noise/warp primitives with an **i64-emulated lattice hash cross-proven to 2.8e-8 vs the
> f64 oracle** + `primitive_parity_check.gd` (`biome_page` suite) (0e38ffd); **4a.4** verified the mountain fixture
> schema (records[] + apron meshgrid params) + documented the gaussian-as-GPU-passes approach (5cc3c1d); **4a.5**
> the 25-pass mountain page pipeline (`biome_page_4a.glsl` + `Wg10BiomePageCompute`) + two-tier parity check vs the
> fixture (0114b26, hardened: seed-range guard + kparams pre-validate + discharge_fd invariant). **cargo test =
> 167 passed, 0 failed** (165 + 2 concat-helper tests). **Windowed gates GREEN on RTX 5090/D3D12:**
> `tools/gate.py --suite page_measure` (→ coarse-fact decision) + `--suite biome_page` (→ both parity checks pass).
> The legacy `height_page.glsl` + kernel atlas remain the runtime default — nothing flipped (this is behind a flag).
> **NEXT: Slice 4b** — the other 10 recipes + `compose_biomes` + grammar weights, same fixture-parity pattern,
> PLUS the coarse-drainage-fact bake/cache the §3.1 measurement now requires (live-per-page flow is too slow at
> 576²). Plan: `docs/superpowers/plans/2026-06-02-slice4-gpu-page-integration.md`. Branch:
> `slice4-gpu-page-integration`. Memory `worldgen10-biome-composition-layer`, `worldgen10-godot46-string-format`.

> **▶ ALL 11 BIOME RECIPES PORTED TO RUST (2026-06-02) — Slice-3 CPU port bulk DONE.** Every seam-safe biome
> recipe (mountain/volcanic/glacial/karst/grassland/desert/temperate/tundra/rainforest/coast/wetland) ported to
> Rust as an apron-grid pipeline on `recipe_noise` + `array_ops`, each **parity machine-exact vs its Python
> original** (1e-12 to 1e-16; volcanic's 1e-12 = porting numpy PCG64+ziggurat for vent placement). Fixture-gated
> per biome (compact fixtures: store {spacing,ox,oz,apron_px}, rebuild meshgrid analytically). Shared
> `recipes::helpers` (affine_remap/smoothstep/rotated/flow_channels_seam_safe/apron_meshgrid/noise wrappers) reused;
> per-biome divergences caught + handled (glacial sigma=1.85, karst weight_gain=1.62, grassland freq_mul=1.70,
> rainforest steps=4 + dual-mask drainage, temperate two-spread valley, etc). Wired into lib.rs: **full cargo test
> = 148 passed, 0 failed.** Commits b556fa7…1fa2568 (pushed). **NEXT (Slice-3 finish):** port `compose_biomes` +
> the grammar biome-weight field to Rust → replace `sample_kernel` in `height.rs` → CPU integration parity gate →
> then Slice 4 (GLSL/render + remove kernel atlas). Memory `worldgen10-biome-composition-layer`.

> **▶ GPU-FLOW GATE PASSED (2026-06-02) — Slice-3 #1 risk retired on real hardware.** The drainage operator
> (`flow_accumulation_mfd`, a sequential CPU sweep) reformulated as iterative PULL relaxation in a GLSL compute
> shader (`flow_accum_spike.glsl`), measured on RTX 5090/D3D12 windowed: **~1.9 ms for a 256² page at 128 iters**
> (converges bit-stable by 128 iters), linear ~0.0147 ms/iter. Comfortably under the 6 ms frame budget → drainage
> **measured to fit live on GPU** (a MEASUREMENT SPIKE, now a REAL gate: `flow_spike_check.gd` returns nonzero on
> over-budget/non-convergence; suite `python tools/gate.py --suite gpu_flow`). NOT "risk eliminated forever" — a
> measured PASS on this hardware, to be re-confirmed by the gate + re-validated at the real per-page render
> integration (Slice 4). Leaning LIVE drainage; baked-facts fallback stays available if the integrated path
> regresses. Commit 4b392b6 (cargo 132). Measurement gotcha recorded:
> local-RD compute timestamps (`get_captured_timestamp_gpu_time`) are UNRELIABLE on this box (reported >wall-clock);
> use wall-clock differential across iter counts. Also: local RenderingDevice must be `.free()`d each run (driver
> slot exhaustion). **Slice-3 port shape now fully de-risked** — remaining: port the 11 recipe compositions
> (apron-grid pipelines on recipe_noise+array_ops) → compose_biomes → replace `sample_kernel` → CPU then GLSL
> parity gates. **Runevision erosion filter** (owner-flagged) banked as the lead candidate for the deferred
> local-detail/erosion layer (Phase-6-detail/7A) — local, GPU single-pass, chunk-safe; COMPLEMENTARY to
> flow_accumulation (structure) — memory `worldgen10-runevision-erosion-candidate`.

> **▶ AUDIT RESPONSE (2026-06-02) — owner audit fixed + verified, pushed.** An owner audit found real issues
> (validated against current code, not the stale snapshot). FIXED: F4 registry now forwards `apron_px` (seam-safe
> path reachable through composition) · F5 `height_favored` blur → `mode='nearest'` (apron-safe) · F6 tautological
> terrain-edit seam test → real carve-then-slice test + honest `xfail` documenting the ~4.47m independent-window
> gap (commit 89e4758) · F7 page_pool `free_all` stale-state→`acquire` panic + F8 `configure` RID leak (ce61449,
> cargo 126 + **gpu gate green windowed**) · F1 HANDOFF reconciled (Slice-3 unblocked+in-progress, not BLOCKED) +
> F2 dirty-tree noted (f24db1f) · F11 stray 14MB wrong-CWD artifact deleted · F10 `gate.py` gained a `pytest`
> suite so "gate green" covers the Python side (243 tests; 5f32dfd). TRIAGE (documented-known, no code fix):
> F3 biome_compose_world exporter is review-only/known-rough, SUPERSEDED by the fast Python renderers
> (`render_biome_blocks_fast`/`render_biome_compose_fast`) — not a port target. F9 displayed-mesh detail diverges
> from facts/collision by ~88m BY DESIGN (M3 shader detail; base-terrain parity is what's gated). F12 road/river/
> lake/POI are explicitly SKETCHES (spec-acknowledged). My CPU port work (recipe_noise/array_ops) was NOT
> implicated — parity machine-exact. All pushed through 5f32dfd.

> **▶ CURRENT (2026-06-01, late) — BIOME-COMPOSITION LAYER (Fork B) + SCALE CONTRACT done; Slice-3 unblocked.**
> The "shouldn't be bound to kernels" insight → a full biome-composition layer: `biome_compose` (compose_biomes
> + height_favored blend), `biome_registry` (name→recipe), `seam_safe` (shared apron/affine helpers). **All 11
> biomes made seam-safe** (apron_px path + affine_remap + nearest blurs + REAL MFD flow-accumulation connected
> drainage + crop; seam <1e-3 visually-seamless; legacy path byte-identical). **Full suite 238 passed.** Probes
> killed "biomes = one v2 engine" (Fork A dead) and proved cross-recipe blend is tractable. **Scale contract
> resolved** (spec `…/2026-06-01-worldgen-scale-contract-design.md`): on-foot real-metre anchor (mountain
> ~3.5km/1000m slope~0.29 … wetland ~9km/110m; ~30km regions); the "not tall enough" struggle was an
> overview-vs-on-foot ILLUSION — real mountains are broad swells at true scale (correct); "towering" is a future
> detail layer (cliffs/crags, Phase 6), not a slope fudge; content scale authoritative, presentation scales
> decoupled. Fast Python render-first loop replaced the slow JSON→Godot loop. Commits af921c7…44b88c1.
> **Slice-3 Rust port plan written + reviewed** (`…/plans/2026-06-01-slice3-rust-port-plan.md`, gated on this
> offline stack; the GPU-flow-accumulation cost gate is its top de-risk). Memory `worldgen10-biome-composition-layer`,
> `worldgen10-gpu-rust-first-principle`. **NEXT: Slice-3 Rust port** (or further biome look-tuning/materials if owner wants).

> **▶ BASE STACK ACCEPTED — `v2` (2026-06-01, owner flew A|B|v2 + accepted v2).** The roadmap's Slice-3 blocker
> (accept a BASE height stack to port) is RESOLVED. Owner flew `rough_world_abv_review.tscn` (keys 1/2/3) and
> accepted **v2** ("it looks good") — the owner-eye acceptance the gate can't give (DESIGN §7.3). This unblocks
> Slice 3 (Rust port). Evidence behind the pick — fresh Tier-1 ABV traversability gate on a regenerated
> `rough_world_abv.json` (k=0 policy = the scene's real policy): **v2 is the only variant whose low corridor
> actually CROSSES the block** (WE+NS) at 100×/200× and grades `candidate`; A and B both read `blocked` at 25×
> and `thin` everywhere (no crossing route at any scale):
> | variant | 25× | 100× | 200× | crosses |
> |---|---|---|---|---|
> | A approved | blocked 31.5% | thin 83.5% | thin 97.3% | never |
> | B keeper_v1 | blocked 16.2% | thin 86.8% | thin 98.1% | never |
> | v2 best-of-both | thin 49.2% | candidate 96.6% | candidate 100% | WE+NS |
> Relief ptp: A 1.721 · B 1.685 · v2 1.423 (v2 is the flattest — its honest cost, owner judged acceptable).
> NOTE: the old `corr(A,B)=+0.13 / B=35% of A` drift claim does NOT reproduce on this matched-core export (B ptp
> ≈ 98% of A). **v2 = `keeper_v2.compose_windowed_height_v2` (A's regimes on B's seam-safe substrate; seam-exact,
> 23 tests).** PORT TARGET for Slice 3. (STATUS working note; not committed — owner stages/commits by name.)

> **▶ CURRENT (2026-06-01):** Phase 5. The **tunable TERRAIN-EDIT FRAMEWORK is BUILT + OWNER-ACCEPTED.** An
> edit = (Placement WHERE + Profile WHAT) → seam-exact world-local delta, composed at the M4 edit-provider seam;
> edits READ facts, stay separate. `tools/dem_pack/terrain_edits/` — `apply` (blend/bound/combine/EditContext),
> `placement` (low_corridor_route / contour_sweep / cross_waypoint), `profile` (thin_climbing_trail /
> graded_valley), `edit` (TerrainEdit + apply_edits), `configs` (mountain_trail, mountain_trail_connected,
> road/river/lake/poi SKETCHES). **13 offline tests green.** Wired into the real mountain 9x9 chunk scene
> (`export_godot_terrain_edit_chunks.py` + `terrain_edit_chunks_review.tscn`; carve-big-field-then-slice =
> seam-exact; fly + walk + collision). Commits ed7d03b … 77c3828.
> **Owner flew it + accepted.** Thin Fellowship trails PRESERVE the mountain (no gouge/cliff/wide gash). Trail
> placement is a tunable spectrum: `mountain_trail()` sparse single pass · `mountain_trail(route_count=N)` spread
> · `mountain_trail_connected()` = 4 arms meeting at a central waypoint → ONE network spanning all four edges
> (full L↔R + U↔D, meet-in-the-middle; the chunk-scene default). **Honest:** the connected net is GEOMETRICALLY
> full (gated, touches all 4 edges); ~66% of its length is fully walkable, the rest short steep scrambles
> (depth-cap-vs-walkable tension, a tunable via `depth_cap_m`/`floor_grade_frac`). Owner saw one ~50 m
> walkable-mask gap, judged it a fine walkable scramble.
> Spec §9 (built+accepted): `docs/superpowers/specs/2026-06-01-worldgen-terrain-edit-framework-design.md`. Full
> carve/routing trace + negative results: memory `worldgen10-tier3-corridor-built-mountain-gap`.
> **NEXT (deferred, owner-gated):** flesh out road/river/lake/POI editors as needed; runtime sample/bake split +
> Rust port (Slice 3); cross-chunk seam-exactness for independent-window streaming (today = carve-then-slice,
> fine for the 9x9 review; independent windows need world-anchored fact-derived routes). Pass DENSITY ties to the
> player-to-world scale contract (`MOUNTAIN_BIOME_PROMOTION_2026-05-31.md`). (Below: prior history + ROADMAP box.)

> **(2026-05-31, history):** the framework goal emerged from Tier-3 traversability: corridor router + carve_ramp
> resolved a real mountain wall (pass NETWORK in the 9x9), owner flew the wide carve → "too wide / gouges peaks /
> cliff drops" → asked for thin Fellowship trails + "make it tunable, won't just be for this" → the framework
> above. Mountains landed via `mountain_synthesis.py` (`MOUNTAIN_BIOME_PROMOTION_2026-05-31.md`).

> **Latest session handoff: `docs/plans/SESSION_HANDOFF_2026-05-30.md`** — read it for the exact
> point-in-time state (Slice 2 paused for structure research). Current addendum: the B-bug closeout is now
> gate-verified after an editor-closed rebuild: **cargo 121 passed**, **fast 6/6**, **gpu 4/4**, **m3 9/9**.
> `origin/main` is synced through the B-bug closeout + first Slice 2A probe. Current coarse-structure matrix
> and landform-regime work is local research/probe state until accepted or explicitly committed. Roadmap
> Phase 5 is now realigned around an 85%-target geography-engine prototype before any Rust/GLSL port.

> **Mountain biome promotion note (2026-05-31):** `docs/plans/MOUNTAIN_BIOME_PROMOTION_2026-05-31.md`
> records the current mountain-kernel candidate, the 81-chunk static review artifact, and the key finding:
> player/world scale is a project-wide contract problem, not another per-biome height knob. Mountain is a
> yellow/keep candidate, not a runtime promotion, until a player-to-world scale policy exists and the owner
> accepts the on-foot valley read.

> **Glacial biome promotion note (2026-05-31):** `docs/plans/GLACIAL_BIOME_PROMOTION_2026-05-31.md`
> records the current glacial-kernel candidate, the single generated-world review, and the 81-chunk continuity
> review. Glacial is **promoted as an owner-accepted setup biome**: seams and checks are green and the owner
> accepted it as good enough to move on. It is still not a runtime promotion; detailed tuning and the same
> project-wide player/world scale policy remain later gates.

> **Karst biome promotion note (2026-05-31):** `docs/plans/KARST_BIOME_PROMOTION_2026-05-31.md`
> records the first-pass karst synthesis, reference sheets, Godot single-world export, scene, and smoke gate.
> Karst is **promoted as an owner-accepted setup biome**. For the current biome setup sweep, promote families
> that broadly read right and are not obviously broken; defer deep tuning, 9x9 continuity, and runtime/player
> scale proof until the full biome set exists or a specific blocker appears.

> **Volcanic biome promotion note (2026-05-31):** `docs/plans/VOLCANIC_BIOME_PROMOTION_2026-05-31.md`
> records the first-pass volcanic synthesis, reference sheets, Godot single-world export, scene, and smoke
> gate. Volcanic is **promoted as an owner-accepted setup biome**. Keep all four synth styles as useful
> volcanic variants for later runtime design; do not collapse them into one averaged shape during setup. Also,
> volcanic-origin terrain does **not** require an obvious cone in every zone; lava fields, rift provinces,
> caldera remnants, and eroded volcanic highlands are valid volcanic reads.

> **Biome setup index (2026-05-31):** `docs/plans/BIOME_SETUP_INDEX_2026-05-31.md` is the organizing tracker
> for this sweep. It records promoted, active, and pending biome families plus where setup artifacts belong.

> **Desert biome promotion note (2026-05-31):** `docs/plans/DESERT_BIOME_PROMOTION_2026-05-31.md`
> records the first-pass desert synthesis, reference sheets, Godot single-world export, scene, and smoke gate.
> Desert is **promoted as an owner-accepted setup biome**. Keep all four synth styles as useful desert DNA:
> dune seas, yardang/deflation basins, rocky basin-range deserts, and wadi/erg margins. Do not collapse desert
> into only dunes during setup.

> **Grassland biome setup pass (2026-05-31):** first-pass grassland synthesis is built and review-ready:
> `tools/dem_pack/grassland_synthesis.py`, `render_grassland_synthesis.py`,
> `export_godot_grassland_world_review.py`, `wg-10/worldgen_terrain/generated/review/grassland_world_3d.json`,
> and `wg-10/worldgen_terrain/harness/grassland_world_review.tscn`. Smoke gate:
> `[wg10-grassland-world-review] status=pass`. Owner review is pending before setup promotion.

> **Biome review queue (2026-06-01):** `docs/plans/BIOME_REVIEW_QUEUE_2026-06-01.md` lists all remaining
> unvalidated setup scenes prepared for tomorrow: grassland, coast, rainforest, temperate, tundra, and wetland.
> Each has generator/render/export/review-scene/test artifacts, rendered 90 km + 200 km contact sheets, and a
> passing headless Godot smoke gate. No interactive scenes were launched for these final queue items. Coast and
> wetland are explicitly terrain/mask setup passes only; water/sea-level/flooding behavior remains later work.
> Final non-visual validation rerun: Python synthesis tests `36 passed`; all six headless Godot review gates
> returned `status=pass items=8 selected=4`.

> **All-biome transition review scene (2026-06-01):** `wg-10/worldgen_terrain/harness/biome_transition_world_review.tscn`
> is built for visual validation of biome adjacency. It uses all 12 current setup families, gives each family a
> 3x3 chunk block, and blends height/color across biome borders in one continuous 12x9 chunk lattice. Generated
> payload: `wg-10/worldgen_terrain/generated/review/biome_transition_world_3x3.json`. Exporter/layout tests:
> `3 passed`; headless Godot smoke gate: `[wg10-biome-transition-world-review] status=pass biomes=12 chunks=108
> labels=12`.

---

## CURRENT DIRECTION: Worldgen Core rebuild — 85%-target geography engine

Spec baseline: `docs/superpowers/specs/2026-05-30-worldgen-core-design.md`. Current roadmap realignment:
the goal is no longer "find a better warped-noise combo." The goal is an **85%-class geography read**: at
normal game/fly-camera distances, terrain should read as plausible real geography with coherent basins,
ranges, ridges, drainage-shaped corridors, and local variation that follows landform history. It is not a
promise to be indistinguishable from real USGS DEMs under expert GIS inspection. Kernels are still kept as a
real-world DNA/reference library, not sampled as tiling height textures. Keeps render/grammar/facts/
relief_scale. Full context: memory `worldgen10-north-star-vision`, `worldgen10-wg9-height-recipe`; loose
ends in `LOOSE_ENDS_LEDGER.md`.

**Roadmap update from structure audit + owner review:** `STRUCTURE_AUDIT_EXTRACT.md` has been folded into
ROADMAP Phase 5, then realigned after the owner judged the broad combo/matrix sheets: the least-bad
basin/range/flow/fine-detail cell was still **not great**. Current next work is **Slice 2A geography-engine
prototype** (offline Python, render-first): explicit landform regimes, irregular ridge/uplift skeletons,
drainage-shaped corridors over coarse fields where feasible, per-regime process/detail, and DEM-reference
contact sheets at 200 km, 40 km, and close crop. Multifractal/warp/Worley ideas are allowed only as
components inside that hierarchy, not as the milestone by themselves. Slice 2B then fixes the metric/schema
set (HI, slope moments, curvature signs, VRM, windowed relief, patch/regime proportions, ridge/valley
spacing; audit `anisotropy`). No Rust/GLSL port until an owner-accepted offline image set exists. If an
85%-class sheet requires real routed coarse drainage, pull ROADMAP Phase 7B forward before the Rust port.

**Expectation gates for the 85% target:** Green means at least one generated patch reads as real geography,
not "nice noise"; basins/ranges/valleys/ridges have recognizable logic; there are no visible straight
scaffolds, cells, chunks, or repeated stamps; and the same candidate holds at 200 km, 40 km, and close crop
beside real DEM references. Yellow means the stills improve but another scale breaks, or ridges exist while
drainage remains decorative. Red means sheets still look basically the same, the best result is only
"least bad," or weird procedural lines/masks remain visible.

**Roadmap adherence audit (2026-05-31):** the current work is aligned with Phase 5 Slice 2A and the Phase 7B
pull-forward escape hatch, but it does **not** yet satisfy the roadmap's runtime port gate. What is real: the
owner selected the rough-highlands skeleton family as the current best direction; the 25.6 km Godot review
harness is usable for scale/corridor review; the adjacent 3x3 chunk proof now has deterministic seed variation,
independent world-window chunk generation, exact/near-exact fixture height seams, visual seam audit, Godot
scene smoke coverage, and owner seam-visibility acceptance. The spec-to-implementation bridge is now frozen as
`rough_highlands_keeper_v1`: contract spec, deterministic sample fixture, golden contact-sheet hash, fact
boundaries, scale/relief policy, and fixture regression tests. What is still missing before a Rust/GLSL port:
owner terrain/travel acceptance beyond seam visibility, a decision on whether the private `route_texture`
corridor branch becomes a public fact or is replaced, and then a Rust CPU skeleton-facts implementation plan
against the frozen fixture. Decision: no more broad visual combo search; next work is either final owner
travel/terrain review or Rust CPU skeleton-facts parity against the frozen contract when the owner greenlights
porting.

**⚠ KEEPER FORMULA FORK (2026-05-31) — BLOCKS Slice 3.** "rough_highlands" now names three different height
formulas, and the one the owner approved by eye is NOT the one that was frozen. **A — approved look:**
`geography_skeleton.compose_height` (rough_anchor), the 6-regime softmax-blend generator behind
`rough_world_review.tscn` (the 90 km scene the owner liked). **B — frozen keeper:**
`export_godot_rough_world_chunks._compose_windowed_height` (`rough_highlands_keeper_v1`), a from-scratch
seam-safe rewrite — hard masks, no softmax regimes, no post-tanh smoothing, a *different* skeleton generator
(`geography_skeleton_windows`). **C — streaming spike:** `height_page_rough.glsl`, a closed-form GLSL
approximation of B. Verified on identical world coords (seed 133, rough_anchor, 129²): `corr(A,B) = +0.13`
(weakly related, NOT an inversion) and **B relief = 35% of A** (much flatter). So B is a genuinely different,
much flatter terrain only loosely related to the approved A; "owner accepted the direction + seams" silently
hardened into the frozen *formula* B and no one re-validated that B reproduces A's shape. B exists for a real
reason — A's per-window `_condition()` percentile normalization breaks seams — but the honest fix was to keep
A's structure and drop only that normalization, not rewrite the terrain. **Resolution path:** (1) cheap owner
gate — an honest A-vs-B side-by-side at matched coords + fly scale (rendered:
`D:\tmp\wg10_geography_engine\ab_keeper_compare_{topdown,oblique}.png`); (2) if B is rejected, rebuild B to
reproduce A's regime-softmax structure while staying seam-safe (`rough_highlands_keeper_v2`). Slice 3 must not
port anything until this is resolved — porting the keeper today means porting a terrain the owner never
approved. **The rendered A-vs-B is NOT a clear win for either:** A has more fine relief amplitude (B is 35% of
A's relief) but reads as fairly uniform "rough everywhere"; B is flatter but the oblique view reads as
*better-organized* macro landforms (distinct peaks, broad valleys). So this is a genuine owner-eye trade
(ruggedness vs. organization), not an obvious regression — do not assume B loses. Full trace + reproducible
check: memory `worldgen10-keeper-formula-fork`.

**Fork-resolution session update (2026-05-31, later):** the fork analysis above stands; what changed is we
acted on it. (Note: "v2" here = the NEW `rough_highlands_keeper_v2` best-of-both generator, NOT the older
2026-05-30 "Skeleton v2" 7B-lite checkpoint elsewhere in this doc.)
- **`keeper_v2` BUILT** (`tools/dem_pack/keeper_v2.py`, committed `c56f30e..46b0481`, 8 TDD tasks, suite
  **23 passed**): A's 6-regime softmax structure composed on B's seam-safe windowed substrate. Seam-exactness
  **gated and bit-exact (border delta 0.0)** — the apron-cropped-blur + fixed-affine-remap design holds. Two
  real bugs caught in review: a too-loose blur reach guard (would silently break seams) and three global
  `geo.norm01` calls (did break seams, 0.0118 → fixed to 0.0). All knobs tunable (pillar 1), incl. seam-safe
  realism knobs `post_tanh_gain` + `final_blur_mix` (amplitude can't push realism via the pre-tanh
  `relief_amplitude` alone — it saturates; `post_tanh_gain` raises peaks linearly instead).
- **A | B | v2 in-place switcher scene** (`rough_world_abv_review.tscn`, committed `77396bc`): flip the three
  formulas at matched coords / same camera (keys 1/2/3). `rough_world_review.gd` `DATA_PATH` → `@export
  data_path` + initial-select clamp (backward-compatible; existing 6-item scene unaffected). Owner reviewed:
  all three "look good for what they are."
- **Tier-1 traversability gate** (`report_abv_traversability.py`, committed `482b1d5`): runs the analyzer over
  A|B|v2. Verdict confirms the owner's eye — **A is too spiky for a play area** (blocked @25×, slope_p90 1.42,
  **no crossing corridor at any scale**); **v2 is the most traversable** (only one reaching "candidate" with a
  WENS-crossing low-corridor at play scales). B sits between, also no crossing corridor.
- **Owner direction (supersedes "pick A or B or v2"):** keep ALL THREE as selectable variants (pillar 1), and
  pursue **guaranteed regime-aware traversability** as the real quality bar. Tiers: Tier-1 measure/gate DONE;
  Tier-2 bias knobs (later); **Tier-3 = guaranteed routes through barrier regions** (the true target) —
  brainstormed to an approved design, spec + plan written
  (`docs/superpowers/specs/2026-05-31-worldgen-tier3-guaranteed-traversability-design.md`,
  `docs/superpowers/plans/2026-05-31-tier3-guaranteed-traversability.md`), and offline-Python build STARTED.
  **BUILD FINDING (2026-05-31, spec §1.2):** barrier detection + the verify-first no-op are built and
  **seam-safe** (`tools/dem_pack/traverse_corridor.py`, 9 tests green; keeper_v2 9 green). But the **carve is
  blocked**: a globally-routed least-cost-path carve CANNOT be seam-exact (adjacent windows route differently →
  border delta 0.62 ≠ 0, proven), and no purely-local seam-exact operator guarantees a *connected* crossing
  (both prototyped + rejected with data). The seam-exact connected carve depends on a cross-seam-stitched
  **connected-corridor fact = the unbuilt connectivity half of Phase 7B**. The module is honest: real barriers
  are reported `carve_pending` (never falsely "resolved", never a seam-breaking carve). Also measured: barriers
  are span×relief-dependent, NOT regime-weight (caps ~0.32); 25.6 km/260 m default has no slope-wall, small
  spans + high relief do. Memory: `worldgen10-tier3-seam-exact-carve`, `worldgen10-tier3-barrier-measurements`,
  `worldgen10-tier3-guaranteed-traversability`. **Next decision (owner, pillar-judged):** (i) pull Phase 7B
  connected seam-joined corridor forward, then carve along it; (ii) scope guarantee to channel-where-available;
  or (iii) Tier-2 param-bias instead of carve.
- **Fork status:** no longer "unresolved/pick one" — it's "three kept variants + v2 is the traversability
  front-runner; the real next decision is guaranteed-traversability (Tier-3), not picking a single keeper."
  Slice 3 (Rust port) stays blocked until an owner-accepted final stack exists.

**B-bug closeout state:** B1/B2/B3 are now closed for the rebuild precondition. Evidence: Rust DLL rebuilt
after the editor was closed; `cargo test` isolated target **121 passed / 0 failed**; `fast` **6/6**; `gpu`
**4/4**; `m3` **9/9**. The new B2 capacity-pressure gate passed non-vacuously
(`full_delta=3`, `pressure_held=3`, `resident=9`), proving the live view holds pinned coarsest pages under a
deliberately tight pool budget. B3's hardened perf gate also passed with terrain-vs-sky and detail-on/off
checks active (`GPU p99=0.082ms`, `terrain_frac_min=1.000`, `detail_delta=0.53739`).

**Validated code-path audit addendum (2026-05-31):** treat the latest whole-project audit as leads, not as
visual truth. Locally verified: the OLD `dem_v1` kernels-as-height path multiplies z-score kernels by
`height_range_m` in CPU and both GLSL height paths. The shipped arrays have std≈1 and peak-to-peak z spans
**3.97–11.16** (median **5.56**), exactly matching WG9 `height_range_m / height_std_m`; correct metres for
`normalized_height.npy` are therefore `z × height_std_m` unless the kernels are rebaked to a documented bounded
range. This bug does **not** directly judge the current Python skeleton review scene, but it must be fixed or
explicitly bypassed before any Rust/GLSL port or kernel-detail layer copies the old `relief_m` contract. Also
verified: the gate pack contains **24** kernel files, every palette is `[A,B,A]`, and footprint/page-scale
mismatch is larger than the pasted audit stated: actual `footprint_m` is **37.7–222.6 km** (median **194.3
km**) against the current **8.192 km** level-0 page. Runtime scale remains a separate engineering issue:
`BASE_SPAN`, `PAGE_PX`, 2^L level spans, and shader detail frequency are still coupled. The review scene's
6–26 km horizontal scale control is a useful **content/landform-density** knob, not a substitute for the future
per-level runtime scale rework.

**Slice 2A render probes:** Batch 1 (`D:\tmp\wg10_structure_ab\`) was rejected by owner eye: all variants
looked basically the same, meaning stronger warp/ridge/noise variants still changed texture more than
geography. Batch 2 / matrix (`D:\tmp\wg10_structure_matrix\`) found the least-bad cell as **basins + ranges /
flow + fine detail**, but owner verdict remains **not great**. The current landform-regime probe also shows
weird line/scaffold artifacts; real WG9/DEM reference kernels make the gap obvious. Key conclusion: real
geography is not an averaged set of per-biome statistics; it is a nuanced mix of landform regimes and local
histories. Next algorithm direction is now explicit in ROADMAP Phase 5: hierarchical landform composition
with coarse regions/regimes, irregular ridge/drainage skeletons, and per-region process/detail, judged against
real DEM contact sheets before any runtime port. Current owner preference from the v5 geography-engine sheet:
the **far bottom-right synthetic panel (`badlands_mix`) is the best so far**; next probe should focus around
that parameter neighborhood rather than broad unrelated variants. Follow-up probes added an oblique
scene-read render plus metric reports. The scene probe makes the current best candidates look more
scene-plausible than the forensic hillshade alone, but the metric pass confirms the 45 km synth is still
too smooth vs references: synthesized local relief/highpass/slope are far below real DEM rows. Next
improvement target is therefore not more broad parameter search; it is richer close-scale process/detail tied
to the accepted badlands/range structural frame. Independent review agreed this is **Yellow and plateauing**:
the current "hierarchy" is still mostly weighted blends of shared global noise fields, and drainage is still
decorative. Decision: **pull 7B-lite forward inside Slice 2A as an offline proof**. Next prototype should
build a coarse world-anchored uplift/ridge skeleton first, route flow on that skeleton, derive regimes from
crest distance / accumulation / slope breaks, and carve channels causally before adding noise as material.

**7B-lite skeleton v1 checkpoint:** implemented as offline Python in `tools/dem_pack/geography_skeleton.py`
with renderer/tests beside the earlier geography-engine probes. It builds a coarse uplift/ridge skeleton,
routes coarse flow, derives range/foothill/basin/fan/badlands weights from skeleton facts, carves channels
from discharge, then adds local material. Evidence: focused dem_pack tests **19 passed** (pytest cache warning
only); rendered contact/debug sheets to `D:\tmp\wg10_geography_engine\geography_skeleton_v1_200km.png`,
`D:\tmp\wg10_geography_engine\geography_skeleton_v1_45km_close.png`, and matching `_debug.png` files. Initial
engineering read: this is structurally different from v5 and has more connected, causal drainage, but it is
not final-green; the 45 km view still shows coarse routed-channel artifacts and variants remain too similar.
Owner review: **"looks pretty good tbh, we are getting better"**. Treat this as a **Yellow+ / keep**
checkpoint: skeleton-first is now the active direction, and the next iteration should fix flow artifacts/scale
blending instead of returning to broad noise-combo tuning.

**7B-lite skeleton v2 checkpoint:** implemented in the same offline Python prototype.
Changes vs v1: coarse routing now uses multiple-flow accumulation instead of single-neighbor D8 integer
routing; primary channels and tributaries are separated; basin/fan floors damp incision more than badlands,
foothills, and range cores; scenarios now change process weights/widths/smoothing rather than only contrast.
Evidence: focused tests **21 passed** (pytest cache warning only); rendered
`D:\tmp\wg10_geography_engine\geography_skeleton_v2_200km.png`,
`D:\tmp\wg10_geography_engine\geography_skeleton_v2_45km_close.png`, and matching debug sheets/notes.
Engineering read before owner review: v2 is a real iteration on the skeleton-first path and reduces some
D8-style scar risk, but the 45 km sheet still has some synthetic-looking basin-edge/channel shapes. Owner
verdict on the opened sheet: **"`SYN rough highlands` is great"**. Treat `rough_highlands` as the current
Skeleton v2 keeper/current-best panel and focus the next image work around that process family. This completes
the bounded Skeleton v2 offline goal, but it is not a Rust/GLSL port greenlight or full Phase-7B runtime
architecture acceptance.

**Next Slice 2A action:** run a narrow rough-highlands focus pass instead of a broad matrix. Keep it offline:
render `rough_highlands` plus process-neighbor variants at 200 km, 45 km, and an oblique scene-read view;
check whether the keeper still reads well outside the forensic hillshade sheet; then update this section with
the owner verdict before any port discussion.

**Rough-highlands focus pass:** renderer added at `tools/dem_pack/render_geography_skeleton_focus.py` and run
offline. Outputs: `D:\tmp\wg10_geography_engine\geography_skeleton_rough_focus_200km.png`,
`D:\tmp\wg10_geography_engine\geography_skeleton_rough_focus_45km_close.png`,
`D:\tmp\wg10_geography_engine\geography_skeleton_rough_focus_scene.png`, plus debug/notes files. Evidence:
focused tests **21 passed** (pytest cache warning only). Initial engineering read: the pass is correctly
narrowed around the owner-selected family; `rough_anchor`, `rough_broad_crests`, and `rough_sharp_front` are
the useful neighborhood. The offline oblique scene probe supports the terrain read, but its own faceted
painter-renderer is only a review aid, not a Godot/runtime visual gate. This pass fed the later Godot
generated-world review scene; the owner keep signal is on the Godot scene setup and rough-highlands direction,
not a final Phase 5 terrain acceptance.

**Godot generated-world review scene:** the first Godot tile-comparison scene was rejected by owner eye as
gross, and correctly so: it independently normalized tiny tiles and made the DEM refs read as blown-out cliff
cards. It is superseded. The active review artifact is now
`wg-10/worldgen_terrain/harness/rough_world_review.tscn`, backed by
`wg-10/worldgen_terrain/generated/review/rough_world_3d.json` from
`tools/dem_pack/export_godot_rough_world_review.py`. It uses the current Python skeleton generator to export a
larger 90 km world for each rough-highlands focus variant, then displays one generated world at a time in
Godot so the owner can switch variants in-place from the same fly-camera view. Keys: `1-4` refs, `5-0` synth,
`[`/`]` prev/next, `F` focus, `G` overview, `+/-` relief, `R` reset relief, `,/.` horizontal scale,
`K` relief policy (`k=0` / `0.5` / `1.0`), `P` overlay cycle (`terrain` / `slope` / `corridor`), `L` flat lighting. Lighting was brightened; shadows and fog are disabled for
readability; `L` is a no-shadow/unshaded review fallback. Owner verdict on the first generated-world scene:
the rough-highlands synth is promising, but the review scale was invalid for player-scale judgment because a
90 km source world was squeezed into a 128-unit scene block. Scale is therefore now a first-class review
knob: default is now the owner-preferred 200x horizontal expansion (25.6 km scene width if 1 Godot unit = 1 m),
with 10/25/50/100/150/200x presets, independent relief, and a visual relief-policy probe matching the offline
audit (`k=0` current fixed-height behavior, `k=1` slope-invariant control around the 25.6 km reference span).
This does not declare the terrain production-ready;
it separates shape-quality review from game-scale/traversability review. Current working expectation: roughly
25 km may be a good playable review block for this terrain density, but final games need this tunable because
too much or too little landform density can both hurt pacing. Next owner pass should judge the same variants at
the 25 km default with `P` cycled through slope and corridor overlays to see whether playable corridors/slopes can be made plausible without
losing the real-geography read. Owner clarification: mountains and very tall landforms are acceptable, even
desirable, as long as the generated world also provides traversable structure; the fix is not global flattening,
it is passes, valley floors, ramps, shelves, basin/fan corridors, and route continuity through or around high
relief. Non-visual evidence from the generated-world scene build: generated JSON was rebuilt;
`python -m pytest tools\dem_pack\test_geography_engine.py tools\dem_pack\test_geography_skeleton.py
tools\dem_pack\test_worldgen_proto.py -q` is **23 passed** (pytest cache warning only). The latest
scale/no-fog/default-25km/corridor-overlay/relief-policy harness edit passes `git diff --check`; focused
traversability + skeleton-window tests are **14 passed**; Godot `--import` exits 0 with no GDScript parse error (known PDB shortening
warning; sandboxed editor-settings save warning only). The corridor overlay is a visual review aid over the
same p55-low/passable-slope idea used in the audit, not proof of a runtime pathfinder. The metrics pass now also writes rough-skeleton reports:
`D:\tmp\wg10_geography_engine\geography_metrics_skeleton_rough_200km.{csv,md}` and
`D:\tmp\wg10_geography_engine\geography_metrics_skeleton_rough_45km_close.{csv,md}`. This is still
offline/static generated review data, not a Rust/GLSL terrain port and not Phase 7B runtime drainage. Owner
quick read on the current 25 km Godot review scene/corridor overlay: **"seems good"**. Treat that as a keep
signal for the review setup, not full Phase 5 terrain acceptance.

**Rough-world traversability/scale audit:** added `tools/dem_pack/analyze_rough_world_traversability.py`
with focused tests (`test_rough_world_traversability.py`, now **6 passed**, pytest cache warning only). It
reads the same conditioned `rough_world_3d.json` mesh used by the review scene and writes
`D:\tmp\wg10_geography_engine\rough_world_traversability_scale.{csv,md}`. This is a slope/connectivity
heuristic, not a gameplay navmesh and not visual acceptance. The external scale audit was validated with one
correction: the scene's current `k=0` relief policy keeps vertical scale fixed while X/Z scale changes, so
slopes fall exactly as 1/span; the low-passable corridor component is large and touches two edges, but does
not currently cross opposite edges. The report now includes a relief exponent probe (`k=0`, `0.5`, `1.0`) and
a stricter structural-corridor grade. `k=1` is the slope-invariant control around the 25.6 km reference span,
not a runtime decision. The new grade keeps the legacy passability label for comparison but rejects flat or
diffuse passability; a flat field is no longer allowed to score structural `candidate`. Current k=0 result:
~6.4 km remains blocked, ~12.8 km is only structural `thin` even though the old passability grade says
candidate, and ~19.2-25.6 km are structural candidates. Interpretation: the horizontal content/landform-density
knob is real, but slope/passability alone is not a sufficient acceptance signal. The next quality question is
whether the 25.6 km-scale world has enough interesting traversable routes, passes, shelves, and valley/fan
corridors under owner visual review.

**Adjacent-chunk proof (AFK goal in progress):** added offline exporter
`tools/dem_pack/export_godot_rough_world_chunks.py`, generated
`wg-10/worldgen_terrain/generated/review/rough_world_chunks_3x3.json`, and added a flyable Godot review scene
`wg-10/worldgen_terrain/harness/rough_world_chunks_review.tscn`. This proof uses the current `rough_anchor`
keeper family over a 3x3 set of **25.6 km** chunks, with two deterministic seeds (`133`, `211`) and a `T`
seed switch in-scene. The latest payload is now `rough_world_chunks_v2_independent_windows`: each chunk is
generated from its own deterministic world-coordinate skeleton window with a **25.6 km apron** at **200 m**
spacing, then cropped to the 25.6 km core. The review scene's corridor overlay now prefers the exported
structural route/corridor mask, and the scene now has default-off seam inspection aids: `B` toggles cyan seam
guide lines and `N` jumps to the next shared border. A static contact-sheet renderer
(`tools/dem_pack/render_rough_world_chunks_review.py`) writes
`D:\tmp\wg10_geography_engine\rough_world_chunks_review_contact.png` for quick terrain/seam/corridor/slope
inspection of both seeds. Important boundary: this is still **offline Python + static Godot JSON**, not a
final infinite streaming/runtime architecture, full terrain/gameplay acceptance, or Rust/GLSL port. Dedicated proof/audit report:
`docs/plans/CHUNK_CONTINUITY_PROOF_2026-05-31.md`.

Evidence written to `D:\tmp\wg10_geography_engine\rough_world_chunks_3x3_seams.{csv,md}`: shared-border
height max abs delta is **0.000000** across all seams; minimum structural corridor component match fraction is
**0.917**; adjacent center/east chunks differ materially (`mean_abs_delta` **0.225** for seed 133, **0.385**
for seed 211); center chunk changes across seeds (`mean_abs_delta` **0.396**). Focused tests including rough
chunks, rough traversability, skeleton, and skeleton-window checks are **31 passed** (pytest cache warning only).
Godot `--import` exits 0 with no GDScript parse error (known PDB shortening warning; sandboxed editor-settings
save warning only). New focused Godot scene smoke check
`worldgen_terrain/tests/rough_world_chunks_review_check.gd` passes and verifies the actual review scene builds
**9** chunk meshes, **12** seam guides, **2** seed worlds, default-off guides, and next-seam focus. The
owner/visible Windows fly scene remains the review gate. The seam report still includes the legacy
isolated-window diagnostic proving why the old
`compose_height` review path cannot be used chunk-by-chunk: separate adjacent 25.6 km windows produce
conditioned seam max deltas of **0.661** on x and **1.442** on z for seed 133. A new non-rendered
virtual-travel stress report (`D:\tmp\wg10_geography_engine\rough_world_chunks_virtual_travel.{csv,md}`) builds
a wider **5x5 / 128 km** lattice from independent windows for both seeds: 40 seams per seed, height max
**0.000000**, corridor min **0.971** for seed 133 and **1.000** for seed 211, adjacent median deltas
**0.348/0.371**, and max adjacent corr **0.341/0.389**. This supports the M3-backed streaming direction but is
not a streaming/cache/player-travel proof. A new offline visual seam report
(`D:\tmp\wg10_geography_engine\rough_world_chunks_visual_seams.{csv,md}`) mirrors the Godot review mesh's edge
height, normal, slope, default terrain-color, and corridor-edge math; current 3x3 report is zero across all
shared edges for both seeds (`height_delta_m=0.0000`, `normal_max_angle_deg=0.0000`,
`terrain_color_max_delta=0.000000`, corridor mismatches `0`). This reduces seam-risk before owner review but
does not replace flying the scene. Owner visual read on the opened scene:
**"from what i can see seams are good visually"**. Treat this as seam-visibility acceptance for the bounded
3x3 proof; terrain/gameplay quality and arbitrary infinite runtime acceptance remain open.

**5x5 terrain/travel review scene:** added a wider static Godot review payload and scene:
`tools/dem_pack/export_godot_rough_world_travel_review.py`,
`wg-10/worldgen_terrain/generated/review/rough_world_chunks_travel_5x5.json`, and
`wg-10/worldgen_terrain/harness/rough_world_travel_review.tscn`. It reuses the same rough-highlands
independent-window contract over **5x5** chunks, **128 km** wide, at **65x65 vertices per chunk** so the owner
can judge travel pacing and route/corridor feel beyond the tighter seam-review scene. Report:
`D:\tmp\wg10_geography_engine\rough_world_chunks_travel_5x5.{csv,md}`; contact sheet:
`D:\tmp\wg10_geography_engine\rough_world_chunks_travel_5x5_contact.png`; variant sheet:
`D:\tmp\wg10_geography_engine\rough_world_chunks_travel_5x5_variants.png`. Current metrics: **80** shared seams,
height max **0.000000**, corridor min **0.905**, normal max **0.0000 deg**, corridor mismatches **0**, adjacent
pair median delta **0.359**, and max adjacent corr **0.487**. Owner first read on the opened 5x5 scene:
**"seems good"**, but it may be too flat / not enough elevation, with the caveat that the current scene is
untextured and has no biomes/materials. Treat this as a yellow keep signal for continuity/travel scale, not
final terrain acceptance.

**5x5 relief/dressing review pass (in progress):** the 5x5 payload now carries four review variants:
`current_plain` (1.00x/plain), `medium_dressed` (1.25x/review biome), `high_dressed` (1.50x/review biome),
and `high_route_read` (1.65x/route-read dressing). The Godot scene adds `V` to cycle variants; `+/-` still
nudges relief, `R` resets to the selected variant, and `P`/`B`/`N` still cover overlay, seam guides, and
seam focus. This is review dressing only, not Phase 6 surfacing. Verification: focused Python checks are
**23 passed** (pytest cache warning only); Godot smoke checks verify the 3x3 scene still builds **9** chunk
meshes / **12** seam guides with fallback variants, and the 5x5 travel scene builds **25** chunk meshes /
**40** seam guides while cycling to the 1.25x dressed variant. This is still offline/static review data, not
streamed runtime terrain. Next: owner fly should compare the named variants and decide whether the keeper is
good enough to start the Rust CPU skeleton-facts parity spike, or whether terrain shape needs another offline
iteration before porting.

**30x30 bounded distance proxy:** per owner scale correction, the long-distance review shows a full **30x30**
area, not a 5x5 visible window over a hidden lattice. This artifact has been demoted from any "infinite"
claim: it is a bounded static proxy for scale/readability only. New artifacts:
`wg-10/worldgen_terrain/generated/review/rough_world_chunks_travel_lattice_30x30.json`,
`wg-10/worldgen_terrain/harness/rough_world_distance_proxy.tscn`, and
`wg-10/worldgen_terrain/tests/rough_world_distance_proxy_check.gd`. The payload is **768 km** wide, **30x30**
chunks, **41x41 vertices per chunk**, two seeds, and the same relief/dressing variants, but the scene defaults
to the keeper baseline (`current_plain`, 1.00x relief). It is a distance/continuation review, not the close
route-detail gate; the 5x5/65 scene remains the route/corridor detail review. Current 30x30 report:
`D:\tmp\wg10_geography_engine\rough_world_chunks_travel_lattice_30x30.{csv,md}`; height max seam delta
**0.000100**, normal max **0.0016 deg**, corridor edge mismatches **0**, adjacent median delta **0.376**, max
adjacent corr **0.670**. The older corridor-component match metric falls to **0.267** at this coarse distance
resolution, so it is explicitly **not** used as the route acceptance gate for 30x30. Verification:
`python -m pytest tools\dem_pack\test_rough_world_chunks.py tools\dem_pack\test_rough_highlands_keeper_contract.py tools\dem_pack\test_geography_skeleton_windows.py -q`
is **24 passed** (pytest cache warning only); Godot smoke checks pass for 3x3, 5x5, and 30x30. The first
30x30 scene drew **900** live chunks and was owner-reported as laggy and visually still the same scale. That
was a review-scene design error: fitting a larger world into the same viewport hides scale, and 900 static
MeshInstances is not how the runtime should stream. The distance-proxy scene now opens as one decimated
full-lattice overview mesh, while `F`/`N` switches into a centered **7x7** detail/seam window (**49** live
chunks, **84** lazy seam guides). Latest distance-proxy smoke evidence:
`[wg10-rough-distance-proxy] status=pass overview_meshes=1 detail_chunks=49 seam_guides=84 seeds=2`.
Use Godot 4.6's explicit `--scene` launch flag for visible review; passing the `.tscn` positionally fell back
to the missing main-scene path in this setup. This still does not implement runtime streaming/cache; it proves
a large deterministic reviewable world area only. The next real "infinite" work must build on the M3
streamer/page/cache architecture with a deterministic rough-highlands provider, not on larger static JSON.

**M3-backed rough-highlands streaming spike (2026-05-31):** this is the first scene with BOTH real
streaming AND rough-highlands-style height — built on M3, not a static JSON bake. Files:
`wg-10/worldgen_terrain/shaders/height_page_rough.glsl` (new), `…/harness/rough_world_streaming_review.tscn`
+ `.gd` (new), `…/tests/rough_world_streaming_review_check.gd` (new). The new GLSL is a drop-in for
`height_page.glsl`'s producer seam: a **closed-form per-texel `height_at(x,z)`** (recursive domain warp +
ridged-multifractal uplift + broad low band + masked detail, in absolute world metres) that approximates the
rough_highlands keeper's LOCAL STRUCTURE using the same worldgen_proto primitives. It KEEPS the dem_v1 atlas
bindings 3..8 (declared + kept-live) so the UNCHANGED Rust producer (`page_compute::compute_page_cached`)
binds them against the shader's uniform set — **zero Rust change, no editor-closed rebuild**; the pool reuses
the dem_v1 pack with the atlas loaded-but-unread. Verified: headless `--import` exits 0 (no GLSL/GDScript
parse error); the windowed streaming gate passes —
`[wg10-rough-streaming] status=pass frames=60 fallback_fired=true provider=rough` — proving the rough GLSL
compiles to SPIR-V, the kept bindings match the Rust uniform set, pages are produced by world coord + stream
ahead in the travel direction, never-black holds (non-vacuous fallback), bounded work/budget, stream-ahead
converges to the finest level, and the provider is deterministic.
**HONEST SCOPE (keeper contract §9 / north-star):** this is the keeper's local-structure approximation
streamed live — relief + ridges + broad valleys (it directly addresses the "too flat" read with real
mid-scale ridged structure, not just a relief multiplier). It is **NOT** the routed/windowed keeper (no carved
connected corridors — that needs the apron + flow-accumulation + EDT that cannot run per-texel, and is the
later Phase 7B runtime subsystem, still blocked on owner acceptance), and **NOT** yet Rust CPU / GPU parity
(the prototype is GPU-only; CPU-facts parity comes with the real Slice-3 port). Passed gate != owner
acceptance — owner must fly `rough_world_streaming_review.tscn` (windowed) and judge whether it reads
large/infinite at the right scale before this direction is accepted.

**Port/Phase-7B non-visual groundwork:** the Slice 2A spec now names the minimum runtime story if the keeper
depends on routed structure: world-anchored coarse skeleton windows, seam/apron continuity, facts/collision
queries for skeleton fields, Python-vs-Rust fixtures, GPU world-coordinate sampling, cache/order independence,
and parity/perf/visible==collision gates. This is documentation of the required subsystem boundary only; no
Rust/GLSL port has started.

**Spec-to-implementation bridge (Slice 2A-close):** `rough_highlands_keeper_v1` is now a frozen candidate
contract, not just review code. Artifacts:
`docs/superpowers/specs/2026-05-31-worldgen-rough-highlands-keeper-contract.md`,
`tools/dem_pack/export_rough_highlands_keeper_contract.py`,
`tools/dem_pack/fixtures/rough_highlands_keeper_v1.json`, and
`tools/dem_pack/test_rough_highlands_keeper_contract.py`. The fixture locks fixed sample points, normalized
height/review metres, corridor booleans, skeleton facts, seam/variation/virtual-travel summaries, and a
reproducible contact-sheet SHA-256. First implementation target, when/if porting is greenlit: Rust CPU
skeleton-facts core with Python fixture parity. GPU mirroring comes after CPU facts and seam gates.

**Phase-7B-lite window seam spike:** added offline Python proof code in
`tools/dem_pack/geography_skeleton_windows.py` plus report writer
`tools/dem_pack/analyze_geography_skeleton_windows.py`. It builds fixed world-anchored coarse skeleton windows
with aprons, routes accumulation inside the extended window, crops authoritative core facts, and exposes
uplift/routed-surface/discharge/tributary/channel-axis plus saturated distance-to-crest/channel facts. Evidence:
`python -m pytest tools\dem_pack\test_geography_engine.py tools\dem_pack\test_geography_skeleton.py
tools\dem_pack\test_geography_skeleton_windows.py tools\dem_pack\test_worldgen_proto.py -q` is **30 passed**
(pytest cache warning only). Seam report written to
`D:\tmp\wg10_geography_engine\geography_skeleton_window_seams.{csv,md}`. The report/tests now also include a
coarse routed-corridor continuity check: if a channel-derived corridor mask enters a window edge, the adjacent
window must continue it within a small seam band. Current focused evidence is **14 passed** for the rough
traversability + skeleton-window tests, and default window reports show corridor match fraction 1.0 for the
sampled seeds/origins. This proves the windowing shape and a bounded seam strategy for sampled skeleton facts;
it does **not** prove owner-accepted terrain, full hydrology, or a Rust/GLSL implementation.

**Phase 7B runtime design:** added
`docs/superpowers/specs/2026-05-31-worldgen-phase7b-drainage-skeleton-design.md`. It defines the future
subsystem boundary: deterministic world-keyed windows, extended/apron routing, authoritative core facts,
saturated distance facts, cache semantics, Facts/collision/render parity, and the slice order for a future
Rust/GLSL port. This is design-ready groundwork only; do not implement it until Phase 5 accepts a specific
keeper that actually requires routed skeleton facts.

**Phase 6 surfacing design:** added
`docs/superpowers/specs/2026-05-31-worldgen-phase6-surfacing-design.md`. It reframes the old normals/materials
work for the post-pivot roadmap: one shared `SurfaceDescriptor`, analytic normals first, data-driven material
packs, deterministic scatter/dressing, and parity/perf/seam gates. This is also design-ready only; no Phase 6
implementation until Phase 5 has an owner-accepted live height core.

**Phase 7A local filter design:** added
`docs/superpowers/specs/2026-05-31-worldgen-phase7a-local-erosion-filters-design.md`. It defines local erosion/
gully/detail filters as bounded polish over an accepted height/descriptor path, distinct from Phase 7B
connected drainage. It is blocked on Phase 5/6 acceptance and the analytic gradient feasibility gate.

**Slice 2B metric/schema audit groundwork:** added
`tools/dem_pack/analyze_geography_metric_schema.py` to measure approved WG9 kernels with the existing
distillation metrics plus the newer cheap geomorphometric diagnostics. Reports:
`D:\tmp\wg10_geography_engine\geography_metric_schema_audit_kernels.csv`,
`D:\tmp\wg10_geography_engine\geography_metric_schema_audit_families.{csv,md}`, and
`D:\tmp\wg10_geography_engine\geography_metric_schema_audit_summary.md`. Findings from the generated summary:
`anisotropy` family medians span **0.314123-0.788793**, so it is **not dead**, but overlap means it should be
a secondary/context metric rather than the sole `warp_amount` driver; current `vrm_7px` reports zero-range at
this normalization/scale and should not be promoted as-is. Also fixed `biome_distill._structure_tensor_coherence`
to avoid divide-by-zero warnings while preserving the intended zero-coherence fallback.

### Slice 2 — biome distillation: OFFLINE TOOLING BUILT + GATED; the LOOK is NOT yet accepted (2026-05-30)
Spec: `docs/superpowers/specs/2026-05-30-worldgen-slice2-biome-distillation-design.md`; plan:
`docs/superpowers/plans/2026-05-30-worldgen-slice2-biome-distillation.md`. **What's DONE (committed, gated):**
the offline distillation pipeline — `tools/dem_pack/biome_distill.py` (structural metrics → generator knobs,
pure, 16 fixture-gated tests), `distill_biomes.py` (real DEMs by family → `biome_params.json`, spike-guarded
median), `attach_biome_params` in `dem_pack_lib.py` (additive validated per-family pack table), `render_biomes.py`
(real-vs-synth hillshades). Full dem_pack pytest suite green.
**Two real findings caught OFFLINE (render-first did its job — cheap, before any runtime):**
1. **The first two structural metrics were DEAD on real DEMs** — structure-tensor `ridge_linearity` ≈ 0.30 and
   argmax `dominant_wavelength_m` ≈ 25 km for EVERY family (don't vary on real 512px terrain). Also WG9's
   metadata `ridge_density`/`valley_density` are a dead-constant 0.100. Fix applied: drive `valley_depth` ←
   incision/relief (height-normalized carving), `ridge_strength` ← slope; the dead metrics are kept for
   diagnostics but no longer drive any knob (a trap-gate test proves height alone doesn't buy ridge_strength).
2. **A scale bug made the synth "sandpaper"** (base wavelength 8 km vs ~190 km tiles → features repeated ~24×)
   AND the distilled octave amplitudes were INVERTED (fine octaves dominated). Fixing both (decaying fBm amps +
   continental base scale) removed the sandpaper and produced visible macro structure.
**BUT — OWNER VERDICT on the tuned renders: "still not terrain — it all looks like the same noise, doesn't look
like the real world."** This is the same deeper truth the spectral refutation already exposed: **plain warped/
ridged noise produces ROUGHNESS but not real STRUCTURE (connected ridgelines + branching drainage = the thing
that reads as real geography).** Per-biome params now genuinely differ (mountain rugged vs grassland smooth —
the distillation architecture works), but the *generator* can't yet turn those params into real-world-looking
structure. **DECISION (owner): PAUSE Slice 2; research the right way to get real structure from the kernels
before more param-tuning** (candidate directions: stronger domain warp, ridged-multifractal, and especially the
owner's standing "distilled-erosion" idea — offline-run real erosion → learn a cheap LOCAL operator → apply
online; tracked in the ledger as the big enhancement). **The distillation tooling + the metric fixes are KEPT**
(they're the param-extraction half and they work); what's under research is the GENERATOR's structure stage.
**NEXT:** execute ROADMAP Slice 2A (geography-engine prototype). The B1/B2/B3 FIX-NOW work is closed and
verified, so the distillation tooling remains kept, but scalar tuning is not the next move until the
landform/regime hierarchy can pass the owner's structure read against real DEM references.

**Slice 1 — generator prototype (OFFLINE, render-first): ACCEPTED by owner eye (2026-05-30).**
`tools/dem_pack/worldgen_proto.py` (`value_noise`/`fbm`/`ridged_fbm`/`domain_warp`/`generate`) + 7 tests
green incl. the NON-REPETITION autocorrelation gate (no spike at the old 8192 m page / 50-100 km kernel
periods → the warped field provably doesn't tile). `render_worldgen.py` → hillshaded PNGs for 3 biomes
(200 km + 10 km closeup + transition strip). **Owner verdict: "pretty good, a little noisy."** Mountains
read as a real range from above (ranges/valleys/canyons, no grid/repeat), plains as soft lowlands, badlands
distinct, transition seamless (no hard line) — ALL from one `generate()` with different knobs (the
architecture working). **Scale finding (caught+fixed mid-render): the macro base octave must be LOW-freq
continental (~45-80 km) or a 200 km view is high-freq "sandpaper"; the 10 km closeup proved the warped
ridges/valleys work.** Owner reviewed a de-noise comparison and PREFERRED the current (more-detailed)
toolkit over de-noised variants — "hard to know until it's in a live scene." **So: NO toolkit change; final
noise judgment deferred to the live-scene fly.** Honest framing (owner-confirmed): warped noise = PLAUSIBLE
terrain, NOT real connected erosion (Grand-Canyon look = real-world history); **distilled-erosion is a big
LATER roadmap enhancement** (ledger), not needed for the foundation. This old "NEXT: Slice 2 distill real
DEMs" note is superseded by the structure audit and roadmap realignment above: Slice 2 tooling is built/kept,
and the next accepted step is Slice 2A geography-engine prototype. (Precondition before the RUST build,
Slice 3: ledger B1/B2/B3 is now closed.)

[below: prior milestone status — M5/shaded-scale/synthesis — partly superseded by the worldgen rebuild; see
the LOOSE_ENDS_LEDGER for what's KEEP / FOLD-IN / TABLED.]

---

# ⏷ EVERYTHING BELOW IS SUPERSEDED HISTORY (pre-worldgen-pivot)

The current state is the "Worldgen Core rebuild" section ABOVE. The sections below
(shaded-terrain-at-scale, M5 detail, the synthesis attempt, M3/M4 detail) describe earlier work,
kept for the record + the bug-lessons. **Do NOT treat their "IN PROGRESS / NEXT" items as live** —
they are re-sequenced or tabled per `LOOSE_ENDS_LEDGER.md` (KEEP / FOLD-IN / TABLED) and the
ROADMAP forward plan. What's still KEPT + true from here: the render pipeline, grammar, facts,
`relief_scale` (RELIEF_SCALE=0.25 shipped — any older "× 0.35" arithmetic below is dead), the
hardened perf gate, and the gate counts (fast 6 · gpu 4 · m3 9 · cargo 121 · dem_pack pytest 22).

---

## [SUPERSEDED — history] Shaded terrain at the right scale (the WG9 looks-alright baseline)

Spec: `docs/superpowers/specs/2026-05-30-shaded-terrain-at-scale-design.md`. Closing the owner's "still a
heightmap, not real terrain" gap to WG9's proven baseline (finer mesh + sane relief + normals/lighting),
designed as ONE coupled milestone. 4 slices: **S1 relief scale** → S2 normals/lighting → S3 mesh density
(perf-gated) → S4 integrate + M5 detail tune. M5 detail's S2-S4 fold in here.

**Slice 1 — relief_scale knob: DONE (code), owner A/B fly PENDING.** Commits `55fdd3b..c49640b` (+ review
fix). One authoritative `relief_scale` config knob multiplies the base height field, applied IDENTICALLY
on render (shader `VERTEX.y * relief_scale`) AND all 3 facts consume points (`get_height`,
`get_collision_field` closure, `bake_collision_region` — via a `scaled_base()` helper = `height::height()
* relief_scale`). Folds in the old `height_scale` → ONE relief knob. Raw `height::height` formula UNTOUCHED
(M2 parity intact). Default `RELIEF_SCALE := 0.25` (~2765 m → ~690 m; a STARTING value for owner live-tune).
- **Gates:** fast 6/6 (`[facts] relief_scale ok` max_err=0 — scaled==unscaled×0.25 exactly) · **gpu 4/4:
  base parity maxd=0.000932 m UNCHANGED + `relief_scale parity ok maxd=0.000233 m` (=0.0009×0.25) →
  visible==collision HELD with the knob** · m3 8/8 (perf gate now measures at the shipped 0.25, GPU
  p99=0.083 ms) · cargo 115.
- **Two-stage review:** spec-compliant; parity verified across ALL 4 base-height paths; 3 implementer
  deviations confirmed correct (f64 not f32 at bake, `%.9f` not invalid `%g`, has_method RED guard). ONE
  must-fix closed: stale `HEIGHT_SCALE := 0.35` constants in the gates renamed to `RELIEF_SCALE := 0.25`
  so the perf gate measures the SHIPPED relief, not a different one.
- **NEXT:** owner A/B fly (`m3_review.tscn` — relief is now ~4× shorter; confirm "sane height, not 2.7 km
  spikes"), then S2 (normals + basic lighting — the big "looks like terrain" lever).

## [SUPERSEDED — history] M5 — Detail & masks (Slice 1 detail seam, pre-pivot)

**✅ OWNER RE-FLY (2026-05-30, after the fix): "I can see a small difference with the detail now."**
The toggle/visibility fixes WORKED — detail is now perceptible at fly scale (was invisible before).
The S1 detail SEAM is proven on-screen. Owner's caveat — **"hard to tell because we're still [looking at]
the heightmap, really not real terrain"** — is CORRECT and EXPECTED: the shader is still the `unshaded`
DEBUG height-color (blue→yellow by elevation, no materials/normals/lighting), so geometry barely reads
against flat color bands. Flat unshaded color HIDES shape; it's lighting+normals (M6) that make geometry
READ as terrain. So S1's detail is confirmed working, but FULL visual judgment of the look is correctly
deferred until there's real shading (M6) + proper scale (the scale milestone) to see it against — exactly
why the sequencing decision put detail-tuning AFTER mesh density + relief, and why M6 materials are the
"looks like terrain not a heightmap" milestone. **S1 detail seam ACCEPTED as a working foundation; final
look judgment deferred to post-M6, as designed.**

[history] OWNER FLY FINDING (2026-05-30, before the fix): pressing N showed NO visible detail change.
Root-caused systematically (3 offscreen probes + independent code audit) — TWO real defects, NOT a wiring failure:

1. **Toggle starts ON, so first N turns it OFF (phase-inverted).** `m3_review.gd` registers
   `wg_detail_amp` at `DETAIL_AMP`(=60) on load AND `_detail_on := true`, so the scene starts detail-ON.
   The owner, expecting "N enables detail," presses once → amp 60→0 (OFF). (The audit corrected an earlier
   wrong guess that the shader auto-registers at 0 — in Godot 4.x a `global uniform` does NOT auto-register
   with no `shader_globals` in project.godot, so the `add(60)` is authoritative → starts ON.) Also
   INCONSISTENT with the m5 gate, which registers at 0.0. **Fix:** start OFF (`_detail_on:=false`, register
   `0.0`) so N-on matches expectation + the gate convention.
2. **Effective amplitude is near-invisible at fly scale (CONFIRMED by audit).** Detail peak = `DETAIL_AMP 60
   × HEIGHT_SCALE 0.35 = ±21 m`. On the m3_review `GRID_RES=64` finest mesh, vertex spacing = 8192/64 =
   128 m → Nyquist wavelength 256 m. The fBm's 5 octaves are ~1111/556/278/139/69 m; **only octaves 0–1
   (1111, 556 m) survive — octaves 2–4 are below the mesh and aliased away.** So the rendered detail is
   ~±16 m of gentle km-scale swell, invisible from 1200 m+ altitude over km-scale relief. The **m5 gate
   "passed"** because it captures a SINGLE tile at close ortho range with `GRID_RES=128` (carries one more
   octave) — exactly the gate-vs-human SCALE MISMATCH the owner has been warning about. **Fix:** raise amp
   and/or lower base frequency (so octaves live above the shipped mesh Nyquist) and/or raise shipped
   GRID_RES — tuned at the REAL fly scale, owner-judged.

**Wiring CONFIRMED SOUND by the audit (not the problem):** global→all-45-tile-materials applies
automatically; no per-frame clobber (Rust `bind_tile` only sets per-tile uniforms, never the global);
`KEY_N` reaches `m3_review._input` (fly_camera doesn't consume keys); the global-set propagates (a probe
moved the render 0.58 when amp 0→60 in close capture). The getter returning null is a known editor-only
quirk, not proof of failure. **This finding is itself the strongest evidence for the owner's standing
concern: gates must verify at a REPRESENTATIVE scale or they pass while the human sees nothing.**

**FIXES APPLIED (2026-05-30, awaiting owner re-fly to confirm visibility):**
- **Toggle phase (definitely correct):** `m3_review.gd` now starts detail OFF (`_detail_on:=false`,
  register `wg_detail_amp` at 0.0) so the FIRST N press turns detail ON, matching the m5 gate's 0.0
  baseline + the operator's expectation. (Was: started ON, so first N turned it off.)
- **Visibility tuning (reasoned + gate-safe, but NOT probe-validated at fly scale):** `DETAIL_AMP`
  60→**350** (×0.35 = ~122 m effective, up from ~21 m) in `m3_review.gd`; `WG_DETAIL_FREQ` 0.0009→
  **0.00025** in the shader (octaves now ~4000/2000/1000/500/250 m, all at/above the GRID_RES=64 Nyquist
  256 m, vs 3 octaves aliased away before). m3 suite still 7/7 (m5 non_vacuous diff 0.0026→0.0035 — broader
  detail shows MORE), m3_accept p99=4.97 ms (<6).
- **HONEST LIMITATION (the owner's exact point):** I could NOT validate the *visibility at fly scale* with
  an offscreen probe — every probe I built fills the frame with ONE tile at close range, where even amp=60
  reads as a 0.33 pixel delta (looks "very visible"), yet the owner saw nothing on the real fly (1200 m
  altitude, 5 levels, 197 km horizon). **The probe over-reports visibility → it is NOT a trustworthy
  proxy for the fly.** So the amp/freq values are reasoned improvements that the gate confirms don't break
  correctness, but **whether they're actually perceptible at fly scale can ONLY be confirmed by the owner's
  re-fly.** This is itself the strongest case for the S4 hardened gate to measure at a representative
  multi-level fly scale, not a close single-tile capture.

## [SUPERSEDED — history] M5 — Detail & masks (older detail-visibility notes)

**Slice 1 — fBm + uniform detail: GATE-GREEN, owner fly not yet done (so NOT "accepted").**
Spec: `docs/superpowers/specs/2026-05-30-m5-detail-masks-design.md`. Plan:
`docs/superpowers/plans/2026-05-30-m5-slice1-fbm-uniform-detail.md`. Commits `bf159e7..5db3817`
(pushed to origin; backed up at `C:\Backups\worldgen10\worldgen10_2026-05-30_M5s1-checkpoint`).

What landed (render-only; Rust core UNTOUCHED — cargo still 115):
- **`ring_displace.gdshader`** now adds bounded procedural **fBm detail** to `VERTEX.y` *after* the
  base height `h = mix(h_fine,h_coarse,t)` is formed. Detail = `wg_fbm_detail(world.xz) * wg_detail_amp`
  — a pure function of WORLD XZ (edge-safe by construction), normalized so `|detail| ≤ wg_detail_amp`
  (closed-form bound). New funcs `wg_hash2` / `wg_value_noise` (cubic-smoothstep weights + a contrast
  curve) / `wg_fbm_detail` (5 octaves, gain 0.5, base freq 0.0009 ≈ 1/1111 m). `wg_detail_amp` is a
  GLOBAL shader param (default 0 = byte-identical to pre-M5; the harness/gate sets it). The base
  `h`/`h_fine`/`h_coarse` math is byte-identical — detail rides ON TOP, so facts/collision (which read
  the base via the pure-Rust `facts_api` path) are untouched. The display varying was renamed
  `v_height → v_render_height` (it now carries base+detail for the height-color; NOT the facts height).
  Slice 1 is FLAT amplitude — no slope modulation (S3) and no LOD fade (S2) yet.
- **`m5_detail_check.gd`** (new, `m3` suite → now **7 checks**, WINDOWED): asserts (1) BOUNDED
  (saturated-pixel frac < 0.20), (2) **EDGE-SAFE WITH DETAIL ON** — renders the two abutting tiles
  SEPARATELY and compares tile A's right-edge column vs tile B's left-edge column (same shared world
  edge), `seam_max_luma_delta = 0.00392 < 0.01` → abutting tiles AGREE on the shared edge (the M3 seam
  contract SURVIVES M5 — the scariest risk, retired first), (3) NON-VACUOUS (`diff 0.0026 > 0.001`,
  threshold derived from realized amplitude, not a guess). The seam test was strengthened in review
  from an across-the-seam compare (which mixed terrain gradient with seam error) to the separate-tile
  compare (isolates seam agreement).
- **Parity contract proven intact:** `gpu` suite still **4/4**, `facts_collision_parity_check`
  **maxd = 0.000932 m** (identical to the M4 baseline) WITH detail on — detail did not move the base.
- **`m3_review.gd`**: press **N** to toggle detail on/off live (the owner A/B). Default detail ON at
  60 m peak.

**Gates: cargo 115 · fast 6/6 · gpu 4/4 · m3 7/7 · all fail=0.** Built subagent-driven (impl + spec
review + code-quality review per task); both reviews passed (spec verified base-parity against the
real Rust path; quality strengthened the seam test + cleanups).

**NOT YET DONE (the honest baseline — DESIGN §7.3):** the OWNER ACCEPTANCE FLY. Gate-green proves
bounded/edge-safe/base-untouched/perf — it does NOT prove "looks good / less blobby." The owner has
not yet flown `m3_review.tscn` to judge the LOOK (does detail read as real shape? any shimmer/crawl at
speed? any perceived seam the gate's epsilon missed?). **Until that fly, Slice 1 is GATED, not
ACCEPTED.** Owner deferred the fly; resuming Slices 2–4 should keep this provisional (S2 LOD fade
modifies this same detail — if the owner's fly finds a look problem, fix before/with S2).

**Perf de-risked early (throwaway probes, 2026-05-30 — both uncommitted, cleaned up):**
- Probe A (detail OFF vs ON over the m3_accept ~1000 m/s flight): detail OFF p99 = 1.90 ms · ON = 1.82 ms
  · delta ≈ 0 → S1 fBm adds no measurable frame time at the current method's resolution.
- **Probe B (GPU-time honesty — prompted by the owner's "is the profiling measuring REAL work?" callout):**
  benchmarked a LIGHT scene (4-subdiv, trivial shader) vs a HEAVY scene (9×200-subdiv ≈ 360k verts + a
  64-iter noise vertex loop) via wall-time-per-`force_draw`, vsync off. **light = 0.686 ms · heavy =
  0.878 ms · ratio = 1.28×.** VERDICT: **PARTIAL** — wall-time-per-draw captures SOME GPU cost (heavy >
  light, so not vacuous) but is **dominated by ~0.69 ms of CPU-submit/frame-pacing overhead**, so a ~90×
  vertex-load increase moved it only 1.28×. **⇒ the current p99 method is GPU-cost-INSENSITIVE** — fine
  for confirming S1's already-cheap detail, but TOO SOFT as a regression gate (a real GPU regression in
  M6/M7 could hide under the CPU-submit floor). **This makes the owner's concern concrete and CORRECT.**

**HARDENED-GATE DE-RISK (2026-05-30, 2 more throwaway probes) — the "true GPU time" plan HIT A WALL,
found a working alternative:**
- **RD timestamp READ is NOT exposed** on this Godot 4.6 build: `rd.capture_timestamp` exists but
  `get_captured_timestamp_gpu_result` returns method-not-present. So "measure GPU time via RenderingDevice
  timestamps" is **unavailable here.**
- **Wall-time-per-draw is fully async-hidden:** a 256-iter vertex loop × 4 big tiles gave ratio **1.00×**
  (pinned at 8.33 ms = the vsync/present cadence). The SubViewport GPU work hides entirely behind the
  present interval — wall time is useless for GPU cost (worse than the earlier 1.28×).
- **WORKING SIGNAL FOUND — `RenderingServer.get_rendering_info(RENDERING_INFO_TOTAL_PRIMITIVES_IN_FRAME)`:**
  real, exposed, strongly load-responsive (heavy scene = 242,406 primitives / 3 draw calls vs ~0 light).
  A monotonic GEOMETRY-LOAD metric that a regression moves directly + deterministically, no async issue.

**⇒ RESOLVED — REAL GPU TIME IS AVAILABLE after all (research + verify, 2026-05-30).** The earlier "no GPU
time" wall was a WRONG METHOD NAME, not a missing capability. Web research (cited in commit) + a verify
probe found:
- **`RenderingServer.viewport_get_measured_render_time_gpu(viewport_rid)`** returns REAL per-viewport GPU
  milliseconds. Enable once with `viewport_set_measure_render_time(rid, true)`. **VERIFIED on this D3D12
  box:** light scene 0.0196 ms vs heavy 0.1761 ms = **8.97× load response** (wall-time was 1.00–1.28× =
  useless). Non-zero, strongly load-responsive.
- **Zero observer-effect (the owner's constraint):** it's a DEFERRED read of an already-completed frame's
  timestamp (write this frame, read a finished frame N later) — **no fence, no GPU stall.** Godot docs
  explicitly state it "accurately reflects GPU utilization even if framerate is capped via V-Sync" — which
  is exactly why it works where the vsync-pinned wall-time failed. Returns 0 only on Metal (not us).
- The earlier `get_captured_timestamp_gpu_RESULT` has_method=false was a NAME bug — the real RenderingDevice
  method is `get_captured_timestamp_gpu_TIME` (a finer per-pass alternative, also non-stalling, if needed).
- Native D3D12 timestamp via `get_driver_resource` was researched + REJECTED (can't inject markers into
  Godot's draw command list; crash-history fragility) — not needed, the viewport timer is the clean path.

**⇒ HARDENED-GATE DESIGN (final):** primary signal = `viewport_get_measured_render_time_gpu` (REAL GPU ms,
no stall) for the p99/budget assertion, PLUS the did-real-work co-asserts (pages streamed + nonblack +
relief/detail present + primitive count in expected range) so a green number provably = the real streaming-
clipmap-with-detail render at fly scale. CPU `view.update` time kept as the streaming-cost signal. This
fully answers the owner's "is profiling real?" — a real GPU-ms number that MOVES 9× with load, can't pass
on an empty scene, and doesn't distort what it measures.

**⇒ BUILT + GREEN (2026-05-30): `m5_perf_hardened_check.gd`** (m3 suite → now **8 checks**, WINDOWED).
Scripted ~1000 m/s flight, detail ON, 5 levels. Result: **REAL GPU p99 = 0.082 ms** (mean 0.075, max
0.082; budget 6) with the did-real-work floors all cleared — nonblack 1.000, stream_events 45 (pages
streamed under motion), prim_max 368,640 (geometry drawn), resident 45, cpu_update_mean 0.771 ms. A green
result PROVABLY = the real streaming-clipmap-with-detail render (can't pass empty/static/black). Relief-
VARIETY deliberately NOT asserted here (a fly-POV color-bucket count is the wrong instrument — mostly
distant fogged terrain; relief variety is proven by m3_view_check's top-down ortho). **Finding while
building it:** the OLD `m3_accept_check` (wall-time) is FLAKY — one run reported p99=5.96 ms + a phantom
**max=77.26 ms "stall" with compute_frames=0** (no GPU work could cause a real 77 ms stall → it's wall-
time/GC/pacing noise, not a regression). That intermittent phantom is EXACTLY why wall-time was replaced;
m3_accept is kept for now (passes most runs) but the hardened GPU-ms gate is the trustworthy perf signal.
The m5_perf gate measures GRID_RES=64 today — when the scale milestone raises near-field density, this gate
is the instrument that says whether the denser mesh stays within the GPU budget.

[superseded plan] The S4 hardened perf gate was to measure TRUE GPU time
via `RenderingDevice` timestamp queries (NOT wall-time-per-draw), AND co-assert the scene did REAL WORK
(relief variety present + detail non-vacuous + tiles visible + pages streamed/counter-moved + nonblack),
so a green p99 provably corresponds to the real streaming-clipmap-with-detail render under motion. A
green number that isn't doing real work is worthless (the WG9 failure mode). The S3 descriptor (+4 page
taps) is the first slice with real GPU cost — measured against this on the hardened gate. (m3_accept TODAY
is substantially real — vsync-off + real motion + streaming-proven + nonblack≥0.85 — but its TIMER is the
soft spot; hardening it is the S4 job.) See memory: profiling-must-be-real.

**Independent pillar-audit of the shipped S1 slice (2026-05-30): all 4 pillars PASS (3 with notes);
"sound to build S2/S3 on top of."** Confirmed the 3 core contracts independently — bounded (closed-form
`|fbm|≤1` normalization, holds with smoothstep), base-untouched (`h` byte-identical; Rust `facts_api::
get_height` is shader-independent), edge-safe (the gate's separate-tile A-right-vs-B-left seam compare is
the STRONG test). S2/S3 extend cleanly (the detail term is one multiplicative expression they wrap;
`t`/`world_span`/`page_origin`/`height_tex` already present; `textureSize` gives texel size — no new
uniform/Rust). **Three carry-forwards for S4 (none blocking):**
  1. **Gate the "base-unchanged-with-detail-on" invariant explicitly** (spec §8 inv 3). T4 already showed
     `facts_collision_parity_check`=0.000932 m green with the suite, but the m5 gate proves base-untouched
     only structurally — fold an explicit detail-on parity assertion into S4's gate-consolidation pass.
  2. **`WG_DETAIL_FREQ=0.0009` GRID_RES mismatch:** the m5 gate renders at GRID_RES=128 but `m3_review.gd`
     (the owner fly) uses GRID_RES=64. The freq was chosen against one; record which in S4 config so the
     shipped visual matches what was gated. (If the owner's fly look differs from the gate's PNG, this is
     why — a mesh-density/detail-frequency interaction, tune in S4, not a correctness bug.)
  3. **Bounded gate is a saturation proxy** (`sat<0.20`); fine for S1's fixed octaves/gain, but when S4
     opens them to config, switch to a direct arithmetic bound (`detail_amp × Σ gain^i`).

**Next M5 slices:** S2 LOD fade (detail → 0 into coarse/morph band) · S3 surface descriptor +
slope modulation (the M6/M7 reusable seam) · S4 config + p99 sign-off + docs audit (+ the 3 carry-forwards).

---

Last updated: 2026-05-30 (**M3 RENDER LAYER STRUCTURALLY DONE — rebuilt prove-one-thing-at-a-time, folded into the real classes, all gates green.** The post-slice-8 multi-level render was "a mess" under a real fly (sheets/seams/switching) because slices 4→8 stacked without proving live continuity — gates proved properties (p99, never-black, data-seam=0) but never *perceptual continuity in a flown POV*. Fix: kept the proven CPU/GPU leaves (pool, page_policy, schedule_policy, streamer, ring_geometry, page_compute — clean one-directional deps), rebuilt ONLY the presentation (`Wg10TerrainView` + `Wg10ClipmapRings` + `ring_displace.gdshader`) one step at a time in `proving_ground.tscn`, owner-flown each step, then FOLDED the proven model into the real classes.

**Real bugs found + fixed (each owner-confirmed) — do NOT reintroduce:**
1. **Page sampler defaulted to REPEAT wrap** → tile-edge vertices (uv=1) wrapped to the page's opposite edge → seams at EVERY tile boundary (the dominant "sheets"). Fix: `filter_linear, repeat_disable` (clamp-to-edge) in the shader.
2. **Velocity lead unit-wrong + unclamped** — `lead_frames` × m/s gave ~64 km lead at sprint, flying the ring off the camera (pop-in, lag-under-you, churn). Fix: renamed `lead_seconds`; `SchedulePolicy::coverage_center` CLAMPS to ±(radius−0.5)·span so the camera is ALWAYS in its ring; view reads the clamped centre from the streamer (no desync).
3. **Step-5 "LOD line" = morph was OFF** (each tile bound its own page as the morph target). Fix: each non-coarsest tile geomorphs toward its REAL parent page (level+1) over its 3×3 outer band.
4. **Tiles vanishing on rotation / creep-blink = frustum-cull of GPU-displaced flat meshes.** Fix: `Wg10ClipmapRings` sets a tall custom AABB per tile (GPU-displaced meshes ALWAYS need this).
5. **Coarsest-level boundary cross blanked the screen** (all 9 coarse tiles repage at once, budget can't fill them, hiding them = no blanket). Fix: coarsest level HOLDS LAST-GOOD on a miss (never hide the bottom blanket); finer levels still hide (covered below).
6. **"Loads then unloads" = VIEW DISTANCE > loaded extent, NOT a bug.** 3 levels load only ~49 km but the camera saw ~524 km, so ground popped in/out at the loaded edge. Fix: m3_review NUM_LEVELS 3→5 (reach ~197 km) + far plane matched to the loaded edge + distance fog fading to sky before the edge. The page is ALWAYS resident when wanted (probe: 252/252) — nothing actually unloads.

**Folded-back render model (the real `Wg10TerrainView::update`):** every level draws its full 3×3; an unready tile is HIDDEN so the coarser full 3×3 underneath shows through (never-black) EXCEPT the coarsest holds last-good; a resident tile samples its own page by world UV and geomorphs toward its real parent if not coarsest. `Wg10ClipmapRings` got `set_tile_visible` + the custom AABB + debug methods (`debug_tile_states`, `debug_disable_culling`). `ring_displace.gdshader` has the clamp sampler + a `wg_dbg_mode` morph-heatmap (press M in m3_review; K toggles cull-disable; a flip-log shows tile HIDE/SHOW/REPAGE).

**ALL gates green on the rebuilt path: m3 6/6 (accept p99≈1.9–3.9 ms<6), gpu 2/2, fast 5/5, cargo 103.** `m3_review.tscn` flies the REAL components (5 levels, fog). **Render layer is structurally complete.** Remaining oddities (LOD detail pop at level boundaries, the "squareness"/extreme heights) are TEST-RIG SCALE + CONTENT artifacts — at production human-scale/speed the active zone is large vs view distance so ground loads before you reach it; the look is fixed by saner pack relief + M6 materials/normals + M7 erosion, NOT the render layer. Tuning knobs (NUM_LEVELS, RADIUS_PAGES, far, fog, MORPH_REGION) are all config, set later vs real content.

**Workflow:** `tools/build_rust.ps1` rebuilds Rust without killing the editor (reloadable DLL releases on focus-loss; alt-tab + retry). GDScript/shader changes hot-reload (no rebuild). Local backup at `C:\Backups\worldgen10\` (source+data+git, excludes target/.godot). The debug scaffolding stays in m3_review (harmless, off by default, useful for M4 + LOD tuning). **NEXT: M4 — Facts API (get_height + Jolt collision).**

[superseded by the reset] **M3 slice 8 — visual stability DONE; seam + geomorph fixed, a visual-continuity gate locks it, p99 still green**. The owner's first fly of slice 7 reported "crazy switching" at speed — a real defect the timing/no-black gate could not see. Root cause (code-traced): THREE render-time sampling defects, the height *field* is continuous. (1) The geomorph factor was tile-LOCAL (`cheb=max(|VERTEX.x|,|VERTEX.z|)/half_span`), so with 9 tiles per level the morph fired at every tile edge → an interior morph lattice that swept under motion. (2) The fine UV (`VERTEX.xz/span+0.5`) mapped edge vertices onto the texture BORDER (a half-texel off the texel centers). (3) Pages used a texel-CENTER generation convention, so abutting pages' boundary samples sat one texel apart → a hard inter-tile seam. **Fixes:** (1) geomorph now measured from the 3×3 NEIGHBORHOOD center (normalized to 1.5·span) so it engages only at the level's true outer ring; (2) fine page sampled by true world UV (new `page_origin` uniform); (3) page generation switched to texel-CORNER (`u=px/(N-1)`: texel 0→origin, N-1→origin+span) so abutting pages SHARE boundary samples → seam zero by construction. `height_at()` UNCHANGED → M2 parity unaffected (verified: gpu suite still 2/2). **New `m3_continuity_check`** (windowed): reads back the REAL production pages and asserts abutting shared edges are bit-equal (`seam_east=seam_north=0.0`), plus a perspective-POV morph-banding ceiling (`jump_frac=0.0`). Needs CAN_COPY_FROM on the page textures (added; no render-path cost — p99 held). **m3 suite 6 checks fail=0** (p99=1.88 ms at ~1000 m/s); fast 5, gpu 2; 103 cargo tests green. **M3 still has ONE box left: the owner's RE-fly of `m3_review.tscn`** (the final authority, §7.3) now that the switching is fixed.

[prior] **M3 slice 7 — page-compute caching DONE; the p99 acceptance gate is GREEN**. The slice-6 90 ms spike was redundant per-page CPU setup (recompiling the shader + re-uploading the ~25 MB kernel atlas EVERY page — the dispatch itself is fire-and-forget). Fix: cache the shader+pipeline+6 pack-buffer RIDs ONCE in `Wg10PagePool` (`PageComputeContext`, built at configure / freed at free_all), per-page work shrinks to a uniform set + push constant + dispatch. Re-measured: **p99=2.41 ms (budget 6) | max=3.29 ms | compute-frame max=2.90 ms (was 90) | render-only ≤2.66 ms** at ~1000 m/s. **Async page production NOT needed** — caching alone resolved it. The automated acceptance gate (`m3_accept_check`) is GREEN with a `compute_ms_max<6ms` ceiling locking it in.

[prior] **M3 slice 5b DONE — 3×3 ring tiling + rings↔streamer live wiring, proven under motion**. `Wg10ClipmapRings` rebuilt to N levels × 9 page tiles (each level a 3×3 neighborhood that SURROUNDS the camera; finer-on-top overlap via render_priority); `Wg10TerrainView` drives the live loop via the read-only `get_resident_page` (never computes on the render path) + coarser fallback. `m3_view_check` passes WINDOWED over a 5-position +x sweep across page boundaries: **full coverage** (nonblack≥0.98 — the 3×3 surrounds the camera, fixing 5a's 0.25), real relief, no z-fight (two settled captures pixel-stable), never-black, **view triggers zero compute** after steady state, tile↔page mapping. PNG eyeballed (terrain fills the frame + follows the camera; faint tile-edge lines but no gaps). m3 suite **4** checks fail=0 (m3_rings retired, m3_view added); fast 5, gpu 2 unchanged; **103** cargo tests green. M3 in progress — remaining: fly camera + diagnostics overlay + p99<6ms acceptance gate + manual fly)

---

## [SUPERSEDED — history] M0–M4 state snapshot (pre-pivot; the "NEXT: M5" / "m3 6/6" / "look is downstream" lines below are the OLD, now-disproven framing)

**Phase:** M0 + M1 + first DEM pack + M2 parity + **M3 render (structurally done)** + **M4 Facts API (DONE)** — all green. M4 = the drop-in `Wg10Facts` (RefCounted): `get_height` = `clamp(base + edit-provider.delta, bedrock_floor, ceiling)` (parity-gated base, untouched); `get_collision_field` (sparse CPU, no readback, Jolt-ready — caller owns the body); the adaptable edit seam (circular `StampEdits` + `apply_edit`/`clear_edits`/`set_bedrock`, pluggable provider for future caves); and `bake_collision_region` (GPU bulk, OFF-FRAME readback only). Gates: `facts_check` (fast — no-edit base parity + dig/clamp/clear + collision==point), `facts_collision_parity_check` (gpu — visible==collision on base, maxd 0.0009 m, the §4 don't-float/sink contract), `facts_bake_check` (gpu — GPU bake == CPU collision, maxd 0.0070 m). **cargo 115, fast 6/6, gpu 4/4, m3 6/6.** Deferred (tracked as **Milestone 8** in ROADMAP, not built): VISIBLE edits (composing the edit delta into the GPU render — the meteor crater you SEE; M4 ships collidable-but-not-visible) + edit persistence. **NEXT: M5 (detail/masks)** → M6 biomes/materials → M7 erosion — these (plus content) are where the "squareness/LOD-pop/blobby" look gets fixed; the foundation (gen + perf + parity + facts) is AAA-capable, the look is downstream. Owner's `m3_review.tscn` acceptance fly still welcome as the §7.3 sign-off. Proving-ground + debug scaffolding stay (harmless, off by default).

- Godot 4.6 project at `wg-10/` (Forward+, D3D12, Jolt, .NET `wg10`).
- Native `wg10_terrain` Rust GDExtension **builds and loads in Godot 4.6**.
  `Wg10Hash` (RefCounted) exposes `stable_hash_ints`, `hash_grid`, `value_noise`,
  `fbm`. `Wg10Grammar` (RefCounted) exposes `load_pack_json` + `family_ids` /
  `weight_values` (parallel packed arrays). `Wg10Height` (RefCounted) exposes
  `load_pack_dir` + `height` + `family_signature` queries.
- Deterministic core ported from WG9 into `wg-10/rust/src/hash.rs` (pure, no
  `godot` imports): FNV-1a `stable_hash`, `hash_grid`, `value_noise`, `fbm`,
  `fade`, `smoothstep_unit`. **Bit-exact vs WG9 `hash_reference.json`** (the
  fixture is vendored at `wg-10/worldgen_terrain/fixtures/`).
- **GPU-portable integer hash** `hash::stable_hash_ints(salt: u32, &[i64]) -> u32`
  (`hash.rs`): pure u32-wrapping FNV-1a fold, bit-identical on CPU and GLSL `uint`.
  Golden-value locked. Separate from the bedrock `hash_grid` (64-bit-multiply
  scheme, untouched).
- **Grammar rolls refactored** (`grammar.rs`): the 5 roll sites switched from
  string-join hashing to `stable_hash_ints` with distinct integer salts. New
  seed-space (accepted; WG10 grammar was never a WG9 parity contract). All grammar
  property tests pass unchanged; WG9-bit-exact bedrock untouched.
- **Terrain-pack v1 loader/validation** (`wg-10/rust/src/pack.rs`): schema
  `worldgen10.terrain_pack.v1`, validated on load, rejects malformed packs with
  descriptive errors, never silent defaults. `FAMILIES_PER_PALETTE = 3` fixed.
  `Pack` carries `family_kernels: BTreeMap<String, FamilyKernel>` via loaders
  `load_pack_with_base`/`load_pack_dir`.
- **Pure-Rust NumPy-v1.0 `.npy` reader** (`wg-10/rust/src/npy.rs`): parses
  C-order `<f4`/`<f8` 2-D arrays; rejects bad magic, version≠1, non-float dtype,
  Fortran order, non-2D shape, zero dims, overflowing shape. Descriptive errors,
  no silent defaults.
- **Grammar core** (`wg-10/rust/src/grammar.rs`): region/province locate (floor
  semantics), palette decision, `family_weights` corner blend — bounded, no heap
  allocation, normalized, deterministic, seam-continuous. Produces WEIGHTS ONLY —
  never reads kernel data.
- **Height core** (`wg-10/rust/src/height.rs`, pure, no godot): `sample_kernel`
  (tiled bilinear, scaled to `relief_m` — C0 across footprint seams; visible
  creases at footprint repeats are EXPECTED for naive tiling);
  `moderation` amplitude-only; `height(x,z,seed,&Pack)` = blend each
  grammar-selected family's moderated kernel sample by its weight.
- **First real DEM terrain pack** (`wg-10/worldgen_terrain/packs/dem_v1/`):
  115-kernel approved map across 12 families (coast, badlands, grassland, karst,
  glacial, mountain, rainforest, desert, volcanic, wetland, temperate, tundra),
  6–13 kernels each. Built by `tools/dem_pack/` (Python) from WG9's 602-kernel
  user shortlist + metric-driven family inferences. Rust crate **unchanged** — real
  pack loads through the existing M1/M2 loader/grammar/height interfaces.
  Temperate and tundra rebalanced from 1 kernel each (WG9) to 7 each via 12 new
  DEMs fetched from OpenTopo COP30 (0.5° bbox). Build-time spike filter dropped 3
  corrupt kernels (|Z|>12: Mekong delta z=44, Sahel Chad z=14, South Georgia z=12).
  Kernels are **Z-SCORE normalized** (mean 0, std 1) — height legitimately goes
  negative and can exceed `relief_m`; this is correct. `relief_m`=height_range_m
  (real elevation span ~990–2765 m); `footprint_m`=approx_sample_spacing_m×sample_px
  (~50 km); `footprint_scale` knob exists for M3 visual tuning. Committed gate
  subset only; full set generated on demand. Manual tag review deferred.
- **GPU compute shader** `height_field.glsl` (`wg-10/worldgen_terrain/shaders/`):
  hand-ported GLSL compute shader implementing hash→grammar→height end-to-end.
  Dispatched by `Wg10GpuCompute` (`gpu_compute.rs`), the only new
  RenderingDevice file; packs kernel atlas + coords as storage buffers, reads
  back height + family-signature buffers. **Runs WINDOWED** (headless
  RenderingDevice returns null on this D3D12 setup).
- **CPU/GPU parity gate** (`gpu_parity_check.gd`, `gpu` suite): verified on
  D3D12/RTX 5090 Laptop GPU over 576 coords with synthetic kernels. Tier 1:
  family-selection signatures EXACT (bit-exact). Tier 2: height within f32 epsilon
  (ABS_EPS=1e-2 m, observed max delta 7.67e-5 m — 130× headroom).
  `parity::family_signature` on CPU mirrors the GPU's signature;
  `Wg10Height::family_signature` exposes it.
- **DEM property gate** (`dem_pack_check.gd`, `fast` suite, HEADLESS): asserts
  finite output, bounded by `max_relief×12`, determinism, and height variety across
  a real DEM pack grid.
- **DEM GPU-parity gate** (`gpu_parity_dem_check.gd`, `gpu` suite, WINDOWED):
  dispatches real 512×512 kernels (~25 MB atlas) on D3D12/RTX 5090. Tier-1 family
  signatures EXACT; Tier-2 height maxd=0.040 m on ~6 km relief (within tolerance).
  **This validated the M2 kernel-atlas at real 512×512 scale — the named atlas-at-
  scale risk is closed.**
- **M3 slice 1 — `Wg10PageCompute` native class** (`page_compute.rs`,
  `height_page.glsl`): runs on the GLOBAL RenderingDevice (no readback); writes
  one DEM height page into an R32F `Texture2DRD`. Scene consts drive page
  origin/span/px, grid resolution, camera, height_scale — config-driven, no
  scattered magic numbers.
- **`ring_displace.gdshader`**: spatial shader sampling the `Texture2DRD` in
  `vertex()` to displace a flat ring mesh. Combined with `Wg10PageCompute`, the
  full compute → Texture2DRD → material → displaced-mesh path is proven.
- **`m3_slice1_check.gd`** (`m3` suite, WINDOWED): renders one static page +
  ring + frame, captures to `m3_slice1.png`, asserts real relief (distinct
  quantized colors ≥ 8; flat/black frames fail). Passes: distinct=18,
  nonblack_frac=1.0. Non-vacuous — a flat plane yields 2 buckets → fail.
  PNG inspected by eye: clear mountain/ridge/valley relief visible.
- **M3 slice 2 — `PagePolicy`** (`page_policy.rs`, pure Rust, no godot): the
  eviction bookkeeping — fixed-capacity slots, (level,origin)→slot map, LRU order,
  protected set. Returns DECISIONS (Reuse/Allocate/AllocateEvicting/Full); owns no
  RIDs. The WG9-killer rules proven headless (11 cargo tests): protected pages
  NEVER evicted, budget NEVER exceeded, cache hits reuse the slot,
  all-protected→Full (no panic, no wrong evict), release makes a slot evictable,
  re-acquire re-protects, deterministic, + `rollback(key)` (used on producer
  failure to keep policy/texture state consistent — no phantom slot, no panic, no
  stale content).
- **M3 slice 2 — `Wg10PagePool`** (`page_pool.rs`, godot): THE single owner of
  all page RIDs (the §5.2 anti-WG9 rule). Asks PagePolicy what to do. The ONLY
  texture_create/free_rid for pages live here (3 internal free sites: free_all
  teardown + two produce-failure cleanups). acquire_page/release_page/stats/
  configure/free_all. Eviction REUSES the slot's texture (same dims → zero
  mid-run RID churn).
- **M3 slice 2 — `Wg10PageCompute` refactored to stateless producer:**
  `compute_into_texture` writes height into a pool-provided RID — no longer creates
  or owns textures. Dispatch byte-identical to slice 1 (parity-proven).
  Slice-1 regression-guarded: m3_slice1_check acquires its page via Wg10PagePool;
  still renders distinct=18 byte-identical PNG (rendering preserved).
- **`m3_pool_check.gd`** (`m3` suite, WINDOWED): drives acquire/release on a
  capacity-2 pool; asserts RIDs reuse on hit (created stays 2), budget never
  exceeded (resident≤2), protected page survives over-budget acquire, Full returns
  null (full_events≥1), eviction reuses slot (recomputed, not created), pooled page
  renders distinct=18. Pool driven by explicit acquire/release — NOT a frame loop.
- **M3 slice 3 — `SchedulePolicy`** (`schedule_policy.rs`, pure Rust, no godot): the
  stream-ahead brain. `coverage(pos,vel)` = velocity-led multi-level page ring;
  `coarser_fallback(missing,resident)` = walk up to the first resident coarser
  ancestor (the never-black resolution); `plan_frame(pos,vel,resident)` = bounded,
  **coarsest-first** prioritized acquire/release plan (release sorted, deterministic).
  14 headless cargo tests incl. a 2000-sample LCG **never-black property test**.
  Reuses `page_policy::PageKey` (world-metre origins) — ONE key vocabulary across
  policy/pool/scheduler.
- **M3 slice 3 — `Wg10Streamer`** (`streamer.rs`, godot): the §5.4 frame-loop driver.
  Holds a SchedulePolicy + a Wg10PagePool handle; `update(cam_x,cam_z,vel_x,vel_z)`
  reads pool residency → plan_frame → release departing → acquire ≤ N synchronously
  (a Full/null acquire is served by coarser fallback, not an error). `stats()` +
  `coverage_keys()` expose the loop. Owns NO RIDs, contains NO scheduling math
  (delegates), holds NO meshes. **Async-ready seam:** reads only the *observed*
  resident set, never assumes same-frame residency — a background producer drops in
  behind `acquire_page` later with zero scheduler change.
- **M3 slice 3 — `resident_keys()`** added to `PagePolicy` (Vec<PageKey>, pure,
  tested) and `Wg10PagePool` (flat PackedInt64Array of (level,ox,oz) triples,
  read-only — pool stays the single RID owner). The only pool change.
- **`m3_stream_check.gd`** (`m3` suite, WINDOWED): drives the streamer over a
  synthetic 60-frame straight-line sweep at 6000 m/s and asserts the stream-ahead
  invariants: (1) acquired/frame ≤ max_per_frame, (2) resident ≤ capacity, (3)
  **never-black** — every covered page is resident OR has a resident coarser fallback,
  every frame (coarse blanket warmed via the streamer's OWN loop, not hand-primed),
  (4) determinism — identical per-frame counts across two independent sweeps, (5)
  non-vacuous — the fallback path genuinely fires (`fallback_fired=true`). Passes.
  **This is the first slice driven under MOTION by a live frame loop.** Coarsest-first
  priority + lead/budget tuning (LEAD_FRAMES=8 > one coarse span; MAX_PER_FRAME=3
  absorbs a coarse column/crossing) make never-black STRUCTURAL at this speed.
- **M3 slice 4 — `ring_geometry`** (`ring_geometry.rs`, pure Rust, no godot):
  `RingLayout` (level L span = base_span·2^L; hole = inner level's span → gapless tiling)
  + `band_mesh` (centered XZ lattice + 2 CCW triangles per kept cell; level 0 filled,
  level L>0 a hollow square annulus). 7 cargo tests incl. consistent-winding + hollow-
  center + a `grid_res % 4 == 0` divisibility guard (asserts gapless seam alignment).
- **M3 slice 5b — `Wg10ClipmapRings`** (`clipmap_rings.rs`, godot **Node3D**): rebuilt to
  **N levels × 9 page tiles** — each level a 3×3 neighborhood of one-page full-grid meshes
  (27 `MeshInstance3D` at 3 levels), so the level SURROUNDS the camera. Levels overlap (coarse
  keeps its full 3×3; the finer level draws on top via `ShaderMaterial.render_priority` =
  `num_levels-1-level`); the geomorph blends at the finer's outer edge → gapless by
  construction. `configure` (build-once + grid_res%4 guards), `bind_tile(level,dx,dz,…)`
  (places a tile at its page corner + span/2 and sets its uniforms incl. `coarse_origin` —
  never rebuilds geometry), `level_count`/`tile_count`/`total_vertex_count`/`bound_page_key`.
  Owns NO page RIDs — a pure presenter; the view owns the tile↔page math.
- **M3 slice 4 — geomorph in `ring_displace.gdshader`**: in each level's outer
  transition region (square Chebyshev band of width `morph_region`), `mix(h_fine,
  h_coarse, t)` blends this level's height toward the next-coarser page's height at the
  same WORLD position (via MODEL_MATRIX), `t=1` at the outer edge → adjacent levels agree
  on the seam, no crack/pop. Backward-compatible: `morph_region=0` + coarse_tex==height_tex
  reproduces the slice-1 displacement (slice-1/2 gates still pass byte-identical).
- **M3 slice 5b — `Wg10TerrainView`** (`terrain_view.rs`, godot Node3D): the drop-in terrain
  node + live-loop coordinator. Holds Gd handles to pool/streamer/rings; `update(cam,vel)`
  runs `streamer.update` then, per level per tile (3×3), fetches the page via the **read-only
  `get_resident_page`** (NEVER computes — the anti-WG9 render-path rule) with coarser fallback
  on a miss, and calls `rings.bind_tile`. Page key = `floor(cam/span)·span + (dx,dz)·span` =
  the scheduler's `coverage(radius_pages=1)`, so the view's lookups hit exactly what the
  streamer made resident. Owns NO RIDs/meshes/scheduling math.
- **`m3_view_check.gd`** (`m3` suite, WINDOWED): drives `Wg10TerrainView` over a 5-position +x
  sweep across page boundaries; at each NON-ZERO position renders top-down ortho centered on
  the camera and asserts: **full coverage** (nonblack≥0.98 — the 3×3 surrounds the camera,
  fixing 5a's 0.25), real relief, **no z-fight** (two settled captures pixel-stable in the
  overlap), never-black + budget, **view-zero-compute** (after the streamer reaches steady
  state, created+recomputed stays flat — the view is read-only), **tile↔page mapping** (CPU:
  level-0 tile (1,0) → page origin (BASE_SPAN,0)). status=pass positions=5 tiles=27. PNG
  eyeballed: terrain fills the frame + follows the camera across boundary crossings; faint
  tile-edge lines (visual polish, see watch-items) but no gaps. (The slice-4 `m3_rings_check`
  one-page gate was retired — its geometry is gone; this supersedes it.)
- Gate runner: `python tools/gate.py --suite fast` → `[gate] suite=fast checks=5
  fail=0` (headless). `--suite gpu` → `[gate] suite=gpu checks=2 fail=0 skip=0`
  (windowed). `--suite m3` → `[gate] suite=m3 checks=5 fail=0 skip=0` (windowed; incl. m3_accept p99 GREEN).
- Three living docs (DESIGN, ROADMAP, STATUS). Architecture locked — see DESIGN.

## What works

- **Deterministic hash/noise bedrock, proven bit-exact** against WG9 — at both
  the Rust unit level and through the Godot native boundary (hash parity +
  determinism gates).
- **Grammar property gate** (`grammar_check.gd`, fast suite): asserts sum=1,
  determinism, id/weight array parallelism, and family variety across a region
  grid (no single-palette collapse).
- **Height property gate** (`height_check.gd`, fast suite): asserts finite output,
  determinism across two independent calls, bounded output within pack relief
  range, and variety across a grid (no flat-collapse).
- **DEM property gate** (`dem_pack_check.gd`, fast suite): finite, bounded
  (max_relief×12), deterministic, varied — on real DEM pack kernels. HEADLESS.
- **Fast suite: 5 checks, fail=0** (headless).
- **GPU parity gate** (`gpu_parity_check.gd`, `gpu` suite): family selection EXACT
  + height within f32 epsilon on D3D12/RTX 5090; runs windowed. Returns SKIP code 2
  on no-GPU/headless box.
- **DEM GPU-parity gate** (`gpu_parity_dem_check.gd`, `gpu` suite): real 512×512
  kernels (~25 MB atlas) dispatched + read back on D3D12/RTX 5090. Tier-1 EXACT,
  Tier-2 maxd=0.040 m on ~6 km relief. Validates M2 atlas at real scale — atlas-
  at-scale risk closed.
- **GPU suite: 2 checks, fail=0** (windowed).
- **M3 slice-1 gate** (`m3_slice1_check.gd`, `m3` suite, WINDOWED): distinct=18,
  nonblack_frac=1.0, fail=0. One static page, one ring, one frame — Texture2DRD→
  material→displaced-mesh path proven. PNG inspected: real DEM mountain/ridge/
  valley relief visible. (Regression-guarded through slice 2: still passes
  distinct=18 after the pool refactor.)
- **M3 pool gate** (`m3_pool_check.gd`, `m3` suite, WINDOWED): capacity-2 pool,
  explicit acquire/release. created=2 (RID reuse on hit), resident≤2 (budget
  enforced), full_events≥1 (Full path exercised), pooled page distinct=18.
- **M3 stream gate** (`m3_stream_check.gd`, `m3` suite, WINDOWED): Wg10Streamer over
  a 60-frame 6000 m/s sweep — bounded work (≤max_per_frame), budget (≤capacity),
  never-black (every covered page resident or coarser-fallback-resident, every
  frame), determinism (two independent sweeps identical), non-vacuous
  (`fallback_fired=true`). status=pass. First slice driven under MOTION.
- **M3 rings gate** (`m3_rings_check.gd`, `m3` suite, WINDOWED): 2-level rings + real DEM
  pages, top-down ortho. nonblack=1.000 (no holes), distinct=17 (real relief), seam
  continuity + morph continuity (crack-free level-0/1 boundary), verts=8450 unchanged
  after recenter (translate, not rebuild). PNG eyeballed: nested rings, continuous seam.
- **m3 suite: 4 checks, fail=0** (windowed). fast=5, gpu=2 unchanged.
- **103 Rust unit/property tests green** (96 prior + 7 ring_geometry). One exact-value
  anchor: all-flat pack yields `height == 500.0` at any coord. The SchedulePolicy
  never-black property is a 2000-sample LCG sweep; ring_geometry asserts consistent
  winding + gapless hollow bands + grid_res%4 divisibility.
- **Verification shape for M3:** windowed + visual + invariant. The render gates
  (slice 1/2) prove the render path; the stream gate (slice 3) proves the scheduling
  invariants under motion; the rings gate (slice 4) proves seamless multi-level geometry
  + cheap recenter. Value-correctness leans on the M2 gpu_parity gate. Global
  RenderingDevice is null under --headless on this D3D12 box — same constraint as the gpu
  suite. SKIP code 2 returned on no-GPU/headless box.
- Slices 3+4 give a velocity-aware scheduler driving seamless clipmap ring geometry that
  recenters cheaply and never goes black. NOT yet present: a fly CAMERA (WASD/mouse) +
  movement controller, diagnostics/UI overlay, a perf number (p99), or a manual fly-test —
  the rings gate uses a SCRIPTED camera + recenter, not interactive flight. The scheduler
  (slice 3) and the rings (slice 4) are not yet wired together in a live loop (the rings
  gate binds pages directly for a static capture). M3 milestone OPEN. (Honest baseline —
  slice 4 proves "the rings render seamless terrain and recenter without rebuilding under a
  scripted move"; wiring rings↔scheduler under a real fly camera + the p99 acceptance gate
  is the remaining M3 work.)

## What's next

1. **M3 close-out — the OWNER's manual fly (the ONLY thing left for M3).** The render pipeline
   is complete and the automated acceptance gate is GREEN: p99=2.41 ms at ~1000 m/s, no-black,
   never-stall. Per §7.3, gate-green is necessary but NOT sufficient — the owner's live fly is
   the final authority. **To do this:** launch `wg-10/worldgen_terrain/harness/m3_review.tscn`
   windowed (the Godot editor → run that scene, or
   `Godot_console.exe --path wg-10 res://worldgen_terrain/harness/m3_review.tscn`). Controls:
   **WASD** move, **Shift** sprint (to ~1000s m/s), **mouse** look, **Space/C** up/down, **ESC**
   release mouse. Watch the HUD (top-left): fps, frame p99 (should stay well under 6 ms),
   resident pages. **Confirm:** terrain surrounds you, follows smoothly at speed, no stalls/
   hitches crossing page boundaries, no black holes/gaps. If it feels right → M3 is DONE; tell
   me and I'll mark the milestone closed and move to M4. If anything's off → that's a real
   finding, tell me what you saw and I'll fix it.
2. **Visual tuning of `relief_m` / `footprint_m`** (deferred to M3): physical
   ground-truth values in place; visual feel needs the renderer. `footprint_scale`
   knob exists for then.
3. **Tile-edge lines** (visual polish, surfaced slice 5b): faint lines at page-tile
   boundaries (per-page bilinear edge / no cross-page filter). Not gaps — coverage=1.0.
   Fix later with a 1-texel page overlap or edge clamp.
4. **Full-pack streaming** (deferred to M3): gate-committed subset loads now;
   full ~115-kernel set is generated on demand but not yet streamed.
5. **Anti-repetition / kernel variety tuning**: naive single-kernel tiling
   visibly creases at footprint seam boundaries (C0 not C1); deferred until the
   renderer can show it.

## Decisions locked

- Native backend: **Rust GDExtension** (carried forward from WG9).
- Renderer acceptance budget: **frame p99 < 6 ms at ~1000 m/s**.
- Finest-ring spacing / ring count: **config-driven, value deliberately not
  locked** — tune against real assets later.

## Known risks / watch-items

- OpenTopo kernel methodology REVIEWED 2026-05-28 (see DESIGN §9): sound, cache
  is sufficient, no blocking issues. Two follow-ups for future packs: mask NoData
  holes properly; improve family tagging (591/703 WG9 kernels were `uncategorized`;
  dem_v1 approved map covers 115 across 12 families, tag accuracy unreviewed).
- Grammar↔kernel coupling RESOLVED 2026-05-29 (see DESIGN §9): moderation is
  amplitude-only in the height layer; grammar never reads kernel data.
- **GPU kernel-atlas for varied sizes — CLOSED 2026-05-29** (see DESIGN §9):
  validated on real 512×512 kernels at ~25 MB atlas; no redesign needed.
- **DEM kernel Z-score normalization:** height is NOT [0,1]; goes negative; can
  exceed `relief_m`. Build-time filter drops |Z|>12 spikes. Normal behavior —
  document clearly for any M3 shader work that consumes the pages.
- **Manual tag review deferred:** dem_v1 approved map seeded from confidence≥0.7
  metric inferences; no human thumbnail review done. Tooling ready for when it is.
- Naive kernel tiling creases at footprint seam boundaries (C0, not C1) — expected
  behavior; deferred until the renderer can show it.
- Finest-ring spacing affects near-detail radius and interacts with future
  asset/texture scale; tune against real assets in M3.
- **GPU compute is windowed-only:** `Wg10GpuCompute`, `Wg10PageCompute`, and all
  `gpu`/`m3` gates require a windowed run; headless returns null RenderingDevice
  on this D3D12 setup. SKIP code 2 is returned on no-GPU/headless box — never
  miscounted as a pass.
- **Texture RID ownership — RESOLVED slice 2:** `Wg10PagePool` is now the single
  owner of all page RIDs (DESIGN §5.2). free_all/teardown + two produce-failure
  cleanup sites cover every allocation. The slice-1 one-shot is regression-gated
  via the pool path.
- **Slice-4 carry-forwards — CLOSED (slice 5a):** (1) per-level page span — `acquire_page`
  now computes a level-L page over `world_span·2^level` (was flat, only correct at L0).
  (2) geomorph `coarse_origin` — the coarse sample is corner-relative
  `(world.xz − coarse_origin)/coarse_span`, so the seam stays closed off-origin. Both
  proven by the slice-4 rings gate (distinct=41) under the new convention.
- **Render-path compute — GUARDED (slice 5a):** a CONSUMER (e.g. the future view) must
  fetch pages via the read-only `Wg10PagePool::get_resident_page` (returns a resident page's
  texture or null, NEVER computes), not `acquire_page` (which synchronously dispatches GPU
  compute on a miss). Only the streamer's `acquire_page` may produce pages, bounded per
  frame. A view that called `acquire_page` would reintroduce WG9's synchronous-compute-under-
  motion disease — the moving gate caught exactly this; the read-only accessor is the fix.
- **Clipmap level surrounds the camera — RESOLVED (slice 5b):** each level is now a 3×3
  page neighborhood (N levels × 9 one-page tiles in `Wg10ClipmapRings`; finer-on-top overlap
  via render_priority). `m3_view_check` proves nonblack≥0.98 (full coverage) at non-zero
  camera positions under motion — 5a's 0.25 is fixed. `Wg10TerrainView` drives the 3×3 live
  loop read-only (zero view compute, asserted). The scheduler/pool/rings/view all share the
  `floor(cam/span)·span` page-key convention.
- **Tile-edge lines — visual polish, DEFERRED (not a gap):** the 3×3 render shows faint lines
  at page-tile boundaries (each tile samples its own page texture; no cross-page filtering /
  edge clamp). NOT holes/cracks — coverage=1.0 and the gate confirms continuity; relief is
  continuous across them. Cause: bilinear at each page's texture edge. Fix (later visual
  tuning, not a correctness slice): sample with a 1-texel page overlap or clamp/extend page
  edges. Recorded so it isn't mistaken for a seam failure.
- **Overlap overdraw — a p99 input (slice 5b → acceptance gate):** the 3×3 levels OVERLAP
  (the finer 3×3 over the coarse center, finer drawn on top). This is FIXED, bounded overdraw
  (not free) — recorded as an explicit input to the M3-closing p99<6ms acceptance gate, where
  it's measured under the real fly camera. If p99 is tight, the toroidal-rebind + hollow-coarse
  optimizations are the known levers (deferred until measured).
- **View vs streamer key alignment under MOTION (honest correction, slice-5b audit):** the
  view queries the camera-position 3×3 (`floor(cam/span)·span + (dx,dz)·span`); the streamer's
  `coverage` uses a VELOCITY-BIASED centre (`cam + vel·lead_frames`). They coincide exactly at
  vel=0. Under motion the streamer prefetches AHEAD, so the camera-position fine pages are in
  the streamer's coverage only while `vel·lead_frames < ~1.5·span` — beyond that the view
  correctly falls back to coarser at the camera position (never-black, by design; NOT a bug).
  At the M3 target **~1000 m/s** with `lead_frames=8`, bias = 8000 m < 1.5·8192 → fine pages at
  the camera ARE covered, so the p99 gate runs in the safe range. The fly-cam slice must
  CONFIRM this empirically and, if fine detail lags at speed, tune `lead_frames` down or widen
  the streamer `radius_pages`. (The 5b gate's 6000 m/s warm-up has a 48 km bias — fine detail
  lags there, but coverage is still ~1.0 via the coarse blanket, which the gate verifies.)
- **Tile-bind minor follow-ups (slice-5b audit, non-blocking):** (a) both-null fallback leaves
  a tile at its previous transform (stale-but-bounded; the coarse blanket makes it transient) —
  add a clarifying comment. (b) `bound_page_key` returns `Vector2i` (i32) truncating the i64
  page origin — fine for M3 scale, revisit at M4 planetary scale.
- **Async page production — NOT NEEDED (slice 7 resolved the spike via caching).** The
  slice-6 p99 gate's 90 ms "compute" spike turned out NOT to be GPU work or genuinely-expensive
  compute (the dispatch is fire-and-forget) — it was **redundant per-page CPU setup**:
  recompiling GLSL→SPIRV + re-uploading the ~25 MB kernel atlas EVERY page. Slice 7 caches those
  once (`PageComputeContext` in `Wg10PagePool`): compute-frame cost dropped 90 ms → **2.9 ms**,
  p99 → **2.41 ms** (budget 6). So the async/threading path was the WRONG fix (it would just move
  redundant work to another thread, with `RenderingDevice`-thread-safety risk). The async-ready
  seam remains valuable for the future (M5–M7 genuinely-multi-pass pages may re-fire the trigger
  with a real per-page cost — *then* it's the lever), but it is NOT needed for M3.

## Build / run gotchas (learned 2026-05-28 wiring the toolchain)

- **`CARGO_TARGET_DIR` is set globally on this machine** (to
  `D:\cargo-target-kalshi`). It OVERRIDES `wg-10/rust/.cargo/config.toml`'s
  `target-dir`, so `cargo build`/`cargo test` send output to the global dir and
  the `.gdextension` can't find the dll. **Unset it per-invocation** when
  building/testing this crate: `$env:CARGO_TARGET_DIR=$null; cargo build`. The
  committed `.cargo/config.toml` makes the local layout correct on a clean
  machine (no global var) — it's only this machine that needs the unset.
- **`.gdextension` library path is `res://rust/target/debug/wg10_terrain.dll`** —
  resolved from the PROJECT ROOT, not relative to the `.gdextension` file.
  Godot `res://` cannot escape the project root with `..`.
- **GDExtension only loads after an editor import pass** writes
  `.godot/extension_list.cfg`. A bare `--headless --script` run on a clean
  checkout will NOT register `Wg10Hash`. `tools/gate.py` runs
  `--headless --import` first to handle this; do the same for any new check.
- **`--quit` without a main scene pops a blocking ALERT dialog** (even headless).
  Use `--script` (SceneTree) for checks, never `--quit`, in automated runs.
- Headless is fine for this pure-CPU layer; GPU work (M2+) won't run headless.

## Reference

- Predecessor: `d:/workflows/worldgen9` — read for knowledge (formulas,
  contracts, lessons); do not copy code. Its render layer is the cautionary
  tale (per-chunk synchronous GPU pages → 128 ms/chunk → black slabs + 5 fps at
  speed).
- Godot binary used for gates:
  `C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe`
