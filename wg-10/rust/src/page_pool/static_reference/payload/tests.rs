use super::*;

fn one_chunk_review_payload(corridor: Option<Vec<i64>>) -> StaticReviewPayload {
    StaticReviewPayload {
        generator_version: "mountain_synthesis_v0_9x9_original_scene_scale_review_pass_network"
            .into(),
        source_scope: "coherent_full_field_carved_with_pass_network_sliced_for_review".into(),
        chunk_count: 1,
        chunk_n: 2,
        chunk_span_m: 10.0,
        world_span_m: 10.0,
        source_world_span_m: None,
        source_scene_ratio: None,
        world_origin_x_m: None,
        world_origin_z_m: None,
        height_scale_m: 100.0,
        feature_span_m: 90_000.0,
        seeds: vec![StaticSeed {
            pass_network: Some(StaticPassNetwork {
                routes: 12,
                band_walkable_frac: 0.75,
                carved_frac: 0.25,
            }),
            stats: Some(StaticConditioningStats {
                source_min: -1.0,
                source_max: 2.0,
                source_ptp: 3.0,
                p05: -0.8,
                p50: 0.1,
                p95: 1.5,
                conditioned_min: -0.9,
                conditioned_max: 0.8,
                conditioned_ptp: 1.7,
            }),
            chunks: vec![StaticChunk {
                chunk_x: 0,
                chunk_z: 0,
                n: 2,
                span_m: 10.0,
                display_origin_x_m: -5.0,
                display_origin_z_m: -5.0,
                height: vec![0.0, 1.0, 2.0, 3.0],
                corridor,
                low_pass_hint: None,
                floor_hint: None,
                rock_hint: None,
                snow_hint: None,
            }],
        }],
    }
}

fn one_chunk_review_payload_with_hints() -> StaticReviewPayload {
    let mut payload = one_chunk_review_payload(Some(vec![1, 0, 1, 0]));
    let chunk = &mut payload.seeds[0].chunks[0];
    chunk.low_pass_hint = Some(vec![1.0, 0.0, 1.0, 0.0]);
    chunk.floor_hint = Some(vec![1.0, 0.25, 0.75, 0.0]);
    chunk.rock_hint = Some(vec![0.0, 0.5, 0.75, 1.0]);
    chunk.snow_hint = Some(vec![0.0, 0.0, 1.0, 1.0]);
    payload
}

fn runtime_tile_payload_with_hints() -> StaticRuntimeTilePayload {
    StaticRuntimeTilePayload {
        generator_version: "mountain_world_layer_runtime_tile_v1".into(),
        source_scope: "generated_mountain_world_layer_tiles_for_runtime_cache".into(),
        chunk_count: 1,
        chunk_n: 2,
        field_n: 2,
        world_span_m: 10.0,
        height_scale_m: 100.0,
        feature_span_m: 90_000.0,
        tiles: vec![StaticRuntimeTile {
            generator_version: "mountain_world_layer_runtime_tile_v1".into(),
            source_scope: "generated_mountain_world_layer_tile_for_runtime_cache".into(),
            chunk_count: 1,
            chunk_n: 2,
            field_n: 2,
            field_origin_x_m: -5.0,
            field_origin_z_m: -5.0,
            field_span_m: 10.0,
            source_origin_x_m: 100.0,
            source_origin_z_m: 200.0,
            source_span_m: 35.15625,
            source_scene_ratio: 3.515625,
            height_scale_m: 100.0,
            pass_network: Some(StaticPassNetwork {
                routes: 12,
                band_walkable_frac: 0.75,
                carved_frac: 0.25,
            }),
            stats: Some(StaticConditioningStats {
                source_min: -1.0,
                source_max: 2.0,
                source_ptp: 3.0,
                p05: -0.8,
                p50: 0.1,
                p95: 1.5,
                conditioned_min: -0.9,
                conditioned_max: 0.8,
                conditioned_ptp: 1.7,
            }),
            fields: StaticRuntimeTileFields {
                height: vec![0.0, 1.0, 2.0, 3.0],
                corridor: Some(vec![1, 0, 1, 0]),
                low_pass_hint: Some(vec![1.0, 0.0, 1.0, 0.0]),
                floor_hint: Some(vec![1.0, 0.25, 0.75, 0.0]),
                rock_hint: Some(vec![0.0, 0.5, 0.75, 1.0]),
                snow_hint: Some(vec![0.0, 0.0, 1.0, 1.0]),
            },
        }],
    }
}

