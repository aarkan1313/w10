//! WORLD-mode Godot API and diagnostic reports for `Wg10PagePool`.
//!
//! WORLD routing math lives in `world_route`; this module owns only the
//! Godot-visible knobs and reports that explain the current WORLD preview.

use godot::prelude::*;

use super::{world_route, Wg10PagePool};

#[godot_api(secondary)]
impl Wg10PagePool {
    /// Limit active runtime biomes for WORLD page production.
    ///
    /// `active_limit <= 0` means full compose. The owner fly scene uses a bounded preview until
    /// WORLD page production is off the frame loop; the full multi-biome compose path stays
    /// available for proof gates and future background production.
    #[func]
    pub fn set_biome_world_active_limit(&mut self, active_limit: i64) -> GString {
        let Some(world) = self.biome_world.as_mut() else {
            return GString::from(
                "set_biome_world_active_limit: pool is not configured for WORLD biome synthesis",
            );
        };
        world.active_limit = if active_limit <= 0 {
            usize::MAX
        } else {
            active_limit as usize
        };
        GString::new()
    }

    /// Diagnostic: return the strongest page-center runtime biome for a world-routed page.
    ///
    /// This is deliberately read-only and does not allocate or dispatch page compute. Runtime
    /// WORLD production composes the texel-corner weight field; this selector is only a label/debug
    /// proxy for HUDs and route-color diagnostics.
    #[func]
    pub fn debug_world_biome_for_page(&self, level: i64, origin_x: f64, origin_z: f64) -> GString {
        let Some(world) = self.biome_world.as_ref() else {
            return GString::new();
        };
        let world_span = self.world_span * 2f64.powi(level as i32);
        GString::from(&self.select_world_biome_name(world, origin_x, origin_z, world_span))
    }

    /// Diagnostic: report page-center runtime-biome weight stats used by WORLD route labels.
    ///
    /// Runtime WORLD page production consumes a per-texel weight field. These page-center stats
    /// remain useful for explaining route labels, coarse/fine disagreement, and material runner-up
    /// weight that a whole-page label cannot show.
    #[func]
    pub fn debug_world_biome_report_for_page(
        &self,
        level: i64,
        origin_x: f64,
        origin_z: f64,
    ) -> Dictionary<GString, Variant> {
        let mut out = Dictionary::<GString, Variant>::new();
        let Some(world) = self.biome_world.as_ref() else {
            return out;
        };
        let world_span = self.world_span * 2f64.powi(level as i32);
        let center_x = origin_x + world_span * 0.5;
        let center_z = origin_z + world_span * 0.5;
        let weights = self.world_biome_weights(world, origin_x, origin_z, world_span);

        let mut ranked: Vec<(&String, &f64)> = weights.iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

        let selected = world_route::selected_biome_name(&weights);
        let selected_weight = ranked.first().map(|(_, weight)| **weight).unwrap_or(0.0);
        let runner_up_weight = ranked.get(1).map(|(_, weight)| **weight).unwrap_or(0.0);
        let weight_sum: f64 = weights.values().sum();
        let active_count = weights.values().filter(|w| **w > 1.0e-9).count() as i64;

        let probes = [
            (origin_x, origin_z),
            (origin_x + world_span, origin_z),
            (origin_x, origin_z + world_span),
            (origin_x + world_span, origin_z + world_span),
        ];
        let mut corner_route_mismatches = 0i64;
        let mut max_probe_active_count = active_count;
        let mut min_probe_top_weight = selected_weight;
        let mut max_probe_runner_up_weight = runner_up_weight;
        for (x, z) in probes {
            let probe_weights = self.world_biome_weights_at(world, x, z);
            let probe_selected = world_route::selected_biome_name(&probe_weights);
            if probe_selected != selected {
                corner_route_mismatches += 1;
            }
            let mut probe_ranked: Vec<(&String, &f64)> = probe_weights.iter().collect();
            probe_ranked.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
            let probe_top = probe_ranked
                .first()
                .map(|(_, weight)| **weight)
                .unwrap_or(0.0);
            let probe_runner_up = probe_ranked
                .get(1)
                .map(|(_, weight)| **weight)
                .unwrap_or(0.0);
            let probe_active = probe_weights.values().filter(|w| **w > 1.0e-9).count() as i64;
            max_probe_active_count = max_probe_active_count.max(probe_active);
            min_probe_top_weight = min_probe_top_weight.min(probe_top);
            max_probe_runner_up_weight = max_probe_runner_up_weight.max(probe_runner_up);
        }

        out.set("selected_weight", selected_weight);
        out.set("runner_up_weight", runner_up_weight);
        out.set("active_count", active_count);
        out.set("weight_sum", weight_sum);
        out.set("center_x", center_x);
        out.set("center_z", center_z);
        out.set("corner_route_mismatches", corner_route_mismatches);
        out.set("max_probe_active_count", max_probe_active_count);
        out.set("min_probe_top_weight", min_probe_top_weight);
        out.set("max_probe_runner_up_weight", max_probe_runner_up_weight);
        out
    }

    /// Diagnostic: sample the per-texel runtime-biome weight field consumed by the WORLD compose
    /// producer. This is CPU-side today, but it uses the same texel-corner page mapping as the
    /// runtime page producer and folds grammar families into supported runtime biome names exactly
    /// like the runtime field.
    #[func]
    pub fn debug_world_biome_weight_field_report_for_page(
        &self,
        level: i64,
        origin_x: f64,
        origin_z: f64,
        samples_px: i64,
    ) -> Dictionary<GString, Variant> {
        let mut out = Dictionary::<GString, Variant>::new();
        let Some(world) = self.biome_world.as_ref() else {
            return out;
        };
        let samples = samples_px.clamp(2, 65) as usize;
        let world_span = self.world_span * 2f64.powi(level as i32);
        let field = self.world_biome_weight_field(world, origin_x, origin_z, world_span, samples);
        let n = field.rows * field.cols;
        let mut max_sum_delta = 0.0f32;
        let mut min_sum = f32::INFINITY;
        let mut max_sum = f32::NEG_INFINITY;
        let mut max_texel_active_count = 0i64;
        for idx in 0..n {
            let mut sum = 0.0f32;
            let mut active = 0i64;
            for weights in &field.weights {
                let w = weights[idx];
                sum += w;
                if w > 1.0e-9 {
                    active += 1;
                }
            }
            min_sum = min_sum.min(sum);
            max_sum = max_sum.max(sum);
            max_sum_delta = max_sum_delta.max((sum - 1.0).abs());
            max_texel_active_count = max_texel_active_count.max(active);
        }

        out.set("rows", field.rows as i64);
        out.set("cols", field.cols as i64);
        out.set("sample_count", n as i64);
        out.set("active_biomes", field.names.len() as i64);
        out.set("max_texel_active_count", max_texel_active_count);
        out.set("min_sum", min_sum as f64);
        out.set("max_sum", max_sum as f64);
        out.set("max_sum_delta", max_sum_delta as f64);
        out
    }
}
