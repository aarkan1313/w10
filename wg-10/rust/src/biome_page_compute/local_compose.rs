//! Local RenderingDevice compose/blend engines for biome composition parity gates.

use godot::classes::{
    rendering_device::ShaderStage, RdShaderSource, RdUniform, RenderingDevice, RenderingServer,
};
use godot::prelude::*;

use super::abi::{POOL_SLOTS, STABLE_ITERS, TRUNCATE};
use super::helpers::{
    bytes_to_f32s, f32s_to_bytes, make_image_uniform, make_scratch_image_1x1,
    make_storage_uniform,
};
use super::kernels::{gaussian_kernel1d, KERNEL_STRIDE};
use super::scheduler::Scheduler;
use super::sigma_registry::{compose_sigmas, KernelParams};
use super::Wg10BiomePageCompute;

impl Wg10BiomePageCompute {
    /// COMPOSE engine (Slice-4b.11): the shared GPU setup for the compose layer. Allocates the SAME
    /// machine binding set (0..40) as `run_inner` (the machine declares them all, so the uniform set
    /// must satisfy every binding), uploads the compose initial buffers (height=acc0, base=acc_w0,
    /// pool0=f, pool1=w), builds the kernel buffer with the single compose relief sigma (6.0), opens
    /// one compute list, builds a Scheduler with the compose params, runs `op` (the fold/blend
    /// sequence), then reads back `height` (the composed result -- compose has NO apron / crop, the
    /// whole rows*cols field is the answer). WINDOWED only (local RD null headless). Concats the
    /// MOUNTAIN fragment purely to satisfy the machine's `biome_pass()` declaration -- the compose
    /// passes are handled INLINE in main() and never reach the fragment.
    #[allow(clippy::too_many_arguments)]
    fn run_compose_engine(
        &self,
        rows: usize,
        cols: usize,
        favor_strength: f32,
        relief_confidence_floor: f32,
        acc0: &[f32],   // -> height  (binding 14)
        accw0: &[f32],  // -> base    (binding 9)
        f0: &[f32],     // -> pool0   (binding 24)
        w0: &[f32],     // -> pool1   (binding 25)
        use_favored: bool,
    ) -> Result<(Vec<f32>, Vec<f32>), String> {
        let n = rows * cols;
        if acc0.len() != n || accw0.len() != n || f0.len() != n || w0.len() != n {
            return Err(format!("compose engine: buffer len != rows*cols ({n})"));
        }
        let prim = self.primitives_src.as_deref().ok_or("no GLSL source loaded")?;
        let machine = self.machine_src.as_deref().ok_or("no GLSL source loaded")?;
        let fragment = self.compose_fragment.as_deref()
            .ok_or("no compose fragment loaded (call load_compose_fragment)")?;

        let mut rd: Gd<RenderingDevice> = RenderingServer::singleton()
            .create_local_rendering_device()
            .ok_or_else(|| {
                "create_local_rendering_device returned null (headless / no device)".to_string()
            })?;

        // compile: machine + a fragment (mountain) so biome_pass() is defined (compose passes never
        // reach it -- they're inline in main()).
        let machine_plus_fragment = format!("{machine}\n{fragment}");
        let glsl_stripped =
            crate::primitive_probe::concat_glsl_hoist_version(prim, &machine_plus_fragment);
        let mut src = RdShaderSource::new_gd();
        src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
        let spirv = match rd.shader_compile_spirv_from_source(&src) {
            Some(s) => s,
            None => { rd.free(); return Err("shader_compile_spirv_from_source returned null".to_string()); }
        };
        {
            let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
            if !err.is_empty() { rd.free(); return Err(format!("GLSL compile error: {err}")); }
        }
        let shader = rd.shader_create_from_spirv(&spirv);
        if shader.is_invalid() { rd.free(); return Err("shader_create_from_spirv returned invalid RID".into()); }

        let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };
        let field_bytes = n * 4;
        let zeros = vec![0.0_f32; n];
        let zeros_pba = PackedByteArray::from(f32s_to_bytes(&zeros).as_slice());
        let mk_zero = |rd: &mut Gd<RenderingDevice>| -> Rid {
            rd.storage_buffer_create_ex(bsize(field_bytes)).data(&zeros_pba).done()
        };
        let mk_data = |rd: &mut Gd<RenderingDevice>, data: &[f32]| -> Rid {
            let pba = PackedByteArray::from(f32s_to_bytes(data).as_slice());
            rd.storage_buffer_create_ex(bsize(field_bytes)).data(&pba).done()
        };

