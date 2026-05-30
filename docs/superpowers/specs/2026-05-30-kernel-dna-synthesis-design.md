# Kernel-DNA Synthesis — design spec (kernels INFORM an infinite procedural world)

**Date:** 2026-05-30
**Milestone:** new, FOUNDATIONAL — replaces the tiled-pixel-sampling height core. Supersedes the
kernel-tiling/footprint concern; absorbs the mesh-density question. The "shaded terrain at scale"
relief_scale (Slice 1) stays; normals + mesh-density reconcile in (see §8).
**Status:** design approved (brainstorm), pre-plan.
**Predecessor state:** M0–M4 done; relief_scale knob landed (visible==collision held); hardened GPU-time
perf gate built. Owner flew → "blobby, placed, not a contiguous landmass — kernels just placed, not
informing." Root-caused (see §1).

> Living-docs rule: write-once history. DESIGN/ROADMAP/STATUS + HANDOFF stay current. Memory:
> `worldgen10-kernel-dna-synthesis` (the north star).

---

## 1. The problem (root cause) & the vision

**Root cause of "placed, not contiguous":** `height::height` calls `sample_kernel`, which reads each
kernel's 512×512 DEM pixels as a TEXTURE via `(x/footprint_m).rem_euclid(1.0)` → every kernel TILES
(repeats every `footprint_m`); `height()` cross-fades these tiling textures by grammar weight. There is NO
continuous large-scale shape — terrain = "cross-faded repeating stamps." Worse: `footprint_m = spacing ×
px × footprint_scale(=1.0)` = the kernel's REAL-WORLD size (~50–220 km), while a clipmap level-0 page is
8 km — so up close you see ~5% of one 200 km kernel stretched flat (the "blobby" smear).

**The vision (owner-confirmed):** the real-world kernels are the SOURCE that INFORMS an infinite procedural
generator — driving height/shape/topology — then erosion + other passes layer on top, and biomes/materials
derive from the same kernel-informed data. Kernels inform the WHOLE STACK, not get stamped.

**The concrete sense of "inform" (owner-chosen): STATISTICAL DNA DRIVES SYNTHESIS.** Analyze each kernel
into a compact terrain SIGNATURE (its spectral character); runtime SYNTHESIZES infinite, non-repeating,
continuous noise parameterized by grammar-blended signatures. The kernel pixels are NEVER sampled at
runtime — the kernel is the DNA, not the body. This dissolves tiling (nothing stamped → nothing repeats)
and gives contiguity for free (noise is continuous).

## 2. Architecture & data flow

```
OFFLINE (tools/dem_pack/, Python):
  kernel DEM 512×512 → radial power-spectrum analysis → SIGNATURE
  signature = { amp_octaves[N], base_freq_per_m, relief_m }   ← terrain DNA (~10 floats)
  stored in the pack INSTEAD OF raw pixels

RUNTIME (height.rs pure Rust + GLSL mirror — parity-gated):
  height(x,z,seed,pack):
    weights   = grammar::family_weights(x,z,seed,pack)     # UNCHANGED — the WHERE
    amp[i]    = Σ_f weight_f · signature_f.amp_octaves[i]   # blend the DNA (vector sum)
    relief    = Σ_f weight_f · signature_f.relief_m
    H = 0; freq = synth_base_freq                           # × synth_scale (config)
    for i in 0..N: H += amp[i] · value_noise(x·freq, z·freq); freq *= 2.0
    return H · relief
```

**Replaced:** `sample_kernel` (tiling pixel reader) → spectral synthesis; pack `kernel` pixels → `signature`.
**Kept unchanged:** `grammar.rs` (family weights = the WHERE), `Wg10Facts` (reads `height()`), the
determinism + CPU/GPU parity CONTRACT, `relief_scale` (still multiplies the field), the raw hash/noise
primitives (still bit-exact vs WG9).

## 3. The signature (spectral analysis + schema)

Offline analysis (Python): per kernel, 2D FFT → radially-averaged power spectrum → bin into N octaves →
**amplitude-per-octave curve**, normalized so the synthesized field has unit RMS, then scaled by relief.

