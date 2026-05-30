import pytest
import dem_pack_lib as lib


def test_seed_family_map_accepts_high_confidence_suggested():
    inferences = [
        {"kernel_id": "a", "inferred_family": "glacial", "family_confidence": 0.8, "tag_status": "suggested"},
        {"kernel_id": "b", "inferred_family": "mountain", "family_confidence": 0.6, "tag_status": "suggested"},
        {"kernel_id": "c", "inferred_family": "coast", "family_confidence": 0.9, "tag_status": "retained"},
        {"kernel_id": "d", "inferred_family": "desert", "family_confidence": 0.5, "tag_status": "unresolved"},
    ]
    shortlist_ids = ["a", "b", "c", "d"]
    m = lib.seed_family_map(shortlist_ids, inferences, threshold=0.7)
    assert m["map"] == {"a": "glacial", "c": "coast"}      # >=0.7 suggested/retained
    assert set(m["excluded"]) == {"b", "d"}                # below threshold or unresolved


def test_seed_family_map_excludes_kernel_with_no_inference():
    m = lib.seed_family_map(["x"], [], threshold=0.7)
    assert m["map"] == {}
    assert m["excluded"] == ["x"]


def test_compose_palettes_chunks_by_three_same_type():
    # 4 badlands ids -> 2 palettes; last padded by cycling front ids.
    fam_of = {"b1": "badlands", "b2": "badlands", "b3": "badlands", "b4": "badlands"}
    pals = lib.compose_palettes(fam_of)
    # palette 0 = first 3 sorted; palette 1 = [b4, b1, b2] (pad by cycling front)
    ids = sorted(fam_of)  # b1,b2,b3,b4
    assert pals[0]["families"] == ["b1", "b2", "b3"]
    assert pals[1]["families"] == ["b4", "b1", "b2"]
    assert all(len(p["families"]) == 3 for p in pals)


def test_compose_palettes_single_kernel_type_repeats():
    fam_of = {"t1": "tundra"}
    pals = lib.compose_palettes(fam_of)
    one = [p for p in pals if p["id"].startswith("tundra")]
    assert len(one) == 1
    assert one[0]["families"] == ["t1", "t1", "t1"]


def test_compose_palettes_palette_ids_unique_and_deterministic():
    fam_of = {"m1": "mountain", "m2": "mountain", "m3": "mountain", "c1": "coast", "c2": "coast", "c3": "coast"}
    a = lib.compose_palettes(fam_of)
    b = lib.compose_palettes(fam_of)
    assert a == b                                          # deterministic
    pids = [p["id"] for p in a]
    assert len(pids) == len(set(pids))                    # unique ids


def test_build_pack_dict_shape_and_family_fields():
    fam_of = {"m1": "mountain", "m2": "mountain", "m3": "mountain"}
    meta = {
        "m1": {"height_range_m": 1000.0, "approx_sample_spacing_m": 100.0, "sample_px": 512},
        "m2": {"height_range_m": 800.0, "approx_sample_spacing_m": 90.0, "sample_px": 512},
        "m3": {"height_range_m": 1200.0, "approx_sample_spacing_m": 110.0, "sample_px": 512},
    }
    pack = lib.build_pack_dict(fam_of, meta, footprint_scale=1.0)
    assert pack["schema"] == "worldgen10.terrain_pack.v1"
    assert pack["version"] == 1
    assert set(pack["families"]) == {"m1", "m2", "m3"}
    # relief = height_range_m; footprint = spacing * px * scale
    assert pack["families"]["m1"]["relief_m"] == 1000.0
    assert pack["families"]["m1"]["footprint_m"] == 100.0 * 512 * 1.0
    assert pack["families"]["m1"]["kernel"] == "kernels/m1.npy"
    # every palette family resolves to a real family + has 3
    fam_ids = set(pack["families"])
    for p in pack["palettes"]:
        assert len(p["families"]) == 3
        assert all(f in fam_ids for f in p["families"])
    # compatibility references real palette ids
    pal_ids = {p["id"] for p in pack["palettes"]}
    for k, v in pack["compatibility"].items():
        assert k in pal_ids
        assert all(x in pal_ids for x in v)


def test_build_pack_dict_rejects_bad_relief():
    fam_of = {"m1": "mountain", "m2": "mountain", "m3": "mountain"}
    meta = {"m1": {"height_range_m": 0.0, "approx_sample_spacing_m": 100.0, "sample_px": 512},
            "m2": {"height_range_m": 800.0, "approx_sample_spacing_m": 90.0, "sample_px": 512},
            "m3": {"height_range_m": 1200.0, "approx_sample_spacing_m": 110.0, "sample_px": 512}}
    try:
        lib.build_pack_dict(fam_of, meta, footprint_scale=1.0)
        assert False, "should reject relief<=0"
    except ValueError as e:
        assert "relief" in str(e) and "m1" in str(e)


