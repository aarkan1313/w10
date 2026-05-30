# WorldGen10 M3 — Slice 6 (close-out): Fly Harness + p99 Acceptance Gate Design

**Date:** 2026-05-29
**Status:** Approved (brainstorming) — pending writing-plans
**Layer:** Render pipeline (M3) — slice 6: the fly-test harness (camera/movement, profiling, diagnostics overlay, review scene) + the automated renderer p99<6ms + no-black acceptance gate. Closes M3 (pending the owner's manual fly).
**Builds on:** slice 5b (3×3 ring tiling + `Wg10TerrainView` live loop), slice 3 (streamer), slice 2 (pool).
**Followed by:** M4 (Facts API) — after the owner's manual fly signs off M3.

---

## 0. Framing

The render pipeline is complete and proven under *scripted* motion: pages → bounded
single-owner pool → velocity-aware scheduler → 3×3 clipmap rings → live view; it surrounds
the camera, stays seamless, is never-black, and triggers no synchronous compute on the render
path (the WG9 disease, held structurally). What remains to **close M3** is the harness around
it and the perf-acceptance verdict.

DESIGN §7.3 defines M3 done as **perf gate + visual gate + the owner's manual fly**, and is
explicit that automated vision is a *regression catcher, not the sole authority* — WG9's core
failure was green counters over a broken render. So this slice builds the harness + an
automated p99/no-black gate (the regression catcher), and the **M3 milestone stays open** for
the owner's manual fly (the final authority).

The harness components follow §6.4: each self-contained, narrow-interface, config-driven,
composable — drop-in to any Godot 4.6 project, like the terrain addon. The review scene only
*assembles* them.

---

## 1. Scope

**In scope (slice 6):**
- **`Wg10FlyCamera`** — free-fly camera+movement rig (WASD + Shift-speed + mouse-look +
  Space/C vertical; bindings/speeds/sensitivity in config). Feeds `Wg10TerrainView.update`
  from input each frame. Knows nothing about terrain.
- **`Wg10Profiler`** — total-frame-delta ring buffer → `p99()`/`mean()`/`max()`, plus a
  GPU-time cross-check stat. Attachable to any scene; no terrain/renderer internals.
- **`Wg10DiagnosticsOverlay`** — live fps / p99 / pool stats HUD, read through narrow
  interfaces (profiler + `Wg10TerrainView.stats()`); knows nothing about how they're produced.
- **Review scene** (`m3_review.tscn` + thin script) — assembles `{Wg10TerrainView (+ pool +
  streamer + rings) + Wg10FlyCamera + Wg10Profiler + Wg10DiagnosticsOverlay}` and wires them.
  Pure assembly (no component logic).
- **`m3_accept_check.gd`** — the automated acceptance gate: drives the SAME `Wg10TerrainView.
  update` loop over a scripted ~1000 m/s flight path (straight runs + turns across many page
  boundaries), captures per-frame total frame time, asserts **p99 < 6 ms** AND **no-black**
  (nonblack≈1 sampled per frame) over the run.

**Out of scope (YAGNI / deferred):** a standalone UI-chrome component (the overlay IS the HUD;
no second consumer yet); a ground-follow camera rig (free-fly is what M3 needs); tile-edge-line
visual polish (a recorded backlog item, not acceptance-blocking).

**The acceptance split (§7.3):** the automated gate green = the SLICE is done & committed. The
**M3 milestone stays `[ ]` OPEN** with one remaining box — the **owner's manual fly sign-off**.
The slice hands the owner the review scene + launch instructions + what to look for; the owner
flies it at full speed and confirms no stalls / no black. Gate green is necessary, not
sufficient; the manual fly is the final authority.

---

## 2. Harness components (§6.4 rules: self-contained, narrow interface, config-driven, composable)

All four live under a harness addon folder (`wg-10/worldgen_terrain/harness/`) so they are a
drop-in unit, separate from the terrain addon. GDScript (these are scene-side glue, not the
deterministic core — no Rust).

### 2.1 `Wg10FlyCamera` (Node3D with a Camera3D child, or a Camera3D script)
Free-fly rig. Each `_process(delta)`: read input (WASD → horizontal, Space/C → vertical,
mouse motion → look while captured, Shift → speed multiplier), integrate position and compute
velocity (Δposition/Δt). **Config** (exported resource or dictionary): move speed, sprint
multiplier, mouse sensitivity, vertical speed, key/mouse bindings — no magic numbers.
**Narrow interface:** exposes the current `position: Vector3` and `velocity: Vector3` (read
via a getter or a `moved(position, velocity)` signal each frame). The review scene reads those
and calls `Wg10TerrainView.update(pos.x, pos.z, vel.x, vel.z)`. The camera contains zero
terrain knowledge — it's a generic fly-cam.

### 2.2 `Wg10Profiler` (Node)
Frame-time capture. `_process(delta)`: push `delta` (seconds) into a fixed-size ring buffer
(e.g. 512 frames). Methods: `p99() -> float` (ms), `mean() -> float`, `max() -> float`,
`fps() -> float`, `gpu_ms() -> float` (read Godot's GPU/render time via
`Performance.get_monitor(...)` or `RenderingServer.get_rendering_info(...)` — the available
GPU-time monitor in 4.6). Percentiles computed over the captured window. **Config:** ring size.
**Narrow interface:** the methods above; attach to any scene, knows nothing about terrain. The
total-frame-delta is the honest 6ms-budget number (CPU+GPU+present); `gpu_ms` is the diagnostic
split that tells us WHERE the time goes if it's tight.

### 2.3 `Wg10DiagnosticsOverlay` (CanvasLayer + Label/RichTextLabel)
Reads stats through interfaces and renders a corner HUD: fps, frame p99 (ms), gpu (ms), and
pool stats (resident / created / full_events) from `Wg10TerrainView.stats()`. **Config:**
corner/anchor, font size, update interval. It holds references to a profiler and a view (set
by the review scene) and polls their narrow interfaces — it never reaches into terrain or
renderer internals. Decoupled from what it displays.

### 2.4 Review scene (`m3_review.tscn` + `m3_review.gd`)
The thin assembly point (§7.4). On `_ready`: instantiate + configure pool/streamer/rings,
configure `Wg10TerrainView`, add the `Wg10FlyCamera`, attach a `Wg10Profiler`, add a
`Wg10DiagnosticsOverlay` and point it at the profiler + view. Each frame: read the camera's
pos/vel → `view.update(...)`. Composable: removing or swapping any one component does not break
the rest (the script wires only narrow interfaces). This is the scene the OWNER launches for
the manual fly.

---

## 3. The automated acceptance gate: `m3_accept_check.gd` (`m3` suite, WINDOWED)

Needs the global RenderingDevice (windowed); SKIP code 2 headless. It does NOT use the
interactive camera — it drives the SAME `Wg10TerrainView.update` loop the fly-cam drives, but
from a **scripted flight path** (so the perf number is representative and reproducible):

- Assemble pool + streamer + rings + `Wg10TerrainView` (3 levels) in a SubViewport with a
  perspective camera positioned behind/above the moving point (a flight POV, not top-down — the
  budget is about what a player sees). Attach a `Wg10Profiler`.
- **Scripted ~1000 m/s flight path:** a sequence of straight runs + turns over many frames
  (e.g. several hundred frames at ~1000 m/s, crossing many page boundaries, including direction
  changes so the streamer's lead is exercised in multiple headings). Each frame: advance the
  scripted pos/vel, `view.update(...)`, position the POV camera at the point, render, and let
  the profiler capture the frame delta.
- **Disable vsync** for the run (`DisplayServer.window_set_vsync_mode(VSYNC_DISABLED)` /
  project setting) so frame time reflects real render cost, not the monitor refresh cap.
- After a warm-up (let the streamer fill + frame times settle), capture the measured window.
- **Assertions:**
  1. **p99 < 6 ms** — `profiler.p99()` over the measured window. THE M3 number.
  2. **No-black** — sample the rendered frame each measured frame (or every Nth): nonblack
     fraction ≈ 1.0 over the terrain region (never a black hole/gap at speed).
  3. **Never-stall** — `max()` frame time below a sane ceiling (e.g. < 33 ms / no single frame
     spike that would be a visible hitch) — catches a stall even if p99 passes.
  4. Bounded pool (resident ≤ capacity) + non-empty residency throughout (sanity).
- Print the numbers (`p99=.. mean=.. max=.. gpu=..`) so they're visible in the gate log, and
  save a PNG or two from the run. Wire into the `m3` suite.

**Honesty note (§7.3):** this gate is the **regression catcher**, not the acceptance authority.
The DESIGN budget is `p99 < 6 ms at ~1000 m/s`; if the windowed measurement environment makes
the absolute number unrepresentative (driver/windowing overhead), the gate still guards against
*regressions* (relative) and the owner's manual fly is the real-feel authority. If p99 fails,
that's a real finding — investigate (the 3×3 overlap overdraw is the prime suspect; levers:
toroidal rebind, hollow-coarse, fewer/cheaper tiles) — do NOT raise the threshold to pass.

---

## 4. Files

**New (harness addon — `wg-10/worldgen_terrain/harness/`):**
- `fly_camera.gd` — `Wg10FlyCamera`.
- `profiler.gd` — `Wg10Profiler`.
- `diagnostics_overlay.gd` — `Wg10DiagnosticsOverlay`.
- `m3_review.tscn` + `m3_review.gd` — the review scene + assembly script.

**New (gate):**
- `wg-10/worldgen_terrain/tests/m3_accept_check.gd` — the automated p99/no-black gate.

**Modify:**
- `tools/gate.py` — add `m3_accept_check.gd` to the `m3` suite.
- (If `Wg10TerrainView` needs a tiny accessor for the overlay, e.g. `stats()` already exists —
  reuse it; add nothing unless a real gap appears.)

**Soft cap:** GDScript files small + focused; each component one responsibility.

---

## 5. Definition of done

- The four harness components exist as self-contained, config-driven, composable units (§6.4),
  and the review scene assembles them (no component logic in the scene).
- `m3_accept_check` passes WINDOWED: **p99 < 6 ms** + no-black + never-stall over the scripted
  ~1000 m/s flight path; prints the numbers; saves a PNG. Wired into the `m3` suite (→ 5 checks).
- `fast`/`gpu` unchanged; cargo green.
- STATUS + ROADMAP updated: the harness + automated acceptance gate DONE; **M3 milestone stays
  OPEN** with the single remaining box = the owner's manual fly. The owner is handed: how to
  launch `m3_review.tscn`, the controls, and what to look for (no stalls, no black, smooth at
  speed). The automated p99 number is recorded.
- Each task committed separately.

**M3 closes** when the owner flies `m3_review.tscn` at full speed and confirms no stalls / no
black (§7.3 — gate green is necessary, not sufficient; the manual fly is the final authority).

---

## 6. Risks & mitigations

- **Windowed frame-time isn't representative (vsync / driver / window overhead).** Disable
  vsync for the run; report mean/p99/max/gpu split so the number is interpretable. Treat the
  gate primarily as a regression catcher (relative), with the owner's manual fly as the
  real-feel authority. Record the measured number honestly in STATUS, caveated.
- **p99 fails the 6 ms budget.** A real finding, not a threshold to relax. Prime suspect: the
  3×3 overlap overdraw (recorded as a p99 input in 5b). Levers, in order: toroidal tile rebind,
  hollow the coarse center under the finer 3×3, reduce grid_res / tile count, cheaper shader.
  If it fails, surface it to the owner with the gpu/cpu split and the candidate fixes.
- **Harness component coupling creep.** §6.4 rules are the guard: each component drop-in-able in
  isolation, narrow interface, no cross-component dependency. The review scene is the only place
  they meet, and only via their public surfaces. A component that needs another component's
  internals is a design smell to fix, not route around.
- **Manual fly finds a problem the automated gate missed.** Expected and fine — that's exactly
  why the manual fly is the authority. Any finding is a real bug → fix, then re-fly.
- **Mouse-look / input capture in a gate context.** The gate uses the SCRIPTED path (no input),
  so input capture is only the interactive scene's concern; the gate never depends on input.
