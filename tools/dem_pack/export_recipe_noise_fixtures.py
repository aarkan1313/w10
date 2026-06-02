"""Export parity fixtures for the worldgen_proto noise primitives.

These fixtures are the PARITY ORACLE for the Rust port (`wg-10/rust/src/recipe_noise.rs`).
Each record samples one primitive at fixed WORLD-coordinate inputs (including negatives)
with explicit params/seed and captures worldgen_proto's exact f64 output. The Rust port
must reproduce every expected_output within a tight epsilon (deterministic f64 math).

Run from repo root:  python tools/dem_pack/export_recipe_noise_fixtures.py
Writes:              tools/dem_pack/fixtures/recipe_noise_fixtures.json

ONLY the seam-safe, per-point-local primitives are exported (the ones the 11 biome
recipes call as pure f(x,z)): value_noise, fbm, ridged_fbm, ridged_multifractal,
domain_warp, recursive_domain_warp, cellular_edges, range_spine_field, fault_block_field.
flow_accumulation_channels is intentionally OMITTED: it is a whole-grid sort+flow operator
(worldgen_proto's own docstring says it is "intentionally not a cheap local per-page
operator") and is not part of the seam-safe local f(x,z) port.
"""

import json
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import worldgen_proto as wg  # noqa: E402

GENERATOR_VERSION = "recipe_noise_fixtures/v1"

# A spread of world coordinates incl. negatives, near-integer, large magnitude.
SAMPLE_POINTS = [
    (0.0, 0.0),
    (1.5, 2.5),
    (-1.5, 2.5),
    (2.5, -1.5),
    (-3.25, -7.75),
    (123.5, -88.25),
    (-512.0, 511.999),
    (1024.25, 2048.75),
    (-2049.0, 33.0),
    (40000.0, -25000.0),
    (-128000.0, 64000.0),
    (333333.0, -222222.0),
]


def _scalar(v):
    """Pull a python float out of a numpy 0-d array result."""
    return float(np.asarray(v).reshape(-1)[0])


