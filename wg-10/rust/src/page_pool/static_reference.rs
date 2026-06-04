//! Static accepted-reference page producer for `Wg10PagePool`.
//!
//! This is deliberately a reference bridge, not a new runtime synthesis path:
//! it streams the owner-accepted generated mountain-network payload through the
//! same page pool and clipmap renderer so renderer behavior can be reviewed
//! independently from the live biome recipe.

use godot::classes::RenderingDevice;
use godot::prelude::*;
use serde::Deserialize;
use std::path::Path;

use crate::biome_page_compute::f32s_to_bytes;

#[derive(Clone)]
pub(super) struct StaticHeightRuntime {
    grid: Vec<f32>,
    grid_n: usize,
    origin_x_m: f64,
    origin_z_m: f64,
    span_x_m: f64,
    span_z_m: f64,
    pub(super) feature_span_m: f64,
}

#[derive(Deserialize)]
struct StaticPayload {
    chunk_count: usize,
    chunk_n: usize,
    chunk_span_m: f64,
    world_span_m: f64,
    height_scale_m: f64,
    feature_span_m: f64,
    seeds: Vec<StaticSeed>,
}

#[derive(Deserialize)]
struct StaticSeed {
    chunks: Vec<StaticChunk>,
}

#[derive(Deserialize)]
struct StaticChunk {
    chunk_x: usize,
    chunk_z: usize,
    n: usize,
    span_m: f64,
    display_origin_x_m: f64,
    display_origin_z_m: f64,
    height: Vec<f32>,
}

