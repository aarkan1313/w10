//! Read-only and policy-state Godot API methods for `Wg10PagePool`.

use godot::classes::Texture2Drd;
use godot::prelude::*;

use crate::page_policy::PageKey;

use super::producer::ProducerKind;
use super::Wg10PagePool;

#[godot_api(secondary)]
impl Wg10PagePool {
    /// True when the pool is producing pages via the GPU biome path.
    #[func]
    pub fn uses_biome_path(&self) -> bool {
        self.uses_active_biome_path()
    }

    /// Human-readable active producer mode for diagnostics/HUDs.
    #[func]
    pub fn biome_runtime_mode(&self) -> GString {
        GString::from(self.active_runtime_mode_label())
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
        if !matches!(
            self.active_producer_kind(),
            Some(ProducerKind::SingleBiome | ProducerKind::World)
        ) {
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

    /// Region-fact producer stats for HUD/review: how many region grids are cached, how many
    /// super-region bakes are in flight, and whether the region-fact producer is active. Read-only.
    #[func]
    pub fn region_fact_stats(&self) -> Dictionary<GString, Variant> {
        let mut d = Dictionary::<GString, Variant>::new();
        d.set("active", self.region_cfg.is_some());
        d.set("cached_regions", self.region_cache.len() as i64);
        d.set("baking_in_flight", self.region_baking.len() as i64);
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

    pub(crate) fn get_resident_static_material_page(
        &self,
        level: i64,
        origin_x: f64,
        origin_z: f64,
    ) -> Option<Gd<Texture2Drd>> {
        if !self.has_active_presentation_materials() {
            return None;
        }
        let policy = self.policy.as_ref()?;
        let key = PageKey {
            level: level as i32,
            origin_x: origin_x as i64,
            origin_z: origin_z as i64,
        };
        let slot = policy.slot_of(&key)?;
        self.slot_material_wrap[slot].clone()
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
