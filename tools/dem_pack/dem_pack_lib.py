"""Pure helpers for the WorldGen10 real DEM pack tools. No file I/O here — these
take parsed dicts so they are unit-testable. (review_tags.py / build_pack.py do
the I/O.) See docs/superpowers/specs/2026-05-29-real-dem-pack-design.md."""
from __future__ import annotations
import math

SCHEMA = "worldgen10.terrain_pack.v1"
FAMILIES_PER_PALETTE = 3

# grammar_constants carried from the height_pack default (tunable later).
DEFAULT_GRAMMAR_CONSTANTS = {
    "region_size_m": 32768.0,
    "province_size_regions": 4,
    "palette_primary_pct": 72,
    "palette_compatible_pct": 22,
    "moderation_min": 0.4,
    "moderation_strength": 0.5,
}


def seed_family_map(shortlist_ids, inferences, threshold=0.7):
    """Seed an approved family map from WG9 inferences. retained/suggested with
    confidence >= threshold are accepted to inferred_family; everything else
    (low-confidence, unresolved, or no inference) is excluded. The USER then
    edits this before Phase B."""
    inf_by_id = {x["kernel_id"]: x for x in inferences}
    accepted = {}
    excluded = []
    for kid in shortlist_ids:
        x = inf_by_id.get(kid)
        if x is None:
            excluded.append(kid)
            continue
        status = x.get("tag_status")
        conf = float(x.get("family_confidence") or 0.0)
        fam = x.get("inferred_family")
        if status in ("retained", "suggested") and conf >= threshold and fam and fam != "uncategorized":
            accepted[kid] = fam
        else:
            excluded.append(kid)
    return {"map": accepted, "excluded": excluded}


def compose_palettes(fam_of):
    """Compose exactly-3-family palettes from {kernel_id -> family}. Group by
    family type; within a type sort ids lexicographically and chunk by 3; pad the
    last chunk by cycling the type's earliest ids (so every palette is same-type,
    exactly 3). Deterministic. Returns [{id, families:[3]}]."""
    by_fam = {}
    for kid, fam in fam_of.items():
        by_fam.setdefault(fam, []).append(kid)
    palettes = []
    for fam in sorted(by_fam):
        ids = sorted(by_fam[fam])
        n = len(ids)
        # chunk into groups of 3, padding the final group by cycling from front.
        idx = 0
        chunk_no = 0
        while idx < n:
            group = ids[idx:idx + FAMILIES_PER_PALETTE]
            i = 0
            while len(group) < FAMILIES_PER_PALETTE:
                group.append(ids[i % n])
                i += 1
            palettes.append({"id": f"{fam}_{chunk_no}", "families": group})
            idx += FAMILIES_PER_PALETTE
            chunk_no += 1
    return palettes


def _compose_compatibility(palettes):
    """Each palette is compatible with the other palettes of the same terrain
    type, plus one default cross-type neighbor (the next palette in sorted order,
    cyclic) so the grammar's compatible-roll always resolves."""
    pid_order = [p["id"] for p in palettes]
    type_of = {p["id"]: p["id"].rsplit("_", 1)[0] for p in palettes}
    compat = {}
    for i, p in enumerate(palettes):
        pid = p["id"]
        same = [q for q in pid_order if q != pid and type_of[q] == type_of[pid]]
        cross = pid_order[(i + 1) % len(pid_order)]
        lst = same[:]
        if cross != pid and cross not in lst:
            lst.append(cross)
        compat[pid] = lst
    return compat