#[test]
fn payload_contract_metadata_and_corridor_are_preserved() {
    let rt = StaticHeightRuntime::from_payload(StaticPayload::Review(one_chunk_review_payload(
        Some(vec![1, 0, 1, 0]),
    )))
    .expect("payload should parse");

    assert_eq!(
        rt.source_scope,
        "coherent_full_field_carved_with_pass_network_sliced_for_review"
    );
    assert_eq!(rt.pass_network_routes, 12);
    assert!((rt.pass_network_walkable_frac - 0.75).abs() < 1.0e-12);
    assert!((rt.pass_network_carved_frac - 0.25).abs() < 1.0e-12);
    assert!(rt.has_conditioning_stats);
    assert!((rt.conditioning_stats.source_ptp - 3.0).abs() < 1.0e-12);
    assert!((rt.conditioning_stats.p05 + 0.8).abs() < 1.0e-12);
    assert!((rt.conditioning_stats.p95 - 1.5).abs() < 1.0e-12);
    assert!((rt.conditioning_stats.conditioned_ptp - 1.7).abs() < 1.0e-12);
    assert!(rt.has_corridor);
    assert!((rt.corridor_frac - 0.5).abs() < 1.0e-12);
    assert!((rt.corridor_fraction_for_page(-5.0, -5.0, 10.0, 2) - 0.5).abs() < 1.0e-12);
    assert_eq!(rt.sample(-5.0, -5.0), 0.0);
    assert_eq!(rt.sample(5.0, 5.0), 300.0);
}

#[test]
fn payload_outside_domain_fades_to_low_floor_instead_of_smearing_edge() {
    let rt = StaticHeightRuntime::from_payload(StaticPayload::Review(one_chunk_review_payload(
        Some(vec![1, 0, 1, 0]),
    )))
    .expect("payload should parse");

    assert_eq!(rt.sample(5.0, 5.0), 300.0);
    assert_eq!(rt.sample(50.0, 50.0), 0.0);
    assert!(!rt.sample_corridor(50.0, 50.0));
    assert_eq!(rt.corridor_fraction_for_page(45.0, 45.0, 10.0, 2), 0.0);
}

#[test]
fn payload_without_source_scope_is_rejected() {
    let mut payload = one_chunk_review_payload(None);
    payload.source_scope.clear();

    let err = match StaticHeightRuntime::from_payload(StaticPayload::Review(payload)) {
        Ok(_) => panic!("empty scope should be rejected"),
        Err(err) => err,
    };
    assert!(err.contains("source_scope"));
}

#[test]
fn payload_invalid_conditioning_stats_are_rejected() {
    let mut payload = one_chunk_review_payload(None);
    payload.seeds[0].stats.as_mut().unwrap().p95 = -0.8;

    let err = match StaticHeightRuntime::from_payload(StaticPayload::Review(payload)) {
        Ok(_) => panic!("invalid conditioning stats should be rejected"),
        Err(err) => err,
    };
    assert!(err.contains("conditioning stats"));
}

#[test]
fn payload_material_hints_are_preserved_and_page_sampled() {
    let rt = StaticHeightRuntime::from_payload(StaticPayload::Review(
        one_chunk_review_payload_with_hints(),
    ))
    .expect("payload should parse");

    assert!(rt.has_material_hints);
    assert!((rt.material_hint_fracs.low_pass - 0.5).abs() < 1.0e-12);
    assert!((rt.material_hint_fracs.floor - 0.5).abs() < 1.0e-12);
    assert!((rt.material_hint_fracs.rock - 0.75).abs() < 1.0e-12);
    assert!((rt.material_hint_fracs.snow - 0.5).abs() < 1.0e-12);

    let page = rt
        .material_hint_fractions_for_page(-5.0, -5.0, 10.0, 2)
        .expect("page hints should sample");
    assert!((page.low_pass - 0.5).abs() < 1.0e-12);
    assert!((page.floor - 0.5).abs() < 1.0e-12);
    assert!((page.rock - 0.5625).abs() < 1.0e-12);
    assert!((page.snow - 0.5).abs() < 1.0e-12);
}

#[test]
fn payload_material_hints_outside_domain_do_not_smear_edge() {
    let rt = StaticHeightRuntime::from_payload(StaticPayload::Review(
        one_chunk_review_payload_with_hints(),
    ))
    .expect("payload should parse");

    let page = rt
        .material_hint_fractions_for_page(45.0, 45.0, 10.0, 2)
        .expect("page hints should sample");
    assert_eq!(page.low_pass, 0.0);
    assert_eq!(page.floor, 0.0);
    assert_eq!(page.rock, 0.0);
    assert_eq!(page.snow, 0.0);
}

