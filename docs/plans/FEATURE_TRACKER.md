# WG10 Feature Tracker — living checklist

> **Purpose:** one place to track what we SHOULD have, what we DO have, and the gate each
> feature still owes. Updated in place (it is a checklist, not a 4th source of truth — the
> three living docs DESIGN / ROADMAP / STATUS remain authoritative; this just rolls their
> per-feature status into one scannable table). Last reconciled against code + an independent
> spec-to-impl audit: **2026-06-05** (cargo lib 268/268; carved look on screen).

## Definition of done (DESIGN §7.3)

A feature is **DONE** only when all three hold:
- **Perf gate** — a measured gate proves it fits budget (or is off-frame by design).
- **Correctness/visual gate** — a numeric parity/seam gate OR a rendered review surface.
- **Owner confirmation** — for anything with a LOOK, the owner has flown/eyeballed it.

Legend: ✅ done · 🟡 in progress / partial · ⏳ gated (waiting on an earlier gate) · ⬜ not started · 🧪 numeric-green-pending-owner-eye

---

## Engine machinery (M0–M4) — ✅ COMPLETE

| Feature | Perf | Correct | Owner | Status | Evidence |
|---|---|---|---|---|---|
| M0 Project skeleton, 3-doc rule, gate harness | — | ✅ | ✅ | ✅ | Godot 4.6.2 + Rust GDExtension; `tools/gate.py` |
| M1 Worldgen core (CPU) + parity foundation | ✅ | ✅ | ✅ | ✅ | hash→noise→grammar→kernel; parity fixtures; cargo green |
| M2 GPU formula + CPU/GPU parity | ✅ | ✅ | ✅ | ✅ | GLSL compute; Tier-1 exact / Tier-2 f32-eps; RTX 5090 |
| M3 Render pipeline at speed (clipmap/rings/geomorph/stream) | ✅ p99 1.88 ms | ✅ continuity gate | ✅ flown | ✅ | `m3` suite; never-black; 3×3 tiling |
| M4 Facts API + collision + edit seam | ✅ off-frame bake | ✅ collision parity 0.0009 m | ✅ | ✅ | `facts_*` gates; `get_collision_field`/`apply_edit` |

---

## Phase 5 — terrain content layer (CURRENT) — 🟡 late Slice 4

| Feature | Perf | Correct | Owner | Status | Evidence / note |
|---|---|---|---|---|---|
| Slice 1 — offline generator prototype | ✅ | ✅ render | ✅ "good direction" | ✅ | `worldgen_proto.py` |
| Slice 2 — biome distillation tooling | — | 🟡 | ⬜ look not accepted | 🟡 | superseded as param source by hand-authored synths; kept as refine tool |
| Slice 2A — geography-engine / keeper formulas | ✅ | ✅ seam-exact | 🟡 seams ok, gameplay TBD | 🟡 | keeper_v2 (A's regimes on B's seam-safe substrate); Tier-3 traversability is the open quality bar |
| Slice 3 — Rust generator core (all 11 biomes) | ✅ | ✅ CPU 1e-9 + GPU 1e-6 + compose 1e-4 | ⏳ | ✅* | *CPU = cargo (12 tests); GPU/compose = windowed `biome_page` on RTX 5090 |
| Carve ported to Rust (routing + ramp + EDT) | ✅ ~19 ms | ✅ routes bit-exact; ramp tolerance-gated (EDT ties) | — | ✅ | `pass_network/`; routes bit-exact, ramp bit-exact on the assembly fixture's routes (NOT universal) |
| condition_world ported (smooth percentile field) | ✅ ~2 ms | ✅ interior bit-exact; field 0-ULP seam-exact | — | ✅ | `condition_world.rs` + `region_bake/percentile_provider.rs` |
| bake_region assembly (macro→carve→condition) | ✅ | ✅ end-to-end Python parity (height p99 0.09 m) | — | ✅ | `bake_region_matches_python_seamsafe_pipeline` |
| **Slice 4 — region-fact producer (carved look ON SCREEN)** | ✅ off-frame async | ✅ seam-exact internal 0-ULP; never-black | 🧪 **owner A/B pending** | 🧪 | `ProducerKind::RegionFact`; windowed `region_macro`/`bake_worker`/`region_rung1` green |
| └ async super-region bake worker (own RD) | ✅ | ✅ 2-region round-trip | — | ✅ | `region_bake/worker.rs`; Drop-safe |
| └ smooth percentile field (swappable provider) | ✅ | ✅ 0-ULP incl. outer borders | — | ✅ | `outer_seam_tests.rs` |
| └ super-region carve-then-slice | ✅ | ✅ 0-ULP internal by construction | — | ✅ | `super_region_tests.rs` |
| └ outer-border conditioned seam (~5.9 m) | — | 🟡 measured, k-knob tradeoff | — | 🟡 | raise k, or core-local-anchored carve (deferred behind scale contract) |
| Unified review scene (fly+profile every feature) | — | ✅ smoke-verified | 🟡 the A/B venue | ✅ | `feature_review.tscn`, 8 steps |
| Slice 5 — live scale tune + owner fly | ⏳ | ⏳ | ⏳ | ⏳ | gated on the Slice-4 owner visual A/B |

