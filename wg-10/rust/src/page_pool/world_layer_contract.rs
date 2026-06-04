//! Mountain world-layer contract taxonomy reports.

use godot::prelude::*;

use super::producer::ProducerKind;
use super::{StaticHeightRuntime, Wg10PagePool};

#[godot_api(secondary)]
impl Wg10PagePool {
    /// Diagnostic report for the mountain world-layer acceptance contract.
    ///
    /// This deliberately separates "what producer is active" from "which
    /// contract facts it owns". The live single-biome mountain recipe can be a
    /// review candidate without claiming the accepted pass-network /
    /// whole-field-conditioning contract.
    #[func]
    pub fn mountain_world_layer_contract_report(&self) -> Dictionary<GString, Variant> {
        let mut out = Dictionary::<GString, Variant>::new();
        let runtime_mode = self.active_runtime_mode_label();

        out.set("runtime_mode", runtime_mode);
        out.set("accepted_visual_baseline", false);
        out.set("explicit_live_candidate", false);
        out.set("live_world_layer_candidate", false);
        out.set("has_source_display_mapping", false);
        out.set("has_mountain_macro_field", false);
        out.set("has_pass_network_routes", false);
        out.set("has_route_carving", false);
        out.set("has_page_stable_conditioning", false);
        out.set("has_material_hints", false);
        out.set("has_facts_collision_story", false);
        out.set("satisfies_mountain_world_layer_contract", false);

        if let Some(reference) = self.static_ref.as_ref() {
            let has_pass_network = reference.pass_network_routes > 0;
            let has_route_carving = reference.pass_network_carved_frac > 0.0;
            let has_conditioning = reference_has_conditioning(reference);
            out.set("contract_kind", "accepted_static_reference_visual_baseline");
            out.set("source_scope", reference.source_scope.clone());
            out.set("accepted_visual_baseline", true);
            out.set(
                "has_source_display_mapping",
                reference_has_source_display_mapping(reference),
            );
            out.set("has_mountain_macro_field", true);
            out.set("has_pass_network_routes", has_pass_network);
            out.set("has_route_carving", has_route_carving);
            out.set("has_page_stable_conditioning", has_conditioning);
            out.set("has_material_hints", reference.has_material_hints);
            out.set(
                "blocking_gap",
                "live facts/collision story and procedural world-layer synthesis remain open",
            );
        } else if matches!(self.active_producer_kind(), Some(ProducerKind::World)) {
            let preview_ref = self.world_preview_ref.as_ref();
            out.set(
                "contract_kind",
                if preview_ref.is_some() {
                    "world_route_reference_height_preview"
                } else {
                    "grammar_routed_runtime_biome_composition"
                },
            );
            out.set(
                "source_scope",
                if preview_ref.is_some() {
                    "grammar_route_over_accepted_reference_height"
                } else {
                    "grammar_routed_page_weight_field"
                },
            );
            out.set("has_source_display_mapping", true);
            out.set("has_mountain_macro_field", true);
            out.set("has_world_preview_reference", preview_ref.is_some());
            if let Some(bound_reference) = preview_ref {
                let reference = bound_reference.reference();
                let has_pass_network = reference.pass_network_routes > 0;
                let has_route_carving = reference.pass_network_carved_frac > 0.0;
                let has_conditioning = reference_has_conditioning(reference);
                out.set("height_source", "accepted_reference_payload_for_preview");
                out.set("procedural_world_layer_height", false);
                out.set("reference_source_scope", reference.source_scope.clone());
                out.set(
                    "has_source_display_mapping",
                    reference_has_source_display_mapping(reference),
                );
                out.set("has_pass_network_routes", has_pass_network);
                out.set("has_route_carving", has_route_carving);
                out.set("has_page_stable_conditioning", has_conditioning);
                out.set("has_material_hints", reference.has_material_hints);
                out.set("blocking_gap", "WORLD route/weight diagnostics use accepted reference height for owner preview; full procedural WORLD height remains open until async/cache production");
            } else {
                out.set("blocking_gap", "WORLD composes runtime-biome pages but does not own the accepted mountain pass-network or conditioning facts");
            }
        } else if matches!(self.active_producer_kind(), Some(ProducerKind::SingleBiome)) {
            let layer_ref = self.mountain_layer_ref.as_ref();
            out.set(
                "contract_kind",
                if layer_ref.is_some() {
                    "single_mountain_world_layer_reference_bridge"
                } else {
                    "single_seam_safe_mountain_page_recipe"
                },
            );
            out.set("source_scope", "display_to_source_transform_page_synthesis");
            out.set("explicit_live_candidate", true);
            out.set("live_world_layer_candidate", true);
            out.set("has_source_display_mapping", true);
            out.set("has_mountain_macro_field", true);
            out.set("has_bound_world_layer_reference", layer_ref.is_some());
            out.set("height_consumes_world_layer_facts", layer_ref.is_some());
            if let Some(bound_reference) = layer_ref {
                let reference = bound_reference.reference();
                let has_pass_network = reference.pass_network_routes > 0;
                let has_route_carving = reference.pass_network_carved_frac > 0.0;
                let has_conditioning = reference_has_conditioning(reference);
                out.set("reference_source_scope", reference.source_scope.clone());
                out.set("height_source", "bound_world_layer_reference_payload");
                out.set("procedural_world_layer_height", false);
                out.set(
                    "has_source_display_mapping",
                    reference_has_source_display_mapping(reference),
                );
                out.set("has_pass_network_routes", has_pass_network);
                out.set("has_route_carving", has_route_carving);
                out.set("has_page_stable_conditioning", has_conditioning);
                out.set("has_material_hints", reference.has_material_hints);
                out.set("blocking_gap", "height/material/facts are reference-backed for owner visual recovery; live procedural GPU height and facts/collision story remain open");
            } else {
                out.set("blocking_gap", "missing pass-network routes, route carving, page-stable conditioning, material hints, and facts/collision story");
            }
        } else {
            out.set("contract_kind", "legacy_dem_kernel_atlas");
            out.set("source_scope", "legacy_kernel_sampling");
            out.set(
                "blocking_gap",
                "legacy atlas path is not the accepted mountain world-layer producer",
            );
        }

        out
    }
}

fn reference_has_conditioning(reference: &StaticHeightRuntime) -> bool {
    reference.has_conditioning_stats
        && reference.conditioning_stats.source_ptp > 0.0
        && reference.conditioning_stats.conditioned_ptp > 0.0
}

fn reference_has_source_display_mapping(reference: &StaticHeightRuntime) -> bool {
    reference.source_origin_x_m.is_finite()
        && reference.source_origin_z_m.is_finite()
        && reference.source_span_x_m.is_finite()
        && reference.source_span_z_m.is_finite()
        && reference.source_scene_ratio.is_finite()
        && reference.source_span_x_m > 0.0
        && reference.source_span_z_m > 0.0
        && reference.source_scene_ratio > 0.0
}
