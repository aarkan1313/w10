//! WorldGen10 Task 4a.3: GLSL noise/warp primitive PROBE on a LOCAL RenderingDevice.
//!
//! De-risks mirroring the offline f64 noise primitives (`worldgen_proto.py`) to GLSL f32,
//! where the lattice hash `_hash2` must be emulated with two u32 words because GLSL base
//! profile (#version 450, Godot compute) has NO 64-bit integers. This class compiles
//! `recipe_primitives.glsl` (the i64-emulated hash + f32 primitives) concatenated with
//! `primitive_probe.glsl` (the 1x1x1 `main`), dispatches one invocation that evaluates one
//! selected primitive at one coord, and reads back one float.
//!
//! It is NOT wired into the render/streaming path. A windowed Godot check
//! (`primitive_parity_check.gd`) drives it and compares every sample to the f64 oracle
//! fixture within an f32 epsilon. GPU compute (RenderingDevice) is null headless on this
//! D3D12 box, so the check skips (rc 2) when `create_local_rendering_device()` is null.
//!
//! API usage MIRRORS `flow_spike.rs` exactly (same godot-crate version): strip `#[...]`
//! header lines, RdShaderSource COMPUTE stage, compile_spirv_from_source, create_from_spirv,
//! storage_buffer_create_ex, uniform_set_create, compute_pipeline, dispatch, buffer_get_data,
//! then free_rid each RID, free the shader (cascades the uniform set), and rd.free().

use godot::prelude::*;
use godot::classes::{
    RenderingServer, RdShaderSource, RdUniform, RenderingDevice,
    rendering_device::{UniformType, ShaderStage},
};

// fn_sel codes -- MUST match primitive_probe.glsl + export_primitive_parity_fixture.py.
fn fn_sel(name: &str) -> Option<i32> {
    match name {
        "hash2" => Some(0),
        "value_noise" => Some(1),
        "fbm" => Some(2),
        "ridged_multifractal" => Some(3),
        "warp_x" => Some(4),
        "warp_z" => Some(5),
        _ => None,
    }
}

