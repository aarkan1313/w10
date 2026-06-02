//! WorldGen10 Slice-4a MEASUREMENT spike: real per-page GPU cost at apron dimensions.
//!
//! Answers the spec SS3.1 OPEN question: does a per-page LIVE pipeline (apron grid +
//! flow relaxation + recipe work) fit the frame budget, or must we fall back to a
//! coarse-drainage-fact cache? Extends `flow_spike.rs` from the 256^2 flow-only spike
//! to the TRUE per-page working grid (core_px + 2*apron) with a representative recipe
//! load. MEASUREMENT-ONLY, never wired to the render path. WINDOWED only (local RD is
//! null headless on this box).
//!
//! Honest metric = WALL-clock differential across grid sizes / iteration counts (the
//! flow-spike finding: get_captured_timestamp_gpu_time is unreliable on local RD; the
//! differential cancels fixed per-submit overhead). See `page_measure_check.gd`.

use godot::prelude::*;
use godot::classes::{
    RenderingServer, RdShaderSource, RdUniform, RenderingDevice,
    rendering_device::{UniformType, ShaderStage},
};

/// Working-grid dimension for a page: core + an apron on each side.
pub fn apron_dim(core_px: usize, apron_px: usize) -> usize {
    core_px + 2 * apron_px
}

/// Representative recipe SURFACE for the flow pass to route through: a multi-octave
/// ridged sum (mirrors the structure the real mountain recipe feeds into flow_channels).
/// Row-major f32, length dim*dim. This stands in for the recipe's `base` field so the
/// measured flow cost is on a realistic surface; it is NOT recipe-exact (Task 4a.3 is).
pub fn recipe_load_field(dim: usize, seed: i32) -> Vec<f32> {
    crate::flow_spike::make_ridged_field(dim, seed)
}

fn make_storage_uniform(binding: i32, rid: Rid) -> Gd<RdUniform> {
    let mut u = RdUniform::new_gd();
    u.set_uniform_type(UniformType::STORAGE_BUFFER);
    u.set_binding(binding);
    u.add_id(rid);
    u
}

fn f32s_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for &x in v { b.extend_from_slice(&x.to_le_bytes()); }
    b
}

