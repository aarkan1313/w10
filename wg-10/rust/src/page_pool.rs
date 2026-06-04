//! Single owner of all page-texture RIDs (DESIGN §5.2).
//!
//! `Wg10PagePool` is the ONLY place that calls `texture_create` and `free_rid`
//! on page textures.  It asks `PagePolicy` what to do (reuse / allocate /
//! evict), then executes by calling `compute_into_texture` from `page_compute`.
//!
//! Anti-WG9 rule enforced here: one place creates, one place frees.
//! `compute_into_texture` frees only ITS own transient buffers/pipeline/shader —
//! never the page texture RID.  All `free_rid` on page textures is pool-internal,
//! at exactly three sites: (a) `free_all` (teardown), (b) Allocate compute-failure
//! cleanup (free the just-created texture), (c) AllocateEvicting compute-failure
//! cleanup (drop the now-stale slot texture).  Single-owner discipline holds: only
//! the pool ever frees page textures.
//!
//! On any producer failure (texture_create or compute_into_texture) the pool calls
//! `PagePolicy::rollback(key)` so policy state matches reality — no phantom-resident
//! key (which could later panic an eviction `.expect`), no stale mapping returning
//! wrong/null content on re-acquire.

use godot::prelude::*;
use godot::classes::{RenderingServer, Texture2Drd};
use crate::pack;
use crate::gpu_compute::{build_pack_buffers, PackBuffers};
use crate::page_policy::PagePolicy;
use crate::page_compute::{PageComputeContext, build_page_compute_context};
use crate::biome_page_compute;
use std::collections::BTreeMap;
use std::path::Path;

mod acquire;
mod configure;
mod lifecycle;
mod static_reference;
mod state_api;
mod world_route;

use static_reference::StaticHeightRuntime;

struct BiomeWorldRuntime {
    pack: pack::Pack,
    contexts: BTreeMap<String, biome_page_compute::BiomePageComputeContext>,
    compose_ctx: biome_page_compute::BiomePageComputeContext,
}

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

// ---------------------------------------------------------------------------
// Wg10PagePool
// ---------------------------------------------------------------------------

/// Single owner of all page-texture RIDs.
///
/// Call order:
///   1. `configure(...)` — load pack + GLSL, set policy capacity.
///   2. `acquire_page(level, origin_x, origin_z)` — get (or compute) a page.
///   3. `release_page(level, origin_x, origin_z)` — unprotect (LRU-eligible).
///   4. `free_all()` — teardown; frees ALL page texture RIDs.
#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10PagePool {
    policy:       Option<PagePolicy>,
    slot_tex:     Vec<Option<Rid>>,
    slot_wrap:    Vec<Option<Gd<Texture2Drd>>>,
    pack:         Option<pack::Pack>,
    pack_buffers: Option<PackBuffers>,
    glsl_source:  Option<String>,
    compute_ctx:  Option<PageComputeContext>,

    // Biome GPU producer path (Slice-4). Flag-gated; legacy kernel path is the DEFAULT
    // (use_biome_path=false) for A/B + rollback. The original `biome_ctx` is the proven single
    // recipe path used by parity/perf gates. `biome_world` is the grammar-routed runtime path:
    // it owns grammar-only pack data and a cached context per compiled recipe.
    use_biome_path:      bool,
    biome_ctx:           Option<biome_page_compute::BiomePageComputeContext>,
    biome_world:         Option<BiomeWorldRuntime>,
    static_ref:          Option<StaticHeightRuntime>,
    biome_feature_span_m: f64,
    /// SCALE-INVARIANCE: the FIRST clipmap level (0 = finest) that bakes WITHOUT the drainage carve.
    /// A page at `level` runs `flow_on = level < biome_flow_max_level`. Default 2 => flow on levels
    /// 0,1 (near camera, where carved valleys read), off 2.. (coarse, where the macro surface
    /// suffices and the two flow passes are too costly). Set by `configure_biome`.
    biome_flow_max_level: i64,

    page_px:      i64,
    world_span:   f64,
    seed:         i64,

    // stats
    created:      i64,
    reused:       i64,
    recomputed:   i64,
    full_events:  i64,

    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10PagePool {
    fn init(base: Base<RefCounted>) -> Self {
        Self {
            policy:       None,
            slot_tex:     Vec::new(),
            slot_wrap:    Vec::new(),
            pack:         None,
            pack_buffers: None,
            glsl_source:  None,
            compute_ctx:  None,
            use_biome_path:       false,
            biome_ctx:            None,
            biome_world:          None,
            static_ref:           None,
            biome_feature_span_m: 90000.0,
            biome_flow_max_level: 2,
            page_px:      256,
            world_span:   1000.0,
            seed:         0,
            created:      0,
            reused:       0,
            recomputed:   0,
            full_events:  0,
            base,
        }
    }
}

