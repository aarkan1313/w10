//! WorldGen10 Slice-3 #1 RISK SPIKE: GPU flow-accumulation by iterative relaxation.
//!
//! MEASUREMENT-ONLY. Answers: can the MFD drainage operator
//! (`array_ops::flow_accumulation_mfd` / `geography_skeleton._flow_accumulation_mfd`),
//! a sequential sorted high->low sweep on CPU, run live on the GPU within the per-page
//! frame budget at PAGE_PX=256? The CPU sweep cannot be one GPU dispatch, so we use the
//! standard PULL relaxation (see `shaders/flow_accum_spike.glsl`): one dispatch per
//! relaxation step, ping-ponging two acc buffers, K steps.
//!
//! This class is NOT wired into the render/streaming path. It exists so a windowed Godot
//! check can drive it on real hardware (RTX 5090 / D3D12) and report REAL GPU time via
//! RenderingDevice timestamps (vsync-immune, no host stall inside the dispatch loop) plus
//! a convergence delta. All GPU work for a run happens in a single submit/sync.

use godot::prelude::*;
use godot::classes::{
    RenderingServer, RdShaderSource, RdUniform, RenderingDevice,
    rendering_device::{UniformType, ShaderStage},
};

// ---------------------------------------------------------------------------
// Pure helpers (testable without a Godot runtime)
// ---------------------------------------------------------------------------

/// Deterministic ridged-ish test height field, row-major f32, length dim*dim.
/// Cheap multi-octave value-noise sum turned crest-biased with `1 - |n|`, plus a broad
/// tilt so there are long monotone descending paths (the thing that bounds relaxation
/// iteration count). NOT the real keeper height; just a representative rough surface so
/// the flow operator has real structure to route through.
pub fn make_ridged_field(dim: usize, seed: i32) -> Vec<f32> {
    fn hash2(ix: i32, iz: i32, seed: i32) -> f32 {
        let mut h = (ix as u32)
            .wrapping_mul(374_761_393)
            .wrapping_add((iz as u32).wrapping_mul(668_265_263))
            .wrapping_add((seed as u32).wrapping_mul(362_437));
        h ^= h >> 13;
        h = h.wrapping_mul(1_274_126_177);
        h ^= h >> 16;
        (h & 0x7fff_ffff) as f32 / 0x7fff_ffff as f32
    }
    fn fade(t: f32) -> f32 { t * t * t * (t * (t * 6.0 - 15.0) + 10.0) }
    fn value_noise(x: f32, z: f32, seed: i32) -> f32 {
        let fx = x.floor();
        let fz = z.floor();
        let (x0, z0) = (fx as i32, fz as i32);
        let (tx, tz) = (fade(x - fx), fade(z - fz));
        let c00 = hash2(x0, z0, seed);
        let c10 = hash2(x0 + 1, z0, seed);
        let c01 = hash2(x0, z0 + 1, seed);
        let c11 = hash2(x0 + 1, z0 + 1, seed);
        let top = c00 + (c10 - c00) * tx;
        let bot = c01 + (c11 - c01) * tx;
        ((top + (bot - top) * tz) * 2.0 - 1.0).clamp(-1.0, 1.0)
    }

    let n = dim * dim;
    let mut field = vec![0.0_f32; n];
    let inv = 1.0 / dim as f32;
    for y in 0..dim {
        for x in 0..dim {
            // Normalized [0,1) page coords scaled to a few noise periods.
            let u = x as f32 * inv;
            let v = y as f32 * inv;
            // Crest-biased ridged sum (3 octaves) for ridgelines + valleys.
            let mut h = 0.0_f32;
            let mut amp = 1.0_f32;
            let mut norm = 0.0_f32;
            // Start at a low base frequency (~2 periods/page) so basins are large and flow
            // paths are LONG and meandering -- this stresses the relaxation iteration count
            // (a strongly-tilted field would converge in far fewer iters and understate cost).
            let mut freq = 2.0_f32;
            for o in 0..4 {
                let nse = value_noise(u * freq, v * freq, seed + o);
                h += amp * (1.0 - nse.abs());
                norm += amp;
                amp *= 0.6;
                freq *= 2.0;
            }
            h /= norm.max(1e-9);
            // Weak diagonal tilt: just enough to break flat ties and give an overall drainage
            // direction, but small so the noise basins (not the tilt) set the flow-path length.
            let tilt = 0.12 * (u + v);
            field[y * dim + x] = h + tilt;
        }
    }
    field
}

