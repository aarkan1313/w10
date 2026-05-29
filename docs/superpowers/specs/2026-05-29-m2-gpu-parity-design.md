# WorldGen10 — M2 GPU Formula + Parity Design

**Date:** 2026-05-29
**Status:** Approved (brainstorming) — pending writing-plans
**Layer:** GPU compute implementation of the full formula (hash→grammar→height) + CPU/GPU parity gate
**Builds on:** M1 grammar (`grammar.rs`) + M1 height (`height.rs`, `pack.rs`, `npy.rs`)
**Followed by:** M3 render pipeline (consumes GPU height pages, no readback)

---

## 0. Framing

DESIGN §2.4 (non-negotiable): "GPU is the default for ALL render-consumed
terrain data … Both sides run the **same deterministic formula** and are
**parity-gated**." M2 makes that real: the GPU computes `height(x,z,seed,pack)`
end-to-end, and a parity gate proves it equals the CPU. This is what M3's render
pipeline streams as GPU height pages.

The pipeline was built "GPU-shaped" (fixed arity, no heap in the hot path) so this
port is tractable — but it is **not** a mechanical transliteration, because the CPU
grammar hashes joined strings and walks maps, neither of which exists in GLSL.
The enabling refactor (§2) fixes that.

---

## 1. Scope

**In scope:**
- **Integer-hash refactor (CPU):** replace the grammar's string-join hashing with
  an integer-domain hash that runs bit-identically on CPU and GPU (§2). Re-lock
  grammar tests. The WG9-bit-exact bedrock (`hash_grid`/`value_noise`/`fbm`) is
  **untouched**.
- **GPU compute shader:** a hand-ported GLSL compute shader running the full
  formula — integer hash → region/province/palette/family rolls → weight blend →
  tiled kernel sample + moderation → `height(x,z)` (§3).
- **`gpu_compute.rs`:** godot-facing RenderingDevice plumbing — build the local
  compute pipeline, upload pack tables + kernels + query coords as buffers,
  dispatch, read back height + family-signature buffers (§3).
- **CPU/GPU parity gate:** a new `gpu` gate suite — Tier 1 exact family selection,
  Tier 2 epsilon height (§4).
- DESIGN/ROADMAP/STATUS updates.

**Out of scope (deferred — do NOT build):**
- The render pipeline (M3): page pool, page scheduler, clipmap rings, the manual
  fly-test. GPU output here is validated by a one-off readback **in the gate only**;
  production no-readback streaming is M3.
- Real DEM pack wiring; anti-repetition / kernel variety tuning.
- A GPU kernel **atlas redesign** for varied real-DEM kernel sizes — the synthetic
  kernels are tiny (4×4) and pack trivially; varied-size packing is a named risk
  to revisit when the real DEM pack lands (§3, §6).

---

## 2. Interface constraints — NON-NEGOTIABLE

1. **Same formula both sides.** CPU and GPU run the identical integer-hash
   formula. The parity gate is the contract that enforces this (§4); any
   divergence fails.
2. **No readback in production.** Readback exists ONLY in the parity gate. The
   shader writes to a buffer; the gate reads it once to compare. M3's renderer
   never reads back (DESIGN §2.4 — readback is what made WG9 cost 128 ms/chunk).
3. **Bedrock untouched.** `hash_grid`/`value_noise`/`fbm` and their WG9
   `hash_reference.json` parity stay bit-exact. Only the **grammar rolls** move to
   the integer hash. The string-based `stable_hash` remains for the bedrock and
   the `Wg10Hash` binding.
4. **Grammar structure unchanged.** Only the *input to the roll hash* changes
   (string prefix → integer salt). The roll arithmetic (`% len`, pct thresholds,
   bias rotation) and the module's shape are unchanged.
