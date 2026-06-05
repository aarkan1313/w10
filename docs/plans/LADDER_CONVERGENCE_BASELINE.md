# Ladder convergence baseline (measured 2026-06-04)

Source: `tools/dem_pack/test_mountain_world_layer_contract.py`
(`test_live_seamsafe_mountain_page_is_not_yet_the_accepted_network_layer`)

## Offline-measured gap (NORMALIZED units, NOT metres)

Live seam-safe mountain recipe vs accepted conditioned payload, over the shared
mapped page (display origin 0,0; `SAMPLE_N` grid):

- mean_abs = 1.211743
- p95_abs  = 2.276974
- peak_abs = 3.200543
- corr     = -0.048456
- ref_ptp  = 1.584039   (accepted *conditioned* field range — ~[0,1.58], normalized)
- live_ptp = 4.914207   (live recipe internal normalized range)

## CRITICAL UNIT CAVEAT (caught during execution, 2026-06-04)

These numbers are in the recipe's **normalized/conditioned space, not metres**:
- `reference` here = `layer.sample_payload_page(...)` = the CONDITIONED payload
  field (percentile/tanh normalized, ptp ~1.58).
- `live` here = `mountain.generate(...)["height"]` = the recipe's internal
  normalized height (ptp ~4.9), BEFORE the runtime's metre/relief scaling.

The test's PURPOSE is to prove the gap EXISTS (it asserts `corr < 0.80`,
`mean_abs > 0.20`), i.e. "the live seam-safe recipe is NOT the conditioned
network layer." It is a *gap-exists* proof, not a *convergence target*.

## Consequence for the ladder Rung 1 gate

The windowed ladder reads back R32F page textures in **scaled metres**:
- The live runtime page (`compute_biome_page_cached`) is in metres (relief-scaled).
- The baked REFERENCE the runtime streams (`configure_static_reference`) is also
  in metres, sampled from the conditioned payload.

So the windowed comparison is **metres-vs-metres**, a DIFFERENT domain from the
offline normalized number above. Therefore:

- DO NOT use `mean_abs=1.21` (normalized) as the Rung 1 metres budget — that is
  an apples-to-oranges comparison and would make a green gate meaningless.
- INSTEAD: Rung 1's gate computes its OWN metres-domain convergence on the first
  run, prints it, and that measured value becomes the recorded baseline here.
  Thereafter Rung 1 is a "no regression vs this recorded metres baseline" gate
  (direction + no-regression policy, per the spec).
- The offline normalized number stays here ONLY as context: it tells us the
  recipe and the conditioned payload are genuinely different fields (corr≈0), so
  we should EXPECT a substantial metres gap at Rung 1 — likely a plateau until
  conditioning/pass-network become live facts (the spec's Rung 1 plateau risk).

## Rung 1 metres baseline (to be filled by the first Rung 1 gate run)

- mean_abs (metres) = <TBD: first measured windowed run>
- p95_abs  (metres) = <TBD>
- peak_abs (metres) = <TBD>
- Recorded: <date>
- Gate thereafter: live mean_abs <= recorded * 1.10 (10% slack for run-to-run
  f32/windowed sampling variance).