def test_build_pack_dict_rejects_bad_footprint_inputs():
    fam_of = {"m1": "mountain", "m2": "mountain", "m3": "mountain"}
    base = {"height_range_m": 1000.0, "approx_sample_spacing_m": 100.0, "sample_px": 512}
    # zero spacing on m1 -> ValueError naming m1
    meta = {"m1": {**base, "approx_sample_spacing_m": 0.0}, "m2": dict(base), "m3": dict(base)}
    try:
        lib.build_pack_dict(fam_of, meta, footprint_scale=1.0)
        assert False, "should reject spacing<=0"
    except ValueError as e:
        assert "m1" in str(e) and "footprint" in str(e)
    # footprint_scale<=0 -> ValueError mentioning footprint_scale
    good = {"m1": dict(base), "m2": dict(base), "m3": dict(base)}
    try:
        lib.build_pack_dict(fam_of, good, footprint_scale=0.0)
        assert False, "should reject footprint_scale<=0"
    except ValueError as e:
        assert "footprint_scale" in str(e)


def test_seed_family_map_excludes_uncategorized_even_if_confident():
    # a high-confidence retained inference whose inferred_family is "uncategorized"
    # must still be excluded (it's not a real family).
    inferences = [{"kernel_id": "u", "inferred_family": "uncategorized", "family_confidence": 0.95, "tag_status": "retained"}]
    m = lib.seed_family_map(["u"], inferences, threshold=0.7)
    assert m["map"] == {}
    assert m["excluded"] == ["u"]


def test_attach_biome_params_adds_table():
    pack = {"schema": lib.SCHEMA, "version": 1, "families": {"k1": {"kernel": "kernels/k1.npy"}}}
    bp = {"mountain": {"relief_m": 1200.0, "octave_amps": [1.0, 0.5, 0.25, 0.12, 0.06, 0.03],
                       "ridge_strength": 0.8, "valley_depth": 0.3, "warp_amount": 2000.0,
                       "base_freq": 1.0/6000, "ridge_freq": 2.0/6000, "valley_freq": 1.2/6000,
                       "warp_freq": 1.0/16200, "slope_bias": 20.0}}
    out = lib.attach_biome_params(pack, bp)
    assert "biome_params" in out
    assert out["biome_params"]["mountain"]["relief_m"] == 1200.0
    assert out["families"] == pack["families"]   # per-kernel entries untouched (additive)


def test_attach_biome_params_rejects_nan_naming_family():
    pack = {"schema": lib.SCHEMA, "version": 1, "families": {}}
    bad = {"badlands": {"relief_m": float("nan"), "octave_amps": [1.0]*6, "ridge_strength": 0.4,
                        "valley_depth": 0.9, "warp_amount": 1800.0, "base_freq": 1.0/2200,
                        "ridge_freq": 2.0/2200, "valley_freq": 1.2/2200, "warp_freq": 1.0/5940,
                        "slope_bias": 30.0}}
    with pytest.raises(ValueError, match="badlands"):
        lib.attach_biome_params(pack, bad)


def test_attach_biome_params_rejects_out_of_domain_freq():
    pack = {"schema": lib.SCHEMA, "version": 1, "families": {}}
    bad = {"coast": {"relief_m": 100.0, "octave_amps": [1.0]*6, "ridge_strength": 0.1,
                     "valley_depth": 0.1, "warp_amount": 500.0, "base_freq": 0.0,  # invalid: freq must be >0
                     "ridge_freq": 0.0, "valley_freq": 0.0, "warp_freq": 0.0, "slope_bias": 5.0}}
    with pytest.raises(ValueError, match="coast"):
        lib.attach_biome_params(pack, bad)


def test_attach_biome_params_rejects_wrong_octave_count():
    # ADDED (code-review hardening): octave_amps must be exactly 6 (else the generator/GLSL mirror mis-reads).
    pack = {"schema": lib.SCHEMA, "version": 1, "families": {}}
    bad = {"desert": {"relief_m": 200.0, "octave_amps": [1.0, 0.5, 0.25],  # only 3, must be 6
                      "ridge_strength": 0.2, "valley_depth": 0.2, "warp_amount": 800.0,
                      "base_freq": 1.0/3000, "ridge_freq": 2.0/3000, "valley_freq": 1.2/3000,
                      "warp_freq": 1.0/8100, "slope_bias": 8.0}}
    with pytest.raises(ValueError, match="desert"):
        lib.attach_biome_params(pack, bad)