/// Build the push constant: 4 i32 (fn_sel + 3 pad) then 8 f32 (a0..a4 + 3 pad) = 48 bytes
/// (multiple of 16, std430-friendly). args fills a0.. in order; missing slots are 0.
fn build_push(sel: i32, args: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(48);
    b.extend_from_slice(&sel.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    for i in 0..8usize {
        let v = args.get(i).copied().unwrap_or(0.0_f32);
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

fn make_storage_uniform(binding: i32, rid: Rid) -> Gd<RdUniform> {
    let mut u = RdUniform::new_gd();
    u.set_uniform_type(UniformType::STORAGE_BUFFER);
    u.set_binding(binding);
    u.add_id(rid);
    u
}

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10PrimitiveProbe {
    primitives_src: Option<String>,
    probe_src: Option<String>,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10PrimitiveProbe {
    fn init(base: Base<RefCounted>) -> Self {
        Self { primitives_src: None, probe_src: None, base }
    }
}

#[godot_api]
impl Wg10PrimitiveProbe {
    /// Load BOTH GLSL files (the helpers/primitives and the probe main) from OS paths and
    /// concatenate them (helpers first). Returns "" on success, an error string otherwise.
    /// Godot GLSL has no #include, so the Rust side joins the two files before compiling.
    #[func]
    pub fn load_shader(&mut self, primitives_path: GString, probe_path: GString) -> GString {
        let prim = match std::fs::read_to_string(primitives_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(format!("primitives glsl: {e}").as_str()),
        };
        let probe = match std::fs::read_to_string(probe_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(format!("probe glsl: {e}").as_str()),
        };
        self.primitives_src = Some(prim);
        self.probe_src = Some(probe);
        GString::new()
    }

    /// Evaluate one primitive at the given args on the GPU and return the single f32 result
    /// (widened to f64 for GDScript). Returns NaN on error (see godot_error log). The probe
    /// concatenates the probe `main` AFTER the primitives helpers, strips `#[...]` lines, then
    /// compiles + dispatches 1x1x1 + reads back one float. A fresh local RenderingDevice is
    /// created and freed per call (matching flow_spike: avoids exhausting device slots).
    #[func]
    pub fn eval(&self, fn_name: GString, args: PackedFloat64Array) -> f64 {
        let name = fn_name.to_string();
        let sel = match fn_sel(&name) {
            Some(s) => s,
            None => {
                godot_error!("Wg10PrimitiveProbe::eval unknown fn '{name}'");
                return f64::NAN;
            }
        };
        let args_f32: Vec<f32> = args.as_slice().iter().map(|&v| v as f32).collect();
        match self.eval_inner(sel, &args_f32) {
            Ok(v) => v as f64,
            Err(e) => {
                godot_error!("Wg10PrimitiveProbe::eval('{name}') error: {e}");
                f64::NAN
            }
        }
    }

    // ---- internal GPU dispatch ----
    fn eval_inner(&self, sel: i32, args: &[f32]) -> Result<f32, String> {
        let prim = self.primitives_src.as_deref().ok_or("no GLSL source loaded")?;
        let probe = self.probe_src.as_deref().ok_or("no GLSL source loaded")?;

        let mut rd: Gd<RenderingDevice> = RenderingServer::singleton()
            .create_local_rendering_device()
            .ok_or_else(|| "create_local_rendering_device returned null (headless / no device)".to_string())?;

        // Concatenate helpers + probe main, strip non-GLSL `#[...]` header lines (same as
        // flow_spike). The probe's own `#version`/`layout`/`main` survive.
        let joined = format!("{prim}\n{probe}");
        let glsl_stripped: String = joined
            .lines()
            .filter(|l| !l.trim_start().starts_with("#["))
            .collect::<Vec<_>>()
            .join("\n");

        let mut src = RdShaderSource::new_gd();
        src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
        // NB: every error path AFTER rd is created must `rd.free()` first — Gd<RenderingDevice>
        // is a manually-managed Godot Object (dropping the Rust wrapper does NOT free the device),
        // and leaking local RDs exhausts device slots in a red-shader debug loop (memory
        // worldgen10-gpu-compute-env). The success path frees at the end.
        let spirv = match rd.shader_compile_spirv_from_source(&src) {
            Some(s) => s,
            None => {
                rd.free();
                return Err("shader_compile_spirv_from_source returned null".to_string());
            }
        };
        {
            let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
            if !err.is_empty() {
                rd.free();
                return Err(format!("GLSL compile error: {err}"));
            }
        }
        let shader = rd.shader_create_from_spirv(&spirv);
        if shader.is_invalid() {
            rd.free();
            return Err("shader_create_from_spirv returned invalid RID".into());
        }

        // Single output buffer: one f32 (init 0.0).
        let init = 0.0_f32.to_le_bytes();
        let init_pba = PackedByteArray::from(init.as_slice());
        let out_rid = rd
            .storage_buffer_create_ex(4)
            .data(&init_pba)
            .done();

        let mut uniforms: Array<Gd<RdUniform>> = Array::new();
        uniforms.push(&make_storage_uniform(0, out_rid));
        let uset = rd.uniform_set_create(&uniforms, shader, 0);

        let push = build_push(sel, args);
        let push_pba = PackedByteArray::from(push.as_slice());
        let pipeline = rd.compute_pipeline_create(shader);

        let cl = rd.compute_list_begin();
        rd.compute_list_bind_compute_pipeline(cl, pipeline);
        rd.compute_list_bind_uniform_set(cl, uset, 0);
        rd.compute_list_set_push_constant(cl, &push_pba, push_pba.len() as u32);
        rd.compute_list_dispatch(cl, 1, 1, 1);
        rd.compute_list_end();

        rd.submit();
        rd.sync();

        let out_pba = rd.buffer_get_data(out_rid);
        let bytes = out_pba.to_vec();
        if bytes.len() < 4 {
            // free before bailing
            rd.free_rid(out_rid);
            rd.free_rid(pipeline);
            rd.free_rid(shader);
            rd.free();
            return Err(format!("readback: expected 4 bytes, got {}", bytes.len()));
        }
        let value = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);

        rd.free_rid(out_rid);
        rd.free_rid(pipeline);
        // Freeing the shader cascades its uniform set.
        rd.free_rid(shader);
        rd.free();

        Ok(value)
    }
}

// ---------------------------------------------------------------------------
// Unit tests (pure helpers; no Godot runtime)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fn_sel_maps_known_names() {
        assert_eq!(fn_sel("hash2"), Some(0));
        assert_eq!(fn_sel("value_noise"), Some(1));
        assert_eq!(fn_sel("fbm"), Some(2));
        assert_eq!(fn_sel("ridged_multifractal"), Some(3));
        assert_eq!(fn_sel("warp_x"), Some(4));
        assert_eq!(fn_sel("warp_z"), Some(5));
        assert_eq!(fn_sel("nope"), None);
    }

    #[test]
    fn push_constant_is_48_bytes() {
        assert_eq!(build_push(0, &[1.0, 2.0, 3.0]).len(), 48);
        assert_eq!(build_push(5, &[]).len(), 48);
    }

    #[test]
    fn push_constant_packs_sel_then_args() {
        let p = build_push(3, &[1.5, -2.0]);
        // first 4 bytes = sel = 3
        assert_eq!(i32::from_le_bytes([p[0], p[1], p[2], p[3]]), 3);
        // floats start at byte 16 (after 4 i32)
        let a0 = f32::from_le_bytes([p[16], p[17], p[18], p[19]]);
        let a1 = f32::from_le_bytes([p[20], p[21], p[22], p[23]]);
        assert_eq!(a0, 1.5);
        assert_eq!(a1, -2.0);
    }
}
