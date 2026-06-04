//! Godot-callable configuration API for `Wg10PagePool`.
//!
//! The pool core owns page texture state. This module owns the public producer
//! configuration entry points so legacy, single-biome, WORLD, and static
//! reference setup do not stay mixed into the core type definition.

use crate::biome_page_compute;
use crate::gpu_compute::build_pack_buffers;
use crate::pack;
use crate::page_compute::build_page_compute_context;
use godot::classes::RenderingServer;
use godot::prelude::*;
use std::collections::BTreeMap;
use std::path::Path;

use super::{StaticHeightRuntime, Wg10PagePool};

const RUNTIME_BIOMES: [&str; 11] = [
    "coast",
    "desert",
    "glacial",
    "grassland",
    "karst",
    "mountain",
    "rainforest",
    "temperate",
    "tundra",
    "volcanic",
    "wetland",
];

#[godot_api(secondary)]
impl Wg10PagePool {
    /// Load the terrain pack + GLSL source and initialise the policy/slot vectors.
    ///
    /// Returns `""` on success, or an error string on failure (leaves the pool
    /// in a not-ready state).
    ///
    /// `pack_dir`   - OS path to the terrain-pack directory
    /// `pack_file`  - filename within `pack_dir`, e.g. `"terrain_pack.json"`
    /// `glsl_path`  - OS path to `height_page.glsl`
    /// `capacity`   - maximum number of resident page textures
    /// `page_px`    - page resolution in pixels (width == height, multiple of 16)
    /// `world_span` - world-space size of one page in metres
    /// `seed`       - grammar seed
    #[func]
    pub fn configure(
        &mut self,
        pack_dir: GString,
        pack_file: GString,
        glsl_path: GString,
        capacity: i64,
        page_px: i64,
        world_span: f64,
        seed: i64,
    ) -> GString {
        self.free_before_reconfigure();

        let pack =
            match pack::load_pack_dir(Path::new(&pack_dir.to_string()), &pack_file.to_string()) {
                Ok(p) => p,
                Err(e) => return GString::from(&format!("pack: {e}")),
            };

        let pb = build_pack_buffers(&pack);

        let glsl = match std::fs::read_to_string(glsl_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(&format!("glsl: {e}")),
        };

        let mut rd0 = match RenderingServer::singleton().get_rendering_device() {
            Some(r) => r,
            None => {
                return GString::from(
                    "configure: global RenderingDevice unavailable (windowed-only)",
                )
            }
        };
        let ctx = match build_page_compute_context(&mut rd0, &pb, &glsl) {
            Ok(c) => c,
            Err(e) => return GString::from(&format!("compute context: {e}")),
        };

        self.install_legacy_configuration(pack, pb, glsl, ctx, capacity, page_px, world_span, seed);

        GString::new()
    }

    /// Configure the pool to produce pages via the GPU biome path (mountain,
    /// Slice-4 live-fly) instead of the legacy kernel atlas. Builds the biome
    /// compute context on the global rd. Legacy `configure` stays available for
    /// A/B + rollback. Windowed-only, like `configure`.
    ///
    /// Returns `""` on success, or an error string on failure.
    #[func]
    #[allow(clippy::too_many_arguments)]
    pub fn configure_biome(
        &mut self,
        primitives_glsl_path: GString,
        machine_glsl_path: GString,
        mountain_glsl_path: GString,
        capacity: i64,
        page_px: i64,
        apron_px: i64,
        world_span: f64,
        feature_span_m: f64,
        flow_iters: i64,
        relief_m: f64,
        flow_max_level: i64,
        seed: i64,
    ) -> GString {
        self.free_before_reconfigure();

        let prim = match std::fs::read_to_string(primitives_glsl_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(&format!("configure_biome: glsl: {e}")),
        };
        let machine = match std::fs::read_to_string(machine_glsl_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(&format!("configure_biome: glsl: {e}")),
        };
        let mountain = match std::fs::read_to_string(mountain_glsl_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(&format!("configure_biome: glsl: {e}")),
        };

        let mut rd0 = match RenderingServer::singleton().get_rendering_device() {
            Some(r) => r,
            None => {
                return GString::from(
                    "configure_biome: global RenderingDevice unavailable (windowed-only)",
                )
            }
        };

        let ctx = match biome_page_compute::build_biome_page_context(
            &mut rd0,
            &prim,
            &machine,
            &mountain,
            page_px as usize,
            apron_px as usize,
            flow_iters as usize,
            relief_m as f32,
        ) {
            Ok(c) => c,
            Err(e) => return GString::from(&format!("configure_biome: context: {e}")),
        };

        self.install_biome_configuration(
            ctx,
            capacity,
            page_px,
            world_span,
            feature_span_m,
            flow_max_level,
            seed,
        );

        GString::new()
    }

