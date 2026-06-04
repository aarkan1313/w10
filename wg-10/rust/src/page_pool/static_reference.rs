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
    pub(super) generator_version: String,
    pub(super) source_scope: String,
    pub(super) height_scale_m: f64,
    pub(super) feature_span_m: f64,
    pub(super) has_corridor: bool,
    pub(super) corridor_frac: f64,
    pub(super) pass_network_routes: i64,
    pub(super) pass_network_walkable_frac: f64,
    pub(super) pass_network_carved_frac: f64,
}

#[derive(Deserialize)]
struct StaticPayload {
    generator_version: String,
    source_scope: String,
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
    pass_network: Option<StaticPassNetwork>,
}

#[derive(Deserialize)]
struct StaticPassNetwork {
    routes: i64,
    band_walkable_frac: f64,
    carved_frac: f64,
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
    corridor: Option<Vec<i64>>,
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
        if payload.source_scope.trim().is_empty() {
            return Err("static reference: source_scope must be non-empty".into());
        }
        if payload.generator_version.trim().is_empty() {
            return Err("static reference: generator_version must be non-empty".into());
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
        let any_corridor = seed.chunks.iter().any(|chunk| chunk.corridor.is_some());
        let all_corridor = seed.chunks.iter().all(|chunk| chunk.corridor.is_some());
        if any_corridor && !all_corridor {
            return Err("static reference: corridor must be present on all chunks or none".into());
        }
        let mut corridor_grid = all_corridor.then(|| vec![0_u8; grid_len]);
        let mut corridor_filled = all_corridor.then(|| vec![false; grid_len]);

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
            if let Some(corridor) = chunk.corridor.as_ref() {
                if corridor.len() != expected_heights {
                    return Err(format!(
                        "static reference: chunk ({},{}) corridor={} expected {}",
                        chunk.chunk_x,
                        chunk.chunk_z,
                        corridor.len(),
                        expected_heights
                    ));
                }
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
                    if let (Some(corridor), Some(cg), Some(cf)) = (
                        chunk.corridor.as_ref(),
                        corridor_grid.as_mut(),
                        corridor_filled.as_mut(),
                    ) {
                        let value = if corridor[src] != 0 { 1_u8 } else { 0_u8 };
                        if cf[dst] && cg[dst] != value {
                            return Err(format!(
                                "static reference: corridor seam mismatch at flat index {dst}"
                            ));
                        }
                        cg[dst] = value;
                        cf[dst] = true;
                    }
                }
            }
        }

        if let Some(idx) = filled.iter().position(|v| !*v) {
            return Err(format!(
                "static reference: missing grid sample at flat index {idx}"
            ));
        }
        if let Some(cf) = corridor_filled.as_ref() {
            if let Some(idx) = cf.iter().position(|v| !*v) {
                return Err(format!(
                    "static reference: missing corridor sample at flat index {idx}"
                ));
            }
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
        let corridor_frac = corridor_grid
            .as_ref()
            .map(|corridor| {
                corridor.iter().filter(|v| **v != 0).count() as f64 / corridor.len() as f64
            })
            .unwrap_or(0.0);
        let pass_network = seed.pass_network.as_ref();
        let pass_network_routes = pass_network.map(|p| p.routes).unwrap_or(0);
        let pass_network_walkable_frac = pass_network
            .map(|p| p.band_walkable_frac)
            .unwrap_or(0.0);
        let pass_network_carved_frac = pass_network.map(|p| p.carved_frac).unwrap_or(0.0);

        Ok(Self {
            grid,
            grid_n,
            origin_x_m: min_x,
            origin_z_m: min_z,
            span_x_m: span_x,
            span_z_m: span_z,
            generator_version: payload.generator_version,
            source_scope: payload.source_scope,
            height_scale_m: payload.height_scale_m,
            feature_span_m: payload.feature_span_m,
            has_corridor: corridor_grid.is_some(),
            corridor_frac,
            pass_network_routes,
            pass_network_walkable_frac,
            pass_network_carved_frac,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn one_chunk_payload(corridor: Option<Vec<i64>>) -> StaticPayload {
        StaticPayload {
            generator_version: "mountain_synthesis_v0_9x9_original_scene_scale_review_pass_network".into(),
            source_scope: "coherent_full_field_carved_with_pass_network_sliced_for_review".into(),
            chunk_count: 1,
            chunk_n: 2,
            chunk_span_m: 10.0,
            world_span_m: 10.0,
            height_scale_m: 100.0,
            feature_span_m: 90_000.0,
            seeds: vec![StaticSeed {
                pass_network: Some(StaticPassNetwork {
                    routes: 12,
                    band_walkable_frac: 0.75,
                    carved_frac: 0.25,
                }),
                chunks: vec![StaticChunk {
                    chunk_x: 0,
                    chunk_z: 0,
                    n: 2,
                    span_m: 10.0,
                    display_origin_x_m: -5.0,
                    display_origin_z_m: -5.0,
                    height: vec![0.0, 1.0, 2.0, 3.0],
                    corridor,
                }],
            }],
        }
    }

    #[test]
    fn payload_contract_metadata_and_corridor_are_preserved() {
        let rt = StaticHeightRuntime::from_payload(one_chunk_payload(Some(vec![1, 0, 1, 0])))
            .expect("payload should parse");

        assert_eq!(
            rt.source_scope,
            "coherent_full_field_carved_with_pass_network_sliced_for_review"
        );
        assert_eq!(rt.pass_network_routes, 12);
        assert!((rt.pass_network_walkable_frac - 0.75).abs() < 1.0e-12);
        assert!((rt.pass_network_carved_frac - 0.25).abs() < 1.0e-12);
        assert!(rt.has_corridor);
        assert!((rt.corridor_frac - 0.5).abs() < 1.0e-12);
        assert_eq!(rt.sample(-5.0, -5.0), 0.0);
        assert_eq!(rt.sample(5.0, 5.0), 300.0);
    }

    #[test]
    fn payload_without_source_scope_is_rejected() {
        let mut payload = one_chunk_payload(None);
        payload.source_scope.clear();

        let err = match StaticHeightRuntime::from_payload(payload) {
            Ok(_) => panic!("empty scope should be rejected"),
            Err(err) => err,
        };
        assert!(err.contains("source_scope"));
    }
}
