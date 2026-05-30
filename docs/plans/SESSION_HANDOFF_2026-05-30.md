# WorldGen10 — Session Handoff (2026-05-30, late)

**For the next chat taking over.** This is a point-in-time handoff layered on top of the permanent
`docs/plans/HANDOFF.md` (read that first for the project's what/why/how). This file says exactly what
happened THIS session, what is committed, what is verified vs not, and the precise next actions.

**Current-session addendum:** after this handoff was written, the structure audit was folded into ROADMAP,
the B2 unit tests were re-run honestly (**121 passed / 0 failed**, isolated target), the B2
capacity-pressure gate was added to the m3 suite, the Rust DLL was rebuilt after the editor closed, and the
gates are now green: **fast 6/6 · gpu 4/4 · m3 9/9**. The B-bug closeout is no longer blocking Slice 2A.

> Read order for a fresh session: `docs/plans/HANDOFF.md` → this file → `docs/plans/STATUS.md` (top) →
> `docs/plans/LOOSE_ENDS_LEDGER.md` → the two specs the HANDOFF names. Memory index:
> `C:\Users\josep\.claude\projects\d--workflows\memory\MEMORY.md` (esp. `worldgen10-slice2-structure-gap`,
> `worldgen10-b-bugs-status`).

---

## 1. One-paragraph state

The **Slice 2 biome-distillation OFFLINE TOOLING is built and gated** (commits `61b46cf`..`3957f3a`), but
the **look is NOT accepted**: the owner's verdict on the rendered terrain was *"still not terrain — same
noise, doesn't look like the real world."* That is the same deep truth as the earlier spectral refutation:
**plain warped/ridged noise produces roughness, not connected STRUCTURE (ridgelines + branching drainage).**
Slice 2 is therefore **PAUSED**, and the owner is taking the *structure approach* to a research/review chat
(a ready-to-paste research prompt is in §5). In parallel this session closed loose-end code bugs: **B1 (RID
leak) and B3 (perf-gate hole) are source-fixed + committed; B2 (structural never-black) is source-fixed +
unit-tested + committed.** Current addendum: the editor-closed rebuild and gates have since verified this
batch (**fast 6/6 · gpu 4/4 · m3 9/9**). `main` is ~21 commits ahead of `origin/main` (not pushed this
session).

## 2. What is committed this session (all on `main`, NOT pushed)

| Commit | What |
|---|---|
| `6aa64d7`..`61b46cf` | Slice 2 distillation: `biome_distill.py`, `distill_biomes.py`, `attach_biome_params` (pack-writer) |
| `6e0cd9c`,`cf17b97` | **Metric mapping REVISION** — first metrics were dead on real DEMs (see §3); switched to height-normalized shape (valley←incision/relief, ridge←slope, fixed base scale) |
| `3957f3a` | `render_biomes.py` (real-vs-synth hillshades) |
| `11ec698` | STATUS — honest "tooling done, look not accepted, paused" finding |
| `0cb0a79` | **B1** RID leak: Rust `Drop` impl + `free_all` at the 2 leak sites + killed the 2 wrong comments |
| `7e37f29` | **B3** perf gate: terrain-vs-sky nonblack + detail on/off assertion (GDScript, no rebuild) |
| `0b1e2f9` | **B2** structural never-black: pin displayed coarse pages (policy+pool+view) + 6 unit tests |
| `990fc7e`,`1208b50` | ledger corrections (incl. correcting a wrong "B3 done" note) |

**Verified this session:** `cargo test` = **121 passed / 0 failed** (was 115; +6 B2 pinning tests).
`cargo check` clean (only pre-existing dead-code warnings). dem_pack pytest was green before the pause.
**NOT verified this session:** the windowed gates (`--suite gpu`, `--suite m3`) — they need a Rust rebuild
of the DLL, which needs the Godot editor closed (it holds `wg10_terrain.dll`).

## 3. Slice 2 — the full story (so the next chat doesn't re-tread it)

**Goal:** distill 115 real DEMs (12 biome families) → per-biome PARAMS that drive the warped-noise generator
(`worldgen_proto.generate`), so generated terrain reads as that biome. **Architecture works** (per-biome
params genuinely differ); the **generator's structure stage is the gap.**

Findings caught OFFLINE (render-first earned its keep — all cheap, before any runtime):
1. **First structural metrics were DEAD on real 512px DEMs:** structure-tensor `ridge_linearity` ≈ 0.30 and
   argmax `dominant_wavelength_m` ≈ 25 km for EVERY family; WG9 metadata `ridge_density`/`valley_density`
   are a dead-constant 0.100. → Switched to **height-normalized shape**: `valley_depth` ← incision/relief,
   `ridge_strength` ← slope, `relief_m` ← relief, freqs ← a fixed config base scale. A trap-gate test proves
   height alone doesn't buy ridge_strength. Dead metrics kept for diagnostics, no longer drive knobs.
2. **Sandpaper bug:** base wavelength 8 km vs ~190 km DEM tiles (features repeated ~24×) AND the distilled
   octave amplitudes were INVERTED (fine octaves dominated). fBm needs amps that DECAY with frequency + a
   continental base scale. Fixing both produced visible macro structure.
3. **Owner verdict after the fixes: still reads as "same noise, not the real world."** → the root issue is
   that the *generator* (plain warp+ridged+valley noise) can't make connected ridgelines/drainage. This is
   the structure-research question (§5), NOT more param-tuning.

**Kept (works, don't rebuild):** the whole distillation pipeline + the metric fixes. **Under research:** the
generator's structure stage. The hand-tuned-params Slice 1 `worldgen_proto.generate` is unchanged and still
the python prototype.

## 4. The B-bugs (loose ends) — exact status + the ONE remaining verify

All three were FINDINGS FIX-NOW items, precondition for Slice 3 (the first runtime build). Source audit +
fixes this session:

- **B1 (PagePool GPU-RID leak): SOURCE-FIXED + committed `0cb0a79`.** `impl Drop for Wg10PagePool` →
  `free_all_impl()` (RIDs release on drop, structural); `m3_review.gd` keeps `_pool` + `_exit_tree` free_all;
  `m5_detail_check.gd` calls `free_all` at all 3 pool-owning returns + the 2 wrong comments deleted. cargo
  check clean.
- **B3 (perf-gate sky-nonblack hole): SOURCE-FIXED + committed `7e37f29`.** `_terrain_frac` counts a
  lower-frame pixel as terrain only if it differs from the sky color (`SKY`+`SKY_DELTA`+`MIN_TERRAIN_FRAC`);
  `_detail_on_off_delta` asserts detail ON vs OFF changes the frame (`DETAIL_DELTA_MIN`). GDScript-only.
- **B2 (never-black capacity-dependent, not structural): SOURCE-FIXED + UNIT-TESTED + committed `0b1e2f9`.**
  Added a `pinned` slot set in `page_policy` (independent of `protected`); eviction skips pinned slots;
  `release` won't unprotect a pin; whole-capacity-pinned → `Full` (coarser fallback, never recycle a shown
  slot). `page_pool` exposes `clear_display_pins`/`pin_displayed_page`/`is_displayed_pinned`. `terrain_view`
  clears pins each frame, pins every bound page, and on coarsest HOLD-LAST-GOOD re-validates the held page is
  resident-as-itself (else hides) + pins it. 6 new unit tests (cargo 121 green).

**B-bug closeout verification (now done):**
1. **Rust DLL rebuilt after the owner closed Godot:**
   ```powershell
   $env:GODOT_BIN = "C:\tmp\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe"
   powershell -ExecutionPolicy Bypass -File .\tools\build_rust.ps1
   ```
2. **Gate results:**
   ```powershell
   python tools/gate.py --suite fast   # 6/6
   python tools/gate.py --suite gpu    # 4/4
   python tools/gate.py --suite m3     # 9/9
   ```
   The new B2 capacity-pressure check passes non-vacuously (`full_delta=3`, `pressure_held=3`, `resident=9`).
   B3's hardened perf check passes with terrain-vs-sky and detail-on/off active (`GPU p99=0.082ms`,
   `terrain_frac_min=1.000`, `detail_delta=0.53739`).

