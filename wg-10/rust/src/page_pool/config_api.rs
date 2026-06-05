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

    /// Configure the pool to produce pages via the RegionFact baked-look path: an async
    /// super-region bake worker (carve + condition, off-frame on its own RenderingDevice) feeds
    /// per-region facts the pool samples for each page. Windowed-only (the worker spawns its own RD,
    /// the pool's acquire still needs the global RD to write page textures).
    ///
    /// `region_span_m` is ONE region cell's world span; it is forced to equal the grammar
    /// `region_size_m` so the sliced region facts tile the world exactly on the grammar region grid
    /// (the pack's region_size_m is overridden to match — a small `region_span_m` makes a fast gate).
    ///
    /// Returns `""` on success, or an error string on failure.
    #[func]
    #[allow(clippy::too_many_arguments)]
    pub fn configure_region_fact(
        &mut self,
        pack_json_path: GString,
        primitives_path: GString,
        machine_path: GString,
        mountain_fragment_path: GString,
        region_n: i64,
        k: i64,
        apron_px: i64,
        seed: i64,
        feature_span_m: f64,
        height_scale_m: f64,
        flow_iters: i64,
        flow_on: bool,
        page_px: i64,
        region_span_m: f64,
    ) -> GString {
        self.free_before_reconfigure();

        if region_n < 2 || k < 1 || apron_px < 0 {
            return GString::from("configure_region_fact: need region_n>=2, k>=1, apron_px>=0");
        }
        if region_span_m <= 0.0 {
            return GString::from("configure_region_fact: region_span_m must be > 0");
        }

        // Split the full pack JSON path into (dir, file) for load_pack_dir.
        let pack_path = pack_json_path.to_string();
        let pack_path_buf = Path::new(&pack_path);
        let pack_dir = match pack_path_buf.parent() {
            Some(d) => d,
            None => return GString::from("configure_region_fact: pack_json_path has no parent dir"),
        };
        let pack_file = match pack_path_buf.file_name().and_then(|f| f.to_str()) {
            Some(f) => f.to_string(),
            None => return GString::from("configure_region_fact: pack_json_path has no filename"),
        };
        let mut pack = match pack::load_pack_dir(pack_dir, &pack_file) {
            Ok(p) => p,
            Err(e) => return GString::from(&format!("pack: {e}")),
        };
        // Tile the world on a region grid whose cell == region_span_m so the worker's sliced facts
        // (origins at gi*region_span_m) line up exactly with `region_of`.
        pack.grammar_constants.region_size_m = region_span_m;

        let prim = match std::fs::read_to_string(primitives_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(&format!("primitives glsl: {e}")),
        };
        let machine = match std::fs::read_to_string(machine_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(&format!("machine glsl: {e}")),
        };
        let fragment = match std::fs::read_to_string(mountain_fragment_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(&format!("mountain fragment glsl: {e}")),
        };

        let region_n = region_n as usize;
        let spacing_m = region_span_m / (region_n - 1) as f64;
        let worker = crate::region_bake::BakeWorker::spawn(prim, machine, fragment);

        let cfg = super::RegionFactConfig {
            region_n,
            k: k as usize,
            apron_px: apron_px as usize,
            flow_iters: flow_iters.max(0) as usize,
            flow_on,
            feature_span_m,
            region_span_m,
            spacing_m,
            height_scale_m,
            seed,
            region_size_m: region_span_m,
            pass: crate::pass_network::PassNetworkParams::default(),
            traverse: crate::pass_network::TraverseParams::default(),
            ramp: crate::pass_network::RampParams::default(),
            coarse_stride_m: region_span_m,
            window_radius_m: region_span_m * 0.5,
            window_samples: 33,
        };

        // world_span: one page's world span. Use region_span_m as a sane default (a page tile);
        // the gate/owner can drive any clipmap level off this base.
        self.install_region_fact_configuration(
            pack,
            worker,
            cfg,
            64, // capacity: enough pages for a small fly + the bake-pending fallbacks
            page_px,
            region_span_m,
            seed,
        );

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

    /// Ladder Rung 0 plumbing producer: write a CLOSED-FORM height
    /// `h = amp * sin(wx/lambda) * cos(wz/lambda)` into each page so a gate can predict every
    /// texel. De-risks the un-intercept flip (produce -> stream -> read -> match an oracle)
    /// independent of biome content. This is a debug/proving producer, not shipped terrain.
    #[func]
    pub fn configure_analytic(
        &mut self,
        capacity: i64,
        page_px: i64,
        world_span: f64,
        amp: f64,
        lambda: f64,
    ) -> GString {
        if !amp.is_finite() || !lambda.is_finite() || lambda == 0.0 {
            return GString::from("configure_analytic: amp/lambda must be finite and lambda != 0");
        }
        if page_px < 2 {
            return GString::from(&format!("configure_analytic: page_px {page_px} must be >= 2"));
        }
        self.free_before_reconfigure();
        self.install_analytic_configuration(
            super::producer::AnalyticParams { amp, lambda },
            capacity,
            page_px,
            world_span,
        );
        GString::new()
    }
}
