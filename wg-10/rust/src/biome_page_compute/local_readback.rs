//! Local RenderingDevice readback engine for biome page parity gates.

use godot::classes::{
    rendering_device::ShaderStage, RdShaderSource, RdUniform, RenderingDevice, RenderingServer,
};
use godot::prelude::*;

use super::abi::{POOL_SLOTS, TRUNCATE};
use super::helpers::{
    bytes_to_f32s, f32s_to_bytes, make_image_uniform, make_scratch_image_1x1,
    make_storage_uniform,
};
use super::kernels::{gaussian_kernel1d, KERNEL_STRIDE};
use super::schedule_coast::schedule_coast;
use super::schedule_desert::schedule_desert;
use super::schedule_glacial::schedule_glacial;
use super::schedule_grassland::schedule_grassland;
use super::schedule_karst::schedule_karst;
use super::schedule_mountain::schedule_mountain;
use super::schedule_rainforest::schedule_rainforest;
use super::schedule_temperate::schedule_temperate;
use super::schedule_tundra::schedule_tundra;
use super::schedule_volcanic::schedule_volcanic;
use super::schedule_wetland::schedule_wetland;
use super::scheduler::Scheduler;
use super::sigma_registry::{biome_sigmas, KernelParams};
use super::Wg10BiomePageCompute;

impl Wg10BiomePageCompute {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_inner(
        &self,
        spacing: f32,
        ox: f32,
        oz: f32,
        rows: usize,
        cols: usize,
        apron: usize,
        seed: i32,
        feature_span_m: f32,
        biome_fragment: &str,
        biome: &str,
        flow_iters: usize,
    ) -> Result<Vec<f32>, String> {
        if rows <= 2 * apron || cols <= 2 * apron {
            return Err(format!("apron {apron} too large for padded {rows}x{cols}"));
        }
        let prim = self.primitives_src.as_deref().ok_or("no GLSL source loaded")?;
        let machine = self.machine_src.as_deref().ok_or("no GLSL source loaded")?;
        let n = rows * cols;
        let core_rows = rows - 2 * apron;
        let core_cols = cols - 2 * apron;
        let core_n = core_rows * core_cols;

        let mut rd: Gd<RenderingDevice> = RenderingServer::singleton()
            .create_local_rendering_device()
            .ok_or_else(|| {
                "create_local_rendering_device returned null (headless / no device)".to_string()
            })?;

        // --- compile: concat primitives + machine + biome fragment, strip #[...] lines AND hoist
        // #version to line 1. The machine (NOT the primitives helpers, NOT the fragment) carries
        // the single #version; concat_glsl_hoist_version scans the WHOLE joined text and pulls the
        // first #version to line 1, so passing (primitives, machine + "\n" + fragment) keeps the
        // machine's #version as the first non-helper line and appends the fragment last. ---
        let machine_plus_fragment = format!("{machine}\n{biome_fragment}");
        let glsl_stripped =
            crate::primitive_probe::concat_glsl_hoist_version(prim, &machine_plus_fragment);
        let mut src = RdShaderSource::new_gd();
        src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
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

        // --- allocate buffers ---
        // NOTE: duplicates alloc_apron_buffers' recipe (kept inline so the 210 byte-identical
        // readback tests stay frozen). Unify once the 576 parity gate is green.
        let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };
        let field_bytes = n * 4;
        let zeros = vec![0.0_f32; n];
        let zeros_pba = PackedByteArray::from(f32s_to_bytes(&zeros).as_slice());
        let mk_field = |rd: &mut Gd<RenderingDevice>| -> Rid {
            rd.storage_buffer_create_ex(bsize(field_bytes)).data(&zeros_pba).done()
        };
        let b_wx = mk_field(&mut rd);          // 0
        let b_wz = mk_field(&mut rd);          // 1
        let b_regional = mk_field(&mut rd);    // 2
        let b_ranges = mk_field(&mut rd);      // 3
        let b_ridge_detail = mk_field(&mut rd);// 4
        let b_near_detail = mk_field(&mut rd); // 5
        let b_range_env = mk_field(&mut rd);   // 6
        let b_lowland = mk_field(&mut rd);     // 7
        let b_massif = mk_field(&mut rd);      // 8
        let b_base = mk_field(&mut rd);        // 9
        let b_primary = mk_field(&mut rd);     // 10
        let b_trib = mk_field(&mut rd);        // 11
        let b_high = mk_field(&mut rd);        // 12
        let b_valley = mk_field(&mut rd);      // 13
        let b_height = mk_field(&mut rd);      // 14
        let b_floor = mk_field(&mut rd);       // 15
        let b_gauss_in = mk_field(&mut rd);    // 16
        let b_gauss_mid = mk_field(&mut rd);   // 17
        let b_gauss_out = mk_field(&mut rd);   // 18

