//! RegionFact producer plumbing for `Wg10PagePool`: the async super-region bake worker drain,
//! the enqueue-once super-region mapping, and the never-black flat fallback. The carve+condition
//! happens off-frame on the worker thread (its own RenderingDevice); the pool only (a) drains
//! finished region facts into a per-region cache and (b) enqueues the super-region a page needs.
//!
//! SUPER-KEY ARITHMETIC (load-bearing — verified against region_bake::worker::bake_one_super):
//!   * A page at world (x,z) lives in region cell `region_of(x,z) = (rx,rz)`.
//!   * The super-region containing region (rx,rz) with super size k is super-key
//!     `(rx.div_euclid(k), rz.div_euclid(k))`.
//!   * `gpu_macro_region` treats the request's `super_x0_m` as the PADDED origin and offsets the
//!     conditioned core inward by `apron*spacing`: `core_x0 = super_x0 + apron*spacing`.
//!   * `bake_super_region` slices region (gi,gj) at world origin `core_x0 + gi*region_span_m`.
//!   * We want region (skx*k+gi)'s world origin to land on the true grammar grid:
//!     `(skx*k+gi)*region_size_m`. With `region_span_m == region_size_m` this forces
//!     `core_x0 = skx*k*region_size_m`, hence
//!     `super_x0_m = skx*k*region_size_m - apron*spacing`.

use godot::classes::RenderingDevice;
use godot::prelude::*;

use crate::biome_page_compute::f32s_to_bytes;
use crate::region_bake::SuperBakeRequest;

use super::region_fact::RegionFactRuntime;
use super::Wg10PagePool;

impl Wg10PagePool {
    /// Drain all finished super-region bakes from the worker into the region cache. Borrow-safe:
    /// collect every `try_recv` result into a local Vec FIRST (immutable borrow of the worker),
    /// THEN mutate the cache/in-flight set (no overlapping borrow of `self`).
    pub(in crate::page_pool) fn drain_region_bakes(&mut self) {
        let mut done = Vec::new();
        if let Some(worker) = self.region_worker.as_ref() {
            while let Ok(result) = worker.rx.try_recv() {
                done.push(result);
            }
        }
        if done.is_empty() {
            return;
        }
        let region_size_m = match self.pack.as_ref() {
            Some(p) => p.grammar_constants.region_size_m,
            None => return,
        };
        let half = region_size_m * 0.5;
        for r in done {
            self.region_baking.remove(&r.super_key);
            match r.result {
                Ok(facts) => {
                    for fact in facts {
                        // Region cell from the fact's world origin (+half a cell to dodge the
                        // floor boundary; the origin lands on an exact region multiple).
                        let pack = match self.pack.as_ref() {
                            Some(p) => p,
                            None => return,
                        };
                        let (rx, rz) = crate::grammar::region_of(
                            fact.region_origin_x_m + half,
                            fact.region_origin_z_m + half,
                            pack,
                        );
                        let runtime = RegionFactRuntime::new(
                            fact.grid_m,
                            fact.grid_n,
                            fact.region_origin_x_m,
                            fact.region_origin_z_m,
                            fact.region_span_m,
                            fact.region_span_m,
                        );
                        self.region_cache.insert((rx, rz), runtime);
                    }
                }
                Err(msg) => {
                    let (skx, skz) = r.super_key;
                    godot_error!(
                        "Wg10PagePool: super-region bake ({skx},{skz}) failed: {msg}"
                    );
                    // No tight retry: super_key already removed from `region_baking` above; a later
                    // acquire that still misses the cache will re-enqueue.
                }
            }
        }
    }

    /// Enqueue the super-region containing region (rx,rz) for baking, once. No-op if that
    /// super-region is already baking (in flight) or the region is already cached.
    pub(in crate::page_pool) fn ensure_super_baked(&mut self, rx: i64, rz: i64) {
        let cfg = match self.region_cfg.as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        let k = cfg.k as i64;
        let super_key = (rx.div_euclid(k), rz.div_euclid(k));
        if self.region_baking.contains(&super_key) {
            return;
        }
        let (skx, skz) = super_key;
        // PADDED super origin (see module-level arithmetic note): core lands on the true region grid.
        let super_x0_m = (skx * k) as f64 * cfg.region_size_m - cfg.apron_px as f64 * cfg.spacing_m;
        let super_z0_m = (skz * k) as f64 * cfg.region_size_m - cfg.apron_px as f64 * cfg.spacing_m;

        let req = SuperBakeRequest {
            super_key,
            region_n: cfg.region_n,
            k: cfg.k,
            apron_px: cfg.apron_px,
            flow_iters: cfg.flow_iters,
            flow_on: cfg.flow_on,
            feature_span_m: cfg.feature_span_m,
            region_span_m: cfg.region_span_m,
            spacing_m: cfg.spacing_m,
            height_scale_m: cfg.height_scale_m,
            super_x0_m,
            super_z0_m,
            seed: cfg.seed,
            pass: cfg.pass,
            traverse: cfg.traverse,
            ramp: cfg.ramp,
            coarse_stride_m: cfg.coarse_stride_m,
            window_radius_m: cfg.window_radius_m,
            window_samples: cfg.window_samples,
        };

        let sent = self
            .region_worker
            .as_ref()
            .and_then(|w| w.tx.as_ref())
            .map(|tx| tx.send(req).is_ok())
            .unwrap_or(false);
        if sent {
            self.region_baking.insert(super_key);
        } else {
            godot_error!(
                "Wg10PagePool: region bake worker unavailable; cannot enqueue super ({skx},{skz})"
            );
        }
    }

    /// Write a flat (all-zero) page so the screen is never black before the bake lands. Single-owner
    /// discipline: only `texture_update` on a pool-owned RID (never create/free here).
    pub(in crate::page_pool) fn write_flat_fallback_page(
        &self,
        rd: &mut Gd<RenderingDevice>,
        tex_rid: Rid,
        page_px: i64,
    ) -> Result<(), String> {
        if page_px < 2 {
            return Err(format!("region fact fallback: page_px {page_px} must be >= 2"));
        }
        let n = page_px as usize;
        let samples = vec![0.0_f32; n * n];
        let bytes = f32s_to_bytes(&samples);
        let pba = PackedByteArray::from(bytes.as_slice());
        let err = rd.texture_update(tex_rid, 0, &pba);
        if err != godot::global::Error::OK {
            return Err(format!("region fact fallback: texture_update failed: {err:?}"));
        }
        Ok(())
    }

    /// Reset all RegionFact producer state to unconfigured. Dropping the worker closes its request
    /// channel and joins the thread (see `BakeWorker::drop`).
    pub(in crate::page_pool) fn clear_region_fact_state(&mut self) {
        self.region_worker = None;
        self.region_cache.clear();
        self.region_baking.clear();
        self.region_cfg = None;
    }
}