        // Named buffers 0..18. Compose roles: 6=range_envelope(relief_a), 7=lowland(w_acc),
        // 8=massif(relief_b), 9=base(acc_w), 14=height(acc). The rest are inert scratch (zeroed).
        let mut named: Vec<Rid> = Vec::with_capacity(19);
        for b in 0..19usize {
            let rid = match b {
                9 => mk_data(&mut rd, accw0),  // base = acc_w0
                14 => mk_data(&mut rd, acc0),  // height = acc0
                _ => mk_zero(&mut rd),
            };
            named.push(rid);
        }

        // kernel buffer (19): single compose relief sigma (6.0) at slot 0.
        let sigmas = compose_sigmas();
        let n_slots = sigmas.len();
        let mut packed = vec![0.0_f32; n_slots * KERNEL_STRIDE];
        for (slot, &sg) in sigmas.iter().enumerate() {
            let k = gaussian_kernel1d(sg, TRUNCATE);
            if k.len() > KERNEL_STRIDE {
                rd.free_rid(shader); rd.free();
                return Err(format!("compose kernel len {} (sigma {sg}) > KERNEL_STRIDE {KERNEL_STRIDE}", k.len()));
            }
            let base = slot * KERNEL_STRIDE;
            packed[base..base + k.len()].copy_from_slice(&k);
        }
        let packed_pba = PackedByteArray::from(f32s_to_bytes(&packed).as_slice());
        let b_kernel = rd.storage_buffer_create_ex(bsize(packed.len() * 4)).data(&packed_pba).done();
        let kparams = KernelParams::from_sigmas(&sigmas);

        let b_flow_pre = mk_zero(&mut rd); // 20
        let b_acc_a = mk_zero(&mut rd);    // 21
        let b_acc_b = mk_zero(&mut rd);    // 22

        // core output (23): unused by compose (no CROP), but the binding must exist for the set.
        let b_core = mk_zero(&mut rd);

        // pool slots 24..39: pool0 = f, pool1 = w, the rest inert.
        let mut b_pool: Vec<Rid> = Vec::with_capacity(POOL_SLOTS);
        for slot in 0..POOL_SLOTS {
            let rid = match slot {
                0 => mk_data(&mut rd, f0),
                1 => mk_data(&mut rd, w0),
                _ => mk_zero(&mut rd),
            };
            b_pool.push(rid);
        }

        // vent buffer (40): inert for compose (machine declares the binding).
        let vent_stride = crate::recipes_volcanic::volcanic::VENT_STRIDE;
        let maxv = crate::recipes_volcanic::volcanic::MAX_VENTS;
        let vent_zeros = vec![0.0_f32; maxv * vent_stride];
        let vent_pba = PackedByteArray::from(f32s_to_bytes(&vent_zeros).as_slice());
        let b_vents = rd.storage_buffer_create_ex(bsize(vent_zeros.len() * 4)).data(&vent_pba).done();

