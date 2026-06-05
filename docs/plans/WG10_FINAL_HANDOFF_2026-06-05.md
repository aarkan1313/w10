# WG10 Final Handoff - 2026-06-05

> **SUPERSEDED.** This doc predates the carve-port arc. The CURRENT pickup point is
> `docs/plans/WG10_HANDOFF_2026-06-05_CARVE_PORTED.md` (carve fully ported to Rust,
> all-11 biome parity confirmed, bake_region assembled, producer wiring next). Read
> that + `docs/plans/STATUS.md` top. The content below is kept for history only.

Purpose: stop this recovery session with one concise pickup point for the next
chat. The next chat should continue from the progression harness and add one
feature at a time. Do not restart the old mixed-mode investigation.

## Current Branch

- Branch: `slice4-gpu-page-integration`
- Latest scoped runtime/progression checkpoint before this handoff:
  `282b2ac feat(slice4): gate progression pass-network facts`
- Tag: `backup-slice4-pass-network-progression-20260604-282b2ac`
- Previous handoff-scope doc checkpoint:
  `0c04aaf docs(slice4): clarify progression audit scope`
- Tag: `backup-slice4-audit-scope-20260604-0c04aaf`
- Previous hitch fix:
  `6860e6a fix(slice4): reduce material fact hitch`
- Tag: `backup-slice4-material-hitch-20260604-6860e6a`

The worktree still contains many preexisting modified Rust files and untracked
biome/harness/doc artifacts. They are not part of the latest scoped commits. Do
not clean, reset, or broad-checkout the tree.

## Current Truth

- Mode `1` / `REFERENCE` is the accepted mountain-network visual baseline.
- Mode `2` / `MOUNTAIN/network_ref` is a reference-backed bridge. It uses the
  accepted payload for height/material/facts and is not final procedural biome
  synthesis.
- Mode `2` / `MOUNTAIN/close_debug` is the raw live mountain recipe candidate.
  It is prototype-only and is expected to report missing material/pass-network
  facts.
- Mode `3` / `WORLD` is diagnostic. Owner preview binds accepted reference
  height/material facts while keeping route/weight reports live.
- Modes `1`, `2`, and `3` matching in normal review is intentional until a
  generated/procedural world-layer candidate is proven.
- Final terrain textures have not been started. Current material presentation is
  a simple shader palette plus low-resolution material fact masks for
  readability only.

## What Is Proven

Latest pass-network progression proof:

```powershell
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'; python tools\gate.py --suite review_progression
```

Result after `282b2ac`:

- `wg10_progression_review_check.gd` passed.
- `wg10_progression_motion_check.gd` passed.
- `wg10_progression_repage_visual_check.gd` passed.
- Suite result: `review_progression` = 3/3.
- Motion proof: all four steps had zero hide/show/full events,
  `repage=26`, `repage_frame_max=8`, and `acquired_max=1`.
- Fixed-camera visual proof: 12 L0/L1/L2 boundary checks passed with worst
  mean/p95/p99 RGB delta `0.000831/0.002614/0.020915`.

Latest strict owner-stress proof from the material-hitch recovery:

- `review_runtime_stress` = 1/1.
- All six REFERENCE/MOUNTAIN/WORLD morph off/on cases stayed under the
  `16.7 ms` CPU p99/max and GPU p99 budgets.
- Hide/show/full events remained zero.
- Bridge captures matched where MOUNTAIN/network_ref and WORLD preview are
  expected to match REFERENCE.

## Progression Harness State

Primary scene:

- `wg-10/worldgen_terrain/harness/wg10_progression_review.tscn`
- `wg-10/worldgen_terrain/harness/wg10_progression_review.gd`

Active ladder:

1. `reference_baseline`
2. `mountain_network_bridge`
3. `mountain_close_debug_candidate`
4. `world_reference_preview`

Implemented review features:

1. `source_display_report` plus visible source/display overlay.
2. `material_fact_report` plus visible material-fact overlay.
3. `pass_network_report` plus visible pass-network overlay.

The smoke gate proves all reports and overlays. Probe mode hides overlays so
motion and pixel-delta checks continue to measure the terrain render path.

## Next Work

Next single feature: procedural/generated mountain world-layer candidate.

Acceptance for that next step:

- It must enter the progression harness as a separate candidate, not by
  changing the accepted REFERENCE or MOUNTAIN/network bridge.
- It must consume or generate the same contract shape: source/display mapping,
  material facts, pass-network facts, conditioning facts, and eventually
  facts/collision parity.
- It must reduce the measured visual/numeric gap to REFERENCE.
- It must keep `review_progression`, `review_runtime_visual`, and
  `review_runtime_stress` green.

Do not do next:

- Do not chase final textures.
- Do not make modes `2` or `3` look different by color tuning.
- Do not call raw `MOUNTAIN/close_debug` accepted.
- Do not turn full WORLD compose back on synchronously in the owner fly.
- Do not refactor by line count alone. The current source-size audit found no
  active Rust/GDScript/GLSL/Python terrain source file over 1000 lines; the
  actual refactor target is ownership and mode taxonomy.

## First Files To Read

1. `docs/plans/STATUS.md`
2. `docs/plans/ROADMAP.md`
3. `docs/plans/WG10_IMPLEMENTATION_SPEC_AUDIT_AND_VALIDATION_PLAN_2026-06-04.md`
4. `docs/plans/WG10_ARCHITECTURE_BASELINE_AUDIT_2026-06-04.md`
5. `wg-10/worldgen_terrain/harness/wg10_progression_review.gd`
6. `wg-10/worldgen_terrain/tests/wg10_progression_review_check.gd`

## Pickup Commands

Run Godot gates serially; do not run multiple Godot suites in parallel because
the GDExtension DLL import/copy path can race.

```powershell
git status --short
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'; python tools\gate.py --suite review_progression
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'; python tools\gate.py --suite review_runtime_visual
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'; python tools\gate.py --suite review_runtime_stress
```

Use `mountain_fly_review.tscn` for owner free-fly only after the gated
progression step is green.
