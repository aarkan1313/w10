# WorldGen10 — Terrain Structure Decision Memo

_Synthesized from the 5-survey + adversarial-verification research run (raw outputs in `STRUCTURE_RESEARCH_RAW.md`). The workflow's own synthesis agent hit the session usage limit; this memo is reconstructed from the completed surveys + verdicts + a direct read of `worldgen_proto.py` / `height.rs` / `biome_distill.py`._

---

## 0. Code-reality flag (must fix understanding first)

There are **two generators** in the tree and they disagree:

- **`worldgen_proto.py`** = the warped-noise generator (macro fBm + ridged ranges − ridged valleys). This is the one the owner judged "still looks like noise."
- **`wg-10/rust/src/height.rs`** = STILL the old **kernel-tiling `sample_kernel`** path (the blobby/repeating one your own memory flagged as the root cause). It was never ported to the warped-noise design.

So "it's just a one-line edit to the existing octave loop" is **only true in numpy**. Any recommendation here is a numpy-prototype change first (render-first, owner-judged), then a Rust+GLSL port. Don't treat the Rust path as the baseline.

---

## 1. Verdict on the frame: "distill DEMs → params → local warped-noise generator"

**The frame is RIGHT, but the current generator is missing the cheap ingredients that make noise read as terrain — and the distillation is feeding it dead params.** The frame's hard ceiling (it can never produce hydrologically-real connected drainage networks) is real but is NOT what's failing today. Today's failure is mundane and fixable.

Three concrete defects visible in `worldgen_proto.generate`:

1. **No multifractal weighting.** Octaves are summed with fixed `octave_amps` — every region gets identical roughness. Real terrain is *heterogeneous*: smooth valley floors, rough peaks. This is the literal "looks like the same noise everywhere" tell. Musgrave's fix is `weight *= signal` carried across the octave loop — **zero extra noise taps, pure scalar, trivially parity-safe.** Highest leverage / lowest cost change available.
2. **Single-pass warp.** One warp at one magnitude = jitter, not meander. Recursive/double warp `fbm(p + fbm(p + fbm(p)))` is what turns jitter into flow-like coherence.
3. **Valleys are an unrelated noise field.** `h -= valley_depth * ridged_fbm(...)` subtracts an *independent* ridged field that has no relationship to where the ridges/relief are. So drainage doesn't sit between ridges — it's two noises stacked. This is the phase problem in miniature and is the biggest "not a landscape" cue.

And the distillation is feeding garbage into legitimate knobs: `ridge_linearity`, argmax `dominant_wavelength`, and `anisotropy` came out **dead-constant across all 12 biomes** (your dead-end #2), so `warp_amount` and most structure params are effectively constant. The generator's H/gain/lacunarity/offset knobs — which *do* vary across biomes — aren't even being distilled.

**What the frame can NEVER do:** produce drainage that is hydrologically connected (every valley draining to a trunk river to a basin), because a pure local `f(x,z)` cannot know its upstream catchment area. That's a genuine mathematical wall. **Does it matter for the owner's bar?** Partially. "Connected ridgelines" — yes, achievable locally (ridged multifractal + warp gives connected crests). "Branching drainage valleys" — achievable *visually* (dendritic-looking) but not *hydrologically*; good enough for the eye at gameplay scale, not good enough for "every river reaches the sea."

---

## 2. The phase/structure problem — honest resolution

**Where the impossibility is REAL:** flow accumulation is a global integral over the upstream watershed. A point can't know how much water flows through it without knowing the whole basin above it. So: true trunk-river drainage networks are **out** for pure local `f(x,z)`. This is the same reason spectral synthesis failed (phase = global relationship between frequencies; a per-pixel formula can't reconstruct it). Your owner's thesis is correct.

**The escape hatches (ranked by realism vs cost):**