    /// Configure the pool for grammar-routed live biome pages. Page acquisition
    /// samples the grammar into a per-texel weight field, dispatches each active
    /// cached GPU biome context, then folds results through the compose machine.
    #[func]
    #[allow(clippy::too_many_arguments)]
    pub fn configure_biome_world(
        &mut self,
        pack_dir: GString,
        pack_file: GString,
        capacity: i64,
        page_px: i64,
        apron_px: i64,
        world_span: f64,
        feature_span_m: f64,
        flow_iters: i64,
        relief_m: f64,
        flow_max_level: i64,
        seed: i64,
    ) -> GString {
        self.free_before_reconfigure();

        let pack_dir_s = pack_dir.to_string();
        let pack_file_s = pack_file.to_string();
        let pack_dir_path = Path::new(&pack_dir_s);
        let pack_path = pack_dir_path.join(&pack_file_s);
        let pack_json = match std::fs::read_to_string(&pack_path) {
            Ok(s) => s,
            Err(e) => {
                return GString::from(&format!(
                    "configure_biome_world: cannot read pack {pack_path:?}: {e}"
                ))
            }
        };
        let pack = match pack::load_pack_grammar_only(&pack_json) {
            Ok(p) => p,
            Err(e) => return GString::from(&format!("configure_biome_world: pack: {e}")),
        };

        let worldgen_dir = match pack_dir_path.parent().and_then(|p| p.parent()) {
            Some(p) => p.to_path_buf(),
            None => {
                let msg = format!(
                    "configure_biome_world: cannot derive shader dir from pack dir {pack_dir_path:?}"
                );
                return GString::from(&msg);
            }
        };
        let shader_dir = worldgen_dir.join("shaders");
        let prim_path = shader_dir.join("recipe_primitives.glsl");
        let machine_path = shader_dir.join("biome_page.glsl");
        let prim = match std::fs::read_to_string(&prim_path) {
            Ok(s) => s,
            Err(e) => {
                return GString::from(&format!(
                    "configure_biome_world: primitives {prim_path:?}: {e}"
                ))
            }
        };
        let machine = match std::fs::read_to_string(&machine_path) {
            Ok(s) => s,
            Err(e) => {
                return GString::from(&format!(
                    "configure_biome_world: machine {machine_path:?}: {e}"
                ))
            }
        };

        let mut rd0 = match RenderingServer::singleton().get_rendering_device() {
            Some(r) => r,
            None => {
                return GString::from(
                    "configure_biome_world: global RenderingDevice unavailable (windowed-only)",
                )
            }
        };

        let mut contexts: BTreeMap<String, biome_page_compute::BiomePageComputeContext> =
            BTreeMap::new();
        let mut compose_fragment: Option<String> = None;
        for biome in RUNTIME_BIOMES {
            let frag_path = shader_dir.join(format!("biome_{biome}.glsl"));
            let fragment = match std::fs::read_to_string(&frag_path) {
                Ok(s) => s,
                Err(e) => {
                    for (_, ctx) in contexts {
                        biome_page_compute::free_biome_page_context(&mut rd0, &ctx);
                    }
                    return GString::from(&format!(
                        "configure_biome_world: fragment {frag_path:?}: {e}"
                    ));
                }
            };
            if biome == "mountain" {
                compose_fragment = Some(fragment.clone());
            }
            let ctx = match biome_page_compute::build_biome_page_context_for_biome(
                &mut rd0,
                &prim,
                &machine,
                &fragment,
                biome,
                page_px as usize,
                apron_px as usize,
                flow_iters as usize,
                relief_m as f32,
                seed,
                feature_span_m,
            ) {
                Ok(c) => c,
                Err(e) => {
                    for (_, ctx) in contexts {
                        biome_page_compute::free_biome_page_context(&mut rd0, &ctx);
                    }
                    return GString::from(&format!("configure_biome_world: context {biome}: {e}"));
                }
            };
            contexts.insert(biome.to_string(), ctx);
        }
        let compose_fragment = match compose_fragment {
            Some(fragment) => fragment,
            None => {
                for (_, ctx) in contexts {
                    biome_page_compute::free_biome_page_context(&mut rd0, &ctx);
                }
                return GString::from("configure_biome_world: missing mountain compose fragment");
            }
        };
        let compose_ctx = match biome_page_compute::build_biome_compose_context(
            &mut rd0,
            &prim,
            &machine,
            &compose_fragment,
            page_px as usize,
            relief_m as f32,
        ) {
            Ok(c) => c,
            Err(e) => {
                for (_, ctx) in contexts {
                    biome_page_compute::free_biome_page_context(&mut rd0, &ctx);
                }
                return GString::from(&format!("configure_biome_world: compose context: {e}"));
            }
        };

        self.install_biome_world_configuration(
            pack,
            contexts,
            compose_ctx,
            capacity,
            page_px,
            world_span,
            feature_span_m,
            flow_max_level,
            seed,
        );

        GString::new()
    }

    /// Configure the pool to stream a generated static height payload through the
    /// runtime page/clipmap renderer. This is an owner-review reference mode: it
    /// proves the renderer can show the accepted mountain-network world layer,
    /// but it does not replace the live biome recipe/world producer.
    #[func]
    pub fn configure_static_reference(
        &mut self,
        payload_path: GString,
        capacity: i64,
        page_px: i64,
        world_span: f64,
        seed: i64,
    ) -> GString {
        self.free_before_reconfigure();

        let static_ref =
            match StaticHeightRuntime::from_json_path(Path::new(&payload_path.to_string())) {
                Ok(reference) => reference,
                Err(e) => return GString::from(&e),
            };

        self.install_static_reference_configuration(
            static_ref, capacity, page_px, world_span, seed,
        );

        GString::new()
    }
}