def main():
    records = []

    def add(primitive, inputs, expected):
        records.append({
            "primitive": primitive,
            "inputs": inputs,
            "expected_output": expected,
        })

    # worldgen_proto operates on numpy arrays; wrap each scalar coord as a 0-d array.
    def A(x):
        return np.array(float(x), dtype=np.float64)

    seeds = [0, 1, 7, 42, 1337, -5]

    # ---- value_noise(wx, wz, seed) -> [-1,1] ----
    for si, seed in enumerate(seeds):
        for (wx, wz) in SAMPLE_POINTS[: 8 if si % 2 == 0 else 6]:
            out = _scalar(wg.value_noise(A(wx), A(wz), seed=seed))
            add("value_noise", {"wx": wx, "wz": wz, "seed": seed}, out)

    # ---- fbm(wx, wz, base_freq, octaves, seed, gain, lacunarity) ----
    fbm_cfgs = [
        (1.0 / 2000.0, 6, 2, 0.5, 2.0),
        (1.0 / 8000.0, 4, 17, 0.55, 2.0),
        (1.0 / 500.0, 3, 43, 0.5, 1.9),
        (1.0 / 30000.0, 5, 9101, 0.56, 2.0),
        (1.0 / 1234.0, 4, -5, 0.48, 2.1),
    ]
    for (bf, oct_, seed, gain, lac) in fbm_cfgs:
        for (wx, wz) in SAMPLE_POINTS[:7]:
            out = _scalar(wg.fbm(A(wx), A(wz), bf, oct_, seed=seed, gain=gain, lacunarity=lac))
            add("fbm", {"wx": wx, "wz": wz, "base_freq": bf, "octaves": oct_,
                        "seed": seed, "gain": gain, "lacunarity": lac}, out)

    # ---- ridged_fbm(...) -> [0,1] ----
    for (bf, oct_, seed, gain, lac) in [
        (1.0 / 2000.0, 4, 3, 0.5, 2.0),
        (1.0 / 1500.0, 4, 100, 0.55, 2.0),
        (1.0 / 2500.0, 4, 200, 0.5, 2.0),
        (1.0 / 800.0, 5, 7, 0.46, 1.9),
    ]:
        for (wx, wz) in SAMPLE_POINTS[:8]:
            out = _scalar(wg.ridged_fbm(A(wx), A(wz), bf, oct_, seed=seed, gain=gain, lacunarity=lac))
            add("ridged_fbm", {"wx": wx, "wz": wz, "base_freq": bf, "octaves": oct_,
                               "seed": seed, "gain": gain, "lacunarity": lac}, out)

    # ---- ridged_multifractal(..., offset, weight_gain) -> [0,1] ----
    rmf_cfgs = [
        (1.0 / 2500.0, 5, 6, 0.5, 2.0, 1.0, 1.35),
        (1.0 / 105000.0, 5, 2, 0.57, 2.0, 1.0, 1.35),
        (1.0 / 65000.0, 4, 3, 0.55, 2.0, 1.0, 1.35),
        (1.0 / 18000.0, 5, 23, 0.55, 2.0, 1.0, 1.35),
        (1.0 / 1500.0, 4, 100, 0.6, 1.9, 0.9, 1.5),
        (1.0 / 3000.0, 3, 430, 0.42, 2.0, 1.1, 1.2),
    ]
    for (bf, oct_, seed, gain, lac, off, wg_) in rmf_cfgs:
        for (wx, wz) in SAMPLE_POINTS[:6]:
            out = _scalar(wg.ridged_multifractal(A(wx), A(wz), bf, oct_, seed=seed,
                                                 gain=gain, lacunarity=lac,
                                                 offset=off, weight_gain=wg_))
            add("ridged_multifractal", {"wx": wx, "wz": wz, "base_freq": bf, "octaves": oct_,
                                        "seed": seed, "gain": gain, "lacunarity": lac,
                                        "offset": off, "weight_gain": wg_}, out)

    # ---- domain_warp(wx, wz, warp_amount, warp_freq, seed) -> (x, z) ----
    for (amt, wf, seed) in [
        (1500.0, 1.0 / 6000.0, 4),
        (2500.0, 1.0 / 8000.0, 5),
        (0.0, 1.0 / 6000.0, 4),   # no-op branch
        (900.0, 1.0 / 9000.0, -5),
    ]:
        for (wx, wz) in SAMPLE_POINTS[:7]:
            ox, oz = wg.domain_warp(A(wx), A(wz), amt, wf, seed=seed)
            add("domain_warp", {"wx": wx, "wz": wz, "warp_amount": amt, "warp_freq": wf, "seed": seed},
                {"out_x": _scalar(ox), "out_z": _scalar(oz)})

    # ---- recursive_domain_warp(wx, wz, warp_amount, warp_freq, seed, steps, decay, freq_mul) ----
    for (amt, wf, seed, steps, decay, fmul) in [
        (1500.0, 1.0 / 6000.0, 4, 3, 0.55, 1.9),
        (2700.0, 1.0 / 8000.0, 5, 2, 0.55, 1.9),
        (1875.0, 1.0 / 8000.0, 5, 3, 0.55, 1.9),
        (0.0, 1.0 / 6000.0, 4, 3, 0.55, 1.9),   # no-op branch
        (1200.0, 1.0 / 9000.0, -5, 4, 0.6, 2.0),
    ]:
        for (wx, wz) in SAMPLE_POINTS[:7]:
            ox, oz = wg.recursive_domain_warp(A(wx), A(wz), amt, wf, seed=seed,
                                              steps=steps, decay=decay, freq_mul=fmul)
            add("recursive_domain_warp",
                {"wx": wx, "wz": wz, "warp_amount": amt, "warp_freq": wf, "seed": seed,
                 "steps": steps, "decay": decay, "freq_mul": fmul},
                {"out_x": _scalar(ox), "out_z": _scalar(oz)})

    # ---- cellular_edges(wx, wz, freq, seed, sharpness) -> [0,1] ----
    for (freq, seed, sharp) in [
        (1.0 / 3000.0, 7, 2.0),
        (1.0 / 5000.0, 160, 1.30),
        (1.0 / 1200.0, 300, 2.6),
        (1.0 / 800.0, 11, 1.25),
    ]:
        for (wx, wz) in SAMPLE_POINTS[:6]:
            out = _scalar(wg.cellular_edges(A(wx), A(wz), freq, seed=seed, sharpness=sharp))
            add("cellular_edges", {"wx": wx, "wz": wz, "freq": freq, "seed": seed, "sharpness": sharp}, out)

    # ---- range_spine_field(wx, wz, cell_size, width, seed, neighborhood) -> [0,1] ----
    for (cs, width, seed, nbh) in [
        (65000.0, 7000.0, 8, 2),
        (76000.0, 21000.0, 408, 2),
        (76000.0, 6500.0, 408, 2),
        (85000.0, 24000.0, 510, 2),
    ]:
        for (wx, wz) in SAMPLE_POINTS[5:12]:
            out = _scalar(wg.range_spine_field(A(wx), A(wz), cell_size=cs, width=width,
                                               seed=seed, neighborhood=nbh))
            add("range_spine_field", {"wx": wx, "wz": wz, "cell_size": cs, "width": width,
                                      "seed": seed, "neighborhood": nbh}, out)

    # ---- fault_block_field(wx, wz, cell_size, width, seed, neighborhood) -> [-1,1] ----
    for (cs, width, seed, nbh) in [
        (80000.0, 9000.0, 9, 2),
        (90000.0, 9000.0, 500, 2),
        (85000.0, 7500.0, 510, 2),
    ]:
        for (wx, wz) in SAMPLE_POINTS[5:12]:
            out = _scalar(wg.fault_block_field(A(wx), A(wz), cell_size=cs, width=width,
                                               seed=seed, neighborhood=nbh))
            add("fault_block_field", {"wx": wx, "wz": wz, "cell_size": cs, "width": width,
                                      "seed": seed, "neighborhood": nbh}, out)

    out_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
    os.makedirs(out_dir, exist_ok=True)
    out_path = os.path.join(out_dir, "recipe_noise_fixtures.json")

    by_prim = {}
    for r in records:
        by_prim[r["primitive"]] = by_prim.get(r["primitive"], 0) + 1

    doc = {
        "generator_version": GENERATOR_VERSION,
        "source": "tools/dem_pack/worldgen_proto.py",
        "note": "World-coordinate parity oracle for wg-10/rust/src/recipe_noise.rs (f64).",
        "counts": by_prim,
        "records": records,
    }
    with open(out_path, "w", encoding="ascii") as f:
        json.dump(doc, f, indent=2)

    print("wrote", out_path)
    print("total records:", len(records))
    for k in sorted(by_prim):
        print("  {0:24s} {1}".format(k, by_prim[k]))


if __name__ == "__main__":
    main()
