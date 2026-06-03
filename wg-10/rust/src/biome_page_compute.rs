//! WorldGen10 Slice-4a: GPU apron PAGE pipeline for the MOUNTAIN seam-safe recipe.
//!
//! `Wg10BiomePageCompute` mirrors `recipes.rs::mountain::generate_seamsafe` (the f64 parity
//! ORACLE) as a MULTI-DISPATCH GPU pipeline. Slice-4b concat-selection: it concatenates three
//! GLSL parts -- `recipe_primitives.glsl` (proven f32 noise/warp leaves) + `biome_page.glsl`
//! (the GENERIC pass machine: bindings, leaf helpers, generic passes + main()) + the selected
//! per-biome FRAGMENT `biome_<name>.glsl` (the biome-specific `biome_pass()` body) -- compiles
//! one compute shader per biome, and dispatches it once per pass with a different `pass`
//! push-constant. The primitives + machine are the STABLE two parts (loaded once via
//! `load_shaders`); the fragment is selected + concatenated per `generate_core_page` call.
//!
//! The whole-field operators become their own passes:
//!   * gaussian = separable (COPY src -> gauss_in, AXIS0 down rows, AXIS1 across cols),
//!     with the 1-D kernel built CPU-side (a port of `array_ops::gaussian_kernel1d`) and
//!     uploaded via `buffer_update` per distinct sigma (clamp-to-edge 'nearest', truncate
//!     4.0, radius int(truncate*sigma+0.5), normalized) -> EXACTLY array_ops.
//!   * flow accumulation = the PULL relaxation from `flow_accum_spike.glsl`, K=STABLE_ITERS
//!     ping-pong steps (an APPROXIMATION of the CPU sorted sweep; spec 4 Tier-2).
//!
//! Mirrors `primitive_probe.rs`/`flow_spike.rs` for the godot RenderingDevice API
//! (concat+strip+compile, storage buffers, uniform set, compute_list, submit/sync,
//! buffer_get_data, free + rd.free()). Readback happens ONLY in the `generate_core_page`
//! TEST entry (never the render path). WINDOWED only (local RD is null headless on this box).

use godot::prelude::*;
use godot::classes::{
    RenderingServer, RdShaderSource, RdUniform, RenderingDevice,
    RdTextureFormat, RdTextureView,
    rendering_device::{UniformType, ShaderStage, DataFormat, TextureUsageBits},
};

mod abi;
mod kernels;
mod sigma_registry;

use abi::*;
pub(crate) use kernels::*;
pub(crate) use sigma_registry::*;

// ---------------------------------------------------------------------------
// byte helpers
// ---------------------------------------------------------------------------

fn f32s_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for &x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

fn bytes_to_f32s(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

/// f32 slice -> PackedFloat64Array (the GPU result widened back to f64 for the GDScript caller).
fn f32s_to_packed_f64(v: &[f32]) -> PackedFloat64Array {
    let mut out = PackedFloat64Array::new();
    out.resize(v.len());
    let sl = out.as_mut_slice();
    for i in 0..v.len() {
        sl[i] = v[i] as f64;
    }
    out
}

/// Biome selector from a fragment path: the file stem with a leading `biome_` stripped.
/// e.g. ".../biome_mountain.glsl" -> "mountain", ".../biome_grassland.glsl" -> "grassland".
/// Falls back to the bare stem (then the whole string) if the conventions don't match, so the
/// `run_inner` match arm reports a precise "no schedule for biome '<x>'" error.
fn biome_stem(path: &str) -> String {
    let file = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path);
    let stem = file.strip_suffix(".glsl").unwrap_or(file);
    stem.strip_prefix("biome_").unwrap_or(stem).to_string()
}

fn make_storage_uniform(binding: i32, rid: Rid) -> Gd<RdUniform> {
    let mut u = RdUniform::new_gd();
    u.set_uniform_type(UniformType::STORAGE_BUFFER);
    u.set_binding(binding);
    u.add_id(rid);
    u
}

/// Image-binding RdUniform (the RUNTIME producer binds the caller's R32F page texture at
/// binding 41 via this). Same shape as page_compute.rs::make_image_uniform (replicated here --
/// 6 lines -- rather than exposing that module-private helper cross-module).
fn make_image_uniform(binding: i32, rid: Rid) -> Gd<RdUniform> {
    let mut u = RdUniform::new_gd();
    u.set_uniform_type(UniformType::IMAGE);
    u.set_binding(binding);
    u.add_id(rid);
    u
}

/// Allocate a 1x1 R32F STORAGE image. The biome_page machine now DECLARES (and statically uses,
/// in the PASS_CROP_IMG branch) a write-only image2D at binding 41 for the RUNTIME producer.
/// Godot's `uniform_set_create` validates a set against EVERY binding the shader statically uses,
/// so the THREE readback-only paths (run_inner / run_compose_engine / run_blend_inner) -- which
/// never dispatch PASS_CROP_IMG -- must still BIND something at 41 or the set is rejected. They
/// bind this throwaway 1x1 image: it is NEVER written (none of their passes touch out_img), so
/// every readback (core_out / height) is byte-identical to before the PASS_CROP_IMG addition.
/// The caller frees it alongside the other RIDs. (The runtime producer binds the REAL page
/// texture at 41 instead; this scratch is only for the readback harness.)
fn make_scratch_image_1x1(rd: &mut Gd<RenderingDevice>) -> Rid {
    let mut fmt = RdTextureFormat::new_gd();
    fmt.set_width(1);
    fmt.set_height(1);
    fmt.set_format(DataFormat::R32_SFLOAT);
    fmt.set_usage_bits(TextureUsageBits::STORAGE_BIT | TextureUsageBits::CAN_COPY_FROM_BIT);
    let view = RdTextureView::new_gd();
    rd.texture_create(&fmt, &view)
}

/// Build the 96-byte push constant (std430): 12 i32 (48B) then 12 f32 (48B).
/// Layout MUST match biome_page.glsl Params.
///
/// `vent_count` occupies the former `ipad1` int slot (index 10). It is 0 for every non-volcanic
/// biome (the 10 proven biomes pass vent_count=0 -> the exact bytes the hardcoded `0` produced
/// before, so their push is byte-identical). VOLCANIC passes its actual vent count there.
/// Compose params carried in the two leading float PADS (pad0 = favor_strength,
/// pad1 = relief_confidence_floor). 0.0 for every non-compose dispatch -> the bytes are
/// byte-identical to the former all-zero pad block, so the 11 proven biomes are unaffected.
#[allow(clippy::too_many_arguments)]
fn build_push(
    pass: i32,
    rows: i32,
    cols: i32,
    apron_px: i32,
    seed: i32,
    kradius: i32,
    copy_sel: i32,
    flow_dir: i32,
    koffset: i32,
    pool_sel: i32,
    vent_count: i32,
    spacing: f32,
    ox: f32,
    oz: f32,
    feature_span_m: f32,
    flow_power: f32,
    favor_strength: f32,         // -> pad0
    relief_confidence_floor: f32, // -> pad1
    relief_m: f32,                // -> pad2 (RUNTIME crop-to-image height scale; 0 elsewhere)
) -> Vec<u8> {
    let mut b = Vec::with_capacity(96);
    // 12 ints: pass,rows,cols,apron,seed,kradius,copy_sel,flow_dir,koffset,pool_sel,vent_count + 1 pad.
    for v in [pass, rows, cols, apron_px, seed, kradius, copy_sel, flow_dir, koffset, pool_sel, vent_count, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    // 12 floats: spacing,ox,oz,feature_span_m,flow_power, pad0(favor_strength), pad1(relief_conf_floor),
    // pad2(relief_m) + 4 pad.
    for v in [spacing, ox, oz, feature_span_m, flow_power, favor_strength, relief_confidence_floor, relief_m] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for _ in 0..4 {
        b.extend_from_slice(&0.0_f32.to_le_bytes());
    }
    b
}

// ---------------------------------------------------------------------------
// Scheduler: the per-biome dispatch SEAM. Holds all per-dispatch state so a biome's pass
// chain can live in a standalone `schedule_<biome>()` fn (instead of inline closures/macros).
// `run_inner` allocates buffers + opens ONE compute list, builds a Scheduler, then hands it
// to the selected schedule fn. Every future biome adds a `schedule_<biome>()` + one match arm.
//
// IMPORTANT: this is a PURE code-structure seam. `dispatch`/`gauss`/`flow_channels` carry the
// SAME bodies the old `dispatch` closure + `gauss!`/`flow_channels!` macros had; the GPU
// dispatch sequence, push-constant values, STABLE_ITERS loop, and discharge_fd invariant are
// byte-identical to the pre-refactor inline schedule.
// ---------------------------------------------------------------------------

/// Per-dispatch state for one open compute list. Built once `run_inner` has the list open; the
/// schedule fn drives it. `cl` matches the type `compute_list_begin()` returns (i64 in the
/// Godot 4.6 bindings).
pub(crate) struct Scheduler<'a> {
    rd: &'a mut Gd<RenderingDevice>,
    cl: i64,
    uset: Rid,
    rows: i32,
    cols: i32,
    apron: i32,
    seed: i32,
    spacing: f32,
    ox: f32,
    oz: f32,
    feature_span_m: f32,
    /// VOLCANIC: active vents (forwarded into every dispatch's push constant). 0 for the 10
    /// non-volcanic biomes -> their push bytes are byte-identical to the pre-vent layout.
    vent_count: i32,
    /// COMPOSE params, forwarded into every dispatch as pad0/pad1. 0.0/0.0 for the 11 biome
    /// schedules (byte-identical push); the compose schedule sets them from the record's cfg.
    favor_strength: f32,
    relief_confidence_floor: f32,
    /// RUNTIME relief scale (metres): PASS_CROP_IMG multiplies the normalized recipe height by this
    /// before the texture write, so the render shader sees METRES (like legacy `* relief_m`). 0.0 for
    /// the readback test harness (it crops to the BUFFER via PASS_CROP, which ignores pad2 -> the
    /// fixture parity is byte-identical). Set to the configured relief only on the runtime producer.
    relief_m: f32,
    wg_full_x: u32,
    wg_full_y: u32,
    wg_core_x: u32,
    wg_core_y: u32,
    kparams: KernelParams,
    /// Flow PULL-relaxation step count for this run. The recipe page path passes STABLE_ITERS
    /// (so it is byte-identical to the const-loop it replaced); the convergence-measurement entry
    /// (`generate_core_page_iters`) passes a swept value to find the real 576-production
    /// convergence count. Compose/blend engines never run flow -> they set it to STABLE_ITERS too.
    flow_iters: usize,
    /// SCALE-INVARIANCE: enable the drainage carve. `true` reproduces the parity-proven schedule
    /// (both flow_channels passes + the carve run). `false` (coarse clipmap levels) makes
    /// `schedule_mountain` SKIP the two expensive flow_channels passes and instead zero the channel
    /// masks (PASS_ZERO_FLOW_MASKS) -> the MACRO surface, mirroring the CPU oracle's `flow_on==false`
    /// `else` branch. The readback test harnesses (`run_inner` etc.) and every non-mountain schedule
    /// set this `true` so their parity-frozen dispatch sequence is byte-identical to before this flag.
    flow_on: bool,
}

impl<'a> Scheduler<'a> {
    /// One full pass dispatch + trailing barrier (so the next reader sees the writes). Same body
    /// as the old `dispatch` closure, plus the additive `pool_sel` push-constant for the generic
    /// pool passes (0 for every mountain dispatch -> byte-identical to the pre-pool push for
    /// mountain, since pool_sel maps to a former int-pad slot).
    #[allow(clippy::too_many_arguments)]
    fn dispatch(
        &mut self,
        pass: i32,
        kradius: i32,
        koffset: i32,
        copy_sel: i32,
        flow_dir: i32,
        flow_power: f32,
        pool_sel: i32,
        wgx: u32,
        wgy: u32,
    ) {
        self.rd.compute_list_bind_uniform_set(self.cl, self.uset, 0);
        let pc = PackedByteArray::from(
            build_push(
                pass, self.rows, self.cols, self.apron, self.seed, kradius, copy_sel, flow_dir,
                koffset, pool_sel, self.vent_count, self.spacing, self.ox, self.oz,
                self.feature_span_m, flow_power, self.favor_strength, self.relief_confidence_floor,
                self.relief_m,
            )
            .as_slice(),
        );
        self.rd.compute_list_set_push_constant(self.cl, &pc, pc.len() as u32);
        self.rd.compute_list_dispatch(self.cl, wgx, wgy, 1);
        self.rd.compute_list_add_barrier(self.cl);
    }

    /// Full-field dispatch (the overwhelmingly common case): wgx/wgy = full padded dims, and the
    /// no-kernel/no-copy/no-flow/no-pool params default to 0. Convenience wrapper so schedule fns
    /// read cleanly.
    fn dispatch_full(&mut self, pass: i32, copy_sel: i32, flow_dir: i32, flow_power: f32) {
        self.dispatch(pass, 0, 0, copy_sel, flow_dir, flow_power, 0, self.wg_full_x, self.wg_full_y);
    }

    /// Full-field POOL copy/stash dispatch (pool_sel selects the slot). Used by COPY_POOL (slot
    /// -> gauss_in) and POOL_FROM_GAUSS (gauss_out -> slot) so a biome can blur ANY pool slot.
    fn dispatch_pool(&mut self, pass: i32, pool_sel: i32) {
        self.dispatch(pass, 0, 0, 0, 0, 0.0, pool_sel, self.wg_full_x, self.wg_full_y);
    }

    /// gaussian(sigma) on gauss_in -> gauss_out (AXIS0 then AXIS1, packed kernel by koffset).
    /// Same body as the old `gauss!` macro.
    fn gauss(&mut self, sigma: f64) {
        let (ko, kr) = self.kparams.kp(sigma);
        self.dispatch(PASS_GAUSS_AXIS0, kr, ko, 0, 0, 0.0, 0, self.wg_full_x, self.wg_full_y);
        self.dispatch(PASS_GAUSS_AXIS1, kr, ko, 0, 0, 0.0, 0, self.wg_full_x, self.wg_full_y);
    }

    /// Blur a scratch-pool slot in place: COPY_POOL(slot) -> gauss_in, gaussian(sigma), then the
    /// blur lives in gauss_out (the caller reads gauss_out, or POOL_FROM_GAUSS stashes it back).
    fn gauss_pool(&mut self, slot: i32, sigma: f64) {
        self.dispatch_pool(PASS_COPY_POOL, slot);
        self.gauss(sigma);
    }

    /// flow_channels_seam_safe(flow_pre, width_px, power): pre-blur 1.15 -> K relax ->
    /// log1p discharge -> spread gaussian(width). Leaves spread discharge in gauss_out.
    /// Same body as the old `flow_channels!` macro (incl STABLE_ITERS loop + discharge_fd
    /// invariant). Thin wrapper over `flow_channels_ex` with the SHARED pre-blur sigma=1.15
    /// (the 6 proven biomes call THIS -> byte-identical dispatch sequence as before the refactor).
    fn flow_channels(&mut self, power: f32, width: f64) {
        self.flow_channels_ex(power, width, 1.15);
    }

    /// flow_channels_seam_safe with a PARAMETERIZED pre-blur sigma (the machine hook GLACIAL
    /// needs: its troughs pre-blur with sigma=1.85, NOT the shared 1.15). Identical body to the
    /// old `flow_channels` otherwise (pre-blur -> K relax -> log1p discharge -> spread
    /// gaussian(width)). `preblur_sigma` MUST be present in the biome's `*_sigmas()` list so
    /// `kparams` pre-validation covers it (the `gauss(preblur_sigma)` below resolves a kernel slot).
    ///
    /// Now decomposed into `flow_discharge` (the common prefix: pre-blur -> relax -> DISCHARGE,
    /// leaving the raw log1p discharge in gauss_in) + the trailing single spread `gauss(width)`.
    /// This is a PURE refactor: the dispatch sequence for the 8 proven biomes is byte-identical
    /// (flow_discharge emits exactly the same PASS_FLOW_PRE_PREBLUR_IN .. PASS_DISCHARGE dispatches
    /// the old inline body did, in the same order, with the same push-constant values).
    fn flow_channels_ex(&mut self, power: f32, width: f64, preblur_sigma: f64) {
        self.flow_discharge(power, preblur_sigma);
        // spread sigma = max(width, 0.1) (all widths here are >= 0.1)
        self.gauss(width.max(0.1));
    }

    /// flow_channels_seam_safe WITHOUT the trailing spread blur: the common PREFIX of
    /// `flow_channels_ex`, up to AND INCLUDING the PASS_DISCHARGE dispatch (which writes the raw
    /// log1p discharge into gauss_in). It does NOT do the final spread `gauss`.
    ///
    /// TEMPERATE needs this because it spreads the RAW discharge at TWO different sigmas (1.8 for
    /// `valleys`, 4.2 for `broad_valleys`), so it cannot use the single-spread flow_channels_ex.
    /// After `flow_discharge`, the raw discharge lives in gauss_in. The generic gaussian
    /// (`gauss(sigma)`) reads gauss_in (AXIS0 -> gauss_mid) then gauss_mid (AXIS1 -> gauss_out) and
    /// NEVER writes gauss_in, so temperate can call `gauss(1.8)` (read the spread from gauss_out),
    /// then `gauss(4.2)` (which re-reads the SAME intact gauss_in). No pool staging of the raw
    /// discharge is required. `preblur_sigma` MUST be present in the biome's `*_sigmas()` list so
    /// `kparams` pre-validation covers the `gauss(preblur_sigma)` below.
    fn flow_discharge(&mut self, power: f32, preblur_sigma: f64) {
        // pre-blur sigma=preblur_sigma (1.15 for the shared path; 1.85 for glacial)
        self.dispatch_full(PASS_FLOW_PRE_PREBLUR_IN, 0, 0, 0.0);
        self.gauss(preblur_sigma);
        self.dispatch_full(PASS_FLOW_PRE_FROM_GAUSS, 0, 0, 0.0);
        // acc init = 1.0 (both buffers)
        self.dispatch_full(PASS_ACC_INIT, 0, 0, 0.0);
        // K ping-pong relaxation steps. In PASS_FLOW_RELAX, flow_dir selects the WRITE
        // target: fd=0 reads acc_a writes acc_b; fd=1 reads acc_b writes acc_a. The last
        // step is i=STABLE_ITERS-1, fd=(STABLE_ITERS-1)%2, so it writes:
        //   STABLE_ITERS even -> last fd=1 -> final result in acc_a
        //   STABLE_ITERS odd  -> last fd=0 -> final result in acc_b
        let iters = self.flow_iters;
        for i in 0..iters {
            let fd = if i % 2 == 0 { 0 } else { 1 };
            self.dispatch_full(PASS_FLOW_RELAX, 0, fd, power);
        }
        // PASS_DISCHARGE: here flow_dir selects the READ buffer holding the final acc
        // (OPPOSITE of PASS_FLOW_RELAX, where it selects the write target) -> fd=0 reads
        // acc_a, fd=1 reads acc_b. So discharge_fd must equal the parity of the LAST write:
        //   iters odd  -> final in acc_b -> discharge_fd=1
        //   iters even -> final in acc_a -> discharge_fd=0
        // (The recipe page path passes iters=STABLE_ITERS=128 -> byte-identical to the old const loop.)
        let discharge_fd: i32 = if iters % 2 == 1 { 1 } else { 0 };
        debug_assert_eq!(
            discharge_fd,
            1 - ((iters as i32 - 1) % 2),
            "discharge_fd must read the buffer the LAST relax step wrote"
        );
        // raw log1p discharge -> gauss_in (NO trailing spread here; the caller spreads it).
        self.dispatch_full(PASS_DISCHARGE, 0, discharge_fd, 0.0);
    }

