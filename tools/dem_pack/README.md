# WorldGen10 real DEM pack tools

Two-phase: review tags (human checkpoint), then build the pack.

## Phase A — review tags
    python tools/dem_pack/review_tags.py
Open `dem_tag_review.html` in a browser; review the inferred families.
Edit `kernel_family_map.approved.json` (the source of truth): the `map`
(`{kernel_id: family}`) is what gets built; move ids to/from `excluded` as needed.
`--reseed` regenerates the seed (overwriting your edits); omit it to preserve.

## Phase B — build the pack (after you approve the map)
    python tools/dem_pack/build_pack.py --validate          # full pack
    python tools/dem_pack/build_pack.py --gate-subset 24 --validate   # gate subset
Writes `wg-10/worldgen_terrain/packs/dem_v1/` (terrain_pack.json + kernels/*.npy).

WG9 (`D:/workflows/worldgen9`) is read-only; all outputs land in this repo.