## 5. The STRUCTURE research (the real blocker) — ready-to-paste prompt

The owner is taking this to a research/review chat. The question: **how to make procedural terrain read as
real-world geography (connected ridgelines + drainage) while staying local / parity-able / infinite.** Survey:
ridged multifractal (Musgrave), domain-warp magnitude, terrain-specific noises, and especially the owner's
**distilled-erosion** idea (offline run real hydraulic erosion → learn a cheap LOCAL operator → apply online
per-page). Plus: what is the RIGHT set of structural measurements to pull from a DEM to parameterize such a
generator (beyond slope/relief/incision)? The full prompt is in the session log; the short version:

> "Distill DEMs → params → local warped-noise generator" gives roughness, not structure (phase = structure,
> which noise lacks — proven twice now by the owner's eye). Research local/parity-safe techniques that DO
> produce connected ridgelines + drainage (ridged multifractal, strong domain warp, analytic/learned erosion
> operators, NMS-style fakes), recommend a concrete direction with the infinite/parity/perf tradeoff, and say
> what to measure from the DEMs to drive it.

When the research lands: brainstorm → spec → plan → slice-by-slice (render-images-first, owner-flown
acceptance), same as always. The distillation tooling is the param-extraction half and is KEPT; the new work
is the generator's structure stage (and possibly a richer metric set to feed it).

## 6. Operating notes that bit this session (so the next chat avoids them)

- **The tool channel hiccupped twice** (reads/edits transiently returned empty, then recovered). If a read
  comes back empty, RE-RUN it before concluding a file is missing/corrupt — don't act on the empty result.
  (One wrong "B3 already done" note came from acting on an empty read; corrected in `1208b50`.)
- **`cargo check` against an isolated target dir** (`env -u CARGO_TARGET_DIR CARGO_TARGET_DIR=D:/tmp/wg10_check_target cargo check`)
  compiles WITHOUT touching the editor-loaded debug DLL — use it to validate Rust edits while the editor is
  open. The real `cargo build` for the editor still needs the editor closed.
- **Don't kill the owner's Godot editor to rebuild** (memory `worldgen10-dont-kill-editor`) — ASK them to
  close it.
- **`git push` works** from the assistant (cached GCM token) but was NOT done this session — `main` is ~21
  ahead of `origin/main`. Push when the owner says, or at a sync point.
- Scratch renders/tools created + cleaned this session: `tune_scale.py` (removed), `biome_params.json`
  (transient, regenerated by `distill_biomes.py`, not committed), `D:\tmp\*.png` probes (removed).

## 7. The immediate next action (pick based on what the owner wants)

- **If continuing from here:** B1/B2/B3 are DONE for the rebuild precondition. Next action is Slice 2A
  structure-basis salvage (offline Python, render-first), then owner image review. Push at the next sync point.
- **If the structure research is back:** brainstorm the new generator-structure approach (§5) → spec → plan →
  execute; the distillation tooling feeds it.
- **Either way:** the distillation LOOK is owner-judged; gates prove invariants only. Keep STATUS honest
  (passed-a-gate ≠ owner-accepted).
