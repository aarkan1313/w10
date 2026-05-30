# WorldGen10 Structure Research — salvaged agent outputs

_Final synthesis agent hit session limit; this is the raw survey+verdict output salvaged from agent transcripts._



========================================================================
## AGENT agent-a008c3557f5b08851.jsonl  (final text 64 chars)
========================================================================

You've hit your session limit · resets 9:50pm (America/New_York)


========================================================================
## AGENT agent-a01938670c6364de4.jsonl  (final text 23680 chars)
========================================================================

I now have everything needed, including the canonical stream power law (U = K·A^m·S^n) that underpins the analytical-erosion and tectonic-uplift literature. I have enough to write the brief. Let me do one final targeted check on the steady-state slope-area relation that gives valley spacing, since that's the load-bearing geomorphology insight for the owner.

The steady-state relation U = K·A^m·S^n rearranges to S = (U/K)^(1/n)·A^(-m/n), which is the key: slope decreases as a power of upstream drainage area. This is the connection between uplift, erosion, and the characteristic concave-up river-profile structure. I have all the source material. Writing the synthesis now.

---

# Technical Brief: Domain Warping, Analytic Uplift, and Local Erosion Structure for WorldGen10

## 0. Grounding note on the actual code (important correction)

Two different generators exist in the tree, and they are not the same thing:

- **`worldgen_proto.py`** (`D:/workflows/worldgen10/tools/dem_pack/worldgen_proto.py`) — this **is** the warp → macro-fBm → ridged ridges → inverted-ridged valley generator described in the prompt. It warps once by `warp_amount` (≈ `0.35 * anisotropy * 8000 m`) at `warp_freq = 1/(2.7·8000m)`, 3-octave warp field.
- **`wg-10/rust/src/height.rs`** — the *current Rust runtime* does **not** run that formula at all. It **tiled-bilinear-samples the DEM kernels** (`sample_kernel` wraps every `footprint_m`) and blends families by grammar weight. The file's own comments admit "visible creases at footprint seams are expected for naive tiling." **This is dead-end #3 (tiling kernels) shipping as the runtime height.** Whatever you decide below, the Rust height path needs to be replaced by the analytic generator to even be in the conversation — right now CPU/GPU "parity" is parity over a tiled stamp, not over the procedural function.

The distillation (`biome_distill.py`) confirms the dead constants noted in memory: `ridge_linearity ≈ 0.30` and `dominant_wavelength_m ≈ 25 km` for every family, so they are explicitly **not read** by `params_from_metrics`. Only `relief_m`, `slope_bias_deg`, `incision/relief`, `amp_profile`, and `anisotropy` actually vary and drive knobs.

With that grounded, here is the survey.

---

## 1. Domain warping — magnitude is the whole game

**The canonical IQ construction** (iquilezles.org/articles/warp):

```
q = vec2( fbm(p + (0,0)),       fbm(p + (5.2,1.3)) );
r = vec2( fbm(p + 4.0*q + (1.7,9.2)), fbm(p + 4.0*q + (8.3,2.8)) );
return fbm( p + 4.0*r );
```
The offset constants "don't have any special meaning." The **load-bearing number is the `4.0` multiplier** on `q` and `r`.

**Magnitude analysis — this is where your current generator is mis-tuned.** Warp displacement should be read as *a fraction of the warped feature's wavelength*, not as an absolute metre count:
- **Displacement ≪ wavelength (< ~10%)**: jitter. Edges wobble, nothing reorganizes. "Looks like the same noise."
- **Displacement ≈ 0.3–1.0 × wavelength**: the IQ `4.0` regime. Features bend, fold, and form sinewy/meandering organic shapes. This is where warp stops being cosmetic and starts creating apparent flow.
- **Displacement ≫ wavelength**: it breaks apart — concentric artifacts spiral around the warp field's own peaks/abysses (corroborated by the Jason Dookeran and 3DWorld writeups: "the whole thing breaks apart, with concentric artifacts").

**Your current setting is almost certainly too weak AND applied at the wrong scale.** `warp_amount = 0.35 · anisotropy · 8000m`. With real `anisotropy ≈ 0.30` (from the distill), that's `0.35 · 0.30 · 8000 ≈ 840 m` of displacement — but it's warping a **macro fBm whose base wavelength is 8000 m**. So you're displacing an 8 km feature by ~0.84 km ≈ **0.1 × wavelength: textbook jitter, not the `4.0` regime.** IQ's `p + 4.0*r` with unit-amplitude fbm and unit-frequency domain means displacement on the *order of the feature wavelength itself*. To match IQ you want `warp_amount` in the range **~0.3–1.0 × (1/base_freq)** = **2400–8000 m**, i.e. 3–10× what you ship now. The `anisotropy·0.35` mapping caps you well below the structural threshold for every biome.

**Why recursive warp creates flow-like coherence (the mechanism IQ doesn't spell out, but it's real):** a single warp `f(p + a·g(p))` shifts each sample by a smooth low-frequency vector field. Because `g` is itself smooth, *nearby points move together* — that's what turns isotropic blobs into elongated, banded, foliated shapes (the "flow" look). **Double warp** `f(p + a·g(p + a·g(p)))` warps the warp field, so the bending varies smoothly across space — straight folds become curved, branching, turbulent. Each level of recursion adds one more order of large-scale spatial correlation to the displacement. **But — and this is the honest verdict — warp does not create drainage. It produces *correlated curvature* (folds, swirls, foliation: it genuinely looks like tectonically folded strata), which is why everyone says it resembles "terrain deformed by tectonic movement." It does not produce *connected downhill channels*. It rearranges phase locally but has no notion of "downhill," so it cannot make a connected drainage tree.** It is the right tool for the *tectonic/fold* half of your problem, the wrong tool for the *fluvial/drainage* half.

| | Verdict |
|---|---|
| Local? | **Yes** — bounded noise lookups, no global state. |
| Parity-able? | **Yes** — pure fbm/add/mul, trivially identical in Rust+GLSL. |
| Cheap? | Single warp = 2 extra fbm evals; double = 6. Affordable. |
| Structure or nicer noise? | **Folds/foliation = real-ish structure. Drainage = no.** Fixes the "noise" complaint partially by giving directional coherence, but won't pass as connected ranges/valleys alone. |

**Action:** raise warp to the `4.0`/wavelength-fraction regime and use **double (recursive) warp**. This alone will move the needle on "looks like the same noise" because your warp is currently sub-threshold.

---

## 2. Worley / cellular noise — good for cells & rims, weak for drainage

**Facts (Worley 1996; IQ voronoi/voronoilines; Book of Shaders 12):**
- `F1` = distance to nearest seed; `F2` = second nearest.
- `F1` alone with raised rims ⇒ **craters** (SideFX/Astrographer recipes: rim raised, interior depressed). Excellent, cheap, parity-able for **volcanic/karst/badlands** biomes.
- **`F2 − F1` → 0 along cell boundaries ⇒ a connected web of ridge/crack lines.** This is the tempting "drainage-like network."

**The catch IQ flags directly** (voronoilines): `F2 − F1` "is not a distance really, as it expands and contracts," so the network has uneven thickness. The *true* edge distance needs his two-pass method — find the owner cell, then `dot(0.5*(mr+r), normalize(r−mr))` to each neighbor edge — which is a **3×3 or 5×5 cell scan (9–25 samples)**, still local and parity-able but several× the cost of plain fbm.

**Is cell-boundary structure cheap local drainage-network-LIKE connectivity? Partially, and this is the key nuance:**
- A Voronoi edge graph *is* a connected planar network — that's genuinely more than noise can do, and it's the single cheapest way to get **guaranteed connectivity**.
- **But it is the wrong topology.** Real drainage networks are **dendritic trees** (branches merge downhill, never cross, have a single outlet). Voronoi edges form **loops/cells** (a tessellation), every junction is a 3-way meeting of *cells*, and there is no downhill direction. It reads as "cracked mud / reptile skin / basalt columns," not "river valleys." This is exactly why karst/badlands/volcanic look right with it and mountain/temperate do not.
- **The fix that makes it a tree (the important technique):** don't use the raw edges — build a **minimum-spanning-tree / downhill-connected jittered-grid graph** ("pseudoerosion" family, Janert; dandrino terrain-erosion-3-ways). Jitter a grid of points (Poisson-ish), connect each point to its lowest neighbor in a **3×3** window, then per pixel take distance-to-nearest-segment of that local tree. That converts the loopy Voronoi tessellation into a branching dendritic network and is still local (3×3 cells). The honest cost: connecting "lowest neighbor" needs a height to sort by — so this layers *on top of* a base height function, it isn't standalone.

| | Verdict |
|---|---|
| Local? | **Yes** (3×3–5×5 cell window). |
| Parity-able? | **Yes** (fixed-iteration cell scan; the standard hashed-grid Worley). |
| Cheap? | F1/F2 cheap; true edge-distance / MST-tree = 9–25 samples, moderate. |
| Structure or nicer noise? | **Real connectivity, wrong topology** for rivers (cells not trees) unless you add the downhill-MST step. **Right for craters/karst/columnar/tectonic-block boundaries; not for dendritic drainage out of the box.** |

---

## 3. Analytic uplift / continent shaping — strongly recommended, this is the real fix for "connected ranges"

This is the most under-used lever in your current generator and the one geomorphology says actually produces structure.

**(a) Continent/highland masks.** `smoothstep` of a very-low-frequency fBm (wavelength ≫ feature scale) → a coherent 0..1 highland/lowland mask that multiplies relief and gates where ridges appear. Your generator *has* a weak version of this (`upland = smoothstep(h)`), but it's derived from the *same* macro fBm as the detail, so it can't impose independent large-scale organization. Use a **separate, much lower-frequency** uplift field. Local, parity, cheap (1–2 octaves). Verdict: real large-scale structure (where mountains *are*), not drainage.

**(b) Tectonic mountain belts via cellular boundaries.** Place "plates" as a coarse Worley field; the **plate boundaries (`F2−F1` ridges) become mountain belts** — this is how you get *long, continuous, connected ranges with a coherent trend* instead of scattered ridged-noise peaks. This is the single best cheap trick for "a real connected mountain range" because the belt is connected **by construction** (it's a continuous Voronoi boundary), and you then drape ridged-fBm detail *along* it. Local, parity, cheap-ish. **This directly attacks the owner's "branching connected ridgelines" requirement.**

**(c) The geomorphology that should drive (a)+(b) — the stream power law.** The entire academic terrain literature (Cordonnier "Large Scale Terrain from Tectonic Uplift and Fluvial Erosion"; Tzathas "Physically-based analytical erosion," EG 2024) is built on one equation. At steady state, **uplift balances erosion**:

```
U = K · A^m · S^n          (stream power law; common m≈0.5·n, m/n≈0.5)
⇒  S = (U/K)^(1/n) · A^(−m/n)
```
Read that second form: **channel slope falls off as a power of upstream drainage area A.** That is *the* reason real terrain reads as real — concave-up valleys, steep headwaters, flat trunks, and a characteristic ridge-to-valley spacing set by U/K. **This is the "phase/structure" the owner keeps hitting:** it is a relationship between a point's slope and *how much area drains through it*, which is inherently non-local (A is an integral over the whole upstream basin). That is precisely why classical noise can never produce it and why FFT/spectral failed — the structure is in the **flow-routing/phase relationship, not in any amplitude spectrum.**

| | Verdict |
|---|---|
| Local? | Masks/belts **yes**; true `A` (drainage area) **no** (global integral). |
| Parity-able? | Masks/belts **yes**; `A` needs approximation to be parity-able. |
| Cheap? | Masks/belts **yes**. |
| Structure? | **Belts = genuine connected-range structure (best cheap win). Full uplift+erosion balance = the real thing but needs the A approximation in §6.** |

---

## 4. The combination question: `warp(uplift + belts + ridged + local-erosion)`

**Yes — `warp(uplift + ridged)` is categorically better than your current `warp(sum-of-bands)`, but the win comes from the *uplift/belt term*, not the warp.** Recommended composition, in order:

```
1. uplift(p)        = smoothstep(lowfreq_fbm)            // where land is high  (§3a)  — coherent regions
2. belt(p)          = ridge_profile(F2−F1 of coarse Worley)// long connected ranges (§3b) — connectivity
3. base(p)          = uplift * macro_fBm                  // landmass
4. ranges(p)        = belt * ridged_fBm                   // detail ON the belts, not everywhere
5. h = base + ranges
6. h = erosion_filter(h, ∇h)                              // §5/§6 — dendritic gullies, the drainage
7. p was pre-warped by DOUBLE warp at ~0.5–1.0×wavelength // §1 — folds/meander, applied to the DOMAIN
```

The reason this reads as "terrain" and the current sum-of-bands doesn't: sum-of-bands superimposes statistically-identical noise at every location, so **every place looks like every other place** (the owner's exact complaint). `uplift` + `belt` impose *independent large-scale organization* (here-is-a-range, here-is-a-basin) that the detail then decorates. Warp folds it organically. Erosion filter (next) adds the dendritic drainage that nothing above can.

---

## 5. The breakthrough you should adopt: the slope-aligned erosion *filter* (clayjohn → Fewes → runevision/Phacelle)

This is the most important practical finding in this survey and it directly resolves the owner's "real erosion needs global computation" thesis. There is a **local, single-pass, per-pixel noise that produces branching dendritic gullies/ridges** — described in the field as "**somewhat of a holy grail in online procedural terrain generation**."

**Lineage:** guil's *Gavoronoise* (Gabor×Voronoi stripes) → clayjohn *Eroded Terrain Noise* (2018, Shadertoy `MtGcWh`) → Fewes *Terrain Erosion Noise* (2023, `7ljcRW`) → Rune Skovbo Johansen *Phacelle* / *Fast and Gorgeous Erosion Filter* (2026).

**The mechanism (clayjohn, in his words):** "a noise function that looked like eroded terrain, complete with branching structure, that could be run in a single pass pixel shader." Concretely:
1. You have height **and its gradient ∇h** at the point (analytic — see §7). Water flows along **−∇h**.
2. Lay down **stripes perpendicular to the flow** using a cosine wave (gully bottoms) and its sine (the slope of the gully) — Gabor/Voronoi-interpolated so it tiles infinitely from a **3×3 cell window**.
3. **Stripe frequency ∝ slope**, so stripes are thicker where flat; at peaks/valleys (slope→0) stripes go infinitely thick, pinning peaks/valleys onto ridge lines.
4. **Accumulate the stripe's analytic derivative into the gradient** fed to the next octave. Because the *sides* of each gully create new local gradients, the next octave's stripes branch off at an angle ⇒ **fractal dendritic branching**, exactly the drainage topology Voronoi-edges lack.

**Why this beats your ridged-noise valleys (the current bar):** ridged-fBm valleys are still isotropic noise — valleys point in random directions and don't connect downhill. The erosion filter's gullies are **aligned to the actual local flow direction and branch like a real drainage tree**, while remaining a pure function of `(p, ∇h)`. Runevision: "produces gorgeous branching gullies and ridges, while still allowing every point to be evaluated in isolation… fast, GPU-friendly, and trivial to generate in chunks." It is applied **as a filter on top of any height function** — drop it on top of your §4 stack.

| | Verdict |
|---|---|
| Local? | **Yes** — per-point + 3×3 cell window, no simulation, no global pass. |
| Parity-able? | **Yes** — fixed-octave loop, sin/cos/dot/normalize; identical Rust+GLSL. The owner's parity discipline is satisfiable. |
| Cheap? | ~a few octaves; Phacelle ≈ 16 taps/pixel. Comparable to a Worley-edge eval. |
| Structure or nicer noise? | **STRUCTURE — connected branching dendritic gullies.** This is the closest thing to "real drainage" that is genuinely local+cheap. The honest limit (runevision): "stripes cannot consistently produce unbroken lines" — a gully can stop mid-slope; it's drainage-*like*, not flow-routed truth. |

**This is the single highest-value thing to prototype next**, in `worldgen_proto.py`, render-first, before any Rust rebuild. Port clayjohn/Fewes `erosion()` from Shadertoy.

---

## 6. The owner's "offline-erosion → learned local operator" thesis — partly already solved analytically

The owner wants: run real hydraulic erosion offline, distill a cheap local online operator. The literature says **you may not need to learn it — there's a closed-form analytic version**, and the §5 filter is a hand-crafted version of exactly that operator.

- **Tzathas et al., "Physically-based analytical erosion for fast terrain generation," CGF/Eurographics 2024:** uses **analytical solutions of the stream power law** instead of iterative simulation; "relies on a fast yet accurate **approximation of drainage area and flow routing** to compute the erosion interactively." Input = terrain + uplift map + time → eroded terrain. **The honest constraint:** their drainage-area approximation is *fast* and *interactive* but is **not fully per-pixel-local** — `A` is fundamentally an upstream integral. For your hard "bounded-neighborhood" constraint, you can only approximate `A` from *local* proxies (concavity, local slope, a few-ring flow-accumulation, or substitute the §5 stripe density). So: the **stream power law is the physics to target; the §5 erosion filter is the local approximation of it; a learned operator is a third option only if the filter's eye-test fails.**
- **Cordonnier "Large Scale Terrain from Tectonic Uplift and Fluvial Erosion":** confirms the thesis at the macro scale — realistic large terrain = **uplift map + fluvial (stream-power) erosion**, *not* noise. This validates §3's uplift/belt approach as the principled backbone.
- **Learned operators exist** (graph-NN terrain, neural erosion approximations, Terrain/Infinite-Diffusion arXiv 2512.08309 which explicitly bills noise as "fast and infinite, yet fundamentally limited in realism and large-scale coherence"), **but** they break parity (a neural net in GLSL+Rust with bit-identical output is a nightmare) and are heavier per-pixel. **Recommendation: exhaust the analytic erosion filter (§5) first; reach for learning only if the eye-test demands it.** If you do learn, learn *coefficients* of the analytic operator (a handful of per-biome scalars: erosion strength, branch angle bias, stripe-frequency-vs-slope curve), not a black-box network — those stay parity-able and slot straight into your biome-param distillation.

---

## 7. Cross-cutting enabler: carry analytic derivatives everywhere

