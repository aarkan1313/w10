r"""Biome recipe registry (Fork B): name -> a uniform recipe callable.

Each biome keeps its own *_synthesis.generate recipe; this adapts them to ONE signature
generate(wx, wz, seed, feature_span_m) -> height ndarray (dropping the diagnostic masks the
synths return for review). The composition layer (biome_compose) consumes these; it never
imports the synths directly. Adding a biome = one REGISTRY entry, no compose-layer change.
"""
from __future__ import annotations
from dataclasses import dataclass
from typing import Callable
import numpy as np

import mountain_synthesis as mountain
import grassland_synthesis as grassland
import desert_synthesis as desert
import glacial_synthesis as glacial
import karst_synthesis as karst
import volcanic_synthesis as volcanic
import temperate_synthesis as temperate
import tundra_synthesis as tundra
import rainforest_synthesis as rainforest
import coast_synthesis as coast
import wetland_synthesis as wetland


@dataclass(frozen=True)
class Recipe:
    name: str
    generate: Callable[..., np.ndarray]


def _adapt(mod) -> Callable[..., np.ndarray]:
    """Wrap a *_synthesis module's generate so it returns the bare height array."""
    def gen(wx, wz, seed: int = 0, feature_span_m: float | None = None) -> np.ndarray:
        out = mod.generate(wx, wz, seed=int(seed), feature_span_m=feature_span_m)
        return np.asarray(out["height"], dtype=np.float64)
    return gen


REGISTRY: dict[str, Recipe] = {
    name: Recipe(name, _adapt(mod))
    for name, mod in (
        ("mountain", mountain), ("grassland", grassland), ("desert", desert),
        ("glacial", glacial), ("karst", karst), ("volcanic", volcanic),
        ("temperate", temperate), ("tundra", tundra), ("rainforest", rainforest),
        ("coast", coast), ("wetland", wetland),
    )
}


def get_recipe(name: str) -> Recipe:
    if name not in REGISTRY:
        raise KeyError(f"unknown biome recipe {name!r}; known: {sorted(REGISTRY)}")
    return REGISTRY[name]
