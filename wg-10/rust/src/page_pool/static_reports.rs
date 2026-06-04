//! Static-reference and bound world-layer sampled reports.

use godot::prelude::*;

use super::{StaticHeightRuntime, Wg10PagePool};

#[godot_api(secondary)]
impl Wg10PagePool {
    /// Diagnostic report for the accepted world-layer facts bound beside live MOUNTAIN.
    ///
    /// Empty when no reference facts are bound.
    #[func]
    pub fn mountain_world_layer_reference_report(&self) -> Dictionary<GString, Variant> {
        let Some(reference) = self.mountain_layer_ref.as_ref() else {
            return Dictionary::<GString, Variant>::new();
        };
        reference_report_dict(reference.reference())
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
        let reference = reference.reference();
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
        let reference = reference.reference();
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
            reference.reference(),
            self.world_span,
            level,
            origin_x,
            origin_z,
            samples_px,
        )
    }
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

fn reference_report_dict(reference: &StaticHeightRuntime) -> Dictionary<GString, Variant> {
    let mut out = Dictionary::<GString, Variant>::new();
    out.set("generator_version", reference.generator_version.clone());
    out.set("source_scope", reference.source_scope.clone());
    out.set("height_scale_m", reference.height_scale_m);
    out.set("feature_span_m", reference.feature_span_m);
    out.set(
        "has_source_display_mapping",
        reference_has_source_display_mapping(reference),
    );
    out.set("display_origin_x_m", reference.origin_x_m);
    out.set("display_origin_z_m", reference.origin_z_m);
    out.set("display_span_x_m", reference.span_x_m);
    out.set("display_span_z_m", reference.span_z_m);
    out.set("source_origin_x_m", reference.source_origin_x_m);
    out.set("source_origin_z_m", reference.source_origin_z_m);
    out.set("source_span_x_m", reference.source_span_x_m);
    out.set("source_span_z_m", reference.source_span_z_m);
    out.set("source_scene_ratio", reference.source_scene_ratio);
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