impl StaticHeightRuntime {
    pub(super) fn from_json_path(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("static reference: cannot read {path:?}: {e}"))?;
        let payload: StaticPayload = serde_json::from_str(&text)
            .map_err(|e| format!("static reference: invalid json {path:?}: {e}"))?;
        Self::from_payload(payload)
    }

    fn from_payload(payload: StaticPayload) -> Result<Self, String> {
        if payload.chunk_count == 0 {
            return Err("static reference: chunk_count must be > 0".into());
        }
        if payload.chunk_n < 2 {
            return Err("static reference: chunk_n must be >= 2".into());
        }
        let seed = payload
            .seeds
            .first()
            .ok_or("static reference: payload has no seeds")?;
        let expected_chunks = payload
            .chunk_count
            .checked_mul(payload.chunk_count)
            .ok_or("static reference: chunk_count overflow")?;
        if seed.chunks.len() != expected_chunks {
            return Err(format!(
                "static reference: chunks={} expected {}",
                seed.chunks.len(),
                expected_chunks
            ));
        }

        let step = payload.chunk_n - 1;
        let grid_n = payload
            .chunk_count
            .checked_mul(step)
            .and_then(|v| v.checked_add(1))
            .ok_or("static reference: grid_n overflow")?;
        let grid_len = grid_n
            .checked_mul(grid_n)
            .ok_or("static reference: grid size overflow")?;
        let mut grid = vec![0.0_f32; grid_len];
        let mut filled = vec![false; grid_len];

        let mut min_x = f64::INFINITY;
        let mut min_z = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_z = f64::NEG_INFINITY;

        for chunk in &seed.chunks {
            if chunk.n != payload.chunk_n {
                return Err(format!(
                    "static reference: chunk ({},{}) n={} expected {}",
                    chunk.chunk_x, chunk.chunk_z, chunk.n, payload.chunk_n
                ));
            }
            let expected_heights = payload
                .chunk_n
                .checked_mul(payload.chunk_n)
                .ok_or("static reference: height len overflow")?;
            if chunk.height.len() != expected_heights {
                return Err(format!(
                    "static reference: chunk ({},{}) heights={} expected {}",
                    chunk.chunk_x,
                    chunk.chunk_z,
                    chunk.height.len(),
                    expected_heights
                ));
            }
            if (chunk.span_m - payload.chunk_span_m).abs() > 1.0e-6 {
                return Err(format!(
                    "static reference: chunk ({},{}) span={} expected {}",
                    chunk.chunk_x, chunk.chunk_z, chunk.span_m, payload.chunk_span_m
                ));
            }
            if chunk.chunk_x >= payload.chunk_count || chunk.chunk_z >= payload.chunk_count {
                return Err(format!(
                    "static reference: chunk index out of range ({},{})",
                    chunk.chunk_x, chunk.chunk_z
                ));
            }

            min_x = min_x.min(chunk.display_origin_x_m);
            min_z = min_z.min(chunk.display_origin_z_m);
            max_x = max_x.max(chunk.display_origin_x_m + chunk.span_m);
            max_z = max_z.max(chunk.display_origin_z_m + chunk.span_m);

            let gx0 = chunk.chunk_x * step;
            let gz0 = chunk.chunk_z * step;
            for z in 0..payload.chunk_n {
                for x in 0..payload.chunk_n {
                    let gx = gx0 + x;
                    let gz = gz0 + z;
                    let dst = gz * grid_n + gx;
                    let src = z * payload.chunk_n + x;
                    grid[dst] = chunk.height[src] * payload.height_scale_m as f32;
                    filled[dst] = true;
                }
            }
        }

        if let Some(idx) = filled.iter().position(|v| !*v) {
            return Err(format!(
                "static reference: missing grid sample at flat index {idx}"
            ));
        }
        let span_x = max_x - min_x;
        let span_z = max_z - min_z;
        if (span_x - payload.world_span_m).abs() > 0.01
            || (span_z - payload.world_span_m).abs() > 0.01
        {
            return Err(format!(
                "static reference: stitched span {:.3}x{:.3} expected {:.3}",
                span_x, span_z, payload.world_span_m
            ));
        }

        Ok(Self {
            grid,
            grid_n,
            origin_x_m: min_x,
            origin_z_m: min_z,
            span_x_m: span_x,
            span_z_m: span_z,
            feature_span_m: payload.feature_span_m,
        })
    }

    pub(super) fn write_page_texture(
        &self,
        rd: &mut Gd<RenderingDevice>,
        target_rid: Rid,
        page_origin_x: f64,
        page_origin_z: f64,
        world_span: f64,
        page_px: i64,
    ) -> Result<(), String> {
        if page_px < 2 {
            return Err(format!("static reference: page_px {page_px} must be >= 2"));
        }
        let page_px = page_px as usize;
        let mut samples = vec![0.0_f32; page_px * page_px];
        let denom = (page_px - 1) as f64;
        for z in 0..page_px {
            let wz = page_origin_z + world_span * z as f64 / denom;
            for x in 0..page_px {
                let wx = page_origin_x + world_span * x as f64 / denom;
                samples[z * page_px + x] = self.sample(wx, wz);
            }
        }
        let bytes = f32s_to_bytes(&samples);
        let pba = PackedByteArray::from(bytes.as_slice());
        let err = rd.texture_update(target_rid, 0, &pba);
        if err != godot::global::Error::OK {
            return Err(format!("static reference: texture_update failed: {err:?}"));
        }
        Ok(())
    }

    fn sample(&self, x_m: f64, z_m: f64) -> f32 {
        let u = ((x_m - self.origin_x_m) / self.span_x_m).clamp(0.0, 1.0);
        let v = ((z_m - self.origin_z_m) / self.span_z_m).clamp(0.0, 1.0);
        let gx = u * (self.grid_n - 1) as f64;
        let gz = v * (self.grid_n - 1) as f64;
        let x0 = gx.floor() as usize;
        let z0 = gz.floor() as usize;
        let x1 = (x0 + 1).min(self.grid_n - 1);
        let z1 = (z0 + 1).min(self.grid_n - 1);
        let tx = (gx - x0 as f64) as f32;
        let tz = (gz - z0 as f64) as f32;
        let h00 = self.grid[z0 * self.grid_n + x0];
        let h10 = self.grid[z0 * self.grid_n + x1];
        let h01 = self.grid[z1 * self.grid_n + x0];
        let h11 = self.grid[z1 * self.grid_n + x1];
        let hx0 = h00 + (h10 - h00) * tx;
        let hx1 = h01 + (h11 - h01) * tx;
        hx0 + (hx1 - hx0) * tz
    }
}
