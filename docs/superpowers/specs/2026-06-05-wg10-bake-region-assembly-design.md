# WG10 bake_region Assembly — Design

Date: 2026-06-05
Status: design; owner approving. Continuation of the carve-port arc
(`2026-06-04-wg10-connected-carve-to-live-path-design.md`).

## Purpose

All three offline "baked look" pieces are now ported to Rust and parity-verified
individually: carve routing (`pass_network::carve_routes`, bit-exact), carve_ramp
(`pass_network::carve_ramp_delta`, 99% bit-identical), and condition_world
(`condition_world::condition_world`, interior bit-exact). This spec assembles them
into ONE pure-Rust function — `bake_region` — that produces a conditioned + carved
region height the way the accepted "mountain chunk network" look is built, and
gates the ASSEMBLED result end-to-end against a Python oracle.

This is the last pure-Rust step before the live-producer integration. Landing it
means the ENTIRE offline look-pipeline is reproduced in Rust, verified — leaving
only the producer/GPU/LRU plumbing for a later session.

## The pipeline being assembled

The accepted look is (Python `mountain_world_layer.build_network_world:480-488`):
`macro = mountain.generate(...)` → `carved = raw + carve_pass_network(raw).delta`
→ `height, stats = condition_world(carved)`. The composition ORDER is load-bearing:
**carve operates on the RAW macro field, THEN condition_world normalizes the carved
result** (not the reverse).

## CRITICAL scoping decision (owner-approved): seam-safe branch, not full-field

`build_network_world` calls `mountain.generate` with NO apron → the **full-field
diagnostic branch** (per-window `zscore`/`norm01`). The Rust `recipes::mountain_seamsafe`
ports the **seam-safe branch** (`apron_px>0`, fixed affine constants, DoG channels) —
a DIFFERENT branch by design (mountain_synthesis.py module docstring). They do not and
should not bit-match.

`bake_region` therefore assembles + gates the **seam-safe pipeline** — the path the
LIVE runtime actually uses — NOT `build_network_world`. Every Rust piece is already
parity-proven on the seam-safe branch. The Python oracle for the gate runs the SAME
seam-safe branch (the `mountain.generate(..., apron_px=MOUNTAIN_APRON_PX, flow_on=...)`
invocation, as in `test_mountain_world_layer_contract._live_seamsafe_page`), then the
SAME carve + condition. (Porting the full-field branch is explicitly NOT in scope; the
live runtime uses seam-safe, so the full-field path may never run live.)

### Pillar check (owner gate: "if it follows pillars")
- **Seamless** ✅ — seam-safe branch is defined by seam-exactness (fixed affine, no
  per-window stats); this is the MORE seam-safe choice.
- **Fast** ✅ — measured: carve ~19 ms, condition ~2 ms, macro is the existing recipe;
  all CPU, off-frame.
- **Infinite-procedural** ✅ — condition_world needs GLOBAL percentiles, valid because
  the bake is over a finite REGION (a baked tile), which IS the "baked look,
  procedurally" north star (generate per-region on demand, not pure infinite f(x,z)).
- **Parity-safe** ✅ — every piece parity-gated; the assembly adds an end-to-end gate.

## Components

- **NEW: `wg-10/rust/src/bake_region.rs`** — `bake_region(wx, wz, n, seed, feature_span_m,
  apron_px, spacing_m, span_m, height_scale_m, flow_on, pass_params, ramp_params)
  -> BakeResult { height: Vec<f64> (conditioned+carved, n*n), stats: ConditionStats,
  carve_delta: Vec<f64> }`. Pure Rust. Composes:
  1. `let raw = recipes::mountain_seamsafe(wx, wz, n, n, seed, feature_span_m, apron_px, spacing_m, flow_on)`.
  2. `let routes = pass_network::carve_routes(&raw, n, span_m, height_scale_m, &pass_params, &traverse_from(pass/ramp))`.
  3. `let carve_delta = pass_network::carve_ramp_delta(&raw, n, span_m, height_scale_m, &routes, &ramp_params)`.
  4. `let raw_carved: Vec<f64> = raw[i] + carve_delta[i]`.
  5. `let (height, stats) = condition_world::condition_world(&raw_carved, n)`.
  Return all three. (Routes are an intermediate; carve_delta + height + stats are the
  outputs the producer will eventually consume.)
  > NOTE: `carve_routes` needs a `TraverseParams`; `carve_ramp_delta` needs `RampParams`.
  > The Python `carve_pass_network` derives `p_trav` from span/height_scale and uses
  > `PassNetworkParams`. The Rust assembly must construct the same param values the Python
  > oracle uses (slope_budget etc.) so routes match — the fixture will carry them.
