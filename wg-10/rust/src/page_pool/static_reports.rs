//! Static-reference and mountain-world-layer diagnostic reports.

use godot::prelude::*;
use std::path::Path;

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
            let has_conditioning = reference.has_conditioning_stats
                && reference.conditioning_stats.source_ptp > 0.0
                && reference.conditioning_stats.conditioned_ptp > 0.0;
            out.set("contract_kind", "accepted_static_reference_visual_baseline");
            out.set("source_scope", reference.source_scope.clone());
            out.set("accepted_visual_baseline", true);
            out.set("has_source_display_mapping", true);
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
            out.set("contract_kind", "grammar_routed_runtime_biome_composition");
            out.set("source_scope", "grammar_routed_page_weight_field");
            out.set("has_source_display_mapping", true);
            out.set("has_mountain_macro_field", true);
            out.set("blocking_gap", "WORLD composes runtime-biome pages but does not own the accepted mountain pass-network or conditioning facts");
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
            if let Some(reference) = layer_ref {
                let has_pass_network = reference.pass_network_routes > 0;
                let has_route_carving = reference.pass_network_carved_frac > 0.0;
                let has_conditioning = reference_has_conditioning(reference);
                out.set("reference_source_scope", reference.source_scope.clone());
                out.set("height_source", "bound_world_layer_reference_payload");
                out.set("procedural_world_layer_height", false);
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

    /// Bind the accepted mountain world-layer payload as a fact cache beside the live
    /// single-biome MOUNTAIN producer.
    ///
    /// This does not replace live GPU height production. It exposes page-stable corridor,
    /// material, route, and conditioning facts for reports/material presentation while the live
    /// height producer is still being ported to consume those facts.
    #[func]
    pub fn bind_mountain_world_layer_reference(&mut self, payload_path: GString) -> GString {
        if !matches!(self.active_producer_kind(), Some(ProducerKind::SingleBiome)) {
            return GString::from(
                "bind_mountain_world_layer_reference: pool is not configured for live single-biome synthesis",
            );
        }
        let reference =
            match StaticHeightRuntime::from_json_path(Path::new(&payload_path.to_string())) {
                Ok(reference) => reference,
                Err(e) => return GString::from(&e),
            };
        self.mountain_layer_ref = Some(reference);
        GString::new()
    }

    /// Diagnostic report for the accepted world-layer facts bound beside live MOUNTAIN.
    ///
    /// Empty when no reference facts are bound.
    #[func]
    pub fn mountain_world_layer_reference_report(&self) -> Dictionary<GString, Variant> {
        let Some(reference) = self.mountain_layer_ref.as_ref() else {
            return Dictionary::<GString, Variant>::new();
        };
        reference_report_dict(reference)
    }

    /// Diagnostic report for the accepted static mountain world-layer payload.
    ///
    /// Empty when the active producer is not `configure_static_reference`.
    #[func]
    pub fn static_reference_report(&self) -> Dictionary<GString, Variant> {
        let Some(reference) = self.static_ref.as_ref() else {
            return Dictionary::<GString, Variant>::new();
        };
        reference_report_dict(reference)
    }

    pub(crate) fn static_reference_corridor_fraction_for_page(
        &self,
        level: i64,
        origin_x: f64,
        origin_z: f64,
        samples_px: usize,
    ) -> Option<f64> {
        let reference = self.static_ref.as_ref()?;
        if !reference.has_corridor {
            return None;
        }
        let world_span = self.world_span * 2f64.powi(level as i32);
        Some(reference.corridor_fraction_for_page(origin_x, origin_z, world_span, samples_px))
    }

    pub(crate) fn mountain_world_layer_corridor_fraction_for_page(
        &self,
        level: i64,
        origin_x: f64,
        origin_z: f64,
        samples_px: usize,
    ) -> Option<f64> {
        let reference = self.mountain_layer_ref.as_ref()?;
        if !reference.has_corridor {
            return None;
        }
        let world_span = self.world_span * 2f64.powi(level as i32);
        Some(reference.corridor_fraction_for_page(origin_x, origin_z, world_span, samples_px))
    }

    pub(crate) fn static_reference_material_hint_means_for_page(
        &self,
        level: i64,
        origin_x: f64,
        origin_z: f64,
        samples_px: usize,
    ) -> Option<(f64, f64, f64, f64)> {
        let reference = self.static_ref.as_ref()?;
        if !reference.has_material_hints {
            return None;
        }
        let world_span = self.world_span * 2f64.powi(level as i32);
        let hints = reference
            .material_hint_fractions_for_page(origin_x, origin_z, world_span, samples_px)?;
        Some((hints.low_pass, hints.floor, hints.rock, hints.snow))
    }

    pub(crate) fn mountain_world_layer_material_hint_means_for_page(
        &self,
        level: i64,
        origin_x: f64,
        origin_z: f64,
        samples_px: usize,
    ) -> Option<(f64, f64, f64, f64)> {
        let reference = self.mountain_layer_ref.as_ref()?;
        if !reference.has_material_hints {
            return None;
        }
        let world_span = self.world_span * 2f64.powi(level as i32);
        let hints = reference
            .material_hint_fractions_for_page(origin_x, origin_z, world_span, samples_px)?;
        Some((hints.low_pass, hints.floor, hints.rock, hints.snow))
    }

    /// Diagnostic report for accepted static-reference facts sampled over one runtime page.
    ///
    /// Empty when the active producer is not `configure_static_reference`.
    #[func]
    pub fn static_reference_page_report(
        &self,
        level: i64,
        origin_x: f64,
        origin_z: f64,
        samples_px: i64,
    ) -> Dictionary<GString, Variant> {
        let Some(reference) = self.static_ref.as_ref() else {
            return Dictionary::<GString, Variant>::new();
        };
        reference_page_report_dict(
            reference,
            self.world_span,
            level,
            origin_x,
            origin_z,
            samples_px,
        )
    }

    /// Diagnostic report for accepted world-layer facts sampled over one live MOUNTAIN page.
    ///
    /// Empty when no reference facts are bound beside the live producer.
    #[func]
    pub fn mountain_world_layer_reference_page_report(
        &self,
        level: i64,
        origin_x: f64,
        origin_z: f64,
        samples_px: i64,
    ) -> Dictionary<GString, Variant> {
        let Some(reference) = self.mountain_layer_ref.as_ref() else {
            return Dictionary::<GString, Variant>::new();
        };
        reference_page_report_dict(
            reference,
            self.world_span,
            level,
            origin_x,
            origin_z,
            samples_px,
        )
    }
}

