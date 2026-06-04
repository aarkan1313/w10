//! Page acquisition compute and rollback helpers for `Wg10PagePool`.

use godot::classes::{RenderingDevice, RenderingServer, Texture2Drd};
use godot::prelude::*;

use crate::grammar;
use crate::page_compute::compute_page_cached;
use crate::page_policy::{Decision, PageKey};

use super::{BiomeWorldRuntime, Wg10PagePool};

#[godot_api(secondary)]
impl Wg10PagePool {
    /// Acquire or compute the page texture for `(level, origin_x, origin_z)`.
    ///
    /// Cache hits return the resident `Texture2Drd`. Misses create or reuse an
    /// owned R32F texture and dispatch the active producer into it.
    #[func]
    pub fn acquire_page(
        &mut self,
        level: i64,
        origin_x: f64,
        origin_z: f64,
    ) -> Option<Gd<Texture2Drd>> {
        if !self.is_configured() {
            godot_error!("Wg10PagePool: acquire_page called before configure()");
            return None;
        }

        let mut rd = match RenderingServer::singleton().get_rendering_device() {
            Some(r) => r,
            None => {
                godot_error!(
                    "Wg10PagePool: global RenderingDevice unavailable (windowed-only mode)"
                );
                return None;
            }
        };

        let key = PageKey {
            level: level as i32,
            origin_x: origin_x as i64,
            origin_z: origin_z as i64,
        };
        let decision = self.policy.as_mut().unwrap().acquire(key);

        let span_l = self.world_span * 2f64.powi(level as i32);
        let (ox, oz, ws, ppx, sd) = (origin_x, origin_z, span_l, self.page_px, self.seed);
        let flow_on = level < self.biome_flow_max_level;

        match decision {
            Decision::Reuse(slot) => {
                self.reused += 1;
                self.slot_wrap[slot].clone()
            }

            Decision::Allocate(slot) => {
                let tex_rid = match self.create_page_texture(&mut rd) {
                    Some(rid) => rid,
                    None => {
                        self.policy.as_mut().unwrap().rollback(key);
                        return None;
                    }
                };

                if let Err(e) =
                    self.dispatch_page_compute(&mut rd, tex_rid, ox, oz, ws, ppx, sd, flow_on)
                {
                    godot_error!("Wg10PagePool: compute_page_cached failed (slot {slot}): {e}");
                    self.rollback_failed_allocate(&mut rd, key, tex_rid);
                    return None;
                }

                let mut wrap = Texture2Drd::new_gd();
                wrap.set_texture_rd_rid(tex_rid);

                self.slot_tex[slot] = Some(tex_rid);
                self.slot_wrap[slot] = Some(wrap.clone());
                self.created += 1;
                Some(wrap)
            }

            Decision::AllocateEvicting { slot, evicted: _ } => {
                let tex_rid = self.slot_tex[slot].expect("AllocateEvicting: slot must be occupied");

                if let Err(e) =
                    self.dispatch_page_compute(&mut rd, tex_rid, ox, oz, ws, ppx, sd, flow_on)
                {
                    godot_error!(
                        "Wg10PagePool: compute_page_cached failed on eviction (slot {slot}): {e}"
                    );
                    self.rollback_failed_eviction(&mut rd, key, slot);
                    return None;
                }

                self.recomputed += 1;
                self.slot_wrap[slot].clone()
            }

            Decision::Full => {
                self.full_events += 1;
                godot_warn!("Wg10PagePool: all slots protected, returning null (Full)");
                None
            }
        }
    }
}

impl Wg10PagePool {
    /// Dispatch the active producer path into an already-owned page texture RID.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn dispatch_page_compute(
        &self,
        rd: &mut Gd<RenderingDevice>,
        tex_rid: Rid,
        origin_x: f64,
        origin_z: f64,
        world_span: f64,
        page_px: i64,
        seed: i64,
        flow_on: bool,
    ) -> Result<(), String> {
        if self.use_biome_path {
            let bctx = if let Some(world) = self.biome_world.as_ref() {
                self.select_world_biome_context(world, origin_x, origin_z, world_span)?
            } else {
                self.biome_ctx.as_ref().unwrap()
            };
            crate::biome_page_compute::compute_biome_page_cached(
                rd,
                bctx,
                tex_rid,
                origin_x,
                origin_z,
                world_span,
                page_px,
                self.biome_feature_span_m,
                seed,
                flow_on,
            )
        } else {
            let ctx = self.compute_ctx.as_ref().unwrap();
            let num_palettes = self.pack_buffers.as_ref().unwrap().num_palettes;
            compute_page_cached(
                rd,
                ctx,
                &self.pack.as_ref().unwrap().grammar_constants,
                num_palettes,
                tex_rid,
                origin_x,
                origin_z,
                world_span,
                page_px,
                seed,
            )
        }
    }

    fn select_world_biome_context<'a>(
        &self,
        world: &'a BiomeWorldRuntime,
        origin_x: f64,
        origin_z: f64,
        world_span: f64,
    ) -> Result<&'a crate::biome_page_compute::BiomePageComputeContext, String> {
        let cx = origin_x + world_span * 0.5;
        let cz = origin_z + world_span * 0.5;
        let weights = grammar::family_weights(cx, cz, self.seed, &world.pack);
        let mut by_biome: std::collections::BTreeMap<&str, f64> = std::collections::BTreeMap::new();
        for &(family_idx, weight) in weights.entries() {
            let Some(family_id) = world.pack.family_ids.get(family_idx as usize) else {
                continue;
            };
            let biome = runtime_biome_from_family_id(family_id);
            if world.contexts.contains_key(biome) {
                *by_biome.entry(biome).or_insert(0.0) += weight;
            }
        }

        let selected = by_biome
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(biome, _)| *biome)
            .unwrap_or("mountain");
        world
            .contexts
            .get(selected)
            .or_else(|| world.contexts.get("mountain"))
            .ok_or_else(|| {
                format!(
                    "select_world_biome_context: no context for selected biome '{selected}' and no mountain fallback"
                )
            })
    }

    /// Roll back a failed compute into a newly-created texture.
    pub(super) fn rollback_failed_allocate(
        &mut self,
        rd: &mut Gd<RenderingDevice>,
        key: PageKey,
        tex_rid: Rid,
    ) {
        rd.free_rid(tex_rid);
        self.policy.as_mut().unwrap().rollback(key);
    }

    /// Roll back a failed recompute into an evicted slot.
    pub(super) fn rollback_failed_eviction(
        &mut self,
        rd: &mut Gd<RenderingDevice>,
        key: PageKey,
        slot: usize,
    ) {
        if let Some(old_rid) = self.slot_tex[slot].take() {
            rd.free_rid(old_rid);
        }
        self.slot_wrap[slot] = None;
        self.policy.as_mut().unwrap().rollback(key);
    }
}

fn runtime_biome_from_family_id(family_id: &str) -> &str {
    let stem = family_id.split_once("__").map(|(stem, _)| stem).unwrap_or(family_id);
    match stem {
        // Badlands has accepted setup artifacts, but no GPU fragment/schedule in the 11-biome
        // runtime set yet. Desert is the closest available routed fallback until badlands is ported.
        "badlands" => "desert",
        other => other,
    }
}
