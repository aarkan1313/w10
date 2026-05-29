"""Generate synthetic .npy kernel fixtures + a height terrain pack for tests.
Deterministic, no randomness. Run from anywhere:
    python wg-10/worldgen_terrain/fixtures/gen_kernels.py
Requires numpy. Writes real NumPy-v1.0 C-order float32 arrays (and one
Fortran-order array for the reject test).
"""
import json
import os
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
KDIR = os.path.join(HERE, "kernels")
os.makedirs(KDIR, exist_ok=True)

# flat: constant 0.5 everywhere -> slope 0 -> moderation 1.0 -> exact anchor.
flat = np.full((4, 4), 0.5, dtype="<f4")
np.save(os.path.join(KDIR, "flat.npy"), flat, allow_pickle=False)

# ramp: linear 0..1 across columns, constant down rows -> exact bilinear, nonzero slope.
ramp = np.tile(np.linspace(0.0, 1.0, 4, dtype="<f4"), (4, 1)).astype("<f4")
np.save(os.path.join(KDIR, "ramp.npy"), ramp, allow_pickle=False)

# bad_fortran: a Fortran-ordered array the reader must reject.
bad = np.asfortranarray(np.full((4, 4), 0.25, dtype="<f4"))
np.save(os.path.join(KDIR, "bad_fortran.npy"), bad, allow_pickle=False)

# Height pack: 4 palettes. The "flat_only" palette's 3 families all use flat ->
# weight-independent exact anchor (height = relief_m * 0.5). Other palettes mix.
pack = {
    "schema": "worldgen10.terrain_pack.v1",
    "version": 1,
    "grammar_constants": {
        "region_size_m": 32768.0,
        "province_size_regions": 4,
        "palette_primary_pct": 72,
        "palette_compatible_pct": 22,
        "moderation_min": 0.4,
        "moderation_strength": 0.5,
    },
    "palettes": [
        {"id": "flat_only", "families": ["fa", "fb", "fc"]},
        {"id": "ramps",     "families": ["ra", "rb", "fa"]},
        {"id": "mixed",     "families": ["fa", "ra", "fb"]},
        {"id": "ramps2",    "families": ["rb", "fc", "ra"]},
    ],
    "compatibility": {
        "flat_only": ["mixed", "ramps"],
        "ramps":     ["mixed", "ramps2"],
        "mixed":     ["flat_only", "ramps"],
        "ramps2":    ["ramps", "mixed"],
    },
    "families": {
        "fa": {"kernel": "kernels/flat.npy", "relief_m": 1000.0, "footprint_m": 8192.0},
        "fb": {"kernel": "kernels/flat.npy", "relief_m": 1000.0, "footprint_m": 8192.0},
        "fc": {"kernel": "kernels/flat.npy", "relief_m": 1000.0, "footprint_m": 8192.0},
        "ra": {"kernel": "kernels/ramp.npy", "relief_m": 600.0,  "footprint_m": 4096.0},
        "rb": {"kernel": "kernels/ramp.npy", "relief_m": 800.0,  "footprint_m": 16384.0},
    },
}
with open(os.path.join(HERE, "height_pack.json"), "w") as f:
    json.dump(pack, f, indent=2)
    f.write("\n")

# All-flat pack: EVERY palette uses only flat families, so height == relief_m*0.5
# == 500.0 at ANY coordinate, independent of the grammar roll. This is the
# robust exact-value anchor (no scanning, no luck).
flat_pack = {
    "schema": "worldgen10.terrain_pack.v1",
    "version": 1,
    "grammar_constants": {
        "region_size_m": 32768.0,
        "province_size_regions": 4,
        "palette_primary_pct": 72,
        "palette_compatible_pct": 22,
        "moderation_min": 0.4,
        "moderation_strength": 0.5,
    },
    "palettes": [
        {"id": "a", "families": ["fa", "fb", "fc"]},
        {"id": "b", "families": ["fb", "fc", "fa"]},
    ],
    "compatibility": {"a": ["b"], "b": ["a"]},
    "families": {
        "fa": {"kernel": "kernels/flat.npy", "relief_m": 1000.0, "footprint_m": 8192.0},
        "fb": {"kernel": "kernels/flat.npy", "relief_m": 1000.0, "footprint_m": 8192.0},
        "fc": {"kernel": "kernels/flat.npy", "relief_m": 1000.0, "footprint_m": 8192.0},
    },
}
with open(os.path.join(HERE, "flat_pack.json"), "w") as f:
    json.dump(flat_pack, f, indent=2)
    f.write("\n")

print("wrote flat.npy ramp.npy bad_fortran.npy height_pack.json flat_pack.json")
