//! Active producer classification and dispatch for `Wg10PagePool`.
//!
//! The pool still owns page texture slots and policy. This module owns the
//! "which runtime produces this page?" decision so acquisition/lifecycle code
//! does not duplicate implicit `Option` routing.

use godot::classes::{RenderingDevice, Texture2Drd};
use godot::prelude::*;
use std::collections::BTreeMap;

use crate::page_compute::compute_page_cached;

use super::{world_route, BiomeWorldRuntime, Wg10PagePool};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProducerKind {
    Legacy,
    SingleBiome,
    World,
    StaticReference,
}

impl ProducerKind {
    pub(super) fn runtime_mode(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::SingleBiome => "single",
            Self::World => "world",
            Self::StaticReference => "static_reference",
        }
    }

    pub(super) fn uses_biome_path(self) -> bool {
        !matches!(self, Self::Legacy)
    }
}

impl Wg10PagePool {
    pub(super) fn active_producer_kind(&self) -> Option<ProducerKind> {
        if self.static_ref.is_some() {
            Some(ProducerKind::StaticReference)
        } else if self.biome_world.is_some() {
            Some(ProducerKind::World)
        } else if self.biome_ctx.is_some() {
            Some(ProducerKind::SingleBiome)
        } else if self.pack.is_some()
            && self.pack_buffers.is_some()
            && self.glsl_source.is_some()
            && self.compute_ctx.is_some()
        {
            Some(ProducerKind::Legacy)
        } else {
            None
        }
    }

    pub(super) fn active_runtime_mode_label(&self) -> &'static str {
        self.active_producer_kind()
            .map(ProducerKind::runtime_mode)
            .unwrap_or("legacy")
    }

    pub(super) fn uses_active_biome_path(&self) -> bool {
        self.active_producer_kind()
            .is_some_and(ProducerKind::uses_biome_path)
    }

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
        match self.active_producer_kind() {
            Some(ProducerKind::StaticReference) => {
                let static_ref = self
                    .static_ref
                    .as_ref()
                    .ok_or("static reference producer missing runtime")?;
                static_ref.write_page_texture(rd, tex_rid, origin_x, origin_z, world_span, page_px)
            }
            Some(ProducerKind::World) => {
                let world = self
                    .biome_world
                    .as_ref()
                    .ok_or("WORLD producer missing runtime")?;
                let field =
                    self.world_biome_weight_field(world, origin_x, origin_z, world_span, page_px as usize);
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
            }
            Some(ProducerKind::SingleBiome) => {
                let ctx = self
                    .biome_ctx
                    .as_ref()
                    .ok_or("single-biome producer missing runtime")?;
                let source_origin_x =
                    origin_x * self.biome_source_scale + self.biome_source_offset_x_m;
                let source_origin_z =
                    origin_z * self.biome_source_scale + self.biome_source_offset_z_m;
                let source_world_span = world_span * self.biome_source_scale;
                crate::biome_page_compute::compute_biome_page_cached(
                    rd,
                    ctx,
                    tex_rid,
                    source_origin_x,
                    source_origin_z,
                    source_world_span,
                    page_px,
                    self.biome_feature_span_m,
                    seed,
                    flow_on,
                )
            }
            Some(ProducerKind::Legacy) => {
                let ctx = self
                    .compute_ctx
                    .as_ref()
                    .ok_or("legacy producer missing compute context")?;
                let pack = self
                    .pack
                    .as_ref()
                    .ok_or("legacy producer missing pack")?;
                let pack_buffers = self
                    .pack_buffers
                    .as_ref()
                    .ok_or("legacy producer missing pack buffers")?;
                compute_page_cached(
                    rd,
                    ctx,
                    &pack.grammar_constants,
                    pack_buffers.num_palettes,
                    tex_rid,
                    origin_x,
                    origin_z,
                    world_span,
                    page_px,
                    seed,
                )
            }
            None => Err("Wg10PagePool: no configured page producer".into()),
        }
    }

    pub(super) fn refresh_static_material_texture(
        &mut self,
        rd: &mut Gd<RenderingDevice>,
        slot: usize,
        origin_x: f64,
        origin_z: f64,
        world_span: f64,
        page_px: i64,
    ) -> Result<(), String> {
        let wants_material = self
            .static_ref
            .as_ref()
            .is_some_and(|reference| reference.has_presentation_materials());
        if !wants_material {
            return Ok(());
        }

        let tex_rid = match self.slot_material_tex[slot] {
            Some(rid) => rid,
            None => {
                let rid = self
                    .create_static_material_texture(rd)
                    .ok_or("static reference: material texture_create failed")?;
                let mut wrap = Texture2Drd::new_gd();
                wrap.set_texture_rd_rid(rid);
                self.slot_material_tex[slot] = Some(rid);
                self.slot_material_wrap[slot] = Some(wrap);
                rid
            }
        };

        let Some(static_ref) = self.static_ref.as_ref() else {
            return Ok(());
        };
        static_ref.write_material_page_texture(
            rd,
            tex_rid,
            origin_x,
            origin_z,
            world_span,
            page_px,
        )
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
    ) -> BTreeMap<String, f64> {
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
    ) -> BTreeMap<String, f64> {
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
        if world.active_limit == 1 {
            return world_route::single_biome_weight_field_for_page(
                self.seed,
                &world.pack,
                origin_x,
                origin_z,
                world_span,
                page_px,
                &supported,
            );
        }
        world_route::biome_weight_field_for_page(
            self.seed,
            &world.pack,
            origin_x,
            origin_z,
            world_span,
            page_px,
            world.active_limit,
            &supported,
        )
    }
}
