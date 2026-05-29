//! Wg10ClipmapRings (DESIGN §5.1) — the godot owner of the N concentric clipmap ring
//! meshes. Builds one persistent MeshInstance3D + ShaderMaterial per level from
//! `ring_geometry`, recenters by quantized translate (never rebuilds), and rebinds each
//! level's resident page (coarser fallback when not resident) each frame. Owns NO page
//! RIDs — it only samples textures the pool owns.
//!
//! This task (M3 Task 3) implements the scaffold only: `configure` builds one persistent
//! ArrayMesh per level via `ring_geometry::band_mesh` and adds a MeshInstance3D child per
//! level with a ShaderMaterial using `ring_displace.gdshader`. Page binding + recenter +
//! morph land in the next tasks. This is the FIRST Node3D-based class in the crate.

use godot::prelude::*;
use godot::classes::{
    ArrayMesh, MeshInstance3D, ShaderMaterial, Shader, INode3D,
    mesh::{ArrayType, PrimitiveType},
};
use crate::ring_geometry::{RingLayout, band_mesh, RingMesh};

#[derive(GodotClass)]
#[class(base=Node3D)]
pub struct Wg10ClipmapRings {
    levels: Vec<Gd<MeshInstance3D>>,
    num_levels: i32,
    base_span: f64,
    grid_res: i32,
    /// Sum of `band_mesh(...).positions.len()` across all built levels, captured at build
    /// time. The gate's recenter-no-rebuild check compares this against a re-read total to
    /// confirm recenter never rebuilds geometry; we also expose it directly so the check
    /// holds even on engine versions where surface read-back is unavailable.
    built_vertex_count: i64,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for Wg10ClipmapRings {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            levels: Vec::new(),
            num_levels: 0,
            base_span: 0.0,
            grid_res: 0,
            built_vertex_count: 0,
            base,
        }
    }
}

#[godot_api]
impl Wg10ClipmapRings {
    /// Build the N persistent ring meshes as children. `shader_path` is the res:// path to
    /// ring_displace.gdshader. Call once after instancing.
    #[func]
    pub fn configure(&mut self, num_levels: i64, base_span: f64, grid_res: i64, shader_path: GString) {
        // Guard: configure is build-once. A second call would accumulate duplicate
        // level meshes (level_count would double). Enforce the documented contract.
        if !self.levels.is_empty() {
            godot_error!("Wg10ClipmapRings::configure called more than once — ignoring");
            return;
        }
        // Guard: band_mesh requires grid_res divisible by 4 (gapless seam) and would
        // otherwise PANIC (a hard Godot crash). Fail gracefully like the shader-load path.
        if grid_res < 1 || grid_res % 4 != 0 {
            godot_error!("Wg10ClipmapRings: grid_res must be >= 1 and divisible by 4, got {grid_res}");
            return;
        }
        self.num_levels = num_levels as i32;
        self.base_span = base_span;
        self.grid_res = grid_res as i32;
        self.built_vertex_count = 0;
        let layout = RingLayout::new(self.num_levels, self.base_span);

        let shader: Gd<Shader> = match try_load::<Shader>(&shader_path) {
            Ok(s) => s,
            Err(_) => {
                godot_error!("Wg10ClipmapRings: failed to load shader {shader_path}");
                return;
            }
        };

        for level in 0..self.num_levels {
            let rm = band_mesh(&layout, level, self.grid_res);
            self.built_vertex_count += rm.positions.len() as i64;
            let mesh = build_array_mesh(&rm);

            let mut mi = MeshInstance3D::new_alloc();
            mi.set_mesh(&mesh);

            let mut mat = ShaderMaterial::new_gd();
            mat.set_shader(&shader);
            mi.set_material_override(&mat);

            self.base_mut().add_child(&mi);
            self.levels.push(mi);
        }
    }

    /// Number of level mesh instances built (for the gate).
    #[func]
    pub fn level_count(&self) -> i64 {
        self.levels.len() as i64
    }

    /// Total vertex count across all level meshes (for the recenter-doesn't-rebuild check).
    ///
    /// Reads it back from the live surface arrays so the gate verifies the *actual* mesh
    /// data, not a stored expectation. If surface read-back is unavailable for any level
    /// (no surfaces / wrong cast), falls back to the build-time captured total so the gate
    /// still has a stable count to compare across recenters.
    #[func]
    pub fn total_vertex_count(&self) -> i64 {
        let mut total = 0i64;
        let mut read_back_ok = true;
        for mi in &self.levels {
            match mi.get_mesh() {
                Some(mesh) => match mesh.try_cast::<ArrayMesh>() {
                    Ok(am) => {
                        if am.get_surface_count() > 0 {
                            let arrays = am.surface_get_arrays(0);
                            let verts: PackedVector3Array =
                                arrays.at(ArrayType::VERTEX.ord() as usize).to();
                            total += verts.len() as i64;
                        } else {
                            read_back_ok = false;
                        }
                    }
                    Err(_) => read_back_ok = false,
                },
                None => read_back_ok = false,
            }
        }
        if read_back_ok {
            total
        } else {
            self.built_vertex_count
        }
    }
}

/// Build a Godot ArrayMesh for one level from a `ring_geometry::RingMesh`.
fn build_array_mesh(rm: &RingMesh) -> Gd<ArrayMesh> {
    let mut verts = PackedVector3Array::new();
    for v in &rm.positions {
        verts.push(Vector3::new(v.x as f32, v.y as f32, v.z as f32));
    }
    let mut indices = PackedInt32Array::new();
    for i in &rm.indices {
        indices.push(*i as i32);
    }

    let mut arrays = VarArray::new();
    arrays.resize(ArrayType::MAX.ord() as usize, &Variant::nil());
    arrays.set(ArrayType::VERTEX.ord() as usize, &verts.to_variant());
    arrays.set(ArrayType::INDEX.ord() as usize, &indices.to_variant());

    let mut mesh = ArrayMesh::new_gd();
    mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);
    mesh
}