def build_pack_dict(fam_of, meta, footprint_scale=1.0):
    """Assemble a worldgen10.terrain_pack.v1 dict from {kernel_id->family} +
    {kernel_id->kernel.json metadata}. relief_m=height_range_m;
    footprint_m=approx_sample_spacing_m*sample_px*footprint_scale. Raises
    ValueError naming the offending kernel on bad metadata.

    LEGACY / SCAFFOLDING (the kernel-tiling pack feeding height.rs::sample_kernel + height_page.glsl,
    being REPLACED at Slice 4 by the 11-biome composition stack). `relief_m = height_range_m` is the
    KNOWN z-score over-amplification bug (z-score DEMs should use height_std_m, not the full range —
    DESIGN, LOOSE_ENDS_LEDGER): it over-amplifies relief ~3.97-11.16x. The fix is the Slice-4 swap
    (which removes kernel sampling from the runtime), NOT patching this assembler. Do NOT build new
    detail/render work on this pack path."""
    if footprint_scale <= 0.0:
        raise ValueError(f"footprint_scale must be > 0, got {footprint_scale}")
    families = {}
    for kid, fam in fam_of.items():
        m = meta.get(kid)
        if m is None:
            raise ValueError(f"kernel {kid!r}: no metadata")
        relief = float(m.get("height_range_m") or 0.0)
        spacing = float(m.get("approx_sample_spacing_m") or 0.0)
        px = int(m.get("sample_px") or 0)
        if relief <= 0.0:
            raise ValueError(f"kernel {kid!r}: relief (height_range_m) must be > 0, got {relief}")
        if spacing <= 0.0 or px <= 0:
            raise ValueError(f"kernel {kid!r}: footprint inputs must be > 0 (spacing={spacing}, px={px})")
        families[kid] = {
            "kernel": f"kernels/{kid}.npy",
            "relief_m": relief,
            "footprint_m": spacing * px * footprint_scale,
        }
    palettes = compose_palettes(fam_of)
    if not palettes:
        raise ValueError("no palettes composed (empty family map)")
    compatibility = _compose_compatibility(palettes)
    return {
        "schema": SCHEMA,
        "version": 1,
        "grammar_constants": dict(DEFAULT_GRAMMAR_CONSTANTS),
        "palettes": palettes,
        "compatibility": compatibility,
        "families": families,
    }


REQUIRED_BIOME_PARAM_KEYS = (
    "relief_m", "octave_amps", "ridge_strength", "valley_depth", "warp_amount",
    "base_freq", "ridge_freq", "valley_freq", "warp_freq", "slope_bias",
)
N_OCTAVE_AMPS = 6


def _validate_biome_params(family, bp):
    """Reject NaN/degenerate/out-of-domain params with a descriptive error NAMING the family
    (pillar 4 — no silent default; parity-readiness — finite, f32-representable, in-domain)."""
    for k in REQUIRED_BIOME_PARAM_KEYS:
        if k not in bp:
            raise ValueError(f"biome_params[{family!r}]: missing key {k!r}")
    amps = bp["octave_amps"]
    if not isinstance(amps, (list, tuple)) or len(amps) != N_OCTAVE_AMPS:
        raise ValueError(f"biome_params[{family!r}]: octave_amps must be length {N_OCTAVE_AMPS}")
    scalars = {k: bp[k] for k in REQUIRED_BIOME_PARAM_KEYS if k != "octave_amps"}
    for k, v in list(scalars.items()) + [(f"octave_amps[{i}]", a) for i, a in enumerate(amps)]:
        fv = float(v)
        if not math.isfinite(fv):
            raise ValueError(f"biome_params[{family!r}]: {k} not finite ({v})")
    for fk in ("base_freq", "ridge_freq", "valley_freq", "warp_freq"):
        if float(bp[fk]) <= 0.0:
            raise ValueError(f"biome_params[{family!r}]: {fk} must be > 0 (got {bp[fk]})")
    if float(bp["relief_m"]) <= 0.0:
        raise ValueError(f"biome_params[{family!r}]: relief_m must be > 0 (got {bp['relief_m']})")
    if not (0.0 <= float(bp["ridge_strength"]) <= 1.0):
        raise ValueError(f"biome_params[{family!r}]: ridge_strength out of [0,1] ({bp['ridge_strength']})")
    if not (0.0 <= float(bp["valley_depth"]) <= 1.0):
        raise ValueError(f"biome_params[{family!r}]: valley_depth out of [0,1] ({bp['valley_depth']})")


def attach_biome_params(pack_dict, biome_params):
    """Additively attach a per-FAMILY biome_params table to a pack dict (validated). Returns a NEW dict;
    the existing per-kernel `families` entries + kernels are untouched (atlas removal is Slice 4)."""
    for family, bp in biome_params.items():
        _validate_biome_params(family, bp)
    out = dict(pack_dict)
    out["biome_params"] = {f: dict(bp) for f, bp in biome_params.items()}
    return out
