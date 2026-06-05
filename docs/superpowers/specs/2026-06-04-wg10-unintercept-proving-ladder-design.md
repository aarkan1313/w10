# WG10 Un-Intercept Proving Ladder — Design

Date: 2026-06-04
Status: design approved by owner (sections 1–3); spec pending owner review.
Branch context: `slice4-gpu-page-integration`.

## Purpose

The owner's ask: "we've kinda brute forced this project — audit to spec and
review, then make a list of all active features we should have based on where we
are at, then make a scene and test them sequentially, adding one feature at a
time, proving and hardening as we go."

This spec is the output of that ask, in three parts:

1. A blunt honesty audit of where the project actually is (recorded below, with
   the frontier map).
2. The canonical active-feature list, expressed as a sequential proving ladder.
3. The design of a new clean proving scene that adds one feature at a time, each
   with a numeric convergence gate, a perf gate, and an owner fly.

## Part 1 — Honesty Audit (grounded against source, 2026-06-04)

Two independent code-level audits (live runtime path; offline pipeline + test
suites) converged on one crux finding.

**Crux: nothing the owner flies today is procedurally generated.** All three
"accepted" runtime modes in `mountain_fly_review.tscn` (REFERENCE,
MOUNTAIN/network_ref, WORLD preview) stream the same baked
`mountain_world_layer_tiles.json` one-shot offline artifact. The live GPU biome
pipeline (11 biomes, flow/drainage solver, compose) is real and parity-proven,
but it is exercised only by test fixtures and is deliberately intercepted in the
owner fly: the producer checks "is a reference bound? → return the baked payload"
and never reaches GPU compute.

Evidence (precise): `wg-10/rust/src/page_pool/producer.rs:143-146`
(SingleBiome returns the bound mountain reference before reaching
`compute_biome_page_cached` at line 157) and `:109-114` (World returns the bound
world preview reference before reaching `compute_biome_world_page_composed`).
The binding is applied in
`wg-10/worldgen_terrain/harness/mountain_fly_producers.gd:214-216` (mountain
network preset) and `:203-204` (world preset). The `close_debug` mountain preset
(`:217`) returns `""` — no reference bound — so the live GPU path *does* run
there, but at `FEATURE_SPAN_CLOSE_DEBUG_M = 3500` (`:29`) versus the reference's
`90000`, which is why it reads as a different place (architecture audit §2).

### Frontier map

| Layer | Claimed | Actually | Verdict |
| --- | --- | --- | --- |
| M0–M2 hash/grammar/GPU-parity | done | done, gated | real |
| M3 render pipeline (pool/streamer/clipmap) | done | done, hardened, 8+ gates clean | real |
| M4 Facts API base-terrain parity | done | done for old base path only | not wired to live biome modes |
| Phase 5 offline 11 biome synths + compose | accepted | exist, pytest-proven, offline only | real but scaffolding |
| Phase 5 Rust port (recipes_*.rs) | done | ported, parity-exact, bypassed at runtime | parked |
| Phase 5 GPU biome_page pipeline | "runs live" | parity-proven on fixtures, intercepted in owner fly | overstated |
| Drainage / flow | "integrated, ~1.9 ms" | code exists, never runs in accepted modes; perf gate is measurement-only | parked |
| Material facts (RGBA) | runtime feature | real but baked (from payload, not computed) | readability bridge |
| Collision parity for live modes | implied | not wired — `collision_field()` never called by live producers | scaffold |
| Terrain edits | built | Python only, zero runtime integration | parked |
| Phases 6–14 | designed | design-only | not started |

The render/streamer/pool foundation is genuinely solid and proven. The entire
gap is **content**: there is no live path where a procedural page is generated →
streamed → rendered → proven against the accepted bar.

## Part 2 — Active-Feature List (the ladder)

The active features that need proving are not UI overlays (the prior session
already built those). They are the **un-intercept steps**: turning each
parked-but-real capability into a live, rendered, gated path, measured against
the baked REFERENCE as the pre-accepted quality oracle.

Ladder (each rung = one producer config + convergence gate + perf gate + owner
fly; rung N+1 is added only after N is green-and-flown):