fn build_push(dim: i32, power: f32) -> Vec<u8> {
    let mut b = Vec::with_capacity(16);
    b.extend_from_slice(&dim.to_le_bytes());
    b.extend_from_slice(&power.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b
}

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10PageMeasure {
    glsl_source: Option<String>,
    last_wall_us: f64,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10PageMeasure {
    fn init(base: Base<RefCounted>) -> Self {
        Self { glsl_source: None, last_wall_us: 0.0, base }
    }
}

#[godot_api]
impl Wg10PageMeasure {
    /// Load the flow GLSL (reuse flow_accum_spike.glsl for the cost measurement).
    #[func]
    pub fn load_shader(&mut self, glsl_path: GString) -> GString {
        match std::fs::read_to_string(glsl_path.to_string()) {
            Ok(s) => { self.glsl_source = Some(s); GString::new() }
            Err(e) => GString::from(format!("glsl: {e}").as_str()),
        }
    }

    /// Run `iters` flow-relaxation steps on a `dim`x`dim` representative recipe surface.
    /// Returns wall-clock MILLISECONDS around submit()+sync() (the honest upper bound on
    /// real GPU work; the check takes a differential across dims/iters). Negative on error.
    /// Unlike `Wg10FlowSpike::run`, this does NO acc readback (no `get_last_acc`): it is a
    /// pure cost measurement, so only the wall-clock time is retained.
    #[func]
    pub fn run(&mut self, dim: i64, iters: i64, power: f64, seed: i64) -> f64 {
        match self.run_inner(dim as usize, iters as usize, power as f32, seed as i32) {
            Ok(wall_us) => { self.last_wall_us = wall_us; wall_us / 1000.0 }
            Err(e) => { godot_error!("Wg10PageMeasure::run error: {e}"); -1.0 }
        }
    }

    #[func]
    pub fn last_wall_us(&self) -> f64 { self.last_wall_us }

    fn run_inner(&self, dim: usize, iters: usize, power: f32, seed: i32) -> Result<f64, String> {
        if iters == 0 { return Err("iters must be >= 1".into()); }
        let glsl = self.glsl_source.as_deref().ok_or("no GLSL source loaded")?;
        let n = dim * dim;

        let mut rd: Gd<RenderingDevice> = RenderingServer::singleton()
            .create_local_rendering_device()
            .ok_or_else(|| "create_local_rendering_device returned null (headless)".to_string())?;

        // --- compile shader ---
        let glsl_stripped: String = glsl
            .lines()
            .filter(|l| !l.trim_start().starts_with("#["))
            .collect::<Vec<_>>()
            .join("\n");
        let mut src = RdShaderSource::new_gd();
        src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
        let spirv = rd
            .shader_compile_spirv_from_source(&src)
            .ok_or_else(|| "shader_compile_spirv_from_source returned null".to_string())?;
        {
            let cerr = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
            if !cerr.is_empty() {
                return Err(format!("GLSL compile error: {cerr}"));
            }
        }
        let shader = rd.shader_create_from_spirv(&spirv);
        if shader.is_invalid() {
            return Err("shader_create_from_spirv invalid".into());
        }

        // --- buffers ---
        let field = recipe_load_field(dim, seed);
        let height_bytes = f32s_to_bytes(&field);
        let ones = vec![1.0_f32; n];
        let ones_bytes = f32s_to_bytes(&ones);

        let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size > u32") };
        let height_pba = PackedByteArray::from(height_bytes.as_slice());
        let acc_init_pba = PackedByteArray::from(ones_bytes.as_slice());

        let height_rid = rd
            .storage_buffer_create_ex(bsize(height_bytes.len()))
            .data(&height_pba)
            .done();
        let acc_a_rid = rd
            .storage_buffer_create_ex(bsize(ones_bytes.len()))
            .data(&acc_init_pba)
            .done();
        let acc_b_rid = rd
            .storage_buffer_create_ex(bsize(ones_bytes.len()))
            .data(&acc_init_pba)
            .done();

        // Two uniform sets: set_ab has prev=A(bind1),next=B(bind2); set_ba has prev=B,next=A.
        // height is always bind0.
        let build_set = |rd: &mut Gd<RenderingDevice>, prev: Rid, next: Rid| -> Rid {
            let mut uniforms: Array<Gd<RdUniform>> = Array::new();
            uniforms.push(&make_storage_uniform(0, height_rid));
            uniforms.push(&make_storage_uniform(1, prev));
            uniforms.push(&make_storage_uniform(2, next));
            rd.uniform_set_create(&uniforms, shader, 0)
        };
        let set_ab = build_set(&mut rd, acc_a_rid, acc_b_rid);
        let set_ba = build_set(&mut rd, acc_b_rid, acc_a_rid);

        let push_pba = PackedByteArray::from(build_push(dim as i32, power).as_slice());
        let pipeline = rd.compute_pipeline_create(shader);
        let wg = ((dim as u32) + 15) / 16;

        // --- dispatch loop ---
        let cl = rd.compute_list_begin();
        rd.compute_list_bind_compute_pipeline(cl, pipeline);
        for i in 0..iters {
            let set = if i % 2 == 0 { set_ab } else { set_ba };
            rd.compute_list_bind_uniform_set(cl, set, 0);
            rd.compute_list_set_push_constant(cl, &push_pba, push_pba.len() as u32);
            rd.compute_list_dispatch(cl, wg, wg, 1);
            rd.compute_list_add_barrier(cl);
        }
        rd.compute_list_end();

        let wall0 = std::time::Instant::now();
        rd.submit();
        rd.sync();
        let wall_us = wall0.elapsed().as_secs_f64() * 1.0e6;

        // --- free ---
        rd.free_rid(height_rid);
        rd.free_rid(acc_a_rid);
        rd.free_rid(acc_b_rid);
        rd.free_rid(pipeline);
        // Freeing the shader cascades its uniform sets (set_ab, set_ba).
        rd.free_rid(shader);
        rd.free();

        Ok(wall_us)
    }
}
