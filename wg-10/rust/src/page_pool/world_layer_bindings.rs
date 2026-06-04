//! Binding accepted world-layer reference payloads beside active producers.

use godot::prelude::*;
use std::path::Path;

use super::producer::ProducerKind;
use super::world_layer_reference::BoundWorldLayerReference;
use super::Wg10PagePool;

#[godot_api(secondary)]
impl Wg10PagePool {
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
            match BoundWorldLayerReference::from_json_path(Path::new(&payload_path.to_string())) {
                Ok(reference) => reference,
                Err(e) => return GString::from(&e),
            };
        let source_transform = reference.source_transform();
        self.biome_source_scale = source_transform.scale;
        self.biome_source_offset_x_m = source_transform.offset_x_m;
        self.biome_source_offset_z_m = source_transform.offset_z_m;
        self.mountain_layer_ref = Some(reference);
        GString::new()
    }

    /// Bind the accepted mountain reference as the owner-facing WORLD preview height/materials.
    ///
    /// WORLD route and weight diagnostics still come from `configure_biome_world`; this only
    /// replaces the synchronous owner-fly height page with the accepted reference height so mode 3
    /// does not present the known one-biome-per-page WORLD compose artifact as terrain.
    #[func]
    pub fn bind_world_preview_reference(&mut self, payload_path: GString) -> GString {
        if !matches!(self.active_producer_kind(), Some(ProducerKind::World)) {
            return GString::from(
                "bind_world_preview_reference: pool is not configured for WORLD biome synthesis",
            );
        }
        let reference =
            match BoundWorldLayerReference::from_json_path(Path::new(&payload_path.to_string())) {
                Ok(reference) => reference,
                Err(e) => return GString::from(&e),
            };
        self.world_preview_ref = Some(reference);
        GString::new()
    }

    pub(crate) fn has_world_preview_reference(&self) -> bool {
        matches!(self.active_producer_kind(), Some(ProducerKind::World))
            && self.world_preview_ref.is_some()
    }
}
