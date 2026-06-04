//! Runtime apron buffer ownership and allocation for biome page compute.

use godot::classes::RenderingDevice;
use godot::prelude::*;

use super::abi::{POOL_SLOTS, TRUNCATE};
use super::helpers::f32s_to_bytes;
use super::kernels::{gaussian_kernel1d, KERNEL_STRIDE};
use super::sigma_registry::{biome_sigmas, compose_sigmas, KernelParams};

/// All the per-page-INVARIANT apron working-grid buffers the machine's uniform set binds
/// (the 19 named fields 0..18, the packed kernel buffer 19, flow_pre/acc_a/acc_b 20..22, the core
/// 23, the POOL_SLOTS pool buffers 24..24+SLOTS-1, and the vent buffer 40). Allocated ONCE by
/// `alloc_apron_buffers` and owned by `BiomePageComputeContext`. `kparams` resolves each sigma to
/// its packed-kernel (koffset, kradius). The buffer ROLES + bindings are byte-identical to what
/// `run_inner` allocates inline (same sizes, same zero-init, same kernel packing).
pub(super) struct ApronBuffers {
    /// bindings 0..=18 (wx, wz, regional, ranges, ridge_detail, near_detail, range_envelope,
    /// lowland, massif, base, primary_mask, tributary_mask, high_mask, valley_mask, height,
    /// floor_mask, gauss_in, gauss_mid, gauss_out) -- the 19 fixed named fields, in binding order.
    fields: Vec<Rid>,
    pub(super) kernel: Rid, // binding 19 (packed gaussian kernels at slot*KERNEL_STRIDE)
    flow_pre: Rid,          // binding 20
    acc_a: Rid,             // binding 21
    acc_b: Rid,             // binding 22
    core: Rid, // binding 23 (storage; schedule_mountain's trailing PASS_CROP writes it, inert)
    pool: Vec<Rid>, // bindings 24..24+POOL_SLOTS-1
    vents: Rid, // binding 40
    pub(super) vent_count: i32,
    pub(super) kparams: KernelParams,
}

impl ApronBuffers {
    /// (binding, rid) pairs for the WHOLE machine uniform set EXCEPT the runtime output image
    /// (binding 41). Same binding map `run_inner` builds. The runtime uniform set appends the
    /// image; the test harness (run_inner) does not (and never dispatches PASS_CROP_IMG).
    pub(super) fn buffer_bindings(&self) -> Vec<(i32, Rid)> {
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

    pub(super) fn field_rid(&self, binding: usize) -> Rid {
        debug_assert!(binding < self.fields.len(), "field binding out of range");
        self.fields[binding]
    }

    pub(super) fn core_rid(&self) -> Rid {
        self.core
    }

    pub(super) fn pool_rid(&self, slot: usize) -> Rid {
        debug_assert!(slot < self.pool.len(), "pool slot out of range");
        self.pool[slot]
    }

    /// Free every RID this owns. The B1 RID-leak lesson: miss none (19 fields + kernel +
    /// flow_pre/acc_a/acc_b + core + POOL_SLOTS pool + vents).
    pub(super) fn free(&self, rd: &mut Gd<RenderingDevice>) {
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
pub(super) fn alloc_apron_buffers(
    rd: &mut Gd<RenderingDevice>,
    rows: usize,
    cols: usize,
    core_n: usize,
    biome: &str,
    seed: i32,
    feature_span_m: f32,
) -> Result<ApronBuffers, String> {
    let sigmas = match biome_sigmas(biome) {
        Some(s) => s,
        None => {
            return Err(format!(
                "no sigma list for biome '{biome}' (add a biome_sigmas arm)"
            ))
        }
    };
    let (vent_packed, vent_count): (Vec<f32>, usize) = if biome == "volcanic" {
        crate::recipes_volcanic::volcanic::packed_vents(
            &crate::recipes_volcanic::volcanic::STRATOVOLCANO_CLUSTER,
            seed as i64,
            feature_span_m as f64,
        )
    } else {
        zero_vent_buffer()
    };
    alloc_apron_buffers_with_sigmas(rd, rows, cols, core_n, &sigmas, vent_packed, vent_count)
}

pub(super) fn alloc_compose_buffers(
    rd: &mut Gd<RenderingDevice>,
    rows: usize,
    cols: usize,
    core_n: usize,
) -> Result<ApronBuffers, String> {
    let sigmas = compose_sigmas();
    let (vent_packed, vent_count) = zero_vent_buffer();
    alloc_apron_buffers_with_sigmas(rd, rows, cols, core_n, &sigmas, vent_packed, vent_count)
}

fn zero_vent_buffer() -> (Vec<f32>, usize) {
    let stride = crate::recipes_volcanic::volcanic::VENT_STRIDE;
    let maxv = crate::recipes_volcanic::volcanic::MAX_VENTS;
    (vec![0.0_f32; maxv * stride], 0)
}

fn alloc_apron_buffers_with_sigmas(
    rd: &mut Gd<RenderingDevice>,
    rows: usize,
    cols: usize,
    core_n: usize,
    sigmas: &[f64],
    vent_packed: Vec<f32>,
    vent_count: usize,
) -> Result<ApronBuffers, String> {
    let n = rows * cols;
    let bsize = |len: usize| -> u32 { u32::try_from(len).expect("buffer size exceeds u32") };
    let field_bytes = n * 4;
    let zeros = vec![0.0_f32; n];
    let zeros_pba = PackedByteArray::from(f32s_to_bytes(&zeros).as_slice());
    let mk_field = |rd: &mut Gd<RenderingDevice>| -> Rid {
        rd.storage_buffer_create_ex(bsize(field_bytes))
            .data(&zeros_pba)
            .done()
    };

    // 19 named fields (bindings 0..=18), in the SAME order as run_inner.
    let mut fields: Vec<Rid> = Vec::with_capacity(19);
    for _ in 0..19 {
        fields.push(mk_field(rd));
    }

    // packed kernel buffer (19): all requested sigmas' kernels at slot*KERNEL_STRIDE.
    let helper_free = |rd: &mut Gd<RenderingDevice>, fields: &[Rid]| {
        for &rid in fields {
            rd.free_rid(rid);
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
    let kparams = KernelParams::from_sigmas(sigmas);

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

    // VENT buffer (40): zeroed for non-volcanic biomes and compose.
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
        vent_count: vent_count as i32,
        kparams,
    })
}