#[test]
fn runtime_tile_payload_contract_metadata_and_materials_are_preserved() {
    let rt = StaticHeightRuntime::from_payload(StaticPayload::RuntimeTiles(
        runtime_tile_payload_with_hints(),
    ))
    .expect("runtime tile payload should parse");

    assert_eq!(
        rt.source_scope,
        "generated_mountain_world_layer_tile_for_runtime_cache"
    );
    assert_eq!(rt.pass_network_routes, 12);
    assert!((rt.pass_network_walkable_frac - 0.75).abs() < 1.0e-12);
    assert!((rt.pass_network_carved_frac - 0.25).abs() < 1.0e-12);
    assert!(rt.has_conditioning_stats);
    assert!((rt.conditioning_stats.source_ptp - 3.0).abs() < 1.0e-12);
    assert!(rt.has_corridor);
    assert!((rt.corridor_frac - 0.5).abs() < 1.0e-12);
    assert!(rt.has_material_hints);
    assert_eq!(rt.source_origin_x_m, 100.0);
    assert_eq!(rt.source_origin_z_m, 200.0);
    assert_eq!(rt.source_span_x_m, 35.15625);
    assert_eq!(rt.source_span_z_m, 35.15625);
    assert_eq!(rt.source_scene_ratio, 3.515625);
    assert!((rt.material_hint_fracs.low_pass - 0.5).abs() < 1.0e-12);
    assert!((rt.material_hint_fracs.floor - 0.5).abs() < 1.0e-12);
    assert!((rt.material_hint_fracs.rock - 0.75).abs() < 1.0e-12);
    assert!((rt.material_hint_fracs.snow - 0.5).abs() < 1.0e-12);
    assert_eq!(rt.sample(-5.0, -5.0), 0.0);
    assert_eq!(rt.sample(5.0, 5.0), 300.0);

    let page = rt
        .material_hint_fractions_for_page(-5.0, -5.0, 10.0, 2)
        .expect("page hints should sample");
    assert!((page.low_pass - 0.5).abs() < 1.0e-12);
    assert!((page.floor - 0.5).abs() < 1.0e-12);
    assert!((page.rock - 0.5625).abs() < 1.0e-12);
    assert!((page.snow - 0.5).abs() < 1.0e-12);
}

#[test]
fn runtime_tile_json_payload_deserializes_through_static_payload() {
    let payload: StaticPayload = serde_json::from_value(serde_json::json!({
        "generator_version": "mountain_world_layer_runtime_tile_v1",
        "source_scope": "generated_mountain_world_layer_tiles_for_runtime_cache",
        "chunk_count": 1,
        "chunk_n": 2,
        "field_n": 2,
        "world_span_m": 10.0,
        "height_scale_m": 100.0,
        "feature_span_m": 90000.0,
        "tiles": [
            {
                "generator_version": "mountain_world_layer_runtime_tile_v1",
                "source_scope": "generated_mountain_world_layer_tile_for_runtime_cache",
                "chunk_count": 1,
                "chunk_n": 2,
                "field_n": 2,
                "field_origin_x_m": -5.0,
                "field_origin_z_m": -5.0,
                "field_span_m": 10.0,
                "source_origin_x_m": 100.0,
                "source_origin_z_m": 200.0,
                "source_span_m": 35.15625,
                "source_scene_ratio": 3.515625,
                "height_scale_m": 100.0,
                "pass_network": {
                    "routes": 12,
                    "band_walkable_frac": 0.75,
                    "carved_frac": 0.25
                },
                "stats": {
                    "source_min": -1.0,
                    "source_max": 2.0,
                    "source_ptp": 3.0,
                    "p05": -0.8,
                    "p50": 0.1,
                    "p95": 1.5,
                    "conditioned_min": -0.9,
                    "conditioned_max": 0.8,
                    "conditioned_ptp": 1.7
                },
                "fields": {
                    "height": [0.0, 1.0, 2.0, 3.0],
                    "corridor": [1, 0, 1, 0],
                    "low_pass_hint": [1.0, 0.0, 1.0, 0.0],
                    "floor_hint": [1.0, 0.25, 0.75, 0.0],
                    "rock_hint": [0.0, 0.5, 0.75, 1.0],
                    "snow_hint": [0.0, 0.0, 1.0, 1.0]
                }
            }
        ]
    }))
    .expect("runtime tile json payload should deserialize");

    let rt = StaticHeightRuntime::from_payload(payload).expect("runtime tile should load");
    assert_eq!(rt.sample(5.0, 5.0), 300.0);
    assert!((rt.corridor_fraction_for_page(-5.0, -5.0, 10.0, 2) - 0.5).abs() < 1.0e-12);
}
