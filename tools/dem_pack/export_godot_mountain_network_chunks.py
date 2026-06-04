r"""Export the accepted mountain-network world-layer payload.

The construction lives in ``mountain_world_layer``. This file only writes the
generated review JSON consumed by ``mountain_network_chunks_review.tscn`` and
the REFERENCE runtime bridge.

Run:
    python tools/dem_pack/export_godot_mountain_network_chunks.py
Writes:
    wg-10/worldgen_terrain/generated/review/mountain_network_chunks.json
"""

from __future__ import annotations

import json
from pathlib import Path

import mountain_synthesis as mountain
from mountain_world_layer import build_network_payload, build_network_world


OUT = Path("wg-10/worldgen_terrain/generated/review/mountain_network_chunks.json")


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    payload = build_network_payload(styles=mountain.STYLES)
    OUT.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    print(f"wrote {OUT}")
    for wd in payload["seeds"]:
        pn = wd["pass_network"]
        print(f"  {wd['style_key']}: routes={pn['routes']} walkable={pn['band_walkable_frac']} carved={pn['carved_frac']}")


if __name__ == "__main__":
    main()