        // PACKED kernel buffer (19): all distinct sigmas' kernels at fixed offsets
        // (slot * KERNEL_STRIDE). Built + uploaded ONCE so the whole pipeline runs inside a
        // SINGLE compute list (no mid-list buffer_update); the active kernel is selected by
        // the `koffset` push constant. Build the kernels in the fixed sigma order. The sigma SET
        // is biome-specific (each schedule_<biome> requests its own blurs); pick it BEFORE the
        // list opens so a wrong/missing biome errors cleanly.
        let sigmas = match biome_sigmas(biome) {
            Some(s) => s,
            None => {
                rd.free_rid(shader);
                rd.free();
                return Err(format!("no sigma list for biome '{biome}' (add a biome_sigmas arm)"));
            }
        };
        let n_slots = sigmas.len();
        let mut packed = vec![0.0_f32; n_slots * KERNEL_STRIDE];
        for (slot, &sg) in sigmas.iter().enumerate() {
            let k = gaussian_kernel1d(sg, TRUNCATE);
            if k.len() > KERNEL_STRIDE {
                rd.free_rid(shader);
                rd.free();
                return Err(format!("gaussian kernel len {} (sigma {sg}) > KERNEL_STRIDE {KERNEL_STRIDE}", k.len()));
            }
            let base = slot * KERNEL_STRIDE;
            packed[base..base + k.len()].copy_from_slice(&k);
        }
        let packed_pba = PackedByteArray::from(f32s_to_bytes(&packed).as_slice());
        let b_kernel = rd
            .storage_buffer_create_ex(bsize(packed.len() * 4))
            .data(&packed_pba)
            .done(); // 19
        // sigma -> (koffset, kradius) lookup, resolved BEFORE the compute list opens. Stored as a
        // small fixed Vec (KernelParams) so the in-list `.expect` is provably-unreachable and the
        // borrow-checker is happy across the open-list `&mut rd` reborrows in the Scheduler.
        let kparams = KernelParams::from_sigmas(&sigmas);

        let b_flow_pre = mk_field(&mut rd);    // 20
        let b_acc_a = mk_field(&mut rd);       // 21
        let b_acc_b = mk_field(&mut rd);       // 22

        // core output (23)
        let core_zeros = vec![0.0_f32; core_n];
        let core_pba = PackedByteArray::from(f32s_to_bytes(&core_zeros).as_slice());
        let b_core = rd
            .storage_buffer_create_ex(bsize(core_n * 4))
            .data(&core_pba)
            .done(); // 23

        // GENERIC scratch POOL (bindings 24..24+POOL_SLOTS-1): POOL_SLOTS reusable field buffers
        // any biome can stage sub-fields in (grassland uses all 12). Allocated for EVERY biome so
        // the uniform set always satisfies the machine's pool bindings (mountain just never reads
        // them -> its result is unchanged). Additive: the fixed named buffers above are untouched.
        let b_pool: Vec<Rid> = (0..POOL_SLOTS).map(|_| mk_field(&mut rd)).collect();

