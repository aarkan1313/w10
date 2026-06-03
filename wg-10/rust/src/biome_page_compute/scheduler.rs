//! Dispatch scheduler for WG10 biome page compute.
//!
//! `Scheduler` owns no buffers; it wraps one open compute list and exposes the proven dispatch
//! vocabulary used by the per-biome schedules and compose/blend engines.

use godot::prelude::*;
use godot::classes::RenderingDevice;

use super::abi::*;
use super::{build_push, KernelParams};

/// Per-dispatch state for one open compute list. Built once `run_inner` has the list open; the
/// schedule fn drives it. `cl` matches the type `compute_list_begin()` returns (i64 in the
/// Godot 4.6 bindings).
pub(crate) struct Scheduler<'a> {
    pub(crate) rd: &'a mut Gd<RenderingDevice>,
    pub(crate) cl: i64,
    pub(crate) uset: Rid,
    pub(crate) rows: i32,
    pub(crate) cols: i32,
    pub(crate) apron: i32,
    pub(crate) seed: i32,
    pub(crate) spacing: f32,
    pub(crate) ox: f32,
    pub(crate) oz: f32,
    pub(crate) feature_span_m: f32,
    /// VOLCANIC: active vents (forwarded into every dispatch's push constant). 0 for the 10
    /// non-volcanic biomes -> their push bytes are byte-identical to the pre-vent layout.
    pub(crate) vent_count: i32,
    /// COMPOSE params, forwarded into every dispatch as pad0/pad1. 0.0/0.0 for the 11 biome
    /// schedules (byte-identical push); the compose schedule sets them from the record's cfg.
    pub(crate) favor_strength: f32,
    pub(crate) relief_confidence_floor: f32,
    /// RUNTIME relief scale (metres): PASS_CROP_IMG multiplies the normalized recipe height by this
    /// before the texture write, so the render shader sees METRES (like legacy `* relief_m`). 0.0 for
    /// the readback test harness (it crops to the BUFFER via PASS_CROP, which ignores pad2 -> the
    /// fixture parity is byte-identical). Set to the configured relief only on the runtime producer.
    pub(crate) relief_m: f32,
    pub(crate) wg_full_x: u32,
    pub(crate) wg_full_y: u32,
    pub(crate) wg_core_x: u32,
    pub(crate) wg_core_y: u32,
    pub(crate) kparams: KernelParams,
    /// Flow PULL-relaxation step count for this run. The recipe page path passes STABLE_ITERS
    /// (so it is byte-identical to the const-loop it replaced); the convergence-measurement entry
    /// (`generate_core_page_iters`) passes a swept value to find the real 576-production
    /// convergence count. Compose/blend engines never run flow -> they set it to STABLE_ITERS too.
    pub(crate) flow_iters: usize,
    /// SCALE-INVARIANCE: enable the drainage carve. `true` reproduces the parity-proven schedule
    /// (both flow_channels passes + the carve run). `false` (coarse clipmap levels) makes
    /// `schedule_mountain` SKIP the two expensive flow_channels passes and instead zero the channel
    /// masks (PASS_ZERO_FLOW_MASKS) -> the MACRO surface, mirroring the CPU oracle's `flow_on==false`
    /// `else` branch. The readback test harnesses (`run_inner` etc.) and every non-mountain schedule
    /// set this `true` so their parity-frozen dispatch sequence is byte-identical to before this flag.
    pub(crate) flow_on: bool,
}

impl<'a> Scheduler<'a> {
    /// One full pass dispatch + trailing barrier (so the next reader sees the writes). Same body
    /// as the old `dispatch` closure, plus the additive `pool_sel` push-constant for the generic
    /// pool passes (0 for every mountain dispatch -> byte-identical to the pre-pool push for
    /// mountain, since pool_sel maps to a former int-pad slot).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dispatch(
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
    pub(crate) fn dispatch_full(&mut self, pass: i32, copy_sel: i32, flow_dir: i32, flow_power: f32) {
        self.dispatch(pass, 0, 0, copy_sel, flow_dir, flow_power, 0, self.wg_full_x, self.wg_full_y);
    }

