"""Render hillshaded PNGs of the worldgen prototype for the owner to judge by eye (render-first).
Writes to D:\\tmp\\. NOT a test — a runnable inspection tool. Run: python render_worldgen.py"""
import numpy as np
from PIL import Image
import worldgen_proto as wg

OUT = r"D:\tmp"

# Scale fix (S1 finding): the base octave must be LOW-frequency continental (~60 km) so a 200 km view
# shows real landmass structure, not high-freq sandpaper. fbm octaves then double down to local detail.
# warp/ridge/valley freqs lowered to match (ranges/valleys should be tens of km, not ~1 km).
MOUNTAIN = {"relief_m": 1200.0, "octave_amps": [1.0,0.55,0.3,0.16,0.08,0.04],
            "ridge_strength": 0.9, "valley_depth": 0.5, "warp_amount": 18000.0,
            "base_freq": 1.0/60000.0, "ridge_freq": 1.0/22000.0, "valley_freq": 1.0/28000.0, "warp_freq": 1.0/90000.0}
PLAINS   = {"relief_m": 180.0, "octave_amps": [1.0,0.4,0.18,0.08,0.03,0.01],
            "ridge_strength": 0.05, "valley_depth": 0.15, "warp_amount": 12000.0,
            "base_freq": 1.0/80000.0, "ridge_freq": 1.0/22000.0, "valley_freq": 1.0/40000.0, "warp_freq": 1.0/100000.0}
BADLANDS = {"relief_m": 400.0, "octave_amps": [1.0,0.6,0.4,0.25,0.15,0.08],
            "ridge_strength": 0.4, "valley_depth": 0.9, "warp_amount": 14000.0,
            "base_freq": 1.0/45000.0, "ridge_freq": 1.0/14000.0, "valley_freq": 1.0/11000.0, "warp_freq": 1.0/70000.0}


def hillshade(z, exaggeration=1.0, az=315.0, alt=45.0):
    zn = (z - z.min()) / (np.ptp(z) + 1e-9)
    gy, gx = np.gradient(zn * 80.0 * exaggeration)
    slope = np.pi/2.0 - np.arctan(np.sqrt(gx*gx + gy*gy))
    aspect = np.arctan2(-gx, gy)
    azr = np.radians(360 - az + 90); altr = np.radians(alt)
    sh = np.sin(altr)*np.sin(slope) + np.cos(altr)*np.cos(slope)*np.cos(azr - aspect)
    return np.clip(sh, 0, 1)


def grid(n, span, ox=0.0, oz=0.0):
    ii = np.linspace(0, span, n)
    return np.meshgrid(ii + ox, ii + oz)


def save(name, sh):
    Image.fromarray((sh*255).astype(np.uint8), mode="L").save(rf"{OUT}\{name}.png")
    print(f"wrote {OUT}\\{name}.png")


def main():
    # 1. Each biome over a LARGE area (200 km) — judge contiguity + structure + no-repeat.
    for nm, p in [("mountain", MOUNTAIN), ("plains", PLAINS), ("badlands", BADLANDS)]:
        wx, wz = grid(1024, 200000.0)
        save(f"worldgen_{nm}_200km", hillshade(wg.generate(wx, wz, p, seed=7)))
    # 2. A close-up (10 km) of mountains — judge near-field detail.
    wx, wz = grid(1024, 10000.0, ox=120000.0, oz=80000.0)
    save("worldgen_mountain_10km", hillshade(wg.generate(wx, wz, MOUNTAIN, seed=7), exaggeration=2.0))
    # 3. A BIOME-TRANSITION strip: mountains (left) blending to plains (right). The REAL grammar
    #    param-blend is S3-S5; here we approximate by lerping the two results across X just to SEE
    #    whether the blend reads SEAMLESS (no hard line). (Not the real blend — eyeball only.)
    n = 1024; span = 200000.0
    wx, wz = grid(n, span)
    t = np.linspace(0.0, 1.0, n).reshape(1, -1)       # 0=mountain .. 1=plains across X
    hm = wg.generate(wx, wz, MOUNTAIN, seed=7); hp = wg.generate(wx, wz, PLAINS, seed=7)
    strip = hm*(1-t) + hp*t
    save("worldgen_transition_strip", hillshade(strip, exaggeration=1.5))


if __name__ == "__main__":
    main()
