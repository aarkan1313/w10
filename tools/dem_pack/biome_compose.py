r"""Biome composition layer (Fork B): blend the OUTPUTS of distinct biome recipes at boundaries.

Knows nothing about which recipes exist — it takes per-recipe height fields + a weight field and
composes one height. blend_mode is tunable: 'height_favored' (primary; bias toward the locally
higher-relief recipe so structure stays crisp through the band) or 'field' (cheap lerp fallback).
Decided + tuned by render-first probes (see the spec's BLEND PROBE banners). Defaults baked from
the clash tuning sweep: favor_strength=2.0 + a narrow transition band give the crispest natural
transition (the band width itself is owned by the grammar/weight-field, not this config).
"""
from __future__ import annotations
from dataclasses import dataclass
import numpy as np
from scipy.ndimage import gaussian_filter


@dataclass(frozen=True)
class BlendConfig:
    mode: str = "height_favored"     # 'height_favored' | 'field'
    relief_sigma_px: float = 6.0     # blur radius for the local-relief proxy (height_favored)
    favor_strength: float = 2.0      # how hard to bias toward the higher-relief recipe in the band


def _blend_field(a: np.ndarray, b: np.ndarray, w_a: np.ndarray) -> np.ndarray:
    """Plain weighted lerp: w_a on a, (1-w_a) on b."""
    a = np.asarray(a, dtype=np.float64); b = np.asarray(b, dtype=np.float64)
    w = np.asarray(w_a, dtype=np.float64)
    return w * a + (1.0 - w) * b


def _blend_height_favored(a: np.ndarray, b: np.ndarray, w_a: np.ndarray, cfg: "BlendConfig") -> np.ndarray:
    """Bias the blend weight toward whichever recipe has stronger LOCAL relief inside the
    transition band, so structured terrain (e.g. mountain ridges) is not ghost-flattened into a
    low mound by a neutral average. Outside the band (w_a at 0 or 1) this reduces to the field blend."""
    a = np.asarray(a, dtype=np.float64); b = np.asarray(b, dtype=np.float64)
    w = np.asarray(w_a, dtype=np.float64)
    relief_a = np.abs(a - gaussian_filter(a, sigma=cfg.relief_sigma_px))
    relief_b = np.abs(b - gaussian_filter(b, sigma=cfg.relief_sigma_px))
    favor = relief_a / (relief_a + relief_b + 1e-9)            # ~1 where a has the structure
    band = 1.0 - np.abs(2.0 * w - 1.0)                         # 1 at band center, 0 at the pure ends
    w_adj = np.clip(w + (favor - 0.5) * cfg.favor_strength * band, 0.0, 1.0)
    return w_adj * a + (1.0 - w_adj) * b
