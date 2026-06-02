"""Export the f32-vs-f64 parity fixture for the GLSL noise/warp primitives.

The AUTHORITATIVE oracle is `worldgen_proto.py` (pure-numpy f64). This script
evaluates each primitive at a fixed, adversarial set of sample coords and writes
the f64 `expected` to a JSON fixture. The windowed parity gate
(`primitive_parity_check.gd`) drives the GLSL probe (`Wg10PrimitiveProbe`) and
asserts the GPU f32 result matches `expected` within an f32-budget epsilon.

Coverage rationale (this is the whole point of the task -> de-risk the hash):
  * zero coords                  -> trivial baseline
  * small positive               -> normal path
  * SMALL NEGATIVE               -> the arithmetic-right-shift sign path in _hash2
                                    (numpy int64 `>>` is arithmetic; GLSL i64 emulation
                                    must sign-extend + arithmetic-shift identically)
  * LARGE coords (~1e6)          -> the int64 wrapping-multiply path: ix*374761393 etc.
                                    overflows 32 bits, so the GLSL uvec2 64-bit emulation
                                    of the low-64 wrapping product MUST be exact
  * seeds incl 0 and large       -> seed*362437 also feeds the same wrapping sum

ASCII only (Windows cp1252). Use -> not unicode arrows.
"""
import json
import os
import sys

import numpy as np

# Import the oracle. This script lives in tools/dem_pack/ alongside worldgen_proto.py.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import worldgen_proto as wp  # noqa: E402

# fixtures/ lives under the Godot project so the .gd check can load it via res://.
REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT_PATH = os.path.join(
    REPO_ROOT, "wg-10", "worldgen_terrain", "fixtures", "primitive_parity_fixture.json"
)

GENERATOR_VERSION = "primitive_parity_fixture/v1"


def hash2_scalar(ix, iz, seed):
    """Call the numpy-array oracle _hash2 with scalar ints -> scalar float."""
    h = wp._hash2(np.array([int(ix)], dtype=np.int64),
                  np.array([int(iz)], dtype=np.int64),
                  int(seed))
    return float(h[0])


# Integer lattice samples for the HASH (PHASE A). These are the de-risking samples:
# every later primitive is built on this hash, so it is proven in isolation first.
HASH2_SAMPLES = [
    # zero / baseline
    (0, 0, 0),
    (1, 0, 0),
    (0, 1, 0),
    (1, 1, 0),
    # small positive
    (2, 3, 0),
    (7, 11, 5),
    # SMALL NEGATIVE (arithmetic-shift sign path)
    (-1, 0, 0),
    (0, -1, 0),
    (-1, -1, 0),
    (-3, -7, 5),
    (-123, 456, 9),
    # LARGE coords (int64 wrapping-multiply path)
    (1000000, 0, 0),
    (1000000, 1000000, 0),
    (-1000000, 1000000, 7),
    (123456, -789012, 13),
    # large seed
    (5, 9, 1000000),
]


def value_noise_scalar(wx, wz, seed):
    return float(wp.value_noise(np.array([wx], dtype=np.float64),
                                np.array([wz], dtype=np.float64), int(seed))[0])


def fbm_scalar(wx, wz, base_freq, octaves, seed, gain, lac):
    return float(wp.fbm(np.array([wx], dtype=np.float64),
                        np.array([wz], dtype=np.float64),
                        base_freq, octaves, int(seed), gain, lac)[0])


def ridged_mf_scalar(wx, wz, base_freq, octaves, seed):
    return float(wp.ridged_multifractal(np.array([wx], dtype=np.float64),
                                        np.array([wz], dtype=np.float64),
                                        base_freq, octaves, int(seed))[0])


def warp_scalar(wx, wz, amount, freq, seed):
    ox, oz = wp.recursive_domain_warp(np.array([wx], dtype=np.float64),
                                      np.array([wz], dtype=np.float64),
                                      amount, freq, int(seed))
    return float(np.atleast_1d(ox)[0]), float(np.atleast_1d(oz)[0])