    // -------------------------------------------------------------------------
    // COMPOSE layer (Slice-4b.11): bit-close GPU port of biome_compose.rs. The compose buffer
    // roles (see biome_page.glsl): acc=height, acc_w=base, f=pool0, w=pool1, w_acc=lowland,
    // relief_a=range_envelope, relief_b=massif. The caller pre-loads acc(height) and acc_w(base)
    // = fields[0]/weights[0], then for each subsequent (f,w) loads pool0=f, pool1=w, calls
    // compose_step, then accw_add. Standalone blend_field/blend_height_favored load height=a,
    // pool0=b, lowland=w_a and call blend_field_step / blend_favored_step directly.
    // -------------------------------------------------------------------------

    /// Compute relief_a = |acc - gaussian_nearest(acc, 6.0)| into range_envelope. Blurs the
    /// accumulator (height) via COPY_ACC -> gauss_in, gauss(6.0) -> gauss_out, then the abs-diff.
    /// The STORE pass (biome_page.glsl PASS_COMPOSE_RELIEF_A_STORE) snaps pure f32-blur self-noise
    /// to 0 (COMPOSE_RELIEF_F32_FLOOR_REL) so a FLAT field reproduces the f64 oracle's relief==0 --
    /// the root-cause fix for the favored_ramp_flat_flat (rec=4) windowed failure (2.76% / ~13 m):
    /// f32 gaussian(constant) != constant, and signal=total/(total+1e-3) amplified that spurious
    /// relief into a w_adj drift times |a-b|. The snap is inert on structured relief (>> the floor).
    fn compose_relief_a(&mut self) {
        self.dispatch_full(PASS_COMPOSE_COPY_ACC, 0, 0, 0.0);
        self.gauss(COMPOSE_RELIEF_SIGMA);
        self.dispatch_full(PASS_COMPOSE_RELIEF_A_STORE, 0, 0, 0.0);
    }

    /// Compute relief_b = |f - gaussian_nearest(f, 6.0)| into massif. Blurs f (pool0) via
    /// COPY_POOL(0) -> gauss_in, gauss(6.0) -> gauss_out, then the abs-diff.
    fn compose_relief_b(&mut self) {
        self.dispatch_pool(PASS_COPY_POOL, 0);
        self.gauss(COMPOSE_RELIEF_SIGMA);
        self.dispatch_full(PASS_COMPOSE_RELIEF_B_STORE, 0, 0, 0.0);
    }

    /// w_acc = acc_w / (acc_w + w + 1e-12) into lowland (base=acc_w, pool1=w).
    fn compose_wacc(&mut self) {
        self.dispatch_full(PASS_COMPOSE_WACC, 0, 0, 0.0);
    }

    /// FIELD blend step: w_acc in lowland already set. height <- w_acc*height + (1-w_acc)*pool0.
    fn blend_field_step(&mut self) {
        self.dispatch_full(PASS_COMPOSE_BLEND_FIELD, 0, 0, 0.0);
    }

    /// FAVORED blend step: requires relief_a (range_envelope), relief_b (massif), w_acc (lowland)
    /// pre-computed. favor_strength / relief_confidence_floor ride pad0/pad1 (Scheduler fields).
    fn blend_favored_step(&mut self) {
        self.compose_relief_a();
        self.compose_relief_b();
        self.dispatch_full(PASS_COMPOSE_BLEND_FAVORED, 0, 0, 0.0);
    }

    /// acc_w += w  (base += pool1).
    fn compose_accw_add(&mut self) {
        self.dispatch_full(PASS_COMPOSE_ACCW_ADD, 0, 0, 0.0);
    }
}

/// The MOUNTAIN dispatch schedule (style = ALPINE_BRANCHING), `pub(crate)` so the readback test
/// harness (`run_inner`) AND the runtime producer run the SAME proven sequence (DRY -- one
/// schedule, two hosts). Operates on an already-built `Scheduler` bound to the apron buffers; pure
/// dispatch, no rd/buffer ownership.
///
/// EXACTLY the pre-refactor sequence + params; this is the reference pattern every future biome
/// `schedule_<biome>()` copies. The constants here (valley_width_px/trib_width/floor_smooth) mirror
/// `mountain_sigmas()` so the gauss/flow widths resolve to pre-validated kernel slots.
pub(crate) fn schedule_mountain(s: &mut Scheduler) {
    let valley_width_px = 2.4_f64;
    let trib_width = (valley_width_px * 0.42).max(0.6);
    let floor_smooth = 4.0_f64.max(0.2);

    // 0) meshgrid ; 1) pointwise
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);
    s.dispatch_full(PASS_POINTWISE, 0, 0, 0.0);

    // 2) range_envelope = smoothstep(0.24,0.58, gaussian(ranges, 5.0))
    s.dispatch_full(PASS_COPY, CP_RANGES, 0, 0.0);
    s.gauss(5.0);
    s.dispatch_full(PASS_RANGE_ENV, 0, 0, 0.0);

    // 3) lowland: broad_range = gaussian(ranges, 7.0); combine with regional
    s.dispatch_full(PASS_COPY, CP_RANGES, 0, 0.0);
    s.gauss(7.0);
    s.dispatch_full(PASS_LOWLAND, 0, 0, 0.0);

    // 4) massif: gaussian(ranges,1.8) -> massif_inner; then gaussian(massif,2.0) writeback
    s.dispatch_full(PASS_COPY, CP_RANGES, 0, 0.0);
    s.gauss(1.8);
    s.dispatch_full(PASS_MASSIF_INNER, 0, 0, 0.0);
    s.dispatch_full(PASS_COPY, CP_MASSIF, 0, 0.0);
    s.gauss(2.0);
    s.dispatch_full(PASS_MASSIF_WRITEBACK, 0, 0, 0.0);

    // 5) base
    s.dispatch_full(PASS_BASE, 0, 0, 0.0);

    // 6 + 7) primary + tributary channels (flow_on gated). When flow_on==false (coarse clipmap
    // levels) BOTH expensive flow_channels_seam_safe passes are SKIPPED and the two masks are
    // zeroed -> the carve terms in PASS_ASSEMBLE vanish -> the MACRO surface. EXACTLY the CPU
    // oracle's `if flow_on { ... } else { primary_mask = tributary_mask = zeros }` branch.
    if s.flow_on {
        // 6) primary channels: flow_channels_seam_safe(base, valley_width, power=0.48)
        s.dispatch_full(PASS_FLOW_PRE_BASE, 0, 0, 0.0);
        s.flow_channels(0.48_f32, valley_width_px);
        s.dispatch_full(PASS_PRIMARY_MASK, 0, 0, 0.0);

        // 7) tributaries: flow_channels_seam_safe(rough_surface, trib_width, power=0.34)
        s.dispatch_full(PASS_FLOW_PRE_ROUGH, 0, 0, 0.0);
        s.flow_channels(0.34_f32, trib_width);
        s.dispatch_full(PASS_TRIB_MASK, 0, 0, 0.0);
    } else {
        // primary_mask = tributary_mask = 0 (the cached buffers persist across pages -> re-zero).
        s.dispatch_full(PASS_ZERO_FLOW_MASKS, 0, 0, 0.0);
    }

    // 8) high_mask / valley_mask
    s.dispatch_full(PASS_MASKS, 0, 0, 0.0);

    // 9) assemble height
    s.dispatch_full(PASS_ASSEMBLE, 0, 0, 0.0);

    // 10) floor blend
    s.dispatch_full(PASS_COPY, CP_VALLEY, 0, 0.0);
    s.gauss(1.2);
    s.dispatch_full(PASS_FLOOR_MASK, 0, 0, 0.0);
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);
    s.gauss(floor_smooth);
    s.dispatch_full(PASS_FLOOR_BLEND, 0, 0, 0.0);

    // 11) final: height_blur = gaussian(height,1.2); final_blend; affine
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);
    s.gauss(1.2);
    s.dispatch_full(PASS_FINAL, 0, 0, 0.0);

    // 12) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

