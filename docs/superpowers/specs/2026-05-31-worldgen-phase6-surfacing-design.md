# WorldGen10 - Phase 6 Surfacing And Materials Design

**Date:** 2026-05-31
**Milestone:** Phase 6 - materials and surfacing.
**Status:** design-ready, implementation gated by Phase 5 owner-accepted live height.
**Parents:** `docs/plans/ROADMAP.md` Phase 6,
`docs/superpowers/specs/2026-05-30-shaded-terrain-at-scale-design.md`,
`docs/superpowers/specs/2026-05-30-m5-detail-masks-design.md`.

---

## 1. Purpose

Phase 6 turns an accepted height field into terrain that reads as AAA terrain under normal game viewing:
lighting, normals, material response, biome color, and dressing. It is not allowed to hide a bad height
algorithm. It is the layer that makes accepted geography legible.

Current owner direction is clear: do not spend implementation time on surfacing until the height core works.
This spec therefore freezes the design boundary and gates so Phase 6 can start cleanly after Phase 5.

## 2. Non-Goals

- No implementation before Phase 5 has an owner-accepted live height core.
- No material trickery to mask missing ridgelines, basins, or drainage.
- No separate slope/curvature derivations in materials, scatter, and erosion.
- No authoring one-off shader constants that cannot be moved into config packs.
- No CPU/GPU/facts divergence: surface descriptors must be derived from the same accepted height field.

## 3. Shared Surface Descriptor

Everything in Phase 6 consumes one descriptor:

```text
SurfaceDescriptor {
  height_m
  normal
  slope
  curvature
  height_band
  biome_weights
  skeleton/regime weights, if Phase 5 keeper has them
  moisture hint, optional
  exposure/roughness hint, optional
}
```

Descriptor rules:

- It is computed in world coordinates.
- It uses the accepted height path, including any Phase 7B skeleton facts if they are part of the keeper.
- CPU and GPU definitions must match within gate tolerances.
- Materials, scatter, and later erosion read this descriptor instead of rederiving their own slope or masks.

## 4. Normals And Lighting

Normals are the first Phase 6 implementation slice because they make geometry readable without changing
height.

Required behavior:

- analytic normals from the accepted generated field or from parity-safe page samples;
- edge-safe normals across adjacent pages;
- no visible lighting seam at page or clipmap boundaries;
- debug toggle to compare lit material against height-debug material;
- perf remains inside the hardened GPU-time gate.

If Phase 5 uses skeleton facts, normals must reflect the final composed height, not the pre-incision skeleton.

## 5. Material Packs

Material packs are data/config, not hardcoded shader branches. A pack maps descriptor fields to:

- albedo ramps;
- roughness;
- macro normal/detail normal intensity;
- talus/rock/soil/snow/vegetation blend weights;
- optional wetness or biome moisture response.

Minimum pack shape:

```text
MaterialPack {
  version
  families[]
  slope_bands[]
  height_bands[]
  regime_overrides[]
  albedo_curves
  roughness_curves
  normal_detail_curves
  scatter_tags
}
```

The pack must be swappable so WG10 stays a framework. Desert, alpine, alien, stylized, and realistic games
should be config choices over the same descriptor seam.

## 6. Scatter And Dressing

Scatter/dressing consumes descriptor + material pack tags:

- rocks/talus on steep exposed slopes;
- sediment/debris at fan toes and basin edges;
- vegetation where biome/moisture/slope allow;
- snow or scree by height/slope/exposure;
- sparse hero props only after the base scatter is deterministic and bounded.

Scatter must be deterministic, chunk-safe, and budgeted. It must not depend on camera order. It should share
hash/grid conventions with the terrain generator.

## 7. Gates

Non-visual gates:

- descriptor determinism at fixed world points;
- CPU-vs-GPU descriptor parity for height/slope/curvature/normal;
- edge-normal seam gate on adjacent production pages;
- material pack validation: finite curves, known families/regimes, bounded weights;
- scatter determinism and no duplicate/edge-pop at chunk boundaries;
- perf gate with Phase 6 enabled.

Visual/manual gates:

- owner fly with lit normals vs debug height view;
- owner accepts that materials improve terrain readability without hiding height failures;
- no visible page seams, shimmer, tiling, or repeated dressing patterns.

## 8. Slice Plan

Do not start until Phase 5 is accepted live.

1. **Descriptor spec fixtures.** Freeze descriptor fields from the accepted height algorithm.
2. **Analytic normals.** Implement lit normals with seam and perf gates.
3. **Material pack v0.** Add slope/height/biome/regime material mapping with validation.
4. **Descriptor parity.** CPU facts and GPU shader agree on descriptor samples.
5. **Scatter v0.** Deterministic rocks/debris/vegetation tags, boundary-pop gate.
6. **Owner fly.** Compare debug height, lit terrain, and material/scatter view.

## 9. Current Boundary

Phase 6 is design-ready, not implementation-ready. The blocker is still Phase 5: there must be an
owner-accepted live height core first. Starting Phase 6 earlier risks polishing the wrong algorithm and
breaking the project rule that gates prove invariants while the owner judges terrain look.
