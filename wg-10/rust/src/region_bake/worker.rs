//! Async region-bake worker: a dedicated thread owning its own RenderingDevice (per-thread; never
//! shared with the pool's RD). Super-region bake requests in via one channel; finished sliced region
//! facts out via another. The worker does: GPU super-macro readback -> bake_super_region (carve +
//! condition over the whole super-field, seam-exact internally) -> slice -> send k*k region grids.
#![allow(dead_code)]
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;

use crate::pass_network::{PassNetworkParams, RampParams, TraverseParams};
use crate::region_bake::{bake_super_region, gpu_macro_region, SmoothFieldPercentiles};

/// One super-region bake request. `super_key` identifies the super-region (its lower-left region
/// coords or a super-grid index — the pool decides). All sizes are in the super-region's terms.
pub struct SuperBakeRequest {
    pub super_key: (i64, i64),
    pub region_n: usize, // single region grid side
    pub k: usize,        // super-region is k*k regions
    pub apron_px: usize,
    pub flow_iters: usize,
    pub flow_on: bool,
    pub feature_span_m: f64,
    pub region_span_m: f64, // ONE region's world span
    pub spacing_m: f64,     // = region_span_m / (region_n - 1)
    pub height_scale_m: f64,
    pub super_x0_m: f64, // super-region world origin (the PADDED origin convention gpu_macro_region expects)
    pub super_z0_m: f64,
    pub seed: i64,
    pub pass: PassNetworkParams,
    pub traverse: TraverseParams,
    pub ramp: RampParams,
    // coarse percentile params (SmoothFieldPercentiles): keep simple, derived from region_span_m.
    pub coarse_stride_m: f64,
    pub window_radius_m: f64,
    pub window_samples: usize,
}

/// One baked region sliced from the super-region: the conditioned grid (already scaled to METRES) +
/// world bounds. (The pool builds a RegionFactRuntime from this.)
pub struct BakedRegionFact {
    pub region_origin_x_m: f64,
    pub region_origin_z_m: f64,
    pub region_span_m: f64,
    pub grid_n: usize,
    pub grid_m: Vec<f32>, // conditioned height * height_scale_m, f32, row-major n*n
}

pub struct SuperBakeResult {
    pub super_key: (i64, i64),
    pub result: Result<Vec<BakedRegionFact>, String>, // k*k region facts, or an error
}

pub struct BakeWorker {
    pub tx: Option<Sender<SuperBakeRequest>>,
    pub rx: Receiver<SuperBakeResult>,
    handle: Option<JoinHandle<()>>,
}

impl BakeWorker {
    /// Spawn the worker with the three GLSL sources the GPU macro needs.
    pub fn spawn(primitives: String, machine: String, fragment: String) -> Self {
        let (req_tx, req_rx) = std::sync::mpsc::channel::<SuperBakeRequest>();
        let (out_tx, out_rx) = std::sync::mpsc::channel::<SuperBakeResult>();
        let handle = std::thread::Builder::new()
            .name("wg10-region-bake".into())
            .spawn(move || worker_loop(req_rx, out_tx, primitives, machine, fragment))
            .expect("spawn wg10 region-bake worker");
        Self {
            tx: Some(req_tx),
            rx: out_rx,
            handle: Some(handle),
        }
    }
}

impl Drop for BakeWorker {
    fn drop(&mut self) {
        // Close the request channel FIRST (drop the sole Sender) so the worker's rx.recv() returns
        // Err and the loop exits; THEN join. Joining while tx is still alive would deadlock (the
        // worker blocks in recv() forever waiting for the sender that drop() is waiting to join past).
        drop(self.tx.take());
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn worker_loop(
    rx: Receiver<SuperBakeRequest>,
    tx: Sender<SuperBakeResult>,
    primitives: String,
    machine: String,
    fragment: String,
) {
    while let Ok(req) = rx.recv() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bake_one_super(&req, &primitives, &machine, &fragment)
        }))
        .unwrap_or_else(|_| Err("super-region bake panicked".into()));
        if tx
            .send(SuperBakeResult {
                super_key: req.super_key,
                result,
            })
            .is_err()
        {
            break; // pool gone
        }
    }
}