- **NEW: `tools/dem_pack/export_bake_region_fixture.py`** — runs the Python seam-safe
  pipeline (seam-safe `mountain.generate` + carve + condition) over a known region and
  emits `tools/dem_pack/fixtures/bake_region_fixture.json` (wx/wz or the grid spec, all
  params, and the Python `height` + `carve_delta` + stats).
- **NEW: `tools/dem_pack/fixtures/bake_region_fixture.json`** — the oracle.
- **Modify `wg-10/rust/src/lib.rs`** — `mod bake_region;` + `#[cfg(test)] mod bake_region_tests;`.
- **NEW: `wg-10/rust/src/bake_region_tests.rs`** — end-to-end parity gate.

## Data flow

```
wx,wz grid (region, seam-safe apron)
  -> mountain_seamsafe(...)               = raw macro (seam-safe branch)
  -> carve_routes(raw) -> carve_ramp_delta(raw, routes)  = carve_delta (<=0)
  -> raw + carve_delta                    = raw_carved
  -> condition_world(raw_carved)          = (height, stats)   [the conditioned+carved region]
```

## Testing

- **End-to-end parity gate** (`bake_region_tests.rs`): load the fixture, run Rust
  `bake_region` with the fixture's grid+params, compare `height` to the Python `height`.
  TOLERANCE gate (the carve_ramp EDT-tie residual + condition_world gaussian-border
  residual both flow through), reported as mean/p99/peak metres. EXPECTED: p99 small
  (the per-piece residuals are tiny: carve_ramp p99=0, condition interior bit-exact);
  set the budget from the measured p99 ×1.5, with a "huge p99 = real assembly bug
  (wrong order / wrong param / wrong intermediate)" backstop. ALSO assert `carve_delta`
  matches (it's the carve_ramp output, already gated, so this is a composition check) and
  the `stats` percentiles match the Python stats to ~1e-9.
- **Non-vacuous:** assert the region actually carved (some carve_delta < 0) AND
  conditioning actually bounded the field (stats.conditioned_ptp finite + height in ~[-1,1]).
- **No regression:** full `cargo test -p wg10_terrain --lib` stays green (250 + new).

## Error handling

- Mismatched array lengths / bad params → return a clear error or panic in the test
  (this is a pure compute function; the producer wrapper later handles runtime guards).
- Empty routes (no crossable barrier) → carve_delta all-zero; condition still runs;
  height = conditioned raw. Valid (a region with no pass network).

## Known boundary to validate later (NOT in this spec, flagged honestly)

**Per-region condition_world percentiles are a potential cross-region seam risk.**
condition_world normalizes by percentiles computed over THE REGION. Two adjacent
baked regions have different percentile sets → their shared border conditions slightly
differently → a possible seam in the conditioned height. The seam-safe macro + carve
are seam-exact; only the condition normalization varies by region. This spec does NOT
solve it (bake_region is single-region). When the producer bakes MULTIPLE regions
(next session), this must be validated/addressed — options: large regions so borders
are rare, overlap/blend at region seams, or a shared/quantized percentile fact. Recorded
so it is not a silent surprise during the producer integration.

## Out of scope (next session)

- Wiring `bake_region` into `Wg10PagePool` (a producer kind / off-frame bake).
- The region-fact LRU cache + page sampling from a baked region.
- GPU-macro / CPU-carve coordination (here macro is the CPU recipe oracle; the live
  producer will run macro on GPU then carve CPU — that coordination is producer work).
- The cross-region condition seam (above).
- Porting the full-field `mountain.generate` branch (live uses seam-safe).
