//! Godot API entrypoints for biome page readback and runtime parity gates.

use godot::classes::{
    rendering_device::{DataFormat, TextureUsageBits},
    RdTextureFormat, RdTextureView, RenderingDevice, RenderingServer,
};
use godot::prelude::*;

use super::abi::STABLE_ITERS;
use super::{
    biome_stem, build_biome_page_context, bytes_to_f32s, compute_biome_page_cached,
    f32s_to_packed_f64, free_biome_page_context, Wg10BiomePageCompute,
};

#[godot_api(secondary)]
impl Wg10BiomePageCompute {
    /// Run the full biome pass chain for one page and return the core normalized height field.
    #[allow(clippy::too_many_arguments)]
    #[func]
    pub fn generate_core_page(
        &self,
        spacing: f64,
        ox: f64,
        oz: f64,
        padded_rows: i64,
        padded_cols: i64,
        apron_px: i64,
        seed: i64,
        feature_span_m: f64,
        biome_fragment_path: GString,
    ) -> PackedFloat64Array {
        let rows = padded_rows as usize;
        let cols = padded_cols as usize;
        let apron = apron_px as usize;
        if seed < i32::MIN as i64 || seed > i32::MAX as i64 {
            godot_error!("Wg10BiomePageCompute::generate_core_page: seed {seed} outside i32 range (GPU hash is 32-bit-seed); CPU oracle is i64 -> parity impossible. Use a seed in i32 range.");
            return PackedFloat64Array::new();
        }

        let frag_path = biome_fragment_path.to_string();
        let fragment = match std::fs::read_to_string(&frag_path) {
            Ok(s) => s,
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_core_page: biome fragment glsl: {e}");
                return PackedFloat64Array::new();
            }
        };
        let biome = biome_stem(&frag_path);
        match self.run_inner(
            spacing as f32,
            ox as f32,
            oz as f32,
            rows,
            cols,
            apron,
            seed as i32,
            feature_span_m as f32,
            &fragment,
            &biome,
            STABLE_ITERS,
        ) {
            Ok(core) => f32s_to_packed_f64(&core),
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_core_page error: {e}");
                PackedFloat64Array::new()
            }
        }
    }

    /// `generate_core_page` with caller-controlled flow relaxation count for windowed sweeps.
    #[allow(clippy::too_many_arguments)]
    #[func]
    pub fn generate_core_page_iters(
        &self,
        spacing: f64,
        ox: f64,
        oz: f64,
        padded_rows: i64,
        padded_cols: i64,
        apron_px: i64,
        seed: i64,
        feature_span_m: f64,
        biome_fragment_path: GString,
        flow_iters: i64,
    ) -> PackedFloat64Array {
        let rows = padded_rows as usize;
        let cols = padded_cols as usize;
        let apron = apron_px as usize;
        if seed < i32::MIN as i64 || seed > i32::MAX as i64 {
            godot_error!(
                "Wg10BiomePageCompute::generate_core_page_iters: seed {seed} outside i32 range"
            );
            return PackedFloat64Array::new();
        }
        if flow_iters < 1 {
            godot_error!("Wg10BiomePageCompute::generate_core_page_iters: flow_iters must be >= 1");
            return PackedFloat64Array::new();
        }

        let frag_path = biome_fragment_path.to_string();
        let fragment = match std::fs::read_to_string(&frag_path) {
            Ok(s) => s,
            Err(e) => {
                godot_error!(
                    "Wg10BiomePageCompute::generate_core_page_iters: biome fragment glsl: {e}"
                );
                return PackedFloat64Array::new();
            }
        };
        let biome = biome_stem(&frag_path);
        match self.run_inner(
            spacing as f32,
            ox as f32,
            oz as f32,
            rows,
            cols,
            apron,
            seed as i32,
            feature_span_m as f32,
            &fragment,
            &biome,
            flow_iters as usize,
        ) {
            Ok(core) => f32s_to_packed_f64(&core),
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_core_page_iters error: {e}");
                PackedFloat64Array::new()
            }
        }
    }

    /// Runtime-producer readback entry for the windowed 576 parity gate.
    #[allow(clippy::too_many_arguments)]
    #[func]
    pub fn generate_runtime_page_576(
        &self,
        spacing: f64,
        ox: f64,
        oz: f64,
        padded_rows: i64,
        padded_cols: i64,
        apron_px: i64,
        seed: i64,
        feature_span_m: f64,
        mountain_fragment_path: GString,
        flow_iters: i64,
    ) -> PackedFloat64Array {
        if padded_rows != padded_cols {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: padded grid must be square (got {padded_rows}x{padded_cols})");
            return PackedFloat64Array::new();
        }
        let apron = apron_px as usize;
        let padded = padded_rows as usize;
        if padded <= 2 * apron {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: apron {apron} too large for padded {padded}");
            return PackedFloat64Array::new();
        }
        let core_px = padded - 2 * apron;
        if seed < i32::MIN as i64 || seed > i32::MAX as i64 {
            godot_error!(
                "Wg10BiomePageCompute::generate_runtime_page_576: seed {seed} outside i32 range"
            );
            return PackedFloat64Array::new();
        }
        if flow_iters < 1 {
            godot_error!(
                "Wg10BiomePageCompute::generate_runtime_page_576: flow_iters must be >= 1"
            );
            return PackedFloat64Array::new();
        }

        let prim = match self.primitives_src.as_deref() {
            Some(s) => s,
            None => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: no GLSL source loaded (call load_shaders)");
                return PackedFloat64Array::new();
            }
        };
        let machine = match self.machine_src.as_deref() {
            Some(s) => s,
            None => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: no GLSL source loaded (call load_shaders)");
                return PackedFloat64Array::new();
            }
        };
        let frag_path = mountain_fragment_path.to_string();
        let fragment = match std::fs::read_to_string(&frag_path) {
            Ok(s) => s,
            Err(e) => {
                godot_error!(
                    "Wg10BiomePageCompute::generate_runtime_page_576: mountain fragment glsl: {e}"
                );
                return PackedFloat64Array::new();
            }
        };

        let mut rd: Gd<RenderingDevice> = match RenderingServer::singleton()
            .create_local_rendering_device()
        {
            Some(d) => d,
            None => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: create_local_rendering_device returned null (headless / no device)");
                return PackedFloat64Array::new();
            }
        };

        let ctx = match build_biome_page_context(
            &mut rd,
            prim,
            machine,
            &fragment,
            core_px,
            apron,
            flow_iters as usize,
            1.0,
        ) {
            Ok(c) => c,
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: {e}");
                rd.free();
                return PackedFloat64Array::new();
            }
        };

        let mut fmt = RdTextureFormat::new_gd();
        fmt.set_width(core_px as u32);
        fmt.set_height(core_px as u32);
        fmt.set_format(DataFormat::R32_SFLOAT);
        fmt.set_usage_bits(
            TextureUsageBits::STORAGE_BIT
                | TextureUsageBits::SAMPLING_BIT
                | TextureUsageBits::CAN_COPY_FROM_BIT,
        );
        let view = RdTextureView::new_gd();
        let tex = rd.texture_create(&fmt, &view);
        if tex.is_invalid() {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: texture_create returned invalid RID");
            free_biome_page_context(&mut rd, &ctx);
            rd.free();
            return PackedFloat64Array::new();
        }

        let page_px = core_px as i64;
        let world_span = spacing * (page_px as f64 - 1.0);
        let origin_x = ox + apron as f64 * spacing;
        let origin_z = oz + apron as f64 * spacing;

        if let Err(e) = compute_biome_page_cached(
            &mut rd,
            &ctx,
            tex,
            origin_x,
            origin_z,
            world_span,
            page_px,
            feature_span_m,
            seed,
            true,
        ) {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: {e}");
            rd.free_rid(tex);
            free_biome_page_context(&mut rd, &ctx);
            rd.free();
            return PackedFloat64Array::new();
        }

        let raw = rd.texture_get_data(tex, 0);
        let core = bytes_to_f32s(&raw.to_vec());

        rd.free_rid(tex);
        free_biome_page_context(&mut rd, &ctx);
        rd.free();

        let core_n = core_px * core_px;
        if core.len() != core_n {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: texture readback expected {core_n} f32, got {}", core.len());
            return PackedFloat64Array::new();
        }
        f32s_to_packed_f64(&core)
    }

    /// Runtime-producer readback entry with explicit flow gating.
    ///
    /// This mirrors `generate_runtime_page_576`, but lets windowed gates exercise the
    /// scale-invariant coarse-page path (`flow_on=false`) without changing the existing 576
    /// parity call shape.
    #[allow(clippy::too_many_arguments)]
    #[func]
    pub fn generate_runtime_page_flow(
        &self,
        spacing: f64,
        ox: f64,
        oz: f64,
        padded_rows: i64,
        padded_cols: i64,
        apron_px: i64,
        seed: i64,
        feature_span_m: f64,
        mountain_fragment_path: GString,
        flow_iters: i64,
        flow_on: bool,
    ) -> PackedFloat64Array {
        if padded_rows != padded_cols {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_flow: padded grid must be square (got {padded_rows}x{padded_cols})");
            return PackedFloat64Array::new();
        }
        let apron = apron_px as usize;
        let padded = padded_rows as usize;
        if padded <= 2 * apron {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_flow: apron {apron} too large for padded {padded}");
            return PackedFloat64Array::new();
        }
        let core_px = padded - 2 * apron;
        if seed < i32::MIN as i64 || seed > i32::MAX as i64 {
            godot_error!(
                "Wg10BiomePageCompute::generate_runtime_page_flow: seed {seed} outside i32 range"
            );
            return PackedFloat64Array::new();
        }
        if flow_iters < 1 {
            godot_error!(
                "Wg10BiomePageCompute::generate_runtime_page_flow: flow_iters must be >= 1"
            );
            return PackedFloat64Array::new();
        }

        let prim = match self.primitives_src.as_deref() {
            Some(s) => s,
            None => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_flow: no GLSL source loaded (call load_shaders)");
                return PackedFloat64Array::new();
            }
        };
        let machine = match self.machine_src.as_deref() {
            Some(s) => s,
            None => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_flow: no GLSL source loaded (call load_shaders)");
                return PackedFloat64Array::new();
            }
        };
        let frag_path = mountain_fragment_path.to_string();
        let fragment = match std::fs::read_to_string(&frag_path) {
            Ok(s) => s,
            Err(e) => {
                godot_error!(
                    "Wg10BiomePageCompute::generate_runtime_page_flow: mountain fragment glsl: {e}"
                );
                return PackedFloat64Array::new();
            }
        };

        let mut rd: Gd<RenderingDevice> = match RenderingServer::singleton()
            .create_local_rendering_device()
        {
            Some(d) => d,
            None => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_flow: create_local_rendering_device returned null (headless / no device)");
                return PackedFloat64Array::new();
            }
        };

        let ctx = match build_biome_page_context(
            &mut rd,
            prim,
            machine,
            &fragment,
            core_px,
            apron,
            flow_iters as usize,
            1.0,
        ) {
            Ok(c) => c,
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_flow: {e}");
                rd.free();
                return PackedFloat64Array::new();
            }
        };

        let mut fmt = RdTextureFormat::new_gd();
        fmt.set_width(core_px as u32);
        fmt.set_height(core_px as u32);
        fmt.set_format(DataFormat::R32_SFLOAT);
        fmt.set_usage_bits(
            TextureUsageBits::STORAGE_BIT
                | TextureUsageBits::SAMPLING_BIT
                | TextureUsageBits::CAN_COPY_FROM_BIT,
        );
        let view = RdTextureView::new_gd();
        let tex = rd.texture_create(&fmt, &view);
        if tex.is_invalid() {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_flow: texture_create returned invalid RID");
            free_biome_page_context(&mut rd, &ctx);
            rd.free();
            return PackedFloat64Array::new();
        }

        let page_px = core_px as i64;
        let world_span = spacing * (page_px as f64 - 1.0);
        let origin_x = ox + apron as f64 * spacing;
        let origin_z = oz + apron as f64 * spacing;

        if let Err(e) = compute_biome_page_cached(
            &mut rd,
            &ctx,
            tex,
            origin_x,
            origin_z,
            world_span,
            page_px,
            feature_span_m,
            seed,
            flow_on,
        ) {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_flow: {e}");
            rd.free_rid(tex);
            free_biome_page_context(&mut rd, &ctx);
            rd.free();
            return PackedFloat64Array::new();
        }

        let raw = rd.texture_get_data(tex, 0);
        let core = bytes_to_f32s(&raw.to_vec());

        rd.free_rid(tex);
        free_biome_page_context(&mut rd, &ctx);
        rd.free();

        let core_n = core_px * core_px;
        if core.len() != core_n {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_flow: texture readback expected {core_n} f32, got {}", core.len());
            return PackedFloat64Array::new();
        }
        f32s_to_packed_f64(&core)
    }
}