5. **Engine-agnostic core stays engine-agnostic.** The integer hash lives in
   `hash.rs` (no godot). `gpu_compute.rs` is the new godot-facing file (it must
   touch RenderingDevice). `grammar.rs`/`height.rs`/`pack.rs`/`npy.rs` gain no
   godot imports.

### Honest consequence (accepted)

The integer-hash refactor **changes the grammar's roll values** — a new, equally
valid seed-space / world. W10 grammar was never a WG9 parity contract; the
property gates (sum=1, determinism, variety, seam-continuity) and the
roll-independent flat anchor (`height==500`) are unaffected. Any test asserting a
*specific* palette index is re-locked to the new value.

---

## 3. GPU compute architecture

### 3.1 The shader (`wg-10/worldgen_terrain/shaders/height_field.glsl`)

A faithful hand-port with small functions mirroring the Rust fn names:
`mix_u32`, `stable_hash_ints`, `region_of`, `province_of`, `palette_for_region`
(the 5 roll sites), `families_for_region`, `family_weights` corner blend,
`sample_kernel` (tiled bilinear, wrap), `moderation`, `height`. Header comment:
**"EDIT BOTH SIDES — the parity gate (`gpu_parity_check.gd`) is the contract."**

The shader does integer math + f32 float math only — no strings, no maps.

### 3.2 Inputs uploaded as buffers (CPU resolves names→indices at upload)

- **grammar_constants** → push-constant / uniform struct: `region_size_m`,
  `province_size_regions`, `palette_primary_pct`, `palette_compatible_pct`,
  `moderation_min`, `moderation_strength`, `seed`, plus table sizes.
- **palette table** → int buffer `palettes_flat[num_palettes * 3]` of family
  indices (mirrors each `Palette.families` resolved to `family_ids` indices).
- **compatibility table** → flattened int buffer + per-palette (offset, count)
  so the shader reproduces `palette_compatible`'s neighbour pick by index.
- **kernel atlas** → a float buffer packing every family's kernel data
  contiguously, plus a per-family record buffer `(data_offset, rows, cols,
  relief_m, footprint_m)`. The shader samples kernel `f` by reading its record
  then bilinear-fetching from the atlas at `data_offset + row*cols + col`.
- **query coords** → input float buffer of `(x, z)` pairs (the parity grid).
- **outputs** → float buffer `height[i]`; int buffer `family_sig[i]` for the
  Tier-1 exact comparison. **Encoding (fixed):** the set of family ids present in
  the blend at coord `i` (the `FamilyWeights` entries, which the corner blend
  bounds to ≤12), taken as their sorted ascending family indices and folded into a
  single integer signature via `stable_hash_ints(SALT_SIG, &sorted_ids)`. CPU and
  GPU compute this identically from the same entry set, so a match proves the
  grammar selected the same families (the dangerous divergence); it deliberately
  ignores the float weights (Tier 2 covers magnitude).

### 3.3 `gpu_compute.rs` (godot-facing plumbing, no formula)

Exposes (via a `Wg10GpuCompute` GodotClass): load a pack (reuse
`pack::load_pack_dir`), build the RenderingDevice **local** compute pipeline from
the compiled `.glsl`, upload the tables+atlas+coords, dispatch, read back
`height[]` and `family_sig[]`. Pure plumbing; all math is in the shader. This is
the only new file that imports `godot` / touches `RenderingDevice`.

---

## 4. Parity harness + gate

### 4.1 CPU side

A CPU `family_signature(x, z, seed, &Pack) -> u32` helper in `height.rs` (or a
small `parity.rs`): take `grammar::family_weights(...).entries()`, collect the
family ids, sort ascending, fold via `stable_hash_ints(SALT_SIG, &sorted_ids)` —
the identical encoding §3.2 specifies for the shader's `family_sig`. Plus the
existing `height::height` for the Tier-2 value.

### 4.2 The gate (`wg-10/worldgen_terrain/tests/gpu_parity_check.gd`, new `gpu` suite)

