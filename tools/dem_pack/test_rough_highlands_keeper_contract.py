import json
from pathlib import Path

import export_rough_highlands_keeper_contract as contract


FIXTURE = Path("tools/dem_pack/fixtures/rough_highlands_keeper_v1.json")


def _load_fixture():
    return json.loads(FIXTURE.read_text(encoding="utf-8"))


def test_rough_highlands_keeper_fixture_matches_current_python_contract():
    assert _load_fixture() == contract.build_contract()


def test_rough_highlands_keeper_contract_names_port_boundaries():
    data = _load_fixture()
    assert data["keeper_id"] == "rough_highlands_keeper_v1"
    assert data["generator_version"] == "rough_world_chunks_v2_independent_windows"
    assert data["status"] == "candidate_contract_owner_direction_and_seams_accepted_not_runtime_port"
    assert data["facts_contract"]["public_runtime_candidates"] == [
        "uplift",
        "routed_surface",
        "discharge",
        "tributary",
        "channel_axis",
        "crest_dist",
        "channel_dist",
    ]
    assert "route_texture" in data["facts_contract"]["height_private_material_fields"]
    assert "seam_guides" in data["facts_contract"]["review_only_overlays"]


def test_rough_highlands_keeper_contract_keeps_current_seam_and_variation_evidence():
    summary = _load_fixture()["chunk_contract_summary"]
    assert summary["seams"]["rows"] == 24
    assert summary["seams"]["height_max_abs_delta"] <= 2e-4
    assert summary["seams"]["corridor_min_match_frac"] >= 0.90
    assert summary["visual_seams"]["normal_max_angle_deg"] <= 0.01
    assert summary["visual_seams"]["corridor_edge_mismatch_count"] == 0
    assert len(summary["variation"]) >= 2
    assert min(float(row["mean_abs_delta"]) for row in summary["variation"]) > 0.02


def test_rough_highlands_keeper_contract_has_reproducible_review_golden():
    data = _load_fixture()
    golden = data["golden_review_contact_sheet"]
    assert golden["renderer"] == "render_rough_world_chunks_review.contact_sheet"
    assert golden["size_px"] == [404, 204]
    assert len(golden["png_sha256"]) == 64
    assert golden == contract.build_contract()["golden_review_contact_sheet"]