Three of the best techniques (IQ derivative-fBm erosion `a += b·n.x/(1+dot(d,d))`; de Carpentier Swiss/Jordan turbulence; the §5 erosion filter) all require the **analytic gradient ∇h alongside h**. So your noise primitives should return `(value, d/dx, d/dz)` from the start (Brian Sharpe's analytic-derivative value/gradient/Worley noise; IQ "morenoise" — "analytical derivatives… 5 times faster" than central differences). This is cheap, parity-trivial, and unlocks: free normals for shading, slope-moderation without finite differences, and the flow direction the erosion filter needs. **Refactor `value_noise`/`fbm` to return derivatives before building §5.**

De Carpentier's **`iqTurbulence`** is the cheapest immediate upgrade to your macro term (drop-in for step 3–4):
```
dsum += n.yz;
sum  += amp * n.x / (1 + dot(dsum, dsum));   // flat areas accrue detail; slopes stay smooth
```
and his **Swiss turbulence** warps each octave's domain by the accumulated gradient (`p += warp * dsum`), which is a poor-man's flow alignment — a stepping stone to the full §5 filter.

---

## 8. Prioritized recommendations (render-first, parity-safe, all local+cheap)

1. **Fix warp magnitude + go recursive (§1).** Raise `warp_amount` to ~0.3–1.0×(1/base_freq) ≈ 2400–8000 m, double-warp. Cheapest change, directly addresses "same noise." Your current 840 m is sub-threshold jitter.
2. **Add independent low-freq uplift mask + Voronoi-boundary mountain belts (§3a/§3b).** This is what creates *connected ranges* and *here-vs-there* variety. Decorate belts with ridged-fBm instead of spraying ridges everywhere.
3. **Make noise return analytic derivatives (§7).** Enabler for 4.
4. **Port the clayjohn/Fewes slope-aligned erosion filter (§5)** on top of the §1–§3 height. This is the "real drainage, but local" payoff and the strongest beat over WG9's ridged-noise valleys. Prototype in `worldgen_proto.py`, owner eye-tests it.
5. **Use Worley F1-craters / F2−F1 boundaries for the cellular biomes (§2)** (volcanic, karst, badlands) — don't force it on mountain/temperate.
6. **Treat stream power law (§3c/§6) as the target physics; only learn scalar coefficients, never a black box.** Keep parity sacred.

**Distillation tie-in:** the new knobs are biome-distillable from your existing varying metrics — `anisotropy` → warp magnitude (rescaled up, not 0.35); `slope_bias_deg` → belt strength + erosion-filter intensity; `incision/relief` → erosion-filter depth + stream-power K; `amp_profile` → octave amps as today. The dead metrics (`ridge_linearity`, `dominant_wavelength`) stay unused, as you already decided.

**One-line resolution of the owner's tension:** real drainage *connectivity* does need a flow relationship, but the slope-aligned erosion filter (clayjohn/Fewes/runevision) and Voronoi mountain belts deliver *connected branching structure* as **pure local functions of `(p, ∇h)`** — you do not need a global sim, you need (a) gradient-carrying noise, (b) uplift/belt organization so detail isn't uniform, and (c) the slope-aligned gully filter that fakes flow routing per-pixel. Spectral failed because structure lives in flow-phase, not amplitude; these techniques put the phase back in *locally*.

---

### Key sources
- IQ: [Domain warping](https://iquilezles.org/articles/warp/), [fBm](https://iquilezles.org/articles/fbm/), [noise derivatives/morenoise](https://iquilezles.org/articles/morenoise/), [voronoi edge distance](https://iquilezles.org/articles/voronoilines/)
- de Carpentier Scape: [procedural basics](https://www.decarpentier.nl/scape-procedural-basics) (`iqTurbulence`), [extensions](https://www.decarpentier.nl/scape-procedural-extensions) (Swiss/Jordan turbulence)
- Erosion filter: clayjohn *Eroded Terrain Noise* (Shadertoy `MtGcWh`), Fewes *Terrain Erosion Noise* (`7ljcRW`), [runevision Fast & Gorgeous Erosion Filter](https://blog.runevision.com/2026/03/fast-and-gorgeous-erosion-filter.html), [Phacelle](https://blog.runevision.com/2026/01/phacelle-cheap-directional-noise.html), [80.lv writeup](https://80.lv/articles/fast-terrain-erosion-filter-that-emulates-erosion-without-simulation)
- Geomorphology/physics: [Stream power law (Wikipedia)](https://en.wikipedia.org/wiki/Stream_power_law), Tzathas et al. *Physically-based analytical erosion* (CGF/EG 2024, [Wiley 10.1111/cgf.15033](https://onlinelibrary.wiley.com/doi/10.1111/cgf.15033)), Cordonnier *Large Scale Terrain from Tectonic Uplift and Fluvial Erosion*
- Limits of noise: [Red Blob Games terrain-from-noise](https://www.redblobgames.com/maps/terrain-from-noise/) ("no global features like a river flowing from the peak to the ocean"), Infinite/Terrain Diffusion ([arXiv 2512.08309](https://arxiv.org/abs/2512.08309))
- Worley: [Wikipedia](https://en.wikipedia.org/wiki/Worley_noise), [Book of Shaders ch.12](https://thebookofshaders.com/12/); Sharpe analytic-derivative noise

**Relevant local files:** generator to replace = `D:/workflows/worldgen10/wg-10/rust/src/height.rs` (currently tiling kernels — dead-end #3); prototype target = `D:/workflows/worldgen10/tools/dem_pack/worldgen_proto.py`; knob mapping = `D:/workflows/worldgen10/tools/dem_pack/biome_distill.py`.


========================================================================
## AGENT agent-a070888824eae5ce2.jsonl  (final text 16087 chars)
========================================================================

I have everything I need. The grounding in the brief is accurate. Now I can deliver the adversarial verdict.

Key facts confirmed from the code:
- `worldgen_proto.generate` is exactly `value-noise fBm + upland-gated ridged noise + subtractive ridged noise`, all pure `f(x,z)` with fixed-octave loops. Confirmed parity-safe basis with no connectivity.
- `params_from_metrics` reads only `slope_bias_deg`, `incision_depth_m/relief`, `anisotropy`, and `amp_profile`. `ridge_linearity` and `dominant_wavelength_m` are computed-but-not-read (confirmed dead). So ~3 live DOF, not 2 (the brief says 2 — it missed that `anisotropy` still drives `warp_amount` and `amp_profile` drives octave amps).
- `height.rs` still tiled-samples kernels (`sample_kernel` with `rem_euclid` wrap) — the forbidden runtime path is live, exactly as the brief says.

Now the verdict.

---

# ADVERSARIAL VERDICT — the dem-metrics brief

## Bottom line up front

The brief is **mostly correct and unusually honest** — Section 3 ("metrics tune, they don't create") is the load-bearing truth and it states it plainly instead of burying it. Its central claim is sound: **the dead metrics died because they were generic-signal statistics on a 512px field, and geomorphometric metrics on derived fields (hypsometry, curvature signs, slope moments) will genuinely vary across your 12 families.** That part is adopt-worthy and cheap.

But the brief **smuggles the owner's actual problem out the back door.** The owner's complaint is "still looks like noise" — a *phase/connectivity* problem. The brief's headline deliverable (Section 5's metric table) does **nothing** for that. It makes biomes *differ from each other* (a real but lesser win) while leaving every biome individually still-noise. The brief admits this in §3 but then the §5 "minimal high-value subset" quietly omits the only items (§3 network primitives, §4 Strategy 2) that address the real complaint. **A reader who implements the §5 fast-subset will fix "all biomes look the same" and not fix "looks like noise," and will think they followed the brief.** That is the trap to call out.

One factual correction: the brief says you're running on "**two** real degrees of freedom." Wrong — `params_from_metrics` reads **four** live inputs: `slope_bias_deg`→ridge_strength, `incision/relief`→valley_depth, `anisotropy`→warp_amount, `amp_profile`→octave_amps. `ridge_linearity` and `dominant_wavelength_m` are the only truly dead-and-unread ones. Minor, but it's a grounding overclaim.

---

## Per-technique verdict table

Legend — local: is it a bounded f(x,z)? parity: compiles identically to Rust+GLSL? real_structure: does it create connected ridgelines/drainage (the phase), or just better-looking noise?

### The METRICS (offline measurement — these never run at runtime, so "local/parity" applies to the KNOB they drive, not the metric)

| Metric | varies on 512px? | offline-cost | drives a parity-safe knob? | verdict | why |
|---|---|---|---|---|
| **HI + curve skew/kurtosis (#1)** | yes (strong) | trivial (histogram) | yes (amplitude/upland split) | **ADOPT** | Pike–Wilson HI=(mean−min)/(max−min) is real and 3 numbers. Genuinely separates glacial/volcanic/old. No flow routing. Lowest-risk win. |
| **Curvature-sign stats (#2)** | yes (strong) | cheap stencil | partial | **ADOPT (measure), MAYBE (knob)** | The U-vs-V discriminator is real. BUT: the *knob* it drives (`pow()` carve cross-section) reshapes a valley that ridged-noise already places — it does NOT make the valley *connected*. Measure it; don't oversell the knob. |
| **Slope distribution moments (#4)** | yes (strong) | trivial | yes | **ADOPT** | std/skew/fraction-at-repose from the same array. Cheapest real DOF you can add. Clearly maps to ridge_strength/clamp. |
| **VRM (#5)** | yes (good, orthogonal to mean-slope) | cheap | yes (hi-freq octave amp) | **ADOPT** | Genuinely orthogonal to your existing `mean_slope_deg`. Only changes roughness-at-constant-slope = texture, not structure, but it's an honest texture knob and it varies. |
| **Windowed local relief @2km/@10km ratio (#6)** | yes | cheap | yes (base vs ridge freq balance) | **ADOPT (cheap)** | Real texture-scale fingerprint. Safe, local windows, offline. |
| **Drainage density Dd (#3)** | yes (strong; karst-inverter) | needs flow routing | only if you ADD a network primitive | **MAYBE** | Dd is a real discriminator. But its knob ("valley_freq / cell size") is meaningless without a connectivity primitive to set the spacing OF. Measuring Dd to tune ridged-noise frequency just changes how many disconnected creases you get. Adopt **after** §3, not before. |
| **Slope–area θ + k_sn (#7)** | weak/noisy on 512px (brief admits) | needs flow routing; needs channel-pixel count | only with network primitive | **MAYBE → lean REJECT for now** | Brief concedes "noisy on 512px," relies on family-aggregation to rescue it. θ is the right *concept* but you can't fit Flint's law cleanly on 512px tiles, and the knob again presupposes channels exist. High effort, fragile, deferred-value. |
| **Pit/closed-depression density (#8)** | yes (clean karst/glacial flag) | local-ish | drives a SWITCH, not a dial | **MAYBE** | Honestly useful as a *biome classifier sanity check* (karst should have pits, fluvial shouldn't). But the "knob" is "turn on a closed-basin primitive you haven't built." It's a flag, not a generator input yet. |
| **TWI right-tail (#9)** | yes (wetland/coast) | needs flow routing | weak knob ("flatten") | **REJECT for now** | Needs routing for a flatness signal you can get more cheaply from slope moments (#4 already catches flat-wet via low slope). Redundant with #4 at higher cost. |
| **Junction-angle / bifurcation ratio (#10)** | yes (descriptive) | needs full network extraction | NOT a dial (brief admits) | **REJECT as a metric / KEEP as a diagnostic** | Brief is honest that this "is not a scalar." It only tells you *which §3 primitive to pick*. Don't build network extraction on 512px tiles to learn what you already know (badlands=dendritic, volcano=radial). Decide primitives by eye. |

### The dead metrics (brief says don't re-add — I CONCUR on all)
Power spectrum / spectral β, structure-tensor coherence/anisotropy, argmax-DoG wavelength, fractal dimension — all correctly buried. **One catch: your live code STILL reads `anisotropy` (structure-tensor coherence) to drive `warp_amount`.** The brief lists anisotropy as dead (§Tier D) but doesn't flag that it's still wired in. If anisotropy is ~0.30 for everything, `warp_amount` is near-constant across biomes too — so you have a *fifth* dead-ish DOF the brief missed. **Recommend: cut anisotropy→warp_amount, replace with a real knob, OR confirm anisotropy actually varies before trusting it.**

### The GENERATOR PRIMITIVES (§3 — these DO run at runtime; parity/local matters for real)

| Primitive | local? | parity? | real_structure? | verdict | why / where it breaks |
|---|---|---|---|---|
| **Worley/cellular flow-edges (§3.1)** | **YES** | **YES** | **PARTIAL → the best partial available** | **ADOPT (spec this first for the phase problem)** | Voronoi edges ARE a connected, branching, space-filling graph — strictly more network-like than `1−|fbm|`. Analytic, O(9) cell lookups, identical in Rust/GLSL. **Honest limit the brief undersells:** Worley edges branch but are NOT downhill-monotone and junctions are ~120° (Voronoi), not acute Hortonian. So it gives *connectivity* but not *correct drainage hierarchy*. Still: it's the only §3 option that is unconditionally local+parity+cheap AND adds genuine connectivity. This is the one real lever toward the owner's goal. |
| **Bounded N-step gradient-descent trace (§3.2)** | **CONDITIONAL** | **CONDITIONAL** | partial | **MAYBE — has a hidden parity landmine** | A fixed-16-step descent is bounded, fine. BUT: (a) the trace is **data-dependent** — the path each step takes depends on the local gradient, and small CPU/GPU float differences in early steps **diverge** by step 16 → the accumulated "flow proxy" will NOT be bitwise-parity between Rust f64 and GLSL f32. The brief claims "parity-safe" — **this is an overclaim.** You'd need to force f32 on both sides AND accept that the discrete path is a chaotic map. (b) "distance-to-channel" from a 16-step trace only sees 16 cells of macro-terrain; the trunk valley it's trying to find may be 100 cells away. It injects *local* downhillness, not *long-range* connectivity. **Spec only if you fix the f32-parity discipline AND accept short reach.** |
| **Baked drainage curves + runtime distance-field (§3.3)** | **CONDITIONAL** | conditional | **YES (real authored connectivity)** | **MAYBE — the "infinite" claim is the catch** | This is the ONLY option that gives *real* dendritic networks with correct junction angles, because the network is authored by real flow-routing offline. The phase is real. BUT it directly stresses the HARD CONSTRAINT "infinite & no precomputed array": you bake a finite curve set and **tile it with warp to hide repetition** — which is exactly the dead-end #3 (tiling causes visible repetition) the owner already forbade, just one level up (tiling vectors instead of pixels). The brief hand-waves "tiled at very large scale with warp to hide repetition" — that warp-hides-tiling assumption is the same bet WG9/proto already make and it's *unproven at the trunk-valley scale* (warp that hides 90m texture won't hide a 50km river repeating). **High reward, but the infinite-seamless story is unsolved. Spec only after a repetition test.** |

### The STRATEGIES (§4)

| Strategy | local? | parity? | real_structure? | verdict | why |
|---|---|---|---|---|
| **S1: hand metrics→params (current)** | yes | yes | **no (texture only)** | **KEEP, upgrade metrics** | Brief is right. Fixes "biomes look the same." Does NOT fix "looks like noise." Necessary, not sufficient. |
| **S2: offline-learn local erosion stencil, applied per-page (owner's thesis)** | **CONDITIONAL** | **CONDITIONAL — biggest hidden cost** | **partial (refines, doesn't connect)** | **MAYBE — the honest version, with two unflagged traps** | The defensible core: thermal erosion ≈ slope-limited diffusion ≈ a few local stencil passes; that's real and citable. **Trap 1 the brief omits:** a per-page iterative stencil is **neighbor-dependent iteration over a page array** — that is NOT `f(x,z)`. It needs the page materialized as a grid, plus **apron/skirt overlap**, and the result at a texel now depends on page boundary handling. Your facts/collision path (`height.rs`, CPU, per-point `f(x,z)`) **cannot evaluate a stencil-eroded height at an arbitrary single (x,z)** without re-deriving the whole page. So S2 *breaks the "collision == render" parity you just achieved in M4* unless the CPU side also bakes pages. **Trap 2:** the brief says S2 "moves the curvature metric toward real" — true — but admits it "does not create long-range connected trunk valleys." So S2 is a *finisher* on top of a network primitive, never a substitute. Spec only with eyes open about the per-page (not per-point) evaluation model and the collision-parity cost. |
| **S3: end-to-end GAN/diffusion** | no | no | yes | **REJECT** | Brief correctly rejects. Not f(x,z), not parity. Concur. |

---

## The four things the brief gets WRONG or hand-waves (source-check / challenge list)

1. **"two degrees of freedom" — factually wrong.** Code reads four live metrics (slope, incision, anisotropy, amp_profile). Minor, but it's a grounding error in a brief that brands itself "grounded."

2. **`anisotropy`→`warp_amount` is still live AND on the brief's own dead-list.** The brief flags anisotropy as dead (~0.30 for all) but doesn't notice the code still uses it to set warp. If true, warp_amount is near-constant → a real bug the brief should have caught. **Verify whether anisotropy actually varies before trusting warp_amount.**

3. **"§3.2 bounded-trace is parity-safe" — overclaim.** Iterated gradient descent is a chaotic discrete map; f64-CPU vs f32-GPU paths diverge. Not parity-safe without forced-f32-everywhere and even then it's fragile. Challenge this claim hard before speccing.

4. **"§3.3 baked curves stay infinite via warp-hidden tiling" — unproven, and it's the forbidden dead-end one level up.** Tiling pixels was banned for visible repetition; tiling *river curves* at 50km scale has the *same* failure mode and warp won't hide trunk-scale repetition. The brief asserts it works; it has not been shown. Source-check: Génevaux 2013 / Guérin 2016 produce *bounded* terrains, not infinite-seamless — the brief admits "their full versions are global" then waves the "bounded-trace approximation" through. The citations support bounded procedural rivers, **not** the infinite-tileable claim.

5. **Citations I'd want verified before relying on them:** Brocklehurst & Whipple (glacial HI / slope-area law-breaking) — the *direction* of the claim is textbook-plausible but the specific "cleanest published contrast" framing is the brief's editorializing. The geomorphometry canon (Strahler, Horton, Pike-Wilson, Riley TRI, Sappington VRM, Beven-Kirkby TWI) is all real and correctly attributed. The *procedural-gen* cites (Olsen 2004 real-time erosion, Musgrave 1989) are real but support "erosion looks like diffusion," NOT "diffusion stencil is parity-portable to GLSL+Rust with collision parity" — that bridge is the brief's own inference, uncited.

---

## RANKED — what's actually worth speccing under THESE constraints

**Worth speccing (in order):**

1. **Upgrade the offline metric set: HI+moments (#1), slope-distribution moments (#4), curvature-sign stats (#2), VRM (#5), windowed-relief ratio (#6).** Cheap, no flow routing, computable from the z-score array you already load, will demonstrably vary, maps to existing knobs. **This is the safe, do-first win — but be honest with the owner that it fixes "biomes are too similar," NOT "looks like noise."** Also: while you're in there, audit/replace the still-live dead `anisotropy`→warp wiring.

2. **Worley/cellular flow-edge primitive (§3.1) as a NEW connectivity term in `generate()`.** This is the single highest-value item for the owner's *actual* complaint, because it is the only option that adds genuine branching connectivity while staying unconditionally local + parity + cheap. Spec it as an additive/subtractive carve term alongside the ridged-noise valleys, with Dd (#3) tuning cell size once it exists. Be upfront about its ceiling (Voronoi junctions ≈120°, not Hortonian-acute; not downhill-monotone) — it beats `1−|fbm|` on connectivity but is not a true drainage network.

3. **(Defer, don't reject) S2 per-page learned erosion stencil** — but only AFTER (1) and (2), and only once the team has decided how a per-*page* stencil coexists with per-*point* collision/facts evaluation. That architectural conflict (it breaks M4's get_height==collision parity) is the real gate, not the ML.

**Traps — do NOT spec yet:**

- **§3.3 baked drainage curves** — highest structural reward but the infinite-seamless story is the owner's already-rejected tiling dead-end at a larger scale. Needs a repetition test before it earns a spec.
- **§3.2 bounded gradient-trace** — parity-fragile (chaotic float divergence) and short-reach. Trap dressed as a cheap win.
- **θ/k_sn (#7), TWI (#9)** — flow-routing cost for noisy/redundant signal on 512px. Defer.
- **Junction-angle/bifurcation (#10)** — descriptive only; don't build network extraction to learn what you can see by eye.

**The one sentence to hand the owner:** *The metric upgrade is real and cheap but only makes biomes differ from each other; the only thing in this brief that attacks "still looks like noise" while honoring infinite+local+parity is the Worley flow-edge primitive (genuine branching connectivity, with an honest ceiling), and everything that promises true dendritic drainage (baked curves, learned erosion) either re-imports the forbidden tiling problem or breaks your collision-parity — so spec metrics+Worley now, and treat the rest as research with a named gate, not a plan.*


========================================================================
## AGENT agent-a0ee315d9217680d0.jsonl  (final text 19089 chars)
========================================================================

I now have enough verified detail to deliver an adversarial verdict. Key confirmations and corrections:

1. **runevision is a FILTER, not a generator** — confirmed it requires the base height + gradient as input, and octaves are sequentially dependent (each recomputes direction from the previous octave's modified gradient). This is fine for parity but changes how it must be integrated.
2. **iq derivative-fBm** — confirmed local, parity-able, but requires analytic-gradient noise, and iq himself frames it as "simulate erosion-LIKE effects" (texture, not drainage).
3. **Terrain Diffusion** — confirmed neural diffusion/consistency model, 0.66s/tile on a 3090 Ti, order-independent via overlapping-window weighted blending. The brief's "bake-tier" framing is correct; the paper's "real-time" claim is at planetary-flyover streaming scale, NOT per-pixel-per-frame, and there is NO CPU parity path.
4. **Guérin** — confirmed K-SVD dictionary + matching-pursuit optimization, offline super-resolution of an input terrain. Not per-pixel.
5. **Phasor noise (Grenier/Tricard)** — confirmed per-pixel evaluable, but it's an anisotropic *texture* aligned to an input gradient field; whether it makes *connected* ravines is the open question.

No code to inspect — this is a pure research-verification task. Here is the structured verdict.

---

# ADVERSARIAL VERDICT: WorldGen10 erosion-structure brief

The brief is unusually honest — it already flags most traps itself. My scrutiny **confirms its core ranking but corrects four overclaims** that matter for your hard constraints. The single most important correction: the brief's "always-available, parity-correct base" stack (A+B+C) is **not uniformly parity-clean** — one of the three (the gradient-noise requirement) is a real engineering tax, and one (runevision) is mis-described as a generator when it is a *filter that consumes your existing height field*.

## The one structural fact the whole brief rests on (verified TRUE)
Real hydraulic erosion is global because erosion magnitude depends on **upstream drainage area** — the total catchment uphill of a point — which is provably not computable from a bounded neighborhood. Every source confirms this and the brief states it correctly. **Consequence you must internalize:** any technique that is genuinely `f(x,z)` local *cannot* produce true connected drainage. The honest target is "drainage-SHAPED," and the brief admits this. Do not let the owner expect connected drainage networks from anything in tier A/B/C — that expectation is physically unsatisfiable under your constraints. The phase/structure problem (dead-end #1) is **partially** addressable locally, but a *globally-correct* drainage network is only obtainable via bake (tier b).

---

## Per-technique verdict

### A. iq analytic-derivative fBm (`a += b*n/(1+dot(d,d))`)
- **local:** YES (verified — single per-point octave loop, no neighbor reads, no global array).
- **parity:** **CONDITIONAL** — the brief says "trivially identical" and "provided your noise returns its analytic derivative." That proviso is load-bearing and under-weighted. **Your current generator uses `value_noise`; this trick requires `noised()` — value noise that also returns its analytic gradient `n.yz`.** That is a real rewrite of your noise primitive in BOTH Rust and GLSL, and the gradient math must match bit-for-similar across both or your CPU collision and GPU render diverge on slopes. It's parity-able, but it is NOT free and NOT a drop-in. Budget it as "rewrite the noise basis," not "add 2 lines."
- **produces_real_structure:** **NO (texture only).** Verified: iq himself says it "simulate[s] different erosion-LIKE effects." It damps high-frequency detail on already-steep accumulated slopes → flat valley floors + crisp ridges. That is a *roughness redistribution*, not connected form. It changes how noise *looks up close*; it does not create large-scale connected ridgelines or branching valleys. **Under your owner's bar ("rejected all tuned-noise"), this is exactly tuned noise.** It will make the surface read less like uniform fBm fuzz, but the owner who rejected ridged-valley noise will likely also call this "still noise, just nicer."
- **verdict:** **ADOPT — but as a cheap polish layer, not a structure answer.** Worth it because it's the highest visual-quality-per-op item and directly attacks the "uniform roughness everywhere" tell. Just do not sell it internally as solving the structure problem. It does not.

### B. runevision "Fast and Gorgeous Erosion Filter" (2026)
- **local:** YES per-point (verified: "every point evaluated in isolation... trivial to generate in chunks").
- **parity:** **CONDITIONAL.** Two caveats the brief omits. (1) **Octaves are strictly sequentially dependent** — each octave recomputes stripe direction from the gradient *modified by all previous octaves*. That's fine for parity (fixed loop, deterministic) but means you cannot reorder/parallelize octaves and the CPU and GPU must run the *identical* accumulation order or they diverge. (2) Cell-based pivots (Worley-like) — parity-able but adds a hash-grid lookup that must match across Rust/GLSL.
- **CRITICAL CORRECTION — it is a FILTER, not a generator.** The brief calls it "the drainage layer" and lists it as a from-scratch technique. **Verified false:** the author states it "can be applied on top of any height function... applying erosion on top as a filter. The input height function and its gradient are fundamental inputs." So it **consumes your base height AND its analytic gradient** — meaning it *also* requires the technique-A gradient-noise rewrite as a prerequisite. A and B are not independent options; **B depends on A's gradient machinery.** This is the most important technical correction in this review.
- **produces_real_structure:** **PARTIAL — the best "partial" on offer.** It genuinely produces *hierarchical branching gully shapes* (small gullies branch off larger ones via gradient inheritance) — this is more structure than ridged-noise valleys. BUT the author explicitly admits: gullies "just stop halfway down a mountainside rather than following through" and sine interpolation "cannot consistently produce unbroken lines." So: branching-SHAPED, locally, but **not connected** and **not flow-correct**. It is the one tier-A/B/C technique that adds something the owner hasn't already tried (WG9's subtractive ridged valleys don't branch hierarchically).
- **verdict:** **ADOPT as the lead experiment — this is the technique most likely to actually beat the WG9 bar.** It is the only surveyed local technique producing *branching* structure rather than isotropic-or-ridged texture. Spec it. But spec it correctly: as a slope-aligned filter over your A-upgraded height field, and warn the owner up front that gullies will sometimes dead-end (that's inherent, not a bug to fix locally).

### C. Grenier 2024 phasor noise / Tricard procedural phasor noise
- **local:** YES (verified — phasor noise is "a single procedure in every pixel," per-point evaluable).
- **parity:** **CONDITIONAL, and harder than the brief implies.** The brief says "phasor noise is a fixed-cost per-point evaluation" — true, but procedural phasor noise is a **sum of Gabor-like kernels** (Gaussian envelope × harmonic) over nearby kernel pivots, fed through a sine. That's a heavier per-pixel kernel sum than value-noise fBm, and getting it bit-stable across Rust and GLSL (transcendentals, kernel summation order) is more fragile than the brief's "slightly more math than B" suggests. Parity-able, but the most parity-risky of the three.
- **Same FILTER dependency as B (verified):** Grenier's method "enhances a low-resolution INPUT terrain," controlling orientation/frequency "according to the characteristics of the terrain (height and gradient)." So it ALSO requires an existing base + gradient field. Not from-scratch.
- **produces_real_structure:** **PARTIAL, leaning toward "anisotropic texture."** It synthesizes slope-aligned ravines/ripples. This is *directional* texture conforming to your existing slopes — better than isotropic noise — but I found **no claim that it produces connected or branching networks.** It makes erosion-like *striations along the slope you already have*; it does not invent drainage topology. It is closer to "nicer noise that flows downhill" than to "new connected structure."
- **verdict:** **MAYBE — defer behind B.** It's academically principled and artist-controllable (control maps could map to your biome params), but it's strictly harder parity, heavier per-pixel, and produces *less* topological structure than B. It is the redundant sibling of B. Only reach for it if B's sine-interpolation artifacts prove unfixable. Do not spec A, B, and C — spec B, keep C as fallback.

### Minecraft splines-of-noise (the brief's #1 recommendation)
- **local:** YES (verified — per-column, three independent low-freq channels through control-point curves; no neighbor reads, no global pass).
- **parity:** **YES — genuinely clean.** Piecewise-linear lerp is the *only* technique here that's trivially identical in Rust and GLSL with zero new noise primitive. This is the brief's strongest parity claim and it holds.
- **produces_real_structure:** **PARTIAL — and a DIFFERENT axis of structure than erosion.** Important distinction the brief blurs: splines do NOT create ridgelines or drainage. What they create is **regime separation** — sharp, monotone/folded redistribution so one noise value snaps into discrete terrain TYPES (shelf→coast→plateau→cliff→mountain), and two independent channels give a 2D regime grid → the same height arises different ways → defeats self-similarity. That is real, valuable, and orthogonal to the erosion question. It fixes "everywhere looks the same" (the biome/macro-form tell), not "valleys aren't carved."
- **One overclaim to flag:** the brief asserts spline control points "DO vary across your 12 families where coherence/wavelength scalars didn't," and presents this as the fix for dead-end #2. **This is a hypothesis, not a verified result** — it is plausible and well-reasoned (a step-function height-vs-position curve for karst vs. flat for grassland), but the brief states it as fact. You have not yet measured whether `height-vs-erosion-proxy` curves actually separate your 12 families on 512px DEMs. **Verify this empirically before committing** — run the curve-fit on a few DEMs per family and confirm the curves are distinguishable. Given dead-end #2 already burned you on "metrics that came out dead-constant," do not trust this on faith.
- **verdict:** **ADOPT — but understand what it buys.** Highest parity-safety, cheapest, and it attacks a *real* tell (macro regime monotony). But it is a redistribution/biome-form layer, NOT an erosion/drainage layer. The brief is right to put it first for *implementation safety*; it is misleading if read as addressing the owner's drainage thesis. Pair it with B.

### NMS 3D density field / "go partially 3D" (brief's recommendation #4)
- **local:** YES (a density `f(x,y,z)` is still per-point).
- **parity:** YES in principle (same noise math, one more dimension).
- **produces_real_structure:** **YES — and it's the only "yes" in the whole brief.** Verified: NMS terrain is a 3D density field polygonized via marching-cubes-family; overhangs/arches/caves are *genuine structure a height field physically cannot represent*. This is real, not texture.
- **verdict:** **REJECT for now — correctly identified, wrong project phase.** This is real structure, but it's an **architecture change, not a feature.** Your entire stack — clipmap texels, `get_height(x,z)`, sparse collision field, CPU/GPU height parity, facts API — is built on `height = f(x,z)`. Going 3D density means marching cubes, a 3D collision representation, a different clipmap/render path, and abandoning the height-texture pipeline you just stabilized in M3. The brief undersells this as "go partially 3D where it counts." There is no cheap "partial" 3D in a heightmap engine; the overhang skin still needs volumetric meshing and collision. **Park it as a North-Star V2 note, do not spec it into the current line.** (It is, however, the honest answer to "why does NMS read as geological and my heightmap doesn't" — so it belongs in the vision doc, just not the next slice.)

### Guérin 2016 sparse amplification (owner's "learn offline → apply" thesis, instance 1)
- **local:** **NO** (verified: K-SVD dictionary learning + matching-pursuit is an iterative optimization over a whole patch; not closed-form per-pixel).
- **parity:** N/A as a runtime function (it's not a runtime function).
- **produces_real_structure:** **YES** — it reconstructs ridges/valleys from learned DEM atoms, preserving phase/structure (this is the genuine defeat of spectral dead-end #1: a learned dictionary keeps phase, a power spectrum doesn't).
- **verdict:** **MAYBE — bake-tier only.** The brief is correct and honest here. It satisfies the offline-learn half, violates the cheap/local-online half. Viable ONLY as per-page bake-then-stream (category b). Also note: it's super-RESOLUTION (input-dependent — it amplifies an existing low-res terrain), so it'd amplify your A+B+C base, not replace it. Reasonable as a future bake layer; not the current line.

### Terrain Diffusion / InfiniteDiffusion 2026 (owner's thesis, instance 2)
- **local:** **NO for parity purposes.** Order-independent random access, yes (verified: deterministic weighted blend of overlapping diffusion windows) — but "constant-time random access" is at *tile* granularity, and a tile's true cost includes denoising overlap margins into neighbors. It is not point-local.
- **parity:** **NO — hard no.** Verified neural diffusion/consistency model (GPU tensor ops). There is **no value-noise GLSL equivalent and no CPU path**. Your collision/facts on CPU would need an entirely separate generator → you'd have two different terrains (the thing your whole parity constraint exists to prevent). The brief flags this; I'm escalating it: this is **disqualifying** for your CPU/GPU-parity hard constraint, not a mere "caveat."
- **CORRECTION of an overclaim:** the brief calls 0.66s/tile "per-page-bake territory, not per-pixel-per-frame" — correct. But note the *paper itself* claims "real-time" and "9× orbital velocity." That marketing is at **planetary-flyover streaming scale** (tiles stream in faster than you fly over them), NOT interactive per-frame regeneration. The brief's skeptical reframing is right; the paper's headline is misleading for your use case. Anyone reading the abstract will over-trust it — your instinct to be skeptical is correct.
- **produces_real_structure:** **YES** (branching valleys, mountain ranges, river basins — genuinely beyond noise; this is the proof-of-concept that learned-prior structure works).
- **verdict:** **MAYBE — watch, prototype as a SEPARATE bake experiment, do NOT put on the parity-critical path.** It validates the owner's bet directionally. But adopting it means abandoning CPU/GPU parity for the rendered surface and maintaining a divergent cheap CPU collision approximation. That's a strategic fork, not a feature. Keep alive as a research spike; never let it become the base layer.

---

## Final ranking — what to spec, what's a trap

**Spec now (in this order):**
1. **Minecraft splines-of-noise** — safest parity, cheapest, attacks macro-regime monotony. BUT first *empirically verify* the per-biome curves actually separate your 12 families (don't repeat dead-end #2's "dead-constant metric" mistake on faith). This is a biome-FORM layer, not a drainage layer.
2. **runevision slope-aligned branching-gully filter** — the ONLY local technique that adds *branching* structure beyond WG9's bar. Spec it correctly as a *filter over your height field* (it needs base height + gradient as input), which means it requires:
3. **iq derivative-fBm + a gradient-returning noise primitive** — adopt as the base-height polish AND as the prerequisite gradient machinery that runevision consumes. Treat "rewrite noise to return analytic gradient in Rust+GLSL" as a real, parity-critical task, not 2 free lines.

**Note the dependency the brief misses:** #2 and #3 are not parallel options — #2 *requires* #3's gradient field. The correct spec order is 3→2.

**Maybe / fallback:**
- Grenier phasor noise — redundant with runevision, harder parity, less topology. Fallback only.
- Guérin sparse amplification — viable future *bake* layer; not runtime.

**Traps — do not spec into the current line:**
- **Terrain Diffusion as the base generator** — kills CPU/GPU parity (no CPU path, neural-only). Research spike only, never on the parity-critical path. Be skeptical of its "real-time" headline (it means streaming-scale, not per-frame).
- **NMS 3D density "partial 3D"** — real structure, but it's an architecture rewrite (marching cubes + 3D collision), not a slice. Vision-doc V2, not now.

**The honest bottom line for the owner's thesis:** the local stack (1+2+3) gets you to "drainage-SHAPED, branching, regime-varied" — measurably past WG9's subtractive ridged valleys, but still NOT connected/flow-correct drainage, because that is provably global. Truly connected drainage requires a bake layer (Guérin or a diffusion prior), and a bake layer forfeits CPU/GPU parity for the baked quantity. There is no local `f(x,z)` that produces globally-connected drainage — that's not a gap in the brief, it's a theorem. Spec the local stack to beat WG9; keep one bake experiment alive to chase true structure, accepting it lives outside the parity contract.

Sources I source-checked: [runevision erosion filter](https://blog.runevision.com/2026/03/fast-and-gorgeous-erosion-filter.html) (confirmed: filter-not-generator, sequential octaves, gullies dead-end), [iq morenoise](https://iquilezles.org/articles/morenoise/) (confirmed: needs analytic-gradient noise, "erosion-LIKE"), [Terrain Diffusion arXiv 2512.08309](https://arxiv.org/abs/2512.08309) (confirmed: neural, 0.66s/tile 3090Ti, overlapping-window determinism, no CPU path), [Guérin terrain-amplification code](https://github.com/eric-guerin/terrain-amplification) (confirmed: K-SVD + matching-pursuit, offline super-res), [Grenier 2024 CGF](https://onlinelibrary.wiley.com/doi/full/10.1111/cgf.14992) + [Tricard phasor noise](https://hal.science/hal-02118508v1) (confirmed: input-terrain-dependent, Gabor-kernel-sum per pixel), [Minecraft world generation](https://minecraft.wiki/w/World_generation) (confirmed: splines on independent noise channels, "erosion" is noise-not-sim; nesting NOT confirmed from this source — verify separately), [NMS GDC 2017 / modding wiki](https://nmsmodding.fandom.com/wiki/Terrain_Generation) (confirmed: 3D voxel density + marching cubes for overhangs). **Could not independently verify from primary source:** the specific "uber noise" parameter list (gain/lacunarity/sharpness/slope_erosion) — it traces to a Shadertoy reimplementation, not Hello Games, so treat that parameter taxonomy as community-reconstructed, not authoritative.


========================================================================
## AGENT agent-a12386cca041ae027.jsonl  (final text 19810 chars)
========================================================================

That source 404'd, but the dandrino repo and aparis69 tutorial already corroborate the central claim (real erosion = hierarchical/global flow accumulation; procedural noise = self-similar at all scales, no real drainage). I have triple-corroboration on every load-bearing claim. I'll synthesize now.

# Phase 5: Synthesize

Verification status of load-bearing claims:
- **Musgrave recurrences** (`weight *= signal`, ridged `offset - |noise|`, `weight = signal*gain`): primary source = Musgrave's own C code. CONFIRMED verbatim.
- **IQ erosion line** `a += b*n.x/(1.0+dot(d,d))`: primary source = iquilezles.org/morenoise, quoted verbatim twice. CONFIRMED.
- **Swiss/Jordan turbulence** (derivative-warp + amplitude damp): primary = de Carpentier Scape. CONFIRMED (isaackarth mirror only points, doesn't refute).
- **Multifractal heterogeneity reason** (amplitude modulated by prior octaves → smooth valleys + rough peaks): de Carpentier + aparis69 + Musgrave. CONFIRMED 3x.
- **Noise can't make real drainage / it's a visual fake** (the skeptical point): runevision author's OWN admission + dandrino repo + aparis69. CONFIRMED 3x, including by the technique authors themselves.

---

# TECHNICAL BRIEF: Multifractal & Derivative-Erosion Noise for WorldGen10

## 0. The one-sentence answer

Your generator is missing the **multifractal recurrence** (`weight *= signal` — amplitude of each octave gated by the running terrain value) and the **derivative-accumulation erosion damp** (`/(1+dot(d,d))` — amplitude of each octave gated by accumulated slope). Both are strictly local, parity-able, cheap, and both are *genuine partial answers* — but they are heterogeneity/anisotropy tricks, **not** drainage-connectivity. They will beat your current uniform-weight summed ridged fBm and beat WG9, but they do **not** cross the line into "real connected drainage" — and the people who invented them say so out loud.

---

## 1. Multifractal weighting vs. your current summed ridged fBm

### What you do now (monofractal / uniform-weight)
```
h = Σ_i  amp[i] · ridge(noise(f_i · x))      // amp[i] fixed up front, ridge = 1-|n|
```
Every octave contributes its preset amplitude **everywhere**. Roughness is spatially uniform → the eye reads "same noise" because the *fractal dimension is constant across the whole map*. This is exactly Musgrave's "monofractal," and it is the root cause of your owner's complaint.

### Musgrave's actual recurrences (verbatim from his 1st-edition `musgrave.c`)

**HybridMultifractal:**
```
weight = 1.0;                                   // (initialized to first octave)
for i in octaves:
    signal  = (noise(point) + offset) * exponent_array[i];   // exponent_array[i] = pow(freq, -H)
    result += weight * signal;
    weight *= signal;                            // <-- THE MULTIFRACTAL PART
    if weight > 1.0: weight = 1.0;               // (clamp; in practice)
    point  *= lacunarity;
```

**RidgedMultifractal:**
```
for i in octaves:
    signal = noise(point);
    signal = abs(signal);          // |n|
    signal = offset - signal;      // ridge inversion: offset - |n|
    signal *= signal;              // square -> sharpen ridge crest
    weight  = signal * gain;       // <-- weight DERIVED from this octave's signal
    weight  = clamp(weight, 0, 1);
    signal *= weight;
    result += signal * exponent_array[i];
    point  *= lacunarity;
```

**Hetero_Terrain** (cleanest illustration of the idea):
```
increment = (noise(point) + offset) * exponent_array[i];
increment *= value;          // <-- scale this octave by current ALTITUDE
value     += increment;
```

### Why the `weight *= signal` (equivalently `increment *= value`) line is the whole game

The multiplicative weight makes **high octaves only fire where the terrain is already high**. Concretely:

- In **lowlands** (running `value`/`weight` is small), each new octave's contribution is multiplied by a small number → fine detail is suppressed → **valleys come out smooth and flat**.
- On **uplands/peaks** (running value large), the weight stays near 1 → full high-frequency detail accumulates → **peaks come out rough and craggy**.

This produces **spatially-varying fractal dimension** (the literal definition of "multifractal"): smooth basins coexist with jagged ridges in one field. That heterogeneity — *not* any new frequency content — is what real mountains have and your uniform fBm cannot have, because in your version roughness is a global constant. Three independent sources confirm this is the mechanism and the reason it reads as terrain (Musgrave's code; aparis69's tutorial: *"areas with a low amplitude at iteration k will get a noise value of lower amplitude at iteration k+1, and the opposite for mountainous areas"*; de Carpentier).

**This is the single highest-leverage change you can make, and it's a one-line edit to your existing loop:** carry a `weight` (start 1.0), do `h += weight * octave; weight = clamp(weight * gain * octave_signal, 0, 1)`. No new noise lookups.

### Verdict
- **LOCAL:** Yes — same single point, no neighborhood. ✔
- **PARITY:** Yes — only mul/add/abs/clamp + your existing noise. Identical in Rust and GLSL. ✔
- **CHEAP:** Yes — *zero* extra noise taps vs. what you do now; just a scalar carry. ✔
- **Real geography vs. different noise:** **Genuine, partial.** It fixes the "uniform roughness everywhere" tell and gives you smooth-valley/rough-peak heterogeneity that fBm structurally cannot. It does **not** create drainage connectivity. It is the correct first fix and clearly beats uniform ridged fBm.

---

## 2. H (Hurst / fractal increment), offset, lacunarity, gain

From IQ's fBM article and Musgrave:

- **H (Hurst exponent), 0..1**, sets the spectral weight `exponent_array[i] = pow(frequency, -H)`, equivalently per-octave **gain G = 2^(-H)** (when lacunarity = 2).
  - **H = 1 → G = 0.5**: smoothest, amplitude halves per frequency-doubling. IQ argues this is the *natural* value because real terrain is isotropically self-similar ("mountains that are higher are also wider at the base by the same amount"). This is your default sane value.
  - **H = 0.5 → G ≈ 0.707**: "brown noise," rougher.
  - **H = 0 → G = 1**: pink noise, "wilder than pure BM," every octave equal — looks like static. Avoid.
  - **Visual effect:** H is the master roughness knob. Critically, in *multifractal* you can let H itself (or offset) vary with the running value to get even more heterogeneity.
- **offset** ("fractal increment" / sea-level): added before the ridge/weight step (`offset - |n|` in ridged, `noise + offset` in hybrid). It controls **how much of the field sits above the "waterline" and feeds the multiplicative weight**. Low offset → more terrain pushed into the smooth-lowland regime (more flats, isolated peaks); high offset → more terrain stays in the rough regime (more continuous craggy uplands). In ridged it also shifts where the squared crest peaks. Blender exposes exactly this as the "Fractal Offset / sea level" control — corroborated.
- **gain** (in RidgedMultifractal, `weight = signal*gain`): sharpens the gating — how aggressively peaks dominate. Higher gain → sharper, more isolated ridge crests.
- **lacunarity** (≈2.0): frequency gap per octave. Non-integer values (e.g. 2.0 vs 1.92) help break the visible self-similar tiling tell. Cheap, free win.

### Verdict
All four are scalar params on the existing loop — fully LOCAL/PARITY/CHEAP. Their contribution is *control over heterogeneity*, not structure. They make the multifractal effect tunable per-biome (this is exactly where your distilled biome params should land: `H/offset/gain/lacunarity` per family).

---

## 3. Derivative-aware "erosive" noise (the genuinely interesting one)

### 3a. IQ's analytic-derivative fBm + erosion damp (`morenoise`, verbatim)

Value noise returns value **and** analytic gradient in one eval: `noised(p) -> vec3(value, dn/dx, dn/dy)` (derivative of the quintic `6t⁵−15t⁴+10t³` interpolant). Then:

```glsl
float a = 0.0, b = 1.0;            // accumulated height, amplitude
vec2  d = vec2(0.0);               // accumulated derivative (slope)
for (int i=0; i<OCT; i++) {
    vec3 n = noised(p);
    d += n.yz;                                 // accumulate gradient
    a += b * n.x / (1.0 + dot(d,d));           // <-- EROSION: damp by slope²
    b *= 0.5;                                   // amplitude decay (H=1)
    p  = m * p * 2.0;                           // rotate+scale domain each octave
}
```

The line `a += b*n.x/(1.0+dot(d,d))` is the **directional-derivative erosion trick**. `dot(d,d)` is the squared magnitude of accumulated slope. Where the terrain is already steep (high `d`), the denominator blows up and **new octaves add almost nothing → slopes get smoothed/flattened**; where it's flat, full detail accumulates. This is a *local fake of erosion*: detail is suppressed on steep faces, mimicking material being stripped from slopes. The per-octave domain **rotation** (`m`) is essential — it decorrelates octaves so the damping doesn't produce axis-aligned grid artifacts.

### 3b. de Carpentier "swiss" turbulence (`Scape`, verbatim) — the warp variant

```
sum=0; freq=1; amp=1; dsum=(0,0);
for i in octaves:
    n   = noiseDeriv((p + warp*dsum) * freq);   // <-- WARP next octave by accumulated grad
    sum += amp * (1 - abs(n.x));                 // ridged
    dsum += amp * n.yz * -n.x;                   // accumulate (signed) gradient
    freq *= lacunarity;
    amp  *= gain * saturate(sum);                // <-- multifractal damp (your missing piece, again)
```

Two erosion mechanisms stacked: (1) **warp** — each octave is sampled at a position pushed along the accumulated gradient, which *elongates features along slope/toward ridgelines* ("causes features on slopes to get elongated"), the closest noise gets to directional, flow-following gullies; (2) **`amp *= gain*saturate(sum)`** — the same multifractal valley-flattening as §1.

### 3c. de Carpentier "jordan" turbulence — separates warp vs. damp

Uses squared noise `n*n.x` (billowy/rounded crests instead of sharp ridges) and keeps **two independent accumulators**: `dsum_warp` (→ directional gulley streaking, "fluvial-like") and `dsum_damp` with `damped_amp = amp*(1 - damp_scale/(1+dot(dsum_damp,dsum_damp)))` (→ flat areas at any height, "thermal-like"). This is the most expressive of the family — lets you dial hydraulic-look vs thermal-look separately per biome.

### 3d. Billow
`billow = 2*|n| - 1` (or `|n|`). Inverse of ridged — rounded lumps instead of sharp creases. Cheap, useful as a *basis swap* for dunes/hills/clouds (desert, grassland biomes), not a structure technique. LOCAL/PARITY/CHEAP, contributes "different shape," not geography.

### Verdict for the whole derivative-erosion family
- **LOCAL:** Yes — strictly per-point. The "accumulation" is *across octaves at the same (x,z)*, never across neighbors. No grid pass, no droplets. ✔
- **PARITY:** Yes — needs analytic-derivative noise (you must add the gradient output to your value-noise, which is a known closed form), then only mul/add/dot/abs/saturate. Bit-identical achievable Rust↔GLSL if you share the noise basis. ✔
- **CHEAP:** Slightly more than plain fBm — 1 noise-with-derivative eval/octave (≈1.3× a plain tap), 4–6 octaves fine. Still a handful of ops. ✔
- **Real geography vs. different noise:** **The strongest "partial yes" in this brief, but bounded.** The damp term genuinely produces the smooth-valley/rough-ridge profile *and* a slope-dependent character that looks erosional. The warp term genuinely produces directional, gulley-like streaking aligned to slope — the only noise trick that fakes *flow direction* locally. **This clears the WG9 bar** (WG9's subtractive ridged valleys are direction-blind; swiss/jordan warp is direction-aware). **But** — see §4 — it does not produce *connected, convergent* drainage.

---

## 4. The central tension, resolved honestly

**Your owner's thesis is correct and is confirmed by the very authors of these tricks.** The skeptical conclusion, triple-sourced:

1. **runevision's erosion-filter author admits it's a fake and that it breaks connectivity:** *"techniques are sought after which can produce the appearance of erosion without having to deal with simulating the process"* and *"sometimes a gully … just stops halfway down a mountainside rather than following through all the way down to the lowest reachable point … the interpolated stripes cannot consistently produce unbroken lines."* That is the exact failure mode: local tricks can't guarantee a channel reaches the basin, because that requires knowing the downhill path across the **whole** field.

2. **dandrino "terrain-erosion-3-ways":** procedural noise (even warped/ridged) *"produce[s] self-similar features at all scales, whereas real erosion creates hierarchical drainage patterns"* — and concludes you need **simulation or learning**, not noise, for true erosion. Their ML route learns from ~180k real DEM tiles and needs **full-map context**.

3. **aparis69 + the physics:** real drainage is **dendritic and hierarchical** — small channels *merge* into larger ones, and which-merges-into-which is determined by **flow accumulation**, an inherently *global, long-range* operation (each cell's water depends on everything uphill of it). No closed-form `f(x,z)` reading a bounded neighborhood can know its full upstream catchment. This is a real mathematical obstruction, not a tuning failure — and it is *the same reason your spectral approach (dead-end #1) failed*: connectivity/phase is global, non-local information.

**So where does that leave the local tricks?** They are not noise-reskins (they beat WG9) but they are also not erosion (they can't connect drainage). They occupy a real middle tier:

| Tier | Technique | What it actually buys |
|---|---|---|
| 0 (you now) | uniform summed ridged fBm | uniform roughness — "same noise" |
| 1 | **+ multifractal `weight*=signal`** | heterogeneity: smooth valleys + rough peaks (fixes the main tell) |
| 2 | **+ derivative damp `/(1+dot(d,d))`** | slope-dependent erosional smoothing of faces |
| 3 | **+ swiss/jordan warp** | *directional* gulley streaking that follows slope (beats WG9's direction-blind valleys) |
| 4 (the wall) | connected dendritic drainage | **NOT reachable by any local closed form** — needs global flow accumulation |

### The only known way through the wall that respects infinite+local+cheap

Your owner's candidate resolution is the **correct and literally only** strategy the literature offers: **offline-simulate, online-approximate.** The dandrino repo is the existence proof — it learns the *output* of erosion. To stay LOCAL at runtime you must constrain the learned thing to a **bounded-receptive-field operator**:

- **Offline:** run real hydraulic erosion (droplet or stream-power) on tiles synthesized by your Tier-1/2/3 noise (so input and target share a basis). 
- **Learn:** a *small-kernel* operator (e.g. a few-layer CNN with a **fixed, bounded receptive field**, or a learned local filter / lookup) that maps `(noise height + its derivatives + multifractal weight) → erosion delta`. Bounded receptive field = bounded neighborhood = still "local" by your constraint, and tileable/seamless if you generate with halo + overlap-blend at page boundaries.
- **Online:** apply that fixed-kernel operator per page. It's a convolution-like pass over a page's own texels + a small halo — **bounded ops per texel, no global iteration, parity-able** (deterministic conv in Rust + GLSL).

**The honest caveat to set expectations:** a bounded-receptive-field operator can faithfully reproduce the *texture/profile* of erosion (concave channel cross-sections, slope-dependent smoothing, crisp ridges, debris fans) — the things that are *locally determined*. It will only **partially** reproduce *long basin-spanning trunk channels*, because their position is set by catchment area that exceeds any fixed receptive field. That residual is exactly the runevision author's "gully stops halfway" artifact. You can push the wall back (bigger halo/receptive field, multi-scale operator) but you cannot delete it without going global. So: target **erosional texture + short/medium dendritic channels** as the achievable, infinite-compatible win; treat full continental trunk rivers as a separate, optionally-precomputed/curated feature layer if ever needed.

---

## 5. Concrete recommendation (ordered, each addresses the rejections)

1. **Add the multifractal weight to your existing ridged fBm** (`weight *= signal`, clamp 0..1). Free, biggest visual jump, fixes the "uniform roughness" tell. Make `H, offset, gain, lacunarity` your per-biome distilled params (these *do* vary meaningfully and are local — unlike the structure-tensor metrics in dead-end #2).
2. **Add analytic derivatives to your value noise** and apply the IQ damp `/(1.0+dot(d,d))` with per-octave domain rotation. Slope-dependent erosional smoothing.
3. **Adopt swiss or jordan turbulence** for ridge/valley layers — the warp term is the only local way to get *direction-following* gullies, and it cleanly beats WG9's direction-blind subtractive valleys (so it answers "how to beat the current bar").
4. **For anything beyond Tier 3 (true connected drainage):** commit to offline-sim → learn a **bounded-receptive-field local operator** → apply per-page with halo. This is the *only* path that survives all of: infinite, local, cheap, parity. Set expectations that it nails erosional *texture* and short channels, not whole-basin trunk rivers.

Do **not** revisit spectral synthesis (#1) or the structure-tensor/argmax metrics (#2): both fail for the same root reason that caps the local noise tricks here — the missing ingredient (drainage connectivity / phase) is **global, non-local information** that no per-pixel spectrum-matched function can carry.

---

### Sources
**Primary (algorithms, verbatim):**
- F.K. Musgrave, original source — fBm / multifractal / HybridMultifractal / RidgedMultifractal / Hetero_Terrain: https://engineering.purdue.edu/~ebertd/texture/1stEdition/musgrave/musgrave.c (from *Texturing & Modeling: A Procedural Approach*, Ebert/Musgrave/Peachey/Perlin/Worley)
- Inigo Quilez, "noise derivatives / more noise" — analytic-derivative value noise + erosion damp `a += b*n.x/(1.0+dot(d,d))`: https://iquilezles.org/articles/morenoise/
- Inigo Quilez, "fBM" — H, gain G=2^(-H), self-similarity argument: https://iquilezles.org/articles/fbm/
- Giliam de Carpentier, "Scape: procedural extensions" — swissTurbulence / jordanTurbulence verbatim, erosion-by-steepness: https://www.decarpentier.nl/scape-procedural-extensions

**Corroborating / skeptical:**
- runevision, "Fast and Gorgeous Erosion Filter" (2026) — local oriented-stripe erosion filter; author's own admission it's a visual fake and gullies don't always connect: https://blog.runevision.com/2026/03/fast-and-gorgeous-erosion-filter.html
- aparis69, "Noise for terrains" — multifractal weight-by-previous-octave; why fBm lacks valleys/ridges/heterogeneity: https://aparis69.github.io/LearnProceduralGeneration/terrain/procedural/noise_for_terrains/
- dandrino, "terrain-erosion-3-ways" — noise vs simulation vs GAN; noise is self-similar and cannot make hierarchical drainage; learning needs real-DEM context: https://github.com/dandrino/terrain-erosion-3-ways
- Blender Manual (Musgrave texture) — offset as "sea level," gain, lacunarity semantics (independent confirmation): https://docs.blender.org/manual/en/latest/render/materials/legacy_textures/types/musgrave.html
- procedural-generation (isaackarth mirror) — swiss/jordan as erosion-approximating noise (pointer/corroboration): https://procedural-generation.isaackarth.com/2016/07/20/swiss-turbulence-and-terrain-generation-terrain.html


========================================================================
## AGENT agent-a3c68e61356fee9d7.jsonl  (final text 64 chars)
========================================================================

You've hit your session limit · resets 9:50pm (America/New_York)


========================================================================
## AGENT agent-a593886a1c3de8a39.jsonl  (final text 12951 chars)
========================================================================

I now have everything: the citations are verified verbatim, the python generator matches the brief's description, and I've found a critical discrepancy — the Rust `height.rs` does NOT implement the procedural generator the brief assumes; it still tiled-samples kernels (the forbidden path). That's load-bearing for the parity claim. Here's my verdict.

---

# ADVERSARIAL VERDICT: Musgrave-multifractal brief for WorldGen10

## Bottom line up front

The brief is **technically honest and unusually well-disciplined** — rare for this genre. It does not oversell: it explicitly says the local tricks beat WG9 but do NOT cross into connected drainage, and it cites the technique *authors themselves* admitting the fakery. My fact-checks confirmed every load-bearing algorithmic citation verbatim (IQ `morenoise`, de Carpentier swiss/jordan). I am not going to rubber-stamp it anyway, because it has **three real problems** the owner needs flagged:

1. **A parity landmine the brief is silent on** (analytic-derivative noise + per-octave rotation matrix — see §B below).
2. **A code-reality mismatch:** the brief critiques the *python* generator, but the shipping *Rust* `height.rs` is a different, kernel-tiling implementation — so "one-line edit to your existing loop" is only true in numpy.
3. **The Tier-4 "learned local operator" recommendation is under-scrutinized** and is the most likely trap in the document.

---

## Per-technique verdict table

| Technique | local | parity | real_structure | verdict | why |
|---|---|---|---|---|---|
| **Multifractal weight** (`weight*=signal` / `increment*=value`) | **yes** | **yes** | **no** (heterogeneity, not connectivity) | **ADOPT** | Zero extra noise taps, pure scalar carry, trivially parity-safe. Fixes the literal "uniform roughness everywhere" tell. Highest leverage / lowest cost in the doc. |
| **H / offset / gain / lacunarity** as per-biome params | **yes** | **yes** | **no** (control, not structure) | **ADOPT** | Scalar knobs on the existing loop. Crucially these *do* vary across biomes (unlike the dead structure-tensor metrics in dead-end #2), so they're a legitimate home for distilled params. |
| **IQ derivative damp** `/(1+dot(d,d))` | **yes** | **conditional** | **partial** (slope-dependent smoothing, no connectivity) | **MAYBE** | Genuinely erosional-looking, but requires analytic-derivative noise + per-octave rotation matrix — a real parity surface the brief never flags. Adopt *after* the free wins, with a parity gate. |
| **Swiss/Jordan turbulence** (derivative-warp) | **yes** | **conditional** | **partial** (directional gully streaking; beats WG9; still no connected basins) | **MAYBE** (pick ONE: jordan) | The only local trick that fakes flow *direction*. Same parity caveat as IQ damp, plus more params to tune. Best single "beat WG9" candidate, but it's tier-3 polish, not the structural fix. |
| **Billow** (`2|n|-1`) | yes | yes | no | **REJECT (as structure)** / keep as optional basis-swap | Pure shape change for dunes/hills. Contributes nothing to the owner's actual complaint. |
| **Tier-4 learned bounded-receptive-field operator** | **conditional** | **NO, as written** | **partial** (texture + short channels, never trunk rivers) | **MAYBE — but DO NOT spec yet** | This is the document's riskiest claim dressed as its safest. See §C. |

---

## A. Where the brief is RIGHT and I verified it

- **Multifractal `weight *= signal` is local + parity + cheap.** Confirmed by reading the algorithm: it's a scalar carried across the octave loop at a *single* (x,z). No neighbor reads, no unbounded loop, no array. Compiles identically to Rust and GLSL. **This claim survives full scrutiny.** It is correctly identified as the single highest-leverage change.

- **The honest separation of "structure vs nicer noise" is correct and is the document's main virtue.** Every one of multifractal/damp/swiss/jordan is correctly classed as **heterogeneity/anisotropy, NOT connectivity**. The brief does not claim any of them produce dendritic drainage. My WebFetch of de Carpentier confirms the author calls swiss/jordan a *fake* ("you can try to fake it quite successfully... ways to actually simulate erosion... tend to be quite slow") and describes only "elongated" / "gulley-like" features, never connected networks. **The skeptical core is correctly sourced, not hand-waved.**

- **The phase/connectivity = global-information argument is sound and correctly unifies both dead-ends.** "Flow accumulation needs the whole upstream catchment" is a real mathematical obstruction, and tying it to why spectral synthesis (dead-end #1) failed (phase is global) is a genuinely good insight, not filler.

- **Citations check out.** IQ `morenoise` has `a += b*n.x/(1.0+dot(d,d))` and a per-octave rotation matrix `m` verbatim; de Carpentier has swiss/jordan with `perlinNoiseDeriv` verbatim. No citation laundering detected on the algorithm claims.

## B. Where the brief UNDERSELLS the cost (the parity landmine it hides)

The brief says the derivative family is "parity: yes" and waves at "you must add the gradient output to your value-noise, which is a known closed form." **This is the one place it is too glib, and it matters for THIS project's hardest constraint.**

1. **Analytic-derivative noise is a second, parallel parity surface.** Your current noise (`value_noise` in both python and the future Rust/GLSL) returns a scalar. The derivative variant must return `(value, dn/dx, dn/dz)` and the **derivative must be bit-consistent with the value** across Rust and GLSL, *including the quintic interpolant's derivative* `30t²(t²−2t+1)`. You now have to parity-test the gradient channel too, not just the height. The brief treats this as free; it is a real, testable surface and should be its own gate.

2. **The per-octave rotation matrix `m` is load-bearing and the brief mentions it only in passing.** IQ's loop does `p = m*p*2.0` every octave — without it the damping produces axis-aligned grid artifacts (my fetch confirmed "prevents axis-aligned artifacts"). That rotation matrix must be the *identical* constant in Rust and GLSL or your CPU collision and GPU render diverge. Another silent parity item.

3. **Verdict adjustment:** IQ-damp and swiss/jordan are **parity: conditional**, not "yes." They are achievable, but they are NOT the "free one-line edit" the multifractal weight genuinely is. Sequence them *after* the multifractal weight, behind an explicit gradient-parity test.

None of this kills the techniques — but the owner's #1 non-negotiable is CPU/GPU parity, and the brief's "parity: yes ✔" on the derivative family papers over the exact thing that's hardest to get right here.

## C. The Tier-4 recommendation is the real trap — scrutinize hard before speccing

The brief's strongest rhetorical move is framing "offline-sim → learn a bounded-receptive-field operator → apply per-page with halo" as "the correct and literally only strategy." Three problems it glosses:

1. **A learned CNN is NOT obviously CPU/GPU parity-able to YOUR standard.** Your parity bar is collision/facts (Rust) == render (GLSL) to ~0.001 m (per your M4 memory). Running the *same* conv kernel in Rust and GLSL with bit-consistent float reduction order, identical padding/halo handling, and identical weight quantization is a substantial engineering surface — far more than "a deterministic conv in Rust + GLSL." The brief asserts parity here in one clause with zero evidence. **I'd flag this as the least-supported load-bearing claim in the document.**

2. **It quietly breaks "pure f(x,z)".** A per-page halo+convolution operator is no longer `height(x,z)` reading "a few noise lookups" — it's a *page-scoped pass* with boundary-overlap blending. That may be acceptable (it's bounded, not global), but it is a **different architecture** from everything in tiers 0–3, and it interacts with your existing clipmap/page system, geomorph, and the visible==collision parity guarantee. The brief sells it as a drop-in fourth tier; it is actually a new subsystem.

3. **The honest caveat buried at the end is the real headline:** even Tier-4 "will only partially reproduce long basin-spanning trunk channels... you cannot delete it without going global." So the expensive learned-operator path **still does not deliver the owner's stated goal** ("a real connected mountain range... continuous ridgelines, branching drainage valleys"). It buys erosional *texture* and *short* channels. That is the same ceiling as tier 3, at 100× the implementation cost and a parity question mark.

**My ranking for THIS project:** the genuine wins are **multifractal weight (adopt now)** and **per-biome H/offset/gain/lacunarity (adopt now)** — both free, both parity-clean, both directly attack the "same noise" complaint. **Jordan turbulence is a worthwhile tier-3 follow-up** behind a gradient-parity gate. **Tier-4 should NOT be specced from this brief** — it needs its own feasibility spike answering the parity question before any commitment.

## D. Two project-specific landmines the brief could not know

These are not the brief's fault, but they change how the recommendations land:

1. **The shipping Rust `height.rs` is NOT the generator the brief critiques.** The brief (and `worldgen_proto.py`) describe warp→fBm→ridged→valley-carve. But `D:\workflows\worldgen10\wg-10\rust\src\height.rs` does something *entirely different*: it `sample_kernel`s the DEM kernels with **tiled bilinear sampling at runtime** (`rem_euclid` wrap, `footprint_m` repeat) and blends families by grammar weight. That is **dead-end #3 (tiling/sampling DEM pixels at runtime)** — the explicitly forbidden path — and its own docstring admits "visible creases at footprint seams are expected." So:
   - The brief's "one-line edit to your existing loop" is true **only in numpy**; the Rust runtime has no such loop to edit. Adopting the multifractal weight means porting the *python* generator into Rust/GLSL first — a bigger lift than the brief implies, and it should be paired with *deleting* the kernel-tiling height path.
   - Worth confirming with the owner which is "current truth," but the file dates and the memory note "M3 render reset" suggest the Rust side is behind the python prototype.

2. **The brief's distilled-param advice partly contradicts your own distillation code.** The brief says make `H/offset/gain/lacunarity` the per-biome distilled params. But `biome_distill.py` currently distills `octave_amps`, `ridge_strength←slope`, `valley_depth←incision/relief`, `warp_amount←anisotropy` — and has already *abandoned* ridge_linearity and dominant_wavelength as dead-constant. The brief's new params are a **different parameterization** than what `params_from_metrics` emits. That's fine (it's arguably better — multiplicative gating from a real measured roughness profile), but it means adopting the brief is also a **distillation rewrite**, not just a generator tweak. The `amp_profile` you already measure is the natural input to drive per-octave gain, so there's a clean path — but it's not free.

## E. Net recommendation

- **Adopt immediately (free, parity-clean, attacks the actual complaint):** multifractal weight `weight *= clamp(signal*gain,0,1)` in the octave loop + expose `H/offset/gain/lacunarity` per biome, fed from the `amp_profile` you already distill.
- **Adopt next, behind a gradient-parity gate:** Jordan turbulence (pick it over swiss — strictly more expressive, separates warp vs damp). Treat analytic-derivative noise + the rotation matrix as a **first-class parity test**, not an afterthought.
- **Do the prerequisite first:** port/replace the Rust `height.rs` kernel-tiling path with the procedural generator — otherwise none of this reaches the CPU/collision side and you violate parity by construction.
- **Do NOT spec Tier-4 from this brief.** Its parity claim is unsupported, it's a new subsystem not a tier, and by the brief's own admission it still doesn't deliver connected trunk drainage. If pursued, gate it behind a standalone parity-feasibility spike.
- **The owner's core thesis stands, and the brief honestly confirms it:** no local closed-form `f(x,z)` produces connected dendritic drainage — that's a global-information obstruction, the same one that killed spectral. The realistic ceiling for the infinite+local+cheap+parity constraint set is **erosional texture + directional short gullies (tier 3)**. Continental connected drainage is out of scope for a pure generator and should be treated as a separate, optional, curated/precomputed feature layer if ever needed — not promised from noise or from a learned local kernel.

**Files inspected:** `D:\workflows\worldgen10\wg-10\rust\src\height.rs` (the divergent kernel-tiling runtime), `D:\workflows\worldgen10\tools\dem_pack\worldgen_proto.py` (the generator the brief actually describes), `D:\workflows\worldgen10\tools\dem_pack\biome_distill.py` (the distillation that would need rewriting).


========================================================================
## AGENT agent-a95b29a58450570a0.jsonl  (final text 16012 chars)
========================================================================

I now have complete ground truth. The decisive architectural fact: each clipmap level's page is generated by an **independent** `height_at(wx, wz)` evaluation at that level's resolution — the coarse texture is consumed only for **geomorph LOD blending** (`mix(h_fine, h_coarse, t)`), NOT as a structural input. Coarse pages do **not** feed structure into fine pages today; they are just lower-res evaluations of the same function. This is critical for assessing the brief's central recommendation. I have everything I need to write the adversarial verdict.

---

# ADVERSARIAL VERDICT: "erosion-distill" brief

## Bottom line up front

The brief is **substantially correct on the physics and the impossibility proof**, and its single most important claim — *the upstream-drainage-area term `A` is the exact, sharp boundary between local-doable and local-impossible* — is right and well-supported. That part is not hand-waving; it survives scrutiny.

**But the brief's headline recommendation (#1, "use your clipmap to carry the long-range drainage info") rests on a factual error about your codebase that I verified against the actual files.** The brief assumes your clipmap levels form a *hierarchy where coarse carries information into fine*. They do not. In `terrain_view.rs` + `height_page.glsl` + `ring_displace.gdshader`, **every level independently evaluates the same pure `height_at(wx,wz)`**, and the coarse texture is consumed **only for geomorph LOD blending** (`mix(h_fine, h_coarse, t)` at the ring's outer band — `ring_displace.gdshader:113`). There is no coarse→fine *data* path. So "you already have the machinery" is false: you have a render LOD pyramid, not a flow-carrying analysis pyramid. The recommendation is still *architecturally sound*, but it is **net-new infrastructure** (a separate coarse drainage field, its own deterministic generator, its own sampling path, its own parity surface), not a cheap reuse. The brief undersells the cost by a large margin.

That correction changes the rankings.

---

## Per-technique verdict table

| # | Technique | local | parity | produces_real_structure | verdict | 
|---|---|---|---|---|---|
| A | IQ derivative-damped fBm | **yes** | **yes** | **no** (texture only) | **adopt** (as polish, not the fix) |
| B | Thermal/talus relax (fixed-iter) | **conditional** | conditional | **no** (texture only) | **maybe** (see parity trap) |
| C | Slope/curvature-damped roughness | **yes** | **yes** | **no** (texture only) | adopt (subset of A) |
| D | Heightmap-window "erosion" filter | **no** | **no** | **no** | **reject** |
| E | Local incision *given* coarse flow | **conditional** | yes (the local half) | **partial** | **maybe** — the real prize, but gated on F |
| F | Coarse drainage field + local detail | **no** (global, by design) | conditional | **YES** | **adopt** — *this is the only real-structure path* |
| G | Learned local operator (cGAN/erosion-synth) | conditional | **no** (as NN) | partial | **reject as runtime**, maybe offline |
| H | FNO / neural-operator erosion | **no** | **no** | yes | **reject** (correctly cited only as *evidence*, not a recommendation) |

---

## Where I challenge the brief, technique by technique

### A. IQ derivative-damped fBm — brief says local/parity/cheap. VERDICT: TRUE, adopt, but the brief is *honest that it's not the fix*, and you should be too.
- **Local? Yes, genuinely.** It's one extra accumulator (`d`) in your existing fBm loop. No neighbor reads beyond the analytic derivative, which is a closed-form per-octave quantity. No global pass.
- **Parity? Yes, with one caveat the brief omits.** Analytic gradient noise requires you to switch your noise basis. Your current `wg_value_noise` (in `ring_displace.gdshader`) is bilinear-blended hash value noise; its analytic derivative is C0-discontinuous at lattice lines unless you move to a quintic-faded or gradient-noise basis. The numpy mirror (`worldgen_proto.py`) already uses quintic fade (`_fade`), but the **GPU detail shader uses cubic smoothstep** (`f*f*(3-2f)`). So "trivially parity-able" requires first unifying the noise basis across Rust/GLSL/numpy with a C2 fade and an exact analytic derivative on both sides. That's real work and a real parity-gate risk, not free.
- **Real structure? No. The brief says this explicitly and correctly** — IQ himself calls it "erosion-*like*," it's an amplitude/roughness modulator. It will NOT fix "looks like the same noise" on the structural axis. The owner has *already rejected tuned noise*; this is tuned noise with a slope-aware amplitude. **Do not let this be sold as the answer.** It's a finishing layer on top of whatever produces structure.

### B. Thermal/talus relaxation — brief table says "✅ YES — even fixed-iteration GLSL/Rust parity." VERDICT: This is the brief's most dangerous overclaim. Challenge stands.
- **Local per-step, yes. But "fixed-iteration" hides a neighbor-dependent iteration that breaks your `f(x,z)` contract.** Thermal erosion is `h_new[i] = h[i] + Σ neighbor flux`. To evaluate it at one point you must know your neighbors' *current* heights, and to do *N* iterations you must know your neighbors' *iteration-(k-1)* heights — which requires *them* to have read *their* neighbors, transitively reaching `N` cells away. A fixed `N`-iteration thermal relax is a **bounded stencil of radius N**, i.e. it reads a `(2N+1)²` neighborhood, NOT `(x,z)`. 
- **It is therefore NOT a pure `f(x,z)`.** It is compatible with your constraints *only if implemented as a separate page-resident image pass* (read page texel + neighbors, write new image, ping-pong N times) — exactly the kind of multi-pass image operation your current architecture does **not** have. Your pages are produced by a single `imageStore(height_at(wx,wz))` (`height_page.glsl:192`) with no neighbor reads and no ping-pong.
- **Seam problem the brief ignores:** an N-iteration stencil needs an N-texel apron of *correctly-computed* neighbor data around every page, or you get discontinuities at page boundaries (the same tile-seam failure the brief flags for droplets). Your pages are generated independently; a relax pass would need apron generation or it reintroduces seams — and your codebase fought hard to kill seams (the texel-corner convention + clamp_to_edge sampler in `ring_displace.gdshader`).
- **Verdict: maybe, but only as a deliberate multi-pass image stage with apron handling, with a CPU mirror that does the identical N-iteration stencil for facts parity.** The brief's "even fixed-iteration parity-able" is true in principle but materially understates that this is a new image-pass subsystem, not a per-pixel function. And it still produces no drainage structure — only smoothed slopes/talus.

### C. Slope/curvature-damped roughness — subset of A. Same verdict. Texture only.

### D. Heightmap-window "erosion" filter — brief itself half-rejects it. VERDICT: reject, and the brief's own citation undercuts it.
- 7–20 texel window = a stencil, not `f(x,z)`; needs apron; "can't capture regional flow" per the brief's own source. Produces no structure. Nothing here beats your ridged-noise bar. Drop it entirely.

### E. Local incision *given* you're told you're in a channel — VERDICT: this is the genuinely interesting one, and the brief is right that it's CONDITIONAL on F.
- The local half (given a discharge value `A` as input: `incision = k·A^m·slope^n`) is a closed-form, parity-trivial per-pixel op. **That half is real and cheap.**
- But it is *entirely* gated on something supplying `A(x,z)` as a bounded-tap input. Without F, it has nothing to read. So E is not independently adoptable; it's the consumer half of F.

### F. Coarse drainage field + local detail — brief's #1 recommendation. VERDICT: this is the ONLY path to real connected structure, the brief is right about that, BUT the brief's three sub-claims need correction:

1. **"You already have the clipmap machinery for this" — FALSE as stated (verified).** Your coarse levels are independent re-evaluations + LOD blend, not an information-carrying hierarchy. There is no flow field, no accumulation, no coarse→fine structural injection anywhere in the repo (grep for flow/drainage/accumulation/upstream returns only docs and this brief's own artifacts — zero implementation). Adopting F means building: (a) a deterministic coarse base-height function, (b) a flow-routing + accumulation pass over a bounded coarse window, (c) storage of the resulting discharge/distance-to-channel field, (d) a sampling path from fine pages into it, (e) a CPU mirror of all of the above for facts/collision parity. That is a milestone, not a knob.

2. **The "infinite" tension is deeper than the brief admits.** The brief's escape — "cap the basin scale to one coarse clipmap footprint, compute deterministic flow within that bounded window" — has a **seam problem the brief waves away**. If you compute flow inside bounded window W centered near the camera, and the camera moves so the window recenters, the *drainage network changes* because flow routing is globally coupled within W: a divide that was inside W is now outside, rivers re-route, and the *same world point gets a different channel depth depending on where the window was centered*. That violates DETERMINISM (`same (x,z,seed) → same height forever`) — your hardest constraint. The fix is non-trivial: you need a **fixed, seed-anchored tiling of flow-computation windows** (not camera-relative), with deterministic cross-window stitching, so a world point's drainage is computed in the same anchored window every time. The brief never addresses that the window must be world-anchored and deterministic, not camera-relative. This is the crux engineering risk and it is unmentioned.

3. **It genuinely does produce real structure (phase, not just spectrum).** This is the brief's strongest and most defensible claim: a computed flow graph has *connectivity and branching* — the exact phase information FFT discarded and ridged-noise cannot fake. Ridged-noise valleys (your current `worldgen_proto.py` line 85) have no area-scaling and no guaranteed connectivity; a flow-accumulation field does. **If you want to beat the WG9 ridged-noise bar toward real drainage, F is the only candidate on the table that can.** That part of the brief is correct and important.

   **Caveat I'd flag for source-checking:** the brief cites Braun & Willett 2013 O(n) and implies it makes the coarse pass "cheap." It is cheap *asymptotically* but it is a **global ordered traversal (stack-based topological sort over the whole window)** — inherently sequential, GPU-hostile, and must run on CPU or as an awkward multi-pass GPU scan. The brief lists it under "✅ viable" without noting it's the *opposite* of your per-pixel-parallel architecture. Verify the implementation cost before committing.

### G. Learned local operator (cGAN / Guérin erosion-synthesizer) — VERDICT: reject as runtime; the brief's own evidence kills it for your constraints.
- The brief itself notes Guérin's erosion-synth is **conditioned on a sketch/feature map that supplies the global structure** — i.e., it does NOT invent drainage, it's handed drainage. So it's just E+F wearing a neural costume.
- **Parity: no.** A CNN/MLP at runtime is not expressible identically in GLSL+Rust within your "value/gradient noise, fBm, abs, clamp, smoothstep, fixed loops" budget, and matching float results bit-for-bit across a Rust inference path and a GLSL inference path for the parity gate is a nightmare. 
- **Offline use is fine** (fit the *parameters* of your analytic operator from simulated terrain), but that's not "a learned local operator," that's "use ML to tune analytic knobs" — which the brief does eventually say in its recommendation #3. The recommendation is sound; the framing in §4 oversells "learned operator" as if it could be the runtime path.

### H. FNO / neural operators — correctly used only as *evidence* for the impossibility, not as a recommendation. No objection; the brief is honest that FNO needs a global receptive field and is therefore disqualified. Good adversarial hygiene by the author.

---

## Citations I'd want source-checked (overclaim/hand-wave watch)

1. **Braun & Willett 2013 "O(n)" framed as cheap** — true complexity, but the brief omits that it's a sequential global stack traversal hostile to your GPU-parallel per-pixel model. Verify before relying on "cheap coarse pass."
2. **"dandrino notes hydraulic sim is ~O(N³)"** — plausible for naive pipe-model iteration count but I'd confirm the exponent; it reads like a remembered blog figure, not a derived bound. Low stakes (it's only used to argue droplets are bad, which is independently true).
3. **Guérin "erosion synthesizer at very low cost"** — real paper, but the brief should make explicit (it half-does) that the *low cost* is at inference and the *structure* comes from the conditioning input. As written it can be misread as "learned cheap drainage from scratch," which it is not.
4. **The whole "✅✅ clipmap is the natural carrier"** — this is the claim that's wrong *about your repo* (not about the general idea). Not a citation issue; a codebase-fact issue I verified directly.

---

## Final ranking under YOUR hard constraints

**Worth speccing (in order):**

1. **F — coarse world-anchored deterministic drainage field, sampled by fine pages (with E as its consumer).** The *only* technique here that can produce real connected/branching structure (the phase the owner is actually chasing) rather than nicer noise. It cleanly dodges all three named dead ends (no spectrum, no dead-constant scalars, no DEM tiling). **But spec it honestly as a new milestone**, with the two unaddressed risks front-and-center: (i) it is net-new infrastructure, not clipmap reuse; (ii) the flow window MUST be world-anchored + seed-deterministic + deterministically stitched across window seams, or it breaks your determinism constraint. De-risk it OFFLINE first (numpy mirror, render an image, owner-eye judges "does this read as connected drainage?") exactly like the spectral approach was killed cheaply — *before* any GLSL.

2. **A/C — IQ derivative damping / slope-damped roughness** — adopt as a *finishing layer only*, with eyes open that it's texture not structure, and budget the noise-basis unification (C2 fade + exact analytic derivative across all three implementations) as the real (non-zero) cost. Do NOT let it masquerade as the structure fix.

**Traps / reject:**

3. **B (thermal relax)** — only if you consciously accept building a multi-pass image stage with apron handling and a CPU stencil mirror; the brief's "fixed-iteration, even parity-able" badly understates that it's a radius-N stencil, not `f(x,z)`, and reintroduces seams. Not worth it unless F is already built and you want talus polish on top.
4. **D, G(runtime), H** — reject. D is a seam-prone stencil with no structure; G/H can't hold parity and don't invent drainage anyway.

**The one-sentence correction to carry forward:** the brief's physics is right and F is the right bet, but "your clipmap already carries the long-range info" is false in this repo — coarse pages are independent re-evaluations used only for LOD morph (`terrain_view.rs`, `ring_displace.gdshader:113`), so F is a genuine new subsystem whose hardest, unmentioned problem is keeping a bounded flow computation **world-anchored and deterministic** under a moving camera.

Relevant files verified: `D:\workflows\worldgen10\wg-10\rust\src\height.rs`, `D:\workflows\worldgen10\wg-10\rust\src\terrain_view.rs`, `D:\workflows\worldgen10\wg-10\worldgen_terrain\shaders\height_page.glsl`, `D:\workflows\worldgen10\wg-10\worldgen_terrain\shaders\height_field.glsl`, `D:\workflows\worldgen10\wg-10\worldgen_terrain\shaders\ring_displace.gdshader`, `D:\workflows\worldgen10\tools\dem_pack\worldgen_proto.py`, `D:\workflows\worldgen10\tools\dem_pack\biome_distill.py`, `D:\workflows\worldgen10\docs\superpowers\specs\2026-05-30-worldgen10-north-star-vision.md`.


========================================================================
## AGENT agent-ab89d829ca42aeb3c.jsonl  (final text 32248 chars)
========================================================================

I now have the actual code grounded. The brief below is keyed to what your `biome_distill.py` / `worldgen_proto.py` / `height.rs` actually do. This is a technical survey, not a code change.

---

# THE DEM MEASUREMENT QUESTION — a geomorphometric brief

## 0. Why your current metrics came out dead-constant (the diagnosis that drives everything)

Your dead metrics (`amp_profile`, `ridge_linearity`, `anisotropy`, `dominant_wavelength_m`) all share one fatal property: **they are computed on the raw elevation field as a generic 2D signal**, and they are **isotropic, scale-coupled, or amplitude-only**. On a 512×512 COP30 tile (≈ 90–430 m/px → 46–220 km wide), every terrain looks like "1/f-ish anisotropic-ish bumps" by those measures. The structure-tensor coherence sits near 0.30 for *everything* because real terrain at that window has no single dominant gradient direction — ridges fan and branch in all directions. Argmax-of-DoG-band gives ~25 km for everything because the dominant *energy* band is always the macro-landmass band, regardless of biome.

The fix is not a better generic-signal metric. It is to switch to **geomorphometric metrics computed on derived terrain fields** — the *hypsometric distribution*, the *flow-routed channel network*, *curvature sign statistics*, and the *slope–drainage-area relation*. These were invented precisely to discriminate landform *type* and *erosional maturity*, and they are known to vary by 2–10× between the families you have. Below, each metric gets: definition, why it discriminates, expected behavior across your 12 families, and computability constraint (offline-only vs. mappable-to-a-knob).

A crucial framing you must keep: **all of these are statistics of structure. None of them generate phase.** Section 3 confronts that head-on. Metrics buy you *tuning*, and they buy you *which structure-generating primitive to switch on*. They never substitute for a generator that can emit connected drainage.

---

## 1. The geomorphometry metrics that actually distinguish terrain TYPES

I rank these by **expected discriminative power across YOUR 12 families on 512px COP30**, with the weak/dead ones flagged.

### Tier A — strong discriminators, compact, well-precedented

**A1. Hypsometric integral (HI) + full hypsometric curve shape.**
The single most established "what kind of landscape is this" scalar. Definition and discrimination in §2 (it's the centerpiece). HI separates **young/uplifting (convex curve, HI≈0.6)** from **mature/eroded (S-curve, HI≈0.4–0.5)** from **old/peneplain (concave, HI<0.35)**. On your families: glacial cirque/trough terrain and badlands sit at extremes vs. rounded old mountains and grassland. **This will vary.** Strahler (1952) is the origin; Pike & Wilson (1971) showed HI ≡ (mean−min)/(max−min) so it's a 3-number computation, trivially robust on 512px.

**A2. Curvature-sign statistics: fraction-convex vs fraction-concave, and the plan/profile curvature distribution.**
Compute profile curvature (curvature along the slope = erosion/deposition signature) and plan curvature (across-slope = flow convergence/divergence). Then take **distributional features**: % concave (channels/hollows), % convex (ridges/noses), the skew of the curvature histogram, and the **ratio of concave-area to convex-area**.
- Badlands & karst: extreme bimodal curvature — sharp narrow concave channels between sharp convex divides → high curvature variance, near-symmetric convex/concave split with heavy tails.
- Glacial troughs: strongly *concave* cross-profiles (U-shape) — plan-curvature distribution skewed concave at the valley scale; cirque headwalls = extreme convex-up break-of-slope.
- Old mountains / grassland: low curvature variance, gentle, near-Gaussian curvature histogram.
- Volcanic cones: radial plan-curvature signature (convex everywhere near summit).
This is **the metric most likely to separate glacial-U from fluvial-V** because U vs V *is* a cross-profile curvature statement. Compute on your z-score field with a fixed-px window; it's local, cheap, and **varies strongly**. (Wood 1996 thesis; Evans 1980; Schmidt et al. 2003 on curvature definitions.)

**A3. Hypsometric / curvature-derived "valley cross-section form" — the V-vs-U index.**
Specifically: take the cross-valley elevation profile perpendicular to channels and fit a power-law z ∝ |x|^b. **b≈1 → V (fluvial), b≈2 → U (glacial).** This is the literature's direct glacial-valley discriminator (e.g. the "valley shape ratio" / "V-index", James 1996; Li et al. 2001). You can approximate it without channel extraction by binning the curvature-along-contours, but the clean version needs channels (offline-only). **Highest power for glacial vs everything; offline-only.**

**A4. Drainage density Dd = total channel length / area.**
The classic Horton/Strahler metric. Requires flow-routing (D8 or D∞) to extract channels, then a channel threshold. Discrimination:
- **Badlands: extreme high Dd** (this is the *defining* property of badlands — "fine drainage texture", densest dissected terrain on Earth). Expect 10–100× grassland.
- **Karst: anomalously LOW surface Dd** (drainage is underground — sinkholes, no surface channels). This is a *signature inversion* that no generic metric of yours can see.
- Mountain/temperate/rainforest: moderate-high.
- Desert/grassland/tundra/wetland: low.
Dd is one of the **most reliably discriminating** geomorphometric scalars between your families. Offline-only (needs flow routing), but it's a single scalar you can map to a knob. (Horton 1945; Strahler 1957; Montgomery & Dietrich 1989 on Dd and process.)

**A5. Slope–area scaling exponent θ (the channel concavity index).**
The signature of erosional regime. Detailed in §2. **Strong discriminator of process** (debris-flow vs alluvial vs bedrock-incision dominated), compact scalar, but **noisy on 512px** — you need enough channel pixels to fit log S vs log A. Offline-only. (Tarboton et al. 1991; Montgomery & Foufoula-Georgiou 1993; Whipple & Tucker 1999 — stream-power law.)

### Tier B — useful, varies, cheaper

**B1. Terrain Ruggedness Index (TRI, Riley et al. 1999) and Vector Ruggedness Measure (VRM, Sappington et al. 2007).**
TRI = mean |Δz| to the 8 neighbors. VRM = dispersion of unit normal vectors in a window (decouples ruggedness from slope — this is its advantage over TRI/slope, which conflate). Discrimination: badlands & mountain & volcanic high; grassland/wetland/desert low; **glacial is interesting — smooth troughs (low VRM) punctuated by arêtes (high VRM)**, giving a *bimodal* VRM distribution. The **VRM histogram shape** (not just its mean) discriminates. Local, cheap, **and VRM specifically should vary** because it's normalized against slope (your `mean_slope_deg` already captures slope; VRM adds the orthogonal "roughness-at-constant-slope" axis). Mean VRM is mappable to a knob.

**B2. Local relief (relief within a moving window R km) and the relief ratio.**
Local relief at a fixed window (e.g. 2 km, 5 km) is a classic relief metric. You already have global `height_range_m`; **windowed local relief at 2 km** adds dissection scale. Badlands: high local relief at *small* window, modest at large (fine texture). Mountains: high at both. The **ratio relief@2km / relief@10km is a texture-scale fingerprint** that should separate badlands (high ratio) from big mountains (low ratio). Cheap, local, mappable.

**B3. Slope distribution shape (not just mean).**
You only use `mean_slope_deg`. The **standard deviation, skew, and the % of pixels above the threshold-of-stability (~30–35°, the angle of repose)** discriminate strongly. Badlands & mountains have a fat upper tail clipped at the repose angle (a near-vertical wall in the slope CDF) — grassland does not. The **"slope frequency at the modal angle"** and **fraction-at-threshold-slope** are classic (Strahler 1950; Carson & Petley 1970 "characteristic slope angles"). Cheap, you already compute slope; just keep more moments. Mappable.

**B4. Topographic Wetness Index (TWI = ln(A/tanβ)) and Stream Power Index (SPI = A·tanβ) — as DISTRIBUTIONS.**
Both need contributing area A (flow routing → offline). TWI distribution separates **wetland (heavy wet tail, flat → huge A/tanβ)** and broad valleys from dissected uplands. SPI flags incision potential. The *shape* of these distributions (especially TWI's right tail) is a wetland/floodplain fingerprint your current metrics completely miss. Offline-only; distribution-summary mappable. (Beven & Kirkby 1979 TWI; Moore et al. 1991.)

### Tier C — channel-network topology (highest "drainage realism" content, most expensive)

**C1. Strahler stream order, bifurcation ratio Rb, length ratio Rl, drainage texture.**
Once you have the channel network, its **topology** is the deepest descriptor of drainage *connectedness*. Bifurcation ratio Rb (≈3–5 for natural dendritic networks; Horton's laws) and the **junction-angle distribution** distinguish drainage *patterns*: dendritic (badlands, rainforest), parallel (steep uniform slopes — young volcanics, fault scarps), trellis/rectangular (structural control — some karst, folded mountains), radial (volcanic cones). **Drainage pattern is exactly "the thing that makes terrain read as real connected drainage."** This is the richest signal — and the hardest to both measure (needs network extraction) and *reproduce* (see §3). Offline-only, descriptive, **not directly mappable to your current knobs** — it tells you *which generator family* you'd need, not a scalar to plug in. (Horton 1945; Strahler 1952, 1957; Howard 1967 on drainage patterns; Zanardo et al. 2013 on pattern quantification.)

**C2. Peak/pit/saddle counts and the Morse/surface-network density (Maxwell–Warntz / Pfaltz).**
Counts of critical points per km² and the saddle:peak:pit ratio (the Euler relation peaks − saddles + pits = const constrains these). Pit density is a **karst/glacial fingerprint** (sinkholes; kettle-and-kame, cirque tarns) — closed depressions are rare in fluvial terrain and abundant in karst/glacial. **Pit (closed-depression) density is one of the cleanest karst-vs-fluvial discriminators you can compute.** Local-ish, offline. (Pike 2000 geomorphometry review; Wood 1996 morphometric feature classification.)

### Tier D — known-weak / will-stay-dead on your data (don't re-add)
- **Power spectrum / spectral slope β** — dead (your finding; phase-blind). Confirmed.
- **Global structure-tensor coherence / anisotropy** — dead (~0.30 for all; isotropic at 512px).
- **Argmax-DoG "dominant wavelength"** — dead (always the macro band).
- **Fractal dimension D (box-count / variogram)** — *theoretically* a roughness discriminator but empirically **clusters tightly (~2.1–2.3) for almost all natural terrain**; weak separator on 512px. Skip.
- **Single-number mean slope** alone — you have it; it's vetted; keep it but it's not enough.

---

## 2. The two maturity scalars in depth: hypsometric integral and slope–area θ

### 2.1 Hypsometric integral (HI)

**Definition.** The hypsometric curve plots **relative area** a = A(h)/A_total (fraction of the basin above elevation h) on x against **relative elevation** y = (h − h_min)/(h_max − h_min) on y. HI = the area under that curve = ∫₀¹ a dy ∈ [0,1]. The Pike–Wilson shortcut: **HI ≈ (h_mean − h_min)/(h_max − h_min)** — three numbers from the elevation histogram. Robust on 512px; no flow routing needed.

**What it encodes — the "amount of mass left to erode."** A young, uplifted, lightly-dissected block still has most of its volume sitting high → curve bulges up-right → **HI high (~0.6+), convex**. As erosion proceeds it eats the uplands and fills lowlands → **S-shaped, HI ~0.4–0.5 (mature)**. An old peneplain has most area near the base level with isolated monadnocks → **concave curve, HI < 0.35 (old)**. Strahler (1952) defined exactly this young/mature/old triad; Willgoose & Hancock (1998) and Lifton & Chase (1992) tied HI quantitatively to erosional stage and tectonics.

**Why it separates YOUR families:**
- **Glacial troughs:** glaciers excavate mid-slope and overdeepen valley floors, removing mid-elevation mass → curve pulled *down in the middle* → **distinctly low/concave HI**, and a characteristic "step" from valley-floor flats + headwall. This is documented as a hypsometric *signature of glaciation* (Brocklehurst & Whipple 2004 — glacial vs fluvial hypsometry is one of the cleanest published HI contrasts).
- **Rounded old mountains:** mature S to concave, HI ~0.4.
- **Badlands:** intensely dissected but in *dynamic equilibrium* — area distributes fairly evenly across elevations → HI near 0.5 but with a **near-straight (linear) curve**, which is itself diagnostic (curve *shape*, not just integral).
- **Volcanic cones:** convex, high HI (mass piled high, constructional not erosional) — HI distinguishes *constructional* from *erosional* relief.
- **Grassland/wetland/coast:** low HI, concave, mass near base level.

**Use both the integral AND two curve-shape moments** (skewness, kurtosis of the hypsometric curve, per Harlin 1978) — the integral alone can't tell convex-young from straight-badlands. These are 3–4 cheap scalars off the elevation histogram and **they will vary across your 12 families.**

### 2.2 Slope–area scaling exponent θ (channel concavity)

**Definition.** For channel pixels, drainage area A correlates with local slope S as a power law **S = k_s · A^(−θ)** (Flint's law). Plot log S vs log A for channel pixels; **θ = −slope of the regression**; k_s = the intercept = the **steepness index**. θ is the **concavity index**, k_s the **steepness index** (normalized: k_sn at a reference θ).

**What it encodes — the erosional process regime.** From the stream-power incision model E = K·A^m·S^n, at steady state θ = m/n. Empirically:
- **θ ≈ 0.4–0.6: alluvial / bedrock fluvial channels** (the classic range; Tarboton et al. 1991, Whipple & Tucker 1999).
- **θ ≈ 0.0–0.2 (low concavity): debris-flow-dominated steep headwaters** (Stock & Dietrich 2003) — channels stay steep even as area grows. Characteristic of **steep young mountains and badlands headwaters**.
- **θ > 0.6 (high concavity): mature, transport-limited, low-gradient systems** — old eroded landscapes, large lowland rivers.
**k_sn (steepness)** scales with **uplift/erosion rate** — high in tectonically active mountains, low in cratonic old terrain. So the pair (θ, k_sn) is a 2D process fingerprint.

**Why it separates sharp-glacial vs rounded-old vs dendritic-badlands:**
- **Dendritic badlands:** low θ (debris-flow/steep regime) + **high k_sn** + (separately) very high drainage density — the combination is unmistakable.
- **Rounded old mountains:** high θ (concave, mature) + low k_sn.
- **Glacial valleys:** **break Flint's law** — glacial long-profiles are non-concave-up, often with reverse slopes (overdeepenings) and hanging valleys → the slope–area plot is *scattered / non-power-law*, and θ fit is poor/negative. The **failure of the slope–area power law is itself the glacial signature.** (Brocklehurst & Whipple 2002; MacGregor et al. 2000 on glacial long-profiles.)

**Caveat for 512px:** θ needs a decent channel-pixel count to fit cleanly. On 512px COP30 you'll get a usable but noisy fit; aggregate across the ~10 DEMs per family (you already do `aggregate_median`) and it stabilizes. **Offline-only** (needs A from flow routing). Map: θ → channel-profile concavity knob; k_sn → relief/incision-rate knob.

---

## 3. The phase problem, restated — and the honest answer

You've already discovered the deepest truth here, and it generalizes beyond the power spectrum: **every one of these metrics is a low-dimensional STATISTIC of structure. A generator must EMIT the structure (the phase).** A statistic constrains a generator's *parameters*; it cannot inject structure the generator's basis cannot already produce.

So sharpen the question into three honest claims:

**(a) Metrics tune; they don't create.** "drainage density = X, HI = Y, θ = Z" can only steer knobs of a function that *already* produces channel-like connected lows. If your basis is `value-noise fBm + ridged-noise + subtractive ridged-noise` (your `worldgen_proto.generate`), then HI maps to the upland/landmass amplitude split, θ maps to how slope falls with scale (octave gain), Dd maps to ridge/valley frequency, V-vs-U maps to a `pow()` shaping of the carve cross-section. That is real, useful, and is exactly the WG9 recipe working. **It will make biomes *differ* and read as "this biome's roughness/steepness/dissection."** It will **not** create *connected dendritic networks with correct junction angles* because ridged noise has no connectivity constraint — `1 − |noise|` crests are locally linear but globally **disconnected and non-hierarchical**. That is the ceiling of the noise-basis approach, and it's why the owner says "still looks like noise": the *statistics* can be matched while the *topology* (connected, branching, downhill-monotone drainage) is absent.

**(b) Connected drainage is a TOPOLOGICAL / global property; no purely local noise basis produces it.** A real drainage network has three properties noise cannot guarantee locally: every channel is **monotonically downhill to a shared outlet** (global), tributaries **join at acute downstream angles** (Horton/Playfair), and channel spacing obeys a **competition law** (Hack, Perron et al. 2009 on the wavelength of valley spacing). These are the *phase*. So the metrics tell you "you are missing a network with Dd=X, Rb=4, θ=0.5" — and then you still need a *generator primitive* that emits a network.

**(c) Therefore the metric program splits the problem correctly:** metrics for the **continuous/textural** part (slope, relief, ruggedness, curvature-roughness, HI-driven mass distribution) — these noise can match, and metrics genuinely help. And a **separate structure-emitting primitive** for the **network** part — which no scalar can conjure and which is what §4 must supply.

The candidate **local network-emitting primitives** (all parity-safe, all f(x,z) with bounded reads) that go *beyond* ridged noise:

1. **Worley/cellular-warped flow lines (analytic).** Use a Worley/Voronoi feature-point lattice; carve toward the nearest *lower* feature point. Voronoi *edges* form a connected, branching, space-filling graph with controllable cell size (→ Dd) and junction angles. Warped Worley F2−F1 gives **connected linear lows that actually branch** — strictly more network-like than `1−|fBm|`. Still O(few) lookups, parity-safe.
2. **Analytic "downhill" carving via a coarse height + gradient-descent of fixed iteration count.** Evaluate a cheap coarse macro height analytically, then run a **fixed N-step (e.g. 16) gradient-descent trace** from (x,z) accumulating a "distance-to-channel / flow-accumulation proxy." Fixed iteration count = parity-safe and bounded. This injects *downhill-connectedness* locally (Guérin et al. 2016 "Sparse representation of terrains"; Génevaux et al. 2013 procedural rivers — though their full versions are global, the bounded-trace approximation is local).
3. **Precomputed network baked to a sparse vector/curve set, evaluated analytically.** Generate the drainage graph **offline** (real flow routing on a coarse procedural macro-surface, tiled at very large scale with warp to hide repetition), store as a sparse set of spline channels, and at runtime compute **distance-to-nearest-channel** analytically (bounded local query against a spatial hash). This is the standard "procedural rivers as curves" trick and the most reliable way to get *real* connectivity cheaply — the network's *phase* is authored offline, runtime only evaluates a distance field. Parity-safe if the hash query is bounded.

The honest summary: **metrics → noise-basis params gets you biome-correct texture and the WG9 bar. To BEAT WG9 toward real drainage you must add a connectivity-bearing primitive (Worley-flow, bounded-trace, or baked-curve distance field). Metrics then *tune* that primitive (Dd→cell size, θ→carve-profile, junction stats→primitive choice).**

---

## 4. Hand-metrics→params  vs  offline-learn-operator — when each is right

Three strategies, not two. Be explicit about what each can and can't do:

**Strategy 1 — Hand-metrics → params (what you have).**
*Right when:* the generator basis already produces the right *kind* of structure and you only need to set its knobs per biome. Cheap to build, fully interpretable, trivially parity-safe, debuggable ("ridge_strength too high"). *Ceiling:* cannot exceed what the basis can emit. Your `params_from_metrics` is this. Verdict: **keep it for the continuous/textural channel; upgrade the metrics (HI, curvature stats, VRM, Dd, slope-distribution moments) so the knobs actually differ per biome.** This alone fixes "all biomes look the same."

**Strategy 2 — Offline-learn a LOCAL operator that approximates erosion (the owner's thesis).**
Run **real hydraulic/thermal erosion offline** on procedural base terrain, then fit a **cheap local operator** O(neighborhood) → Δheight that approximates the erosion delta. Concretely: erosion's *first-order* effect is well-approximated by **curvature-driven diffusion + flow-weighted incision** — both are *local stencils*. Fit a small CNN, a polynomial in {slope, curvature, local flow-proxy}, or a learned look-up that you then **distill into a fixed-iteration GLSL/Rust stencil**. 
*Right when:* you want erosion's *characteristic look* (smoothed convexities, sharpened concavities, the "settled" appearance) **applied per-page as a fixed-stencil post-pass**, and a few stencil iterations are affordable per page (not per frame per texel — per page at generation time). *Key subtlety the owner must accept:* erosion is iterative-global, but its **converged effect** can be approximated by a **few local stencil passes over a page** (a page is bounded, so this is local + parity-safe if iteration count is fixed and the page has skirt/apron overlap to hide seams). This is the well-known result that **thermal erosion ≈ slope-limited diffusion** and **a few iterations capture most of the visual settling** (Musgrave/Kolb/Mace 1989; Olsen 2004 "real-time hydraulic erosion"; recent learned-erosion work, e.g. neural erosion surrogates). 
*What it adds over Strategy 1:* it produces the **convex-hilltop / concave-valley curvature signature and dendritic refinement** that noise lacks — i.e. it *can move the curvature-distribution metric (A2) toward real values*, which noise tuning cannot. 
*Limits:* a local stencil still does **not** create *long-range connected* trunk valleys from scratch — it *refines and connects existing lows*. So it pairs with, not replaces, a network primitive (§3).

**Strategy 3 — Learn the generator end-to-end (GAN/diffusion DEM synthesis).** Mentioned for completeness; **reject for your constraints.** It produces structure but is not f(x,z), not seamless-infinite, not parity-safe, not cheap-per-texel. Out.

**Decision rule:**
- **Texture/steepness/dissection-density per biome →** Strategy 1 with upgraded metrics (cheap, do first).
- **The "settled, eroded, connected-refined" LOOK that noise can't fake →** Strategy 2 (offline-learn local stencil, applied per-page at gen time, fixed iterations, apron overlap). This is the right home for the owner's erosion thesis — **erosion's local stencil is learnable and parity-portable; erosion's global routing is not, so don't try to learn the routing — supply it with a §3 network primitive.**
- **Long-range connected trunk drainage →** §3 baked-curve or Worley-flow primitive (authored/analytic connectivity), tuned by Dd/θ/junction metrics.

The cleanest architecture: **macro fBm landmass (S1-tuned) + network primitive carrying connectivity (S3, Dd/θ-tuned) + per-page learned erosion stencil for the settled curvature signature (S2) — and the metrics' job is to set the dials of all three.**

---

## 5. The recommended metric set — 5–10 that will (a) vary on 512px COP30 and (b) map to local-generator knobs

Ranked. Each: what it measures, why it varies across your 12, the knob it drives, and cost.

| # | Metric | Discriminates (expected) | Drives knob | Cost / locality |
|---|--------|--------------------------|-------------|-----------------|
| 1 | **Hypsometric integral + curve skew/kurtosis** (Pike–Wilson + Harlin moments) | glacial(low/concave) vs volcanic/young(convex) vs badlands(linear) vs old(concave). **Strong.** | macro upland/landmass amplitude split; `upland` smoothstep center; relief mass distribution | Trivial (elevation histogram). Offline, no routing. **Add first.** |
| 2 | **Curvature-sign distribution**: %concave, %convex, curvature variance, concave:convex area ratio | **glacial-U vs fluvial-V**, badlands/karst bimodal vs grassland Gaussian. **Strong.** | valley carve cross-section `pow()` (V vs U); ridge sharpness; erosion-stencil target | Cheap local stencil. Offline; could also gate the S2 stencil. |
| 3 | **Drainage density Dd** (channel length / area, from D8/D∞ + threshold) | **badlands very high, karst anomalously low, desert/grassland low.** Strong + a karst inverter. | network-primitive cell size / `valley_freq`; carve spacing | Needs flow routing → **offline-only**; one scalar. |
| 4 | **Slope distribution moments**: std, skew, **fraction ≥ repose-angle (~33°)** | badlands/mountain fat clipped tail vs grassland/wetland none. **Strong, cheap.** | ridge_strength, slope_bias, amplitude clamp at repose | Trivial local (you already compute slope; keep moments). Offline-cheap, runtime-cheap. **Add first.** |
| 5 | **Vector Ruggedness Measure (VRM)** mean + **histogram bimodality** (Sappington 2007) | roughness *decoupled from slope*; glacial bimodal (smooth trough + sharp arête). **Good — orthogonal to your existing mean-slope.** | high-freq octave amplitude; ruggedness/detail gain | Local, cheap. Offline + runtime-cheap. |
| 6 | **Windowed local relief at 2 km AND 10 km + their ratio** | texture *scale*: badlands(high@2km) vs big-mountain(high@both). **Good.** | base_freq vs ridge_freq balance; octave amp profile shape | Local windows. Offline-cheap. |
| 7 | **Slope–area concavity θ + steepness k_sn** (Flint's law fit on channel pixels) | process regime: debris-flow/badlands(low θ,high k_sn) vs mature(high θ) vs **glacial(law breaks)**. **Strong but noisy@512px** | channel long-profile concavity; incision-vs-area carve law | Needs routing → **offline-only**; aggregate per family to denoise. |
| 8 | **Closed-depression (pit) density + saddle:peak:pit ratio** | **karst & glacial high (sinkholes, tarns, kettles); fluvial ~0.** A clean karst/glacial flag. | switch a "depression/closed-basin" primitive on/off; karst pitting overlay | Local-ish critical-point count. Offline. |
| 9 | **TWI right-tail / flat-wet fraction** (ln(A/tanβ)) | **wetland & floodplain & coast** heavy wet tail. Separates the flat-wet families your metrics currently can't. | flatten/aggradation term; wetland flatness knob | Needs routing → offline-only; one tail statistic. |
| 10 | **Junction-angle + bifurcation-ratio summary** (network topology) | drainage *pattern*: dendritic(badlands/rainforest) vs parallel(young volcanics) vs trellis/rectangular(structural) vs radial(volcanic cone) | **selects which network primitive** (§3) per biome — not a scalar dial | Needs network extraction → offline, descriptive. Use to choose primitive, not to set a number. |

**Minimal high-value subset if you want to move fast:** **#1 (HI+moments), #4 (slope moments), #2 (curvature signs), #5 (VRM)** are all **cheap, local, no-flow-routing, and will vary** — these alone should break the dead-constant problem and make biomes visibly differ under your existing generator. Then add **#3 (Dd)** and **#7 (θ)** offline as the two "erosional maturity / process" scalars once you have flow routing, and **#8 (pit density)** to catch karst. **#10** is not a dial — it's the trigger to adopt a §3 network primitive.

**Two concrete fixes to your existing `biome_distill.py`:**
- Replace the dead `dominant_wavelength_m` (argmax-DoG, always ~25 km) and the dead `anisotropy`/`ridge_linearity` (structure-tensor ~0.30) with **HI+moments (#1), slope-distribution moments (#4), curvature-sign stats (#2), VRM (#5)** — all computable from the same z-score array you already load, no new data, no flow routing. This is a small edit to `metrics_for_dem` and `params_from_metrics`.
- Your `incision_depth` (concavity-gated regional-minus-local) is a *reasonable cheap proxy for Dd×depth* — keep it, but it conflates density and depth; **add true Dd (#3) offline** to separate "many shallow channels" (badlands) from "few deep canyons" (some desert/mountain).

---

## Key references (geomorphometry canon)

- **Strahler, A.N. (1952)** "Hypsometric (area-altitude) analysis of erosional topography," *GSA Bulletin* — origin of HI and young/mature/old.
- **Strahler, A.N. (1957)** "Quantitative analysis of watershed geomorphology," *AGU Trans.* — drainage density, stream order, ruggedness.
- **Horton, R.E. (1945)** "Erosional development of streams…," *GSA Bulletin* — Dd, Horton's laws, bifurcation ratio.
- **Pike, R.J. & Wilson, S.E. (1971)** "Elevation-relief ratio, hypsometric integral…," *GSA Bulletin* — HI ≈ (mean−min)/(max−min).
- **Harlin, J.M. (1978)** hypsometric curve skewness/kurtosis as maturity descriptors.
- **Tarboton, Bras, Rodríguez-Iturbe (1991)** slope–area scaling / channel concavity; **Montgomery & Foufoula-Georgiou (1993)** channel initiation by slope–area; **Whipple & Tucker (1999)** stream-power incision (θ=m/n).
- **Stock & Dietrich (2003)** valleys by debris flow (low-θ regime); **Montgomery & Dietrich (1989/1992)** Dd and process.
- **Brocklehurst & Whipple (2002, 2004)** glacial vs fluvial hypsometry and slope–area (the U-vs-V / law-breaking signatures).
- **Riley, DeGloria, Elliot (1999)** TRI; **Sappington et al. (2007)** VRM (ruggedness decoupled from slope).
- **Evans (1980); Wood (1996, thesis); Schmidt et al. (2003)** curvature definitions and morphometric feature classification (peak/pit/saddle, plan/profile curvature).
- **Beven & Kirkby (1979)** TWI; **Moore, Grayson, Ladson (1991)** terrain analysis (SPI/TWI).
- **Pike, R.J. (2000)** "Geomorphometry — diversity in quantitative surface analysis," *Prog. Phys. Geog.* — the field review.
- **Howard (1967); Zanardo et al. (2013); Perron et al. (2009)** drainage *pattern* quantification and valley-spacing wavelength (the "phase"/connectivity literature).
- Procedural-generation side for §3/§4: **Musgrave, Kolb, Mace (1989)** erosion; **Olsen (2004)** real-time erosion; **Génevaux et al. (2013)** procedural rivers; **Guérin et al. (2016)** sparse terrain representation; recent **neural-erosion surrogate** work.

---

**Grounding note (files inspected, all absolute):**
- `D:\workflows\worldgen10\tools\dem_pack\biome_distill.py` — confirmed the dead metrics are explicitly flagged in-code (`ridge_linearity ~0.30`, `dominant_wavelength_m ~25 km`, both computed-but-not-read in `params_from_metrics`); the only *live* discriminators currently driving knobs are `mean_slope_deg` (meta) and `incision_depth_m/relief` — i.e. you're effectively running on **two** real degrees of freedom. The recommendations above add 4 cheap no-routing DOF (#1,#2,#4,#5) computable from the same loaded z-score array.
- `D:\workflows\worldgen10\tools\dem_pack\worldgen_proto.py` — generator basis is `value-noise fBm + ridged-noise ridges + subtractive ridged-noise valleys`; confirmed §3's ceiling: `ridged_fbm = 1−|noise|` produces locally-linear but globally-disconnected, non-hierarchical crests → cannot emit connected dendritic networks regardless of metric tuning.
- `D:\workflows\worldgen10\tools\dem_pack\distill_biomes.py` — per-family `aggregate_median` over ~kernels; this is the right place to denoise the noisy-on-512px metrics (θ, Dd) by family aggregation.
- `D:\workflows\worldgen10\wg-10\rust\src\height.rs` — current Rust runtime still **tiled-samples kernels** (`sample_kernel` wraps/repeats), i.e. the forbidden path is still the live runtime; the proto/distill pipeline is the intended replacement. Any new metric must feed `params` consumed by a `generate()` port, not the kernel sampler.


========================================================================
## AGENT agent-aded0fb863e6c68dc.jsonl  (final text 20483 chars)
========================================================================

I now have comprehensive, well-sourced coverage across all five sub-questions, including the crux evidence on global dependency (stream power law, Braun-Willett ordering, FNO global receptive field, droplet cross-tile migration). Writing the brief.

---

# TECHNICAL BRIEF: Can erosion be a LOCAL, parity-able, infinite-compatible operator?

**Verdict in one line:** Partially. A local operator CAN reproduce the *short-range* products of erosion (thermal/talus relaxation, slope-damped roughness, smoothed valleys, local incision-as-a-function-of-slope) and these are real, shippable wins over your current ridged-noise bar. A local operator CANNOT reproduce *long-range fluvial drainage networks* (the branching, area-accumulating river/canyon structure the owner is actually chasing), because that structure is defined by a global integral — upstream drainage area — that no bounded-neighborhood `f(x,z)` can know. The only escape is to **inject the long-range information from a coarser level** (clipmap/hierarchical flow), which you already have the machinery for. That hybrid is the recommendation.

---

## 1. How real GPU hydraulic erosion works, and WHY it is global/iterative

**The two canonical real-time methods, both global-by-iteration:**

**(a) Eulerian "virtual pipe" shallow-water model — Mei, Decaudin & Hu, "Fast Hydraulic Erosion Simulation and Visualization on GPU" (Pacific Graphics 2007).** The per-timestep pipeline (confirmed from the implementation report and the Interactive Erosion Simulator) is:
1. Water increment from rain: `d1 = d + Δt·r`
2. Flow via virtual pipes: outflow flux to the **4 neighbors** driven by hydrostatic height differences (water+terrain), then flux-limited so you can't drain more than is present
3. Water-height update from net flux (inflow − outflow)
4. Velocity field from flux differences
5. Erosion/deposition by sediment capacity `C = Kc · sin(α) · |v|` (α = local tilt, v = velocity); dissolve where capacity > suspended load, deposit where it's exceeded
6. Sediment advection (semi-Lagrangian backtrace `s(x − u·Δt, y − v·Δt)`)
7. Evaporation `d·(1 − Ke·Δt)`

Every single step is **local (4-neighbor)**. The catch — and this is the crux — **information propagates exactly one cell per iteration.** Water has to physically march downhill cell-by-cell over hundreds of timesteps before a connected drainage channel exists. The locality is per-step; the *result* is global because it's the fixed point of many local steps. (Jákó, "Fast Hydraulic and Thermal Erosion on the GPU," CESCG 2011, adds thermal erosion to the same pipe framework.)

**(b) Lagrangian droplet/particle erosion** (Mei-derived; nickmcd "Procedural Hydrology"; dandrino's "terrain-erosion-3-ways"; Frozen Fractal devlog). Each raindrop spawns, follows the gradient downhill, picks up sediment ∝ slope·speed, deposits when it slows, evaporates. This is even *worse* for your constraints: each droplet is a **data-dependent, variable-length, unbounded loop** (path length depends on the terrain, which is being modified as you go), and droplets routinely travel far — the Frozen Fractal author states the killer fact directly: *"a droplet can affect the terrain far away from where it's initially spawned. This makes it difficult to apply erosion to individual terrain chunks."* dandrino notes hydraulic sim is ~O(N³) in grid dimension.

**(c) The geomorphology ground truth — the stream power law:** `E = K · A^m · S^n` where **A is upstream drainage area** and S is local slope (Wikipedia "Stream power law"; Cordonnier et al. 2016; Braun & Willett 2013). This is the equation that makes rivers look like rivers, and it depends *explicitly on A* — a watershed-scale cumulative quantity. The incision rate at a point is governed by how much land drains *into* it, which can be the entire upstream basin. **This is the precise mathematical statement of the long-range dependency.** A local f(x,z) has no access to A.

**The long-range dependency, stated precisely:** drainage is a *connected* phenomenon. A river is deep *here* because of all the water collected *upstream over kilometres*. The depth at (x,z) is a function of an integral over a watershed whose shape itself depends on the whole height field. There is no local closed form for "am I in a river and how big is it" without first solving the global flow-routing problem.

---

## 2. Existing local/analytic erosion fakes that actually ship

**(a) IQ directional-derivative erosion — Inigo Quilez, "Better/more noise" (iquilezles.org/articles/morenoise/).** The fBm accumulates analytic derivatives `d` across octaves and damps each octave by the accumulated slope:
```
a += b * n.x / (1.0 + dot(d, d));   // d = running sum of derivatives
```
Effect: where the running gradient is steep, higher octaves are suppressed → **flat valley floors and smooth lowlands, rough ridges/peaks.** This is *fully local, deterministic, trivially CPU/GPU parity-able, one extra accumulator in your existing fBm loop.* 

**Honest quality assessment (important):** IQ himself uses deliberately modest language — *"erosion-like effects"*, *"rich variety of shapes... flat areas as well as more rough areas."* He **never claims it produces drainage or river networks.** It is an *amplitude/roughness modulator*, not a structure generator. It makes noise look *less uniformly noisy* (which is real and worth having) but it does **not** create connected, branching valley networks. It will not, by itself, beat your "looks like the same noise" problem on the structural axis — but it's a cheap, free additive improvement to the *texture* of whatever structure you do generate.

**(b) Slope-damped fBm / "erosion noise":** the generalized version of the above — modulate octave amplitude (or warp) by local slope/curvature. Same verdict: local, cheap, parity-safe, improves *plausibility of surface roughness distribution*, does not create drainage topology.

**(c) Heightmap-filter "erosion" (proceduralpixels.com "fastest erosion algorithm ever"):** a local windowed (7–20 texel) distance-to-threshold/slab operator. The author is refreshingly honest: it reproduces **smoothing, sediment-down-slope, talus/sandy slopes**, but explicitly *"works only on a heightmap — doesn't produce water flow velocity"* and the *"local window size can't capture regional flow patterns."* It's *"not physically accurate but believable for gamedev."* Confirms the pattern: **local window → local erosion features only.**

**Pattern across all three: every local fake reproduces SHORT-RANGE erosion (diffusion, talus, slope-conditioned roughness, smoothed valleys) and NONE reproduce LONG-RANGE drainage.** This is not a coincidence — it's the receptive-field limit (§3, §5).

---

## 3. Can DRAINAGE / FLOW ACCUMULATION be a local analytic function? — The core impossibility

**No, not as a pure `f(x,z)` with bounded reads. Here's the honest proof-sketch:**

Flow accumulation A(x,z) = "how many upstream cells drain through here" is, by definition, *a global integral over the upstream contributing area* (Wikipedia "Flow accumulation"; multiple GIS sources: *"Each flow accumulation value is determined by all upstream cells"*). Computing it requires:
1. flow direction at every cell (local — D8/D-infinity/MFD), then
2. **accumulation along the entire upstream network** (global).

The canonical efficient algorithm — **Braun & Willett 2013, "A very efficient O(n), implicit and parallel method to solve the stream power equation"** — makes the dependency structure explicit: it builds a **receiver/donor tree** over the whole grid, computes a **topological ordering** (stack), and accumulates drainage area / solves incision *in that order*. It's O(n) but it is fundamentally a **global ordered traversal of the entire domain**, not a local stencil. You cannot start in the middle and know your answer — you must have processed everything upstream first. That ordering IS the long-range dependency, formalized.

GPU flow-accumulation work confirms this is the hard part: algorithms decompose into "pixel-independent / short-pixel" ops (easy, parallel) and **"iterative long-pixel (LP) operations"** (the accumulation, which is inherently sequential/iterative). You get speedups via topological sort, but you never get rid of the global traversal.

**So a purely local operator literally cannot know its upstream area.** This is the wall the owner is bumping into, and it is real, not a tooling limitation.

**The escapes (and which are viable for WG10):**

- **❌ Bounded upstream window:** "just look N cells uphill." Fails — drainage area is unbounded; a major river's basin is hundreds of km. Any finite window truncates the basin and gives wrong (and *discontinuous at the window edge* → tile-seam) results. This is exactly the tile-boundary failure mode reported in the streaming-erosion literature: *"each tile starts dry... water crossing boundaries isn't tracked... huge gaps between tiles."*

- **✅ Precomputed coarse flow field + local detail (THE viable escape):** compute drainage **once, cheaply, at a COARSE resolution** where the whole relevant domain fits, store a low-res `flow/discharge field` (and/or distance-to-channel), then have the per-pixel local operator *read the coarse field* (a bounded number of texture taps) and add high-frequency detail. The coarse field *carries the long-range information* so the local operator doesn't have to derive it. This is precisely the structure of **Cordonnier et al. 2016 "Large Scale Terrain Generation from Tectonic Uplift and Fluvial Erosion"**: build a stream graph over the whole domain (global), then *blend landform feature kernels* locally to make the DEM. It is also the structure of FNO landscape-evolution learning (§4) where the global receptive field carries A.

- **✅✅ Hierarchical / clipmap flow (the WG10-native version of the above):** Your render is *already a clipmap with coarse levels covering huge extents*. Run the (cheap, global-but-tiny) flow/drainage computation on the **coarsest clipmap levels** — there the "whole map" is only a small grid, so a global pass is affordable and even infinite-compatible if you make the coarse field itself a deterministic function of a coarse procedural base. Then finer levels read the coarse drainage field and carve local detail. **The clipmap hierarchy is the natural carrier of the long-range dependency.** Your memory notes already flag "coarse-span / coarsest-first" as the scheduler's correctness principle — the *same coarse-carries-the-structure principle applies to drainage.* This is the single most important architectural insight in this brief.

The unavoidable caveat for *infinite* terrain: drainage area is genuinely unbounded, so true infinity means even the "coarse" level can't contain a continental basin. The honest resolution is to **cap the basin scale**: decide a maximum drainage extent (say, one coarse clipmap footprint), compute deterministic flow within that bounded-but-large window, and accept that you model rivers up to that scale and no larger. This is a *design choice that trades true-infinite-basins for tractability* — and it's the same choice every shipping open-world game makes implicitly.

---

## 4. Erosion-via-learned-local-operator: what the literature actually shows

**The owner's distillation idea (offline-run real erosion → fit a cheap local operator on local features) is a real, published research direction — with one consistent caveat that maps exactly onto §3.**

- **Guérin et al. 2017, "Interactive Example-Based Terrain Authoring with Conditional Generative Adversarial Networks" (SIGGRAPH Asia / ACM TOG).** pix2pix-style cGAN trained on real DEMs paired with sketch/feature maps (rivers, valleys, ridges, level sets). Crucially it includes a separate **"erosion synthesizer" that generates erosion evolution at very low cost** — i.e., a learned approximation of expensive simulation. *This is exactly the distillation the owner proposed, and it works.* BUT: it's a CNN (bounded receptive field), it's *conditioned on a sketch/feature map that supplies the global structure* (the user draws where rivers go), and it operates on bounded patches — the long-range structure comes from the *conditioning input*, not from the network inventing drainage from scratch. (Same lineage: **Argudo et al. 2018 "Terrain Super-resolution through Aerial Imagery and Fully Convolutional Networks"**; the **"Terrain Amplification using Multi Scale Erosion"** TOG 2024 line; **Lochner et al. 2023 "Interactive Authoring of Terrain using Diffusion Models."**)

- **"Learning how landscapes evolve with neural operators" (ESurf 2025).** Learns the stream-power evolution operator (elevation_t → elevation_t+1) — and it uses **Fourier Neural Operators specifically because they have a GLOBAL receptive field.** The authors state plainly: drainage area A is *"an inherently global spatial dependency"* and the FNO captures it via Fourier (spectral) convolutions that mix information globally. **"Enhancing Fourier Neural Operators with Local Spatial Features" (arXiv 2503.17797)** spells out the tradeoff: FNOs use global spectral convs *because purely spatial/convolutional operators have a bounded receptive field that cannot efficiently capture global interactions.* In plain terms: **the ML literature independently confirms that to learn erosion's drainage behaviour you need a global (or at least multi-scale) receptive field; a small local stencil/MLP provably cannot capture it.**

- **dandrino "terrain-erosion-3-ways":** trains a Progressive-Growing GAN on ~180k real USGS DEM tiles → output *"basically indistinguishable from real terrain,"* but *"very little control,"* expensive training, and **doesn't scale to higher resolution / doesn't tile seamlessly.**

**What IS local-izable from this literature (and matches your assets):** a small learned operator that takes **local features (slope, curvature, aspect) PLUS a coarse-flow/discharge tap** and outputs an **incision/deposition displacement** is feasible and is essentially what Cordonnier's kernel-blending + Guérin's erosion-synthesizer do. The learned part handles the *local response*; the *coarse-flow input* (§3) handles the *long-range part*. A learned operator with **only** local inputs (no coarse flow) will reproduce thermal/talus/roughness but not drainage — the receptive-field theorem again.

**Relevance to your 115-DEM / 12-biome program:** distill *two different things* per biome: (1) **local style** — slope→roughness response, talus angle, curvature→incision profile — these a local operator/MLP can carry, and these are what give a biome its *surface character*; (2) **structural parameters** — drainage density, channel concavity (the m/n of stream power), branching/bifurcation ratio (Horton/Strahler stats) — these parameterize the *coarse flow field generator*, not the per-pixel op. This split is the actionable form of "distill the DNA."

---

## 5. Honest feasibility verdict — exactly where the CAN/CANNOT line sits

| Erosion phenomenon | Range | Local operator? | Evidence |
|---|---|---|---|
| Thermal/talus relaxation (material slumps to repose angle) | strictly local (immediate neighbors) | ✅ **YES** — even fixed-iteration GLSL/Rust parity | Jákó 2011; thermal step is 4-neighbor only |
| Slope-damped roughness (flat valleys, rough peaks) | local | ✅ **YES** — IQ derivative trick, ~free | iquilezles morenoise |
| Diffusive smoothing / hillslope creep | local | ✅ **YES** — small fixed stencil | proceduralpixels; curvature diffusion |
| Local incision *given* you know you're in a channel | local IF channel/flow is supplied | ✅ **CONDITIONAL** — needs coarse-flow input | Cordonnier kernel-blend; Guérin erosion synth |
| Drainage **network topology** (branching dendritic rivers, correct concavity) | **global** (∝ upstream area A) | ❌ **NO** as pure f(x,z); ✅ only via coarse-flow injection | Stream power E=KA^mS^n; Braun-Willett global ordering; FNO needs global receptive field |
| Long-profile river grading / base-level connection | **global** | ❌ **NO** locally | flow accumulation = global integral |

**The line is sharp and it is the upstream-drainage-area term A.** Anything in erosion that does NOT depend on A (thermal, diffusion, slope-conditioned roughness, talus) → **local, parity-able, cheap, do it now.** Anything that DOES depend on A (the actual river/canyon drainage network that makes terrain "read as real") → **cannot be a pure local function**, full stop, by the math.

**The owner's thesis is correct AND has a concrete resolution.** "Real connected drainage needs global computation, in tension with local+infinite" — true. The resolution is **not** to find a magic local function that derives drainage (impossible), nor to give up. It is: **move the global computation to the coarse clipmap level where it is cheap and bounded, bake it into a coarse flow/discharge/distance-to-channel field, and make the per-pixel local operator a CONSUMER of that field plus local features.** The local operator then legitimately produces drainage-consistent incision because the long-range information was handed to it. This is what Cordonnier (graphics), Guérin (learning), and the FNO work (geoscience) all do, each in their own vocabulary.

**Concrete recommendation, ranked:**
1. **Beat the ridged-noise bar with a coarse drainage field, not a better local noise.** On coarse clipmap levels, compute a deterministic flow/discharge field (Braun-Willett-style ordering over the bounded coarse grid, or even a cheaper hierarchical D8 accumulation). Finer levels sample it and carve channels whose depth/width ∝ A^p — this gives *connected, branching, area-scaled valleys*, which is the thing ridged-noise fundamentally cannot fake (ridged noise has no connectivity or area-scaling).
2. **Add the IQ derivative-damping + a fixed-iteration thermal/talus relax** as the cheap local surface-character layer on top (per-biome talus angle, per-biome roughness response). Free-ish, parity-trivial, real improvement.
3. **Distillation done right:** fit the *local* response (slope/curvature/coarse-flow → displacement) as a tiny operator from offline-simulated terrain; fit the *structural* stats (drainage density, concavity m/n, bifurcation ratio) as parameters of the coarse-flow generator. Do NOT try to fit a local-only operator to reproduce drainage — the receptive-field theorem says it will fail, and that's why a pure local stencil/MLP on local features alone keeps coming out looking like noise.

**Why this won't repeat your dead ends:** it does not touch power spectra (it builds *connectivity/phase* structure explicitly via the flow graph — the exact thing FFT discarded); it does not rely on structure-tensor/dominant-wavelength scalars (it uses an actual computed flow field, not a single coherence number); it does not tile/sample DEM pixels (the DEMs only calibrate biome parameters offline). The one new dependency it accepts — a coarse global pass — is exactly the dependency the math proves is unavoidable, placed where your clipmap architecture already makes it cheap.

---

### Key sources
- Mei, Decaudin, Hu, "Fast Hydraulic Erosion Simulation and Visualization on GPU," Pacific Graphics 2007 — pipe model, per-step 4-neighbor locality.
- Jákó, "Fast Hydraulic and Thermal Erosion on the GPU," CESCG 2011 — thermal = local.
- Quilez, "Better/more noise" iquilezles.org/articles/morenoise/ — derivative-damped fBm; "erosion-like," not drainage.
- Wikipedia "Stream power law" + "Flow accumulation" — E=KA^mS^n; A = global upstream integral.
- Braun & Willett 2013, "A very efficient O(n)... method to solve the stream power equation" — global receiver/donor topological ordering.
- Cordonnier et al. 2016, "Large Scale Terrain Generation from Tectonic Uplift and Fluvial Erosion" — global stream graph → local kernel blend (the coarse+local pattern).
- Guérin et al. 2017, "Interactive Example-Based Terrain Authoring with Conditional GANs" — learned erosion synthesizer, conditioned on structure.
- "Learning how landscapes evolve with neural operators," ESurf 2025 + "Enhancing FNOs with Local Spatial Features," arXiv 2503.17797 — FNOs need GLOBAL receptive field for drainage; local convs can't.
- dandrino "terrain-erosion-3-ways"; proceduralpixels "fastest erosion algorithm"; Frozen Fractal "Hydraulic erosion" devlog — practitioner confirmation that local fakes give local features only and droplets cross tiles.


========================================================================
## AGENT agent-af7fe3be3a432394b.jsonl  (final text 17167 chars)
========================================================================

I have comprehensive, cross-verified coverage across all five angles. Synthesizing the technical brief now.

---

# TECHNICAL BRIEF: How infinite engines fake erosion-grade structure without global simulation

## Verdict up front (the honest categorization, your question #4)

Across every published infinite/streaming engine surveyed, **none do real hydraulic erosion at runtime.** Real erosion is global-iterative-self-reinforcing by definition (the amount of erosion at a point depends on upstream **drainage area** — the total area uphill of it — which cannot be known from a local neighborhood). Multiple independent sources state this plainly: "it is not possible to split the world into chunks... when applying hydraulic erosion"; erosion is "self-reinforcing — if by random chance a slight valley forms, it'll collect slightly more water... causing it to erode more quickly." So runtime structure is ALWAYS one of:

- **(a) cleverly-shaped noise** — NMS, Outerra's fractal detail, Star Citizen base terrain, iq derivative-erosion, runevision erosion-noise, Grenier phasor-noise. **This is where the live answer to your problem lives.**
- **(b) offline-authored / offline-baked heightmaps streamed in** — Star Citizen "directed" ecosystems, Frozen Fractal's 1.5s bake, anyone who runs droplet sim then ships the result. Erosion that is genuinely simulated is always (b).
- **(c) splines/curves applied to noise** — Minecraft 1.18, redblobgames redistribution. This is a *combination/redistribution* layer, not a structure-generator.

Your owner's thesis ("offline-learn erosion → cheap local online operator") is essentially trying to invent a principled **(a)** that carries **(b)**'s structure. The good news: two recent papers do almost exactly that, and one 2026 blog technique is the closest practical thing to what you want. Details below.

---

## 1. No Man's Sky — "uber noise," analytic derivatives, density-function (not heightmap) terrain

Two GDC 2017 talks: Sean Murray, *Building Worlds in No Man's Sky Using Math(s)*; Innes McKendrick, *Continuous World Generation in 'No Man's Sky'*.

What is actually established:
- **3D density function, not a heightmap.** NMS terrain is a signed density field `f(x,y,z) -> density`, polygonized (voxel/marching-cubes-family). This is *why* they get caves, overhangs, arches — things a 2D heightmap `f(x,z)` physically cannot represent. **This is the single biggest structural lever you are leaving on the table by being heightmap-only.** Connected overhang/cliff/arch structure reads as "geological" precisely because it's impossible in a height field.
- **"Uber noise"** is their name for a single fused multi-octave noise that bakes ridged + billow + domain-warp + slope/altitude-based amplitude damping into one analytic-derivative function — not a stack of separate passes. Confirmed parameter families from the uber-noise lineage (Shadertoy `NstfWs`, and the technique writeups): per-octave **gain, lacunarity, sharpness/ridge-offset** (blends smooth↔billow↔ridged), **slope_erosion** (damps octave amplitude where accumulated gradient is steep), **altitude_erosion**, **feature amplitude/damping**, and **domain perturb (warp)**. It's a **single local per-point function with a fixed octave loop** — exactly your parity/cheap constraints.
- **No runtime erosion sim.** Geology-reading-ness comes from (i) the 3D density formulation, (ii) analytic-derivative slope-erosion inside the noise, (iii) domain warp. They also blend real DEM data in places (matches your kernel-DNA instinct, but as bias not tiling).

LOCAL: yes. PARITY: yes (value/gradient noise + add/mul/div/abs/clamp/dot, fixed loop). CHEAP: yes. REAL STRUCTURE: **the 3D density part yes** (overhangs are genuine structure); the 2D noise part is still believable-at-a-glance, same class as what you have.

**Takeaway for you:** your generator is `f(x,z)`. NMS's "looks geological" is substantially a `f(x,y,z)` artifact. Even partial 3D (a density-field skin near cliffs/ridgelines for overhangs) would buy structure noise cannot.

---

## 2 & 3. Minecraft 1.18 — splines-of-noise, and exactly how "erosion" is a pure local function

This is the most directly transplantable idea and it's worth being precise because it answers "could biome params BE splines?" with **yes**.

The mechanism (verified across Minecraft Wiki, the terrain-shaper docs, and Kniberg's "in a nutshell"):
- Several **independent low-frequency 2D noise channels**: `continentalness`, `erosion`, `weirdness` (→ folded into `peaks_and_valleys`/`ridges`), plus temperature/humidity for biomes only.
- Each channel value (range −1..1) is fed as the **X-axis input to a spline**; the spline's Y output is a terrain quantity. Three output splines: **offset** (base height), **factor** (vertical squash / how strongly 3D noise is allowed to deviate), **jaggedness** (peak roughness amplitude).
- **Splines are nested/2D**: a control point's *value* can itself be another spline keyed on a different channel. So the real function is `offset = spline_cont( c, spline_ero( e, spline_pv(pv) ) )` — a small piecewise-linear cascade.
- **"Erosion" is NOT a simulation.** It is literally one more low-freq noise channel run through a spline: high erosion → spline outputs low offset + low factor → **flat low terrain**; low erosion → high offset + high factor → **mountains**. The *name* is evocative; the *implementation* is "a curve applied to a noise sample." Fully local per-column, no neighbor reads, no passes. Then `final_density = f(offset, factor, depth_gradient, 3D_noise)` makes it 3D (cheese/noodle caves, overhangs).

Why splines-of-noise read as coherent terrain types cheaply: a spline is a **sharp, monotone-or-folded redistribution**. Plain fBm gives you a unimodal blur; a spline lets one noise value snap into discrete *regimes* (ocean shelf → coast → plateau → cliff → mountain) with steep transitions noise can't produce on its own, while two independent channels (continentalness × erosion) give a **2D regime grid** so the same height can arise different ways → variety without self-similarity. This is exactly the redblobgames `pow(e, exponent)` redistribution idea generalized to arbitrary control-point curves and multiple independent inputs.

LOCAL: yes (per-column + depth). PARITY: yes (piecewise-linear lerp = trivial in Rust+GLSL). CHEAP: yes (a few noise samples + a few lerps). REAL STRUCTURE: **partial** — splines give *coherent terrain-type transitions and cliffs/plateaus* (structure your current generator lacks), but **not branching drainage**. Minecraft has no rivers-as-erosion; its "rivers" are just the low band of the PV spline.

**Direct recommendation for your distillation:** This is the fix for your dead-end #2. Your structure-tensor coherence and dominant-wavelength came out dead-constant because they measure the *wrong thing* (single-scalar field statistics). Instead, distill each biome DEM into **spline control points**, not scalars: fit `height-vs-continentalness` and especially `height-vs-erosion-proxy` and `slope-vs-elevation` curves per biome family. Those curves DO vary across your 12 families (a karst plateau's height-vs-position curve is a step function; a grassland's is nearly flat; a mountain's is steep-and-folded) where a single coherence number does not. **Biome params should be splines, not 9 scalars.**

---

## 4 (the core question). The four LOCAL erosion-faking techniques that actually beat ridged-valley noise

These are the live candidates, ranked by how much real structure they add per unit cost/risk. All are `f(x,z)`, parity-able, cheap.

### A. iq analytic-derivative fBm erosion ("swiss"/derivative-damped fBm) — DO THIS FIRST, it's nearly free
Inigo Quilez, *morenoise* / *fbm*. The entire trick is two lines inside your existing octave loop:
```
a += b * n.x / (1.0 + dot(d, d));   // n.x = noise value, b = octave amp
d += n.yz;                          // n.yz = analytic gradient of this octave
```
Each octave's contribution is **damped by the accumulated slope so far**. Effect: lowlands stay smooth/flat, detail concentrates on already-steep faces → **flat valley floors + sharp ridges**, the visual signature of erosion. Verified: per-point, fixed loop (~8–15 octaves), uses only value/gradient-noise + add/mul/div/dot — trivially identical in Rust and GLSL. **This is strictly better than your current "subtract ridged-noise valley" and costs ~3 extra ALU ops/octave**, provided your noise returns its analytic derivative (value/gradient noise does). Caveat: it's *believable-at-a-glance erosion texture*, not a connected drainage network. But it directly attacks "looks like the same noise."

### B. runevision "Fast and Gorgeous Erosion Filter" (2026) — the closest thing to what you actually want
Rune Skovbo Johansen, blog 2026-03. Explicitly engineered for your exact constraints — quote: produces "branching gullies and ridges, while still allowing every point to be evaluated in isolation... fast, GPU-friendly, and trivial to generate in chunks," and was motivated by "avoid anything simulated because then you cannot easily make infinite terrains."
Mechanism (verified): Worley-like **cells, each with a random pivot**; within a cell, **ridged stripe waves are extruded along the local gradient direction**; **each successive octave recomputes stripe direction from the gradient modified by previous octaves** → smaller gullies branch off larger ones → **hierarchical branching that mimics drainage**. ~5 octaves, deterministic `f(x,z)`, single pass.
This is the one technique surveyed that produces **branching connected gully structure** locally and cheaply. **It beats ridged-noise valleys.**
Honest limitation the author admits: **no real flow accumulation** — gullies sometimes "just stop halfway down rather than following through," and they don't connect into a true global drainage network (because that *is* global). So it's "drainage-shaped," not drainage-correct. For a game framework, that's almost certainly enough.

### C. Grenier et al. 2024, *Real-time Terrain Enhancement with Controlled Procedural Patterns* (Computer Graphics Forum / Eurographics) — the academic version of B
Uses **Phasor noise kernels aligned to the terrain's slope/flow direction** to add **spatially-varying, slope-aligned erosion ravines** in real time on GPU, cascadable (narrow gullies nested in large ones), controlled by a palette of control maps. Verified: synthesizes detail "aligned with the slope of the terrain" vs isotropic noise; "efficiently implemented in graphics hardware for real-time synthesis." Same family as B but with a principled anisotropic-noise basis and artist control maps (which could be your per-biome params). LOCAL/PARITY/CHEAP: yes (phasor noise is a fixed-cost per-point evaluation); slightly more math than B.

### D. Domain-warp + flow-alignment generally
Your generator already domain-warps. The upgrade is **anisotropic, slope-aligned warp** (warp *along* the gradient, not isotropically) — this is the common kernel under both B and C and is what turns "wiggly noise" into "things that flow downhill." Cheap, parity-able, but on its own only believable-at-a-glance.

---

## The two techniques that match your owner's "learn offline, apply local" thesis — and the honest verdict on each

**Guérin et al. 2016, *Sparse representation of terrains*** (Eurographics; code at github.com/eric-guerin/terrain-amplification). This is the literal published instance of "learn a dictionary from real DEMs offline, synthesize new terrain as a sparse combination of those learned atoms." It **carries real structure** (it reconstructs ridges and erosion patterns from the dictionary, i.e. it preserves *phase/structure*, defeating your spectral dead-end #1). **But** synthesis is a **sparse-coding optimization (matching-pursuit), which is iterative and not a closed-form `f(x,z)`** — it's an **offline/bake** amplification, not a per-pixel shader eval. So it fits your "offline-learn" half but **violates the cheap/local online half** as published. Viable only if you bake per-page and stream (category b), not as a live density function.

**Terrain Diffusion (arXiv 2512.08309, 2026), "A Diffusion-Based Successor to Perlin Noise."** Strikingly on-target: trained offline on real DEMs (90m MERIT + ETOPO1), it provides **InfiniteDiffusion** — lazy, tile-based-with-overlap, **deterministic `f(x,z,seed)` with proven seed-consistency independent of query order**, producing **branching valleys and mountain ranges that noise cannot** (explicitly framed as solving the "noise can't make branching valleys" problem = your phase problem). Per-512×512-tile cost ~0.66 s on an RTX 3090 Ti, "9× faster than orbital velocity." **This is the strongest published proof that "offline-learn-structure → deterministic local-ish online generation" works and beats noise.** Caveats for you: (1) it's a **neural net, not GLSL/Rust value-noise** — no CPU/GPU *parity* in your sense, and collision/facts on CPU would need a separate cheap path; (2) 0.66s/tile is per-page-bake territory, not per-pixel-per-frame; (3) it's category (a)+(b) hybrid. But it directly validates the owner's core bet and is the thing to watch / prototype as the page-bake layer.

---

## Bottom line / recommended path (in order)

1. **Drop the 9 scalars; distill biomes into SPLINES** (Minecraft model): per-family curves `offset(continentalness)`, `factor/flatness(erosion-proxy)`, `slope(elevation)`, `jaggedness(pv)`. These vary across your 12 families where coherence/wavelength scalars didn't. Pure local, trivial parity. Fixes dead-end #2.
2. **Swap your subtractive ridged-valley for iq derivative-damped fBm** (technique A). Near-free, immediately attacks "looks like noise," gives flat valleys + sharp crests.
3. **Add runevision-style slope-aligned branching-gully noise** (technique B, or Grenier phasor noise C) as the drainage layer. This is the one that genuinely *beats* ridged-noise valleys with branching structure, while staying local+cheap+parity-able. Accept that gullies are drainage-*shaped*, not globally-connected — that's the price of local+infinite, and every shipping engine pays it.
4. **Go partially 3D where it counts** (NMS lesson): a local density-field skin on cliff/ridge faces buys overhangs/arches — real structure a height field cannot express — for the "reads as geological" win.
5. **Keep the owner's offline-learn bet alive as a page-BAKE layer, not a per-pixel one**: Guérin sparse-amplification (carries real DEM structure) or a Terrain-Diffusion-style learned prior, baked per page and streamed, with the cheap local A+B+C stack as the always-available, parity-correct base. This is the only honest way to get true learned drainage structure given that real erosion is irreducibly global.

## Key sources
- No Man's Sky: [Murray, GDC 2017 "Building Worlds Using Math(s)"](https://www.gdcvault.com/play/1024514/Building-Worlds-Using); [McKendrick, GDC 2017 "Continuous World Generation"](https://www.gdcvault.com/play/1024265/Continuous-World-Generation-in-No); [uber-noise technique writeup](https://ithy.com/article/innovations-procedural-noise-terrain-35cvalyh); [Uber noise Shadertoy NstfWs](https://www.shadertoy.com/view/NstfWs)
- Minecraft 1.18 splines: [Minecraft Wiki World generation](https://minecraft.wiki/w/World_generation); [Noise router](https://minecraft.wiki/w/Noise_router); [terrain-shaper offset/factor/jaggedness](https://www.steamnews.org/articles/technology/minecraft-procedural-world-terrain-generation); [Kniberg "terrain generation in a nutshell"](https://x.com/henrikkniberg/status/1490449049002123265)
- iq analytic-derivative erosion: [morenoise](https://iquilezles.org/articles/morenoise/); [fbm](https://iquilezles.org/articles/fbm/); [Rainforest shader](https://www.shadertoy.com/view/4ttSWf)
- Local branching-gully erosion noise: [runevision "Fast and Gorgeous Erosion Filter" (2026)](https://blog.runevision.com/2026/03/fast-and-gorgeous-erosion-filter.html); [Grenier et al. 2024 phasor terrain](https://onlinelibrary.wiley.com/doi/10.1111/cgf.14992) ([HAL open copy](https://udl.hal.science/hal-04360714v2))
- Redistribution/curves: [redblobgames terrain from noise](https://www.redblobgames.com/maps/terrain-from-noise/)
- Erosion is global (why runtime sim is impossible): [Frozen Fractal hydraulic erosion](https://frozenfractal.com/blog/2025/6/6/around-the-world-23-hydraulic-erosion/); [Ivo van der Veen, can't chunk erosion](https://medium.com/@ivo.thom.vanderveen/improved-terrain-generation-using-hydraulic-erosion-2adda8e3d99b)
- Offline-learn → apply: [Guérin et al. 2016 sparse terrain representation](https://hal.science/hal-01258986) ([code](https://github.com/eric-guerin/terrain-amplification)); [Terrain Diffusion, arXiv 2512.08309](https://arxiv.org/html/2512.08309v3); [terrain-erosion-3-ways (GAN is global, not local)](https://github.com/dandrino/terrain-erosion-3-ways)
- Outerra / Star Citizen (real-DEM-base + fractal detail; "procedural but directed" hybrid): [Outerra Grokipedia](https://grokipedia.com/page/outerra); [Star Citizen Planet Tech](https://starcitizen.tools/Planet_Tech_v1)


plus The current frame is too weak: DEM -> a few scalar params -> local warped-noise height function will not reliably produce real terrain structure. It can match roughness, slope histograms, relief, and maybe “mountain-ish” texture. It will not create the organized phase/topology that makes geography read as real: connected ridgelines, nested drainage basins, tributary hierarchy, downstream widening, ridge/valley spacing, and basin-scale anisotropy.

The better frame is:

DEM -> biome morphology distributions -> deterministic hydrology/structure scaffold -> local procedural detail

Noise should become the residual/detail layer, not the source of landform structure.

Why Warped Noise Hits A Wall

Ridged multifractals and domain warping are useful, but they remain local stochastic fields. Musgrave’s classic fractal terrain work already treated fBm-style terrain as a first approximation, then added erosion to get stream/valley networks. The key limitation is that plain fractal surfaces have broadly stationary statistical character unless heavily modulated.

Recent terrain-enhancement work states the same problem bluntly: procedural fBm/noise is efficient and local, but most noise combinations fail at natural landforms because they lack structure and topographic foundation.

So your owner’s “all looks like the same noise” verdict is technically coherent. The generator is producing terrain-colored noise, not geography.

What Actually Fits The Constraints

Use a structural scaffold first:

Generate deterministic large-scale uplift/range fields.
Generate ridge and drainage graphs or curve skeletons from hashed macro-cells.
Build height from distance-to-ridge, distance-to-channel, basin position, and valley cross-section profiles.
Add erosion-like local detail with derivative-aware noises: Swiss/Jordan turbulence, oriented phasor patterns, anisotropic ridged noise, small fBm.
Let biome params control graph density, valley shape, branching, roughness, and erosion-detail strength.
This matches the better research direction. Génevaux et al. generate terrain from hierarchical drainage networks, using rivers as modeling elements and an analytic continuous representation rather than one monolithic noise function. Cordonnier et al. combine uplift with stream-power erosion, then convert stream-graph information into terrain kernels, explicitly targeting dendritic rivers, watersheds, and mountain ridges.

Runtime Spec Direction

Make the runtime height function roughly:

h(p) =
  uplift_macro(p)
+ ridge_sdf_profile(p, biome)
- valley_sdf_carve(p, biome, channel_order_proxy)
+ slope_aligned_erosion_detail(p, grad_proxy, biome)
+ residual_noise(p, biome)
Important: ridge_sdf_profile and valley_sdf_carve should come from deterministic procedural features, not raster DEM kernels. For each point, query a bounded neighborhood of macro feature cells, for example 3x3 or 5x5 hashed cells. That keeps it infinite, deterministic, CPU/GPU-portable, and fixed-cost enough.

This is still “local” in the engineering sense, but not “one stationary noise function.” It is a bounded procedural scene graph.

DEM Measurements To Extract

Stop treating DEMs mainly as spectral/noise targets. Extract structural distributions per biome:

Hypsometry: curve, integral, skew/kurtosis; not just min/max relief.
Multi-scale roughness: slope, curvature, TPI, local relief at physical radii, not pixel radii.
Drainage: D8/D-infinity flow accumulation, drainage density, channel-initiation threshold, length-to-channel, Strahler order distribution.
Network laws: branching ratio, Hack’s law, slope-area/concavity, stream-power index.
Ridge/valley geometry: ridge-valley spacing, ridge continuity, valley width/depth, V vs U cross-section, asymmetry, junction angles.
Anisotropy: dominant ridge/range orientation, orientation dispersion, elongated vs radial basins.
Biome-specific flags: glacial U-valleys/cirques, volcanic cones/calderas/radial gullies, badlands high drainage density, karst depressions, wetlands low HAND/flatness.
Restore real metres before measuring slope or curvature. A z-scored DEM without pixel spacing and vertical range will hide exactly the physical cues you need.

Geomorphometry literature supports this broader attribute set: slope/aspect/curvatures, catchment area, topographic index, and stream-power index are standard DEM-derived terrain attributes. Drainage density and hillslope length-to-channel are also explicit DEM texture measures.

Distilled Erosion

Yes, the owner’s “distilled erosion” idea is the right research bet, with one caveat: do not expect a purely local learned operator to invent basin topology. Use offline erosion to learn the residual operator after a structural scaffold exists.

Practical path:

Generate scaffold terrain.
Offline-run hydraulic/thermal/stream-power erosion on samples.
Learn or fit delta_h = f(local slope, curvature, flow-proxy, distance-to-channel, distance-to-ridge, biome params, noise seed).
Runtime uses a small analytic/spline/table operator, not a large NN, unless parity/perf budgets allow it.
GPU hydraulic erosion is possible but still simulation/stateful; classic GPU papers use shallow-water or iterative erosion models, not single cheap point queries. Newer analytical erosion work is promising, but it still uses multigrid/iterative machinery for 2D terrain, so I’d treat it as an offline teacher or authoring tool, not the final per-pixel runtime.

No Man’s Sky Lesson

Do not use No Man’s Sky as proof that local noise can solve real geomorphology. The public GDC abstract describes continuous voxel world generation, polygonization, texturing, population, and simulation, but not a magic local erosion solution. The production lesson is more likely: accept stylization, use many procedural systems together, and avoid depending on fully realistic drainage everywhere.

Recommendation

Keep the DEM library, but change what it trains. The deliverable should be a BiomeMorphologyParams system plus a deterministic feature scaffold.

Noise-only should be marked as failed for “real geography.” The next milestone should be one biome, probably mountains or badlands, generated from:

procedural ridge/channel skeletons,
distance-field valley carving,
biome-measured drainage density and ridge-valley spacing,
derivative-aware erosion detail,
validation against held-out DEM structural metrics.
That gives you a spec path that preserves infinite/seamless/parity constraints while attacking the actual missing thing: connected terrain structure.





5:25 PM