        // VENT buffer (binding 40): VOLCANIC's CPU-built packed vent list (vx,vz,amp + 4 flow dirs
        // per vent, padded to MAX_VENTS). THE KEY INSIGHT -- the numpy PCG64 RNG that places the
        // vents stays in RUST (recipes_volcanic::packed_vents, parity-exact); this buffer is the
        // only thing the GPU consumes (PURE f32 cone/crater/shield/flow math, NO RNG in GLSL).
        // ADDITIVE: every biome gets the binding (so the uniform set always satisfies the machine's
        // binding 40); the 10 non-volcanic biomes get a zeroed buffer + vent_count=0 (never read).
        let (vent_packed, vent_count): (Vec<f32>, usize) = if biome == "volcanic" {
            crate::recipes_volcanic::volcanic::packed_vents(
                &crate::recipes_volcanic::volcanic::STRATOVOLCANO_CLUSTER,
                seed as i64,
                feature_span_m as f64,
            )
        } else {
            let stride = crate::recipes_volcanic::volcanic::VENT_STRIDE;
            let maxv = crate::recipes_volcanic::volcanic::MAX_VENTS;
            (vec![0.0_f32; maxv * stride], 0)
        };
        let vent_pba = PackedByteArray::from(f32s_to_bytes(&vent_packed).as_slice());
        let b_vents = rd
            .storage_buffer_create_ex(bsize(vent_packed.len() * 4))
            .data(&vent_pba)
            .done(); // 40

        // one uniform set binding the 24 fixed buffers + POOL_SLOTS pool buffers + vent buffer.
        let mut bindings: Vec<(i32, Rid)> = vec![
            (0, b_wx), (1, b_wz), (2, b_regional), (3, b_ranges), (4, b_ridge_detail),
            (5, b_near_detail), (6, b_range_env), (7, b_lowland), (8, b_massif), (9, b_base),
            (10, b_primary), (11, b_trib), (12, b_high), (13, b_valley), (14, b_height),
            (15, b_floor), (16, b_gauss_in), (17, b_gauss_mid), (18, b_gauss_out),
            (19, b_kernel), (20, b_flow_pre), (21, b_acc_a), (22, b_acc_b), (23, b_core),
        ];
        for (k, &rid) in b_pool.iter().enumerate() {
            bindings.push((24 + k as i32, rid));
        }
        bindings.push((40, b_vents));
        let mut uniforms: Array<Gd<RdUniform>> = Array::new();
        for (bind, rid) in bindings.iter() {
            uniforms.push(&make_storage_uniform(*bind, *rid));
        }
        // Scratch 1x1 image at binding 41 (machine declares out_img for the RUNTIME PASS_CROP_IMG;
        // this readback path never dispatches it, so the image is never written -> core readback is
        // byte-identical, but the uniform set must still satisfy binding 41). Pushed into `bindings`
        // AFTER the storage-uniform loop so the trailing free loop frees its texture RID, while its
        // uniform is added as an IMAGE (not a storage buffer) below.
        let scratch_img = make_scratch_image_1x1(&mut rd);
        uniforms.push(&make_image_uniform(41, scratch_img));
        bindings.push((41, scratch_img));
        let uset = rd.uniform_set_create(&uniforms, shader, 0);
        let pipeline = rd.compute_pipeline_create(shader);

        // workgroup counts (local_size 16x16). full-field uses padded dims; crop uses core.
        let wg_full_x = (cols as u32).div_ceil(16);
        let wg_full_y = (rows as u32).div_ceil(16);
        let wg_core_x = (core_cols as u32).div_ceil(16);
        let wg_core_y = (core_rows as u32).div_ceil(16);

