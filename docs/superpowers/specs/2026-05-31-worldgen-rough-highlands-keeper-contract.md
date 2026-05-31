# WorldGen10 - Rough-Highlands Keeper v1 Contract

**Date:** 2026-05-31
**Milestone:** Phase 5 / Slice 2A-close implementation bridge.
**Status:** candidate keeper contract frozen for fixtures; owner accepted direction and bounded seam
visibility, but full terrain/gameplay acceptance and Rust/GLSL runtime acceptance remain open.
**Parents:** `docs/plans/ROADMAP.md` Slice 2A-close,
`docs/superpowers/specs/2026-05-31-worldgen-slice2a-geography-engine-design.md`, and
`docs/superpowers/specs/2026-05-31-worldgen-phase7b-drainage-skeleton-design.md`.

---

## 1. Purpose

This spec freezes the current rough-highlands keeper as an implementation target that can be tested. It turns
the Python/Godot review work from "scripts that looked good" into a named contract with deterministic fixtures,
golden review output, public/private fact boundaries, and port gates.

This is not a port greenlight. The owner accepted the rough-highlands direction and the bounded 3x3 seam
visibility. The next terrain/gameplay question is still whether the keeper has enough traversable structure and
travel pacing beyond the seam proof.

## 2. Identity

- Keeper id: `rough_highlands_keeper_v1`
- Review generator version: `rough_world_chunks_v2_independent_windows`
- Scenario: `rough_anchor` / `rough anchor`
- Source of height contract: `tools/dem_pack/export_godot_rough_world_chunks.py::_compose_windowed_height`
- Source of skeleton facts: `tools/dem_pack/geography_skeleton_windows.py`
- Review scene: `wg-10/worldgen_terrain/harness/rough_world_chunks_review.tscn`
- Contract fixture: `tools/dem_pack/fixtures/rough_highlands_keeper_v1.json`
- Contract exporter: `tools/dem_pack/export_rough_highlands_keeper_contract.py`
- Contract tests: `tools/dem_pack/test_rough_highlands_keeper_contract.py`

## 3. World/Window Contract

The keeper is a deterministic function of seed, world coordinates, and generator version. Adjacent chunks must
not own independent random state. They are independently requested, but every request builds from the same
world-coordinate skeleton rules:

- chunk span: `25,600 m`
- review chunk resolution: `129 x 129`
- fixture chunk resolution: `33 x 33`
- fixture spacing: `800 m`
- window apron: `25,600 m`
- review seeds: `133`, `211`
- review origin: `(60000 m, 36000 m)`

Only a window core is authoritative. The apron exists for routing, smoothing, and edge normals; it is not a
second owner for neighboring core samples.

## 4. Facts Boundary

Candidate public runtime facts:

- `uplift`
- `routed_surface`
- `discharge`
- `tributary`
- `channel_axis`
- `crest_dist`
- `channel_dist`

Private height-material fields:

- `ridge_detail`
- `shoulder_detail`
- `route_texture`
- `small_detail`

Review-only overlays:

- terrain color
- slope bands
- seam guides

The port must not expose review overlays as runtime facts, and it must not flatten public skeleton facts into
unrelated local noise. If render height uses the skeleton, CPU facts/collision must be able to query the same
skeleton facts or a documented composed height derived from them.

## 5. Height Composition

Height is composed in this order:

1. Build world-anchored skeleton window facts.
2. Apply a small world-coordinate recursive domain warp for local material only.
3. Sample private detail fields (`ridge_detail`, `shoulder_detail`, `route_texture`, `small_detail`).
4. Derive masks from skeleton facts: `crest_near`, `channel_near`, `routed_cut`, `wet_floor`, `highland_mask`.
5. Compose uplift/fill, crest/shoulder/ridge material, routed incision, wet-floor lowland damping, and local
   unrouted ridge texture.
6. Apply `tanh(height * 1.18)`.

The exact numeric thresholds and weights are frozen in
`tools/dem_pack/fixtures/rough_highlands_keeper_v1.json` under `height_composition_contract`.

## 6. Corridor Contract

The exported corridor mask is a review/runtime-candidate route fact, not a gameplay navmesh. It is built from:

- `channel_axis >= 0.16`
- or `channel_dist <= spacing_m * 6`
- or private `route_axis >= 0.22`

The first two inputs are public skeleton-fact candidates. The `route_axis` branch is private height material in
the current keeper and must be either promoted deliberately or replaced before a runtime public route API claims
the corridor as an authoritative gameplay path.

## 7. Scale/Relief Policy

The chunk review scene uses fixed vertical relief:

- `review_height_scale_m = 260`
- relief multiplier default: `1.0`
- relief policy: `k=0` fixed vertical relief as horizontal span changes

This is a review policy, not the future runtime scale solution. Runtime still needs Phase 5 Slice 5's separated
content-scale and per-level-resolution policy.

## 8. Fixture And Golden Output

The fixture contains:

- fixed sample points for seeds `133` and `211`;
- normalized height, review metres, corridor boolean, and all public skeleton facts at those points;
- seam/visual-seam/variation/virtual-travel summaries;
- a reproducible contact-sheet PNG SHA-256 for the 96 px panel review sheet.

Regenerate with:

```text
python tools\dem_pack\export_rough_highlands_keeper_contract.py
```

Verify with:

```text
python -m pytest tools\dem_pack\test_rough_highlands_keeper_contract.py -q
```

Any intentional algorithm change must update the fixture, the contract spec, and the owner-facing review
artifact together. A fixture drift without a documented keeper-version change is a regression.

## 9. Slice 3 Port Gate

Before Rust/GLSL work:

- keep this fixture green against Python;
- decide whether the current private `route_texture` corridor branch is runtime-public or review-only;
- add Rust CPU parity against the fixture sample points;
- add Rust adjacent-window seam tests using the same world/window contract;
- only then mirror the accepted composed height in GPU code;
- visible render, CPU facts, and collision must agree on composed height within the existing parity envelope.

Recommended first implementation target, once the owner wants a port: **Rust CPU skeleton-facts core with Python
fixture parity**, not GPU first. The skeleton/facts contract is the load-bearing piece; GPU mirroring comes after
the CPU facts and seam gates are stable.