fn bake_one_super(
    req: &SuperBakeRequest,
    prim: &str,
    machine: &str,
    frag: &str,
) -> Result<Vec<BakedRegionFact>, String> {
    let super_n = req.k * (req.region_n - 1) + 1;
    // 1. GPU super-macro readback over the whole super-field (core_px = super_n).
    let super_raw = gpu_macro_region(
        prim,
        machine,
        frag,
        req.super_x0_m,
        req.super_z0_m,
        req.spacing_m,
        super_n,
        req.apron_px,
        req.flow_iters,
        req.feature_span_m,
        req.seed,
        req.flow_on,
    )?;
    if super_raw.len() != super_n * super_n {
        return Err(format!(
            "super-macro readback {} != {}",
            super_raw.len(),
            super_n * super_n
        ));
    }
    // 2. Seam-exact smooth percentile provider over the super-field (sampler reads the readback).
    //    Core origin offset: gpu_macro_region offsets the core inward by apron; the conditioned
    //    field's world origin is super_x0 + apron*spacing (matching gpu_macro_region's core origin).
    let core_x0 = req.super_x0_m + req.apron_px as f64 * req.spacing_m;
    let core_z0 = req.super_z0_m + req.apron_px as f64 * req.spacing_m;
    let super_span_m = req.region_span_m * req.k as f64;
    // The sampler owns its OWN clone of super_raw so the move closure does not conflict with
    // bake_super_region's &super_raw borrow (off-frame bake — the clone is acceptable).
    let sampler_grid = super_raw.clone();
    let sampler = move |wx: f64, wz: f64| -> f64 {
        // bilinear sample of the super_raw grid at world (wx,wz), clamped to the super-field.
        let u = ((wx - core_x0) / super_span_m).clamp(0.0, 1.0);
        let v = ((wz - core_z0) / super_span_m).clamp(0.0, 1.0);
        let gx = u * (super_n as f64 - 1.0);
        let gz = v * (super_n as f64 - 1.0);
        let x0 = gx.floor() as usize;
        let z0 = gz.floor() as usize;
        let x1 = (x0 + 1).min(super_n - 1);
        let z1 = (z0 + 1).min(super_n - 1);
        let tx = gx - x0 as f64;
        let tz = gz - z0 as f64;
        let g = &sampler_grid;
        let h00 = g[z0 * super_n + x0];
        let h10 = g[z0 * super_n + x1];
        let h01 = g[z1 * super_n + x0];
        let h11 = g[z1 * super_n + x1];
        let a = h00 + (h10 - h00) * tx;
        let b = h01 + (h11 - h01) * tx;
        a + (b - a) * tz
    };
    let provider = SmoothFieldPercentiles {
        macro_sampler: sampler,
        coarse_stride_m: req.coarse_stride_m,
        window_radius_m: req.window_radius_m,
        window_samples: req.window_samples,
    };
    // 3. Carve + condition over the super-field, slice into k*k region facts.
    let slices = bake_super_region(
        &super_raw,
        super_n,
        req.region_n,
        req.k,
        req.region_span_m,
        req.height_scale_m,
        core_x0,
        core_z0,
        &req.pass,
        &req.traverse,
        &req.ramp,
        &provider,
    );
    // 4. Scale conditioned (tanh) height to METRES; package as BakedRegionFact.
    let mut out = Vec::with_capacity(slices.len());
    for s in slices {
        let grid_m: Vec<f32> = s
            .grid
            .iter()
            .map(|&h| (h * req.height_scale_m) as f32)
            .collect();
        out.push(BakedRegionFact {
            region_origin_x_m: s.origin_x_m,
            region_origin_z_m: s.origin_z_m,
            region_span_m: s.span_m,
            grid_n: s.grid_n,
            grid_m,
        });
    }
    Ok(out)
}