**THE ONE OPEN BLOCKER:** owner visual A/B of the carved baked look — fly **Step 4** of
`feature_review.tscn`. Numeric gates are all green; this is the eye-check that finalizes Slice 4.

---

## Phases 6–9 — planned, gated on Phase 5 acceptance — ⬜ / ⏳

| Phase | Feature | Status | Gate it waits on |
|---|---|---|---|
| 6 | Materials & surfacing (AAA read; replace debug height/slope palette) | ⬜ NOT STARTED | Phase 5 live-height owner acceptance |
| 7A | Local erosion / drainage-shaped detail filters (up-close gully/ridge) | ⬜ NOT STARTED | Phase 6 + analytic-gradient feasibility (Runevision candidate) |
| 7B | Connected drainage milestone | 🟡 PARTIAL — realized by the region-fact off-frame bake; remaining = tuning + acceptance | Phase 5 acceptance + scale contract |
| 8 | Framework modes (bounded / spherical-planet / handmade-area blend) | ⬜ NOT STARTED | Phase 5 acceptance |
| 9 | Visible editable terrain (M4 edit seam's render half) | 🟡 PARTIAL — collision + edit-review surface done; visible-in-rendered-height half deferred | render/worldgen settled |

---

## Cross-cutting deferred items (from LOOSE_ENDS_LEDGER)

- **Player-to-world SCALE CONTRACT** — gates the core-local-anchored carve, pass density, and
  Slice-5 tuning. The single highest-leverage unblock for the "guaranteed traversability" quality bar.
- **Core-local-anchored carve** — the infinite-per-region seam-exact carve (vs today's
  super-region slice). Deferred behind the scale contract (defeated ~10 prior iterations).
- **WORLD multi-biome compose on the live stream** — currently diagnostic (~1.9 s page hitches);
  needs async/cached compose before it's a production producer.
- **Worker GPU-context reuse** — rebuilds RD+context per super-bake (correct + isolated); cache
  if super-bakes become frequent on the live path.
- **Material/AAA textures** — explicitly out of the current acceptance bar (Phase 6).

---

## How to verify the current state (commands)

```powershell
# cargo lib (isolated, no editor): expect 268/268
$env:CARGO_TARGET_DIR='D:/tmp/wg10_check_target'; Push-Location 'D:\workflows\worldgen10\wg-10\rust'; cargo test -p wg10_terrain --lib; Pop-Location
# windowed gates (editor CLOSED, RTX 5090):
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'
python tools\gate.py --suite region_macro   # GPU super-macro readback
python tools\gate.py --suite bake_worker     # async worker round-trip
python tools\gate.py --suite region_rung1    # ON-SCREEN carved page + internal seam
python tools\gate.py --suite gpu             # regression
python tools\gate.py --suite biome_page      # all-11 GPU parity regression
# the review scene (the owner A/B venue):
#   run feature_review.tscn windowed; fly Step 4.
```