1. Guard: GPU/RenderingDevice available — else print a clear `status=skip
   reason=no-gpu` and a NON-zero-distinct signal so a skip is never mistaken for a
   pass (the runner treats skip as not-pass; see §4.3).
2. Load the synthetic height pack through both `Wg10Height`/CPU-signature and
   `Wg10GpuCompute`.
3. Over a coordinate grid (e.g. a few hundred coords spanning negatives, seams,
   far-field):
   - **Tier 1 (exact):** `gpu_family_sig[i] == cpu_family_sig[i]` for EVERY coord.
     Zero tolerance — the integer hash is identical on both sides. Any mismatch =
     fail (this is the "wrong family = different terrain" guard).
   - **Tier 2 (epsilon):** `|cpu_height[i] - gpu_height[i]| <= max(ABS_EPS,
     REL_EPS * relief_at_i)` with `ABS_EPS`/`REL_EPS` documented + justified by the
     f32 mantissa (f64 CPU vs f32 GPU; ~7 significant digits → REL_EPS ~1e-5,
     ABS_EPS ~1e-3 m). Fail if exceeded; report `maxΔ`.
4. Print `[wg10-gpu-parity] status=pass coords=N families_exact=true maxΔ=…`,
   return 0; on any tier failure print `status=fail` + the first divergent coord,
   return 1.

### 4.3 Run mode

GPU compute needs a real device, so the `gpu` suite runs via a **non-headless**
Godot invocation (a new `--suite gpu` branch in `gate.py`, or a sibling runner).
The headless `fast` suite is unchanged and still the default. A `skip` (no GPU)
is reported distinctly and is **not** counted as a pass.

### 4.4 Rust unit tests

`hash.rs`: `stable_hash_ints` determinism + that distinct salts decorrelate (two
salts on the same args give different rolls); grammar re-lock tests updated to the
new integer-hash outputs (properties unchanged; specific-index expectations
refreshed).

---

## 5. Module boundaries (each one job, under ~600 lines)

- `hash.rs` — +`stable_hash_ints` (+ salt consts); bedrock untouched.
- `grammar.rs` — roll-input swap only (string prefix → integer salt); structure unchanged.
- `parity.rs` (or a small section of `height.rs`) — CPU `family_signature`.
- `height_field.glsl` — the hand-ported formula (the GPU side of the contract).
- `gpu_compute.rs` — RenderingDevice plumbing (the only new godot file).
- `bind_worldgen.rs` — thin exposure of the GPU dispatch for the gate.
- `tools/gate.py` — add the `gpu` suite + non-headless run branch.

---

## 6. DESIGN.md updates this plan must make

- §2.4 / architecture: record that the **same formula now runs on CPU and GPU**
  via an integer-domain hash; readback exists only in the parity gate (production
  is no-readback, deferred to M3).
- Record the **integer-hash refactor** and its accepted consequence (new grammar
  seed-space; bedrock + WG9 parity untouched).
- §9 open items: add the **GPU kernel-atlas-for-varied-sizes** named risk (synthetic
  4×4 kernels pack trivially; real-DEM varied sizes may need an atlas redesign —
  revisit with the real DEM pack), and the **epsilon justification** (f32 vs f64).
- ROADMAP M2: mark "GPU compute implementation (no readback in production)" and
  "CPU/GPU parity gate (bit-close; documented epsilon)" done; note the gate runs
  windowed (needs a real GPU), `fast` stays headless.

---

## 7. Named risks (do not solve now)

- **Kernel atlas for varied sizes** (§6) — synthetic only this plan.
- **f32/FMA reorder divergence** — Tier-2 epsilon absorbs it; if a future
  divergence exceeds epsilon, profile before widening (DESIGN: "documented epsilon
  only if profiled"). Do not loosen epsilon to make a failing gate pass.
- **GPU availability in CI/headless** — the `gpu` suite is windowed + local-device;
  a no-GPU environment skips distinctly (never a silent pass).