```json
"signature": {
  "amp_octaves": [a0, a1, ..., a_{N-1}],   // relative amplitude per octave (the spectral DNA)
  "base_freq_per_m": f0,                     // spatial frequency of octave 0 (1/metres) = largest scale
  "relief_m": R                              // vertical scale (kept from today)
  // extensible seam (v1 OMITS these; shaping adds later): "ridge_strength", "slope_skew", ...
}
```

- **N octaves** ~6–10 (landform → detail). Analysis bins to exactly N; synth sums exactly N.
- **`base_freq_per_m`** sets the largest synthesized feature scale — this SOLVES the footprint problem
  properly (synthesis says "biggest features ~X km" from the spectrum, not a stretched 200 km stamp). A
  config `synth_scale` knob (like relief_scale) dials landform size live.
- **Normalization** keeps the relief contract: unit-RMS field × relief_m × relief_scale.
- **Validation (pillar 4):** reject degenerate spectra (all-zero/NaN/non-finite) with descriptive errors;
  the pack loader validates the signature schema like it validates kernels today.
- **Storage win:** ~10 floats vs 256 K pixels per family — the pack shrinks dramatically; the 25 MB
  runtime kernel atlas DISAPPEARS. Analysis tools keep source DEMs to regenerate signatures.
- **Blend = vector sum:** a signature is an amplitude vector, so blending families is `Σ weight·amp` —
  continuous across regions because `family_weights` is already corner-blended → seamless by construction.

## 4. Runtime synthesis & CPU/GPU parity

The `height()` rewrite (see §2 pseudocode) is a **weighted sum of value-noise octaves** — the exact
operation already CPU/GPU parity-gated in M2 (`value_noise` has a proven bit-close GLSL mirror in
`height_field.glsl`).

**Parity (pillar 3 — the hard contract):**
- The GPU shader mirrors the Rust synth loop, reading the same blended amplitudes. Same `value_noise`,
  same hash primitives (bit-exact vs WG9, unchanged).
- **No tiling, no atlas** → the GPU side gets SIMPLER (no 25 MB atlas upload — just tiny per-family
  amplitude vectors). A perf WIN on the page-compute path.
- `gpu_parity_check` + the DEM parity gate **re-baselined** to the synthesis formula: still assert CPU
  height == GPU height within f32 epsilon over a coord grid. CONTRACT unchanged ("CPU==GPU"); the FORMULA
  it checks is the new synthesis.

**Performance (pillar 2):** N value-noise octaves/sample = the cost class of the M5 detail fBm (measured
~0 ms on the hardened gate). The kernel-atlas sampling + upload disappear → expected CHEAPER than today.
The hardened GPU-time gate measures the real cost.

**Continuity (no seams):** continuous world-space noise + continuous grammar weights → seamless by
construction; the axis-crossing + abutting-page seam tests re-run to confirm.

**Honesty (pillar 4):** v1 synthesis = spectral fBm — captures ROUGHNESS (Alps jagged vs dunes smooth)
correctly, but NOT necessarily STRUCTURE (connected ridgelines, drainage networks). Whether spectral-alone
reads as real terrain is the owner-fly question; the signature SEAM (ridge/slope shaping) is ready to add
if not. We do NOT oversell v1 as "looks exactly like the Alps."

## 5. The four pillars
1. **Adaptable** — signatures are pack data; `synth_scale`/`relief_scale` are config knobs; N octaves config.
2. **Performance** — cheapest synthesis (N noise octaves), atlas removed → expected cheaper; hardened
   GPU-time gate verifies.
3. **Quality** — deterministic, CPU/GPU parity (re-baselined), bounded, seamless-by-construction, no collapse.
4. **No shortcuts** — degenerate-spectrum rejection; honest that v1 is spectral (roughness), shaping seam
   ready; parity re-baselined not faked.

