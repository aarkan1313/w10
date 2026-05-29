# WorldGen10 — M1-Grammar Layer Design

**Date:** 2026-05-28
**Status:** Approved (brainstorming) — pending writing-plans
**Layer:** Region/province grammar + family selection + terrain-pack format v1
**Builds on:** the green hash/noise/fbm bedrock (`wg-10/rust/src/hash.rs`)
**Followed by:** kernel sampling + landform composition → actual height (next plan)

---

## 0. Framing: this is W10, not a port of W9

W9 (`d:/workflows/worldgen9`) is consulted **only** to understand the problem
(what a region grammar must produce) and as a loose **sanity oracle** (if our
variety numbers look insane, W9's `region_grammar_report.json` is a gut-check).
W9 is **not** a parity contract for this layer. The grammar is W10's own design,
optimized for the four pillars (DESIGN §1):

1. **Adaptable/tunable** (top) — pack-driven; retune terrain without code; drop
   into any game.
2. **Performance** — the formula also runs on GPU in M2, so the grammar math is
   GPU-shaped from the start.
3. **Quality** — bounded, seam-exact, no collapse to one palette.
4. **No shortcuts** — validate and reject; properties gated, not assumed.

> Note: the hash **bedrock** fixtures (`hash_reference.json`) still hold — that
> math is universal. Only W9's **grammar** outputs are not our target.

---

## 1. Scope

**In scope (this plan):** for any `(world_x, world_z, seed, pack)`, deterministically
produce a **bounded, normalized blend of terrain families** — i.e. *"what terrain
is here"*, as `(family_id, weight)` summing to 1. Plus the terrain-pack format v1
(spec + Rust loader + validation) that drives it, a thin Godot binding, and
property-based headless gates.

**Out of scope (next plan):** sampling actual height from kernel `.npy` data,
kernel transforms/moderation, landform composition, `height(x,z)`. This layer
outputs **weights only, no height**.

**Why this seam is clean (not an arbitrary chop):** the handoff between this
layer and the height layer is one small, stable value — a fixed-arity list of
`(family_id, weight)`. It is meaningful and testable on its own. Height sampling
can be rebuilt entirely without the grammar noticing; the grammar can be retuned
and height sampling just receives different weights.

---

## 2. Interface constraints — NON-NEGOTIABLE

These exist so the split does not box us in later. A future plan must not violate
them without revisiting this design.

1. **Bounded, GPU-shaped output.** The grammar returns **fixed-arity** family
   weights (no variable-length lists, no allocation in the hot path). This is what
   lets M2 port it to a compute shader unchanged. Flexibility via unbounded
   collections *is* the box — forbidden here.
2. **Family-source is a seam.** The grammar consumes "family set + bias for this
   region" through one narrow interface. Discrete **palettes** are the only
   implementation now. M6's continuous **climate-field** source must be able to
   drop in at this seam **without touching the blend math**. (This is the stubbed
   + noted M6 work — load-bearing, not decorative.)
3. **Versioned pack, read through a stable loader.** The on-disk format may evolve
   (schema string + version). The **in-memory interface** the grammar reads stays
   stable. The grammar never parses JSON itself. Spec only what this plan needs;
   leave documented extension points; do not guess future fields (YAGNI) and do
   not hardcode present ones.

### Named risk to revisit (do not decide now)

**Grammar↔kernel coupling.** Does the family blend need to know anything about
kernel properties to decide weights? In W9, kernel slope fed a "moderation"
factor. Current read: that belongs in the **height** layer (it modulates
contribution *amplitude*, not family *identity*), so the grammar stays free of
kernel data. Decide this on purpose when building the height layer. **If the
grammar ever reaches for kernel data, the seam moved** — stop and re-cut.

---

## 3. Terrain-pack format v1

A pack is a versioned JSON file (hand-authorable; matches the hash-fixture
precedent) plus the kernel assets it references. This plan loads only the
**grammar-relevant** parts; kernel-data fields are present but not loaded yet.

The on-disk file is **JSON**; the sketch below is shown in a YAML-like shorthand
for readability (the planning step writes the exact JSON golden fixture):

```
schema:  "worldgen10.terrain_pack.v1"     # versioned, validated on load
version: 1

grammar_constants:                        # the tunables (pillar 1)
  region_size_m:           32768
  province_size_regions:   4
  palette_primary_pct:     72             # palette-selection roll thresholds
  palette_compatible_pct:  22             # remainder => rare

palettes:                                 # each = exactly FAMILIES_PER_PALETTE (3)
  - id: "alpine"
    families: ["mountain", "glacial", "grassland"]
  - id: "drylands"
    families: ["badlands", "desert", "karst"]
  # ...
  compatibility:                          # compatible-neighbor palettes
    alpine: ["coastal_ridges", "humid_hills"]
    # ...

families:                                 # family-level data
  mountain:
    # grammar needs only that the id EXISTS and is referenced.
    # height-relevant fields (relief_scale_m, kernel_ids, ...) are PRESENT
    # in the pack but NOT loaded by the grammar layer (next plan loads them).
```

Decisions baked in:
- **`FAMILIES_PER_PALETTE = 3` (fixed).** Interface constraint #1 expressed in
  data. A palette with ≠3 families is a **load error**. This keeps the blend
  bounded/GPU-shaped.
- **Loader validates and rejects** (pillar 4): bad schema, unknown family
  referenced by a palette, out-of-range constants, wrong family arity → hard
  error at load, never a silent default.
- **Loader returns an in-memory `Pack` struct** the grammar reads through
  (constraint #3). The grammar never sees JSON.
- **Golden pack fixture (synthetic, W10-authored).** A small hand-authored pack
  (a few palettes/families with toy values) ships in-repo as deterministic test
  ground truth. A realistic starter pack using the real DEM family/palette names
  is deferred to the **height plan**, when kernels actually get loaded — we do not
  author kernel references we cannot yet exercise.

---

## 4. The grammar core (`grammar.rs`)

Pure Rust, no `godot` imports (like `hash.rs`), under the ~600-line cap. Per
sample `(x, z, seed, pack)`:

1. **Locate.** Floor `(x,z)` into a region cell (reusing the bedrock's
   floor-not-truncate seam safety); `province = region.div_euclid(province_size_regions)`.
2. **Decide per region.** `region_palette(region, seed)`: the province sets a
   primary palette; a deterministic roll picks primary / compatible / rare using
   the pack's `*_pct` thresholds. Pure `hash::stable_hash` calls.
3. **Family source (M6 seam, constraint #2).** `families_for(region) ->
   ([FamilyId; 3], [f64; 3] bias)`. One implementation now: palette-based.
4. **Blend.** 4 region corners, `smoothstep` weights (from `hash::smoothstep_unit`
   / `fade` as appropriate), accumulate into a **fixed-size** family→weight map,
   normalize so the total is exactly 1. Bounded arity, no allocation → constraint
   #1, GPU-ready.

**Output:** a small fixed structure of `(family_id, weight)` summing to 1 — the
stable interface the height plan consumes.

Module boundaries (each a unit you can understand/use/test in isolation):
- `pack.rs` — load + validate + in-memory `Pack` struct. (May split from
  `grammar.rs` to respect the 600-line cap; decide during planning.)
- `grammar.rs` — pure selection + blend; depends on `hash` + `Pack`.
- `bind_worldgen.rs` — the only Godot-facing file; exposes grammar for checks.

---

## 5. Binding + gates + done

**Binding.** Extend the thin `bind_worldgen.rs` to expose the grammar (load a
pack, query family weights at a coord) for a headless check. No math there.

**Gates are property-based** (derived from the pillars + DESIGN §4, NOT W9 values):
- weights **sum to 1.0** (exact) at every sample;
- **bounded** family count (never exceeds fixed arity);
- **determinism** — same `(x,z,seed,pack)` → identical weights across callers/runs;
- **seam-exactness** — continuous blend across `x=0`/`z=0` and region boundaries
  (no discontinuity at integer region edges);
- **variety sanity** — palettes vary across a region grid; no single-palette
  collapse (a loose threshold, sanity not parity);
- **loader rejects** malformed packs (each rejection case tested).

**Definition of done:** `cargo test` green; `tools/gate.py` suite green (extend
`fast` or add a suite); ROADMAP/STATUS updated; each task committed separately.
(The perf+visual+manual acceptance rule in DESIGN §7.3 applies to the render
pipeline, not this pure-math CPU layer.)

---

## 6. DESIGN.md updates this plan must make

- §3 (terrain packs): record that pack **v1** is defined (schema
  `worldgen10.terrain_pack.v1`), what it contains, and that it is versioned +
  validated. Note `FAMILIES_PER_PALETTE = 3` and why (GPU-shaped bound).
- Add the **family-source seam** and the **grammar↔kernel coupling** named risk
  to the open-items / architecture notes, so M6 and the height plan inherit them.