        // uniform set (same binding map as run_inner).
        let mut bindings: Vec<(i32, Rid)> = Vec::new();
        for (b, &rid) in named.iter().enumerate() {
            bindings.push((b as i32, rid));
        }
        bindings.push((19, b_kernel));
        bindings.push((20, b_flow_pre));
        bindings.push((21, b_acc_a));
        bindings.push((22, b_acc_b));
        bindings.push((23, b_core));
        for (k, &rid) in b_pool.iter().enumerate() {
            bindings.push((24 + k as i32, rid));
        }
        bindings.push((40, b_vents));
        let mut uniforms: Array<Gd<RdUniform>> = Array::new();
        for (bind, rid) in bindings.iter() {
            uniforms.push(&make_storage_uniform(*bind, *rid));
        }
        // Scratch 1x1 image at binding 41 (machine declares out_img for PASS_CROP_IMG; compose
        // never dispatches it -> result byte-identical, but the set must satisfy binding 41).
        let scratch_img = make_scratch_image_1x1(&mut rd);
        uniforms.push(&make_image_uniform(41, scratch_img));
        bindings.push((41, scratch_img));
        let uset = rd.uniform_set_create(&uniforms, shader, 0);
        let pipeline = rd.compute_pipeline_create(shader);

        let wg_full_x = (cols as u32).div_ceil(16);
        let wg_full_y = (rows as u32).div_ceil(16);

        // pre-validate the compose sigma BEFORE the list opens (kp uses .expect()).
        for &s in &sigmas { let _ = kparams.kp(s); }

        let cl = rd.compute_list_begin();
        rd.compute_list_bind_compute_pipeline(cl, pipeline);
        let mut sched = Scheduler {
            rd: &mut rd,
            cl,
            uset,
            rows: rows as i32,
            cols: cols as i32,
            apron: 0,
            seed: 0,
            spacing: 0.0,
            ox: 0.0,
            oz: 0.0,
            feature_span_m: 0.0,
            vent_count: 0,
            favor_strength,
            relief_confidence_floor,
            relief_m: 0.0,                 // compose engine never crops to the runtime image
            wg_full_x,
            wg_full_y,
            wg_core_x: wg_full_x,
            wg_core_y: wg_full_y,
            kparams,
            flow_iters: STABLE_ITERS, // compose never runs flow; value is irrelevant but required
            flow_on: true,            // compose never runs schedule_mountain; irrelevant but required
        };

        // ONE compose_biomes fold step: acc=height, acc_w=base, f=pool0, w=pool1 pre-loaded.
        //   w_acc = acc_w/(acc_w+w+1e-12) -> lowland
        //   acc   = blend(acc, f, w_acc)  -> height
        //   acc_w += w (PASS_COMPOSE_ACCW_ADD) -> base
        // The engine reads back BOTH height (new acc) and base (new acc_w) for the next step.
        sched.compose_wacc();
        if use_favored {
            sched.blend_favored_step();
        } else {
            sched.blend_field_step();
        }
        sched.compose_accw_add();

        rd.compute_list_end();
        rd.submit();
        rd.sync();

        // read back the composed accumulator (height) AND the updated acc_w (base). BlendPair
        // callers ignore acc_w; the Fold path feeds it to the next step.
        let height_pba = rd.buffer_get_data(named[14]);
        let height = bytes_to_f32s(&height_pba.to_vec());
        let accw_pba = rd.buffer_get_data(named[9]);
        let accw = bytes_to_f32s(&accw_pba.to_vec());

        for (_, rid) in bindings.iter() { rd.free_rid(*rid); }
        rd.free_rid(pipeline);
        rd.free_rid(shader);
        rd.free();

