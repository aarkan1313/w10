"""Render A | B | v2 at matched world coords (seed 133, rough_anchor, 25.6 km core).
Top-down hillshade + oblique scene, with relief numbers labeled. Owner-eye gate.
Writes to D:/tmp/wg10_geography_engine/. Run from repo root with PYTHONPATH=tools/dem_pack."""
from __future__ import annotations
from pathlib import Path
import numpy as np
import geography_skeleton as skel
import geography_skeleton_windows as win
import export_godot_rough_world_chunks as ex
import keeper_v2 as v2
from render_geography_skeleton_focus import FOCUS, oblique_panel
from render_geography_engine import panel_from_height, contact_sheet

OUT = Path("D:/tmp/wg10_geography_engine")

def _abv(seed=133):
    sc = next(s for s in FOCUS if s.key == "rough_anchor")
    spec = ex._window_spec(129, ex.CHUNK_SPAN_M)
    w = win.build_skeleton_window(ex.WORLD_ORIGIN_X_M, ex.WORLD_ORIGIN_Z_M, seed, spec)
    cs = win._core_slice(spec); s0, s1 = cs.start, cs.stop
    B, _ = ex._compose_windowed_height(w, seed, spec); B = np.asarray(B[s0:s1, s0:s1])
    wx = np.asarray(w["wx"])[s0:s1, s0:s1]; wz = np.asarray(w["wz"])[s0:s1, s0:s1]
    A = np.asarray(skel.compose_height(wx, wz, seed=seed, scenario=sc)["height"])
    V = v2.compose_windowed_height_v2(w, seed, spec, v2.KeeperV2Params())
    return A, B, V

def main():
    OUT.mkdir(parents=True, exist_ok=True)
    A, B, V = _abv()
    def lab(z): return f"std {z.std():.2f} ptp {np.ptp(z):.2f}"
    contact_sheet(OUT / "abv_keeper_compare_topdown.png", [
        panel_from_height(A, "A approved", 340, 1.4, sub=lab(A)),
        panel_from_height(B, "B keeper_v1", 340, 1.4, sub=lab(B)),
        panel_from_height(V, "v2 best-of-both", 340, 1.4, sub=lab(V)),
    ], cols=3)
    contact_sheet(OUT / "abv_keeper_compare_oblique.png", [
        oblique_panel(A, "A approved"), oblique_panel(B, "B keeper_v1"), oblique_panel(V, "v2 best-of-both"),
    ], cols=3)
    print(f"A {lab(A)} | B {lab(B)} | v2 {lab(V)}")
    print(f"wrote {OUT/'abv_keeper_compare_oblique.png'}")
    print(f"wrote {OUT/'abv_keeper_compare_topdown.png'}")

if __name__ == "__main__":
    main()