fn reference_has_conditioning(reference: &StaticHeightRuntime) -> bool {
    reference.has_conditioning_stats
        && reference.conditioning_stats.source_ptp > 0.0
        && reference.conditioning_stats.conditioned_ptp > 0.0
}

fn reference_report_dict(reference: &StaticHeightRuntime) -> Dictionary<GString, Variant> {
    let mut out = Dictionary::<GString, Variant>::new();
    out.set("generator_version", reference.generator_version.clone());
    out.set("source_scope", reference.source_scope.clone());
    out.set("height_scale_m", reference.height_scale_m);
    out.set("feature_span_m", reference.feature_span_m);
    out.set("has_corridor", reference.has_corridor);
    out.set("corridor_frac", reference.corridor_frac);
    out.set("has_material_hints", reference.has_material_hints);
    out.set("low_pass_hint_frac", reference.material_hint_fracs.low_pass);
    out.set("floor_hint_frac", reference.material_hint_fracs.floor);
    out.set("rock_hint_frac", reference.material_hint_fracs.rock);
    out.set("snow_hint_frac", reference.material_hint_fracs.snow);
    out.set("pass_network_routes", reference.pass_network_routes);
    out.set(
        "pass_network_walkable_frac",
        reference.pass_network_walkable_frac,
    );
    out.set(
        "pass_network_carved_frac",
        reference.pass_network_carved_frac,
    );
    out.set("has_conditioning_stats", reference.has_conditioning_stats);
    out.set(
        "conditioning_source_min",
        reference.conditioning_stats.source_min,
    );
    out.set(
        "conditioning_source_max",
        reference.conditioning_stats.source_max,
    );
    out.set(
        "conditioning_source_ptp",
        reference.conditioning_stats.source_ptp,
    );
    out.set("conditioning_p05", reference.conditioning_stats.p05);
    out.set("conditioning_p50", reference.conditioning_stats.p50);
    out.set("conditioning_p95", reference.conditioning_stats.p95);
    out.set(
        "conditioning_min",
        reference.conditioning_stats.conditioned_min,
    );
    out.set(
        "conditioning_max",
        reference.conditioning_stats.conditioned_max,
    );
    out.set(
        "conditioning_ptp",
        reference.conditioning_stats.conditioned_ptp,
    );
    out
}

fn reference_page_report_dict(
    reference: &StaticHeightRuntime,
    base_world_span: f64,
    level: i64,
    origin_x: f64,
    origin_z: f64,
    samples_px: i64,
) -> Dictionary<GString, Variant> {
    let mut out = Dictionary::<GString, Variant>::new();
    let samples = samples_px.clamp(2, 65) as usize;
    let world_span = base_world_span * 2f64.powi(level as i32);
    out.set("level", level);
    out.set("origin_x", origin_x);
    out.set("origin_z", origin_z);
    out.set("world_span_m", world_span);
    out.set("samples_px", samples as i64);
    out.set("has_corridor", reference.has_corridor);
    out.set(
        "corridor_frac",
        reference.corridor_fraction_for_page(origin_x, origin_z, world_span, samples),
    );
    out.set("has_material_hints", reference.has_material_hints);
    if let Some(hints) =
        reference.material_hint_fractions_for_page(origin_x, origin_z, world_span, samples)
    {
        out.set("low_pass_hint_mean", hints.low_pass);
        out.set("floor_hint_mean", hints.floor);
        out.set("rock_hint_mean", hints.rock);
        out.set("snow_hint_mean", hints.snow);
    }
    out
}
