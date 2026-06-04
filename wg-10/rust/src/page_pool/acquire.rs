//! Page acquisition compute and rollback helpers for `Wg10PagePool`.

use godot::classes::{RenderingDevice, RenderingServer, Texture2Drd};
use godot::prelude::*;

use crate::page_compute::compute_page_cached;
use crate::page_policy::{Decision, PageKey};

use super::{world_route, BiomeWorldRuntime, Wg10PagePool};

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
            if let Some(static_ref) = self.static_ref.as_ref() {
                static_ref.write_page_texture(
                    rd,
                    tex_rid,
                    origin_x,
                    origin_z,
                    world_span,
                    page_px,
                )
            } else if let Some(world) = self.biome_world.as_ref() {
                let field = self.world_biome_weight_field(
                    world,
                    origin_x,
                    origin_z,
                    world_span,
                    page_px as usize,
                );
                crate::biome_page_compute::compute_biome_world_page_composed(
                    rd,
                    &world.contexts,
                    &world.compose_ctx,
                    tex_rid,
                    origin_x,
                    origin_z,
                    world_span,
                    page_px,
                    self.biome_feature_span_m,
                    seed,
                    flow_on,
                    &field.names,
                    &field.weights,
                )
            } else {
                crate::biome_page_compute::compute_biome_page_cached(
                    rd,
                    self.biome_ctx.as_ref().unwrap(),
                    tex_rid,
                    origin_x,
                    origin_z,
                    world_span,
                    page_px,
                    self.biome_feature_span_m,
                    seed,
                    flow_on,
                )
            }
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

    pub(super) fn select_world_biome_name(
        &self,
        world: &BiomeWorldRuntime,
        origin_x: f64,
        origin_z: f64,
        world_span: f64,
    ) -> String {
        let supported = |biome: &str| world.contexts.contains_key(biome);
        world_route::select_biome_name_for_page(
            self.seed,
            &world.pack,
            origin_x,
            origin_z,
            world_span,
            &supported,
        )
    }

    pub(super) fn world_biome_weights(
        &self,
        world: &BiomeWorldRuntime,
        origin_x: f64,
        origin_z: f64,
        world_span: f64,
    ) -> std::collections::BTreeMap<String, f64> {
        let supported = |biome: &str| world.contexts.contains_key(biome);
        world_route::biome_weights_for_page(
            self.seed,
            &world.pack,
            origin_x,
            origin_z,
            world_span,
            &supported,
        )
    }

    pub(super) fn world_biome_weights_at(
        &self,
        world: &BiomeWorldRuntime,
        x: f64,
        z: f64,
    ) -> std::collections::BTreeMap<String, f64> {
        let supported = |biome: &str| world.contexts.contains_key(biome);
        world_route::biome_weights_at_point(self.seed, &world.pack, x, z, &supported)
    }

    pub(super) fn world_biome_weight_field(
        &self,
        world: &BiomeWorldRuntime,
        origin_x: f64,
        origin_z: f64,
        world_span: f64,
        page_px: usize,
    ) -> world_route::BiomeWeightField {
        let supported = |biome: &str| world.contexts.contains_key(biome);
        world_route::biome_weight_field_for_page(
            self.seed,
            &world.pack,
            origin_x,
            origin_z,
            world_span,
            page_px,
            &supported,
        )
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