        if height.len() != n {
            return Err(format!("compose readback: expected {n} f32, got {}", height.len()));
        }
        Ok((height, accw))
    }

    /// Run ONE compose fold step on the GPU and return (new_acc, new_acc_w). Thin wrapper over
    /// `run_compose_engine` (which reads back both the new acc=height and the GPU-updated
    /// acc_w=base via PASS_COMPOSE_ACCW_ADD). Used by `run_compose_inner` for the N>=2 case.
    #[allow(clippy::too_many_arguments)]
    fn run_compose_step(
        &self,
        rows: usize,
        cols: usize,
        favor_strength: f32,
        relief_confidence_floor: f32,
        acc: &[f32],
        acc_w: &[f32],
        f: &[f32],
        w: &[f32],
        use_favored: bool,
    ) -> Result<(Vec<f32>, Vec<f32>), String> {
        self.run_compose_engine(
            rows, cols, favor_strength, relief_confidence_floor, acc, acc_w, f, w, use_favored,
        )
    }

    /// COMPOSE fold (Slice-4b.11): GPU port of `biome_compose::compose_biomes`. Mirrors the oracle
    /// fold EXACTLY: n==1 returns fields[0]; use_favored = (!mode_is_field) && (n==2); running
    /// accumulator acc=fields[0], acc_w=weights[0], then for each subsequent (f,w):
    /// w_acc=acc_w/(acc_w+w+1e-12), acc=blend(acc,f,w_acc), acc_w+=w. The N>2 fold uses FIELD blend
    /// (use_favored is FALSE for n!=2), so each step is independent and can run as its own GPU
    /// engine call (the per-step acc/acc_w are carried on the CPU between calls).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_compose_inner(
        &self,
        fields: &[Vec<f32>],
        weights: &[Vec<f32>],
        rows: usize,
        cols: usize,
        mode_is_field: bool,
        favor_strength: f32,
        relief_confidence_floor: f32,
    ) -> Result<Vec<f32>, String> {
        let n = rows * cols;
        if fields.len() != weights.len() {
            return Err(format!("compose: fields/weights count mismatch {} vs {}", fields.len(), weights.len()));
        }
        if fields.is_empty() {
            return Err("compose requires at least one field".into());
        }
        if fields.len() == 1 {
            // n==1: return fields[0] unchanged (oracle short-circuit; no GPU needed).
            return Ok(fields[0].clone());
        }
        // use_favored EXACTLY for n==2 && height_favored mode (mirrors compose_biomes).
        let use_favored = (!mode_is_field) && (fields.len() == 2);

        let mut acc = fields[0].clone();
        let mut acc_w = weights[0].clone();
        for k in 1..fields.len() {
            let (new_acc, new_acc_w) = self.run_compose_step(
                rows, cols, favor_strength, relief_confidence_floor,
                &acc, &acc_w, &fields[k], &weights[k], use_favored,
            )?;
            acc = new_acc;
            acc_w = new_acc_w;
        }
        if acc.len() != n {
            return Err(format!("compose result len {} != {n}", acc.len()));
        }
        Ok(acc)
    }

    /// BLEND-PAIR (Slice-4b.11): GPU port of `biome_compose::blend_field` / `blend_height_favored`.
    /// A SINGLE blend with `w_a` used DIRECTLY (loaded into lowland=w_acc), NOT the accumulator
    /// fold. acc=a (height), f=b (pool0).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_blend_inner(
        &self,
        a: &[f32],
        b: &[f32],
        w_a: &[f32],
        rows: usize,
        cols: usize,
        mode_is_field: bool,
        favor_strength: f32,
        relief_confidence_floor: f32,
    ) -> Result<Vec<f32>, String> {
        // The engine loads height=acc0=a, pool0=f=b, and the BlendPair op uses lowland(=w_acc) as
        // the blend weight DIRECTLY -- so pre-load w_a into lowland. The engine writes height=acc0
        // and base=accw0; for BlendPair acc_w is irrelevant, so feed it zeros. We need w_a in
        // lowland, which the engine does NOT take as a param -> use a dedicated load via accw0 slot?
        // No: extend the engine call to also seed lowland. Simpler: the BlendPair op reads lowland,
        // so seed it through a thin wrapper that uploads w_a into binding 7. We thread w_a via the
        // engine's `accw0`? base is binding 9, not lowland. So run a SPECIALIZED engine path.
        self.run_blend_engine(
            rows, cols, favor_strength, relief_confidence_floor, a, b, w_a, !mode_is_field,
        )
    }

    /// BLEND-PAIR engine: like `run_compose_engine` but seeds lowland (w_acc) = w_a DIRECTLY and
    /// runs a single blend step (no acc_w fold). Kept separate so the compose Fold engine's buffer
    /// roles stay clean (Fold computes w_acc from acc_w; BlendPair uses w_a verbatim).
    #[allow(clippy::too_many_arguments)]
    fn run_blend_engine(
        &self,
        rows: usize,
        cols: usize,
        favor_strength: f32,
        relief_confidence_floor: f32,
        a: &[f32],   // -> height (acc)
        b: &[f32],   // -> pool0  (f)
        w_a: &[f32], // -> lowland (w_acc, used directly)
        use_favored: bool,
    ) -> Result<Vec<f32>, String> {
        let n = rows * cols;
        if a.len() != n || b.len() != n || w_a.len() != n {
            return Err(format!("blend engine: buffer len != rows*cols ({n})"));
        }
        let prim = self.primitives_src.as_deref().ok_or("no GLSL source loaded")?;
        let machine = self.machine_src.as_deref().ok_or("no GLSL source loaded")?;
        let fragment = self.compose_fragment.as_deref()
            .ok_or("no compose fragment loaded (call load_compose_fragment)")?;

        let mut rd: Gd<RenderingDevice> = RenderingServer::singleton()
            .create_local_rendering_device()
            .ok_or_else(|| "create_local_rendering_device returned null (headless / no device)".to_string())?;

        let machine_plus_fragment = format!("{machine}\n{fragment}");
        let glsl_stripped =
            crate::primitive_probe::concat_glsl_hoist_version(prim, &machine_plus_fragment);
        let mut src = RdShaderSource::new_gd();
        src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
        let spirv = match rd.shader_compile_spirv_from_source(&src) {
            Some(s) => s,
            None => { rd.free(); return Err("shader_compile_spirv_from_source returned null".to_string()); }
        };
        {
            let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
            if !err.is_empty() { rd.free(); return Err(format!("GLSL compile error: {err}")); }
        }
        let shader = rd.shader_create_from_spirv(&spirv);
        if shader.is_invalid() { rd.free(); return Err("shader_create_from_spirv returned invalid RID".into()); }

        let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };
        let field_bytes = n * 4;
        let zeros = vec![0.0_f32; n];
        let zeros_pba = PackedByteArray::from(f32s_to_bytes(&zeros).as_slice());
        let mk_zero = |rd: &mut Gd<RenderingDevice>| -> Rid {
            rd.storage_buffer_create_ex(bsize(field_bytes)).data(&zeros_pba).done()
        };
        let mk_data = |rd: &mut Gd<RenderingDevice>, data: &[f32]| -> Rid {
            let pba = PackedByteArray::from(f32s_to_bytes(data).as_slice());
            rd.storage_buffer_create_ex(bsize(field_bytes)).data(&pba).done()
        };

        // Named 0..18: 7=lowland(w_a, direct), 14=height(a). The rest inert.
        let mut named: Vec<Rid> = Vec::with_capacity(19);
        for b_i in 0..19usize {
            let rid = match b_i {
                7 => mk_data(&mut rd, w_a),
                14 => mk_data(&mut rd, a),
                _ => mk_zero(&mut rd),
            };
            named.push(rid);
        }

        let sigmas = compose_sigmas();
        let mut packed = vec![0.0_f32; sigmas.len() * KERNEL_STRIDE];
        for (slot, &sg) in sigmas.iter().enumerate() {
            let k = gaussian_kernel1d(sg, TRUNCATE);
            if k.len() > KERNEL_STRIDE {
                rd.free_rid(shader); rd.free();
                return Err(format!("compose kernel len {} (sigma {sg}) > KERNEL_STRIDE", k.len()));
            }
            packed[slot * KERNEL_STRIDE..slot * KERNEL_STRIDE + k.len()].copy_from_slice(&k);
        }
        let packed_pba = PackedByteArray::from(f32s_to_bytes(&packed).as_slice());
        let b_kernel = rd.storage_buffer_create_ex(bsize(packed.len() * 4)).data(&packed_pba).done();
        let kparams = KernelParams::from_sigmas(&sigmas);

        let b_flow_pre = mk_zero(&mut rd);
        let b_acc_a = mk_zero(&mut rd);
        let b_acc_b = mk_zero(&mut rd);
        let b_core = mk_zero(&mut rd);

        let mut b_pool: Vec<Rid> = Vec::with_capacity(POOL_SLOTS);
        for slot in 0..POOL_SLOTS {
            let rid = if slot == 0 { mk_data(&mut rd, b) } else { mk_zero(&mut rd) };
            b_pool.push(rid);
        }

        let vent_stride = crate::recipes_volcanic::volcanic::VENT_STRIDE;
        let maxv = crate::recipes_volcanic::volcanic::MAX_VENTS;
        let vent_zeros = vec![0.0_f32; maxv * vent_stride];
        let vent_pba = PackedByteArray::from(f32s_to_bytes(&vent_zeros).as_slice());
        let b_vents = rd.storage_buffer_create_ex(bsize(vent_zeros.len() * 4)).data(&vent_pba).done();

        let mut bindings: Vec<(i32, Rid)> = Vec::new();
        for (b_i, &rid) in named.iter().enumerate() { bindings.push((b_i as i32, rid)); }
        bindings.push((19, b_kernel));
        bindings.push((20, b_flow_pre));
        bindings.push((21, b_acc_a));
        bindings.push((22, b_acc_b));
        bindings.push((23, b_core));
        for (k, &rid) in b_pool.iter().enumerate() { bindings.push((24 + k as i32, rid)); }
        bindings.push((40, b_vents));
        let mut uniforms: Array<Gd<RdUniform>> = Array::new();
        for (bind, rid) in bindings.iter() { uniforms.push(&make_storage_uniform(*bind, *rid)); }
        // Scratch 1x1 image at binding 41 (machine declares out_img for PASS_CROP_IMG; blend never
        // dispatches it -> result byte-identical, but the set must satisfy binding 41).
        let scratch_img = make_scratch_image_1x1(&mut rd);
        uniforms.push(&make_image_uniform(41, scratch_img));
        bindings.push((41, scratch_img));
        let uset = rd.uniform_set_create(&uniforms, shader, 0);
        let pipeline = rd.compute_pipeline_create(shader);

        let wg_full_x = (cols as u32).div_ceil(16);
        let wg_full_y = (rows as u32).div_ceil(16);
        for &s in &sigmas { let _ = kparams.kp(s); }

        let cl = rd.compute_list_begin();
        rd.compute_list_bind_compute_pipeline(cl, pipeline);
        let mut sched = Scheduler {
            rd: &mut rd, cl, uset,
            rows: rows as i32, cols: cols as i32, apron: 0, seed: 0,
            spacing: 0.0, ox: 0.0, oz: 0.0, feature_span_m: 0.0, vent_count: 0,
            favor_strength, relief_confidence_floor, relief_m: 0.0,
            wg_full_x, wg_full_y, wg_core_x: wg_full_x, wg_core_y: wg_full_y, kparams,
            flow_iters: STABLE_ITERS, // blend never runs flow
            flow_on: true,            // blend never runs schedule_mountain; irrelevant but required
        };
        if use_favored { sched.blend_favored_step(); } else { sched.blend_field_step(); }

        rd.compute_list_end();
        rd.submit();
        rd.sync();

        let height_pba = rd.buffer_get_data(named[14]);
        let height = bytes_to_f32s(&height_pba.to_vec());

        for (_, rid) in bindings.iter() { rd.free_rid(*rid); }
        rd.free_rid(pipeline);
        rd.free_rid(shader);
        rd.free();

        if height.len() != n {
            return Err(format!("blend readback: expected {n} f32, got {}", height.len()));
        }
        Ok(height)
    }
}