        // PRE-VALIDATE every sigma the pipeline will request, BEFORE the compute list is open:
        // KernelParams::kp uses `.expect()`, and a panic AFTER compute_list_begin would unwind
        // with an active list and leak the local RD. Every sigma a schedule_<biome> asks for MUST
        // be in that biome's `*_sigmas()` (the per-biome unit tests, e.g.
        // `mountain_sigmas_cover_all_pipeline_blurs` / `grassland_sigmas_cover_all_pipeline_blurs`,
        // guard this); resolving the whole list here proves the in-list lookups cannot fail.
        for &s in &sigmas {
            let _ = kparams.kp(s);
        }

        // ===== record the WHOLE pipeline into ONE compute list, with a barrier after every
        // dependent dispatch (the proven flow_spike pattern). Then submit + sync once. The
        // per-biome dispatch SEQUENCE lives in a standalone `schedule_<biome>()` fn, driven via
        // the Scheduler seam. =====
        let cl = rd.compute_list_begin();
        rd.compute_list_bind_compute_pipeline(cl, pipeline);

        // Build the Scheduler over the open list, then run the selected biome's schedule. The
        // schedule fns own the dispatch SEQUENCE (byte-identical to the old inline schedule).
        let mut sched = Scheduler {
            rd: &mut rd,
            cl,
            uset,
            rows: rows as i32,
            cols: cols as i32,
            apron: apron as i32,
            seed,
            spacing,
            ox,
            oz,
            feature_span_m,
            vent_count: vent_count as i32,
            favor_strength: 0.0,           // biome path never composes -> 0 (byte-identical push)
            relief_confidence_floor: 0.0,
            relief_m: 0.0,                 // readback harness crops to BUFFER (PASS_CROP, ignores pad2)
            wg_full_x,
            wg_full_y,
            wg_core_x,
            wg_core_y,
            kparams,
            flow_iters,
            // READBACK harness: ALWAYS flow_on (the parity-frozen sequence the 576/biome gates prove
            // against the flow-ON oracle). The flow_on=false coarse-level path is exercised ONLY via
            // the runtime producer (compute_biome_page_cached) + the pool.
            flow_on: true,
        };
        // Biome selector (derived from the fragment path stem in generate_core_page). Each biome
        // adds a `schedule_<name>()` + one match arm here + a `*_sigmas()` arm in `biome_sigmas`.
        match biome {
            "mountain" => schedule_mountain(&mut sched),
            "grassland" => schedule_grassland(&mut sched),
            "desert" => schedule_desert(&mut sched),
            "coast" => schedule_coast(&mut sched),
            "wetland" => schedule_wetland(&mut sched),
            "tundra" => schedule_tundra(&mut sched),
            "glacial" => schedule_glacial(&mut sched),
            "karst" => schedule_karst(&mut sched),
            "temperate" => schedule_temperate(&mut sched),
            "rainforest" => schedule_rainforest(&mut sched),
            "volcanic" => schedule_volcanic(&mut sched),
            other => {
                // drop the Scheduler's &mut borrow before freeing the RD.
                let _ = sched;
                rd.compute_list_end();
                rd.submit();
                rd.sync();
                for (_, rid) in bindings.iter() {
                    rd.free_rid(*rid);
                }
                rd.free_rid(pipeline);
                rd.free_rid(shader);
                rd.free();
                return Err(format!("no schedule for biome '{other}'"));
            }
        }

        rd.compute_list_end();
        rd.submit();
        rd.sync();

        // --- read back the core ---
        let core_out_pba = rd.buffer_get_data(b_core);
        let core = bytes_to_f32s(&core_out_pba.to_vec());

        // --- free everything ---
        for (_, rid) in bindings.iter() {
            rd.free_rid(*rid);
        }
        rd.free_rid(pipeline);
        rd.free_rid(shader); // cascades the uniform set
        rd.free();

        if core.len() != core_n {
            return Err(format!("core readback: expected {core_n} f32, got {}", core.len()));
        }
        Ok(core)
    }
}