1. **Fake connectivity that reads as connected without being hydrological** — ridged *multifractal* (crests connect by construction) + recursive domain warp (bends them into dendritic/meandering shapes) + Worley/cellular F2−F1 cell boundaries (give watershed-like partitioning for free, locally). This is what NMS / Minecraft / WG9 all actually do. **Cheapest, ships now, gets ~80% of the way for the eye.**
2. **Hierarchical / clipmap flow** — carry the long-range information at COARSE clipmap levels (a low-res flow/drainage field computed over a bounded coarse window), let fine levels read it as a slowly-varying input. This is the *only* path to genuinely-connected long drainage that's still streaming-compatible. Real but heavy; it's a research milestone, not slice 1.
3. **Offline-distilled SHORT-range erosion operator** (owner's idea) — can reproduce erosion *texture* (slope-damped smoothing, gully streaking, sediment-rounded valley floors) but **NOT** trunk drainage, because the local operator still can't see upstream area. Worth it later as polish; **the adversarial review flagged the "learned CNN operator" version as the single least-supported / highest-risk claim** in the research — a per-page halo+conv breaks "pure f(x,z)" and its CPU/GPU bit-parity (to your 0.001 m bar) is a large unproven surface. Don't spec the learned version yet.

**Honest ceiling:** with hatch #1 you beat WG9 and get terrain that reads as real geography at a glance and in flight. You do NOT get hydrologically-correct rivers. Hatch #2 is the only thing that does, and it costs a clipmap-flow subsystem.

---

## 3. Recommended direction (committed)

**PRIMARY: Path A — upgrade the noise stack, local-only, ships now.** Specifically, in order:

1. **Ridged multifractal weighting** (`weight *= prev_signal` in the octave loop). Free, parity-trivial, fixes uniform-roughness. *Do this first.*
2. **Per-biome H / gain / lacunarity / offset** as the distilled params (these vary across biomes; the dead structure-tensor metrics don't).
3. **Recursive domain warp** (2-level) replacing the single warp.
4. **Couple valleys to ridges** — carve where the ridge field is *low* / between crests, not an independent field. (Even just `valley *= (1 - ridge_signal)` is a big step.)
5. **Then, behind a gradient-parity gate:** one derivative-aware erosion trick — **pick Jordan turbulence** (de Carpentier) or IQ derivative-damp `/(1+dot(d,d))` — for slope-dependent erosional look. **Parity caveat the research flagged:** this needs analytic-derivative noise returning `(value, dn/dx, dn/dz)` *and* a per-octave rotation matrix, both of which must be bit-identical Rust↔GLSL. That's a second parity surface — gate it, don't assume "parity: yes."

**SEQUENCE AFTER:** Minecraft-style **spline-of-noise channels** (continentalness / erosion / peaks-valleys as separate low-freq noises combined via per-biome splines) — this is the clean, proven home for distilled DEM params and gives coherent biome transitions cheaply and locally. Strong candidate to *become* the param schema.

**DO NOT (yet):** the learned-CNN local erosion operator (parity + "pure f(x,z)" risk, under-supported); full hydraulic sim at runtime (impossible under constraints); any return to spectral/FFT.

**RESERVE:** hierarchical clipmap-flow (hatch #2) as the one real path to true drainage if/when "looks connected" stops being enough.

---

## 4. The DEM measurement answer (replace the dead metrics)

The current metrics fail because they're generic 2D-signal stats. Replace with **geomorphometric** metrics known to vary by erosional regime, and map each to a generator knob:

| DEM metric | What it captures | Generator knob it drives |
|---|---|---|
| **Hypsometric integral** (elevation-area curve) | Erosional maturity — separates young/sharp vs old/rounded vs dissected | multifractal **H / offset** (peakedness vs flatness) |
| **Slope–area scaling exponent θ** | Erosional regime signature (the slope-area relationship) | valley-carving strength + curvature of carve profile |
| **Drainage density** | How finely dissected by channels | valley_freq + valley_depth |
| **Ruggedness (TRI / VRM)** | Local roughness magnitude | gain / high-octave amps |
| **Curvature distribution** (profile/plan, % concave vs convex) | Valley-bottom vs ridge-crest mix | ridge_strength vs valley_depth balance |
| **Relief ratio / local relief** | Amplitude (already have, keep) | relief_m |

These are compact scalars that **should** actually separate mountain/badlands/karst/glacial/etc. on 512px COP30 data (unlike structure-tensor coherence). **Honest limit:** metrics can only *tune* a structure-generating function — they cannot *create* structure the generator's basis doesn't already produce. So fixing the generator (§3) comes first; better metrics are worthless feeding a basis that can't make structure.

**Hand-metrics vs offline-learn:** hand-metrics → params is right for Path A and is far cheaper/parity-safer. Offline-learning is only worth it for the erosion-texture polish later, and even then prefer "learn a few scalar transfer functions / splines" over "learn a conv operator."

---

## 5. Tradeoffs vs infinite / parity / perf

- **Multifractal + recursive warp + coupled valleys:** ~same octave budget (maybe +1 warp fbm = +3 taps). Parity: trivial (scalars). Determinism: unchanged. **Visual ceiling: beats WG9.** This is almost free.
- **Derivative erosion (Jordan/IQ):** +derivative channel per octave, +rotation matrix. Parity: *conditional* — new gradient-parity gate required. Perf: moderate. Ceiling: erosional look, still no connected basins.
- **Splines-of-noise:** cheap (eval a few low-freq noises + spline lookups). Parity: easy (piecewise-linear splines are bit-stable). Good param home.
- **Clipmap-flow (reserved):** real cost — a coarse bounded flow subsystem. Only path to true drainage.

---

## 6. Concrete first slice (render-first, owner-judged — your proven discipline)

**Slice: "Multifractal + recursive warp + coupled valleys" in `worldgen_proto.py`, A/B rendered against current + against a real DEM hillshade.**

1. Add `hybrid_multifractal` octave loop (`weight *= signal`, clamp [0,1]) with per-biome `H`, `offset`, `gain`, `lacunarity`.
2. Replace single warp with 2-level recursive warp.
3. Change valleys from independent subtraction to carve-between-ridges (`valley_depth * ridged * (1 - ridge_signal)` or similar).
4. Render hillshade for mountain + badlands + glacial (the 3 most distinct), side-by-side: **current proto | new proto | real DEM**.
5. Owner judges. If "yes, that's terrain" → port to Rust+GLSL with parity gate, then tackle the metric replacement (§4).

This validates on rendered images before any runtime rebuild — exactly the discipline that killed the spectral approach cheaply (~half a day, no GLSL).