## 6. Verification
- **NEW spectral-fidelity gate** (Python, offline, `tools/dem_pack/` tests): kernel → signature → synth a
  field → re-analyze its spectrum → assert it matches the SOURCE kernel's spectrum within tolerance. Proves
  the kernel→DNA→synth round-trip preserves spectral character ("synth behaves like the real place"). Does
  NOT prove "looks like the Alps to a human" — that's the fly.
- **CPU/GPU parity** (re-baselined `gpu_parity_check` + DEM parity): CPU == GPU on the synth formula.
- **Determinism / bounds / facts-parity / seam:** existing gates re-run on synth (same coord→same height;
  bounded by relief × octave-ceiling; visible==collision held with relief_scale; axis/page seams zero).
- **Hardened GPU-time perf:** expect cheaper (atlas gone).
- **Owner fly (the bar):** contiguous, varied, NON-REPEATING landmass; badlands-region feels different from
  mountain-region. Only the owner judges this.

## 7. Slice plan (prove-one-at-a-time)
- **Slice 1 — Spectral analysis + signature (offline, Python; NO runtime change).** FFT→amp-per-octave in
  `tools/dem_pack/`; emit signatures; the spectral-fidelity gate; build a signature pack. *Retires the core
  research risk (does spectral synth capture a kernel?) OFFLINE before any render-path work.*
- **Slice 2 — Rust synthesis core.** Rewrite `height::height` to blend signatures + synthesize; loader reads
  signatures. Gates: determinism, bounds, variety (fast suite, headless). Raw hash/noise stay bit-exact.
- **Slice 3 — GPU parity + atlas removal.** Mirror the synth in `height_field.glsl`/`height_page.glsl`;
  remove the kernel-atlas upload; re-baseline `gpu_parity_check` (CPU==GPU). Lands the perf win.
- **Slice 4 — Integrate + facts + perf + fly.** Wire the synth pack into render + facts; confirm
  facts/collision parity + relief_scale; hardened GPU-time gate; `synth_scale` live knob. Owner acceptance
  fly. Audit; update docs.

**Order rationale:** the scariest unknown (spectral synth fidelity) is proven OFFLINE in S1 — fail cheap,
add shaping before runtime if needed. Then core (Rust) → parity (GPU) → integrate, each gated.

## 8. Reconciliation with the "shaded terrain at scale" milestone
- **relief_scale (its Slice 1): KEEP** — still multiplies the synthesized field; visible==collision holds.
- **Normals + lighting (its Slice 2): STILL APPLIES, after this** — normals shade whatever the field is;
  they'll shade the SYNTHESIZED field. Sequence normals after the synth core lands (shade the real thing).
- **Mesh density (its Slice 3): ABSORBED here** — Slice 4 perf tuning + `synth_scale` set the landform/mesh
  scale together on the hardened gate.
- The kernel-tiling/footprint_scale question is SUPERSEDED (no tiling exists after synthesis).
ROADMAP updated at spec-commit to reflect this as the foundational milestone M5–M7 sit on.

## 9. Risks & mitigations
- **Spectral synth doesn't look like real terrain (only roughness, no structure)** → proven OFFLINE in S1
  before runtime; signature seam ready for ridge/slope shaping; owner fly is the judge.
- **Parity break on the new formula** → it's weighted value-noise (already parity-proven primitive);
  re-baseline the gate; GLSL mirrors the Rust loop exactly.
- **Perf regression** → expected cheaper (atlas gone); hardened GPU-time gate measures.
- **Losing real-world fidelity** → the signature IS the kernel's measured spectrum; the fidelity gate
  asserts the round-trip preserves it.
- **Determinism/seam regression** → continuous noise + continuous weights = seamless by construction;
  existing seam/determinism gates re-run.

## 10. Definition of done (DESIGN §7.3)
- Spectral-fidelity gate green; CPU/GPU parity re-baselined green; determinism/bounds/facts-parity/seam
  green; hardened GPU-time within budget (ideally cheaper); fast/cargo green.
- Owner fly: "contiguous, non-repeating, varied landmass — kernels INFORM it."
- The 25 MB runtime kernel atlas removed; pack stores signatures.
- DESIGN/ROADMAP/STATUS + HANDOFF updated; spec committed (this file).
