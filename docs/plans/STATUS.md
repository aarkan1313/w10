# WorldGen10 — Status

What is actually true right now. Update this whenever reality changes. If a
manual fly contradicts a claim here, fix this file immediately. (Separating
"what passed a counter gate" from "what is actually accepted" is the whole
point — see DESIGN §7.3.)

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

## Current state

**Phase:** M0 + M1 (bedrock/grammar/height) + first DEM pack + M2 GPU parity ALL green. **M3 render pipeline: leaves (pool/policies/streamer/ring_geometry/page_compute) proven; the PRESENTATION layer is being rebuilt via the prove-one-at-a-time RESET** (`proving_ground.tscn`, owner-flown). Steps 1–7 done (proving ground) AND **folded back into the real `Wg10TerrainView`/`Wg10ClipmapRings`** — all gates green on the rebuilt path (m3 6/6, gpu 2/2, fast 5/5, cargo 103). **M3 needs only the owner's acceptance fly of `m3_review.tscn` (real components).** Then retire the proving ground → M4. The visible "squareness/lines" are extreme DEM data (M6/M7 content fix), not a render defect.

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