# Coordinate samples reused for the f32 primitives. Cover negative + large + seeds.
# value_noise/fbm/ridged take RAW coords (callers pre-multiply by freq), matching the
# oracle signatures exactly; for fbm/ridged we pass an explicit base_freq.
NOISE_COORDS = [
    (0.0, 0.0, 0),
    (0.37, 0.62, 0),
    (1.5, 2.25, 3),
    (-0.4, -0.9, 0),       # negative coords -> floor / arithmetic-shift path
    (-3.7, 5.2, 7),
    (12.34, -56.78, 11),
    (1234.5, 6789.25, 2),  # larger coords
    (-9999.9, 8888.1, 100000),
]


def build_samples():
    samples = []

    # --- PHASE A: hash2 (proven first, in isolation) ---
    for (ix, iz, seed) in HASH2_SAMPLES:
        samples.append({
            "fn": "hash2",
            "args": [float(ix), float(iz), float(seed)],
            "expected": hash2_scalar(ix, iz, seed),
        })

    # --- PHASE B: f32 primitives built on the proven hash ---
    # value_noise(wx, wz, seed)
    for (wx, wz, seed) in NOISE_COORDS:
        samples.append({
            "fn": "value_noise",
            "args": [float(wx), float(wz), float(seed)],
            "expected": value_noise_scalar(wx, wz, seed),
        })

    # fbm(wx, wz, base_freq, octaves, seed, gain=0.5, lacunarity=2.0)
    # Probe fixes octaves=4, gain=0.5, lacunarity=2.0 (the recursive-warp inner call shape).
    FBM_OCT = 4
    FBM_GAIN = 0.5
    FBM_LAC = 2.0
    for (wx, wz, seed) in NOISE_COORDS:
        bf = 0.5
        samples.append({
            "fn": "fbm",
            "args": [float(wx), float(wz), float(bf), float(seed)],
            "expected": fbm_scalar(wx, wz, bf, FBM_OCT, seed, FBM_GAIN, FBM_LAC),
        })

    # ridged_multifractal(wx, wz, base_freq, octaves, seed, defaults)
    RMF_OCT = 5
    for (wx, wz, seed) in NOISE_COORDS:
        bf = 0.5
        samples.append({
            "fn": "ridged_multifractal",
            "args": [float(wx), float(wz), float(bf), float(seed)],
            "expected": ridged_mf_scalar(wx, wz, bf, RMF_OCT, seed),
        })

    # recursive_domain_warp(wx, wz, amount, freq, seed, steps=3, decay=0.55, freq_mul=1.9)
    # Two output channels -> two selectors (warp_x / warp_z).
    WARP_CASES = [
        (0.0, 0.0, 30.0, 0.01, 0),
        (100.0, 200.0, 50.0, 0.005, 3),
        (-300.0, 400.0, 80.0, 0.004, 7),
        (12345.0, -6789.0, 120.0, 0.0008, 100000),
    ]
    for (wx, wz, amount, freq, seed) in WARP_CASES:
        ox, oz = warp_scalar(wx, wz, amount, freq, seed)
        samples.append({
            "fn": "warp_x",
            "args": [float(wx), float(wz), float(amount), float(freq), float(seed)],
            "expected": ox,
        })
        samples.append({
            "fn": "warp_z",
            "args": [float(wx), float(wz), float(amount), float(freq), float(seed)],
            "expected": oz,
        })

    return samples


def main():
    samples = build_samples()
    doc = {
        "generator_version": GENERATOR_VERSION,
        "oracle": "tools/dem_pack/worldgen_proto.py",
        "note": "f64 oracle expected values; GLSL probe is f32 -> compared within ABS_EPS in the .gd gate",
        "samples": samples,
    }
    os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
    with open(OUT_PATH, "w", encoding="ascii") as fh:
        json.dump(doc, fh, indent=2)
        fh.write("\n")

    n_hash = sum(1 for s in samples if s["fn"] == "hash2")
    print("[export-primitive-parity] wrote %s" % OUT_PATH)
    print("[export-primitive-parity] total_samples=%d hash2_samples=%d -> ok" % (len(samples), n_hash))


if __name__ == "__main__":
    main()