    /// Full-field POOL copy/stash dispatch (pool_sel selects the slot). Used by COPY_POOL (slot
    /// -> gauss_in) and POOL_FROM_GAUSS (gauss_out -> slot) so a biome can blur ANY pool slot.
    pub(crate) fn dispatch_pool(&mut self, pass: i32, pool_sel: i32) {
        self.dispatch(pass, 0, 0, 0, 0, 0.0, pool_sel, self.wg_full_x, self.wg_full_y);
    }

    /// gaussian(sigma) on gauss_in -> gauss_out (AXIS0 then AXIS1, packed kernel by koffset).
    /// Same body as the old `gauss!` macro.
    pub(crate) fn gauss(&mut self, sigma: f64) {
        let (ko, kr) = self.kparams.kp(sigma);
        self.dispatch(PASS_GAUSS_AXIS0, kr, ko, 0, 0, 0.0, 0, self.wg_full_x, self.wg_full_y);
        self.dispatch(PASS_GAUSS_AXIS1, kr, ko, 0, 0, 0.0, 0, self.wg_full_x, self.wg_full_y);
    }

    /// Blur a scratch-pool slot in place: COPY_POOL(slot) -> gauss_in, gaussian(sigma), then the
    /// blur lives in gauss_out (the caller reads gauss_out, or POOL_FROM_GAUSS stashes it back).
    pub(crate) fn gauss_pool(&mut self, slot: i32, sigma: f64) {
        self.dispatch_pool(PASS_COPY_POOL, slot);
        self.gauss(sigma);
    }

    /// flow_channels_seam_safe(flow_pre, width_px, power): pre-blur 1.15 -> K relax ->
    /// log1p discharge -> spread gaussian(width). Leaves spread discharge in gauss_out.
    /// Same body as the old `flow_channels!` macro (incl STABLE_ITERS loop + discharge_fd
    /// invariant). Thin wrapper over `flow_channels_ex` with the SHARED pre-blur sigma=1.15
    /// (the 6 proven biomes call THIS -> byte-identical dispatch sequence as before the refactor).
    pub(crate) fn flow_channels(&mut self, power: f32, width: f64) {
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
    pub(crate) fn flow_channels_ex(&mut self, power: f32, width: f64, preblur_sigma: f64) {
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
    pub(crate) fn flow_discharge(&mut self, power: f32, preblur_sigma: f64) {
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
    pub(crate) fn compose_relief_a(&mut self) {
        self.dispatch_full(PASS_COMPOSE_COPY_ACC, 0, 0, 0.0);
        self.gauss(COMPOSE_RELIEF_SIGMA);
        self.dispatch_full(PASS_COMPOSE_RELIEF_A_STORE, 0, 0, 0.0);
    }

    /// Compute relief_b = |f - gaussian_nearest(f, 6.0)| into massif. Blurs f (pool0) via
    /// COPY_POOL(0) -> gauss_in, gauss(6.0) -> gauss_out, then the abs-diff.
    pub(crate) fn compose_relief_b(&mut self) {
        self.dispatch_pool(PASS_COPY_POOL, 0);
        self.gauss(COMPOSE_RELIEF_SIGMA);
        self.dispatch_full(PASS_COMPOSE_RELIEF_B_STORE, 0, 0, 0.0);
    }

    /// w_acc = acc_w / (acc_w + w + 1e-12) into lowland (base=acc_w, pool1=w).
    pub(crate) fn compose_wacc(&mut self) {
        self.dispatch_full(PASS_COMPOSE_WACC, 0, 0, 0.0);
    }

    /// FIELD blend step: w_acc in lowland already set. height <- w_acc*height + (1-w_acc)*pool0.
    pub(crate) fn blend_field_step(&mut self) {
        self.dispatch_full(PASS_COMPOSE_BLEND_FIELD, 0, 0, 0.0);
    }

    /// FAVORED blend step: requires relief_a (range_envelope), relief_b (massif), w_acc (lowland)
    /// pre-computed. favor_strength / relief_confidence_floor ride pad0/pad1 (Scheduler fields).
    pub(crate) fn blend_favored_step(&mut self) {
        self.compose_relief_a();
        self.compose_relief_b();
        self.dispatch_full(PASS_COMPOSE_BLEND_FAVORED, 0, 0, 0.0);
    }

    /// acc_w += w  (base += pool1).
    pub(crate) fn compose_accw_add(&mut self) {
        self.dispatch_full(PASS_COMPOSE_ACCW_ADD, 0, 0, 0.0);
    }
}
