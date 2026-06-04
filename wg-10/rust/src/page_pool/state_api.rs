//! Read-only and policy-state Godot API methods for `Wg10PagePool`.

use godot::classes::Texture2Drd;
use godot::prelude::*;

use crate::page_policy::PageKey;

use super::Wg10PagePool;

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
        if self.biome_world.is_some() {
            GString::from("world")
        } else if self.biome_ctx.is_some() {
            GString::from("single")
        } else {
            GString::from("legacy")
        }
    }

    /// Diagnostic: return the runtime biome selected for a world-routed page.
    ///
    /// This mirrors the exact page-center selector used by `acquire_page`. It is deliberately
    /// read-only and does not allocate or dispatch page compute.
    #[func]
    pub fn debug_world_biome_for_page(&self, level: i64, origin_x: f64, origin_z: f64) -> GString {
        let Some(world) = self.biome_world.as_ref() else {
            return GString::new();
        };
        let world_span = self.world_span * 2f64.powi(level as i32);
        GString::from(&self.select_world_biome_name(world, origin_x, origin_z, world_span))
    }

    /// Diagnostic: report page-center runtime-biome weight stats used by WORLD routing.
    ///
    /// This intentionally mirrors the current hard page selector. If `runner_up_weight` is
    /// material, the current runtime is discarding real grammar weight that the compose producer
    /// should eventually consume instead of choosing one biome for the whole page.
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

        let selected_weight = ranked.first().map(|(_, weight)| **weight).unwrap_or(0.0);
        let runner_up_weight = ranked.get(1).map(|(_, weight)| **weight).unwrap_or(0.0);
        let weight_sum: f64 = weights.values().sum();
        let active_count = weights.values().filter(|w| **w > 1.0e-9).count() as i64;

        out.set("selected_weight", selected_weight);
        out.set("runner_up_weight", runner_up_weight);
        out.set("active_count", active_count);
        out.set("weight_sum", weight_sum);
        out.set("center_x", center_x);
        out.set("center_z", center_z);
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