- **Rung 0 — Un-intercept plumbing (trivial known height).** A throwaway
  analytic producer writes a known height `h = A·sin(x/λ)·cos(z/λ)`. Gate:
  readback matches the analytic formula to f32 epsilon at sampled texels,
  seam-exact across page boundaries, never-black under motion. Proves the flip
  from baked→live works and the harness measures live pages correctly, isolated
  from content quality.
- **Rung 1 — Live mountain macro, flow OFF.** Run the real
  `compute_biome_page_cached` mountain recipe at the reference's scale/seed/
  source-window (90 km, seed 177, accepted source transform), flow disabled.
  Gate: convergence vs baked REFERENCE macro structure. The highest-value
  un-intercept.
- **Rung 2 — Drainage ON.** Enable the GPU flow solver in the live page. Gate:
  valley/channel cells converge closer to REFERENCE than Rung 1, AND a real
  pass/fail perf gate on flow cost at production page size (replaces the current
  measurement-only check).
- **Rung 3 — Material from the live field.** Stop binding baked material facts;
  derive slope/height/curvature channels from the live height. Gate: live
  material masks converge toward the baked REFERENCE masks.
- **Rung 4 — Collision parity on live pages.** Wire `collision_field()` into the
  live producer. Gate: visible(GPU height) == collision(CPU field) on
  live-generated pages (the M4 contract, never proven on the live path).
- **Rung 5 — Multi-biome compose.** Grammar weights → composed multi-biome page
  (lift the WORLD one-biome diagnostic cap inside this scene only). Gate:
  genuinely multi-biome (>1 active biome, real transitions), seam-exact,
  perf-budgeted; convergence checked where a biome overlaps the mountain
  reference region.

Scope discipline:
- **Rungs 0–2 are the spine.** Landing only those crosses the project from
  "flies baked terrain" to "flies live procedural mountains with drainage, gated
  against the accepted bar" — a real, defensible milestone. 3–5 are completion.
- Each rung is independently flyable and revertible. Nothing here touches the
  accepted `mountain_fly_review.tscn` or the baked payload.

## Part 3 — Proving Scene Design

### Architecture & core principle

New scene `wg-10/worldgen_terrain/harness/wg10_unintercept_ladder.tscn` +
`.gd`, assembled from the already-proven components (`Wg10TerrainView`,
`Wg10ClipmapRings`, `Wg10PagePool`, `Wg10Streamer`, fly camera, profiler,
overlay). Per the architecture audit's own recommendation, this is a clean scene
that does not inherit the baked-bridge framing of
`wg10_progression_review.tscn`; that scene is noted as superseded for forward
work but left in place.

Governing rule: **the accepted baked REFERENCE is never mutated.** It is loaded
read-only as the convergence target. Each ladder step is a distinct producer
configuration. A step is promoted only when (a) its numeric convergence gate vs
REFERENCE passes, (b) its perf gate passes, and (c) the owner has flown and
accepted it. No step's unproven output becomes the next step's foundation.

The crux differentiator from the prior session: **gates measure live-vs-baked
convergence, not just "renders non-degenerate."** That is the discipline the
audit found missing.

### Gate strategy

Every rung carries the same gate shape, run serially (the handoff's Godot DLL
import/copy race rule: never run multiple Godot suites in parallel):

1. **Convergence gate** — live page vs baked REFERENCE over the same world
   region, reported as `mean_abs / p95_abs / peak_abs` height delta in metres.
   This is the bar that was missing.
2. **Perf gate** — real GPU time via
   `RenderingServer.viewport_get_measured_render_time_gpu` (not wall-time;
   vsync-immune), page-acquire CPU p99/max under the `16.7 ms` one-frame budget.
3. **Non-vacuous guard** — baked into each gate so a green number cannot be a
   lie: assert the live path actually ran (pages streamed, non-black, camera
   moved, flow actually fired when `flow_on`). Convergence assertions inherit the
   same "did real work" discipline.
4. **Owner fly** — the windowed scene, A/B against REFERENCE, owner accepts or
   rejects.