/// The GRASSLAND dispatch schedule (style = ROLLING_PRAIRIE). Mirrors the field DAG of
/// recipes_grassland.rs::generate_seamsafe ONE-FOR-ONE: warp+macro/secondary -> swells (blur) ->
/// pans (blur 1-swells) -> sandhills/escarpments (whole-field sub-pipelines) -> base_for_flow ->
/// draws (flow channels) -> fine_grain/low_ripple -> assemble -> floor blend -> final. All
/// intermediate fields live in the GENERIC scratch POOL (pool0..pool11; see biome_grassland.glsl
/// for the slot map). The sigmas (smoothing_px=3.7, 5.2, 1.55, 1.4, flow pre-blur 1.15 + spread
/// 2.1, floor 3.7, final 1.1) are all in grassland_sigmas(). This is the PATTERN the other 9 ports
/// copy: pointwise passes write pool slots; blur a slot via gauss_pool(slot,sigma) then read
/// gauss_out (or POOL_FROM_GAUSS to stash); flow channels reuse the proven flow_channels().
fn schedule_grassland(s: &mut Scheduler) {
    let smoothing_px = 3.7_f64;          // ROLLING_PRAIRIE.smoothing_px
    let floor_smooth = smoothing_px.max(0.5);

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; macro_f=pool2 ; secondary=pool3
    s.dispatch_full(GL_POINTWISE, 0, 0, 0.0);

    // 2) swells = clip(affine(gaussian(0.74*macro + 0.26*secondary, smoothing_px), SWELLS))
    s.dispatch_full(GL_COMBO, 0, 0, 0.0);   // gauss_in <- combo
    s.gauss(smoothing_px);                  // gauss_out = gaussian(combo, smoothing_px)
    s.dispatch_full(GL_SWELLS, 0, 0, 0.0);  // pool4 = swells

    // 3) pans = smoothstep(0.54,0.88, gaussian(1 - swells, 5.2))
    s.dispatch_full(GL_ONE_MINUS_SWELLS, 0, 0, 0.0); // gauss_in <- 1 - swells
    s.gauss(5.2);                                    // gauss_out = gaussian(1-swells, 5.2)
    s.dispatch_full(GL_PANS, 0, 0, 0.0);             // pool5 = pans

    // 4) sandhills sub-pipeline: pre (pool11) -> gaussian(1.55) -> clip(affine(., SH_FINAL)) = pool6
    s.dispatch_full(GL_SANDHILL_PRE, 0, 0, 0.0);     // pool11 = softened*envelope*broken
    s.gauss_pool(11, 1.55);                          // gauss_out = gaussian(pool11, 1.55)
    s.dispatch_full(GL_SANDHILL_FINAL, 0, 0, 0.0);   // pool6 = sandhills

    // 5) escarpments sub-pipeline: edge (pool11) -> gaussian(1.4) -> clip(affine(., ESC_FINAL)) = pool7
    s.dispatch_full(GL_ESC_PRE, 0, 0, 0.0);          // pool11 = smoothstep(|bands|)*plateau
    s.gauss_pool(11, 1.4);                           // gauss_out = gaussian(pool11, 1.4)
    s.dispatch_full(GL_ESC_FINAL, 0, 0, 0.0);        // pool7 = escarpments

    // 6) base_for_flow = affine(0.82*swells + 0.28*esc - 0.34*pans, BASE_FLOW) (NO clip) -> flow_pre
    s.dispatch_full(GL_BASE_FOR_FLOW, 0, 0, 0.0);

    // 7) draws = smoothstep(0.60,0.94, flow_channels(base_for_flow, width=2.1, power=0.50))
    //            * (0.42 + 0.58*(1 - pans))    [flow_channels leaves spread discharge in gauss_out]
    s.flow_channels(0.50_f32, 2.1);
    s.dispatch_full(GL_DRAWS, 0, 0, 0.0);            // pool8 = draws

    // 8) texture: fine_grain (pool9) + low_ripple (pool10), rotated angle+1.10 on w_x/w_z
    s.dispatch_full(GL_TEXTURE, 0, 0, 0.0);

    // 9) assemble height (swells/sandhills/escarpments/pans/draws/texture weighted sum)
    s.dispatch_full(GL_ASSEMBLE, 0, 0, 0.0);

    // 10) floor blend: smooth = gaussian(height, max(smoothing_px,0.5)); open_floor blend
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(floor_smooth);                           // gauss_out = gaussian(height, floor_smooth)
    s.dispatch_full(GL_OPEN_FLOOR_BLEND, 0, 0, 0.0);

    // 11) final: height_blur = gaussian(height, 1.1); final_blend = 0.86*h + 0.14*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(1.1);                                    // gauss_out = gaussian(height, 1.1)
    s.dispatch_full(GL_FINAL, 0, 0, 0.0);

    // 12) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

/// The DESERT dispatch schedule (style = DUNE_SEA). Mirrors the field DAG of
/// recipes_desert.rs::generate_seamsafe ONE-FOR-ONE: warp+regional -> basin (blur 1-regional) ->
/// playa (blur basin) -> dunes (whole-field sub-pipeline) -> yardangs (pointwise) ->
/// block_cores/mesas -> base_surface -> washes (flow channels) -> fine/salt -> assemble ->
/// floor blend -> final. All intermediate fields live in the GENERIC scratch POOL (pool0..pool15;
/// see biome_desert.glsl for the slot map). The sigmas (6.2, 5.0, 0.70, 3.2, 2.2, flow pre-blur
/// 1.15 + spread 1.8, floor 5.2, final 0.95) are all in desert_sigmas(). Same PATTERN as
/// schedule_grassland: pointwise passes write pool slots; blur a slot via gauss_pool(slot,sigma)
/// then read gauss_out; flow channels reuse the proven flow_channels().
fn schedule_desert(s: &mut Scheduler) {
    let floor_smooth = 5.2_f64.max(0.2);

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; regional=pool2
    s.dispatch_full(DS_POINTWISE, 0, 0, 0.0);

    // 2) basin = smoothstep(0.34,0.78, 1 - gaussian(regional, 6.2))
    s.gauss_pool(2, 6.2);                            // gauss_out = gaussian(regional, 6.2)
    s.dispatch_full(DS_BASIN, 0, 0, 0.0);            // pool3 = basin

    // 3) playa = smoothstep(0.56,0.90, gaussian(basin, 5.0))
    s.gauss_pool(3, 5.0);                            // gauss_out = gaussian(basin, 5.0)
    s.dispatch_full(DS_PLAYA, 0, 0, 0.0);            // pool4 = playa

    // 4) dunes sub-pipeline: raw (pool15) -> gaussian(0.70) -> clip(affine(., DUNE)) = pool5
    s.dispatch_full(DS_DUNE_PRE, 0, 0, 0.0);         // pool15 = dune raw
    s.gauss_pool(15, 0.70);                          // gauss_out = gaussian(pool15, 0.70)
    s.dispatch_full(DS_DUNE_FINAL, 0, 0, 0.0);       // pool5 = dunes

    // 5) yardangs (pointwise, no blur) = pool6
    s.dispatch_full(DS_YARDANG, 0, 0, 0.0);

    // 6) block_cores: pre (pool12=1-block_edges, pool13=rocky_relief) -> gaussian(3.2) -> pool14
    s.dispatch_full(DS_BLOCK_PRE, 0, 0, 0.0);        // pool12 = 1-block_edges ; pool13 = rocky_relief
    s.gauss_pool(12, 3.2);                           // gauss_out = gaussian(1-block_edges, 3.2)
    s.dispatch_full(DS_BLOCK_CORES, 0, 0, 0.0);      // pool14 = block_cores

    // 7) mesas = clip(0.68*mesa_blocks + 0.32*rocky_relief*(1-0.42*basin)); mesa_blocks uses
    //    gaussian(regional, 2.2) * block_cores * (1-0.68*basin)
    s.gauss_pool(2, 2.2);                            // gauss_out = gaussian(regional, 2.2)
    s.dispatch_full(DS_MESAS, 0, 0, 0.0);            // pool7 = mesas

    // 8) base_surface = affine(0.72*regional + 0.24*mesas - 0.62*basin, BASE) = pool8
    s.dispatch_full(DS_BASE, 0, 0, 0.0);

    // 9) washes = smoothstep(0.57,0.94, flow_channels(base_surface+0.16*mesas, width=1.8,
    //    power=0.43)) * (0.35 + 0.65*(1 - playa))    [flow_channels leaves spread in gauss_out]
    s.dispatch_full(DS_WASH_FLOW_PRE, 0, 0, 0.0);    // flow_pre <- base_surface + 0.16*mesas
    s.flow_channels(0.43_f32, 1.8);
    s.dispatch_full(DS_WASH_FINAL, 0, 0, 0.0);       // pool9 = washes

    // 10) fine (pool10) + salt (pool11), pointwise on w_x/w_z
    s.dispatch_full(DS_FINE_SALT, 0, 0, 0.0);

    // 11) assemble height (base + dune/yardang/wash/playa/mesa relief + detail)
    s.dispatch_full(DS_ASSEMBLE, 0, 0, 0.0);

    // 12) floor blend: smooth_floor = gaussian(height, max(floor_smooth_px,0.2)=5.2); floor blend
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(floor_smooth);                           // gauss_out = gaussian(height, floor_smooth)
    s.dispatch_full(DS_FLOOR_BLEND, 0, 0, 0.0);

    // 13) final: height_blur = gaussian(height, 0.95); final_blend = 0.82*h + 0.18*blur; affine
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(0.95);                                   // gauss_out = gaussian(height, 0.95)
    s.dispatch_full(DS_FINAL, 0, 0, 0.0);

    // 14) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

/// The COAST dispatch schedule (style = CLIFFED_HEADLANDS). Mirrors the field DAG of
/// recipes_coast.rs::generate_seamsafe ONE-FOR-ONE: rotation+warp pointwise (rx/rz/w_x/w_z + the
/// sea/land/nearshore/shelf/inland/headlands/scarp masks + ridge_source) -> channels (flow on
/// ridge_source) -> channel_relief (fjords + grooves) -> islands (cellular_edges seed blurred) ->
/// assemble (texture/sea_floor computed inline) -> sea-smoothing blend -> final. All intermediate
/// fields live in the GENERIC scratch POOL (pool0..pool15; see biome_coast.glsl for the slot map).
/// pool12 is REUSED: it holds ridge_source (consumed by the flow pass) then stages islands_seed.
/// The sigmas (flow pre-blur 1.15 + spread 1.9, islands 2.0, sea 3.0, final 0.9) are all in
/// coast_sigmas(). Same PATTERN as schedule_grassland/desert: pointwise passes write pool slots;
/// blur a slot via gauss_pool(slot,sigma) then read gauss_out; flow channels reuse flow_channels().
fn schedule_coast(s: &mut Scheduler) {
    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: rotation -> pool0=rx, pool1=rz ; warp -> pool2=w_x, pool3=w_z ;
    //    signed=pool4 ; sea=pool5 ; land=pool6 ; nearshore=pool7 ; shelf=pool8 ;
    //    inland_raw=pool9 ; headlands=pool10 ; scarp=pool11 ; ridge_source=pool12
    s.dispatch_full(CO_POINTWISE, 0, 0, 0.0);

    // 2) channels = smoothstep(0.53,0.94->0.92, flow_channels(ridge_source, width=1.9, power=0.47))
    //    * land    [flow_channels leaves spread discharge in gauss_out]
    s.dispatch_full(CO_FLOW_PRE, 0, 0, 0.0);         // flow_pre <- ridge_source (pool12)
    s.flow_channels(0.47_f32, 1.9);
    s.dispatch_full(CO_CHANNELS, 0, 0, 0.0);         // pool13 = channels

    // 3) channel_relief = clip(channels + fjords + fjord_grooves combo) (pointwise)
    s.dispatch_full(CO_CHANNEL_RELIEF, 0, 0, 0.0);   // pool14 = channel_relief

    // 4) islands sub-pipeline: islands_seed (pool12, reused) -> gaussian(2.0) ->
    //    smoothstep(0.50,0.86)*sea*smoothstep(...) = pool15
    s.dispatch_full(CO_ISLANDS_SEED, 0, 0, 0.0);     // pool12 = islands_seed (cellular_edges)
    s.gauss_pool(12, 2.0);                           // gauss_out = gaussian(islands_seed, 2.0)
    s.dispatch_full(CO_ISLANDS, 0, 0, 0.0);          // pool15 = islands

    // 5) assemble height (land*land_height + sea*sea_floor + islands - shelf; texture/sea_floor
    //    computed inline in the pass from w_x/w_z)
    s.dispatch_full(CO_ASSEMBLE, 0, 0, 0.0);

    // 6) sea-smoothing blend: smoothed_sea = gaussian(height, 3.0); sea-weighted blend
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(3.0);                                    // gauss_out = gaussian(height, 3.0)
    s.dispatch_full(CO_SEA_BLEND, 0, 0, 0.0);

    // 7) final: height_blur = gaussian(height, 0.9); final_blend = 0.86*h + 0.14*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(0.9);                                    // gauss_out = gaussian(height, 0.9)
    s.dispatch_full(CO_FINAL, 0, 0, 0.0);

    // 8) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

/// The WETLAND dispatch schedule (style = delta_distributary). Mirrors the field DAG of
/// recipes_wetland.rs::generate_seamsafe ONE-FOR-ONE: warp+macro/micro/meander -> basin (blur
/// 1-macro) -> floodplain (blur 1-|macro-0.42|) -> channels (meander*floodplain) -> fine_flow
/// (flow channels on flow_input) -> channels reassigned -> levees (DoG of channels) -> flat_base
/// (blur of affine combo) -> assemble -> final. All intermediate fields live in the GENERIC
/// scratch POOL (pool0..pool10; see biome_wetland.glsl for the slot map). pool8 is TRANSIENT
/// (stages gaussian(channels,2.2) for the levee DoG). The sigmas (5.8, 5.2, flow pre-blur 1.15 +
/// spread 1.8, 2.2, smoothing_px=4.4, final 1.2) are all in wetland_sigmas(). Same PATTERN as
/// schedule_grassland/desert/coast: pointwise passes write pool slots; blur a slot via
/// gauss_pool(slot,sigma) then read gauss_out (or POOL_FROM_GAUSS to stash); flow channels reuse
/// the proven flow_channels().
fn schedule_wetland(s: &mut Scheduler) {
    let smoothing_px = 4.4_f64;          // delta_distributary.smoothing_px (flat_base blur)

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; macro_f=pool2 ; micro=pool3 ; meander=pool4
    s.dispatch_full(WL_POINTWISE, 0, 0, 0.0);

    // 2) basin = smoothstep(0.48,0.86, gaussian(1 - macro, 5.8))
    s.dispatch_full(WL_ONE_MINUS_MACRO, 0, 0, 0.0); // gauss_in <- 1 - macro_f
    s.gauss(5.8);                                    // gauss_out = gaussian(1-macro, 5.8)
    s.dispatch_full(WL_BASIN, 0, 0, 0.0);            // pool5 = basin

    // 3) floodplain = smoothstep(0.36,0.78, gaussian(1 - |macro-0.42|, 5.2))
    s.dispatch_full(WL_FLOODPLAIN_PRE, 0, 0, 0.0);   // gauss_in <- 1 - |macro_f - 0.42|
    s.gauss(5.2);                                    // gauss_out = gaussian(., 5.2)
    s.dispatch_full(WL_FLOODPLAIN, 0, 0, 0.0);       // pool6 = floodplain

    // 4) channels = meander * floodplain (first assignment)
    s.dispatch_full(WL_CHANNELS_FIRST, 0, 0, 0.0);   // pool7 = channels

    // 5) fine_flow: flow_input = affine(macro - 0.34*basin, FLOW_INPUT) -> flow_pre ;
    //    fine_flow = flow_channels_seam_safe(flow_input, width=1.8, power=0.44) ; channels reassigned
    s.dispatch_full(WL_FLOW_PRE, 0, 0, 0.0);         // flow_pre <- flow_input (NO clip)
    s.flow_channels(0.44_f32, 1.8);                  // gauss_out = spread discharge
    s.dispatch_full(WL_CHANNELS_FLOW, 0, 0, 0.0);    // pool7 = clip(0.68*channels + 0.50*ss(fine_flow))

    // 6) levees = smoothstep(0.02,0.18, gaussian(channels,2.2) - gaussian(channels,5.2))
    //             * (1 - smoothstep(0.42,0.86, channels))
    // stash gaussian(channels,2.2) into pool8 (transient), then compute gaussian(channels,5.2)
    // into gauss_out so WL_LEVEES has BOTH blurs live (pool8 = blur22, gauss_out = blur52).
    s.gauss_pool(7, 2.2);                            // gauss_out = gaussian(channels, 2.2)
    s.dispatch_pool(PASS_POOL_FROM_GAUSS, 8);        // pool8 = gaussian(channels, 2.2)
    s.gauss_pool(7, 5.2);                            // gauss_out = gaussian(channels, 5.2)
    s.dispatch_full(WL_LEVEES, 0, 0, 0.0);           // pool9 = levees

    // 7) flat_base = gaussian(affine(0.42*macro - 0.58*basin + 0.20*floodplain, FLAT_BASE), smoothing_px)
    s.dispatch_full(WL_FLAT_BASE_PRE, 0, 0, 0.0);    // pool10 = flat_base_inner
    s.gauss_pool(10, smoothing_px);                  // gauss_out = gaussian(pool10, smoothing_px)
    s.dispatch_pool(PASS_POOL_FROM_GAUSS, 10);       // pool10 = flat_base

    // 8) assemble height (macro/basin/floodplain/channels/levees/micro + flat_base blend)
    s.dispatch_full(WL_ASSEMBLE, 0, 0, 0.0);

    // 9) final: height_blur = gaussian(height, 1.2); final_blend = 0.88*h + 0.12*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(1.2);                                    // gauss_out = gaussian(height, 1.2)
    s.dispatch_full(WL_FINAL, 0, 0, 0.0);

    // 10) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

/// The TUNDRA dispatch schedule (style = arctic_plain). Mirrors the field DAG of
/// recipes_tundra.rs::generate_seamsafe ONE-FOR-ONE: warp+macro/polygons/stripes/fringe_ridges/
/// foothills/fine -> plain (blur 1-|macro-0.46|) -> pattern (blur 0.56*polygons+0.44*stripes, then
/// *plain) -> fringe (blur fringe_ridges) -> flow_source -> drainage (flow channels) -> base (blur
/// of affine combo) -> assemble -> final. All intermediate fields live in the GENERIC scratch POOL
/// (pool0..pool12; see biome_tundra.glsl for the slot map). The sigmas (5.8, 1.2, 1.8, flow
/// pre-blur 1.15 + spread 2.0, smoothing_px=5.0, final 1.1) are all in tundra_sigmas(). Same
/// PATTERN as schedule_grassland/desert/coast/wetland: pointwise passes write pool slots; blur a
/// slot via gauss_pool(slot,sigma) then read gauss_out (or POOL_FROM_GAUSS to stash); flow channels
/// reuse the proven flow_channels().
fn schedule_tundra(s: &mut Scheduler) {
    let smoothing_px = 5.0_f64;          // arctic_plain.smoothing_px (base blur)

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; macro=pool2 ; polygons=pool3 ; stripes=pool4 ;
    //    fringe_ridges=pool5 ; foothills=pool6 ; fine=pool7
    s.dispatch_full(TU_POINTWISE, 0, 0, 0.0);

    // 2) plain = smoothstep(0.36,0.76, gaussian(1 - |macro - 0.46|, 5.8))
    s.dispatch_full(TU_PLAIN_PRE, 0, 0, 0.0);        // gauss_in <- 1 - |macro - 0.46|
    s.gauss(5.8);                                    // gauss_out = gaussian(., 5.8)
    s.dispatch_full(TU_PLAIN, 0, 0, 0.0);            // pool8 = plain

    // 3) pattern = smoothstep(0.46,0.86, gaussian(0.56*polygons + 0.44*stripes, 1.2)) * plain
    s.dispatch_full(TU_PATTERN_PRE, 0, 0, 0.0);      // gauss_in <- 0.56*polygons + 0.44*stripes
    s.gauss(1.2);                                    // gauss_out = gaussian(., 1.2)
    s.dispatch_full(TU_PATTERN, 0, 0, 0.0);          // pool9 = pattern

    // 4) fringe = smoothstep(0.42,0.84, gaussian(fringe_ridges, 1.8))
    s.gauss_pool(5, 1.8);                            // gauss_out = gaussian(fringe_ridges, 1.8)
    s.dispatch_full(TU_FRINGE, 0, 0, 0.0);           // pool10 = fringe

    // 5) drainage: flow_source = affine(0.62*macro+0.26*foothills+0.22*fringe-0.22*plain,
    //    FLOW_SOURCE) -> flow_pre ; channels = flow_channels_seam_safe(flow_source, width=2.0,
    //    power=0.48) ; drainage = smoothstep(0.58,0.94, channels)
    s.dispatch_full(TU_FLOW_PRE, 0, 0, 0.0);         // flow_pre <- flow_source (NO clip)
    s.flow_channels(0.48_f32, 2.0);                  // gauss_out = spread discharge
    s.dispatch_full(TU_DRAINAGE, 0, 0, 0.0);         // pool11 = drainage

    // 6) base = gaussian(affine(0.74*macro + 0.26*foothills, BASE), smoothing_px)
    s.dispatch_full(TU_BASE_PRE, 0, 0, 0.0);         // pool12 = base_inner
    s.gauss_pool(12, smoothing_px);                  // gauss_out = gaussian(pool12, smoothing_px)
    s.dispatch_pool(PASS_POOL_FROM_GAUSS, 12);       // pool12 = base

    // 7) assemble height (macro_zsc/pattern/fringe/foothills/drainage/fine + base blend)
    s.dispatch_full(TU_ASSEMBLE, 0, 0, 0.0);

    // 8) final: height_blur = gaussian(height, 1.1); final_blend = 0.86*h + 0.14*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(1.1);                                    // gauss_out = gaussian(height, 1.1)
    s.dispatch_full(TU_FINAL, 0, 0, 0.0);

    // 9) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

/// The GLACIAL dispatch schedule (style = fjorded_troughs). Mirrors the field DAG of
/// recipes_glacial.rs::generate_seamsafe ONE-FOR-ONE: warp+regional/ridge_detail/close_detail ->
/// oriented_relief (raw -> blur 1.25) -> relief_env (blur 5.8) -> icefield (blur 7.0) -> massif
/// (raw -> blur 2.8) -> base -> flow_primary (TROUGH flow, pre-blur 1.85) -> axial (raw -> blur
/// 1.224) -> primary_mask -> branch_surface (uses gaussian(primary_mask,1.6)) -> tributary (TROUGH
/// flow, pre-blur 1.85) + trib_mask -> scrapes -> assemble -> floor/ice masks + blends -> final.
/// All intermediate fields live in the GENERIC scratch POOL (pool0..pool15; pool15 is the transient
/// pre-blur staging slot; pool10/pool11/pool7 are REUSED post-mask; see biome_glacial.glsl for the
/// slot map). GLACIAL DIVERGENCE: its trough flow uses flow_channels_ex(power, width, 1.85) (the
/// machine-hook), NOT the shared flow_channels (1.15) -- 1.85 is in glacial_sigmas(). The sigmas
/// (1.224, 1.25, 1.35, 1.6, 1.85, 2.8, 3.264, 4.03, 5.8, 6.2, 6.8, 7.0) are all in glacial_sigmas().
/// Same PATTERN as schedule_tundra: pointwise passes write pool slots; blur a slot via
/// gauss_pool(slot,sigma) then read gauss_out (or POOL_FROM_GAUSS to stash); flow channels reuse
/// the proven flow_channels_ex().
fn schedule_glacial(s: &mut Scheduler) {
    let trough_width_px = 6.8_f64;
    let axial_sigma = (trough_width_px * 0.18).max(0.8);    // 1.224
    let primary_spread = trough_width_px;                    // 6.8 (flow_channels width.max(0.1)=6.8)
    let trib_width = (trough_width_px * 0.48).max(0.8);      // 3.264
    let ice_smooth_px = 6.2_f64;
    let floor_smooth = ice_smooth_px.max(0.2);              // 6.2
    let ice_smooth = (ice_smooth_px * 0.65).max(0.2);       // 4.03

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; regional=pool2 ; ridge_detail=pool3 ; close_detail=pool4
    s.dispatch_full(GC_POINTWISE, 0, 0, 0.0);

    // 2) relief = gaussian(oriented_relief raw, 1.25)
    s.dispatch_full(GC_RELIEF_RAW, 0, 0, 0.0);       // pool15 = oriented_relief raw
    s.gauss_pool(15, 1.25);                          // gauss_out = gaussian(pool15, 1.25)
    s.dispatch_full(GC_RELIEF, 0, 0, 0.0);           // pool5 = relief

    // 3) relief_env = smoothstep(0.22,0.62, gaussian(relief, 5.8))
    s.gauss_pool(5, 5.8);                            // gauss_out = gaussian(relief, 5.8)
    s.dispatch_full(GC_RELIEF_ENV, 0, 0, 0.0);       // pool6 = relief_env

    // 4) icefield = smoothstep(0.48,0.78, gaussian(0.56*regional + 0.44*relief_env, 7.0))
    s.dispatch_full(GC_ICE_INNER, 0, 0, 0.0);        // gauss_in <- ice_inner
    s.gauss(7.0);                                    // gauss_out = gaussian(ice_inner, 7.0)
    s.dispatch_full(GC_ICEFIELD, 0, 0, 0.0);         // pool7 = icefield

    // 5) massif = gaussian(massif_inner, 2.8)
    s.dispatch_full(GC_MASSIF_INNER, 0, 0, 0.0);     // pool15 = massif_inner
    s.gauss_pool(15, 2.8);                           // gauss_out = gaussian(pool15, 2.8)
    s.dispatch_full(GC_MASSIF, 0, 0, 0.0);           // pool8 = massif

    // 6) base = affine(uplift*(1.34*massif + 0.22*relief - 0.16*(1-icefield)), BASE)
    s.dispatch_full(GC_BASE, 0, 0, 0.0);             // pool9 = base

    // 7) flow_primary = trough_channels_seam_safe(base, width=6.8, power=0.58, PRE-BLUR 1.85)
    s.dispatch_full(GC_FLOW_PRE_PRIMARY, 0, 0, 0.0); // flow_pre <- base
    s.flow_channels_ex(0.58_f32, primary_spread, 1.85); // gauss_out = spread discharge (sigma=6.8)
    s.dispatch_full(GC_FLOW_PRIMARY_STASH, 0, 0, 0.0); // pool10 = flow_primary

    // 8) axial = gaussian(axial_troughs raw, max(trough_width_px*0.18, 0.8) = 1.224)
    s.dispatch_full(GC_AXIAL_RAW, 0, 0, 0.0);        // pool15 = axial raw
    s.gauss_pool(15, axial_sigma);                   // gauss_out = gaussian(pool15, 1.224)
    s.dispatch_full(GC_AXIAL, 0, 0, 0.0);            // pool11 = axial

    // 9) primary_mask = smoothstep(0.34,0.84, clip(affine(0.58*flow_primary + 1.18*axial, PRIMARY)))
    s.dispatch_full(GC_PRIMARY_MASK, 0, 0, 0.0);     // pool12 = primary_mask

    // 10) tributary = trough_channels_seam_safe(branch_surface, width=3.264, power=0.36, PRE-BLUR 1.85)
    //     branch_surface = base + 0.10*affine(relief,RELIEF_ZSCORE) - 0.18*gaussian(primary_mask,1.6)
    s.gauss_pool(12, 1.6);                           // gauss_out = gaussian(primary_mask, 1.6)
    s.dispatch_full(GC_BRANCH_SURFACE, 0, 0, 0.0);   // flow_pre <- branch_surface (uses gauss_out)
    s.flow_channels_ex(0.36_f32, trib_width, 1.85);  // gauss_out = spread discharge (sigma=3.264)
    s.dispatch_full(GC_TRIB_MASK, 0, 0, 0.0);        // pool13 = tributary_mask

    // 11) scrapes = striations raw (pointwise, no blur)
    s.dispatch_full(GC_SCRAPES, 0, 0, 0.0);          // pool14 = scrapes

    // 12) assemble height (base + ridge/detail/striation - trough - branch; trough_floor->pool10,
    //     high_ice->pool11)
    s.dispatch_full(GC_ASSEMBLE, 0, 0, 0.0);

    // 13) floor/ice masks: floor_mask = clip(smoothstep(0.36,0.80, gaussian(trough_floor,1.6)));
    //     ice_mask = clip(smoothstep(0.50,0.90, high_ice)) -> pool7
    s.gauss_pool(10, 1.6);                           // gauss_out = gaussian(trough_floor, 1.6)
    s.dispatch_full(GC_FLOOR_MASK, 0, 0, 0.0);       // floor_mask (named buf) ; pool7 = ice_mask

    // 14) floor blend: floor = gaussian(height, max(ice_smooth_px,0.2)=6.2); blend by floor_mask
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(floor_smooth);                           // gauss_out = gaussian(height, 6.2)
    s.dispatch_full(GC_FLOOR_BLEND, 0, 0, 0.0);

    // 15) ice blend: ice_smooth = gaussian(height, max(ice_smooth_px*0.65,0.2)=4.03); blend by
    //     ice_mask; then height -= 0.16*floor_mask
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(ice_smooth);                             // gauss_out = gaussian(height, 4.03)
    s.dispatch_full(GC_ICE_BLEND, 0, 0, 0.0);

    // 16) final: height_blur = gaussian(height, 1.35); final_blend = 0.66*h + 0.34*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(1.35);                                   // gauss_out = gaussian(height, 1.35)
    s.dispatch_full(GC_FINAL, 0, 0, 0.0);

    // 17) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

/// The KARST dispatch schedule (style = tower_karst). Mirrors the field DAG of
/// recipes_karst.rs::generate_seamsafe ONE-FOR-ONE: warp+regional -> plateau (blur 5.8) -> towers
/// (raw sparse_pow -> blur 2.0) -> dolines (raw pits_pow -> blur 2.6) -> lineaments (pointwise) ->
/// cellular (raw -> blur 3.8) -> cockpit_noise (pointwise) -> cockpit (pointwise) -> base ->
/// fine/karren (pointwise; REUSE the dead regional/cellular slots) -> dry_valleys (SHARED flow
/// channels, pre-blur 1.15, spread 2.6) -> masks (tower/cockpit/doline/lineament, tower modulated
/// by doline_mask + dry_valleys) -> assemble -> floor mask + blend -> final. All intermediate
/// fields live in the GENERIC scratch POOL (pool0..pool15; pool15 is the transient blur-staging
/// slot, then REUSED for lineament_mask; pool2/pool7 are REUSED for fine/karren post-base; see
/// biome_karst.glsl for the slot map). KARST uses the PROVEN flow_channels (pre-blur 1.15), NOT the
/// flow_channels_ex hook -- its "custom" flow is just power=0.54, width=2.6 (the spread sigma is the
/// existing width param). The sigmas (0.95, 1.15, 2.0, 2.6, 2.8, 3.8, 5.8) are all in karst_sigmas().
/// Same PATTERN as schedule_desert/glacial: pointwise passes write pool slots; blur a slot via
/// gauss_pool(slot,sigma) then read gauss_out; flow channels reuse the proven flow_channels().
fn schedule_karst(s: &mut Scheduler) {
    let tower_width = 2.0_f64.max(0.2);      // tower_width_px.max(0.2) = 2.0
    let doline_width = 2.6_f64.max(0.2);     // doline_width_px.max(0.2) = 2.6
    let floor_smooth = 2.8_f64.max(0.2);     // floor_smooth_px.max(0.2) = 2.8

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; regional=pool2
    s.dispatch_full(KS_POINTWISE, 0, 0, 0.0);

    // 2) plateau = smoothstep(0.30,0.72, gaussian(regional, 5.8))
    s.gauss_pool(2, 5.8);                            // gauss_out = gaussian(regional, 5.8)
    s.dispatch_full(KS_PLATEAU, 0, 0, 0.0);          // pool3 = plateau

    // 3) towers sub-pipeline: sparse_pow (pool15) -> gaussian(2.0) -> clip(affine(., TOWER_FINAL)) = pool4
    s.dispatch_full(KS_TOWER_PRE, 0, 0, 0.0);        // pool15 = sparse_pow
    s.gauss_pool(15, tower_width);                   // gauss_out = gaussian(pool15, 2.0)
    s.dispatch_full(KS_TOWER_FINAL, 0, 0, 0.0);      // pool4 = towers

    // 4) dolines sub-pipeline: pits_pow (pool15) -> gaussian(2.6) -> clip(affine(., DOLINE_BOWLS)) = pool5
    s.dispatch_full(KS_DOLINE_PRE, 0, 0, 0.0);       // pool15 = pits_pow
    s.gauss_pool(15, doline_width);                  // gauss_out = gaussian(pool15, 2.6)
    s.dispatch_full(KS_DOLINE_FINAL, 0, 0, 0.0);     // pool5 = dolines

    // 5) lineaments (pointwise, no blur) = pool6
    s.dispatch_full(KS_LINEAMENTS, 0, 0, 0.0);

    // 6) cellular = gaussian(cellular_edges raw, 3.8)
    s.dispatch_full(KS_CELLULAR_RAW, 0, 0, 0.0);     // pool15 = cellular_raw
    s.gauss_pool(15, 3.8);                           // gauss_out = gaussian(pool15, 3.8)
    s.dispatch_full(KS_CELLULAR, 0, 0, 0.0);         // pool7 = cellular

    // 7) cockpit_noise (pointwise) = pool8 ; cockpit (pointwise, uses dolines/cellular/cockpit_noise) = pool9
    s.dispatch_full(KS_COCKPIT_NOISE, 0, 0, 0.0);    // pool8 = cockpit_noise
    s.dispatch_full(KS_COCKPIT, 0, 0, 0.0);          // pool9 = cockpit

    // 8) base = affine(plateau_gain*(1.06*plateau + 0.18*regional), BASE) = pool10
    s.dispatch_full(KS_BASE, 0, 0, 0.0);

    // 9) fine/karren (pointwise on w_x/w_z); REUSE pool2 (regional dead) = fine, pool7 (cellular dead) = karren
    s.dispatch_full(KS_FINE_KARREN, 0, 0, 0.0);

    // 10) dry_valleys: flow_pre <- base - 0.30*lineaments - 0.10*dolines ; dry_valleys =
    //     flow_channels(width=2.6, power=0.54) [pre-blur 1.15] ; then smoothstep + scale = pool11
    s.dispatch_full(KS_DV_SURFACE, 0, 0, 0.0);       // flow_pre <- dv_surface (NO clip)
    s.flow_channels(0.54_f32, 2.6);                  // gauss_out = spread discharge (sigma=2.6)
    s.dispatch_full(KS_DV_FINAL, 0, 0, 0.0);         // pool11 = dry_valleys

    // 11) masks: cockpit_mask=pool13, doline_mask=pool14, lineament_mask=pool15 (REUSE),
    //     tower_mask=pool12 (finalized w/ doline_mask + dry_valleys)
    s.dispatch_full(KS_MASKS, 0, 0, 0.0);

    // 12) assemble height (base + tower/lineament relief - cockpit/doline/valley + detail)
    s.dispatch_full(KS_ASSEMBLE, 0, 0, 0.0);

    // 13) floor mask + blend: floor_mask = clip(0.72*doline_mask + 0.56*cockpit_mask + 0.48*dry_valleys);
    //     smoothed_floor = gaussian(height, max(floor_smooth_px,0.2)=2.8); floor blend
    s.dispatch_full(KS_FLOOR_MASK, 0, 0, 0.0);       // floor_mask (named buf)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(floor_smooth);                           // gauss_out = gaussian(height, 2.8)
    s.dispatch_full(KS_FLOOR_BLEND, 0, 0, 0.0);

    // 14) final: height_blur = gaussian(height, 0.95); final_blend = 0.80*h + 0.20*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(0.95);                                   // gauss_out = gaussian(height, 0.95)
    s.dispatch_full(KS_FINAL, 0, 0, 0.0);

    // 15) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

/// The TEMPERATE dispatch schedule (style = appalachian_ridges). Mirrors the field DAG of
/// recipes_temperate.rs::generate_seamsafe ONE-FOR-ONE: warp+macro/folded_remap/hills_raw/fine ->
/// ridges (blur folded_remap 1.1) -> hills (blur hills_raw 2.4) -> upland (blur macro 4.2) ->
/// flow_source -> RAW discharge (flow_discharge, pre-blur 1.15, NO trailing spread) -> valleys
/// (spread discharge 1.8) + broad_valleys (spread the SAME raw discharge 4.2) -> rounded (blur of
/// affine combo, smoothing_px=1.8) -> assemble -> final. All intermediate fields live in the
/// GENERIC scratch POOL (pool0..pool11; see biome_temperate.glsl for the slot map). The sigmas
/// (1.0, 1.1, 1.15, 1.8, 2.4, 4.2) are all in temperate_sigmas().
///
/// TEMPERATE DIVERGENCE (the two-spread crux): temperate's drainage uses `flow_discharge(0.43,
/// 1.15)` (the common PREFIX of flow_channels_ex up to + including PASS_DISCHARGE -- leaving the
/// raw log1p discharge in gauss_in), NOT the single-spread flow_channels/flow_channels_ex the 8
/// proven biomes use. It then spreads that RAW discharge at TWO sigmas: gauss(1.8) -> TE_VALLEYS
/// reads gauss_out (-> pool9), then gauss(4.2) -> TE_BROAD_VALLEYS reads gauss_out (-> pool10). The
/// second gauss(4.2) re-reads the SAME intact gauss_in (the generic gaussian only writes
/// gauss_mid/gauss_out, never gauss_in), so no pool staging of the raw discharge is needed. This is
/// the byte-flow that keeps the 8 proven biomes untouched while temperate gets its dual spread.
fn schedule_temperate(s: &mut Scheduler) {
    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; macro=pool2 ; folded_remap=pool3 ;
    //    hills_raw=pool4 ; fine=pool5
    s.dispatch_full(TE_POINTWISE, 0, 0, 0.0);

    // 2) ridges = smoothstep(0.40,0.82, gaussian(folded_remap, 1.1))
    s.gauss_pool(3, 1.1);                            // gauss_out = gaussian(folded_remap, 1.1)
    s.dispatch_full(TE_RIDGES, 0, 0, 0.0);           // pool6 = ridges

    // 3) hills = clip(affine(gaussian(hills_raw, 2.4), HILLS))
    s.gauss_pool(4, 2.4);                            // gauss_out = gaussian(hills_raw, 2.4)
    s.dispatch_full(TE_HILLS, 0, 0, 0.0);            // pool7 = hills

    // 4) upland = smoothstep(0.50,0.82, gaussian(macro, 4.2))
    s.gauss_pool(2, 4.2);                            // gauss_out = gaussian(macro, 4.2)
    s.dispatch_full(TE_UPLAND, 0, 0, 0.0);           // pool8 = upland

    // 5) flow_source = affine(0.72*macro + 0.32*ridges + 0.28*hills + 0.26*upland, FLOW_SRC) -> flow_pre
    s.dispatch_full(TE_FLOW_PRE, 0, 0, 0.0);         // flow_pre <- flow_source (NO clip)

    // 6) RAW discharge: flow_discharge(power=0.43, pre-blur 1.15) leaves the raw log1p discharge in
    //    gauss_in (NO trailing spread). Then spread it at TWO sigmas (gauss never clobbers gauss_in):
    s.flow_discharge(0.43_f32, 1.15);                // gauss_in = raw log1p discharge
    s.gauss(1.8);                                    // gauss_out = gaussian(discharge, 1.8)
    s.dispatch_full(TE_VALLEYS, 0, 0, 0.0);          // pool9 = valleys (reads gauss_out)
    s.gauss(4.2);                                    // gauss_out = gaussian(discharge, 4.2) (re-reads gauss_in)
    s.dispatch_full(TE_BROAD_VALLEYS, 0, 0, 0.0);    // pool10 = broad_valleys (reads gauss_out)

    // 7) rounded = gaussian(affine(0.52*macro + 0.48*hills, ROUNDED), max(smoothing_px,0.2)=1.8)
    s.dispatch_full(TE_ROUNDED_PRE, 0, 0, 0.0);      // pool11 = rounded_inner
    s.gauss_pool(11, 1.8);                           // gauss_out = gaussian(pool11, 1.8)
    s.dispatch_pool(PASS_POOL_FROM_GAUSS, 11);       // pool11 = rounded

    // 8) assemble height (hills/ridges/upland - valleys/broad_valleys + fine; then 0.76*h + 0.24*rounded)
    s.dispatch_full(TE_ASSEMBLE, 0, 0, 0.0);

    // 9) final: height_blur = gaussian(height, 1.0); final_blend = 0.85*h + 0.15*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(1.0);                                    // gauss_out = gaussian(height, 1.0)
    s.dispatch_full(TE_FINAL, 0, 0, 0.0);

    // 10) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

/// The RAINFOREST dispatch schedule (style = humid_dissected_hills). Mirrors the field DAG of
/// recipes_rainforest.rs::generate_seamsafe ONE-FOR-ONE: warp+macro/plateau_seed/hills_raw/ridges
/// -> hills (blur hills_raw 1.7) -> plateau (blur plateau_seed 4.5, * (1-0.38*ridges)) -> lowland
/// (blur 1-macro 5.4) -> flow_source -> RAW discharge (flow_discharge, pre-blur 1.15, NO trailing
/// spread) -> tributaries (spread discharge 1.15) + trunk (spread the SAME raw discharge 2.2) ->
/// drainage (clip(0.68*trib+0.58*trunk)) -> close (low-freq fbm) -> wet_rounding (blur of affine
/// combo, smoothing_px=2.6) -> assemble -> final. All intermediate fields live in the GENERIC
/// scratch POOL (pool0..pool11; see biome_rainforest.glsl for the slot map). The sigmas
/// (1.0, 1.15, 1.7, 2.2, 2.6, 4.5, 5.4) are all in rainforest_sigmas().
///
/// RAINFOREST DUAL-MASK FLOW (the two-spread crux, like temperate): drainage uses `flow_discharge(
/// 0.38, 1.15)` (the common PREFIX of flow_channels_ex up to + including PASS_DISCHARGE -- leaving
/// the raw log1p discharge in gauss_in), NOT the single-spread flow_channels/flow_channels_ex the 8
/// pre-temperate biomes use. It then spreads that RAW discharge at TWO sigmas: gauss(1.15) ->
/// RF_TRIBUTARIES reads gauss_out (-> pool7), then gauss(2.2) -> RF_TRUNK reads gauss_out (-> pool8).
/// The second gauss(2.2) re-reads the SAME intact gauss_in (the generic gaussian only writes
/// gauss_mid/gauss_out, never gauss_in), so no pool staging of the raw discharge is needed. This is
/// EXACTLY temperate's two-spread sequencing -- NO machine extension.
fn schedule_rainforest(s: &mut Scheduler) {
    let smoothing_px = 2.6_f64.max(0.2);             // humid_dissected_hills.smoothing_px (wet_rounding blur)

    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; macro=pool2 ; plateau_seed=pool3 ;
    //    hills_raw=pool4 ; ridges=pool5
    s.dispatch_full(RF_POINTWISE, 0, 0, 0.0);

    // 2) hills = clip(affine(gaussian(hills_raw, 1.7), HILLS))  (REUSE pool4)
    s.gauss_pool(4, 1.7);                            // gauss_out = gaussian(hills_raw, 1.7)
    s.dispatch_full(RF_HILLS, 0, 0, 0.0);            // pool4 = hills

    // 3) plateau = smoothstep(0.54,0.80, gaussian(plateau_seed, 4.5)) * (1 - 0.38*ridges)  (REUSE pool3)
    s.gauss_pool(3, 4.5);                            // gauss_out = gaussian(plateau_seed, 4.5)
    s.dispatch_full(RF_PLATEAU, 0, 0, 0.0);          // pool3 = plateau

    // 4) lowland = smoothstep(lo_e0,lo_e1, gaussian(1 - macro, 5.4))
    s.dispatch_full(RF_ONE_MINUS_MACRO, 0, 0, 0.0);  // gauss_in <- 1 - macro
    s.gauss(5.4);                                    // gauss_out = gaussian(1 - macro, 5.4)
    s.dispatch_full(RF_LOWLAND, 0, 0, 0.0);          // pool6 = lowland

    // 5) flow_source = affine(0.66*macro + 0.46*hills + 0.28*ridges + 0.20*plateau - 0.36*lowland, FLOW) -> flow_pre
    s.dispatch_full(RF_FLOW_PRE, 0, 0, 0.0);         // flow_pre <- flow_source (NO clip)

    // 6) RAW discharge: flow_discharge(power=0.38, pre-blur 1.15) leaves the raw log1p discharge in
    //    gauss_in (NO trailing spread). Then spread it at TWO sigmas (gauss never clobbers gauss_in):
    s.flow_discharge(0.38_f32, 1.15);                // gauss_in = raw log1p discharge
    s.gauss(1.15);                                   // gauss_out = gaussian(discharge, 1.15)
    s.dispatch_full(RF_TRIBUTARIES, 0, 0, 0.0);      // pool7 = tributaries (reads gauss_out)
    s.gauss(2.2);                                    // gauss_out = gaussian(discharge, 2.2) (re-reads gauss_in)
    s.dispatch_full(RF_TRUNK, 0, 0, 0.0);            // pool8 = trunk (reads gauss_out)

    // 7) drainage = clip(0.68*tributaries + 0.58*trunk)  (REUSE pool7)
    s.dispatch_full(RF_DRAINAGE, 0, 0, 0.0);         // pool7 = drainage

    // 8) close = affine(fbm(w_x,w_z, 1/(span*0.030),4,sseed+210,0.45), CLOSE) (NO clip)
    s.dispatch_full(RF_CLOSE, 0, 0, 0.0);            // pool9 = close

    // 9) wet_rounding = gaussian(affine(0.62*macro + 0.36*hills + 0.26*plateau, WET_ROUNDING), smoothing_px=2.6)
    s.dispatch_full(RF_WET_PRE, 0, 0, 0.0);          // pool11 = wet_inner
    s.gauss_pool(11, smoothing_px);                  // gauss_out = gaussian(pool11, 2.6)
    s.dispatch_pool(PASS_POOL_FROM_GAUSS, 10);       // pool10 = wet_rounding

    // 10) assemble height (hills/ridges/plateau - lowland/drainage + close texture; then 0.72*h + 0.28*wet_rounding)
    s.dispatch_full(RF_ASSEMBLE, 0, 0, 0.0);

    // 11) final: height_blur = gaussian(height, 1.0); final_blend = 0.84*h + 0.16*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(1.0);                                    // gauss_out = gaussian(height, 1.0)
    s.dispatch_full(RF_FINAL, 0, 0, 0.0);

    // 12) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

/// The VOLCANIC dispatch schedule (style = stratovolcano_cluster). Mirrors the field DAG of
/// recipes_volcanic.rs::generate_seamsafe ONE-FOR-ONE: warp+regional/rift -> VENT ACCUMULATION
/// (cones/craters/shields raw + raw flows, looping the uploaded vent buffer) -> flows (blur raw
/// flows 1.1) -> cones/craters/shields affine-remap -> lava_texture/rough_aa -> base -> gullies
/// (SHARED flow_discharge pre-blur 1.15, then a FIXED 1.2 spread) -> caldera bowl/rim + cone_lift
/// (uses gaussian(shields+cones,2.6)) -> assemble -> ash_plain blend (gaussian(max(cones,flows),3.0)
/// + gaussian(height,2.6)) -> final. All intermediate fields live in the GENERIC scratch POOL
/// (pool0..pool15; pool15 is TRANSIENT: raw flows staging, then REUSED for max_cf_blur; see
/// biome_volcanic.glsl for the slot map). The sigmas (1.1, 1.15, 1.2, 2.6, 3.0, 0.85) are all in
/// volcanic_sigmas().
///
/// THE KEY INSIGHT (the most novel port): VOLCANIC's vents are placed by numpy PCG64 RNG, but that
/// RNG STAYS IN RUST (recipes_volcanic::packed_vents, parity-exact). The CPU builds a SMALL packed
/// vent buffer (vx,vz,amp + 4 flow dirs per vent) BEFORE the compute list opens; the GPU's
/// VO_VENT_ACCUM pass loops `vent_count` vents doing PURE f32 cone/crater/shield/flow math -- NO RNG
/// on the GPU. The vent buffer is bound at binding 40 (ADDITIVE; the 10 proven biomes never read it).
///
/// VOLCANIC's gully carve uses `flow_discharge(0.40, 1.15)` (the common PREFIX leaving the raw log1p
/// discharge in gauss_in), then a SINGLE spread at sigma=1.2 -- the gully_channels_seam_safe FIXED
/// spread (NOT the flow-channels width.max(0.1)), so it cannot use flow_channels (whose spread sigma
/// IS the width). It rides the proven flow_discharge prefix + a dedicated gauss(1.2).
fn schedule_volcanic(s: &mut Scheduler) {
    // 0) meshgrid
    s.dispatch_full(PASS_MESHGRID, 0, 0, 0.0);

    // 1) pointwise: warp -> pool0=w_x, pool1=w_z ; regional=pool2 ; rift=pool3
    s.dispatch_full(VO_POINTWISE, 0, 0, 0.0);

    // 2) vent accumulation: loop the uploaded vent buffer (PURE f32 math; RNG already done in Rust)
    //    -> cones(pool4), craters(pool5), shields(pool6) RAW ; raw flows -> pool15 (transient)
    s.dispatch_full(VO_VENT_ACCUM, 0, 0, 0.0);

    // 3) flows = clip(affine(gaussian(raw flows, 1.1), FLOWS)) ; pool15(raw flows) -> blur -> pool7
    s.gauss_pool(15, 1.1);                           // gauss_out = gaussian(raw flows, 1.1)
    s.dispatch_full(VO_FLOWS_FINAL, 0, 0, 0.0);      // pool7 = flows

    // 4) finalize cones/craters/shields: clip(affine(raw, *)) in place (pool4/5/6)
    s.dispatch_full(VO_REMAP, 0, 0, 0.0);

    // 5) lava_texture (pool8) + rough_aa (pool9), pointwise on w_x/w_z (NO clip)
    s.dispatch_full(VO_LAVA_ROUGH, 0, 0, 0.0);

    // 6) base = affine(0.58*regional + 0.52*shields*shield_gain + 0.22*rift, BASE) = pool10
    s.dispatch_full(VO_BASE, 0, 0, 0.0);

    // 7) gullies: radial_surface = base + 1.12*cones - 0.78*craters -> flow_pre ; gully_channels
    //    = flow_discharge(power=0.40, pre-blur 1.15) [raw discharge in gauss_in] -> spread 1.2 ;
    //    gullies = smoothstep(0.52,0.92, discharge) * (0.30 + 0.70*cones) = pool11
    s.dispatch_full(VO_RADIAL, 0, 0, 0.0);           // flow_pre <- radial_surface (NO clip)
    s.flow_discharge(0.40_f32, 1.15);                // gauss_in = raw log1p discharge
    s.gauss(1.2);                                    // gauss_out = gaussian(discharge, 1.2)
    s.dispatch_full(VO_GULLIES, 0, 0, 0.0);          // pool11 = gullies

    // 8) caldera: spc_blur = gaussian(shields + cones, 2.6) ; caldera_bowl(pool12),
    //    caldera_rim(pool13), cone_lift(pool14)
    s.dispatch_full(VO_SPC_PRE, 0, 0, 0.0);          // gauss_in <- shields + cones
    s.gauss(2.6);                                    // gauss_out = gaussian(shields+cones, 2.6)
    s.dispatch_full(VO_CALDERA, 0, 0, 0.0);          // pool12/13/14

    // 9) assemble height (base + cone_lift/shields/rift/flows/caldera_rim - caldera_bowl/gullies + detail)
    s.dispatch_full(VO_ASSEMBLE, 0, 0, 0.0);

    // 10) ash_plain blend: max_cf_blur = gaussian(max(cones,flows), 3.0) -> pool15 (REUSE) ;
    //     smoothed_plain = gaussian(height, 2.6) ; height = blend by ash_plain
    s.dispatch_full(VO_ASH_PRE, 0, 0, 0.0);          // gauss_in <- max(cones, flows)
    s.gauss(3.0);                                    // gauss_out = gaussian(max_cf, 3.0)
    s.dispatch_pool(PASS_POOL_FROM_GAUSS, 15);       // pool15 = max_cf_blur (transient reuse)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(2.6);                                    // gauss_out = gaussian(height, 2.6) = smoothed_plain
    s.dispatch_full(VO_ASH_BLEND, 0, 0, 0.0);        // height = blend (reads pool15 + gauss_out)

    // 11) final: height_blur = gaussian(height, 0.85); final_blend = 0.82*h + 0.18*blur; affine(FINAL)
    s.dispatch_full(PASS_COPY, CP_HEIGHT, 0, 0.0);   // gauss_in <- height
    s.gauss(0.85);                                   // gauss_out = gaussian(height, 0.85)
    s.dispatch_full(VO_FINAL, 0, 0, 0.0);

    // 12) crop core (over core cells)
    s.dispatch(PASS_CROP, 0, 0, 0, 0, 0.0, 0, s.wg_core_x, s.wg_core_y);
}

// ===========================================================================
// RUNTIME mountain page producer (Slice-4b, Task 3): the runtime sibling of the readback TEST
// harness `run_inner`. Runs the SAME parity-proven `schedule_mountain` dispatch sequence, but on
// the GLOBAL RenderingDevice with a CACHED context (compiled once, all buffers allocated once)
// and writes each page into a CALLER-OWNED R32F texture (PASS_CROP_IMG -> binding 41) instead of
// reading the core back. Mirrors page_compute.rs's PageComputeContext ownership model.
//
// The math is proven (the existing biome_page parity gate). This is the runtime PLUMBING. The
// LATER windowed 576 parity gate (Task 4) is what PROVES the dispatch is correct (it asserts the
// runtime texture matches the readback core); cargo-green here only proves it compiles + the pure
// helpers + that the existing harness is byte-identical (the 210 stay green).
// ===========================================================================

/// All the per-page-INVARIANT apron working-grid buffers the machine's uniform set binds
/// (the 19 named fields 0..18, the packed kernel buffer 19, flow_pre/acc_a/acc_b 20..22, the core
/// 23, the POOL_SLOTS pool buffers 24..24+SLOTS-1, and the vent buffer 40). Allocated ONCE by
/// `alloc_apron_buffers` and owned by `BiomePageComputeContext`. `kparams` resolves each sigma to
/// its packed-kernel (koffset, kradius). The buffer ROLES + bindings are byte-identical to what
/// `run_inner` allocates inline (same sizes, same zero-init, same kernel packing).
struct ApronBuffers {
    /// bindings 0..=18 (wx, wz, regional, ranges, ridge_detail, near_detail, range_envelope,
    /// lowland, massif, base, primary_mask, tributary_mask, high_mask, valley_mask, height,
    /// floor_mask, gauss_in, gauss_mid, gauss_out) -- the 19 fixed named fields, in binding order.
    fields: Vec<Rid>,
    kernel: Rid,    // binding 19 (packed gaussian kernels at slot*KERNEL_STRIDE)
    flow_pre: Rid,  // binding 20
    acc_a: Rid,     // binding 21
    acc_b: Rid,     // binding 22
    core: Rid,      // binding 23 (storage; schedule_mountain's trailing PASS_CROP writes it, inert)
    pool: Vec<Rid>, // bindings 24..24+POOL_SLOTS-1
    vents: Rid,     // binding 40
    kparams: KernelParams,
}

impl ApronBuffers {
    /// (binding, rid) pairs for the WHOLE machine uniform set EXCEPT the runtime output image
    /// (binding 41). Same binding map `run_inner` builds. The runtime uniform set appends the
    /// image; the test harness (run_inner) does not (and never dispatches PASS_CROP_IMG).
    fn buffer_bindings(&self) -> Vec<(i32, Rid)> {
        let mut b: Vec<(i32, Rid)> = Vec::with_capacity(24 + POOL_SLOTS + 1);
        for (i, &rid) in self.fields.iter().enumerate() {
            b.push((i as i32, rid)); // 0..=18
        }
        b.push((19, self.kernel));
        b.push((20, self.flow_pre));
        b.push((21, self.acc_a));
        b.push((22, self.acc_b));
        b.push((23, self.core));
        for (k, &rid) in self.pool.iter().enumerate() {
            b.push((24 + k as i32, rid));
        }
        b.push((40, self.vents));
        b
    }

    /// Free every RID this owns. The B1 RID-leak lesson: miss none (19 fields + kernel +
    /// flow_pre/acc_a/acc_b + core + POOL_SLOTS pool + vents).
    fn free(&self, rd: &mut Gd<RenderingDevice>) {
        for &rid in &self.fields {
            rd.free_rid(rid);
        }
        rd.free_rid(self.kernel);
        rd.free_rid(self.flow_pre);
        rd.free_rid(self.acc_a);
        rd.free_rid(self.acc_b);
        rd.free_rid(self.core);
        for &rid in &self.pool {
            rd.free_rid(rid);
        }
        rd.free_rid(self.vents);
    }
}

/// Allocate the full apron working-grid buffer set on `rd` (the SAME set `run_inner` allocates
/// inline, in the same binding order with the same zero-init + kernel packing). Factored so the
/// runtime context builder shares it; `run_inner` is left byte-identical (it still allocates
/// inline -- not worth churning the parity-proven path). `n = rows*cols`, `core_n =
/// core_rows*core_cols`. Returns the buffer set or an Err (freeing nothing partial -- the caller
/// only proceeds on Ok, and on Err the few buffers already created are leaked only on a hard
/// pre-list failure that aborts the whole context build, where the rd is the global one; we free
/// what we hold via the returned-on-error path below). `biome` selects the sigma list.
//
// NOTE: run_inner allocates this SAME buffer set inline (parity-frozen path); keep the two in
// sync until Task 4's 576 gate is green and run_inner can consume this helper.
fn alloc_apron_buffers(
    rd: &mut Gd<RenderingDevice>,
    rows: usize,
    cols: usize,
    core_n: usize,
    biome: &str,
    seed: i32,
    feature_span_m: f32,
) -> Result<ApronBuffers, String> {
    let n = rows * cols;
    let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };
    let field_bytes = n * 4;
    let zeros = vec![0.0_f32; n];
    let zeros_pba = PackedByteArray::from(f32s_to_bytes(&zeros).as_slice());
    let mk_field = |rd: &mut Gd<RenderingDevice>| -> Rid {
        rd.storage_buffer_create_ex(bsize(field_bytes)).data(&zeros_pba).done()
    };

    // 19 named fields (bindings 0..=18), in the SAME order as run_inner.
    let mut fields: Vec<Rid> = Vec::with_capacity(19);
    for _ in 0..19 {
        fields.push(mk_field(rd));
    }

    // packed kernel buffer (19): all distinct biome sigmas' kernels at slot*KERNEL_STRIDE.
    let helper_free = |rd: &mut Gd<RenderingDevice>, fields: &[Rid]| {
        for &rid in fields {
            rd.free_rid(rid);
        }
    };
    let sigmas = match biome_sigmas(biome) {
        Some(s) => s,
        None => {
            helper_free(rd, &fields);
            return Err(format!("no sigma list for biome '{biome}' (add a biome_sigmas arm)"));
        }
    };
    let n_slots = sigmas.len();
    let mut packed = vec![0.0_f32; n_slots * KERNEL_STRIDE];
    for (slot, &sg) in sigmas.iter().enumerate() {
        let k = gaussian_kernel1d(sg, TRUNCATE);
        if k.len() > KERNEL_STRIDE {
            helper_free(rd, &fields);
            return Err(format!(
                "gaussian kernel len {} (sigma {sg}) > KERNEL_STRIDE {KERNEL_STRIDE}",
                k.len()
            ));
        }
        let base = slot * KERNEL_STRIDE;
        packed[base..base + k.len()].copy_from_slice(&k);
    }
    let packed_pba = PackedByteArray::from(f32s_to_bytes(&packed).as_slice());
    let kernel = rd
        .storage_buffer_create_ex(bsize(packed.len() * 4))
        .data(&packed_pba)
        .done(); // 19
    let kparams = KernelParams::from_sigmas(&sigmas);

    let flow_pre = mk_field(rd); // 20
    let acc_a = mk_field(rd); // 21
    let acc_b = mk_field(rd); // 22

    // core output (23)
    let core_zeros = vec![0.0_f32; core_n];
    let core_pba = PackedByteArray::from(f32s_to_bytes(&core_zeros).as_slice());
    let core = rd
        .storage_buffer_create_ex(bsize(core_n * 4))
        .data(&core_pba)
        .done(); // 23

    // POOL (24..24+POOL_SLOTS-1)
    let pool: Vec<Rid> = (0..POOL_SLOTS).map(|_| mk_field(rd)).collect();

    // VENT buffer (40): zeroed for non-volcanic biomes (mountain never reads it).
    let (vent_packed, _vent_count): (Vec<f32>, usize) = if biome == "volcanic" {
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
    let vents = rd
        .storage_buffer_create_ex(bsize(vent_packed.len() * 4))
        .data(&vent_pba)
        .done(); // 40

    Ok(ApronBuffers {
        fields,
        kernel,
        flow_pre,
        acc_a,
        acc_b,
        core,
        pool,
        vents,
        kparams,
    })
}

/// The per-page-INVARIANT GPU resources for the RUNTIME mountain page producer: the compiled
/// shader, the compute pipeline, and the full apron buffer set (`ApronBuffers`). Built ONCE
/// (`build_biome_page_context`) on the GLOBAL rd and reused for every page; only the per-page
/// uniform set (cached buffers + this page's image) + push constant vary. Mirrors
/// page_compute.rs::PageComputeContext. Owns every RID -> `free_biome_page_context` frees them all.
///
/// `apron_dim` is the padded working-grid dim (core + 2*apron); `core_px` the core dim;
/// `apron_px` the apron each side; `flow_iters` the PULL-relaxation step count (STABLE_ITERS for
/// the parity-proven path). The vent_count is fixed to 0 here (mountain is the only wired biome;
/// volcanic would need its own context with the live vent count).
pub(crate) struct BiomePageComputeContext {
    pub shader: Rid,
    pub pipeline: Rid,
    bufs: ApronBuffers,
    pub apron_dim: usize,
    pub core_px: usize,
    pub apron_px: usize,
    pub flow_iters: usize,
    /// RUNTIME relief scale (metres): the normalized recipe height (~[-3,2]) is multiplied by this in
    /// PASS_CROP_IMG before the page texture is written, so the render shader (VERTEX.y = h *
    /// relief_scale) gets metres. Tunable via `configure_biome` (the vertical-scale knob).
    pub relief_m: f32,
}

/// Build the cached runtime context ONCE on the GLOBAL `rd`: concat primitives + machine +
/// mountain fragment (EXACTLY as `run_inner` does, via `concat_glsl_hoist_version`), compile,
/// create the pipeline, allocate the full apron buffer set. `core_px`/`apron_px` size the working
/// grid (mountain: 256 / 160 -> apron_dim 576). Returns Err on any compile/create failure (freeing
/// what it already allocated). The producer is wired for "mountain" only (the proven recipe); the
/// biome string is hardcoded so the sigma list + schedule match.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_biome_page_context(
    rd: &mut Gd<RenderingDevice>,
    primitives_src: &str,
    machine_src: &str,
    mountain_fragment_src: &str,
    core_px: usize,
    apron_px: usize,
    flow_iters: usize,
    relief_m: f32,
) -> Result<BiomePageComputeContext, String> {
    let apron_dim = biome_apron_dim(core_px, apron_px);
    if apron_dim <= 2 * apron_px {
        return Err(format!(
            "build_biome_page_context: apron {apron_px} too large for core {core_px}"
        ));
    }
    // concat primitives + (machine + "\n" + fragment), hoisting the machine's #version to line 1 --
    // byte-identical to run_inner's compile path.
    let machine_plus_fragment = format!("{machine_src}\n{mountain_fragment_src}");
    let glsl_stripped =
        crate::primitive_probe::concat_glsl_hoist_version(primitives_src, &machine_plus_fragment);
    let mut src = RdShaderSource::new_gd();
    src.set_stage_source(ShaderStage::COMPUTE, &glsl_stripped);
    let spirv = rd
        .shader_compile_spirv_from_source(&src)
        .ok_or_else(|| {
            "build_biome_page_context: shader_compile_spirv_from_source returned null".to_string()
        })?;
    {
        let err = spirv.get_stage_compile_error(ShaderStage::COMPUTE);
        if !err.is_empty() {
            return Err(format!("build_biome_page_context: GLSL compile error: {err}"));
        }
    }
    let shader = rd.shader_create_from_spirv(&spirv);
    if shader.is_invalid() {
        return Err("build_biome_page_context: shader_create_from_spirv returned invalid RID".into());
    }
    let pipeline = rd.compute_pipeline_create(shader);
    if pipeline.is_invalid() {
        rd.free_rid(shader);
        return Err(
            "build_biome_page_context: compute_pipeline_create returned invalid RID".into(),
        );
    }

    let core_n = core_px * core_px;
    let bufs = match alloc_apron_buffers(rd, apron_dim, apron_dim, core_n, "mountain", 0, 0.0) {
        Ok(b) => b,
        Err(e) => {
            rd.free_rid(pipeline);
            rd.free_rid(shader);
            return Err(format!("build_biome_page_context: {e}"));
        }
    };

    Ok(BiomePageComputeContext {
        shader,
        pipeline,
        bufs,
        apron_dim,
        core_px,
        apron_px,
        flow_iters,
        relief_m,
    })
}

/// Free EVERY RID the runtime context owns (all apron buffers, pipeline, shader). Per-page uniform
/// sets are freed per page inside `compute_biome_page_cached`. The B1 RID-leak lesson: miss none.
pub(crate) fn free_biome_page_context(rd: &mut Gd<RenderingDevice>, ctx: &BiomePageComputeContext) {
    ctx.bufs.free(rd);
    rd.free_rid(ctx.pipeline);
    rd.free_rid(ctx.shader); // cascades any remaining uniform sets created against it
}

/// Dispatch ONE mountain page into `target_rid` (a caller-owned R32F texture) using the CACHED
/// context. Per-page work only: build the uniform set (cached buffers + this page's image at
/// binding 41), open a compute list, construct a `Scheduler` over the cached buffers + open list,
/// run `schedule_mountain` (the SAME proven sequence the test harness runs -- its trailing
/// PASS_CROP into the core storage buffer is inert here), then dispatch PASS_CROP_IMG (core
/// workgroups) to write `target_rid`, submit + sync. NO readback. Frees ONLY the per-page uniform
/// set; the cached shader/pipeline/buffers persist. `target_rid` is NOT freed (the caller owns it).
///
/// `spacing = world_span / (page_px - 1)` (texel-CORNER convention: texel 0 -> origin, page_px-1
/// -> origin+span), matching height_page.glsl:191-195. The apron-padded origin is
/// `origin - apron_px*spacing` per axis (the meshgrid pass subtracts the apron back off).
///
/// SCALE-INVARIANCE: `spacing` is computed INTERNALLY (callers don't pass it) and world-anchors
/// every gaussian kernel via `mountain_kernels_anchored(spacing)` -> the cached kernel buffer is
/// RE-FILLED per dispatch (the buffer RID stays allocated; only its bytes change) so each clipmap
/// LEVEL bakes its blurs at its OWN spacing -> the macro structure matches across levels (no
/// geomorph warp). `flow_on` gates the drainage carve: `false` on coarse levels SKIPS the two
/// flow_channels passes (cheaper) -> the MACRO surface, mirroring the CPU oracle. At
/// `spacing == S_REF` (32.0) + `flow_on == true` this reproduces the parity-proven page byte-for-byte.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_biome_page_cached(
    rd: &mut Gd<RenderingDevice>,
    ctx: &BiomePageComputeContext,
    target_rid: Rid,
    origin_x: f64,
    origin_z: f64,
    world_span: f64,
    page_px: i64,
    feature_span_m: f64,
    seed: i64,
    flow_on: bool,
) -> Result<(), String> {
    if page_px as usize != ctx.core_px {
        return Err(format!(
            "compute_biome_page_cached: page_px {page_px} != context core_px {} (rebuild the context)",
            ctx.core_px
        ));
    }
    if page_px < 2 {
        return Err(format!("compute_biome_page_cached: page_px {page_px} must be >= 2"));
    }
    if seed < i32::MIN as i64 || seed > i32::MAX as i64 {
        return Err(format!(
            "compute_biome_page_cached: seed {seed} outside i32 range (GPU hash is 32-bit-seed)"
        ));
    }
    // spacing computed INSIDE (texel-corner): callers pass world_span+page_px, not spacing.
    let spacing_f64 = world_span / (page_px as f64 - 1.0);
    let spacing = spacing_f64 as f32;
    let ox = (origin_x - ctx.apron_px as f64 * spacing as f64) as f32;
    let oz = (origin_z - ctx.apron_px as f64 * spacing as f64) as f32;

    // SCALE-INVARIANCE: rebuild the WORLD-anchored gaussian kernels for THIS dispatch's spacing and
    // re-fill the cached kernel buffer (binding 19) in place. `kparams_anchored` keeps the same slot
    // LOOKUP keys (the reference cell sigmas `schedule_mountain` asks for) but with the anchored
    // koffset/kradius; the GLSL machine is unchanged (it reads kradius/koffset from the push constant
    // and taps the now-anchored packed buffer). Built BEFORE the compute list opens (a panic mid-list
    // would leak the open list); the buffer_update is RECORDED on the global RD and auto-submitted
    // before the compute dispatches, exactly like the create-time .data() upload.
    // NOTE (deferred optimization): the packed kernel is rebuilt + re-uploaded PER PAGE (~2.3 KB =
    // 9 slots * 64 taps * 4 B, non-stalling buffer_update). spacing takes only ~NUM_LEVELS distinct
    // values across the whole clipmap, so a cache-by-spacing (build each level's kernels once, keep
    // a small LUT, bind/select instead of re-upload) is the deferred optimization (scale-invariant
    // plan) IF this ever shows on a perf profile. Kept simple-per-page until measured.
    let (packed_kernel, kparams_anchored) = mountain_kernels_anchored(spacing_f64)?;
    let packed_pba = PackedByteArray::from(f32s_to_bytes(&packed_kernel).as_slice());
    let upd = rd.buffer_update(
        ctx.bufs.kernel,
        0,
        (packed_kernel.len() * 4) as u32,
        &packed_pba,
    );
    if upd != godot::global::Error::OK {
        return Err(format!(
            "compute_biome_page_cached: buffer_update(kernel) failed: {upd:?}"
        ));
    }
    // INVARIANT: the anchored kparams must key by the SAME reference sigmas (same slot LAYOUT) the
    // context allocated -- only koffset/kradius differ with spacing. If the slot KEYS ever diverged
    // the re-filled buffer would be indexed by a koffset the schedule never produced. (Also keeps
    // the context's build-time `kparams` a live read, documenting the layout contract.)
    debug_assert_eq!(
        ctx.bufs.kparams.slots.len(),
        kparams_anchored.slots.len(),
        "anchored kparams slot count must match the context's allocated kernel layout"
    );
    debug_assert!(
        ctx.bufs
            .kparams
            .slots
            .iter()
            .zip(kparams_anchored.slots.iter())
            .all(|(a, b)| (a.0 - b.0).abs() < 1e-9 && a.1 == b.1),
        "anchored kparams must key by the SAME reference sigmas at the SAME koffsets"
    );

    // per-page uniform set: the cached buffers (0..40) + this page's image (41).
    let bindings = ctx.bufs.buffer_bindings();
    let mut uniforms: Array<Gd<RdUniform>> = Array::new();
    for (bind, rid) in bindings.iter() {
        uniforms.push(&make_storage_uniform(*bind, *rid));
    }
    uniforms.push(&make_image_uniform(41, target_rid));
    let uset = rd.uniform_set_create(&uniforms, ctx.shader, 0);
    if uset.is_invalid() {
        return Err("compute_biome_page_cached: uniform_set_create returned invalid RID".into());
    }

    let rows = ctx.apron_dim;
    let cols = ctx.apron_dim;
    let apron = ctx.apron_px;
    let core_rows = ctx.core_px;
    let core_cols = ctx.core_px;
    let wg_full_x = (cols as u32).div_ceil(16);
    let wg_full_y = (rows as u32).div_ceil(16);
    let wg_core_x = (core_cols as u32).div_ceil(16);
    let wg_core_y = (core_rows as u32).div_ceil(16);

    // PRE-VALIDATE every sigma BEFORE the list opens (KernelParams::kp `.expect`s; a panic with an
    // open list would leak). The ANCHORED kparams key by the SAME reference sigmas mountain_sigmas()
    // lists (only koffset/kradius differ), so every lookup the schedule makes resolves.
    for &sg in mountain_sigmas().iter() {
        let _ = kparams_anchored.kp(sg);
    }

    let cl = rd.compute_list_begin();
    rd.compute_list_bind_compute_pipeline(cl, ctx.pipeline);

    // Scheduler over the cached buffers + open list. The ANCHORED kparams (built above from this
    // dispatch's spacing) is moved into the Scheduler so the gauss passes use the anchored radii.
    let kparams = kparams_anchored;
    let mut sched = Scheduler {
        rd,
        cl,
        uset,
        rows: rows as i32,
        cols: cols as i32,
        apron: apron as i32,
        seed: seed as i32,
        spacing,
        ox,
        oz,
        feature_span_m: feature_span_m as f32,
        vent_count: 0, // mountain never reads vents
        favor_strength: 0.0,
        relief_confidence_floor: 0.0,
        relief_m: ctx.relief_m, // RUNTIME: scale normalized height -> metres in PASS_CROP_IMG
        wg_full_x,
        wg_full_y,
        wg_core_x,
        wg_core_y,
        kparams,
        flow_iters: ctx.flow_iters,
        flow_on, // SCALE-INVARIANCE: coarse levels pass false -> macro surface (no carve).
    };
    // Run the PROVEN mountain schedule (ends with PASS_CROP into the core storage buffer -- inert
    // here, we don't read it), then crop to the IMAGE.
    schedule_mountain(&mut sched);
    sched.dispatch(PASS_CROP_IMG, 0, 0, 0, 0, 0.0, 0, wg_core_x, wg_core_y);

    rd.compute_list_end();
    // RUNTIME (global RD): fire-and-forget — do NOT submit()/sync() here. This producer runs on the
    // MAIN RenderingDevice (the one the renderer owns), where manual submit/sync is ILLEGAL
    // ("Only local devices can submit and sync" — rendering_device.cpp:6551). The engine auto-submits
    // the global RD's queued work at draw, exactly like the legacy `compute_page_cached`
    // (page_compute.rs:166: "no submit/sync; the engine auto-submits at draw"). Intra-schedule
    // ordering is enforced by the `compute_list_add_barrier` calls RECORDED INTO the list (Scheduler),
    // which are honored at submission regardless of who submits. (The readback test entries use a
    // LOCAL rd via create_local_rendering_device, where submit/sync IS legal — those keep theirs.)
    rd.free_rid(uset); // free ONLY the per-page uniform set; cached resources persist
    Ok(())
}

#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct Wg10BiomePageCompute {
    primitives_src: Option<String>,
    /// The GENERIC machine (biome_page.glsl): bindings + leaf helpers + generic passes + main().
    /// One of the two STABLE parts (the other being primitives); loaded once via load_shaders.
    machine_src: Option<String>,
    /// A biome FRAGMENT (any -- mountain by convention) concatenated ONLY to satisfy the machine's
    /// `biome_pass()` declaration during compose. The compose passes are inline in main() and never
    /// reach the fragment, so the choice is irrelevant. Loaded once via `load_compose_fragment`.
    compose_fragment: Option<String>,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for Wg10BiomePageCompute {
    fn init(base: Base<RefCounted>) -> Self {
        Self { primitives_src: None, machine_src: None, compose_fragment: None, base }
    }
}

#[godot_api]
impl Wg10BiomePageCompute {
    /// Load the two STABLE GLSL parts (primitives helpers + the GENERIC machine) from OS paths
    /// and keep them. The per-biome FRAGMENT is loaded separately, per call, by
    /// `generate_core_page` (it selects which biome to bake). At compile time all three are
    /// concatenated as primitives + machine + fragment (Godot GLSL has no #include). Returns ""
    /// on success, an error string otherwise. Mirrors `Wg10PrimitiveProbe::load_shader`.
    #[func]
    pub fn load_shaders(&mut self, primitives_path: GString, machine_path: GString) -> GString {
        let prim = match std::fs::read_to_string(primitives_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(format!("primitives glsl: {e}").as_str()),
        };
        let machine = match std::fs::read_to_string(machine_path.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(format!("machine glsl: {e}").as_str()),
        };
        self.primitives_src = Some(prim);
        self.machine_src = Some(machine);
        GString::new()
    }

    /// Load the biome FRAGMENT used to satisfy the machine's `biome_pass()` declaration during
    /// COMPOSE (any biome works -- mountain by convention; the compose passes are inline in main()
    /// and never reach the fragment). Returns "" on success, an error string otherwise. Call once
    /// before `compose_fields` / `blend_pair`.
    #[func]
    pub fn load_compose_fragment(&mut self, fragment_path: GString) -> GString {
        match std::fs::read_to_string(fragment_path.to_string()) {
            Ok(s) => {
                self.compose_fragment = Some(s);
                GString::new()
            }
            Err(e) => GString::from(format!("compose fragment glsl: {e}").as_str()),
        }
    }

    /// Run the FULL mountain pass chain for ONE page (style = ALPINE_BRANCHING, matching the
    /// fixture's `style_key`) on a local RenderingDevice and return the CORE f64 height
    /// (length core_rows*core_cols, NORMALIZED recipe units, pre-relief). The apron meshgrid
    /// is rebuilt on the GPU from (spacing, ox, oz, apron_px, padded dims). Readback ONLY
    /// here (test entry). Returns an EMPTY array on error (see godot_error log).
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
        // The GLSL lattice hash is 32-bit-seed throughout (push constant `int seed`), so a seed
        // outside i32 range cannot reach the GPU intact. Fail LOUDLY instead of silently
        // truncating (which would diverge from the i64 CPU oracle without warning). Real fixtures
        // use small seeds; this guards future records / callers.
        if seed < i32::MIN as i64 || seed > i32::MAX as i64 {
            godot_error!("Wg10BiomePageCompute::generate_core_page: seed {seed} outside i32 range (GPU hash is 32-bit-seed); CPU oracle is i64 -> parity impossible. Use a seed in i32 range.");
            return PackedFloat64Array::new();
        }
        // Load the selected per-biome FRAGMENT (the biome_pass() body) for this call.
        let frag_path = biome_fragment_path.to_string();
        let fragment = match std::fs::read_to_string(&frag_path) {
            Ok(s) => s,
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_core_page: biome fragment glsl: {e}");
                return PackedFloat64Array::new();
            }
        };
        // Biome selector = the fragment path stem with a leading `biome_` stripped, e.g.
        // ".../biome_mountain.glsl" -> "mountain". `run_inner` matches on this to pick the
        // per-biome schedule fn.
        let biome = biome_stem(&frag_path);
        match self.run_inner(
            spacing as f32, ox as f32, oz as f32, rows, cols, apron, seed as i32, feature_span_m as f32,
            &fragment, &biome, STABLE_ITERS,
        ) {
            Ok(core) => {
                let mut out = PackedFloat64Array::new();
                out.resize(core.len());
                let sl = out.as_mut_slice();
                for i in 0..core.len() {
                    sl[i] = core[i] as f64;
                }
                out
            }
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_core_page error: {e}");
                PackedFloat64Array::new()
            }
        }
    }

    /// MEASUREMENT entry: `generate_core_page` with the flow PULL-relaxation step count made a
    /// caller parameter (`flow_iters`), so a windowed harness can sweep it at the REAL 576
    /// production apron to find the production convergence count (decides whether live-per-page
    /// flow fits the budget, i.e. whether the coarse-drainage-fact subsystem is needed). NOT a
    /// runtime entry; same readback-only caveat as `generate_core_page`. `generate_core_page`
    /// itself passes STABLE_ITERS, so the parity-proven path is unchanged.
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
            godot_error!("Wg10BiomePageCompute::generate_core_page_iters: seed {seed} outside i32 range");
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
                godot_error!("Wg10BiomePageCompute::generate_core_page_iters: biome fragment glsl: {e}");
                return PackedFloat64Array::new();
            }
        };
        let biome = biome_stem(&frag_path);
        match self.run_inner(
            spacing as f32, ox as f32, oz as f32, rows, cols, apron, seed as i32, feature_span_m as f32,
            &fragment, &biome, flow_iters as usize,
        ) {
            Ok(core) => {
                let mut out = PackedFloat64Array::new();
                out.resize(core.len());
                let sl = out.as_mut_slice();
                for i in 0..core.len() {
                    sl[i] = core[i] as f64;
                }
                out
            }
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_core_page_iters error: {e}");
                PackedFloat64Array::new()
            }
        }
    }

    /// RUNTIME-producer readback entry (Slice-4b, Task 3): exercises the REAL runtime mountain
    /// page producer (`build_biome_page_context` + `compute_biome_page_cached` + the crop-to-image
    /// PASS_CROP_IMG path) end-to-end, but on a LOCAL RenderingDevice + a scratch R32F TEXTURE so
    /// it is test-runnable from a WINDOWED gate. Builds a context, dispatches one page into the
    /// scratch texture, reads the texture back (`texture_get_data`), frees the context + texture +
    /// rd, and returns the CORE f64 height (length core_px*core_px). The LATER windowed 576 parity
    /// gate (Task 4) compares THIS against `generate_core_page` to PROVE the runtime producer
    /// matches the proven readback core bit-for-bit.
    ///
    /// Convention matches `generate_core_page`: `ox`/`oz` are the PADDED-grid origin and `spacing`
    /// the metres/px. The runtime producer takes (origin, world_span, page_px) instead, so this
    /// converts: `page_px = padded_rows - 2*apron_px`, `world_span = spacing*(page_px-1)`,
    /// `origin = ox + apron_px*spacing` (the producer re-subtracts the apron). MOUNTAIN only.
    ///
    /// `flow_iters` = the flow PULL-relaxation step count threaded into `build_biome_page_context`
    /// (mirrors `generate_core_page_iters`). The 576 production page needs MORE than the recipe-path
    /// STABLE_ITERS=128 to converge to the exact f64 sweep oracle (~192 measured), so the windowed
    /// 576 parity gate sweeps this to separate UNDER-CONVERGENCE from a real divergence.
    /// Returns an EMPTY array on error (see godot_error log). WINDOWED only (local RD null headless).
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
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: seed {seed} outside i32 range");
            return PackedFloat64Array::new();
        }
        if flow_iters < 1 {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: flow_iters must be >= 1");
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
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: mountain fragment glsl: {e}");
                return PackedFloat64Array::new();
            }
        };

        // LOCAL rd (test entry; the production caller passes the GLOBAL rd instead).
        let mut rd: Gd<RenderingDevice> = match RenderingServer::singleton().create_local_rendering_device() {
            Some(d) => d,
            None => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: create_local_rendering_device returned null (headless / no device)");
                return PackedFloat64Array::new();
            }
        };

        // Build the cached runtime context (compile + pipeline + all buffers, on this local rd).
        // relief_m = 1.0: the 576 PARITY readback must stay in NORMALIZED units to match the f64
        // oracle (the runtime render path uses the configured metre relief; parity does not).
        let ctx = match build_biome_page_context(
            &mut rd, prim, machine, &fragment, core_px, apron, flow_iters as usize, 1.0,
        ) {
            Ok(c) => c,
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: {e}");
                rd.free();
                return PackedFloat64Array::new();
            }
        };

        // Scratch R32F output texture (caller-owned model; here the test owns + frees it).
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

        // Reconcile the padded-origin convention to the producer's (origin, world_span, page_px).
        let page_px = core_px as i64;
        let world_span = spacing * (page_px as f64 - 1.0);
        let origin_x = ox + apron as f64 * spacing;
        let origin_z = oz + apron as f64 * spacing;

        if let Err(e) = compute_biome_page_cached(
            // flow_on=true: the 576 PARITY readback must match the flow-ON f64 oracle. The
            // spacing-anchored kernels are now built INSIDE from world_span/(page_px-1), so the
            // regenerated oracle (at the SAME spacing) must match bit-for-bit (Tier-2).
            &mut rd, &ctx, tex, origin_x, origin_z, world_span, page_px, feature_span_m, seed, true,
        ) {
            godot_error!("Wg10BiomePageCompute::generate_runtime_page_576: {e}");
            rd.free_rid(tex);
            free_biome_page_context(&mut rd, &ctx);
            rd.free();
            return PackedFloat64Array::new();
        }

        // Read the page texture back (layer 0). R32F -> 4 bytes/texel, core_px*core_px texels.
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
        let mut out = PackedFloat64Array::new();
        out.resize(core.len());
        let sl = out.as_mut_slice();
        for i in 0..core.len() {
            sl[i] = core[i] as f64;
        }
        out
    }

    /// COMPOSE entry (Slice-4b.11): GPU port of `biome_compose::compose_biomes`. Composes
    /// `n_fields` per-recipe height fields (concatenated row-major in `fields_flat`, each
    /// `rows*cols` long) by their per-pixel weights (`weights_flat`, same layout) into one field.
    /// `mode_is_field` chooses the blend mode (true="field", false="height_favored"); the favored
    /// path is applied EXACTLY for n_fields==2 (the fold uses FIELD blend for 3+ -- mirrors the
    /// oracle). Returns the composed field (length rows*cols) or an EMPTY array on error.
    /// Readback ONLY here (test/gate entry). WINDOWED only (local RD null headless).
    #[allow(clippy::too_many_arguments)]
    #[func]
    pub fn compose_fields(
        &self,
        fields_flat: PackedFloat64Array,
        weights_flat: PackedFloat64Array,
        n_fields: i64,
        rows: i64,
        cols: i64,
        mode_is_field: bool,
        favor_strength: f64,
        relief_confidence_floor: f64,
    ) -> PackedFloat64Array {
        let rows = rows as usize;
        let cols = cols as usize;
        let n = rows * cols;
        let nf = n_fields as usize;
        if nf == 0 {
            godot_error!("compose_fields: n_fields must be >= 1");
            return PackedFloat64Array::new();
        }
        if fields_flat.len() != nf * n || weights_flat.len() != nf * n {
            godot_error!(
                "compose_fields: fields/weights flat len mismatch (got {}/{}, expected {})",
                fields_flat.len(), weights_flat.len(), nf * n
            );
            return PackedFloat64Array::new();
        }
        // un-flatten into per-recipe f32 fields (the GPU is f32 throughout).
        let ff = fields_flat.as_slice();
        let wf = weights_flat.as_slice();
        let mut fields: Vec<Vec<f32>> = Vec::with_capacity(nf);
        let mut weights: Vec<Vec<f32>> = Vec::with_capacity(nf);
        for k in 0..nf {
            fields.push(ff[k * n..(k + 1) * n].iter().map(|&x| x as f32).collect());
            weights.push(wf[k * n..(k + 1) * n].iter().map(|&x| x as f32).collect());
        }
        match self.run_compose_inner(
            &fields, &weights, rows, cols, mode_is_field,
            favor_strength as f32, relief_confidence_floor as f32,
        ) {
            Ok(out) => f32s_to_packed_f64(&out),
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::compose_fields error: {e}");
                PackedFloat64Array::new()
            }
        }
    }

    /// BLEND-PAIR entry (Slice-4b.11): GPU port of `biome_compose::blend_field` (mode_is_field=true)
    /// and `biome_compose::blend_height_favored` (mode_is_field=false). A SINGLE blend of `a`/`b`
    /// at per-pixel weight `w_a` (NOT the running-accumulator fold -- w_a is used DIRECTLY, exactly
    /// as the two standalone oracle functions do). Returns the blended field (length rows*cols) or
    /// an EMPTY array on error. Readback ONLY here. WINDOWED only.
    #[allow(clippy::too_many_arguments)]
    #[func]
    pub fn blend_pair(
        &self,
        a: PackedFloat64Array,
        b: PackedFloat64Array,
        w_a: PackedFloat64Array,
        rows: i64,
        cols: i64,
        mode_is_field: bool,
        favor_strength: f64,
        relief_confidence_floor: f64,
    ) -> PackedFloat64Array {
        let rows = rows as usize;
        let cols = cols as usize;
        let n = rows * cols;
        if a.len() != n || b.len() != n || w_a.len() != n {
            godot_error!(
                "blend_pair: a/b/w_a len mismatch (got {}/{}/{}, expected {})",
                a.len(), b.len(), w_a.len(), n
            );
            return PackedFloat64Array::new();
        }
        let a32: Vec<f32> = a.as_slice().iter().map(|&x| x as f32).collect();
        let b32: Vec<f32> = b.as_slice().iter().map(|&x| x as f32).collect();
        let w32: Vec<f32> = w_a.as_slice().iter().map(|&x| x as f32).collect();
        match self.run_blend_inner(
            &a32, &b32, &w32, rows, cols, mode_is_field,
            favor_strength as f32, relief_confidence_floor as f32,
        ) {
            Ok(out) => f32s_to_packed_f64(&out),
            Err(e) => {
                godot_error!("Wg10BiomePageCompute::blend_pair error: {e}");
                PackedFloat64Array::new()
            }
        }
    }

    // ---- internal GPU pipeline ----
    #[allow(clippy::too_many_arguments)]
    fn run_inner(
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
        let glsl_stripped = crate::primitive_probe::concat_glsl_hoist_version(prim, &machine_plus_fragment);
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

    /// COMPOSE engine (Slice-4b.11): the shared GPU setup for the compose layer. Allocates the SAME
    /// machine binding set (0..40) as `run_inner` (the machine declares them all, so the uniform set
    /// must satisfy every binding), uploads the compose initial buffers (height=acc0, base=acc_w0,
    /// pool0=f, pool1=w), builds the kernel buffer with the single compose relief sigma (6.0), opens
    /// one compute list, builds a Scheduler with the compose params, runs `op` (the fold/blend
    /// sequence), then reads back `height` (the composed result -- compose has NO apron / crop, the
    /// whole rows*cols field is the answer). WINDOWED only (local RD null headless). Concats the
    /// MOUNTAIN fragment purely to satisfy the machine's `biome_pass()` declaration -- the compose
    /// passes are handled INLINE in main() and never reach the fragment.
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
        let glsl_stripped = crate::primitive_probe::concat_glsl_hoist_version(prim, &machine_plus_fragment);
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
    fn run_compose_inner(
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
    fn run_blend_inner(
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
        let glsl_stripped = crate::primitive_probe::concat_glsl_hoist_version(prim, &machine_plus_fragment);
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

// ---------------------------------------------------------------------------
// Unit tests (pure helpers; no Godot runtime)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod biome_page_compute_tests {
    use super::*;

    #[test]
    fn kernel_sums_to_one() {
        for &sigma in &[1.0_f64, 1.15, 1.2, 1.8, 2.0, 5.0, 7.0, 2.4] {
            let k = gaussian_kernel1d(sigma, TRUNCATE);
            let s: f64 = k.iter().map(|&v| v as f64).sum();
            assert!((s - 1.0).abs() < 1e-5, "sigma {sigma}: sum {s} != 1");
        }
    }

    #[test]
    fn kernel_is_symmetric() {
        let k = gaussian_kernel1d(2.0, TRUNCATE);
        let n = k.len();
        for i in 0..n {
            assert!((k[i] - k[n - 1 - i]).abs() < 1e-7, "kernel not symmetric at {i}");
        }
    }

    #[test]
    fn kernel_length_matches_radius() {
        // array_ops: lw = int(truncate*sigma + 0.5); length = 2*lw+1.
        // sigma 1.0, truncate 4.0 -> lw = int(4.5) = 4 -> length 9.
        let k = gaussian_kernel1d(1.0, TRUNCATE);
        assert_eq!(k.len(), 9);
        assert_eq!(gaussian_radius(1.0, TRUNCATE), 4);
        // sigma 7.0 -> lw = int(28.5) = 28 -> length 57.
        assert_eq!(gaussian_radius(7.0, TRUNCATE), 28);
        assert_eq!(gaussian_kernel1d(7.0, TRUNCATE).len(), 57);
        // sigma 2.4 -> lw = int(10.1) = 10 -> length 21.
        assert_eq!(gaussian_radius(2.4, TRUNCATE), 10);
    }

    #[test]
    fn kernel_center_is_peak() {
        let k = gaussian_kernel1d(2.0, TRUNCATE);
        let lw = (k.len() - 1) / 2;
        for i in 0..k.len() {
            assert!(k[lw] >= k[i], "center not peak");
        }
    }

    #[test]
    fn all_mountain_kernels_fit_stride() {
        for &sg in &mountain_sigmas() {
            let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
            assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
        }
    }

    /// SCALE-INVARIANCE identity: at spacing == S_REF (32.0), `sigma_cells` is the identity, so the
    /// anchored kernels MUST equal the cell-sigma kernels (the parity-proven content) BYTE-for-byte,
    /// and the kparams slot layout (key + koffset + kradius) MUST match `alloc_apron_buffers`'. This
    /// pins the property the windowed 576 gate depends on: at the production finest level (~32 m/px)
    /// the GPU kernels are unchanged from the cell-sigma path.
    #[test]
    fn anchored_kernels_identity_at_s_ref() {
        use crate::recipes::helpers::S_REF;
        let (packed, kp) = mountain_kernels_anchored(S_REF).expect("kernels fit at S_REF");
        // Reference packed buffer (the cell-sigma path: gaussian_kernel1d(ref, TRUNCATE)).
        let refs = mountain_sigmas();
        for (slot, &ref_sigma) in refs.iter().enumerate() {
            let want = gaussian_kernel1d(ref_sigma, TRUNCATE);
            let base = slot * KERNEL_STRIDE;
            for (j, &w) in want.iter().enumerate() {
                assert_eq!(packed[base + j], w, "slot {slot} (sigma {ref_sigma}) tap {j} differs at S_REF");
            }
            // slot layout: keyed by the REFERENCE sigma, anchored koffset/kradius == cell-sigma ones.
            let (ko, kr) = kp.kp(ref_sigma);
            assert_eq!(ko, (slot * KERNEL_STRIDE) as i32, "koffset drift at S_REF");
            assert_eq!(kr, gaussian_radius(ref_sigma, TRUNCATE) as i32, "kradius drift at S_REF");
        }
    }

    /// Anchoring DIRECTION + lookup-key stability. At a COARSER spacing (> S_REF) every anchored
    /// sigma SHRINKS (covers the same world distance with fewer cells) -> radius <= the cell radius;
    /// the slot KEY stays the reference sigma (so `schedule_mountain`'s `gauss(5.0)` still resolves).
    /// At the production finest level (~32) all kernels fit the stride; this also confirms the
    /// over-stride guard only trips at an unrealistically fine spacing.
    #[test]
    fn anchored_kernels_shrink_and_key_by_reference_when_coarser() {
        use crate::recipes::helpers::{sigma_cells, S_REF};
        let coarse = S_REF * 4.0; // a coarse clipmap level (4x the reference spacing)
        let (_packed, kp) = mountain_kernels_anchored(coarse).expect("coarse kernels fit");
        for &ref_sigma in &mountain_sigmas() {
            let anchored = sigma_cells(ref_sigma, coarse);
            assert!(anchored < ref_sigma + 1e-12, "coarser spacing must shrink sigma");
            // lookup by the REFERENCE sigma (the schedule's key) resolves to the ANCHORED radius.
            let (_ko, kr) = kp.kp(ref_sigma);
            assert_eq!(
                kr,
                gaussian_radius(anchored, TRUNCATE) as i32,
                "kradius must reflect the anchored sigma, keyed by the reference sigma"
            );
        }
    }

    #[test]
    fn mountain_sigmas_cover_all_pipeline_blurs() {
        // every sigma the pass chain asks for must be present (kparams panics otherwise).
        let valley = 2.4_f64;
        let trib = (valley * 0.42_f64).max(0.6);
        let floor = 4.0_f64.max(0.2);
        let s = mountain_sigmas();
        for need in [1.15_f64, 1.20, 1.80, 2.00, 5.00, 7.00, valley, trib, floor, valley.max(0.1), trib.max(0.1)] {
            assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
        }
    }

    #[test]
    fn apron_dim_adds_two_aprons() {
        assert_eq!(apron_dim(24, 160), 344);
        assert_eq!(apron_dim(256, 160), 576);
    }

    #[test]
    fn push_constant_is_96_bytes() {
        let p = build_push(0, 344, 344, 160, 0, 4, 0, 0, 0, 0, 0, 3913.04, 12000.0, -31000.0, 90000.0, 0.48, 0.0, 0.0, 0.0);
        assert_eq!(p.len(), 96);
    }

    #[test]
    fn push_constant_packs_ints_then_floats() {
        // build_push(pass,rows,cols,apron,seed,kradius,copy_sel,flow_dir,koffset,pool_sel,vent_count,spacing,ox,oz,span,power,favor,floor)
        let p = build_push(7, 344, 343, 160, 5, 28, 2, 1, 128, 9, 4, 3913.0, 12000.0, -31000.0, 90000.0, 0.34, 0.0, 0.0, 0.0);
        assert_eq!(i32::from_le_bytes([p[0], p[1], p[2], p[3]]), 7);
        assert_eq!(i32::from_le_bytes([p[4], p[5], p[6], p[7]]), 344);
        assert_eq!(i32::from_le_bytes([p[8], p[9], p[10], p[11]]), 343);
        assert_eq!(i32::from_le_bytes([p[12], p[13], p[14], p[15]]), 160);
        assert_eq!(i32::from_le_bytes([p[16], p[17], p[18], p[19]]), 5);
        assert_eq!(i32::from_le_bytes([p[20], p[21], p[22], p[23]]), 28);
        assert_eq!(i32::from_le_bytes([p[24], p[25], p[26], p[27]]), 2);
        assert_eq!(i32::from_le_bytes([p[28], p[29], p[30], p[31]]), 1);
        assert_eq!(i32::from_le_bytes([p[32], p[33], p[34], p[35]]), 128); // koffset
        assert_eq!(i32::from_le_bytes([p[36], p[37], p[38], p[39]]), 9);   // pool_sel
        assert_eq!(i32::from_le_bytes([p[40], p[41], p[42], p[43]]), 4);   // vent_count (former ipad1)
        // 1 int pad at 44..48; floats start at byte 48.
        let spacing = f32::from_le_bytes([p[48], p[49], p[50], p[51]]);
        assert!((spacing - 3913.0).abs() < 1e-1);
        // floats: spacing(48),ox(52),oz(56),span(60),power(64)
        let flow_power = f32::from_le_bytes([p[64], p[65], p[66], p[67]]);
        assert!((flow_power - 0.34).abs() < 1e-6);
    }

    #[test]
    fn non_volcanic_push_vent_count_is_zero_byte_identical() {
        // The 10 proven biomes pass vent_count=0 -> byte-identical to the former hardcoded `0` pad.
        // Build a representative mountain dispatch push with vent_count=0 and confirm the vent_count
        // int slot (bytes 40..44) is exactly zero (so mountain's 1.89e-6 parity is preserved).
        let p = build_push(8, 344, 344, 160, 0, 0, 0, 0, 0, 0, 0, 2608.7, 12000.0, -31000.0, 60000.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(i32::from_le_bytes([p[40], p[41], p[42], p[43]]), 0);
        assert_eq!(p.len(), 96);
        // The two compose param floats (pad0=favor_strength, pad1=relief_conf_floor) are at bytes
        // 68..72 and 72..76. For a non-compose dispatch they are 0.0 -> byte-identical to the former
        // all-zero pad block, so the 11 proven biomes' push bytes are unchanged.
        assert_eq!(f32::from_le_bytes([p[68], p[69], p[70], p[71]]), 0.0);
        assert_eq!(f32::from_le_bytes([p[72], p[73], p[74], p[75]]), 0.0);
    }

    #[test]
    fn push_constant_carries_compose_params_in_pads() {
        // favor_strength -> pad0 (bytes 68..72), relief_confidence_floor -> pad1 (bytes 72..76).
        let p = build_push(64, 32, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 1e-3, 0.0);
        let favor = f32::from_le_bytes([p[68], p[69], p[70], p[71]]);
        let floor = f32::from_le_bytes([p[72], p[73], p[74], p[75]]);
        assert!((favor - 2.0).abs() < 1e-7, "favor_strength not in pad0");
        assert!((floor - 1e-3).abs() < 1e-9, "relief_confidence_floor not in pad1");
        // the remaining 4 float pads (bytes 76..96) stay zero.
        for off in (76..96).step_by(4) {
            assert_eq!(f32::from_le_bytes([p[off], p[off + 1], p[off + 2], p[off + 3]]), 0.0);
        }
        assert_eq!(p.len(), 96);
    }

    #[test]
    fn compose_sigmas_has_relief_sigma() {
        // The compose relief proxy uses exactly sigma = relief_sigma_px default = 6.0.
        let s = compose_sigmas();
        assert_eq!(s.len(), 1);
        assert!((s[0] - 6.0).abs() < 1e-12);
        // its kernel must fit the packed-kernel stride.
        let len = 2 * gaussian_radius(s[0], TRUNCATE) + 1;
        assert!(len <= KERNEL_STRIDE, "compose kernel len {len} > {KERNEL_STRIDE}");
        // sigma 6.0 -> lw = int(4.0*6.0+0.5) = int(24.5) = 24 -> length 49.
        assert_eq!(gaussian_radius(6.0, TRUNCATE), 24);
        assert_eq!(len, 49);
    }

    #[test]
    fn compose_kernel_matches_array_ops_relief_sigma() {
        // The GPU relief proxy gaussian MUST use the SAME sigma=6.0 kernel as
        // biome_compose.rs::GAUSSIAN_TRUNCATE-driven gaussian_filter_nearest. Verify the kernel
        // sums to ~1 (normalized) and is symmetric (the array_ops contract).
        let k = gaussian_kernel1d(6.0, TRUNCATE);
        let sum: f32 = k.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "compose relief kernel not normalized (sum={sum})");
        let n = k.len();
        for i in 0..n {
            assert!((k[i] - k[n - 1 - i]).abs() < 1e-7, "compose relief kernel not symmetric at {i}");
        }
    }

    #[test]
    fn grassland_sigmas_fit_stride() {
        for &sg in &grassland_sigmas() {
            let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
            assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
        }
    }

    #[test]
    fn grassland_sigmas_cover_all_pipeline_blurs() {
        // every sigma schedule_grassland asks for must be present (kparams panics otherwise).
        // The schedule's gauss(...) calls + flow_channels(power, 2.1) pre-blur(1.15)/spread(2.1).
        let smoothing_px = 3.7_f64;
        let floor_smooth = smoothing_px.max(0.5);
        let draw_spread = 2.1_f64.max(0.1);
        let s = grassland_sigmas();
        for need in [
            smoothing_px, 5.2_f64, 1.55, 1.4, 1.15, draw_spread, floor_smooth, 1.1,
        ] {
            assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
        }
    }

    #[test]
    fn biome_sigmas_known_biomes() {
        assert!(biome_sigmas("mountain").is_some());
        assert!(biome_sigmas("grassland").is_some());
        assert!(biome_sigmas("desert").is_some());
        assert!(biome_sigmas("coast").is_some());
        assert!(biome_sigmas("wetland").is_some());
        assert!(biome_sigmas("tundra").is_some());
        assert!(biome_sigmas("nope").is_none());
    }

    #[test]
    fn wetland_sigmas_fit_stride() {
        for &sg in &wetland_sigmas() {
            let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
            assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
        }
    }

    #[test]
    fn wetland_sigmas_cover_all_pipeline_blurs() {
        // every sigma schedule_wetland asks for must be present (kparams panics otherwise).
        // The schedule's gauss(...)/gauss_pool(...) calls + flow_channels(power, 1.8)
        // pre-blur(1.15)/spread(1.8). Levee DoG uses 2.2 and 5.2; flat_base uses smoothing_px=4.4.
        let smoothing_px = 4.4_f64;
        let flow_spread = 1.8_f64.max(0.1);
        let s = wetland_sigmas();
        for need in [5.8_f64, 5.2, 1.15, flow_spread, 2.2, smoothing_px, 1.2] {
            assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
        }
    }

    #[test]
    fn pool_slots_matches_wetland_pool_map() {
        // wetland's biome_wetland.glsl uses pool0..pool10 (11 slots). POOL_SLOTS must cover it.
        assert!(POOL_SLOTS >= 11, "POOL_SLOTS {POOL_SLOTS} < wetland's 11 pool slots");
    }

    #[test]
    fn coast_sigmas_fit_stride() {
        for &sg in &coast_sigmas() {
            let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
            assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
        }
    }

    #[test]
    fn coast_sigmas_cover_all_pipeline_blurs() {
        // every sigma schedule_coast asks for must be present (kparams panics otherwise).
        // The schedule's gauss(...)/gauss_pool(...) calls + flow_channels(power, 1.9)
        // pre-blur(1.15)/spread(1.9).
        let channel_spread = 1.9_f64.max(0.1);
        let s = coast_sigmas();
        for need in [1.15_f64, channel_spread, 2.0, 3.0, 0.9] {
            assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
        }
    }

    #[test]
    fn pool_slots_matches_coast_pool_map() {
        // coast's biome_coast.glsl uses pool0..pool15 (16 slots, pool12 reused). POOL_SLOTS covers it.
        assert!(POOL_SLOTS >= 16, "POOL_SLOTS {POOL_SLOTS} < coast's 16 pool slots");
    }

    #[test]
    fn pool_slots_matches_grassland_pool_map() {
        // grassland's biome_grassland.glsl uses pool0..pool11 (12 slots). POOL_SLOTS must cover it.
        assert!(POOL_SLOTS >= 12, "POOL_SLOTS {POOL_SLOTS} < grassland's 12 pool slots");
    }

    #[test]
    fn pool_slots_matches_desert_pool_map() {
        // desert's biome_desert.glsl uses pool0..pool15 (16 slots). POOL_SLOTS must cover it.
        assert!(POOL_SLOTS >= 16, "POOL_SLOTS {POOL_SLOTS} < desert's 16 pool slots");
    }

    #[test]
    fn desert_sigmas_fit_stride() {
        for &sg in &desert_sigmas() {
            let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
            assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
        }
    }

    #[test]
    fn desert_sigmas_cover_all_pipeline_blurs() {
        // every sigma schedule_desert asks for must be present (kparams panics otherwise).
        // The schedule's gauss(...)/gauss_pool(...) calls + flow_channels(power, 1.8)
        // pre-blur(1.15)/spread(1.8).
        let floor_smooth = 5.2_f64.max(0.2);
        let wash_spread = 1.8_f64.max(0.1);
        let s = desert_sigmas();
        for need in [
            6.2_f64, 5.0, 0.70, 3.2, 2.2, 1.15, wash_spread, floor_smooth, 0.95,
        ] {
            assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
        }
    }

    #[test]
    fn tundra_sigmas_fit_stride() {
        for &sg in &tundra_sigmas() {
            let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
            assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
        }
    }

    #[test]
    fn tundra_sigmas_cover_all_pipeline_blurs() {
        // every sigma schedule_tundra asks for must be present (kparams panics otherwise).
        // The schedule's gauss(...)/gauss_pool(...) calls + flow_channels(power, 2.0)
        // pre-blur(1.15)/spread(2.0). plain=5.8, pattern=1.2, fringe=1.8, base=smoothing_px=5.0,
        // final=1.1.
        let smoothing_px = 5.0_f64;
        let flow_spread = 2.0_f64.max(0.1);
        let s = tundra_sigmas();
        for need in [5.8_f64, 1.2, 1.8, 1.15, flow_spread, smoothing_px, 1.1] {
            assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
        }
    }

    #[test]
    fn pool_slots_matches_tundra_pool_map() {
        // tundra's biome_tundra.glsl uses pool0..pool12 (13 slots). POOL_SLOTS must cover it.
        assert!(POOL_SLOTS >= 13, "POOL_SLOTS {POOL_SLOTS} < tundra's 13 pool slots");
    }

    #[test]
    fn glacial_sigmas_fit_stride() {
        for &sg in &glacial_sigmas() {
            let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
            assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
        }
    }

    #[test]
    fn glacial_sigmas_cover_all_pipeline_blurs() {
        // every sigma schedule_glacial asks for must be present (kparams panics otherwise).
        // The schedule's gauss(...)/gauss_pool(...) calls + flow_channels_ex(power, width, 1.85)
        // PRE-BLUR(1.85)/spread(width). GLACIAL DIVERGENCE: pre-blur is 1.85 (NOT the shared 1.15),
        // so 1.85 MUST be covered (the machine-hook the whole port hangs on).
        let trough_width_px = 6.8_f64;
        let axial_sigma = (trough_width_px * 0.18).max(0.8);   // 1.224
        let primary_spread = trough_width_px.max(0.1);          // 6.8
        let trib_spread = (trough_width_px * 0.48).max(0.8).max(0.1); // 3.264
        let ice_smooth_px = 6.2_f64;
        let floor = ice_smooth_px.max(0.2);                     // 6.2
        let ice_smooth = (ice_smooth_px * 0.65).max(0.2);       // 4.03
        let s = glacial_sigmas();
        for need in [
            1.25_f64, 5.8, 7.0, 2.8, 1.85, axial_sigma, 1.6, trib_spread, primary_spread,
            floor, ice_smooth, 1.35,
        ] {
            assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
        }
        // The custom pre-blur 1.85 must be present AND distinct from the shared 1.15 (the proven
        // biomes' pre-blur), proving glacial's flow_channels_ex hook is wired, not the default.
        assert!(s.iter().any(|&v| (v - 1.85).abs() < 1e-9), "glacial pre-blur 1.85 missing");
        assert!(!s.iter().any(|&v| (v - 1.15).abs() < 1e-9), "glacial must NOT use the shared 1.15 pre-blur");
    }

    #[test]
    fn pool_slots_matches_glacial_pool_map() {
        // glacial's biome_glacial.glsl uses pool0..pool15 (16 slots; pool15 transient,
        // pool10/pool11/pool7 reused post-mask). POOL_SLOTS must cover it.
        assert!(POOL_SLOTS >= 16, "POOL_SLOTS {POOL_SLOTS} < glacial's 16 pool slots");
    }

    #[test]
    fn glacial_sigmas_is_known_biome() {
        assert!(biome_sigmas("glacial").is_some());
    }

    #[test]
    fn karst_sigmas_fit_stride() {
        for &sg in &karst_sigmas() {
            let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
            assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
        }
    }

    #[test]
    fn karst_sigmas_cover_all_pipeline_blurs() {
        // every sigma schedule_karst asks for must be present (kparams panics otherwise).
        // The schedule's gauss(...)/gauss_pool(...) calls + flow_channels(power, 2.6) pre-blur(1.15)/
        // spread(2.6). KARST uses the SHARED flow_channels (pre-blur 1.15), NOT the glacial-style
        // flow_channels_ex hook -- its "custom" flow is just power=0.54, width=2.6 (the spread sigma
        // is the existing width param). plateau=5.8, towers=2.0, dolines=2.6, cellular=3.8,
        // floor=2.8, final=0.95.
        let tower_width = 2.0_f64.max(0.2);     // 2.0
        let doline_width = 2.6_f64.max(0.2);    // 2.6
        let dv_spread = 2.6_f64.max(0.1);       // 2.6 (dedups against doline_width)
        let floor_smooth = 2.8_f64.max(0.2);    // 2.8
        let s = karst_sigmas();
        for need in [
            5.8_f64, tower_width, doline_width, 3.8, 1.15, dv_spread, floor_smooth, 0.95,
        ] {
            assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
        }
        // KARST uses the SHARED pre-blur 1.15 (NOT a glacial-style custom pre-blur). Assert it is
        // present, proving the dry-valley flow rides the proven flow_channels() path.
        assert!(s.iter().any(|&v| (v - 1.15).abs() < 1e-9), "karst shared pre-blur 1.15 missing");
    }

    #[test]
    fn pool_slots_matches_karst_pool_map() {
        // karst's biome_karst.glsl uses pool0..pool15 (16 slots; pool15 transient -> lineament_mask,
        // pool2/pool7 reused for fine/karren post-base). POOL_SLOTS must cover it.
        assert!(POOL_SLOTS >= 16, "POOL_SLOTS {POOL_SLOTS} < karst's 16 pool slots");
    }

    #[test]
    fn karst_sigmas_is_known_biome() {
        assert!(biome_sigmas("karst").is_some());
    }

    #[test]
    fn temperate_sigmas_fit_stride() {
        for &sg in &temperate_sigmas() {
            let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
            assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
        }
    }

    #[test]
    fn temperate_sigmas_cover_all_pipeline_blurs() {
        // every sigma schedule_temperate asks for must be present (kparams panics otherwise).
        // The schedule's gauss(...)/gauss_pool(...) calls + flow_discharge(power=0.43) PRE-BLUR(1.15)
        // and the TWO independent spreads (1.8 for valleys, 4.2 for broad_valleys). TEMPERATE uses
        // the RAW-discharge flow_discharge (NO single trailing spread); the two spreads ARE the
        // distinct sigmas. ridges=1.1, hills=2.4, upland/broad_valleys=4.2, valleys/rounded=1.8,
        // final=1.0.
        let smoothing_px = 1.8_f64.max(0.2); // rounded blur (dedups against valleys spread 1.8)
        let s = temperate_sigmas();
        for need in [1.0_f64, 1.1, 2.4, 4.2, 1.15, 1.8, smoothing_px] {
            assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
        }
        // TEMPERATE uses the SHARED pre-blur 1.15 (NOT a glacial-style custom pre-blur). Assert it
        // is present, proving the valley flow rides the proven flow_discharge(.., 1.15) prefix.
        assert!(s.iter().any(|&v| (v - 1.15).abs() < 1e-9), "temperate shared pre-blur 1.15 missing");
        // The TWO spread sigmas (1.8 and 4.2) MUST BOTH be present AND distinct -- that is the
        // two-spread crux of the temperate port (one raw discharge, spread twice).
        assert!(s.iter().any(|&v| (v - 1.8).abs() < 1e-9), "temperate valleys spread 1.8 missing");
        assert!(s.iter().any(|&v| (v - 4.2).abs() < 1e-9), "temperate broad_valleys spread 4.2 missing");
    }

    #[test]
    fn pool_slots_matches_temperate_pool_map() {
        // temperate's biome_temperate.glsl uses pool0..pool11 (12 slots). POOL_SLOTS must cover it.
        assert!(POOL_SLOTS >= 12, "POOL_SLOTS {POOL_SLOTS} < temperate's 12 pool slots");
    }

    #[test]
    fn temperate_sigmas_is_known_biome() {
        assert!(biome_sigmas("temperate").is_some());
    }

    #[test]
    fn rainforest_sigmas_fit_stride() {
        for &sg in &rainforest_sigmas() {
            let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
            assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
        }
    }

    #[test]
    fn rainforest_sigmas_cover_all_pipeline_blurs() {
        // every sigma schedule_rainforest asks for must be present (kparams panics otherwise).
        // The schedule's gauss(...)/gauss_pool(...) calls + flow_discharge(power=0.38) PRE-BLUR(1.15)
        // and the TWO independent spreads (1.15 for tributaries, 2.2 for trunk). RAINFOREST uses the
        // RAW-discharge flow_discharge (NO single trailing spread); the two spreads ARE the distinct
        // sigmas. hills=1.7, plateau=4.5, lowland=5.4, wet_rounding=smoothing_px=2.6, final=1.0.
        let smoothing_px = 2.6_f64.max(0.2); // wet_rounding blur (dedups against the listed 2.6)
        let s = rainforest_sigmas();
        for need in [1.0_f64, 1.15, 1.7, 2.2, 2.6, 4.5, 5.4, smoothing_px] {
            assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
        }
        // RAINFOREST uses the SHARED pre-blur 1.15 (NOT a glacial-style custom pre-blur). Assert it
        // is present, proving the drainage rides the proven flow_discharge(.., 1.15) prefix.
        assert!(s.iter().any(|&v| (v - 1.15).abs() < 1e-9), "rainforest shared pre-blur 1.15 missing");
        // The TWO spread sigmas (1.15 and 2.2) MUST BOTH be present -- that is the dual-mask crux of
        // the rainforest port (one raw discharge, spread twice). The tributaries spread (1.15) dedups
        // against the shared pre-blur; the trunk spread (2.2) is its own distinct slot.
        assert!(s.iter().any(|&v| (v - 2.2).abs() < 1e-9), "rainforest trunk spread 2.2 missing");
    }

    #[test]
    fn pool_slots_matches_rainforest_pool_map() {
        // rainforest's biome_rainforest.glsl uses pool0..pool11 (12 slots; pool3/pool4/pool7 reused
        // for plateau/hills/drainage). POOL_SLOTS must cover it.
        assert!(POOL_SLOTS >= 12, "POOL_SLOTS {POOL_SLOTS} < rainforest's 12 pool slots");
    }

    #[test]
    fn rainforest_sigmas_is_known_biome() {
        assert!(biome_sigmas("rainforest").is_some());
    }

    #[test]
    fn volcanic_sigmas_fit_stride() {
        for &sg in &volcanic_sigmas() {
            let len = 2 * gaussian_radius(sg, TRUNCATE) + 1;
            assert!(len <= KERNEL_STRIDE, "sigma {sg} kernel len {len} > {KERNEL_STRIDE}");
        }
    }

    #[test]
    fn volcanic_sigmas_cover_all_pipeline_blurs() {
        // every sigma schedule_volcanic asks for must be present (kparams panics otherwise).
        // flows blur=1.1 ; gully flow_discharge PRE-BLUR(1.15) + FIXED spread(1.2) ;
        // caldera spc_blur=2.6 ; ash max_cf_blur=3.0 ; smoothed_plain=2.6 (dedups) ; final=0.85.
        let s = volcanic_sigmas();
        for need in [0.85_f64, 1.1, 1.15, 1.2, 2.6, 3.0] {
            assert!(s.iter().any(|&v| (v - need).abs() < 1e-9), "missing sigma {need}");
        }
        // VOLCANIC uses the SHARED pre-blur 1.15 (flow_discharge prefix), NOT a glacial-style custom
        // pre-blur -- assert it is present.
        assert!(s.iter().any(|&v| (v - 1.15).abs() < 1e-9), "volcanic shared pre-blur 1.15 missing");
        // The gully spread is a FIXED 1.2 (the gully_channels_seam_safe spread, NOT the flow width),
        // and is distinct from the pre-blur -- assert it is present.
        assert!(s.iter().any(|&v| (v - 1.2).abs() < 1e-9), "volcanic gully spread 1.2 missing");
    }

    #[test]
    fn pool_slots_matches_volcanic_pool_map() {
        // volcanic's biome_volcanic.glsl uses pool0..pool15 (16 slots; pool15 transient -> raw flows,
        // then REUSED for max_cf_blur). POOL_SLOTS must cover it.
        assert!(POOL_SLOTS >= 16, "POOL_SLOTS {POOL_SLOTS} < volcanic's 16 pool slots");
    }

    #[test]
    fn volcanic_sigmas_is_known_biome() {
        assert!(biome_sigmas("volcanic").is_some());
    }

    #[test]
    fn volcanic_vent_count_fits_max_vents() {
        // The CPU vent packing for the fixture seeds must produce vent_count <= MAX_VENTS (so the
        // packed buffer is never truncated). STYLES[0] (stratovolcano_cluster) draws vent_count=4.
        use crate::recipes_volcanic::volcanic;
        for &seed in &[0_i64, 7] {
            let (packed, count) = volcanic::packed_vents(&volcanic::STRATOVOLCANO_CLUSTER, seed, 60000.0);
            assert!(count <= volcanic::MAX_VENTS, "seed {seed}: vent_count {count} > MAX_VENTS");
            assert_eq!(count, 4, "stratovolcano_cluster vent_count should be 4 (got {count})");
            assert_eq!(packed.len(), volcanic::MAX_VENTS * volcanic::VENT_STRIDE);
        }
    }
}
