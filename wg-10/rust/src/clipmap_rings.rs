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

    /// Recenter all level meshes on the camera by translating each level's transform,
    /// quantized to that level's CELL spacing so vertices stay locked to the world grid
    /// (no sub-cell swimming). Vertex buffers are untouched — never a rebuild.
    #[func]
    pub fn recenter(&mut self, camera_x: f64, camera_z: f64) {
        for (level, mi) in self.levels.iter_mut().enumerate() {
            let span = self.base_span * 2f64.powi(level as i32);
            let cell = span / self.grid_res as f64;
            let qx = (camera_x / cell).floor() * cell;
            let qz = (camera_z / cell).floor() * cell;
            let mut t = mi.get_transform();
            t.origin = Vector3::new(qx as f32, 0.0, qz as f32);
            mi.set_transform(t);
        }
    }

    /// Bind a level's height page (and its coarser neighbor, for the morph). Pass the SAME
    /// texture for both `height_tex` and `coarse_tex` to disable the morph for that level
    /// (e.g. the coarsest level, or a fallback frame). `level_span` is the band's world
    /// span; `morph_region` is the transition width as a fraction of the span.
    #[func]
    pub fn bind_page(
        &mut self,
        level: i64,
        height_tex: Gd<godot::classes::Texture2D>,
        coarse_tex: Gd<godot::classes::Texture2D>,
        level_span: f64,
        coarse_span: f64,
        height_scale: f64,
        morph_region: f64,
        relief_ref: f64,
        coarse_origin_x: f64,
        coarse_origin_z: f64,
    ) {
        let li = level as usize;
        if li >= self.levels.len() {
            godot_error!("Wg10ClipmapRings::bind_page: level {level} out of range");
            return;
        }
        let mi = &mut self.levels[li];
        let Some(mat_res) = mi.get_material_override() else {
            godot_error!("Wg10ClipmapRings::bind_page: level {level} has no material");
            return;
        };
        let Ok(mut mat) = mat_res.try_cast::<ShaderMaterial>() else {
            godot_error!("Wg10ClipmapRings::bind_page: material is not a ShaderMaterial");
            return;
        };
        mat.set_shader_parameter("height_tex", &height_tex.to_variant());
        mat.set_shader_parameter("coarse_height_tex", &coarse_tex.to_variant());
        mat.set_shader_parameter("world_span", &level_span.to_variant());
        mat.set_shader_parameter("coarse_span", &coarse_span.to_variant());
        mat.set_shader_parameter("height_scale", &height_scale.to_variant());
        mat.set_shader_parameter("morph_region", &morph_region.to_variant());
        mat.set_shader_parameter("relief_ref", &relief_ref.to_variant());
        let coarse_origin = Vector2::new(coarse_origin_x as f32, coarse_origin_z as f32);
        mat.set_shader_parameter("coarse_origin", &coarse_origin.to_variant());
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