### Threshold policy (direction + no-regression)

Decided with the owner. No arbitrary absolute "must match to X metres" target —
that becomes parity-theater (cf. the owner's 576-residual stance: matching an
arbitrary tie-break was rejected as parity-theater). Instead:

- **Rung 1** must match-or-beat the live-vs-baked gap the offline port already
  proved (the `mountain_world_layer.py` contract test currently measures
  `mean_abs≈1.21, p95≈2.28, peak≈3.20` over a 1700 m relief field). Rung 1 is a
  "no regression from what Python/Rust already proved" gate.
- **Rungs 2–3** must each *reduce* the delta versus the prior rung — drainage and
  material must measurably earn their place, or they are not promoted.
- The owner's eye is the final arbiter on "close enough"; the numbers gate
  *direction*, not an absolute target.

### Why the live mountain path already mostly exists

`mountain_fly_producers.gd:_configure_mountain` already runs the real GPU recipe
when `close_debug` is selected (no reference bound). So "live procedural
mountain" is not new rendering code to build — it already runs. Rung 1's work is
wiring + gating: run that live recipe at the REFERENCE's scale/source-window/seed
(not the 3.5 km debug scale) and measure convergence. The offline contract test
already computes the convergence metric; the gate lifts that target into the
live windowed path.

### Component reuse vs new code

Reused as-is (proven): `Wg10TerrainView`, `Wg10ClipmapRings`, `Wg10PagePool`
(all four producer paths already exist), `Wg10Streamer`, fly camera, profiler,
diagnostics overlay, the real-GPU-time measurement helper.

New code:
- `wg10_unintercept_ladder.tscn` / `.gd` — the scene + step driver (assembly +
  step selection + HUD), kept thin per DESIGN §6.4 (review scenes assemble
  components, they do not contain diagnostic/report logic).
- A trivial analytic producer path for Rung 0 (smallest possible: a known
  closed-form height into a page texture; may be a tiny GLSL or a constant-fill
  variant — chosen at implementation time for least risk).
- One convergence-gate helper (`.gd`) reused across rungs: given a live producer
  config + the baked REFERENCE, sample the same world region from both and report
  `mean_abs/p95_abs/peak_abs` plus the non-vacuous guards.
- Per-rung gate checks (`.gd`) in `wg-10/worldgen_terrain/tests/`, registered as
  serial suites in `tools/gate.py` (e.g. `ladder_rung0` … `ladder_rung5`).
- For Rung 4: wire `collision_field()` into the live dispatch path (the producer
  module) — the one piece of runtime Rust the ladder adds beyond harness/gates.

### Out of scope

- Final terrain textures / art (explicitly deferred by the handoff; the bar is
  geometry, streaming stability, convergence, contract evidence).
- Changing the accepted `mountain_fly_review.tscn` or the baked payload.
- Re-auditing M0–M4 foundations (settled and gated).
- Color/relief tuning to make modes "look different" (the audit's anti-pattern).

### Definition of done (per rung)

A rung is done when: its convergence gate passes under the direction +
no-regression policy, its perf gate passes under the `16.7 ms` budget with real
GPU time, its non-vacuous guards confirm the live path ran, the owner has flown
the windowed scene and accepted it, and STATUS.md records which rung is live and
its measured convergence numbers. Only then is the next rung added.

## Open questions / risks

- **Rung 1 scale alignment.** The live recipe must sample the accepted 270 km
  source window at the 90 km feature span with the accepted source transform
  (`source_scale=3.515625`, center `207000,176000`). If the live seam-safe recipe
  cannot express the baked field's full-field conditioning + pass-network
  carving, Rung 1 will plateau above the offline gap — that is itself a useful,
  honest result (it tells us conditioning/pass-network must become a live fact,
  which is the roadmap's next real fork). The gate reports the plateau; it does
  not hide it.
- **Rung 5 perf.** Synchronous full multi-biome compose previously caused ~1.9 s
  page-build hitches. The rung's perf gate must catch this; if compose can't fit
  the frame budget synchronously, the honest outcome is "compose needs
  async/cache," recorded, not worked around by feel.