#[godot_api]
impl Wg10PagePool {
    // -----------------------------------------------------------------------
    // configure
    // -----------------------------------------------------------------------

    /// Load the terrain pack + GLSL source and initialise the policy/slot vectors.
    ///
    /// Returns `""` on success, or an error string on failure (leaves the pool
    /// in a not-ready state).
    ///
    /// `pack_dir`   — OS path to the terrain-pack directory
    /// `pack_file`  — filename within `pack_dir`, e.g. `"terrain_pack.json"`
    /// `glsl_path`  — OS path to `height_page.glsl`
    /// `capacity`   — maximum number of resident page textures
    /// `page_px`    — page resolution in pixels (width == height, multiple of 16)
    /// `world_span` — world-space size of one page in metres
    /// `seed`       — grammar seed
    #[func]
    pub fn configure(
        &mut self,
        pack_dir:   GString,
        pack_file:  GString,
        glsl_path:  GString,
        capacity:   i64,
        page_px:    i64,
        world_span: f64,
        seed:       i64,
    ) -> GString {
        // --- F8: free-before-reconfigure ---
        // A second configure() would otherwise overwrite slot_tex / slot_wrap /
        // compute_ctx with new GPU resources WITHOUT releasing the old ones, leaking
        // the previous textures' RIDs + compute context on the device. Tear down any
        // existing configuration first (fully resets state per the F7 fix above).
        // Idempotent: a no-op on a fresh, never-configured pool (empty vecs + None
        // Options), so this is safe on the first configure() too.
        self.free_before_reconfigure();

        // --- load pack ---
        let pack = match pack::load_pack_dir(
            Path::new(&pack_dir.to_string()),
            &pack_file.to_string(),
        ) {
            Ok(p)  => p,
            Err(e) => return GString::from(&format!("pack: {e}")),
        };

        // --- build pack buffers ---
        let pb = build_pack_buffers(&pack);

        // --- load GLSL ---
        let glsl = match std::fs::read_to_string(glsl_path.to_string()) {
            Ok(s)  => s,
            Err(e) => return GString::from(&format!("glsl: {e}")),
        };

        // --- build the cached compute context ONCE (slice 7) ---
        // Compile the shader + pipeline + upload the 6 pack buffers (incl. the ~25 MB kernel
        // atlas) here, reused for every page — so per-page production never recompiles/re-uploads
        // (the 90 ms boundary-crossing spike the M3 p99 gate caught). Needs the global RD; the
        // pool is only meaningfully configured windowed (like every pool user).
        let mut rd0 = match RenderingServer::singleton().get_rendering_device() {
            Some(r) => r,
            None    => return GString::from("configure: global RenderingDevice unavailable (windowed-only)"),
        };
        let ctx = match build_page_compute_context(&mut rd0, &pb, &glsl) {
            Ok(c)  => c,
            Err(e) => return GString::from(&format!("compute context: {e}")),
        };

        self.install_legacy_configuration(
            pack, pb, glsl, ctx, capacity, page_px, world_span, seed,
        );

        GString::new()
    }

    // -----------------------------------------------------------------------
    // configure_biome  (Slice-4 GPU biome producer path)
    // -----------------------------------------------------------------------

