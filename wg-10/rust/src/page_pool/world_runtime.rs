//! WORLD runtime state owned by the page pool.
//!
//! Generic pool code owns texture slots and policy. This module owns the
//! grammar-routed WORLD producer's cached pack/context bundle and its teardown.

use std::collections::BTreeMap;

use godot::classes::RenderingDevice;
use godot::prelude::*;

use crate::biome_page_compute;
use crate::pack;

pub(super) struct BiomeWorldRuntime {
    pub(super) pack: pack::Pack,
    pub(super) contexts: BTreeMap<String, biome_page_compute::BiomePageComputeContext>,
    pub(super) compose_ctx: biome_page_compute::BiomePageComputeContext,
    pub(super) active_limit: usize,
}

impl BiomeWorldRuntime {
    pub(super) fn new(
        pack: pack::Pack,
        contexts: BTreeMap<String, biome_page_compute::BiomePageComputeContext>,
        compose_ctx: biome_page_compute::BiomePageComputeContext,
    ) -> Self {
        Self {
            pack,
            contexts,
            compose_ctx,
            active_limit: usize::MAX,
        }
    }

    pub(super) fn free(self, rd: &mut Gd<RenderingDevice>) {
        for (_, ctx) in self.contexts {
            biome_page_compute::free_biome_page_context(rd, &ctx);
        }
        biome_page_compute::free_biome_page_context(rd, &self.compose_ctx);
    }
}
