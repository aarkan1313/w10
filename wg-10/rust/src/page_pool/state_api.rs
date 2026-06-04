//! Read-only and policy-state Godot API methods for `Wg10PagePool`.

use godot::classes::Texture2Drd;
use godot::prelude::*;

use crate::page_policy::PageKey;

use super::{world_route, Wg10PagePool};

#[godot_api(secondary)]
impl Wg10PagePool {
    /// True when the pool is producing pages via the GPU biome path.
    #[func]
    pub fn uses_biome_path(&self) -> bool {
        self.use_biome_path
    }

    /// Human-readable active producer mode for diagnostics/HUDs.
    #[func]
    pub fn biome_runtime_mode(&self) -> GString {
        if self.static_ref.is_some() {
            GString::from("static_reference")
        } else if self.biome_world.is_some() {
            GString::from("world")
        } else if self.biome_ctx.is_some() {
            GString::from("single")
        } else {
            GString::from("legacy")
        }
    }

    /// Configure the source coordinate transform for live biome synthesis.
    ///
    /// Display page coordinates remain unchanged for the renderer/streamer. The live single-biome
    /// producer samples source coordinates as:
    /// `source = display * source_scale + source_offset`.
    /// This lets review presets synthesize from an accepted large source window while still
    /// displaying the result in the normal clipmap coordinate system. Identity is the default.
    #[func]
    pub fn set_biome_source_transform(
        &mut self,
        source_scale: f64,
        source_offset_x_m: f64,
        source_offset_z_m: f64,
    ) -> GString {
        if !source_scale.is_finite() || source_scale <= 0.0 {
            return GString::from(&format!(
                "set_biome_source_transform: source_scale must be finite and > 0, got {source_scale}"
            ));
        }
        if !source_offset_x_m.is_finite() || !source_offset_z_m.is_finite() {
            return GString::from("set_biome_source_transform: offsets must be finite");
        }
        if !self.use_biome_path || self.static_ref.is_some() {
            return GString::from(
                "set_biome_source_transform: pool is not configured for live biome synthesis",
            );
        }
        self.biome_source_scale = source_scale;
        self.biome_source_offset_x_m = source_offset_x_m;
        self.biome_source_offset_z_m = source_offset_z_m;
        GString::new()
    }

    /// Diagnostic report for the active biome source transform.
    #[func]
    pub fn biome_source_transform(&self) -> Dictionary<GString, Variant> {
        let mut out = Dictionary::<GString, Variant>::new();
        out.set("source_scale", self.biome_source_scale);
        out.set("source_offset_x_m", self.biome_source_offset_x_m);
        out.set("source_offset_z_m", self.biome_source_offset_z_m);
        out
    }

    /// Diagnostic report for the accepted static mountain world-layer payload.
    ///
    /// Empty when the active producer is not `configure_static_reference`.
    #[func]
    pub fn static_reference_report(&self) -> Dictionary<GString, Variant> {
        let mut out = Dictionary::<GString, Variant>::new();
        let Some(reference) = self.static_ref.as_ref() else {
            return out;
        };
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
        let mut out = Dictionary::<GString, Variant>::new();
        let Some(reference) = self.static_ref.as_ref() else {
            return out;
        };
        let samples = samples_px.clamp(2, 65) as usize;
        let world_span = self.world_span * 2f64.powi(level as i32);
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

    /// Unprotect a page, marking it LRU-eligible for eviction.
    #[func]
    pub fn release_page(&mut self, level: i64, origin_x: f64, origin_z: f64) {
        if self.policy.is_none() {
            godot_error!("Wg10PagePool: release_page called before configure()");
            return;
        }
        self.policy.as_mut().unwrap().release(PageKey {
            level: level as i32,
            origin_x: origin_x as i64,
            origin_z: origin_z as i64,
        });
    }

    /// Return pool counters and resident page count.
    #[func]
    pub fn stats(&self) -> Dictionary<GString, Variant> {
        let resident = self
            .policy
            .as_ref()
            .map(|p| p.resident_count() as i64)
            .unwrap_or(0);

        let mut d = Dictionary::<GString, Variant>::new();
        d.set("created", self.created);
        d.set("reused", self.reused);
        d.set("recomputed", self.recomputed);
        d.set("full_events", self.full_events);
        d.set("resident", resident);
        d
    }

    /// Resident page keys as a flat array of `(level, origin_x, origin_z)` triples.
    #[func]
    pub fn resident_keys(&self) -> PackedInt64Array {
        let mut out = PackedInt64Array::new();
        if let Some(policy) = self.policy.as_ref() {
            for key in policy.resident_keys() {
                out.push(key.level as i64);
                out.push(key.origin_x);
                out.push(key.origin_z);
            }
        }
        out
    }

    /// Return an already-resident page without allocating, evicting, or dispatching compute.
    #[func]
    pub fn get_resident_page(
        &self,
        level: i64,
        origin_x: f64,
        origin_z: f64,
    ) -> Option<Gd<Texture2Drd>> {
        let policy = self.policy.as_ref()?;
        let key = PageKey {
            level: level as i32,
            origin_x: origin_x as i64,
            origin_z: origin_z as i64,
        };
        let slot = policy.slot_of(&key)?;
        self.slot_wrap[slot].clone()
    }

    /// Clear all display pins before the view re-pins currently bound pages.
    #[func]
    pub fn clear_display_pins(&mut self) {
        if let Some(p) = self.policy.as_mut() {
            p.clear_display_pins();
        }
    }

    /// Pin a resident page as currently displayed so it cannot be recycled while visible.
    #[func]
    pub fn pin_displayed_page(&mut self, level: i64, origin_x: f64, origin_z: f64) {
        if let Some(p) = self.policy.as_mut() {
            p.pin_displayed(PageKey {
                level: level as i32,
                origin_x: origin_x as i64,
                origin_z: origin_z as i64,
            });
        }
    }

    /// True if a page is both resident and display-pinned.
    #[func]
    pub fn is_displayed_pinned(&self, level: i64, origin_x: f64, origin_z: f64) -> bool {
        self.policy.as_ref().map_or(false, |p| {
            p.is_pinned(&PageKey {
                level: level as i32,
                origin_x: origin_x as i64,
                origin_z: origin_z as i64,
            })
        })
    }
}