    /// Configure the pool to produce pages via the GPU biome path (mountain, Slice-4 live-fly)
    /// instead of the legacy kernel atlas. Sets `use_biome_path=true` and builds the biome compute
    /// context on the global rd. Legacy `configure` stays the default path (flag off) for A/B +
    /// rollback. Windowed-only (needs the global RenderingDevice), like `configure`.
    ///
    /// Returns `""` on success, or an error string on failure (leaving the pool not-ready).
    #[func]
    #[allow(clippy::too_many_arguments)]
    pub fn configure_biome(
        &mut self,
        primitives_glsl_path: GString,   // res://.../recipe_primitives.glsl
        machine_glsl_path:    GString,   // res://.../biome_page.glsl
        mountain_glsl_path:   GString,   // res://.../biome_mountain.glsl  (the fragment)
        capacity:   i64,
        page_px:    i64,                 // core px (256) — the apron is added internally
        apron_px:   i64,                 // 160 for mountain
        world_span: f64,                 // world metres per page
        feature_span_m: f64,             // 90000.0 for mountain
        flow_iters: i64,                 // production convergence count (192 per memory)
        relief_m:   f64,                 // VERTICAL SCALE (metres): normalized recipe height * this -> metres
                                         // before the page texture write (the render shader expects metres).
                                         // The tunable vertical-scale knob (~1000 for mountain).
        flow_max_level: i64,             // SCALE-INVARIANCE: first level (0=finest) baked WITHOUT the
                                         // drainage carve. A page at `level` runs flow_on = level <
                                         // flow_max_level. 2 => flow on levels 0,1; off 2.. (coarse).
        seed:       i64,
    ) -> GString {
        // --- F8: free-before-reconfigure (mirror `configure`) ---
        self.free_before_reconfigure();

        // --- read the 3 GLSL sources ---
        let prim = match std::fs::read_to_string(primitives_glsl_path.to_string()) {
            Ok(s)  => s,
            Err(e) => return GString::from(&format!("configure_biome: glsl: {e}")),
        };
        let machine = match std::fs::read_to_string(machine_glsl_path.to_string()) {
            Ok(s)  => s,
            Err(e) => return GString::from(&format!("configure_biome: glsl: {e}")),
        };
        let mountain = match std::fs::read_to_string(mountain_glsl_path.to_string()) {
            Ok(s)  => s,
            Err(e) => return GString::from(&format!("configure_biome: glsl: {e}")),
        };

        // --- global RenderingDevice (windowed-only) ---
        let mut rd0 = match RenderingServer::singleton().get_rendering_device() {
            Some(r) => r,
            None    => return GString::from("configure_biome: global RenderingDevice unavailable (windowed-only)"),
        };

        // --- build the cached biome compute context on the global rd ---
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
            Ok(c)  => c,
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

    // -----------------------------------------------------------------------
    // configure_biome_world  (grammar-routed biome producer path)
    // -----------------------------------------------------------------------

    /// Configure the pool for grammar-routed live biome pages. This is the first runtime world
    /// layer: page acquisition samples the grammar into a per-texel weight field, dispatches each
    /// active cached GPU biome context, then folds the resulting core fields through the GPU
    /// compose machine before writing the page texture.
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
                return GString::from(&format!(
                    "configure_biome_world: cannot derive shader dir from pack dir {pack_dir_path:?}"
                ))
            }
        };
        let shader_dir = worldgen_dir.join("shaders");
        let prim_path = shader_dir.join("recipe_primitives.glsl");
        let machine_path = shader_dir.join("biome_page.glsl");
        let prim = match std::fs::read_to_string(&prim_path) {
            Ok(s) => s,
            Err(e) => return GString::from(&format!(
                "configure_biome_world: primitives {prim_path:?}: {e}"
            )),
        };
        let machine = match std::fs::read_to_string(&machine_path) {
            Ok(s) => s,
            Err(e) => return GString::from(&format!(
                "configure_biome_world: machine {machine_path:?}: {e}"
            )),
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
                    return GString::from(&format!(
                        "configure_biome_world: context {biome}: {e}"
                    ));
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

    // -----------------------------------------------------------------------
    // configure_static_reference  (accepted mountain-network payload bridge)
    // -----------------------------------------------------------------------

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
            static_ref,
            capacity,
            page_px,
            world_span,
            seed,
        );

        GString::new()
    }

    // -----------------------------------------------------------------------
    // free_all  (the ONLY place that frees page texture RIDs)
    // -----------------------------------------------------------------------

    /// Free all page texture RIDs on the global RenderingDevice and clear the
    /// slot vectors.  Safe to call during scene teardown; idempotent.
    ///
    /// As of the B1 fix this is ALSO called automatically from `Drop` (below), so
    /// leak-freedom is structural — a GDScript owner that forgets to call it no
    /// longer leaks. Calling it explicitly is still fine (the second call is a
    /// no-op: the slot vectors are already cleared and all configured Options are
    /// `None`).
    ///
    /// As of the F7 fix this fully resets the pool to the UNCONFIGURED state:
    /// after `free_all()` the `acquire_page`/`get_resident_page` guards correctly
    /// see "not configured" and return None instead of panicking on a stale-but-
    /// half-cleared state. To use the pool again, call `configure()`.
    ///
    /// This is the ONLY site (via `free_all_impl`) that calls `rd.free_rid` on
    /// page textures.
    #[func]
    pub fn free_all(&mut self) {
        self.free_all_impl();
    }
}

// ---------------------------------------------------------------------------
// Drop — structural leak-freedom (B1)
// ---------------------------------------------------------------------------

impl Drop for Wg10PagePool {
    /// Release all page-texture RIDs + the cached compute context when the pool
    /// is dropped, regardless of whether the GDScript owner called `free_all()`.
    ///
    /// A Godot `Rid` is a POD handle — dropping the Rust struct does NOT free the
    /// underlying GPU resource, so without this the RIDs orphan on the device
    /// (the B1 leak). `free_all_impl` guards for "no RenderingDevice" (headless /
    /// already-torn-down), so this is safe at any drop time, and idempotent with
    /// an explicit `free_all()` call.
    fn drop(&mut self) {
        self.free_all_impl();
    }
}

// ---------------------------------------------------------------------------
// Tests - headless state-machine coverage for lifecycle fixes.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod page_pool_tests;
