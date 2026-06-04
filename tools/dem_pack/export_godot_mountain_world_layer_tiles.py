r"""Export runtime-cacheable accepted mountain world-layer tiles.

This writes the generated world-layer tile contract that the future Rust/Godot
runtime producer/cache should consume. The existing
``export_godot_mountain_network_chunks.py`` remains the review-scene chunk
exporter; this file writes a runtime-facing artifact instead of a presentation
payload.

Run:
    python tools/dem_pack/export_godot_mountain_world_layer_tiles.py
Writes:
    wg-10/worldgen_terrain/generated/review/mountain_world_layer_tiles.json
"""

from __future__ import annotations

import json
from pathlib import Path

import mountain_synthesis as mountain
from mountain_world_layer import build_runtime_world_layer_payload


OUT = Path("wg-10/worldgen_terrain/generated/review/mountain_world_layer_tiles.json")


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    payload = build_runtime_world_layer_payload(styles=mountain.STYLES)
    OUT.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    print(f"wrote {OUT}")
    for tile in payload["tiles"]:
        pn = tile["pass_network"]
        print(
            f"  {tile['style_key']}: field_n={tile['field_n']} "
            f"routes={pn['routes']} walkable={pn['band_walkable_frac']} carved={pn['carved_frac']}"
        )


if __name__ == "__main__":
    main()