/// Max absolute difference between two equal-length f32 vectors (NaN-safe-enough for
/// the finite fields here). Used as the convergence metric between iteration counts.
pub fn max_abs_delta(a: &[f32], b: &[f32]) -> f64 {
    debug_assert_eq!(a.len(), b.len());
    let mut m = 0.0_f64;
    for i in 0..a.len() {
        let d = (a[i] as f64 - b[i] as f64).abs();
        if d > m { m = d; }
    }
    m
}

/// Build the 16-byte push constant: i32 dim, f32 power, i32 pad, i32 pad.
fn build_push(dim: i32, power: f32) -> Vec<u8> {
    let mut b = Vec::with_capacity(16);
    b.extend_from_slice(&dim.to_le_bytes());
    b.extend_from_slice(&power.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b
}

fn f32s_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for &x in v { b.extend_from_slice(&x.to_le_bytes()); }
    b
}

fn bytes_to_f32s(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn make_storage_uniform(binding: i32, rid: Rid) -> Gd<RdUniform> {
    let mut u = RdUniform::new_gd();
    u.set_uniform_type(UniformType::STORAGE_BUFFER);
    u.set_binding(binding);
    u.add_id(rid);
    u
}

// ---------------------------------------------------------------------------
// Godot class
// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10FlowSpike {
    glsl_source: Option<String>,
    last_gpu_us: f64,     // measured GPU time for the last run (microseconds)
    last_cpu_us: f64,     // measured CPU timestamp delta for the last run (microseconds)
    last_wall_us: f64,    // wall-clock around submit+sync for the last run (microseconds)
    last_acc: Vec<f32>,   // final acc buffer from the last run (for convergence checks)
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10FlowSpike {
    fn init(base: Base<RefCounted>) -> Self {
        Self { glsl_source: None, last_gpu_us: 0.0, last_cpu_us: 0.0, last_wall_us: 0.0, last_acc: Vec::new(), base }
    }
}

#[godot_api]
impl Wg10FlowSpike {
    /// Load the flow-accum spike GLSL from an OS path. "" on success, error string otherwise.
    #[func]
    pub fn load_shader(&mut self, glsl_path: GString) -> GString {
        match std::fs::read_to_string(glsl_path.to_string()) {
            Ok(s) => { self.glsl_source = Some(s); GString::new() }
            Err(e) => GString::from(format!("glsl: {e}").as_str()),
        }
    }

    /// Run K relaxation iterations of the GPU flow accumulation on a dim x dim ridged
    /// field. Returns the MEASURED GPU time in MILLISECONDS for the whole iteration loop
    /// (one submit/sync). Returns a negative value on error (see godot_error log).
    ///
    /// The final acc buffer is cached; call `get_last_acc()` to fetch it for convergence
    /// analysis between iteration counts. GPU time is captured with RenderingDevice
    /// timestamps placed before/after the dispatch loop inside the same compute list, so
    /// the number is real device time and not polluted by host stalls or vsync.
    #[func]
    pub fn run(&mut self, dim: i64, iters: i64, power: f64, seed: i64) -> f64 {
        match self.run_inner(dim as usize, iters as usize, power as f32, seed as i32) {
            Ok(r) => {
                self.last_gpu_us = r.gpu_us;
                self.last_cpu_us = r.cpu_us;
                self.last_wall_us = r.wall_us;
                self.last_acc = r.acc;
                r.gpu_us / 1000.0 // microseconds -> milliseconds
            }
            Err(e) => {
                godot_error!("Wg10FlowSpike::run error: {e}");
                -1.0
            }
        }
    }

    /// Raw measured GPU time in microseconds for the last run.
    #[func]
    pub fn last_gpu_us(&self) -> f64 { self.last_gpu_us }

    /// CPU timestamp delta (microseconds) across the same dispatch loop, for cross-checking.
    #[func]
    pub fn last_cpu_us(&self) -> f64 { self.last_cpu_us }

    /// Wall-clock microseconds around submit()+sync(), for cross-checking the GPU timestamp.
    #[func]
    pub fn last_wall_us(&self) -> f64 { self.last_wall_us }

    /// Final acc buffer from the last run (one f32 per cell, row-major), as f64 for GDScript.
    #[func]
    pub fn get_last_acc(&self) -> PackedFloat64Array {
        let mut out = PackedFloat64Array::new();
        out.resize(self.last_acc.len());
        let sl = out.as_mut_slice();
        for i in 0..self.last_acc.len() { sl[i] = self.last_acc[i] as f64; }
        out
    }

    // ---- internal GPU dispatch ----
    fn run_inner(
        &self,
        dim: usize,
        iters: usize,
        power: f32,
        seed: i32,
    ) -> Result<RunResult, String> {
        if iters == 0 { return Err("iters must be >= 1".into()); }
        let glsl = self.glsl_source.as_deref().ok_or("no GLSL source loaded")?;
        let n = dim * dim;

        let mut rd: Gd<RenderingDevice> = RenderingServer::singleton()
            .create_local_rendering_device()
            .ok_or_else(|| "create_local_rendering_device returned null (headless / no device)".to_string())?;

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
            let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
            if !err.is_empty() {
                return Err(format!("GLSL compile error: {err}"));
            }
        }
        let shader = rd.shader_create_from_spirv(&spirv);
        if shader.is_invalid() {
            return Err("shader_create_from_spirv returned invalid RID".into());
        }

        // --- buffers ---
        let field = make_ridged_field(dim, seed);
        let height_bytes = f32s_to_bytes(&field);
        let ones = vec![1.0_f32; n];
        let ones_bytes = f32s_to_bytes(&ones);

        let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };
        let height_pba = PackedByteArray::from(height_bytes.as_slice());
        let acc_init_pba = PackedByteArray::from(ones_bytes.as_slice());

        let height_rid = rd
            .storage_buffer_create_ex(bsize(height_bytes.len()))
            .data(&height_pba)
            .done();
        // Two ping-pong acc buffers, both initialised to 1.0.
        let acc_a_rid = rd
            .storage_buffer_create_ex(bsize(ones_bytes.len()))
            .data(&acc_init_pba)
            .done();
        let acc_b_rid = rd
            .storage_buffer_create_ex(bsize(ones_bytes.len()))
            .data(&acc_init_pba)
            .done();

        // Two uniform sets. set_ab: prev=A(bind1), next=B(bind2). set_ba: prev=B(bind1), next=A(bind2).
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
        let wg = ((dim as u32) + 15) / 16; // local_size 16x16

        // --- dispatch loop, timed with GPU timestamps inside one compute list/submit ---
        let cl = rd.compute_list_begin();
        rd.compute_list_bind_compute_pipeline(cl, pipeline);

        rd.capture_timestamp("flow_start");
        // Iteration i reads "prev" and writes "next"; alternate which physical buffer is which.
        // After iteration i, the freshly-written buffer is: B if i even, A if i odd.
        for i in 0..iters {
            let set = if i % 2 == 0 { set_ab } else { set_ba };
            rd.compute_list_bind_uniform_set(cl, set, 0);
            rd.compute_list_set_push_constant(cl, &push_pba, push_pba.len() as u32);
            rd.compute_list_dispatch(cl, wg, wg, 1);
            // Next iteration reads what this one wrote -> need a memory barrier between them.
            rd.compute_list_add_barrier(cl);
        }
        rd.capture_timestamp("flow_end");
        rd.compute_list_end();

        let wall0 = std::time::Instant::now();
        rd.submit();
        rd.sync();
        let wall_us = wall0.elapsed().as_secs_f64() * 1.0e6;

        // --- read GPU + CPU timestamps (microseconds since engine start) ---
        let count = rd.get_captured_timestamps_count();
        let mut t_start: Option<u64> = None;
        let mut t_end: Option<u64> = None;
        let mut c_start: Option<u64> = None;
        let mut c_end: Option<u64> = None;
        for idx in 0..count {
            let name = rd.get_captured_timestamp_name(idx).to_string();
            let tg = rd.get_captured_timestamp_gpu_time(idx);
            let tc = rd.get_captured_timestamp_cpu_time(idx);
            if name == "flow_start" { t_start = Some(tg); c_start = Some(tc); }
            else if name == "flow_end" { t_end = Some(tg); c_end = Some(tc); }
        }
        let gpu_us = match (t_start, t_end) {
            (Some(s), Some(e)) if e >= s => (e - s) as f64,
            _ => -1.0, // signal: timestamps unavailable / unordered
        };
        let cpu_us = match (c_start, c_end) {
            (Some(s), Some(e)) if e >= s => (e - s) as f64,
            _ => -1.0,
        };

        // --- read final acc buffer ---
        // After `iters` iterations the last write went to: B if iters odd? Let's track:
        // i runs 0..iters. iteration i writes "next" = B when i even, A when i odd.
        // So the LAST iteration index is iters-1; it wrote B if (iters-1) even i.e. iters odd,
        // and A if iters even.
        let final_rid = if iters % 2 == 1 { acc_b_rid } else { acc_a_rid };
        let acc_pba = rd.buffer_get_data(final_rid);
        let acc = bytes_to_f32s(&acc_pba.to_vec());

        // --- free ---
        rd.free_rid(height_rid);
        rd.free_rid(acc_a_rid);
        rd.free_rid(acc_b_rid);
        rd.free_rid(pipeline);
        // Freeing the shader cascades its uniform sets (set_ab, set_ba).
        rd.free_rid(shader);
        // The local RenderingDevice itself is an Object (manually managed). Without this the
        // driver runs out of device slots after dozens of runs ("Failed to initialize driver
        // for device" / create_local_rendering_device returns null on later calls).
        rd.free();

        if gpu_us < 0.0 {
            return Err("GPU timestamps unavailable or unordered (capture_timestamp returned no usable times)".into());
        }
        if acc.len() != n {
            return Err(format!("acc readback: expected {n} f32, got {}", acc.len()));
        }
        Ok(RunResult { gpu_us, cpu_us, wall_us, acc })
    }
}

/// Result of one timed flow-accumulation run.
struct RunResult {
    gpu_us: f64,
    cpu_us: f64,
    wall_us: f64,
    acc: Vec<f32>,
}

// ---------------------------------------------------------------------------
// Unit tests (pure helpers; no Godot runtime)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_is_finite_and_right_size() {
        let f = make_ridged_field(16, 7);
        assert_eq!(f.len(), 256);
        assert!(f.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn field_is_deterministic() {
        assert_eq!(make_ridged_field(32, 3), make_ridged_field(32, 3));
    }

    #[test]
    fn field_varies_with_seed() {
        assert_ne!(make_ridged_field(32, 3), make_ridged_field(32, 4));
    }

    #[test]
    fn push_constant_is_16_bytes() {
        assert_eq!(build_push(256, 1.45).len(), 16);
    }

    #[test]
    fn max_abs_delta_basic() {
        let a = vec![1.0_f32, 2.0, 3.0];
        let b = vec![1.0_f32, 2.5, 3.0];
        assert!((max_abs_delta(&a, &b) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn f32_byte_roundtrip() {
        let v = vec![1.0_f32, -2.5, 3.25, 1e9];
        assert_eq!(bytes_to_f32s(&f32s_to_bytes(&v)), v);
    }
}
